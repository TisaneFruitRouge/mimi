//! The hearth daemon: owns the agent loop, memory, models and tools, and serves them to
//! clients over an authenticated local API.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use hearth_protocol::{API_PREFIX, Health, Paths, Status};

pub mod daemon;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct AppState {
    pub paths: Paths,
    pub token: String,
    pub started: Instant,
}

pub fn router(state: Arc<AppState>) -> Router {
    let authed = Router::new()
        .route("/status", get(status))
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
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    fn app() -> Router {
        router(Arc::new(AppState {
            paths: Paths {
                data_dir: "/nonexistent".into(),
                config_dir: "/nonexistent".into(),
            },
            token: "secret".into(),
            started: Instant::now(),
        }))
    }

    async fn get_status(app: Router, auth: Option<&str>) -> StatusCode {
        let mut req = Request::get("/v1/status");
        if let Some(auth) = auth {
            req = req.header(header::AUTHORIZATION, auth);
        }
        app.oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn health_is_public() {
        let res = app()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn status_requires_token() {
        assert_eq!(get_status(app(), None).await, StatusCode::UNAUTHORIZED);
        assert_eq!(
            get_status(app(), Some("Bearer wrong!")).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            get_status(app(), Some("Bearer secret")).await,
            StatusCode::OK
        );
    }
}
