//! Changing and removing events in iCalendar data (CalDAV calendars): the whole event,
//! or one occurrence of a repeating one, through an override (`RECURRENCE-ID`) or an
//! exception date (`EXDATE`), as calendar apps do.

use std::str::FromStr;

use chrono::{DateTime, Duration, TimeZone, Utc};
use icalendar::{
    Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime, EventLike,
};

use super::ics::{self, to_utc};

/// What to change. `None` leaves a part as it is; an empty place or notes removes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub title: Option<String>,
    pub when: Option<When>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct When {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
}

/// An event found in an object: whether it repeats, and which occurrence was meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub repeats: bool,
    /// For a repeating event: the occurrence's original start (its `RECURRENCE-ID`).
    pub occurrence: Option<DateTime<Utc>>,
}

/// The event `uid` whose occurrence starts at `start`, if this object has it.
pub fn find(data: &str, uid: &str, start: DateTime<Utc>, local: &impl TimeZone) -> Option<Found> {
    let cal = Calendar::from_str(data).ok()?;
    let events: Vec<&icalendar::Event> = cal
        .components
        .iter()
        .filter_map(CalendarComponent::as_event)
        .filter(|e| e.get_uid() == Some(uid))
        .collect();
    // An occurrence moved or changed on its own.
    for e in &events {
        if let Some(rid) = e.get_recurrence_id().and_then(|r| to_utc(&r, local))
            && e.get_start().and_then(|s| to_utc(&s, local)) == Some(start)
        {
            return Some(Found {
                repeats: true,
                occurrence: Some(rid),
            });
        }
    }
    let master = events.iter().find(|e| e.get_recurrence_id().is_none())?;
    if !repeats(master) {
        return (master.get_start().and_then(|s| to_utc(&s, local)) == Some(start)).then_some(
            Found {
                repeats: false,
                occurrence: None,
            },
        );
    }
    let later = start.checked_add_signed(Duration::seconds(1))?;
    let generated = ics::events_between(data, "", start, later, local).ok()?;
    generated
        .iter()
        .any(|e| e.uid == uid && e.start == start)
        .then_some(Found {
            repeats: true,
            occurrence: Some(start),
        })
}

fn repeats(e: &icalendar::Event) -> bool {
    e.property_value("RRULE").is_some() || e.multi_properties().contains_key("RDATE")
}

/// Removes one occurrence of a repeating event: an exception date on the series, and
/// the occurrence's own override if it had one.
pub fn remove_occurrence(
    data: &str,
    uid: &str,
    occurrence: DateTime<Utc>,
    local: &impl TimeZone,
) -> Result<String, String> {
    let mut cal = parse(data)?;
    cal.components.retain(|c| match c.as_event() {
        Some(e) => {
            !(e.get_uid() == Some(uid)
                && e.get_recurrence_id().and_then(|r| to_utc(&r, local)) == Some(occurrence))
        }
        None => true,
    });
    let master = master_mut(&mut cal, uid)?;
    let first = master.get_start().ok_or("The event has no start.")?;
    master.exdate(same_form(&first, occurrence, local));
    touch(master);
    Ok(cal.to_string())
}

/// Applies `changes` to the whole event, or to one occurrence of a repeating one.
pub fn change(
    data: &str,
    uid: &str,
    found: Found,
    whole: bool,
    changes: &Changes,
    local: &impl TimeZone,
) -> Result<String, String> {
    let mut cal = parse(data)?;
    let occurrence = match found.occurrence {
        Some(o) if found.repeats && !whole => o,
        _ => {
            if found.repeats && changes.when.is_some() {
                return Err(
                    "Every occurrence of a repeating event can't be moved at once. Move one occurrence at a time, or change the series in the calendar app."
                        .to_owned(),
                );
            }
            let master = master_mut(&mut cal, uid)?;
            apply(master, changes, local);
            return Ok(cal.to_string());
        }
    };
    let existing = cal.components.iter_mut().find_map(|c| match c {
        CalendarComponent::Event(e)
            if e.get_uid() == Some(uid)
                && e.get_recurrence_id().and_then(|r| to_utc(&r, local)) == Some(occurrence) =>
        {
            Some(e)
        }
        _ => None,
    });
    if let Some(e) = existing {
        apply(e, changes, local);
        return Ok(cal.to_string());
    }
    // A new override: a copy of the series for this occurrence only.
    let master = master_mut(&mut cal, uid)?;
    let first = master.get_start().ok_or("The event has no start.")?;
    let first_utc = to_utc(&first, local).ok_or("The event's start can't be read.")?;
    let length = master
        .get_end()
        .and_then(|e| to_utc(&e, local))
        .map(|e| e - first_utc)
        .filter(|d| *d >= Duration::zero())
        .unwrap_or(if matches!(first, DatePerhapsTime::Date(_)) {
            Duration::days(1)
        } else {
            Duration::zero()
        });
    let mut own = master.clone();
    for key in ["RRULE", "RDATE", "EXDATE", "EXRULE", "DURATION", "DTEND"] {
        own.remove_property(key);
        own.remove_multi_property(key);
    }
    own.recurrence_id(same_form(&first, occurrence, local));
    own.starts(same_form(&first, occurrence, local));
    let end = occurrence
        .checked_add_signed(length)
        .ok_or("That time is out of range.")?;
    own.ends(same_form(&first, end, local));
    own.remove_sequence();
    apply(&mut own, changes, local);
    cal.components.push(CalendarComponent::Event(own));
    Ok(cal.to_string())
}

fn parse(data: &str) -> Result<Calendar, String> {
    Calendar::from_str(data)
        .map_err(|_| "The calendar sent an event it can't read back.".to_owned())
}

fn master_mut<'a>(cal: &'a mut Calendar, uid: &str) -> Result<&'a mut icalendar::Event, String> {
    cal.components
        .iter_mut()
        .find_map(|c| match c {
            CalendarComponent::Event(e)
                if e.get_uid() == Some(uid) && e.get_recurrence_id().is_none() =>
            {
                Some(e)
            }
            _ => None,
        })
        .ok_or_else(|| "The event isn't in the calendar anymore.".to_owned())
}

fn apply(e: &mut icalendar::Event, changes: &Changes, local: &impl TimeZone) {
    if let Some(title) = &changes.title {
        e.summary(title);
    }
    if let Some(when) = changes.when {
        for key in ["DTSTART", "DTEND", "DURATION"] {
            e.remove_property(key);
        }
        if when.all_day {
            e.starts(when.start.with_timezone(local).date_naive());
            e.ends(when.end.with_timezone(local).date_naive());
        } else {
            e.starts(when.start).ends(when.end);
        }
    }
    match changes.location.as_deref().map(str::trim) {
        Some("") => {
            e.remove_location();
        }
        Some(l) => {
            e.location(l);
        }
        None => {}
    }
    match changes.notes.as_deref().map(str::trim) {
        Some("") => {
            e.remove_description();
        }
        Some(n) => {
            e.description(n);
        }
        None => {}
    }
    touch(e);
}

/// Marks the event as changed now, so other apps pick up the new version.
fn touch(e: &mut icalendar::Event) {
    let sequence = e.get_sequence().unwrap_or(0) + 1;
    e.sequence(sequence);
    e.timestamp(Utc::now());
    e.last_modified(Utc::now());
}

/// `at` written the way the series writes its start (a date, UTC, a named zone or
/// floating), as `RECURRENCE-ID` and `EXDATE` must be.
fn same_form(first: &DatePerhapsTime, at: DateTime<Utc>, local: &impl TimeZone) -> DatePerhapsTime {
    match first {
        DatePerhapsTime::Date(_) => DatePerhapsTime::Date(at.with_timezone(local).date_naive()),
        DatePerhapsTime::DateTime(CalendarDateTime::Utc(_)) => at.into(),
        DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone { tzid, .. }) => {
            let date_time = match tzid.parse::<chrono_tz::Tz>() {
                Ok(tz) => at.with_timezone(&tz).naive_local(),
                // The reading side takes unknown names as local time; so does this.
                Err(_) => at.with_timezone(local).naive_local(),
            };
            CalendarDateTime::WithTimezone {
                date_time,
                tzid: tzid.clone(),
            }
            .into()
        }
        DatePerhapsTime::DateTime(CalendarDateTime::Floating(_)) => {
            CalendarDateTime::Floating(at.with_timezone(local).naive_local()).into()
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono_tz::Europe::Zurich;

    use super::*;

    const SERIES: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:t\r\nBEGIN:VEVENT\r\n\
UID:standup@x\r\nDTSTART;TZID=Europe/Zurich:20261005T090000\r\n\
DTEND;TZID=Europe/Zurich:20261005T091500\r\nRRULE:FREQ=WEEKLY\r\nSUMMARY:Standup\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

    const SINGLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:t\r\nBEGIN:VEVENT\r\n\
UID:dentist@x\r\nDTSTART:20261002T080000Z\r\nDTEND:20261002T084500Z\r\nSUMMARY:Dentist\r\n\
LOCATION:Rue du Lac 4\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
        Zurich
            .with_ymd_and_hms(2026, 10, d, h, m, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn occurrences(data: &str, uid: &str) -> Vec<(DateTime<Utc>, String)> {
        ics::events_between(data, "", at(1, 0, 0), at(31, 0, 0), &Zurich)
            .unwrap()
            .into_iter()
            .filter(|e| e.uid == uid)
            .map(|e| (e.start, e.title))
            .collect()
    }

    #[test]
    fn finds_single_events_occurrences_and_nothing_else() {
        assert_eq!(
            find(SINGLE, "dentist@x", at(2, 10, 0), &Zurich),
            Some(Found {
                repeats: false,
                occurrence: None
            })
        );
        assert_eq!(find(SINGLE, "dentist@x", at(2, 11, 0), &Zurich), None);
        assert_eq!(find(SINGLE, "other@x", at(2, 10, 0), &Zurich), None);
        assert_eq!(
            find(SERIES, "standup@x", at(12, 9, 0), &Zurich),
            Some(Found {
                repeats: true,
                occurrence: Some(at(12, 9, 0))
            })
        );
        assert_eq!(find(SERIES, "standup@x", at(13, 9, 0), &Zurich), None);
    }

    #[test]
    fn one_occurrence_is_removed_with_an_exception_in_the_series_zone() {
        let data = remove_occurrence(SERIES, "standup@x", at(12, 9, 0), &Zurich).unwrap();
        assert!(
            data.contains("EXDATE;TZID=Europe/Zurich:20261012T090000"),
            "{data}"
        );
        let left: Vec<_> = occurrences(&data, "standup@x")
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(
            left,
            [at(5, 9, 0), at(19, 9, 0), at(26, 9, 0)],
            "the 12th is gone, the others stay"
        );
    }

    #[test]
    fn one_occurrence_is_moved_with_an_override_and_the_rest_stay() {
        let changes = Changes {
            title: Some("Standup (late)".into()),
            when: Some(When {
                start: at(12, 10, 0),
                end: at(12, 10, 30),
                all_day: false,
            }),
            ..Default::default()
        };
        let found = find(SERIES, "standup@x", at(12, 9, 0), &Zurich).unwrap();
        let data = change(SERIES, "standup@x", found, false, &changes, &Zurich).unwrap();
        assert!(
            data.contains("RECURRENCE-ID;TZID=Europe/Zurich:20261012T090000"),
            "{data}"
        );
        assert_eq!(
            occurrences(&data, "standup@x"),
            [
                (at(5, 9, 0), "Standup".to_owned()),
                (at(12, 10, 0), "Standup (late)".to_owned()),
                (at(19, 9, 0), "Standup".to_owned()),
                (at(26, 9, 0), "Standup".to_owned()),
            ]
        );
        // The moved one is found again at its new time, and can be changed or removed.
        let moved = find(&data, "standup@x", at(12, 10, 0), &Zurich).unwrap();
        assert_eq!(moved.occurrence, Some(at(12, 9, 0)));
        let data = remove_occurrence(&data, "standup@x", at(12, 9, 0), &Zurich).unwrap();
        assert_eq!(occurrences(&data, "standup@x").len(), 3);
    }

    #[test]
    fn whole_events_change_but_a_series_is_not_moved_at_once() {
        let found = find(SINGLE, "dentist@x", at(2, 10, 0), &Zurich).unwrap();
        let data = change(
            SINGLE,
            "dentist@x",
            found,
            true,
            &Changes {
                when: Some(When {
                    start: at(3, 11, 0),
                    end: at(3, 11, 45),
                    all_day: false,
                }),
                location: Some(String::new()),
                ..Default::default()
            },
            &Zurich,
        )
        .unwrap();
        let events = ics::events_between(&data, "", at(1, 0, 0), at(31, 0, 0), &Zurich).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            (events[0].start, events[0].end),
            (at(3, 11, 0), at(3, 11, 45))
        );
        assert_eq!(events[0].location, None);
        assert_eq!(events[0].title, "Dentist");
        assert!(data.contains("SEQUENCE:1"));

        let series = find(SERIES, "standup@x", at(12, 9, 0), &Zurich).unwrap();
        let renamed = change(
            SERIES,
            "standup@x",
            series,
            true,
            &Changes {
                title: Some("Team standup".into()),
                ..Default::default()
            },
            &Zurich,
        )
        .unwrap();
        assert!(
            occurrences(&renamed, "standup@x")
                .iter()
                .all(|(_, t)| t == "Team standup")
        );
        let moved = change(
            SERIES,
            "standup@x",
            series,
            true,
            &Changes {
                when: Some(When {
                    start: at(12, 10, 0),
                    end: at(12, 10, 30),
                    all_day: false,
                }),
                ..Default::default()
            },
            &Zurich,
        );
        assert!(moved.is_err());
    }
}
