//! Google Calendar through Google's own API, after the user signs in with Google.
//!
//! Sign-in is OAuth 2.0 for installed apps: the daemon starts a listener on 127.0.0.1 on
//! a random port for the length of the sign-in only, the browser comes back to it with a
//! code, and the daemon exchanges that code straight with Google (PKCE, a random `state`
//! checked on the way back). Only two scopes are asked for: the user's events, and the
//! list of their calendars (read-only). The refresh token lives in the connection's
//! config, in the encrypted database; it never reaches a client or a log. Nothing goes
//! through any server of the project: the app identity (client ID) is only what Google
//! shows on its consent screen.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone, Utc};
use mimi_protocol::{GoogleSignIn, GoogleSignInStatus};
use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::NewEvent;
use super::ics::{Attendee, CalEvent};
use crate::AppState;
use crate::api::error::AppError;

/// Reading and writing events, and listing calendars: nothing else.
pub const SCOPES: &str = "https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/calendar.calendarlist.readonly";

/// How long the browser has to come back from Google's consent page.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// A Google account connected by signing in. A credential: never log it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleAccountConfig {
    /// The account's address (its primary calendar's id).
    pub email: String,
    pub refresh_token: String,
    pub calendars: Vec<GoogleCalendar>,
    /// Google stopped accepting the refresh token: the user must sign in again.
    #[serde(default)]
    pub signed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleCalendar {
    /// Google's id for it, e.g. `sam@gmail.com` or `…@group.calendar.google.com`.
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    /// The user may add and change events in it (owner or writer).
    pub writable: bool,
}

/// Where Google is, and the app identity shown on its consent screen.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub api: String,
    pub client_id: String,
    /// Google gives desktop apps a "client secret" that isn't secret (it ships inside
    /// every copy); PKCE is what protects the exchange.
    pub client_secret: String,
}

impl Endpoints {
    /// Google's own, with the app identity this build was made with
    /// (`MIMI_GOOGLE_CLIENT_ID` / `MIMI_GOOGLE_CLIENT_SECRET` at build time, or the same
    /// variables at run time). `None` when there is none: sign-in isn't offered.
    pub fn google() -> Option<Self> {
        let pick = |runtime: Option<String>, built: Option<&str>| {
            runtime
                .or(built.map(str::to_owned))
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let client_id = pick(
            std::env::var("MIMI_GOOGLE_CLIENT_ID").ok(),
            option_env!("MIMI_GOOGLE_CLIENT_ID"),
        )?;
        let client_secret = pick(
            std::env::var("MIMI_GOOGLE_CLIENT_SECRET").ok(),
            option_env!("MIMI_GOOGLE_CLIENT_SECRET"),
        )
        .unwrap_or_default();
        Some(Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            token: "https://oauth2.googleapis.com/token".to_owned(),
            revoke: "https://oauth2.googleapis.com/revoke".to_owned(),
            api: "https://www.googleapis.com/calendar/v3".to_owned(),
            client_id,
            client_secret,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GoogleError {
    #[error("Google asks you to sign in again (Settings › Connections).")]
    SignedOut,
    #[error("Couldn't reach Google. Check your internet connection.")]
    Unreachable,
    #[error("That event isn't in the calendar anymore.")]
    NotFound,
    #[error("{0}")]
    Other(String),
}

/// Access tokens (valid about an hour) and sign-ins in progress.
#[derive(Default)]
pub struct GoogleAccess {
    /// Tests point this at a fake Google.
    pub endpoints_override: Mutex<Option<Endpoints>>,
    tokens: Mutex<HashMap<Uuid, (Instant, String)>>,
    /// Connections Google stopped accepting since the daemon started, not yet recorded.
    signed_out: Mutex<HashSet<Uuid>>,
    sign_ins: Mutex<HashMap<Uuid, SignInEntry>>,
}

struct SignInEntry {
    started: Instant,
    status: GoogleSignInStatus,
    cancel: CancellationToken,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl GoogleAccess {
    pub fn endpoints(&self) -> Option<Endpoints> {
        lock(&self.endpoints_override)
            .clone()
            .or_else(Endpoints::google)
    }

    /// Connections Google stopped accepting since this was last asked.
    pub fn take_signed_out(&self) -> Vec<Uuid> {
        lock(&self.signed_out).drain().collect()
    }

    pub fn forget(&self, account: Uuid) {
        lock(&self.tokens).remove(&account);
    }

    /// A current access token for the account, refreshed when needed.
    pub async fn access_token(
        &self,
        http: &reqwest::Client,
        account: Uuid,
        config: &GoogleAccountConfig,
    ) -> Result<String, GoogleError> {
        if config.signed_out {
            return Err(GoogleError::SignedOut);
        }
        if let Some((until, token)) = lock(&self.tokens).get(&account)
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let endpoints = self
            .endpoints()
            .ok_or_else(|| GoogleError::Other(UNAVAILABLE.to_owned()))?;
        let granted = token_request(
            http,
            &endpoints,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &config.refresh_token),
            ],
        )
        .await;
        match granted {
            Ok(t) => {
                // A minute early, so a token never expires mid-request.
                let life = t.expires_in.unwrap_or(3600).saturating_sub(60).max(30);
                lock(&self.tokens).insert(
                    account,
                    (
                        Instant::now() + Duration::from_secs(life),
                        t.access_token.clone(),
                    ),
                );
                Ok(t.access_token)
            }
            Err(GoogleError::SignedOut) => {
                lock(&self.signed_out).insert(account);
                Err(GoogleError::SignedOut)
            }
            Err(e) => Err(e),
        }
    }

    /// One Calendar API request, with a fresh token and one retry if Google says the
    /// token has just expired. `None` for an empty answer (a deletion).
    pub async fn api(
        &self,
        http: &reqwest::Client,
        account: Uuid,
        config: &GoogleAccountConfig,
        method: Method,
        url: Url,
        body: Option<&Value>,
    ) -> Result<Option<Value>, GoogleError> {
        for attempt in 0..2 {
            let token = self.access_token(http, account, config).await?;
            let mut req = http
                .request(method.clone(), url.clone())
                .bearer_auth(&token)
                .timeout(Duration::from_secs(20));
            if let Some(body) = body {
                req = req.json(body);
            }
            // reqwest's errors carry the address: never pass them on.
            let res = req.send().await.map_err(|_| GoogleError::Unreachable)?;
            match res.status() {
                StatusCode::UNAUTHORIZED if attempt == 0 => {
                    self.forget(account);
                    continue;
                }
                StatusCode::UNAUTHORIZED => return Err(GoogleError::SignedOut),
                StatusCode::NOT_FOUND | StatusCode::GONE => return Err(GoogleError::NotFound),
                StatusCode::NO_CONTENT => return Ok(None),
                s if s.is_success() => {
                    let text = res.text().await.map_err(|_| GoogleError::Unreachable)?;
                    if text.trim().is_empty() {
                        return Ok(None);
                    }
                    return serde_json::from_str(&text).map(Some).map_err(|_| {
                        GoogleError::Other("Google answered in a way Mimi can't read.".to_owned())
                    });
                }
                StatusCode::FORBIDDEN => {
                    return Err(GoogleError::Other(
                        "Google refused: you may not be allowed to change this calendar."
                            .to_owned(),
                    ));
                }
                s => {
                    return Err(GoogleError::Other(format!(
                        "Google answered with an error ({}).",
                        s.as_u16()
                    )));
                }
            }
        }
        Err(GoogleError::SignedOut)
    }

    fn url(&self, path: &[&str]) -> Result<Url, GoogleError> {
        let base = self
            .endpoints()
            .ok_or_else(|| GoogleError::Other(UNAVAILABLE.to_owned()))?
            .api;
        api_url(&base, path)
    }
}

const UNAVAILABLE: &str = "Google sign-in isn't available in this version of Mimi.";

fn api_url(base: &str, path: &[&str]) -> Result<Url, GoogleError> {
    let mut url = Url::parse(base)
        .map_err(|_| GoogleError::Other("Google's address is wrong.".to_owned()))?;
    url.path_segments_mut()
        .map_err(|_| GoogleError::Other("Google's address is wrong.".to_owned()))?
        .pop_if_empty()
        .extend(path);
    Ok(url)
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

/// Asks Google's token endpoint for tokens. A refused grant means the user has to sign
/// in again.
async fn token_request(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    params: &[(&str, &str)],
) -> Result<TokenAnswer, GoogleError> {
    let body = {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("client_id", &endpoints.client_id);
        if !endpoints.client_secret.is_empty() {
            form.append_pair("client_secret", &endpoints.client_secret);
        }
        for (k, v) in params {
            form.append_pair(k, v);
        }
        form.finish()
    };
    let res = http
        .post(&endpoints.token)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| GoogleError::Unreachable)?;
    let status = res.status();
    let text = res.text().await.map_err(|_| GoogleError::Unreachable)?;
    if status.is_success() {
        return serde_json::from_str(&text).map_err(|_| {
            GoogleError::Other("Google answered in a way Mimi can't read.".to_owned())
        });
    }
    let error = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_default();
    Err(match error.as_str() {
        "invalid_grant" => GoogleError::SignedOut,
        "invalid_client" | "unauthorized_client" => GoogleError::Other(
            "Google doesn't accept this version of Mimi's sign-in anymore.".to_owned(),
        ),
        _ => GoogleError::Other(format!(
            "Google answered with an error ({}).",
            status.as_u16()
        )),
    })
}

// --- Reading -------------------------------------------------------------------------

#[derive(Deserialize)]
struct CalendarList {
    #[serde(default)]
    items: Vec<CalendarListEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarListEntry {
    id: String,
    #[serde(default)]
    summary: String,
    summary_override: Option<String>,
    background_color: Option<String>,
    access_role: Option<String>,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    deleted: bool,
}

/// The account's address and its calendars, with a token straight from sign-in.
async fn list_calendars(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    token: &str,
) -> Result<(String, Vec<GoogleCalendar>), GoogleError> {
    let url = api_url(&endpoints.api, &["users", "me", "calendarList"])?;
    let res = http
        .get(url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| GoogleError::Unreachable)?;
    if !res.status().is_success() {
        return Err(GoogleError::Other(format!(
            "Google didn't list your calendars ({}).",
            res.status().as_u16()
        )));
    }
    let list: CalendarList = res
        .json()
        .await
        .map_err(|_| GoogleError::Other("Google answered in a way Mimi can't read.".to_owned()))?;
    let email = list
        .items
        .iter()
        .find(|c| c.primary)
        .map(|c| c.id.clone())
        .unwrap_or_else(|| "Google".to_owned());
    let mut calendars: Vec<GoogleCalendar> = list
        .items
        .into_iter()
        .filter(|c| !c.hidden && !c.deleted)
        .map(|c| GoogleCalendar {
            name: c
                .summary_override
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(c.summary),
            writable: matches!(c.access_role.as_deref(), Some("owner" | "writer")),
            color: c.background_color,
            id: c.id,
        })
        .collect();
    // The primary calendar first, as Google shows it.
    calendars.sort_by_key(|c| c.id != email);
    Ok((email, calendars))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventList {
    #[serde(default)]
    items: Vec<GEvent>,
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GEvent {
    id: String,
    status: Option<String>,
    summary: Option<String>,
    location: Option<String>,
    description: Option<String>,
    start: Option<GTime>,
    end: Option<GTime>,
    recurring_event_id: Option<String>,
    #[serde(default)]
    attendees: Vec<GPerson>,
    organizer: Option<GPerson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GTime {
    date_time: Option<DateTime<FixedOffset>>,
    date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GPerson {
    email: Option<String>,
    display_name: Option<String>,
}

impl GTime {
    fn instant(&self) -> Option<DateTime<Utc>> {
        match (self.date_time, self.date) {
            (Some(dt), _) => Some(dt.with_timezone(&Utc)),
            (None, Some(d)) => Local
                .from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
                .earliest()
                .map(|d| d.with_timezone(&Utc)),
            _ => None,
        }
    }
}

fn person(p: &GPerson) -> Option<Attendee> {
    let email = p.email.as_ref()?.trim().to_lowercase();
    email.contains('@').then(|| Attendee {
        name: p
            .display_name
            .clone()
            .filter(|n| !n.trim().is_empty() && !n.eq_ignore_ascii_case(&email)),
        email,
    })
}

/// One occurrence as the rest of Mimi reads events. `uid` is the series' id, so an
/// occurrence is found again by calendar, uid and start, as for CalDAV.
fn to_cal_event(e: GEvent, calendar: &str) -> Option<CalEvent> {
    if e.status.as_deref() == Some("cancelled") {
        return None;
    }
    let all_day = e.start.as_ref()?.date_time.is_none();
    let start = e.start.as_ref()?.instant()?;
    let end = e
        .end
        .as_ref()
        .and_then(GTime::instant)
        .filter(|end| *end >= start)
        .unwrap_or(start);
    let text = |s: Option<String>| s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    Some(CalEvent {
        uid: e.recurring_event_id.clone().unwrap_or(e.id.clone()),
        title: text(e.summary).unwrap_or_else(|| "(no title)".to_owned()),
        start,
        end,
        all_day,
        location: text(e.location),
        notes: text(e.description),
        calendar: calendar.to_owned(),
        calendar_id: String::new(),
        attendees: e.attendees.iter().filter_map(person).collect(),
        organizer: e.organizer.as_ref().and_then(person),
    })
}

/// Longest list read for one range; more is left out rather than looping for ever.
const MAX_PAGES: usize = 8;

/// Occurrences overlapping `[from, to)`, expanded by Google itself.
async fn raw_events(
    access: &GoogleAccess,
    http: &reqwest::Client,
    account: Uuid,
    config: &GoogleAccountConfig,
    calendar: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<GEvent>, GoogleError> {
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut url = access.url(&["calendars", calendar, "events"])?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("singleEvents", "true");
            q.append_pair("orderBy", "startTime");
            q.append_pair("maxResults", "2500");
            q.append_pair("timeMin", &from.to_rfc3339());
            q.append_pair("timeMax", &to.to_rfc3339());
            if let Some(p) = &page {
                q.append_pair("pageToken", p);
            }
        }
        let answer = access
            .api(http, account, config, Method::GET, url, None)
            .await?
            .unwrap_or(Value::Null);
        let list: EventList = serde_json::from_value(answer).map_err(|_| {
            GoogleError::Other("Google answered in a way Mimi can't read.".to_owned())
        })?;
        out.extend(list.items);
        page = list.next_page_token;
        if page.is_none() {
            break;
        }
    }
    Ok(out)
}

pub async fn events(
    access: &GoogleAccess,
    http: &reqwest::Client,
    account: Uuid,
    config: &GoogleAccountConfig,
    calendar: &GoogleCalendar,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<CalEvent>, GoogleError> {
    Ok(
        raw_events(access, http, account, config, &calendar.id, from, to)
            .await?
            .into_iter()
            .filter_map(|e| to_cal_event(e, &calendar.name))
            .collect(),
    )
}

// --- Writing -------------------------------------------------------------------------

fn time_json(at: DateTime<Utc>, all_day: bool) -> Value {
    if all_day {
        json!({ "date": at.with_timezone(&Local).date_naive().to_string() })
    } else {
        json!({ "dateTime": at.to_rfc3339() })
    }
}

fn event_body(event: &NewEvent) -> Value {
    let mut body = json!({
        "summary": event.title,
        "start": time_json(event.start, event.all_day),
        "end": time_json(event.end, event.all_day),
    });
    if let Some(l) = &event.location {
        body["location"] = json!(l);
    }
    if let Some(n) = &event.notes {
        body["description"] = json!(n);
    }
    body
}

pub async fn insert(
    access: &GoogleAccess,
    http: &reqwest::Client,
    account: Uuid,
    config: &GoogleAccountConfig,
    calendar: &GoogleCalendar,
    event: &NewEvent,
) -> Result<(), GoogleError> {
    let url = access.url(&["calendars", &calendar.id, "events"])?;
    access
        .api(
            http,
            account,
            config,
            Method::POST,
            url,
            Some(&event_body(event)),
        )
        .await
        .map(|_| ())
}

/// One occurrence found again: as Mimi reads it, Google's id for it, and whether it's
/// part of a repeating series.
pub struct Occurrence {
    pub event: CalEvent,
    pub id: String,
    pub repeats: bool,
}

/// The occurrence of series (or single event) `uid` starting at `start`.
pub async fn find_occurrence(
    access: &GoogleAccess,
    http: &reqwest::Client,
    account: Uuid,
    config: &GoogleAccountConfig,
    calendar: &GoogleCalendar,
    uid: &str,
    start: DateTime<Utc>,
) -> Result<Occurrence, GoogleError> {
    let day = chrono::Duration::days(1);
    let found = raw_events(
        access,
        http,
        account,
        config,
        &calendar.id,
        start - day,
        start + day,
    )
    .await?;
    found
        .into_iter()
        .filter(|e| e.status.as_deref() != Some("cancelled"))
        .find(|e| {
            (e.id == uid || e.recurring_event_id.as_deref() == Some(uid))
                && e.start.as_ref().and_then(GTime::instant) == Some(start)
        })
        .and_then(|e| {
            let id = e.id.clone();
            let repeats = e.recurring_event_id.is_some();
            Some(Occurrence {
                event: to_cal_event(e, &calendar.name)?,
                id,
                repeats,
            })
        })
        .ok_or(GoogleError::NotFound)
}

/// Changes an event (an occurrence's id changes only that occurrence; the series' id,
/// all of them). `fields` is Google's event shape, only what changes.
pub async fn patch(
    access: &GoogleAccess,
    http: &reqwest::Client,
    account: Uuid,
    config: &GoogleAccountConfig,
    calendar: &GoogleCalendar,
    event_id: &str,
    fields: &Value,
) -> Result<(), GoogleError> {
    let url = access.url(&["calendars", &calendar.id, "events", event_id])?;
    access
        .api(http, account, config, Method::PATCH, url, Some(fields))
        .await
        .map(|_| ())
}

pub async fn delete(
    access: &GoogleAccess,
    http: &reqwest::Client,
    account: Uuid,
    config: &GoogleAccountConfig,
    calendar: &GoogleCalendar,
    event_id: &str,
) -> Result<(), GoogleError> {
    let url = access.url(&["calendars", &calendar.id, "events", event_id])?;
    match access
        .api(http, account, config, Method::DELETE, url, None)
        .await
    {
        // Already gone: what was asked for.
        Ok(_) | Err(GoogleError::NotFound) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Changes as Google's event fields.
pub fn patch_fields(changes: &super::edit::Changes) -> Value {
    let mut body = json!({});
    if let Some(t) = &changes.title {
        body["summary"] = json!(t);
    }
    if let Some(w) = changes.when {
        body["start"] = time_json(w.start, w.all_day);
        body["end"] = time_json(w.end, w.all_day);
    }
    if let Some(l) = &changes.location {
        body["location"] = json!(l.trim());
    }
    if let Some(n) = &changes.notes {
        body["description"] = json!(n.trim());
    }
    body
}

/// Tells Google to forget Mimi's access (when the user disconnects). Best effort.
pub async fn revoke(http: &reqwest::Client, endpoints: &Endpoints, refresh_token: &str) {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", refresh_token)
        .finish();
    let sent = http
        .post(&endpoints.revoke)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    if sent.is_err() {
        tracing::warn!("couldn't tell Google to forget a disconnected account");
    }
}

// --- Signing in ----------------------------------------------------------------------

fn random_urlsafe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier))
}

#[cfg(test)]
pub fn tests_challenge(verifier: &str) -> String {
    challenge(verifier)
}

/// Starts signing in: returns Google's consent page for the client to open, and waits
/// in the background for the browser to come back.
pub async fn start_sign_in(
    state: &Arc<AppState>,
    reconnect: Option<Uuid>,
) -> Result<GoogleSignIn, AppError> {
    let access = &state.connections.feeds.google;
    let endpoints = access
        .endpoints()
        .ok_or_else(|| AppError::bad_request(UNAVAILABLE))?;
    if let Some(id) = reconnect {
        let row = crate::connections::store::get(&state.db, id).await?;
        if row.is_none_or(|r| r.integration != super::GOOGLE_ACCOUNT) {
            return Err(AppError::not_found("Google connection"));
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| AppError::bad_request("Couldn't get ready for Google's answer."))?;
    let port = listener.local_addr().map_err(AppError::internal)?.port();
    let redirect = format!("http://127.0.0.1:{port}/");
    let verifier = random_urlsafe(48);
    let expected_state = random_urlsafe(24);
    let mut url = Url::parse(&endpoints.auth).map_err(AppError::internal)?;
    url.query_pairs_mut()
        .append_pair("client_id", &endpoints.client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", &challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &expected_state)
        .append_pair("access_type", "offline")
        // Always ask, so Google always hands over a refresh token.
        .append_pair("prompt", "consent");

    let id = Uuid::now_v7();
    let cancel = CancellationToken::new();
    {
        let mut sign_ins = lock(&access.sign_ins);
        sign_ins.retain(|_, s| s.started.elapsed() < SIGN_IN_TIMEOUT * 2);
        sign_ins.insert(
            id,
            SignInEntry {
                started: Instant::now(),
                status: GoogleSignInStatus::Waiting,
                cancel: cancel.clone(),
            },
        );
    }
    let flow = SignInFlow {
        endpoints,
        redirect,
        verifier,
        expected_state,
        reconnect,
    };
    let state = state.clone();
    tokio::spawn(async move {
        let status = tokio::select! {
            s = flow.run(&state, listener) => s,
            _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => GoogleSignInStatus::Failed {
                error: "The Google sign-in took too long. Try again.".to_owned(),
            },
            _ = cancel.cancelled() => GoogleSignInStatus::Failed {
                error: "The Google sign-in was cancelled.".to_owned(),
            },
        };
        if let Some(entry) = lock(&state.connections.feeds.google.sign_ins).get_mut(&id) {
            entry.status = status;
        }
    });
    Ok(GoogleSignIn {
        id,
        url: url.to_string(),
    })
}

pub fn sign_in_status(state: &AppState, id: Uuid) -> Option<GoogleSignInStatus> {
    lock(&state.connections.feeds.google.sign_ins)
        .get(&id)
        .map(|s| s.status.clone())
}

pub fn cancel_sign_in(state: &AppState, id: Uuid) -> bool {
    lock(&state.connections.feeds.google.sign_ins)
        .get(&id)
        .map(|s| s.cancel.cancel())
        .is_some()
}

struct SignInFlow {
    endpoints: Endpoints,
    redirect: String,
    verifier: String,
    expected_state: String,
    reconnect: Option<Uuid>,
}

impl SignInFlow {
    async fn run(self, state: &Arc<AppState>, listener: TcpListener) -> GoogleSignInStatus {
        let (code, mut browser) = match self.wait_for_code(&listener).await {
            Ok(found) => found,
            Err((error, browser)) => {
                if let Some(mut b) = browser {
                    respond(&mut b, 200, "Sign-in cancelled", &error).await;
                }
                return GoogleSignInStatus::Failed { error };
            }
        };
        // Nothing else may reach the listener from here on.
        drop(listener);
        match self.finish(state, &code).await {
            Ok(connection) => {
                respond(
                    &mut browser,
                    200,
                    "You're signed in",
                    "Your Google Calendar is connected. You can close this tab and go back to Mimi.",
                )
                .await;
                GoogleSignInStatus::Done { connection }
            }
            Err(error) => {
                respond(&mut browser, 200, "Sign-in didn't work", &error).await;
                GoogleSignInStatus::Failed { error }
            }
        }
    }

    /// Waits for the browser to come back with this sign-in's `state`. Anything else
    /// knocking on the port is turned away.
    async fn wait_for_code(
        &self,
        listener: &TcpListener,
    ) -> Result<(String, TcpStream), (String, Option<TcpStream>)> {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return Err(("Couldn't hear back from Google.".to_owned(), None));
            };
            let target =
                match tokio::time::timeout(Duration::from_secs(10), request_target(&mut stream))
                    .await
                {
                    Ok(Some(t)) => t,
                    _ => continue,
                };
            let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
                continue;
            };
            let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
            if params.get("state") != Some(&self.expected_state) {
                respond(
                    &mut stream,
                    404,
                    "Not found",
                    "This page isn't part of a Google sign-in Mimi started.",
                )
                .await;
                continue;
            }
            if let Some(code) = params.get("code").filter(|c| !c.is_empty()) {
                return Ok((code.clone(), stream));
            }
            let error = match params.get("error").map(String::as_str) {
                Some("access_denied") => {
                    "Google sign-in was cancelled. Nothing was connected.".to_owned()
                }
                _ => "Google didn't finish the sign-in. Try again.".to_owned(),
            };
            return Err((error, Some(stream)));
        }
    }

    /// Exchanges the code with Google, lists the calendars and saves the connection.
    async fn finish(
        &self,
        state: &Arc<AppState>,
        code: &str,
    ) -> Result<mimi_protocol::Connection, String> {
        let tokens = token_request(
            &state.http,
            &self.endpoints,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", &self.verifier),
                ("redirect_uri", &self.redirect),
            ],
        )
        .await
        .map_err(|e| match e {
            GoogleError::SignedOut => "Google didn't accept the sign-in. Try again.".to_owned(),
            e => e.to_string(),
        })?;
        let refresh_token = tokens.refresh_token.ok_or_else(|| {
            "Google didn't give lasting access. Try again, and allow both choices on Google's page."
                .to_owned()
        })?;
        let (email, calendars) = list_calendars(&state.http, &self.endpoints, &tokens.access_token)
            .await
            .map_err(|_| {
                "Google didn't list your calendars. Make sure both choices on Google's page are allowed, then try again."
                    .to_owned()
            })?;
        if calendars.is_empty() {
            return Err("That Google account has no calendars.".to_owned());
        }
        let config = GoogleAccountConfig {
            email: email.clone(),
            refresh_token,
            calendars,
            signed_out: false,
        };
        let connection =
            crate::connections::save_google_account(state, self.reconnect, config).await?;
        // The access token from sign-in works right away.
        let life = tokens.expires_in.unwrap_or(3600).saturating_sub(60).max(30);
        lock(&state.connections.feeds.google.tokens).insert(
            connection.id,
            (
                Instant::now() + Duration::from_secs(life),
                tokens.access_token,
            ),
        );
        Ok(connection)
    }
}

/// Reads an HTTP request's head and returns its target (`/?code=…`).
async fn request_target(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 || buf.len() > 16 * 1024 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    (parts.next()? == "GET").then_some(())?;
    let target = parts.next()?;
    target.starts_with('/').then(|| target.to_owned())
}

/// A small plain page for the browser tab. No scripts, nothing loaded from anywhere.
async fn respond(stream: &mut TcpStream, status: u16, title: &str, text: &str) {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{t}</title>\
<style>body{{font:15px -apple-system,system-ui,sans-serif;background:#f5f5f7;color:#1d1d1f;\
display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}\
main{{background:#fff;border-radius:18px;padding:32px 40px;max-width:420px;\
box-shadow:0 1px 3px rgb(0 0 0/.08)}}h1{{font-size:22px;margin:0 0 8px}}p{{margin:0;color:#6e6e73}}</style>\
</head><body><main><h1>{t}</h1><p>{x}</p></main></body></html>",
        t = escape(title),
        x = escape(text)
    );
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
Referrer-Policy: no-referrer\r\nCache-Control: no-store\r\n\
Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_the_sha256_of_the_verifier() {
        // base64url(SHA-256(verifier)) without padding, as RFC 7636 says for S256.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mJ92K9R8-AMr5JoXLNa6H6Sc0UXGuE"),
            "xJAJ1CMbghWXXamGW65iv7KlH1tsLGxQauCA_BtlsI8"
        );
        assert_eq!(random_urlsafe(48).len(), 64);
        assert_ne!(random_urlsafe(24), random_urlsafe(24));
    }

    #[test]
    fn google_events_become_occurrences_of_their_series() {
        let e: GEvent = serde_json::from_value(json!({
            "id": "abc_20261005T070000Z",
            "recurringEventId": "abc",
            "summary": " Standup ",
            "start": {"dateTime": "2026-10-05T09:00:00+02:00"},
            "end": {"dateTime": "2026-10-05T09:15:00+02:00"},
            "attendees": [{"email": "Sam@Example.com", "displayName": "Sam"}, {"displayName": "no mail"}],
            "organizer": {"email": "me@example.com", "self": true}
        }))
        .unwrap();
        let c = to_cal_event(e, "Work").unwrap();
        assert_eq!(c.uid, "abc");
        assert_eq!(c.title, "Standup");
        assert_eq!(c.start.to_rfc3339(), "2026-10-05T07:00:00+00:00");
        assert_eq!(c.end - c.start, chrono::Duration::minutes(15));
        assert_eq!(c.attendees.len(), 1);
        assert_eq!(c.attendees[0].email, "sam@example.com");
        assert!(!c.all_day);

        let day: GEvent = serde_json::from_value(json!({
            "id": "x", "start": {"date": "2026-10-03"}, "end": {"date": "2026-10-04"}
        }))
        .unwrap();
        let c = to_cal_event(day, "Work").unwrap();
        assert!(c.all_day);
        assert_eq!(c.title, "(no title)");
        assert_eq!(c.end - c.start, chrono::Duration::days(1));

        let gone: GEvent =
            serde_json::from_value(json!({"id": "y", "status": "cancelled"})).unwrap();
        assert!(to_cal_event(gone, "Work").is_none());
    }

    #[test]
    fn calendar_addresses_are_encoded() {
        let url = api_url(
            "https://www.googleapis.com/calendar/v3",
            &["calendars", "a b@group.calendar.google.com", "events"],
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://www.googleapis.com/calendar/v3/calendars/a%20b@group.calendar.google.com/events"
        );
    }
}
