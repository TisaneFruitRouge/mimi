//! A small fake of Google's sign-in and Calendar API, for tests. Never Google itself.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::google::Endpoints;

#[derive(Default)]
pub struct Fake {
    /// Calendars as `calendarList` returns them.
    pub calendars: Vec<Value>,
    /// Events by calendar id, already expanded (Google does that with `singleEvents`).
    pub events: HashMap<String, Vec<Value>>,
    /// Refresh tokens Google still accepts.
    pub refresh_tokens: Vec<String>,
    access_tokens: Vec<String>,
    /// PKCE verifiers of every code exchange.
    pub verifiers: Vec<String>,
    pub revoked: Vec<String>,
    pub refreshes: usize,
    next: usize,
}

pub type Shared = Arc<Mutex<Fake>>;

/// Starts the fake; returns its state and the endpoints pointing at it.
pub async fn start(fake: Fake) -> (Shared, Endpoints) {
    let shared: Shared = Arc::new(Mutex::new(fake));
    let app = Router::new()
        .route("/token", post(token))
        .route("/revoke", post(revoke))
        .route("/calendar/v3/users/me/calendarList", get(calendar_list))
        .route(
            "/calendar/v3/calendars/{cal}/events",
            get(list_events).post(insert_event),
        )
        .route(
            "/calendar/v3/calendars/{cal}/events/{id}",
            patch(patch_event).delete(delete_event),
        )
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let endpoints = Endpoints {
        auth: format!("{base}/auth"),
        token: format!("{base}/token"),
        revoke: format!("{base}/revoke"),
        api: format!("{base}/calendar/v3"),
        client_id: "fake-client.apps.googleusercontent.com".to_owned(),
        client_secret: "fake-secret".to_owned(),
    };
    (shared, endpoints)
}

fn form(body: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect()
}

async fn token(State(s): State<Shared>, body: String) -> Response {
    let f = form(&body);
    let mut fake = s.lock().unwrap();
    if f.get("client_id").map(String::as_str) != Some("fake-client.apps.googleusercontent.com") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid_client"})),
        )
            .into_response();
    }
    fake.next += 1;
    let access = format!("access-{}", fake.next);
    match f.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let good = f.get("code").map(String::as_str) == Some("good-code")
                && f.get("redirect_uri")
                    .is_some_and(|r| r.starts_with("http://127.0.0.1:"))
                && f.get("code_verifier").is_some_and(|v| v.len() >= 43);
            if !good {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_grant"})),
                )
                    .into_response();
            }
            fake.verifiers.push(f["code_verifier"].clone());
            let refresh = format!("refresh-{}", fake.next);
            fake.refresh_tokens.push(refresh.clone());
            fake.access_tokens.push(access.clone());
            Json(json!({"access_token": access, "refresh_token": refresh, "expires_in": 3599}))
                .into_response()
        }
        Some("refresh_token") => {
            let known = f
                .get("refresh_token")
                .is_some_and(|r| fake.refresh_tokens.contains(r));
            if !known {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_grant"})),
                )
                    .into_response();
            }
            fake.refreshes += 1;
            fake.access_tokens.push(access.clone());
            Json(json!({"access_token": access, "expires_in": 3599})).into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unsupported_grant_type"})),
        )
            .into_response(),
    }
}

async fn revoke(State(s): State<Shared>, body: String) -> StatusCode {
    let f = form(&body);
    let mut fake = s.lock().unwrap();
    if let Some(t) = f.get("token") {
        fake.refresh_tokens.retain(|r| r != t);
        fake.revoked.push(t.clone());
    }
    StatusCode::OK
}

fn authorized(s: &Shared, headers: &HeaderMap) -> bool {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    s.lock().unwrap().access_tokens.iter().any(|t| t == token)
}

async fn calendar_list(State(s): State<Shared>, headers: HeaderMap) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let items = s.lock().unwrap().calendars.clone();
    Json(json!({ "items": items })).into_response()
}

fn start_of(e: &Value) -> Option<DateTime<Utc>> {
    let t = &e["start"];
    if let Some(dt) = t["dateTime"].as_str() {
        return DateTime::parse_from_rfc3339(dt)
            .ok()
            .map(|d| d.with_timezone(&Utc));
    }
    let date = chrono::NaiveDate::parse_from_str(t["date"].as_str()?, "%Y-%m-%d").ok()?;
    use chrono::TimeZone;
    chrono::Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
}

async fn list_events(
    State(s): State<Shared>,
    headers: HeaderMap,
    Path(cal): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let parse = |k: &str| {
        q.get(k)
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|d| d.with_timezone(&Utc))
    };
    let (from, to) = (parse("timeMin"), parse("timeMax"));
    let items: Vec<Value> = s
        .lock()
        .unwrap()
        .events
        .get(&cal)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| {
            let start = start_of(e);
            start.is_some_and(|st| from.is_none_or(|f| st >= f - chrono::Duration::days(1)))
                && start.is_some_and(|st| to.is_none_or(|t| st < t))
        })
        .collect();
    Json(json!({ "items": items })).into_response()
}

async fn insert_event(
    State(s): State<Shared>,
    headers: HeaderMap,
    Path(cal): Path<String>,
    Json(mut body): Json<Value>,
) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut fake = s.lock().unwrap();
    fake.next += 1;
    body["id"] = json!(format!("new{}", fake.next));
    fake.events.entry(cal).or_default().push(body.clone());
    Json(body).into_response()
}

async fn patch_event(
    State(s): State<Shared>,
    headers: HeaderMap,
    Path((cal, id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut fake = s.lock().unwrap();
    let Some(events) = fake.events.get_mut(&cal) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // The series' id changes every occurrence; an occurrence's id only that one.
    let mut found = false;
    for e in events.iter_mut() {
        if e["id"] == id || e["recurringEventId"] == id {
            for (k, v) in body.as_object().unwrap() {
                e[k] = v.clone();
            }
            found = true;
        }
    }
    if found {
        Json(body).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn delete_event(
    State(s): State<Shared>,
    headers: HeaderMap,
    Path((cal, id)): Path<(String, String)>,
) -> StatusCode {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED;
    }
    let mut fake = s.lock().unwrap();
    let Some(events) = fake.events.get_mut(&cal) else {
        return StatusCode::NOT_FOUND;
    };
    let before = events.len();
    events.retain(|e| e["id"] != id && e["recurringEventId"] != id);
    if events.len() == before {
        StatusCode::GONE
    } else {
        StatusCode::NO_CONTENT
    }
}
