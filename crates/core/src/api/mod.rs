//! HTTP routes. Everything except `/health` sits under `/v1` behind the bearer token.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use mimi_protocol::{API_PREFIX, Health, Status};

use crate::{AppState, VERSION};

mod actions;
mod calendar;
mod connections;
mod conversations;
pub mod error;
mod events;
mod hardware;
mod mail;
mod memory;
mod people;
mod permissions;
mod providers;
mod remote;
mod schedule;
mod settings;
mod updates;
mod web;

pub fn router(state: Arc<AppState>) -> Router {
    let authed = Router::new()
        .route("/status", get(status))
        .route("/settings", get(settings::get).put(settings::put))
        .route("/updates", get(updates::status))
        .route("/updates/check", post(updates::check))
        .route("/permissions", get(permissions::list))
        .route("/permissions/{kind}", axum::routing::put(permissions::put))
        .route("/events", get(events::subscribe))
        .route("/providers", get(providers::list).post(providers::create))
        .route("/providers/presets", get(providers::presets))
        .route("/providers/probe", post(providers::probe))
        .route(
            "/providers/{id}",
            axum::routing::patch(providers::update).delete(providers::delete),
        )
        .route("/providers/{id}/models", get(providers::models))
        .route("/providers/{id}/pull", post(providers::pull))
        .route("/providers/{id}/pull/cancel", post(providers::cancel_pull))
        .route(
            "/providers/{id}/models/{model}",
            axum::routing::delete(providers::delete_model),
        )
        .route("/runtime", get(providers::runtime))
        .route("/pulls", get(providers::pulls))
        .route("/catalog", get(hardware::catalog))
        .route("/integrations", get(hardware::integrations))
        .route(
            "/connections",
            get(connections::list).post(connections::create),
        )
        .route(
            "/connections/{id}",
            axum::routing::delete(connections::delete),
        )
        .route(
            "/google/sign-in",
            get(connections::google_info).post(connections::google_start),
        )
        .route(
            "/google/sign-in/{id}",
            get(connections::google_status).delete(connections::google_cancel),
        )
        .route("/people", get(people::list).post(people::create))
        .route("/people/duplicates", get(people::duplicates))
        .route(
            "/people/duplicates/dismiss",
            post(people::dismiss_duplicate),
        )
        .route("/people/sync", post(people::sync))
        .route("/people/removed", get(people::removed))
        .route("/people/removed/{id}/restore", post(people::restore))
        .route("/people/merge", post(people::merge_many))
        .route("/people/merge/preview", post(people::merge_preview))
        .route("/people/merges/{id}/undo", post(people::undo_merge))
        .route(
            "/people/{id}",
            get(people::get)
                .patch(people::update)
                .delete(people::delete),
        )
        .route("/people/{id}/handles", post(people::add_handle))
        .route(
            "/people/{id}/handles/{handle}",
            axum::routing::delete(people::remove_handle).patch(people::update_handle),
        )
        .route("/people/{id}/events", get(calendar::person_events))
        .route(
            "/people/{id}/conversations",
            get(calendar::person_conversations),
        )
        .route("/people/{id}/merge", post(people::merge))
        .route("/people/{id}/split", post(people::split))
        .route("/people/{id}/memory", get(memory::person_notes))
        .route("/mentions", get(people::mentions))
        .route("/calendars", get(calendar::calendars))
        .route(
            "/calendar/events",
            get(calendar::events).post(calendar::create),
        )
        .route(
            "/calendar/events/{id}",
            axum::routing::patch(calendar::change).delete(calendar::remove),
        )
        .route(
            "/calendar/events/{id}/invitations",
            post(calendar::offer_invitations),
        )
        .route("/calendar/invitations/{id}", get(calendar::invitation))
        .route(
            "/calendar/invitations/{id}/send",
            post(calendar::send_invitations),
        )
        .route("/calendar/guests", get(calendar::guest_suggestions))
        .route("/hardware", get(hardware::get))
        .route("/recommendations", get(hardware::recommendations))
        .route(
            "/conversations",
            get(conversations::list).post(conversations::create),
        )
        .route(
            "/conversations/{id}",
            get(conversations::get)
                .patch(conversations::update)
                .delete(conversations::delete),
        )
        .route("/conversations/{id}/messages", post(conversations::send))
        .route("/conversations/{id}/cancel", post(conversations::cancel))
        .route("/actions/{id}/approve", post(actions::approve))
        .route("/actions/{id}/reject", post(actions::reject))
        .route("/memory", get(memory::overview))
        .route(
            "/memory/note",
            get(memory::get_note)
                .put(memory::put_note)
                .delete(memory::delete_note),
        )
        .route("/memory/profile", axum::routing::put(memory::put_profile))
        .route("/memory/learning", axum::routing::put(memory::put_learning))
        .route("/memory/semantic", axum::routing::put(memory::put_semantic))
        .route("/memory/undo/{revision}", post(memory::undo))
        .route("/memory/forget-all", post(memory::forget_all))
        .route("/schedule", get(schedule::list).post(schedule::create))
        .route(
            "/schedule/{id}",
            axum::routing::patch(schedule::update).delete(schedule::delete),
        )
        .route("/schedule/{id}/run", post(schedule::run))
        .route("/schedule/occurrences", get(schedule::occurrences))
        .route("/schedule/deliveries", get(schedule::deliveries))
        .route("/schedule/deliveries/{id}/done", post(schedule::done))
        .route("/schedule/deliveries/{id}/snooze", post(schedule::snooze))
        .route("/schedule/undo/{revision}", post(schedule::undo))
        .route("/mail", get(mail::overview))
        .route("/mail/presets", get(mail::presets))
        .route("/mail/discover", post(mail::discover))
        .route("/mail/threads", get(mail::threads))
        .route("/mail/threads/{id}", get(mail::thread).delete(mail::delete))
        .route("/mail/threads/{id}/read", post(mail::read))
        .route("/mail/threads/{id}/archive", post(mail::archive))
        .route("/mail/threads/{id}/summarize", post(mail::summarize))
        .route("/mail/threads/{id}/draft", post(mail::draft))
        .route("/mail/send", post(mail::send))
        .route("/mail/refresh", post(mail::refresh))
        .route("/mail/folders", post(mail::create_folder))
        .route(
            "/mail/folders/{id}",
            axum::routing::patch(mail::update_folder).delete(mail::delete_folder),
        )
        .route("/mail/threads/{id}/folders", post(mail::set_thread_folder))
        .route(
            "/mail/jev",
            axum::routing::put(mail::jev_connect).delete(mail::jev_disconnect),
        )
        .route(
            "/mail/messages/{id}/attachments/{index}",
            get(mail::attachment),
        )
        .route("/mail/messages/{id}/content", get(mail::content))
        .route("/mail/messages/{id}/images", post(mail::images))
        .route("/web/login-link", post(web::login_link))
        .route("/web/logout", post(web::logout))
        .route("/remote", get(remote::status))
        .route(
            "/remote/pairing",
            post(remote::offer).delete(remote::cancel_offers),
        )
        .route("/remote/relay", axum::routing::put(remote::set_relay))
        .route(
            "/remote/devices/{id}",
            axum::routing::patch(remote::rename).delete(remote::remove),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
        // Outside the token check: a phone has no token yet when it pairs. The handler
        // only answers requests that came in over iroh.
        .route("/remote/pair", post(remote::pair));

    Router::new()
        .route("/health", get(health))
        .route("/login", get(web::login))
        .nest(API_PREFIX, authed)
        .fallback(web::static_files)
        .with_state(state)
}

async fn health() -> Json<Health> {
    Json(Health {
        version: VERSION.to_owned(),
        build: crate::BUILD.map(str::to_owned),
    })
}

async fn status(State(state): State<Arc<AppState>>) -> Json<Status> {
    Json(Status {
        version: VERSION.to_owned(),
        pid: std::process::id(),
        uptime_secs: state.started.elapsed().as_secs(),
        data_dir: state.paths.data_dir.clone(),
        key_storage: state.key_storage,
    })
}

/// How a request authenticated, available to handlers as an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// The discovery-file token: the desktop app's Rust side, the CLI.
    Bearer,
    /// A browser session cookie.
    Session,
    /// A paired phone, over iroh (`remote`).
    Device(uuid::Uuid),
}

/// Accepts the bearer token, or a browser session cookie. Cookie requests must also
/// come from our own origin: the Host check defeats DNS rebinding, and the Origin check
/// on anything that changes state (and on the event socket) defeats cross-site requests.
/// Requests from phones (over iroh) accept only a phone's own token, issued to the very
/// key the connection proved.
async fn require_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if let Some(peer) = req.extensions().get::<crate::remote::Peer>() {
        let Some(token) = bearer else {
            return StatusCode::UNAUTHORIZED.into_response();
        };
        return match crate::remote::store::authenticate(&state.db, token, &peer.endpoint_id).await {
            Ok(Some(device)) => {
                req.extensions_mut().insert(Auth::Device(device));
                next.run(req).await
            }
            Ok(None) => StatusCode::UNAUTHORIZED.into_response(),
            Err(e) => error::AppError::from(e).into_response(),
        };
    }
    if let Some(token) = bearer {
        if !constant_time_eq(token.as_bytes(), state.token.as_bytes()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        req.extensions_mut().insert(Auth::Bearer);
        return next.run(req).await;
    }

    let Some(session) = web::session_token(&req).map(str::to_owned) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !crate::web::allowed_host(host, state.port) {
        return forbidden("This address isn't allowed to use the web interface.");
    }
    let is_upgrade = req.headers().contains_key(header::UPGRADE);
    if req.method() != Method::GET && req.method() != Method::HEAD || is_upgrade {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        if origin != Some(format!("http://{host}").as_str()) {
            return forbidden("Cross-site requests aren't allowed.");
        }
    }
    match crate::web::session_valid(&state.db, &session).await {
        Ok(true) => {}
        Ok(false) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(e) => return error::AppError::from(e).into_response(),
    }
    req.extensions_mut().insert(Auth::Session);
    next.run(req).await
}

fn forbidden(message: &str) -> Response {
    error::AppError::new(StatusCode::FORBIDDEN, "forbidden", message).into_response()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests;
