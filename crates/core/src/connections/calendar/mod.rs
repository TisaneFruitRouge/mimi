//! Calendars: Google (read through its secret address, written through a pre-filled
//! page the user saves) and CalDAV accounts (read and write).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use self::caldav::{CalDav, RemoteCalendar};
use self::ics::CalEvent;

pub mod caldav;
pub mod ics;
pub mod tools;

pub const GOOGLE: &str = "google_calendar";
pub const CALDAV: &str = "caldav";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleConfig {
    /// The calendar's secret iCal address. A credential: never log it.
    pub ics_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalDavConfig {
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub calendars: Vec<RemoteCalendar>,
}

/// A calendar account, ready to read from.
#[derive(Debug, Clone)]
pub enum Account {
    Google { name: String, config: GoogleConfig },
    CalDav { config: CalDavConfig },
}

/// Normalizes and checks a secret iCal address as pasted by the user.
pub fn parse_ics_url(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix("webcal://")
        .map(|rest| format!("https://{rest}"))
        .unwrap_or_else(|| raw.to_owned());
    let url =
        Url::parse(&raw).map_err(|_| "That doesn't look like a calendar address.".to_owned())?;
    if url.scheme() != "https" {
        return Err("The calendar address must start with https://.".to_owned());
    }
    Ok(url)
}

/// Fetches iCal feeds, keeping them briefly so a conversation doesn't refetch on every
/// question.
#[derive(Default)]
pub struct FeedCache(Mutex<HashMap<String, (Instant, String)>>);

const FEED_TTL: Duration = Duration::from_secs(120);

impl FeedCache {
    pub async fn fetch(&self, http: &reqwest::Client, url: &str) -> Result<String, String> {
        if let Some((at, body)) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(url)
            && at.elapsed() < FEED_TTL
        {
            return Ok(body.clone());
        }
        let res = http
            .get(url)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| {
                "Couldn't reach the calendar. Check your internet connection.".to_owned()
            })?;
        match res.status().as_u16() {
            200..=299 => {}
            401 | 403 | 404 => {
                return Err(
                    "The calendar address doesn't work anymore. It may have been reset in Google Calendar."
                        .to_owned(),
                );
            }
            s => {
                return Err(format!(
                    "The calendar service answered with an error ({s})."
                ));
            }
        }
        let body = res
            .text()
            .await
            .map_err(|_| "The calendar download was interrupted.".to_owned())?;
        if !ics::looks_like_calendar(&body) {
            return Err("That address doesn't lead to a calendar. Copy the \"Secret address in iCal format\".".to_owned());
        }
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(url.to_owned(), (Instant::now(), body.clone()));
        Ok(body)
    }

    pub fn forget(&self, url: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(url);
    }
}

/// Events from every calendar of every account in `[from, to)`, sorted. Accounts that
/// fail are reported alongside rather than failing the whole read.
pub async fn events_between(
    http: &reqwest::Client,
    cache: &FeedCache,
    accounts: &[Account],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> (Vec<CalEvent>, Vec<String>) {
    let mut events = Vec::new();
    let mut problems = Vec::new();
    for account in accounts {
        match account {
            Account::Google { name, config } => match cache.fetch(http, &config.ics_url).await {
                Ok(body) => match ics::events_between(&body, name, from, to, &Local) {
                    Ok(found) => events.extend(found),
                    Err(e) => problems.push(format!("{name}: {e}")),
                },
                Err(e) => problems.push(format!("{name}: {e}")),
            },
            Account::CalDav { config } => {
                let client = CalDav::new(&config.username, &config.password);
                for cal in &config.calendars {
                    let Ok(url) = Url::parse(&cal.url) else {
                        continue;
                    };
                    match client.event_data(&url, from, to).await {
                        Ok(objects) => {
                            for data in objects {
                                if let Ok(found) =
                                    ics::events_between(&data, &cal.name, from, to, &Local)
                                {
                                    events.extend(found);
                                }
                            }
                        }
                        Err(e) => problems.push(format!("{}: {e}", cal.name)),
                    }
                }
            }
        }
    }
    events.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
    (events, problems)
}

/// An event the assistant wants to add.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEvent {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub location: Option<String>,
    pub notes: Option<String>,
}

/// Where a new event can go.
#[derive(Debug, Clone)]
pub enum Target {
    CalDav {
        config: CalDavConfig,
        calendar: RemoteCalendar,
    },
    /// Google calendars are read-only here: the user saves the event themselves.
    Google { name: String },
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::CalDav { calendar, .. } => &calendar.name,
            Target::Google { name } => name,
        }
    }
}

/// Every calendar an event could be added to, CalDAV (direct) first.
pub fn targets(accounts: &[Account]) -> Vec<Target> {
    let mut out = Vec::new();
    for account in accounts {
        if let Account::CalDav { config } = account {
            for calendar in &config.calendars {
                out.push(Target::CalDav {
                    config: config.clone(),
                    calendar: calendar.clone(),
                });
            }
        }
    }
    for account in accounts {
        if let Account::Google { name, .. } = account {
            out.push(Target::Google { name: name.clone() });
        }
    }
    out
}

pub enum Created {
    /// Saved in the calendar.
    Saved { calendar: String },
    /// The user needs to open this page and press Save.
    OpenToSave { url: String },
}

pub async fn create(target: &Target, event: &NewEvent) -> Result<Created, String> {
    match target {
        Target::Google { .. } => Ok(Created::OpenToSave {
            url: google_template_link(event),
        }),
        Target::CalDav { config, calendar } => {
            let uid = format!("{}@hearth", uuid::Uuid::now_v7());
            let url = Url::parse(&calendar.url).map_err(|e| e.to_string())?;
            CalDav::new(&config.username, &config.password)
                .create(&url, &uid, to_ics(&uid, event))
                .await
                .map_err(|e| e.to_string())?;
            Ok(Created::Saved {
                calendar: calendar.name.clone(),
            })
        }
    }
}

/// Google Calendar's "create event" page, pre-filled. Nothing is sent to Google until
/// the user opens it and presses Save.
pub fn google_template_link(event: &NewEvent) -> String {
    let dates = if event.all_day {
        let start = event.start.with_timezone(&Local).date_naive();
        let end = event.end.with_timezone(&Local).date_naive();
        format!("{}/{}", start.format("%Y%m%d"), end.format("%Y%m%d"))
    } else {
        format!(
            "{}/{}",
            event.start.format("%Y%m%dT%H%M%SZ"),
            event.end.format("%Y%m%dT%H%M%SZ")
        )
    };
    let mut url = Url::parse("https://calendar.google.com/calendar/render").expect("static URL");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("action", "TEMPLATE");
        q.append_pair("text", &event.title);
        q.append_pair("dates", &dates);
        if let Some(l) = &event.location {
            q.append_pair("location", l);
        }
        if let Some(n) = &event.notes {
            q.append_pair("details", n);
        }
    }
    url.to_string()
}

fn to_ics(uid: &str, event: &NewEvent) -> String {
    use icalendar::{Calendar, Component, EventLike};
    let mut e = icalendar::Event::new();
    e.uid(uid).summary(&event.title).timestamp(Utc::now());
    if event.all_day {
        e.all_day(event.start.with_timezone(&Local).date_naive());
    } else {
        e.starts(event.start).ends(event.end);
    }
    if let Some(l) = &event.location {
        e.location(l);
    }
    if let Some(n) = &event.notes {
        e.description(n);
    }
    let mut cal = Calendar::new();
    cal.push(e.done());
    cal.to_string()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn ics_urls() {
        assert_eq!(
            parse_ics_url(" webcal://calendar.google.com/calendar/ical/x/private-abc/basic.ics ")
                .unwrap()
                .as_str(),
            "https://calendar.google.com/calendar/ical/x/private-abc/basic.ics"
        );
        assert!(parse_ics_url("http://example.com/a.ics").is_err());
        assert!(parse_ics_url("not a url").is_err());
    }

    #[test]
    fn google_link_is_prefilled() {
        let event = NewEvent {
            title: "Dinner with Sam".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 1, 17, 30, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 1, 19, 0, 0).unwrap(),
            all_day: false,
            location: Some("Café du Lac".into()),
            notes: None,
        };
        let link = google_template_link(&event);
        assert!(link.starts_with("https://calendar.google.com/calendar/render?action=TEMPLATE"));
        assert!(link.contains("text=Dinner+with+Sam"));
        assert!(link.contains("dates=20261001T173000Z%2F20261001T190000Z"));
        assert!(link.contains("location=Caf%C3%A9+du+Lac"));
    }

    #[test]
    fn new_events_round_trip_through_ics() {
        let event = NewEvent {
            title: "Dentist".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 2, 8, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 2, 8, 45, 0).unwrap(),
            all_day: false,
            location: None,
            notes: Some("Bring the insurance card".into()),
        };
        let data = to_ics("x@hearth", &event);
        let back = ics::events_between(
            &data,
            "Cal",
            Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 3, 0, 0, 0).unwrap(),
            &Utc,
        )
        .unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].start, back[0].end), (event.start, event.end));
        assert_eq!(back[0].notes.as_deref(), Some("Bring the insurance card"));
    }
}
