use axum::body::Body;
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use super::*;
use crate::AppState;

const TOKEN: &str = "secret";

fn app() -> Router {
    router(Arc::new(AppState::for_tests(TOKEN)))
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    let body = match body {
        Some(json) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

pub(super) fn parse<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap()
}

async fn status_with(auth: Option<&str>) -> StatusCode {
    let mut req = Request::get("/v1/status");
    if let Some(auth) = auth {
        req = req.header(header::AUTHORIZATION, auth);
    }
    app()
        .oneshot(req.body(Body::empty()).unwrap())
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
async fn api_requires_token() {
    assert_eq!(status_with(None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        status_with(Some("Bearer wrong!")).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(status_with(Some("Bearer secret")).await, StatusCode::OK);
}

#[tokio::test]
async fn settings_round_trip_and_validate() {
    let app = app();
    let (status, body) = call(&app, Method::GET, "/v1/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    let mut settings: hearth_protocol::Settings = parse(body);
    assert_eq!(settings.assistant_name, "Hearth");

    settings.assistant_name = "  Ember ".into();
    let (status, _) = call(
        &app,
        Method::PUT,
        "/v1/settings",
        Some(serde_json::to_value(&settings).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = call(&app, Method::GET, "/v1/settings", None).await;
    assert_eq!(
        parse::<hearth_protocol::Settings>(body).assistant_name,
        "Ember"
    );

    settings.assistant_name = "   ".into();
    let (status, body) = call(
        &app,
        Method::PUT,
        "/v1/settings",
        Some(serde_json::to_value(&settings).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "bad_request");
}
