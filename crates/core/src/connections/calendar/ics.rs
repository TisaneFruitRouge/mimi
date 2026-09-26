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
}

/// Hard cap on occurrences per recurring event, so a daily rule over a long range
/// can't blow up.
const MAX_OCCURRENCES: u16 = 400;

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
        let duration = event
            .get_end()
            .and_then(|end| to_utc(&end, local))
            .map(|end| end - start)
            .or_else(|| event.property_value("DURATION").and_then(parse_duration))
            .filter(|d| *d >= Duration::zero())
            .unwrap_or(if all_day {
                Duration::days(1)
            } else {
                Duration::zero()
            });
        let uid = event.get_uid().unwrap_or_default().to_owned();

        let make = |start: DateTime<Utc>| CalEvent {
            uid: uid.clone(),
            title: event
                .get_summary()
                .unwrap_or("(no title)")
                .trim()
                .to_owned(),
            start,
            end: start + duration,
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
        };
        let overlaps = |s: DateTime<Utc>| {
            s < to && s + duration > from || (duration.is_zero() && s >= from && s < to)
        };

        let recurring =
            event.property_value("RRULE").is_some() && event.get_recurrence_id().is_none();
        if !recurring {
            if overlaps(start) {
                out.push(make(start));
            }
            continue;
        }
        let Ok(set) = event.get_recurrence() else {
            // An unreadable rule: still show the first occurrence rather than nothing.
            if overlaps(start) {
                out.push(make(start));
            }
            continue;
        };
        let window_start = (from - duration).with_timezone(&icalendar::rrule::Tz::UTC);
        let window_end = to.with_timezone(&icalendar::rrule::Tz::UTC);
        for occurrence in set
            .after(window_start)
            .before(window_end)
            .all(MAX_OCCURRENCES)
            .dates
        {
            let occurrence = occurrence.with_timezone(&Utc);
            if overridden.contains(&(uid.clone(), occurrence)) {
                continue;
            }
            if overlaps(occurrence) {
                out.push(make(occurrence));
            }
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
    Ok(out)
}

fn to_utc(value: &DatePerhapsTime, local: &impl TimeZone) -> Option<DateTime<Utc>> {
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

/// Parses an RFC 5545 duration like `PT1H30M`, `P1D` or `P2W`.
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
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let n: i64 = rest[..digits].parse().ok()?;
        let unit = rest[digits..].chars().next()?;
        total += match (unit, in_time) {
            ('W', false) => Duration::weeks(n),
            ('D', false) => Duration::days(n),
            ('H', true) => Duration::hours(n),
            ('M', true) => Duration::minutes(n),
            ('S', true) => Duration::seconds(n),
            _ => return None,
        };
        rest = &rest[digits + 1..];
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
    }
}
