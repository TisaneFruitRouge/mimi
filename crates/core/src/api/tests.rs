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

/// Serves the real router on a loopback port, for tests that need a live socket.
async fn serve() -> (u16, Arc<AppState>) {
    let state = Arc::new(AppState::for_tests(TOKEN));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = router(state.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (port, state)
}

#[tokio::test]
async fn events_require_token_and_deliver_changes() {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let (port, _state) = serve().await;
    let url = format!("ws://127.0.0.1:{port}/v1/events");

    assert!(
        tokio_tungstenite::connect_async(url.as_str())
            .await
            .is_err()
    );

    let mut req = url.as_str().into_client_request().unwrap();
    req.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {TOKEN}").parse().unwrap(),
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();

    let settings = hearth_protocol::Settings {
        assistant_name: "Ember".into(),
        ..Default::default()
    };
    let res = reqwest::Client::new()
        .put(format!("http://127.0.0.1:{port}/v1/settings"))
        .bearer_auth(TOKEN)
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());

    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let event: hearth_protocol::Event = serde_json::from_str(frame.to_text().unwrap()).unwrap();
    assert_eq!(event, hearth_protocol::Event::SettingsChanged { settings });
}
