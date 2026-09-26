//! HTTP routes. Everything except `/health` sits under `/v1` behind the bearer token.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hearth_protocol::{API_PREFIX, Health, Status};

use crate::{AppState, VERSION};

mod conversations;
pub mod error;
mod events;
mod hardware;
mod providers;
mod settings;

pub fn router(state: Arc<AppState>) -> Router {
    let authed = Router::new()
        .route("/status", get(status))
        .route("/settings", get(settings::get).put(settings::put))
        .route("/events", get(events::subscribe))
        .route("/providers", get(providers::list).post(providers::create))
        .route("/providers/presets", get(providers::presets))
        .route("/providers/probe", post(providers::probe))
        .route(
            "/providers/{id}",
            axum::routing::patch(providers::update).delete(providers::delete),
        )
        .route("/providers/{id}/models", get(providers::models))
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
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new()
        .route("/health", get(health))
        .nest(API_PREFIX, authed)
        .with_state(state)
}

async fn health() -> Json<Health> {
    Json(Health {
        version: VERSION.to_owned(),
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

async fn require_token(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), state.token.as_bytes()) => {
            next.run(req).await
        }
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests;
