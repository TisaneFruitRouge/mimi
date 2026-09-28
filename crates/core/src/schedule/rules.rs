//! When things happen: the next occurrence of a schedule, what to do about occurrences
//! that were missed, and schedules in words. Pure functions over an explicit clock and
//! time zone, so they're testable across daylight-saving changes.

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use mimi_protocol::{Schedule, Weekday};

/// Later than this, a delivery is marked late.
pub const LATE_AFTER_MS: i64 = 2 * 60 * 1000;
/// Later than this, it's not worth sending anymore: recorded as missed instead.
pub const MISSED_AFTER_MS: i64 = 12 * 60 * 60 * 1000;

/// How to handle an occurrence that came due at `due`, noticed at `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fire {
    OnTime,
    Late,
    Missed,
}

pub fn classify(due_ms: i64, now_ms: i64) -> Fire {
    match now_ms - due_ms {
        d if d > MISSED_AFTER_MS => Fire::Missed,
        d if d > LATE_AFTER_MS => Fire::Late,
        _ => Fire::OnTime,
    }
}

pub fn parse_time(s: &str) -> Result<Time, String> {
    let s = s.trim();
    let (h, m) = s
        .split_once(':')
        .ok_or_else(|| format!("“{s}” isn't a time like 07:30"))?;
    let (h, m): (i8, i8) = (
        h.trim()
            .parse()
            .map_err(|_| format!("“{s}” isn't a time like 07:30"))?,
        m.trim()
            .get(..2)
            .unwrap_or(m.trim())
            .parse()
            .map_err(|_| format!("“{s}” isn't a time like 07:30"))?,
    );
    Time::new(h, m, 0, 0).map_err(|_| format!("“{s}” isn't a time of day"))
}

pub fn parse_local(s: &str) -> Result<DateTime, String> {
    let s = s.trim().replacen(' ', "T", 1);
    let (date, time) = s
        .split_once('T')
        .ok_or_else(|| format!("“{s}” isn't a date and time like 2026-10-01T09:00"))?;
    let date: Date = date
        .parse()
        .map_err(|_| format!("“{date}” isn't a date like 2026-10-01"))?;
    Ok(date.to_datetime(parse_time(time)?))
}

fn weekday(w: Weekday) -> jiff::civil::Weekday {
    use jiff::civil::Weekday as J;
    match w {
        Weekday::Mon => J::Monday,
        Weekday::Tue => J::Tuesday,
        Weekday::Wed => J::Wednesday,
        Weekday::Thu => J::Thursday,
        Weekday::Fri => J::Friday,
        Weekday::Sat => J::Saturday,
        Weekday::Sun => J::Sunday,
    }
}

/// A local wall-clock time as an instant. Times skipped by a daylight-saving jump move
/// forward by the gap; repeated times use the first occurrence.
fn instant(tz: &TimeZone, dt: DateTime) -> Option<Timestamp> {
    tz.to_ambiguous_zoned(dt)
        .compatible()
        .ok()
        .map(|z| z.timestamp())
}

/// Checks a schedule's own values, before it's saved.
pub fn validate(s: &Schedule) -> Result<(), String> {
    match s {
        Schedule::Once { at } => parse_local(at).map(drop),
        Schedule::Daily { time } | Schedule::Weekdays { time } => parse_time(time).map(drop),
        Schedule::Weekly { days, time } => {
            if days.is_empty() {
                return Err("Pick at least one day of the week.".to_owned());
            }
            parse_time(time).map(drop)
        }
        Schedule::Monthly { day, time } => {
            if !(1..=31).contains(day) {
                return Err("The day of the month must be between 1 and 31.".to_owned());
            }
            parse_time(time).map(drop)
        }
        Schedule::Yearly { month, day, time } => {
            Date::new(2024, *month as i8, *day as i8)
                .map_err(|_| "That isn't a date in the year.".to_owned())?;
            parse_time(time).map(drop)
        }
        Schedule::Interval { minutes } => {
            if *minutes == 0 {
                return Err("The interval must be at least a minute.".to_owned());
            }
            Ok(())
        }
        Schedule::BeforeEvent { event_id, .. } => {
            if event_id.is_empty() {
                return Err("Which event?".to_owned());
            }
            Ok(())
        }
    }
}

/// The first occurrence strictly after `after`, or `None` if there are no more.
/// `anchor` is where interval schedules start counting; `event_start` is the start of
/// the event an event-relative schedule follows, if known.
pub fn next_after(
    schedule: &Schedule,
    after: Timestamp,
    anchor: Timestamp,
    tz: &TimeZone,
    event_start: Option<Timestamp>,
) -> Option<Timestamp> {
    let on_matching_day = |time: &str, matches: &dyn Fn(Date) -> bool| -> Option<Timestamp> {
        let time = parse_time(time).ok()?;
        let mut date = after.to_zoned(tz.clone()).date();
        // A yearly date recurs within 366 days (29 February falls back to the 28th).
        for _ in 0..=370 {
            if matches(date)
                && let Some(t) = instant(tz, date.to_datetime(time))
                && t > after
            {
                return Some(t);
            }
            date = date.tomorrow().ok()?;
        }
        None
    };
    match schedule {
        Schedule::Once { at } => {
            let t = instant(tz, parse_local(at).ok()?)?;
            (t > after).then_some(t)
        }
        Schedule::Daily { time } => on_matching_day(time, &|_| true),
        Schedule::Weekdays { time } => on_matching_day(time, &|d| {
            !matches!(
                d.weekday(),
                jiff::civil::Weekday::Saturday | jiff::civil::Weekday::Sunday
            )
        }),
        Schedule::Weekly { days, time } => {
            let days: Vec<_> = days.iter().copied().map(weekday).collect();
            on_matching_day(time, &|d| days.contains(&d.weekday()))
        }
        Schedule::Monthly { day, time } => {
            on_matching_day(time, &|d| d.day() == (*day as i8).min(d.days_in_month()))
        }
        Schedule::Yearly { month, day, time } => on_matching_day(time, &|d| {
            let target = (*day as i8).min(
                Date::new(d.year(), *month as i8, 1)
                    .map(|m| m.days_in_month())
                    .unwrap_or(28),
            );
            d.month() == *month as i8 && d.day() == target
        }),
        Schedule::Interval { minutes } => {
            let step = *minutes as i64 * 60_000;
            if step <= 0 {
                return None;
            }
            let (a, now) = (anchor.as_millisecond(), after.as_millisecond());
            let n = if now < a { 0 } else { (now - a) / step + 1 };
            Timestamp::from_millisecond(a + n * step).ok()
        }
        Schedule::BeforeEvent { minutes_before, .. } => {
            let t = event_start?
                .checked_sub((*minutes_before as i64).minutes())
                .ok()?;
            (t > after).then_some(t)
        }
    }
}

/// Schedules that go off more often than this are listed once per day in a calendar
/// (with how many times), instead of dozens of separate entries.
pub const FOLD_UNDER_MINUTES: u32 = 120;

/// One time a schedule goes off, or several in the same local day folded into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub first: Timestamp,
    pub last: Timestamp,
    pub count: u32,
}

/// Whether `schedule` is listed as one [`Span`] per day by [`occurrences`].
pub fn folds(schedule: &Schedule) -> bool {
    matches!(schedule, Schedule::Interval { minutes } if *minutes < FOLD_UNDER_MINUTES)
}

/// The local calendar day an instant falls on.
pub fn local_date(t: Timestamp, tz: &TimeZone) -> Date {
    t.to_zoned(tz.clone()).date()
}

/// Every time `schedule` goes off in `[from, to)`, in order, at most `limit` entries: the
/// rule's own occurrences, as [`next_after`] finds them one after another (so the same
/// daylight-saving and short-month behaviour). Schedules that [`folds`] come as one span
/// per local day.
pub fn occurrences(
    schedule: &Schedule,
    from: Timestamp,
    to: Timestamp,
    anchor: Timestamp,
    tz: &TimeZone,
    event_start: Option<Timestamp>,
    limit: usize,
) -> Vec<Span> {
    let mut out = Vec::new();
    // `next_after` is strictly after: start just before `from` so it's included.
    let just_before = |t: Timestamp| Timestamp::from_millisecond(t.as_millisecond() - 1).ok();
    if from >= to {
        return out;
    }
    if let Schedule::Interval { minutes } = schedule
        && folds(schedule)
    {
        let step = *minutes as i64 * 60_000;
        let a = anchor.as_millisecond();
        let mut day = local_date(from, tz);
        while out.len() < limit {
            let Some(next_day) = day.tomorrow().ok() else {
                break;
            };
            let (Some(start), Some(end)) = (
                instant(tz, day.to_datetime(Time::midnight())),
                instant(tz, next_day.to_datetime(Time::midnight())),
            ) else {
                break;
            };
            let (lo, hi) = (start.max(from), end.min(to));
            if lo >= to {
                break;
            }
            if let Some(first) =
                just_before(lo).and_then(|b| next_after(schedule, b, anchor, tz, None))
                && first < hi
            {
                // The last one before the end of the day (or of the range).
                let last = a + (hi.as_millisecond() - 1 - a).div_euclid(step) * step;
                let count = (last - first.as_millisecond()) / step + 1;
                if let Ok(last) = Timestamp::from_millisecond(last) {
                    out.push(Span {
                        first,
                        last,
                        count: count.clamp(1, u32::MAX as i64) as u32,
                    });
                }
            }
            day = next_day;
        }
        return out;
    }
    let mut after = just_before(from);
    while out.len() < limit {
        let Some(t) = after.and_then(|b| next_after(schedule, b, anchor, tz, event_start)) else {
            break;
        };
        if t >= to {
            break;
        }
        out.push(Span {
            first: t,
            last: t,
            count: 1,
        });
        after = Some(t);
    }
    out
}

fn day_name(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

fn ordinal(n: u8) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn duration_words(minutes: u32) -> String {
    match minutes {
        0 => "right".to_owned(),
        m if m % 1440 == 0 => plural(m / 1440, "day"),
        m if m % 60 == 0 => plural(m / 60, "hour"),
        m => plural(m, "minute"),
    }
}

fn plural(n: u32, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// The schedule in words, e.g. "Every weekday at 7:00".
pub fn describe(s: &Schedule) -> String {
    let t = |time: &str| {
        parse_time(time)
            .map(|t| format!("{}:{:02}", t.hour(), t.minute()))
            .unwrap_or_else(|_| time.to_owned())
    };
    match s {
        Schedule::Once { at } => match parse_local(at) {
            Ok(dt) => format!(
                "Once, {} {} {} at {}:{:02}",
                &format!("{:?}", dt.date().weekday())[..3],
                dt.day(),
                &MONTHS[(dt.month() - 1) as usize][..3],
                dt.hour(),
                dt.minute()
            ),
            Err(_) => "Once".to_owned(),
        },
        Schedule::Daily { time } => format!("Every day at {}", t(time)),
        Schedule::Weekdays { time } => format!("Every weekday at {}", t(time)),
        Schedule::Weekly { days, time } => {
            let mut days = days.clone();
            days.sort();
            days.dedup();
            let names: Vec<&str> = days.iter().map(|d| day_name(*d)).collect();
            let list = match names.as_slice() {
                [] => "week".to_owned(),
                [one] => (*one).to_owned(),
                [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            };
            format!("Every {list} at {}", t(time))
        }
        Schedule::Monthly { day, time } => {
            format!("Every month on the {} at {}", ordinal(*day), t(time))
        }
        Schedule::Yearly { month, day, time } => format!(
            "Every year on {} {} at {}",
            day,
            MONTHS
                .get((*month as usize).saturating_sub(1))
                .unwrap_or(&"?"),
            t(time)
        ),
        Schedule::Interval { minutes } => format!("Every {}", duration_words(*minutes)),
        Schedule::BeforeEvent {
            event_title,
            minutes_before,
            ..
        } => {
            if *minutes_before == 0 {
                format!("When “{event_title}” starts")
            } else {
                format!("{} before “{event_title}”", duration_words(*minutes_before))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zurich() -> TimeZone {
        TimeZone::get("Europe/Zurich").unwrap()
    }

    /// An instant from Zurich wall-clock time.
    fn at(s: &str) -> Timestamp {
        instant(&zurich(), parse_local(s).unwrap()).unwrap()
    }

    fn local(t: Timestamp) -> String {
        t.to_zoned(zurich())
            .strftime("%Y-%m-%d %a %H:%M")
            .to_string()
    }

    fn next(s: &Schedule, after: &str) -> Option<String> {
        next_after(s, at(after), at("2026-01-01T00:00"), &zurich(), None).map(local)
    }

    fn time(t: &str) -> String {
        t.to_owned()
    }

    #[test]
    fn once_is_once() {
        let s = Schedule::Once {
            at: "2026-10-01T09:00".into(),
        };
        assert_eq!(
            next(&s, "2026-09-30T12:00").as_deref(),
            Some("2026-10-01 Thu 09:00")
        );
        assert_eq!(next(&s, "2026-10-01T09:00"), None);
    }

    #[test]
    fn daily_weekdays_weekly() {
        let daily = Schedule::Daily {
            time: time("07:00"),
        };
        assert_eq!(
            next(&daily, "2026-10-02T06:59").as_deref(),
            Some("2026-10-02 Fri 07:00")
        );
        assert_eq!(
            next(&daily, "2026-10-02T07:00").as_deref(),
            Some("2026-10-03 Sat 07:00")
        );

        let weekdays = Schedule::Weekdays {
            time: time("07:00"),
        };
        // Friday after 7 → Monday.
        assert_eq!(
            next(&weekdays, "2026-10-02T08:00").as_deref(),
            Some("2026-10-05 Mon 07:00")
        );

        let bins = Schedule::Weekly {
            days: vec![Weekday::Mon, Weekday::Thu],
            time: time("20:00"),
        };
        assert_eq!(
            next(&bins, "2026-10-05T20:30").as_deref(),
            Some("2026-10-08 Thu 20:00")
        );
    }

    #[test]
    fn monthly_and_yearly_clamp_short_months() {
        let rent = Schedule::Monthly {
            day: 31,
            time: time("09:00"),
        };
        assert_eq!(
            next(&rent, "2026-09-01T00:00").as_deref(),
            Some("2026-09-30 Wed 09:00")
        );
        assert_eq!(
            next(&rent, "2026-09-30T10:00").as_deref(),
            Some("2026-10-31 Sat 09:00")
        );

        let leap = Schedule::Yearly {
            month: 2,
            day: 29,
            time: time("08:00"),
        };
        assert_eq!(
            next(&leap, "2026-01-10T00:00").as_deref(),
            Some("2026-02-28 Sat 08:00")
        );
        assert_eq!(
            next(&leap, "2027-03-01T00:00").as_deref(),
            Some("2028-02-29 Tue 08:00")
        );
    }

    #[test]
    fn wall_clock_survives_daylight_saving_changes() {
        // Europe switches to summer time on 29 March 2026 (02:00 → 03:00) and back on
        // 25 October 2026 (03:00 → 02:00).
        let daily = Schedule::Daily {
            time: time("07:00"),
        };
        let before = next(&daily, "2026-03-28T08:00").unwrap();
        let after = next(&daily, "2026-03-29T08:00").unwrap();
        assert_eq!(before, "2026-03-29 Sun 07:00");
        assert_eq!(after, "2026-03-30 Mon 07:00");
        // 23 hours apart in absolute time, still 07:00 on the wall.
        let gap = at("2026-03-29T07:00").duration_since(at("2026-03-28T07:00"));
        assert_eq!(gap.as_secs(), 23 * 3600);

        // A time that doesn't exist on the spring-forward night moves by the gap.
        let skipped = Schedule::Daily {
            time: time("02:30"),
        };
        assert_eq!(
            next(&skipped, "2026-03-28T03:00").as_deref(),
            Some("2026-03-29 Sun 03:30")
        );
        // A time that happens twice in autumn fires once, the first time.
        let twice = Schedule::Daily {
            time: time("02:30"),
        };
        let first = next_after(
            &twice,
            at("2026-10-25T00:00"),
            at("2026-01-01T00:00"),
            &zurich(),
            None,
        )
        .unwrap();
        let again = next_after(&twice, first, at("2026-01-01T00:00"), &zurich(), None).unwrap();
        assert_eq!(local(again), "2026-10-26 Mon 02:30");
    }

    #[test]
    fn intervals_count_from_their_anchor() {
        let s = Schedule::Interval { minutes: 30 };
        let anchor = at("2026-10-01T09:10");
        let n = |after: &str| next_after(&s, at(after), anchor, &zurich(), None).map(local);
        assert_eq!(
            n("2026-10-01T09:00").as_deref(),
            Some("2026-10-01 Thu 09:10")
        );
        assert_eq!(
            n("2026-10-01T09:10").as_deref(),
            Some("2026-10-01 Thu 09:40")
        );
        assert_eq!(
            n("2026-10-01T11:00").as_deref(),
            Some("2026-10-01 Thu 11:10")
        );
    }

    #[test]
    fn event_relative_follows_the_event() {
        let s = Schedule::BeforeEvent {
            event_id: "ev:x".into(),
            event_title: "Dentist".into(),
            minutes_before: 60,
        };
        let anchor = at("2026-01-01T00:00");
        let when = |start: &str, after: &str| {
            next_after(&s, at(after), anchor, &zurich(), Some(at(start))).map(local)
        };
        assert_eq!(
            when("2026-10-02T10:00", "2026-10-01T12:00").as_deref(),
            Some("2026-10-02 Fri 09:00")
        );
        // The dentist moved to the afternoon: the reminder moves with it.
        assert_eq!(
            when("2026-10-02T15:00", "2026-10-01T12:00").as_deref(),
            Some("2026-10-02 Fri 14:00")
        );
        assert_eq!(when("2026-10-02T10:00", "2026-10-02T09:30"), None);
        assert_eq!(
            next_after(&s, at("2026-10-01T12:00"), anchor, &zurich(), None),
            None
        );
    }

    /// Occurrences in a range, as Zurich wall-clock text ("first – last ×count" when folded).
    fn between(s: &Schedule, from: &str, to: &str) -> Vec<String> {
        between_limited(s, from, to, 1000)
    }

    fn between_limited(s: &Schedule, from: &str, to: &str, limit: usize) -> Vec<String> {
        occurrences(
            s,
            at(from),
            at(to),
            at("2026-01-01T00:00"),
            &zurich(),
            None,
            limit,
        )
        .into_iter()
        .map(|o| {
            if o.count == 1 {
                assert_eq!(o.first, o.last);
                local(o.first)
            } else {
                format!("{} – {} ×{}", local(o.first), local(o.last), o.count)
            }
        })
        .collect()
    }

    #[test]
    fn occurrences_of_daily_weekday_and_weekly_rules() {
        let daily = Schedule::Daily {
            time: time("07:00"),
        };
        // One exactly at the start counts; one exactly at the end doesn't.
        assert_eq!(
            between(&daily, "2026-10-05T07:00", "2026-10-09T07:00"),
            [
                "2026-10-05 Mon 07:00",
                "2026-10-06 Tue 07:00",
                "2026-10-07 Wed 07:00",
                "2026-10-08 Thu 07:00",
            ]
        );
        let weekdays = Schedule::Weekdays {
            time: time("08:30"),
        };
        assert_eq!(
            between(&weekdays, "2026-10-02T09:00", "2026-10-10T00:00"),
            [
                "2026-10-05 Mon 08:30",
                "2026-10-06 Tue 08:30",
                "2026-10-07 Wed 08:30",
                "2026-10-08 Thu 08:30",
                "2026-10-09 Fri 08:30",
            ]
        );
        let bins = Schedule::Weekly {
            days: vec![Weekday::Thu, Weekday::Mon],
            time: time("20:00"),
        };
        assert_eq!(
            between(&bins, "2026-10-05T00:00", "2026-10-19T00:00"),
            [
                "2026-10-05 Mon 20:00",
                "2026-10-08 Thu 20:00",
                "2026-10-12 Mon 20:00",
                "2026-10-15 Thu 20:00",
            ]
        );
    }

    #[test]
    fn occurrences_clamp_short_months_and_leap_days() {
        let rent = Schedule::Monthly {
            day: 31,
            time: time("09:00"),
        };
        assert_eq!(
            between(&rent, "2026-01-01T00:00", "2026-07-01T00:00"),
            [
                "2026-01-31 Sat 09:00",
                "2026-02-28 Sat 09:00",
                "2026-03-31 Tue 09:00",
                "2026-04-30 Thu 09:00",
                "2026-05-31 Sun 09:00",
                "2026-06-30 Tue 09:00",
            ]
        );
        let leap = Schedule::Yearly {
            month: 2,
            day: 29,
            time: time("08:00"),
        };
        assert_eq!(
            between(&leap, "2026-01-01T00:00", "2029-01-01T00:00"),
            [
                "2026-02-28 Sat 08:00",
                "2027-02-28 Sun 08:00",
                "2028-02-29 Tue 08:00",
            ]
        );
    }

    #[test]
    fn occurrences_keep_wall_clock_times_across_daylight_saving() {
        let daily = Schedule::Daily {
            time: time("07:00"),
        };
        assert_eq!(
            between(&daily, "2026-03-28T00:00", "2026-03-31T00:00"),
            [
                "2026-03-28 Sat 07:00",
                "2026-03-29 Sun 07:00",
                "2026-03-30 Mon 07:00",
            ]
        );
        // Skipped in spring: moved forward by the gap. Repeated in autumn: once.
        let night = Schedule::Daily {
            time: time("02:30"),
        };
        assert_eq!(
            between(&night, "2026-03-28T00:00", "2026-03-31T00:00"),
            [
                "2026-03-28 Sat 02:30",
                "2026-03-29 Sun 03:30",
                "2026-03-30 Mon 02:30",
            ]
        );
        assert_eq!(
            between(&night, "2026-10-24T12:00", "2026-10-27T00:00"),
            ["2026-10-25 Sun 02:30", "2026-10-26 Mon 02:30"]
        );
    }

    #[test]
    fn frequent_intervals_fold_to_one_entry_a_day() {
        // Every three hours: each one separately.
        let s = Schedule::Interval { minutes: 180 };
        let anchor = at("2026-10-01T09:10");
        let list: Vec<String> = occurrences(
            &s,
            at("2026-10-01T00:00"),
            at("2026-10-02T00:00"),
            anchor,
            &zurich(),
            None,
            100,
        )
        .into_iter()
        .map(|o| local(o.first))
        .collect();
        assert_eq!(
            list,
            [
                "2026-10-01 Thu 09:10",
                "2026-10-01 Thu 12:10",
                "2026-10-01 Thu 15:10",
                "2026-10-01 Thu 18:10",
                "2026-10-01 Thu 21:10",
            ]
        );

        // Every half hour: one entry per day, with how many.
        let s = Schedule::Interval { minutes: 30 };
        let folded: Vec<(String, String, u32)> = occurrences(
            &s,
            at("2026-10-01T12:00"),
            at("2026-10-03T06:00"),
            anchor,
            &zurich(),
            None,
            100,
        )
        .into_iter()
        .map(|o| (local(o.first), local(o.last), o.count))
        .collect();
        assert_eq!(
            folded,
            [
                (
                    "2026-10-01 Thu 12:10".into(),
                    "2026-10-01 Thu 23:40".into(),
                    24
                ),
                (
                    "2026-10-02 Fri 00:10".into(),
                    "2026-10-02 Fri 23:40".into(),
                    48
                ),
                (
                    "2026-10-03 Sat 00:10".into(),
                    "2026-10-03 Sat 05:40".into(),
                    12
                ),
            ]
        );

        // Hourly through the days the clocks change: 23 and 25 hours long.
        let hourly = Schedule::Interval { minutes: 60 };
        assert_eq!(
            between(&hourly, "2026-03-29T00:00", "2026-03-30T00:00"),
            ["2026-03-29 Sun 00:00 – 2026-03-29 Sun 23:00 ×23"]
        );
        assert_eq!(
            between(&hourly, "2026-10-25T00:00", "2026-10-26T00:00"),
            ["2026-10-25 Sun 00:00 – 2026-10-25 Sun 23:00 ×25"]
        );
    }

    #[test]
    fn occurrences_are_capped() {
        let daily = Schedule::Daily {
            time: time("07:00"),
        };
        assert_eq!(
            between_limited(&daily, "2026-01-01T00:00", "2027-02-01T00:00", 10).len(),
            10
        );
        // Every minute for a year is a year of daily entries, never half a million.
        let busy = Schedule::Interval { minutes: 1 };
        let year = between(&busy, "2026-01-01T00:00", "2027-01-01T00:00");
        assert_eq!(year.len(), 365);
        assert_eq!(year[0], "2026-01-01 Thu 00:00 – 2026-01-01 Thu 23:59 ×1440");
        assert!(between(&daily, "2026-10-05T00:00", "2026-10-05T00:00").is_empty());
    }

    #[test]
    fn one_time_and_event_relative_occur_once() {
        let once = Schedule::Once {
            at: "2026-10-01T09:00".into(),
        };
        assert_eq!(
            between(&once, "2026-09-28T00:00", "2026-10-05T00:00"),
            ["2026-10-01 Thu 09:00"]
        );
        assert!(between(&once, "2026-10-05T00:00", "2026-10-12T00:00").is_empty());

        let dentist = Schedule::BeforeEvent {
            event_id: "ev:x".into(),
            event_title: "Dentist".into(),
            minutes_before: 60,
        };
        let found = occurrences(
            &dentist,
            at("2026-10-01T00:00"),
            at("2026-10-08T00:00"),
            at("2026-01-01T00:00"),
            &zurich(),
            Some(at("2026-10-02T10:00")),
            100,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(local(found[0].first), "2026-10-02 Fri 09:00");
    }

    #[test]
    fn catch_up_policy() {
        assert_eq!(classify(1_000_000, 1_000_000 + 30_000), Fire::OnTime);
        assert_eq!(classify(1_000_000, 1_000_000 + 10 * 60_000), Fire::Late);
        assert_eq!(
            classify(1_000_000, 1_000_000 + 13 * 3_600_000),
            Fire::Missed
        );
    }

    #[test]
    fn descriptions() {
        assert_eq!(
            describe(&Schedule::Weekdays { time: time("7:00") }),
            "Every weekday at 7:00"
        );
        assert_eq!(
            describe(&Schedule::Weekly {
                days: vec![Weekday::Thu, Weekday::Mon],
                time: time("20:00")
            }),
            "Every Monday and Thursday at 20:00"
        );
        assert_eq!(
            describe(&Schedule::Monthly {
                day: 1,
                time: time("09:00")
            }),
            "Every month on the 1st at 9:00"
        );
        assert_eq!(
            describe(&Schedule::Yearly {
                month: 3,
                day: 12,
                time: time("09:00")
            }),
            "Every year on 12 March at 9:00"
        );
        assert_eq!(
            describe(&Schedule::Interval { minutes: 120 }),
            "Every 2 hours"
        );
        assert_eq!(
            describe(&Schedule::BeforeEvent {
                event_id: "e".into(),
                event_title: "Dentist".into(),
                minutes_before: 60
            }),
            "1 hour before “Dentist”"
        );
        assert_eq!(
            describe(&Schedule::Once {
                at: "2026-10-01T09:00".into()
            }),
            "Once, Thu 1 Oct at 9:00"
        );
    }

    #[test]
    fn validation() {
        assert!(
            validate(&Schedule::Daily {
                time: time("25:00")
            })
            .is_err()
        );
        assert!(
            validate(&Schedule::Weekly {
                days: vec![],
                time: time("09:00")
            })
            .is_err()
        );
        assert!(
            validate(&Schedule::Yearly {
                month: 2,
                day: 30,
                time: time("09:00")
            })
            .is_err()
        );
        assert!(validate(&Schedule::Interval { minutes: 0 }).is_err());
        assert!(
            validate(&Schedule::Once {
                at: "tomorrow".into()
            })
            .is_err()
        );
    }
}
