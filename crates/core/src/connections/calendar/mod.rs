//! Calendars: Google signed in with Google (read and write through Google's API), Google
//! read through its secret address (written through a pre-filled page the user saves),
//! and CalDAV accounts (read and write).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use self::caldav::{CalDav, RemoteCalendar};
use self::ics::CalEvent;

pub mod caldav;
pub mod edit;
pub mod google;
pub mod guests;
pub mod ics;
pub mod invite;
pub mod tools;

#[cfg(test)]
pub mod google_fake;

/// A Google calendar read through its secret iCal address.
pub const GOOGLE: &str = "google_calendar";
/// A Google account the user signed in with: all its calendars, read and write.
pub const GOOGLE_ACCOUNT: &str = "google";
pub const CALDAV: &str = "caldav";

/// Said when the assistant or the panel tries to change a calendar read through its
/// private address.
pub const READ_ONLY: &str = "This Google calendar is connected through its private address, so it can only be read. To let your assistant change it, connect Google Calendar again with \"Sign in with Google\" in Settings › Connections.";

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

/// A calendar account, ready to read from. `id` is its connection's id.
#[derive(Debug, Clone)]
pub enum Account {
    Google {
        id: uuid::Uuid,
        name: String,
        config: GoogleConfig,
    },
    CalDav {
        id: uuid::Uuid,
        config: CalDavConfig,
    },
    /// Signed in with Google.
    GoogleApi {
        id: uuid::Uuid,
        config: google::GoogleAccountConfig,
    },
}

/// One calendar the user can see, whatever account it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarRef {
    /// Stable across restarts: the connection id, plus the collection for CalDAV.
    pub id: String,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    /// Events can be saved straight into it (CalDAV, or Google signed in with Google).
    /// Google calendars read through their private address are written by the user
    /// through a pre-filled page.
    pub writable: bool,
    pub google: bool,
}

/// A Google calendar read through its address is a whole connection; a CalDAV one is a
/// collection in an account, and a signed-in Google one a calendar of the account.
pub fn calendar_id(connection: uuid::Uuid, collection_url: Option<&str>) -> String {
    match collection_url {
        None => connection.to_string(),
        Some(url) => format!("{connection}:{:08x}", fnv1a(url.as_bytes())),
    }
}

/// Colours for calendars that don't bring their own, picked by id so they never change.
/// Mid-tone so they read as dots on white and as tints behind dark text.
const PALETTE: &[&str] = &[
    "#0a84ff", "#34c759", "#ff9f0a", "#bf5af2", "#ff375f", "#30b0c7", "#ac8e68", "#5e5ce6",
];

/// The calendar's own colour when the server has one (`#rrggbb` or `#rrggbbaa`), else a
/// stable pick from the palette.
pub fn calendar_color(id: &str, own: Option<&str>) -> String {
    // `get`, not slicing: the server's text may have a multi-byte character anywhere.
    if let Some(hex) = own
        .map(str::trim)
        .and_then(|c| c.strip_prefix('#'))
        .and_then(|c| c.get(..6))
        && hex.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return format!("#{}", hex.to_ascii_lowercase());
    }
    PALETTE[fnv1a(id.as_bytes()) as usize % PALETTE.len()].to_owned()
}

pub(crate) fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

/// Every calendar of every account, Google first as the user connected them.
pub fn calendars(accounts: &[Account]) -> Vec<CalendarRef> {
    let mut out = Vec::new();
    for account in accounts {
        match account {
            Account::Google { id, name, .. } => {
                let cid = calendar_id(*id, None);
                out.push(CalendarRef {
                    color: calendar_color(&cid, None),
                    id: cid,
                    name: name.clone(),
                    writable: false,
                    google: true,
                });
            }
            Account::CalDav { id, config } => {
                for cal in &config.calendars {
                    let cid = calendar_id(*id, Some(&cal.url));
                    out.push(CalendarRef {
                        color: calendar_color(&cid, cal.color.as_deref()),
                        id: cid,
                        name: cal.name.clone(),
                        writable: true,
                        google: false,
                    });
                }
            }
            Account::GoogleApi { id, config } => {
                for cal in &config.calendars {
                    let cid = calendar_id(*id, Some(&cal.id));
                    out.push(CalendarRef {
                        color: calendar_color(&cid, cal.color.as_deref()),
                        id: cid,
                        name: cal.name.clone(),
                        writable: cal.writable,
                        google: true,
                    });
                }
            }
        }
    }
    out
}

/// The accounts cut down to the calendars in `allowed` (ids from [`calendar_id`]):
/// what a trusted person's turn works with, so the other calendars are never even read.
pub fn restrict(accounts: Vec<Account>, allowed: &[String]) -> Vec<Account> {
    let keep = |id: String| allowed.contains(&id);
    accounts
        .into_iter()
        .filter_map(|account| match account {
            Account::Google { id, name, config } => {
                keep(calendar_id(id, None)).then_some(Account::Google { id, name, config })
            }
            Account::CalDav { id, mut config } => {
                config
                    .calendars
                    .retain(|c| keep(calendar_id(id, Some(&c.url))));
                (!config.calendars.is_empty()).then_some(Account::CalDav { id, config })
            }
            Account::GoogleApi { id, mut config } => {
                config
                    .calendars
                    .retain(|c| keep(calendar_id(id, Some(&c.id))));
                (!config.calendars.is_empty()).then_some(Account::GoogleApi { id, config })
            }
        })
        .collect()
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

/// Access to calendars over the network: iCal feeds, kept briefly so a conversation
/// doesn't refetch on every question, and Google's API (tokens, sign-ins).
#[derive(Default)]
pub struct FeedCache {
    feeds: Mutex<HashMap<String, (Instant, String)>>,
    pub google: google::GoogleAccess,
}

const FEED_TTL: Duration = Duration::from_secs(120);

impl FeedCache {
    pub async fn fetch(&self, http: &reqwest::Client, url: &str) -> Result<String, String> {
        if let Some((at, body)) = self
            .feeds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(url)
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
        self.feeds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(url.to_owned(), (Instant::now(), body.clone()));
        Ok(body)
    }

    pub fn forget(&self, url: &str) {
        self.feeds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(url);
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
            Account::Google { id, name, config } => {
                let cid = calendar_id(*id, None);
                match cache.fetch(http, &config.ics_url).await {
                    Ok(body) => match ics::events_between(&body, name, from, to, &Local) {
                        Ok(found) => events.extend(found.into_iter().map(|mut e| {
                            e.calendar_id = cid.clone();
                            e
                        })),
                        Err(e) => problems.push(format!("{name}: {e}")),
                    },
                    Err(e) => problems.push(format!("{name}: {e}")),
                }
            }
            Account::CalDav { id, config } => {
                let client = CalDav::new(&config.username, &config.password);
                for cal in &config.calendars {
                    let Ok(url) = Url::parse(&cal.url) else {
                        continue;
                    };
                    let cid = calendar_id(*id, Some(&cal.url));
                    match client.event_data(&url, from, to).await {
                        Ok(objects) => {
                            for data in objects {
                                if let Ok(found) =
                                    ics::events_between(&data, &cal.name, from, to, &Local)
                                {
                                    events.extend(found.into_iter().map(|mut e| {
                                        e.calendar_id = cid.clone();
                                        e
                                    }));
                                }
                            }
                        }
                        Err(e) => problems.push(format!("{}: {e}", cal.name)),
                    }
                }
            }
            Account::GoogleApi { id, config } => {
                for cal in &config.calendars {
                    let cid = calendar_id(*id, Some(&cal.id));
                    match google::events(&cache.google, http, *id, config, cal, from, to).await {
                        Ok(found) => events.extend(found.into_iter().map(|mut e| {
                            e.calendar_id = cid.clone();
                            e
                        })),
                        Err(e) => {
                            problems.push(format!("{}: {e}", cal.name));
                            // Signed out or offline: the other calendars won't do better.
                            if matches!(
                                e,
                                google::GoogleError::SignedOut | google::GoogleError::Unreachable
                            ) {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    events.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
    (events, problems)
}

/// An event to add.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewEvent {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub location: Option<String>,
    pub notes: Option<String>,
    /// People invited. Nobody is emailed by the calendar service.
    #[serde(default)]
    pub guests: Vec<guests::Guest>,
    /// The user's address, as the organizer of an event with guests (CalDAV; Google
    /// sets it itself). See [`guests::organizer_for`].
    #[serde(default)]
    pub organizer: Option<String>,
}

/// Where a new event can go. `id` is the calendar's (see [`calendar_id`]).
#[derive(Debug, Clone)]
pub enum Target {
    CalDav {
        id: String,
        config: CalDavConfig,
        calendar: RemoteCalendar,
    },
    /// Signed in with Google: saved through Google's API.
    GoogleApi {
        id: String,
        account: uuid::Uuid,
        config: google::GoogleAccountConfig,
        calendar: google::GoogleCalendar,
    },
    /// Google calendars read through their address are read-only here: the user saves
    /// the event themselves.
    Google { id: String, name: String },
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::CalDav { calendar, .. } => &calendar.name,
            Target::GoogleApi { calendar, .. } => &calendar.name,
            Target::Google { name, .. } => name,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Target::CalDav { id, .. }
            | Target::GoogleApi { id, .. }
            | Target::Google { id, .. } => id,
        }
    }
}

/// The calendar with this id (see [`calendar_id`]), as a place to add an event.
pub fn target_by_id(accounts: &[Account], wanted: &str) -> Option<Target> {
    all_targets(accounts).into_iter().find(|t| t.id() == wanted)
}

/// Every calendar an event could be added to: the ones saved into directly first
/// (CalDAV, then Google signed in), then Google calendars read through their address.
pub fn targets(accounts: &[Account]) -> Vec<Target> {
    let mut out = all_targets(accounts);
    out.retain(|t| !matches!(t, Target::GoogleApi { calendar, .. } if !calendar.writable));
    out.sort_by_key(|t| match t {
        Target::CalDav { .. } => 0,
        Target::GoogleApi { .. } => 1,
        Target::Google { .. } => 2,
    });
    out
}

fn all_targets(accounts: &[Account]) -> Vec<Target> {
    let mut out = Vec::new();
    for account in accounts {
        match account {
            Account::CalDav { id, config } => {
                for calendar in &config.calendars {
                    out.push(Target::CalDav {
                        id: calendar_id(*id, Some(&calendar.url)),
                        config: config.clone(),
                        calendar: calendar.clone(),
                    });
                }
            }
            Account::GoogleApi { id, config } => {
                for calendar in &config.calendars {
                    out.push(Target::GoogleApi {
                        id: calendar_id(*id, Some(&calendar.id)),
                        account: *id,
                        config: config.clone(),
                        calendar: calendar.clone(),
                    });
                }
            }
            Account::Google { id, name, .. } => out.push(Target::Google {
                id: calendar_id(*id, None),
                name: name.clone(),
            }),
        }
    }
    out
}

pub enum Created {
    /// Saved in the calendar, as `uid` (how Mimi finds it again: the CalDAV UID or
    /// Google's event id) and `ical_uid` (the UID other calendars know it by).
    Saved {
        calendar: String,
        uid: String,
        ical_uid: String,
    },
    /// The user needs to open this page and press Save.
    OpenToSave { url: String },
}

pub async fn create(
    http: &reqwest::Client,
    cache: &FeedCache,
    target: &Target,
    event: &NewEvent,
) -> Result<Created, String> {
    match target {
        Target::Google { .. } => Ok(Created::OpenToSave {
            url: google_template_link(event),
        }),
        Target::GoogleApi {
            account,
            config,
            calendar,
            ..
        } => {
            if !calendar.writable {
                return Err("You can only read that calendar.".to_owned());
            }
            let (uid, ical_uid) =
                google::insert(&cache.google, http, *account, config, calendar, event)
                    .await
                    .map_err(|e| e.to_string())?;
            Ok(Created::Saved {
                calendar: calendar.name.clone(),
                uid,
                ical_uid,
            })
        }
        Target::CalDav {
            config, calendar, ..
        } => {
            if !event.guests.is_empty() && event.organizer.is_none() {
                return Err("Guests need an organizer's address.".to_owned());
            }
            let uid = format!("{}@mimi", uuid::Uuid::now_v7());
            let url = Url::parse(&calendar.url).map_err(|e| e.to_string())?;
            CalDav::new(&config.username, &config.password)
                .create(&url, &uid, to_ics(&uid, event))
                .await
                .map_err(|e| e.to_string())?;
            Ok(Created::Saved {
                calendar: calendar.name.clone(),
                ical_uid: uid.clone(),
                uid,
            })
        }
    }
}

/// An existing event occurrence: found again by its calendar, uid and start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRef {
    pub calendar_id: String,
    pub uid: String,
    pub start: DateTime<Utc>,
}

/// An event as it is now, and whether it repeats.
#[derive(Debug, Clone, Default)]
pub struct Located {
    pub event: CalEvent,
    pub repeats: bool,
    /// The UID other calendars know it by (for Google, its `iCalUID`).
    pub ical_uid: String,
    /// Its version (`SEQUENCE`), the occurrence's own for an override.
    pub sequence: u32,
    /// For one occurrence of a repeating event: its original start (`RECURRENCE-ID`).
    pub occurrence: Option<DateTime<Utc>>,
    /// Google's own list of guests, kept whole so a change doesn't lose their answers.
    pub google_attendees: Vec<serde_json::Value>,
}

/// Finds an event to change or remove. Calendars that can only be read say so.
pub async fn locate(
    http: &reqwest::Client,
    cache: &FeedCache,
    accounts: &[Account],
    r: &EventRef,
) -> Result<Located, String> {
    let target = all_targets(accounts)
        .into_iter()
        .find(|t| t.id() == r.calendar_id)
        .ok_or("That calendar isn't connected anymore.")?;
    match &target {
        Target::Google { .. } => Err(READ_ONLY.to_owned()),
        Target::GoogleApi {
            account,
            config,
            calendar,
            ..
        } => {
            let found = google::find_occurrence(
                &cache.google,
                http,
                *account,
                config,
                calendar,
                &r.uid,
                r.start,
            )
            .await
            .map_err(|e| e.to_string())?;
            let mut event = found.event;
            event.calendar_id = r.calendar_id.clone();
            Ok(Located {
                event,
                repeats: found.repeats,
                ical_uid: found.ical_uid,
                sequence: found.sequence,
                occurrence: found.original_start,
                google_attendees: found.attendees,
            })
        }
        Target::CalDav {
            config, calendar, ..
        } => {
            let (object, found) = dav_object(config, calendar, r).await?;
            let later = r.start + chrono::Duration::seconds(1);
            let mut event =
                ics::events_between(&object.data, &calendar.name, r.start, later, &Local)
                    .ok()
                    .and_then(|events| {
                        events
                            .into_iter()
                            .find(|e| e.uid == r.uid && e.start == r.start)
                    })
                    .ok_or("That event isn't in the calendar anymore.")?;
            event.calendar_id = r.calendar_id.clone();
            Ok(Located {
                event,
                repeats: found.repeats,
                ical_uid: r.uid.clone(),
                sequence: edit::sequence_of(&object.data, &r.uid, found, &Local),
                occurrence: found.occurrence.filter(|_| found.repeats),
                google_attendees: Vec::new(),
            })
        }
    }
}

/// The stored object holding the event, and where in it the occurrence is.
async fn dav_object(
    config: &CalDavConfig,
    calendar: &RemoteCalendar,
    r: &EventRef,
) -> Result<(caldav::DavObject, edit::Found), String> {
    let url = Url::parse(&calendar.url).map_err(|e| e.to_string())?;
    let later = r.start + chrono::Duration::seconds(1);
    let objects = CalDav::new(&config.username, &config.password)
        .objects(&url, r.start, later)
        .await
        .map_err(|e| e.to_string())?;
    objects
        .into_iter()
        .find_map(|o| edit::find(&o.data, &r.uid, r.start, &Local).map(|f| (o, f)))
        .ok_or_else(|| "That event isn't in the calendar anymore.".to_owned())
}

/// Changes an event: this occurrence only, or (`whole`) the whole event or series.
/// Nobody is emailed by the calendar service: Google is told not to, and CalDAV guests
/// of the user's own events (`me`) are marked as handled by the client (`edit::quiet`).
pub async fn change_event(
    http: &reqwest::Client,
    cache: &FeedCache,
    accounts: &[Account],
    r: &EventRef,
    whole: bool,
    changes: &edit::Changes,
    me: &HashSet<String>,
) -> Result<(), String> {
    let target = all_targets(accounts)
        .into_iter()
        .find(|t| t.id() == r.calendar_id)
        .ok_or("That calendar isn't connected anymore.")?;
    match &target {
        Target::Google { .. } => Err(READ_ONLY.to_owned()),
        Target::GoogleApi {
            account,
            config,
            calendar,
            ..
        } => {
            let found = google::find_occurrence(
                &cache.google,
                http,
                *account,
                config,
                calendar,
                &r.uid,
                r.start,
            )
            .await
            .map_err(|e| e.to_string())?;
            let id = if whole && found.repeats {
                if changes.when.is_some() {
                    return Err("Every occurrence of a repeating event can't be moved at once. Move one occurrence at a time, or change the series in Google Calendar.".to_owned());
                }
                r.uid.clone()
            } else {
                found.id
            };
            google::patch(
                &cache.google,
                http,
                *account,
                config,
                calendar,
                &id,
                &google::patch_fields(changes, &found.attendees),
            )
            .await
            .map_err(|e| e.to_string())
        }
        Target::CalDav {
            config, calendar, ..
        } => {
            let (object, found) = dav_object(config, calendar, r).await?;
            let data = edit::change(&object.data, &r.uid, found, whole, changes, &Local)?;
            let data = edit::quiet(&data, &r.uid, me)?.unwrap_or(data);
            CalDav::new(&config.username, &config.password)
                .replace(&object, data)
                .await
                .map_err(|e| e.to_string())
        }
    }
}

/// Removes an event: this occurrence only, or (`whole`) the whole event or series.
/// As with changes, the calendar service tells nobody.
pub async fn remove_event(
    http: &reqwest::Client,
    cache: &FeedCache,
    accounts: &[Account],
    r: &EventRef,
    whole: bool,
    me: &HashSet<String>,
) -> Result<(), String> {
    let target = all_targets(accounts)
        .into_iter()
        .find(|t| t.id() == r.calendar_id)
        .ok_or("That calendar isn't connected anymore.")?;
    match &target {
        Target::Google { .. } => Err(READ_ONLY.to_owned()),
        Target::GoogleApi {
            account,
            config,
            calendar,
            ..
        } => {
            let found = google::find_occurrence(
                &cache.google,
                http,
                *account,
                config,
                calendar,
                &r.uid,
                r.start,
            )
            .await
            .map_err(|e| e.to_string())?;
            let id = if whole && found.repeats {
                r.uid.clone()
            } else {
                found.id
            };
            google::delete(&cache.google, http, *account, config, calendar, &id)
                .await
                .map_err(|e| e.to_string())
        }
        Target::CalDav {
            config, calendar, ..
        } => {
            let (object, found) = dav_object(config, calendar, r).await?;
            let client = CalDav::new(&config.username, &config.password);
            match found.occurrence {
                Some(occurrence) if found.repeats && !whole => {
                    let data = edit::remove_occurrence(&object.data, &r.uid, occurrence, &Local)?;
                    let data = edit::quiet(&data, &r.uid, me)?.unwrap_or(data);
                    client.replace(&object, data).await
                }
                _ => {
                    // A server would tell guests it isn't told to leave alone that the
                    // event is cancelled: say so first, then remove it.
                    let object = match edit::quiet(&object.data, &r.uid, me)? {
                        None => object,
                        Some(quiet) => {
                            client
                                .replace(&object, quiet)
                                .await
                                .map_err(|e| e.to_string())?;
                            dav_object(config, calendar, r).await?.0
                        }
                    };
                    client.delete(&object).await
                }
            }
            .map_err(|e| e.to_string())
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
    if let (false, Some(me)) = (event.guests.is_empty(), &event.organizer) {
        e.append_property(guests::organizer_property(me));
        for g in &event.guests {
            e.append_multi_property(guests::attendee_property(g));
        }
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
            ..Default::default()
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
            ..Default::default()
        };
        let data = to_ics("x@mimi", &event);
        assert!(!data.contains("ATTENDEE") && !data.contains("ORGANIZER"));
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

    /// Guests are written so the CalDAV server emails nobody: the client (Mimi) schedules.
    #[test]
    fn new_events_with_guests_tell_the_server_not_to_email_them() {
        let event = NewEvent {
            title: "Dinner".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 2, 17, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 2, 19, 0, 0).unwrap(),
            guests: vec![guests::Guest {
                name: Some("Sam Carter".into()),
                email: "sam@example.com".into(),
                response: None,
            }],
            organizer: Some("me@example.org".into()),
            ..Default::default()
        };
        let data = to_ics("x@mimi", &event).replace("\r\n ", "");
        assert!(
            data.contains("ORGANIZER;SCHEDULE-AGENT=CLIENT:mailto:me@example.org"),
            "{data}"
        );
        let attendee = data
            .lines()
            .find(|l| l.starts_with("ATTENDEE"))
            .expect("the guest is on the event");
        assert_eq!(
            attendee,
            "ATTENDEE;CN=\"Sam Carter\";PARTSTAT=NEEDS-ACTION;ROLE=REQ-PARTICIPANT;RSVP=TRUE;SCHEDULE-AGENT=CLIENT:mailto:sam@example.com"
        );
    }

    fn caldav(id: uuid::Uuid, cals: &[(&str, &str, Option<&str>)]) -> Account {
        Account::CalDav {
            id,
            config: CalDavConfig {
                server_url: "https://dav.example".into(),
                username: "me".into(),
                password: "pw".into(),
                calendars: cals
                    .iter()
                    .map(|(url, name, color)| RemoteCalendar {
                        url: (*url).into(),
                        name: (*name).into(),
                        color: color.map(str::to_owned),
                    })
                    .collect(),
            },
        }
    }

    #[test]
    fn calendars_have_stable_ids_and_colours() {
        let google_id = uuid::Uuid::from_u128(1);
        let dav_id = uuid::Uuid::from_u128(2);
        let accounts = vec![
            Account::Google {
                id: google_id,
                name: "Holidays".into(),
                config: GoogleConfig {
                    ics_url: "https://example/basic.ics".into(),
                },
            },
            caldav(
                dav_id,
                &[
                    ("https://dav.example/me/home/", "Home", Some("#E8A33DFF")),
                    ("https://dav.example/me/work/", "Work", None),
                ],
            ),
        ];
        let cals = calendars(&accounts);
        let summary: Vec<(&str, bool)> =
            cals.iter().map(|c| (c.name.as_str(), c.writable)).collect();
        assert_eq!(
            summary,
            [("Holidays", false), ("Home", true), ("Work", true)]
        );
        // The server's colour wins (alpha dropped); others come from the palette, the same
        // every time.
        assert_eq!(cals[1].color, "#e8a33d");
        assert!(PALETTE.contains(&cals[2].color.as_str()));
        assert_eq!(calendars(&accounts), cals);
        assert_eq!(cals[0].id, google_id.to_string());
        assert_ne!(cals[1].id, cals[2].id);
        // Ids lead back to the right place to add an event.
        assert!(matches!(
            target_by_id(&accounts, &cals[0].id),
            Some(Target::Google { .. })
        ));
        assert_eq!(
            target_by_id(&accounts, &cals[2].id)
                .map(|t| t.name().to_owned())
                .as_deref(),
            Some("Work")
        );
        assert!(target_by_id(&accounts, "nope").is_none());
    }

    /// The server writes the colour: anything that isn't one gets a palette colour.
    #[test]
    fn odd_colours_fall_back_to_the_palette() {
        for odd in ["#12345é", "#é12345", "#1234", "12345678", "#12345g", "#"] {
            assert!(
                PALETTE.contains(&calendar_color("x", Some(odd)).as_str()),
                "{odd}"
            );
        }
        assert_eq!(calendar_color("x", Some(" #A1B2C3é ")), "#a1b2c3");
    }

    #[test]
    fn attendees_and_organizers_are_read() {
        let data = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:t\r\nBEGIN:VEVENT\r\nUID:x\r\n\
DTSTART:20261002T170000Z\r\nDTEND:20261002T180000Z\r\nSUMMARY:Dinner\r\n\
ORGANIZER;CN=Vincent:mailto:vincent@example.com\r\n\
ATTENDEE;CN=\"Sam Carter\";PARTSTAT=ACCEPTED:mailto:Sam@Example.com\r\n\
ATTENDEE;CUTYPE=INDIVIDUAL;EMAIL=lea@example.com:urn:uuid:123\r\n\
ATTENDEE:mailto:nobody\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = ics::events_between(
            data,
            "Cal",
            Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 3, 0, 0, 0).unwrap(),
            &Utc,
        )
        .unwrap();
        let e = &events[0];
        assert_eq!(
            e.organizer,
            Some(ics::Attendee {
                name: Some("Vincent".into()),
                email: "vincent@example.com".into(),
                ..Default::default()
            })
        );
        let people: Vec<(Option<&str>, &str)> = e
            .attendees
            .iter()
            .map(|a| (a.name.as_deref(), a.email.as_str()))
            .collect();
        assert_eq!(
            people,
            [
                (Some("Sam Carter"), "sam@example.com"),
                (None, "lea@example.com")
            ]
        );
    }

    /// Against a real CalDAV server with no password, e.g. Radicale:
    /// `uvx radicale --auth-type=none --storage-filesystem-folder=/tmp/r` then
    /// `MIMI_TEST_CALDAV=http://127.0.0.1:5232/ cargo test -p mimi-core live_caldav -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_caldav_adds_changes_and_removes_events() {
        use chrono::TimeZone;
        let Ok(server) = std::env::var("MIMI_TEST_CALDAV") else {
            panic!("set MIMI_TEST_CALDAV to a CalDAV server's address");
        };
        let server = Url::parse(&server).unwrap();
        let collection = server
            .join(&format!("mimi-test/cal-{}/", uuid::Uuid::now_v7()))
            .unwrap();
        let made = reqwest::Client::new()
            .request(
                reqwest::Method::from_bytes(b"MKCALENDAR").unwrap(),
                collection.clone(),
            )
            .basic_auth("mimi-test", Some("x"))
            .send()
            .await
            .unwrap();
        assert!(made.status().is_success(), "{}", made.status());
        let dav = CalDav::new("mimi-test", "x");
        let found = dav.discover(&server).await.unwrap();
        let cal = found
            .into_iter()
            .find(|c| c.url == collection.as_str())
            .expect("the new calendar is discovered");
        let id = uuid::Uuid::now_v7();
        let accounts = vec![Account::CalDav {
            id,
            config: CalDavConfig {
                server_url: server.to_string(),
                username: "mimi-test".into(),
                password: "x".into(),
                calendars: vec![cal.clone()],
            },
        }];
        let cid = calendar_id(id, Some(&cal.url));
        let http = reqwest::Client::new();
        let cache = FeedCache::default();
        let at = |d: u32, h: u32| Utc.with_ymd_and_hms(2026, 10, d, h, 0, 0).unwrap();

        // A single event, added the way the assistant and the panel do.
        let target = target_by_id(&accounts, &cid).unwrap();
        let event = NewEvent {
            title: "Dentist".into(),
            start: at(2, 8),
            end: at(2, 9),
            all_day: false,
            location: Some("Rue du Lac 4".into()),
            notes: None,
            ..Default::default()
        };
        assert!(matches!(
            create(&http, &cache, &target, &event).await.unwrap(),
            Created::Saved { .. }
        ));
        // A weekly series.
        let series = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:t\r\nBEGIN:VEVENT\r\n\
UID:standup@mimi-test\r\nDTSTART;TZID=Europe/Zurich:20261005T090000\r\n\
DTEND;TZID=Europe/Zurich:20261005T091500\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\nSUMMARY:Standup\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";
        dav.create(
            &Url::parse(&cal.url).unwrap(),
            "standup@mimi-test",
            series.into(),
        )
        .await
        .unwrap();

        let read = |from: DateTime<Utc>, to: DateTime<Utc>| {
            let (http, cache, accounts) = (&http, &cache, &accounts);
            async move {
                let (events, problems) = events_between(http, cache, accounts, from, to).await;
                assert!(problems.is_empty(), "{problems:?}");
                events
            }
        };
        let events = read(at(1, 0), at(31, 0)).await;
        assert_eq!(events.len(), 5);
        let dentist = events.iter().find(|e| e.title == "Dentist").unwrap();
        let r = EventRef {
            calendar_id: cid.clone(),
            uid: dentist.uid.clone(),
            start: dentist.start,
        };
        let located = locate(&http, &cache, &accounts, &r).await.unwrap();
        assert!(!located.repeats);

        // Move and rename the single event, clearing its place.
        let changes = edit::Changes {
            title: Some("Dentist (Dr. Lee)".into()),
            when: Some(edit::When {
                start: at(3, 10),
                end: at(3, 11),
                all_day: false,
            }),
            location: Some(String::new()),
            notes: None,
            ..Default::default()
        };
        change_event(
            &http,
            &cache,
            &accounts,
            &r,
            false,
            &changes,
            &HashSet::new(),
        )
        .await
        .unwrap();
        let events = read(at(1, 0), at(31, 0)).await;
        let moved = events.iter().find(|e| e.uid == r.uid).unwrap();
        assert_eq!(
            (moved.title.as_str(), moved.start, moved.location.as_deref()),
            ("Dentist (Dr. Lee)", at(3, 10), None)
        );

        // One occurrence of the series moved, another removed, the rest untouched.
        let standups: Vec<_> = events.iter().filter(|e| e.title == "Standup").collect();
        let second = EventRef {
            calendar_id: cid.clone(),
            uid: standups[1].uid.clone(),
            start: standups[1].start,
        };
        assert!(
            locate(&http, &cache, &accounts, &second)
                .await
                .unwrap()
                .repeats
        );
        let later = edit::Changes {
            when: Some(edit::When {
                start: second.start + chrono::Duration::hours(2),
                end: second.start + chrono::Duration::hours(3),
                all_day: false,
            }),
            ..Default::default()
        };
        change_event(
            &http,
            &cache,
            &accounts,
            &second,
            false,
            &later,
            &HashSet::new(),
        )
        .await
        .unwrap();
        let third = EventRef {
            calendar_id: cid.clone(),
            uid: standups[2].uid.clone(),
            start: standups[2].start,
        };
        remove_event(&http, &cache, &accounts, &third, false, &HashSet::new())
            .await
            .unwrap();
        let events = read(at(1, 0), at(31, 0)).await;
        let starts: Vec<_> = events
            .iter()
            .filter(|e| e.uid == "standup@mimi-test")
            .map(|e| e.start)
            .collect();
        assert_eq!(
            starts,
            [
                standups[0].start,
                second.start + chrono::Duration::hours(2),
                standups[3].start
            ]
        );
        // Moving the whole series at once is refused; renaming it isn't.
        let first = EventRef {
            calendar_id: cid.clone(),
            uid: standups[0].uid.clone(),
            start: standups[0].start,
        };
        assert!(
            change_event(
                &http,
                &cache,
                &accounts,
                &first,
                true,
                &later,
                &HashSet::new()
            )
            .await
            .is_err()
        );
        let renamed = edit::Changes {
            title: Some("Team standup".into()),
            ..Default::default()
        };
        change_event(
            &http,
            &cache,
            &accounts,
            &first,
            true,
            &renamed,
            &HashSet::new(),
        )
        .await
        .unwrap();
        // Then the whole series goes, and the single event too.
        remove_event(&http, &cache, &accounts, &first, true, &HashSet::new())
            .await
            .unwrap();
        let moved_ref = EventRef {
            calendar_id: cid.clone(),
            uid: r.uid.clone(),
            start: at(3, 10),
        };
        remove_event(&http, &cache, &accounts, &moved_ref, false, &HashSet::new())
            .await
            .unwrap();
        assert!(read(at(1, 0), at(31, 0)).await.is_empty());

        // An event with a guest: saved with the organizer, kept through a change (the
        // guest's entry untouched), and removed without the server being asked to
        // tell anyone.
        let lunch = NewEvent {
            title: "Lunch".into(),
            start: at(6, 11),
            end: at(6, 12),
            guests: vec![guests::Guest {
                name: Some("Sam".into()),
                email: "sam@example.com".into(),
                response: None,
            }],
            organizer: Some("mimi-test@example.org".into()),
            ..Default::default()
        };
        create(&http, &cache, &target, &lunch).await.unwrap();
        let found = read(at(6, 0), at(7, 0)).await;
        assert_eq!(found[0].attendees[0].email, "sam@example.com");
        assert_eq!(
            found[0].organizer.as_ref().map(|o| o.email.as_str()),
            Some("mimi-test@example.org")
        );
        let lunch_ref = EventRef {
            calendar_id: cid.clone(),
            uid: found[0].uid.clone(),
            start: at(6, 11),
        };
        let me: HashSet<String> = ["mimi-test@example.org".to_owned()].into();
        let renamed = edit::Changes {
            title: Some("Lunch with Sam".into()),
            ..Default::default()
        };
        change_event(&http, &cache, &accounts, &lunch_ref, false, &renamed, &me)
            .await
            .unwrap();
        let found = read(at(6, 0), at(7, 0)).await;
        assert_eq!(found[0].title, "Lunch with Sam");
        assert_eq!(found[0].attendees.len(), 1);
        let located = locate(&http, &cache, &accounts, &lunch_ref).await.unwrap();
        assert_eq!(located.sequence, 1);
        remove_event(&http, &cache, &accounts, &lunch_ref, false, &me)
            .await
            .unwrap();
        assert!(read(at(1, 0), at(31, 0)).await.is_empty());
        let _ = reqwest::Client::new()
            .delete(collection)
            .basic_auth("mimi-test", Some("x"))
            .send()
            .await;
    }
}
