//! Reading events out of iCalendar data, expanding recurring events into the
//! occurrences that fall in a time range.

use std::collections::HashSet;
use std::str::FromStr;

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use icalendar::{
    Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime, EventLike,
};

/// One occurrence of an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalEvent {
    pub uid: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub location: Option<String>,
    pub notes: Option<String>,
    /// The calendar it came from, as the user named it.
    pub calendar: String,
    /// Which calendar, as a stable id (see `calendar::calendar_id`). Empty until the
    /// caller that knows the account fills it in.
    pub calendar_id: String,
    /// Invited people (ATTENDEE), in the order the calendar lists them.
    pub attendees: Vec<Attendee>,
    /// Who created the event (ORGANIZER), if the calendar says.
    pub organizer: Option<Attendee>,
}

/// A person on an event, as the calendar describes them. Written by whoever sent the
/// invitation: untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: String,
}

/// `mailto:sam@example.com` with its `CN` (and `EMAIL`) parameters → an attendee.
fn attendee(p: &icalendar::Property) -> Option<Attendee> {
    let param = |k: &str| {
        p.params()
            .get(k)
            .map(|v| v.value().trim().trim_matches('"').to_owned())
            .filter(|v| !v.is_empty())
    };
    let value = p.value().trim();
    let email = value
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
        .map(|_| value[7..].to_owned())
        .or_else(|| param("EMAIL"))
        .filter(|e| e.contains('@'))?
        .to_lowercase();
    let name = param("CN").filter(|n| !n.eq_ignore_ascii_case(&email));
    Some(Attendee { name, email })
}

/// Hard cap on occurrences per recurring event, so a daily rule over a long range
/// can't blow up.
const MAX_OCCURRENCES: u16 = 400;

/// Hard cap on the occurrences walked through (from the first) to reach a range: a daily
/// rule for 270 years, or an hourly one for 11. Past that the event is left out.
const MAX_STEPS: usize = 100_000;

/// The calendar's own name (`X-WR-CALNAME`), if it has one.
pub fn calendar_name(ics: &str) -> Option<String> {
    Calendar::from_str(ics)
        .ok()?
        .property_value("X-WR-CALNAME")
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
}

/// Whether text parses as iCalendar data at all.
pub fn looks_like_calendar(ics: &str) -> bool {
    ics.trim_start().starts_with("BEGIN:VCALENDAR") && Calendar::from_str(ics).is_ok()
}

/// All event occurrences overlapping `[from, to)`, sorted by start.
pub fn events_between(
    ics: &str,
    calendar: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    local: &impl TimeZone,
) -> Result<Vec<CalEvent>, String> {
    let parsed = Calendar::from_str(ics).map_err(|e| format!("couldn't read the calendar: {e}"))?;
    let events: Vec<&icalendar::Event> = parsed
        .components
        .iter()
        .filter_map(CalendarComponent::as_event)
        .collect();

    // Occurrences that were moved or changed individually replace the generated ones.
    let overridden: HashSet<(String, DateTime<Utc>)> = events
        .iter()
        .filter_map(|e| {
            let rid = to_utc(&e.get_recurrence_id()?, local)?;
            Some((e.get_uid()?.to_owned(), rid))
        })
        .collect();

    let mut out = Vec::new();
    for event in &events {
        if event
            .property_value("STATUS")
            .is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED"))
        {
            continue;
        }
        let Some(start_prop) = event.get_start() else {
            continue;
        };
        let all_day = matches!(start_prop, DatePerhapsTime::Date(_));
        let Some(start) = to_utc(&start_prop, local) else {
            continue;
        };
        // A missing, negative or absurd length falls back to the usual one.
        let duration = event
            .get_end()
            .and_then(|end| to_utc(&end, local))
            .map(|end| end - start)
            .or_else(|| event.property_value("DURATION").and_then(parse_duration))
            .filter(|d| *d >= Duration::zero() && *d <= max_duration())
            .unwrap_or(if all_day {
                Duration::days(1)
            } else {
                Duration::zero()
            });
        let uid = event.get_uid().unwrap_or_default().to_owned();
        let organizer = event.properties().get("ORGANIZER").and_then(attendee);
        let attendees: Vec<Attendee> = event
            .multi_properties()
            .get("ATTENDEE")
            .into_iter()
            .flatten()
            .filter_map(attendee)
            .collect();

        // Calendar data comes from other people: an event at the very end of time must
        // not overflow. Such an event is left out.
        let end_of = |s: DateTime<Utc>| s.checked_add_signed(duration);
        let make = |start: DateTime<Utc>, end: DateTime<Utc>| CalEvent {
            uid: uid.clone(),
            title: event
                .get_summary()
                .unwrap_or("(no title)")
                .trim()
                .to_owned(),
            start,
            end,
            all_day,
            location: event
                .get_location()
                .map(str::to_owned)
                .filter(|s| !s.is_empty()),
            notes: event
                .get_description()
                .map(str::to_owned)
                .filter(|s| !s.is_empty()),
            calendar: calendar.to_owned(),
            calendar_id: String::new(),
            attendees: attendees.clone(),
            organizer: organizer.clone(),
        };
        // The occurrence starting at `s`, if it overlaps the range.
        let occurrence_in_range = |s: DateTime<Utc>| {
            let e = end_of(s)?;
            let overlaps = s < to && e > from || (duration.is_zero() && s >= from && s < to);
            overlaps.then(|| make(s, e))
        };

        let recurring =
            event.property_value("RRULE").is_some() && event.get_recurrence_id().is_none();
        if !recurring {
            out.extend(occurrence_in_range(start));
            continue;
        }
        // A rule repeating every second or minute is taken as unreadable: no real calendar
        // has one, and walking it from an early start up to today would take hours.
        let every_minute = |r: &icalendar::rrule::RRule| {
            use icalendar::rrule::Frequency;
            matches!(r.get_freq(), Frequency::Secondly | Frequency::Minutely)
        };
        let Some(set) = event
            .get_recurrence()
            .ok()
            .filter(|s| !s.get_rrule().iter().any(every_minute))
        else {
            // An unreadable rule: still show the first occurrence rather than nothing.
            out.extend(occurrence_in_range(start));
            continue;
        };
        let window_start = from.checked_sub_signed(duration).unwrap_or(from);
        let mut shown = 0;
        // Occurrences come in order from the first. Walking is capped too, so a rule
        // starting centuries ago gives up rather than stalling.
        for occurrence in set.limit().into_iter().take(MAX_STEPS) {
            let occurrence = occurrence.with_timezone(&Utc);
            if occurrence > to || shown >= MAX_OCCURRENCES {
                break;
            }
            if occurrence < window_start || overridden.contains(&(uid.clone(), occurrence)) {
                continue;
            }
            if let Some(e) = occurrence_in_range(occurrence) {
                out.push(e);
                shown += 1;
            }
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
    Ok(out)
}

pub(crate) fn to_utc(value: &DatePerhapsTime, local: &impl TimeZone) -> Option<DateTime<Utc>> {
    match value {
        DatePerhapsTime::Date(date) => local_midnight(*date, local),
        DatePerhapsTime::DateTime(CalendarDateTime::Utc(dt)) => Some(*dt),
        DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone { date_time, tzid }) => {
            match tzid.parse::<chrono_tz::Tz>() {
                Ok(tz) => tz
                    .from_local_datetime(date_time)
                    .earliest()
                    .map(|d| d.with_timezone(&Utc)),
                // Non-IANA names (Outlook's "W. Europe Standard Time"): best effort.
                Err(_) => local
                    .from_local_datetime(date_time)
                    .earliest()
                    .map(|d| d.with_timezone(&Utc)),
            }
        }
        DatePerhapsTime::DateTime(CalendarDateTime::Floating(naive)) => local
            .from_local_datetime(naive)
            .earliest()
            .map(|d| d.with_timezone(&Utc)),
    }
}

fn local_midnight(date: NaiveDate, local: &impl TimeZone) -> Option<DateTime<Utc>> {
    local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
}

/// The longest an event may last; longer ones are taken as mistakes (or attacks).
fn max_duration() -> Duration {
    Duration::days(10 * 366)
}

/// Parses an RFC 5545 duration like `PT1H30M`, `P1D` or `P2W`. `None` for anything
/// unreadable or longer than `max_duration`.
fn parse_duration(raw: &str) -> Option<Duration> {
    let raw = raw.trim();
    let (negative, raw) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw.strip_prefix('+').unwrap_or(raw)),
    };
    let mut rest = raw.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut in_time = false;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('T') {
            in_time = true;
            rest = r;
            continue;
        }
        // Digits are ASCII, so the count is also a byte offset.
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let n: i64 = rest.get(..digits)?.parse().ok()?;
        let unit = rest.get(digits..)?.chars().next()?;
        let part = match (unit, in_time) {
            ('W', false) => Duration::try_weeks(n),
            ('D', false) => Duration::try_days(n),
            ('H', true) => Duration::try_hours(n),
            ('M', true) => Duration::try_minutes(n),
            ('S', true) => Duration::try_seconds(n),
            _ => return None,
        }?;
        total = total.checked_add(&part).filter(|t| *t <= max_duration())?;
        rest = rest.get(digits + unit.len_utf8()..)?;
    }
    Some(if negative { -total } else { total })
}

#[cfg(test)]
mod tests {
    use chrono_tz::Europe::Zurich;

    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//Google Inc//Google Calendar 70.9054//EN\r
X-WR-CALNAME:Personal\r
X-WR-TIMEZONE:Europe/Zurich\r
BEGIN:VEVENT\r
UID:dentist@google.com\r
DTSTART;TZID=Europe/Zurich:20261001T130000\r
DTEND;TZID=Europe/Zurich:20261001T134500\r
SUMMARY:Dentist\r
LOCATION:Rue du Lac 4\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:standup@google.com\r
DTSTART;TZID=Europe/Zurich:20260928T090000\r
DTEND;TZID=Europe/Zurich:20260928T091500\r
RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r
EXDATE;TZID=Europe/Zurich:20260930T090000\r
SUMMARY:Standup\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:standup@google.com\r
RECURRENCE-ID;TZID=Europe/Zurich:20261002T090000\r
DTSTART;TZID=Europe/Zurich:20261002T100000\r
DTEND;TZID=Europe/Zurich:20261002T101500\r
SUMMARY:Standup (moved)\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:holiday@google.com\r
DTSTART;VALUE=DATE:20261003\r
DTEND;VALUE=DATE:20261004\r
SUMMARY:Day off\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:cancelled@google.com\r
DTSTART:20261001T080000Z\r
DTEND:20261001T090000Z\r
STATUS:CANCELLED\r
SUMMARY:Cancelled thing\r
END:VEVENT\r
END:VCALENDAR\r
";

    fn zurich(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Zurich
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn expands_recurrences_with_exceptions_and_overrides() {
        let events = events_between(
            ICS,
            "Personal",
            zurich(2026, 9, 28, 0, 0),
            zurich(2026, 10, 5, 0, 0),
            &Zurich,
        )
        .unwrap();
        let summary: Vec<(String, DateTime<Utc>)> =
            events.iter().map(|e| (e.title.clone(), e.start)).collect();
        assert_eq!(
            summary,
            vec![
                ("Standup".to_owned(), zurich(2026, 9, 28, 9, 0)),
                // Wednesday the 30th is excluded by EXDATE.
                ("Dentist".to_owned(), zurich(2026, 10, 1, 13, 0)),
                // Friday's occurrence was moved to 10:00.
                ("Standup (moved)".to_owned(), zurich(2026, 10, 2, 10, 0)),
                ("Day off".to_owned(), zurich(2026, 10, 3, 0, 0)),
            ]
        );
        let dentist = &events[1];
        assert_eq!(dentist.end - dentist.start, Duration::minutes(45));
        assert_eq!(dentist.location.as_deref(), Some("Rue du Lac 4"));
        assert!(events[3].all_day);
        assert_eq!(calendar_name(ICS).as_deref(), Some("Personal"));
    }

    #[test]
    fn range_is_respected() {
        let events = events_between(
            ICS,
            "Personal",
            zurich(2026, 10, 1, 0, 0),
            zurich(2026, 10, 2, 0, 0),
            &Zurich,
        )
        .unwrap();
        let titles: Vec<&str> = events.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Dentist"]);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_duration("P1D"), Some(Duration::days(1)));
        assert_eq!(parse_duration("P2W"), Some(Duration::weeks(2)));
        assert_eq!(parse_duration("-PT15M"), Some(Duration::minutes(-15)));
        assert_eq!(parse_duration("nonsense"), None);
        // Absurd lengths are unreadable, not a crash.
        assert_eq!(parse_duration("P999999999D"), None);
        assert_eq!(parse_duration("P99999999999999999W"), None);
        assert_eq!(parse_duration("PT9223372036854775807S"), None);
        assert_eq!(
            parse_duration("P3650DT1H"),
            Some(Duration::hours(3650 * 24 + 1))
        );
        assert_eq!(parse_duration("P1é"), None);
        assert_eq!(parse_duration("PT"), Some(Duration::zero()));
    }

    /// A rule repeating very often from long ago can't stall reading the calendar.
    #[test]
    fn endless_rules_give_up_quickly() {
        let rules = [
            ("20000101T000000Z", "FREQ=SECONDLY", 0),
            ("20000101T000000Z", "FREQ=MINUTELY", 0),
            ("20000101T000000Z", "FREQ=HOURLY", 0),
            (
                "19000101T000000Z",
                "FREQ=YEARLY;BYHOUR=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;BYMINUTE=0,10,20,30,40,50",
                0,
            ),
            ("20000101T000000Z", "FREQ=DAILY", 1),
            ("20000101T000000Z", "FREQ=YEARLY;BYMONTH=10;BYMONTHDAY=1", 1),
        ];
        for (start, rule, expected) in rules {
            let ics = format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:r@example.com\r\n\
                 DTSTART:{start}\r\nRRULE:{rule}\r\nSUMMARY:Again\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            );
            let started = std::time::Instant::now();
            let events = events_between(
                &ics,
                "Personal",
                zurich(2026, 10, 1, 0, 0),
                zurich(2026, 10, 2, 0, 0),
                &Zurich,
            )
            .unwrap();
            assert_eq!(events.len(), expected, "{rule}");
            assert!(
                started.elapsed() < std::time::Duration::from_secs(2),
                "{rule} took {:?}",
                started.elapsed()
            );
        }
    }

    /// Calendars are written by other people: hostile values mustn't crash the daemon.
    #[test]
    fn hostile_calendar_data_is_skipped_not_fatal() {
        let ics = "BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VEVENT\r
UID:long@example.com\r
DTSTART:20261001T080000Z\r
DURATION:P999999999D\r
SUMMARY:Forever\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:end@example.com\r
DTSTART:+2621421231T000000Z\r
DURATION:P3000D\r
SUMMARY:End of time\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:far@example.com\r
DTSTART:20261001T090000Z\r
DTEND:99991231T000000Z\r
SUMMARY:Very long\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:rule@example.com\r
DTSTART:99991231T230000Z\r
DURATION:P3650D\r
RRULE:FREQ=SECONDLY;INTERVAL=2147483647\r
SUMMARY:Last second\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:daily@example.com\r
DTSTART:20261001T100000Z\r
DURATION:P999999999W\r
RRULE:FREQ=DAILY\r
SUMMARY:Daily\r
END:VEVENT\r
END:VCALENDAR\r
";
        let events = events_between(
            ics,
            "Personal",
            zurich(2026, 10, 1, 0, 0),
            zurich(2026, 10, 2, 0, 0),
            &Zurich,
        )
        .unwrap();
        let summary: Vec<(&str, Duration)> = events
            .iter()
            .map(|e| (e.title.as_str(), e.end - e.start))
            .collect();
        // The absurd lengths fall back to zero; the rest are left out or kept as they are.
        assert_eq!(
            summary,
            [
                ("Forever", Duration::zero()),
                ("Very long", Duration::zero()),
                ("Daily", Duration::zero()),
            ]
        );
        // Far in the future, nothing overflows either.
        events_between(
            ics,
            "Personal",
            Utc.with_ymd_and_hms(9999, 12, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(9999, 12, 31, 23, 59, 0).unwrap(),
            &Zurich,
        )
        .unwrap();
    }
}
