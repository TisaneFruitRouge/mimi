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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = Arc::new(AppState::for_tests_on(TOKEN, port));
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

/// A fake OpenAI-compatible server that streams `chunks` as content deltas, waiting
/// `delay` before each.
async fn mock_llm(chunks: Vec<&'static str>, delay: std::time::Duration) -> u16 {
    use axum::routing::{get, post};

    let app = Router::new()
        .route(
            "/v1/models",
            get(|| async { Json(serde_json::json!({"data": [{"id": "mock-model"}]})) }),
        )
        .route(
            "/v1/chat/completions",
            post(move || {
                let chunks = chunks.clone();
                async move {
                    let frames = futures::stream::iter(chunks)
                        .then(move |c| async move {
                            tokio::time::sleep(delay).await;
                            let frame = serde_json::json!({"choices": [{"delta": {"content": c}}]});
                            Ok::<_, std::convert::Infallible>(format!("data: {frame}\n\n"))
                        })
                        .chain(futures::stream::once(async {
                            Ok("data: [DONE]\n\n".to_owned())
                        }));
                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(Body::from_stream(frames))
                        .unwrap()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

use futures::StreamExt;

struct Harness {
    http: reqwest::Client,
    base: String,
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Harness {
    async fn new() -> Self {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let (port, _) = serve().await;
        let mut req = format!("ws://127.0.0.1:{port}/v1/events")
            .into_client_request()
            .unwrap();
        req.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {TOKEN}").parse().unwrap(),
        );
        let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        Self {
            http: reqwest::Client::new(),
            base: format!("http://127.0.0.1:{port}/v1"),
            ws,
        }
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let res = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(serde_json::Value::Null))
    }

    /// Adds the mock as a provider and makes its model the default.
    async fn use_mock(&self, llm_port: u16) {
        let (status, provider) = self
            .call(
                reqwest::Method::POST,
                "/providers",
                serde_json::json!({"name": "Mock", "base_url": format!("http://127.0.0.1:{llm_port}/v1")}),
            )
            .await;
        assert_eq!(status, 200, "{provider}");
        assert_eq!(provider["locality"], "device");
        let (status, _) = self
            .call(
                reqwest::Method::PUT,
                "/settings",
                serde_json::json!({
                    "assistant_name": "Hearth",
                    "default_model": {"provider_id": provider["id"], "model": "mock-model"}
                }),
            )
            .await;
        assert_eq!(status, 200);
    }

    /// Waits for the assistant message to leave `streaming`, collecting the deltas seen.
    async fn wait_for_reply(&mut self, message_id: &str) -> (hearth_protocol::Message, String) {
        let mut deltas = String::new();
        loop {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(10), self.ws.next())
                .await
                .expect("timed out waiting for the reply")
                .unwrap()
                .unwrap();
            let event: hearth_protocol::Event =
                serde_json::from_str(frame.to_text().unwrap()).unwrap();
            match event {
                hearth_protocol::Event::MessageDelta {
                    message_id: id,
                    content,
                    ..
                } if id.to_string() == message_id => deltas += &content,
                hearth_protocol::Event::MessageUpdated { message }
                    if message.id.to_string() == message_id
                        && message.status != hearth_protocol::MessageStatus::Streaming =>
                {
                    return (message, deltas);
                }
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn chat_streams_reply_and_saves_it() {
    let llm = mock_llm(
        vec!["<think>User greets", " me.</think>", "Hello", " there!"],
        std::time::Duration::from_millis(5),
    )
    .await;
    let mut h = Harness::new().await;
    h.use_mock(llm).await;

    let (_, conv) = h
        .call(
            reqwest::Method::POST,
            "/conversations",
            serde_json::json!({}),
        )
        .await;
    let conv_id = conv["id"].as_str().unwrap().to_owned();
    let (status, sent) = h
        .call(
            reqwest::Method::POST,
            &format!("/conversations/{conv_id}/messages"),
            serde_json::json!({"content": "  Hi, who are you?  "}),
        )
        .await;
    assert_eq!(status, 200, "{sent}");
    assert_eq!(sent["assistant_message"]["status"], "streaming");
    assert_eq!(sent["assistant_message"]["locality"], "device");

    let assistant_id = sent["assistant_message"]["id"].as_str().unwrap().to_owned();
    let (reply, deltas) = h.wait_for_reply(&assistant_id).await;
    assert_eq!(reply.status, hearth_protocol::MessageStatus::Complete);
    assert_eq!(reply.content, "Hello there!");
    assert_eq!(reply.reasoning, "User greets me.");
    assert_eq!(deltas, "Hello there!");

    let (_, detail) = h
        .call(
            reqwest::Method::GET,
            &format!("/conversations/{conv_id}"),
            serde_json::Value::Null,
        )
        .await;
    let detail: hearth_protocol::ConversationDetail = parse(detail);
    assert_eq!(detail.conversation.title, "Hi, who are you?");
    let contents: Vec<_> = detail.messages.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(contents, ["Hi, who are you?", "Hello there!"]);
}

#[tokio::test]
async fn chat_reply_can_be_cancelled() {
    let llm = mock_llm(vec!["word "; 200], std::time::Duration::from_millis(20)).await;
    let mut h = Harness::new().await;
    h.use_mock(llm).await;
    let (_, conv) = h
        .call(
            reqwest::Method::POST,
            "/conversations",
            serde_json::json!({}),
        )
        .await;
    let conv_id = conv["id"].as_str().unwrap().to_owned();
    let (_, sent) = h
        .call(
            reqwest::Method::POST,
            &format!("/conversations/{conv_id}/messages"),
            serde_json::json!({"content": "Talk forever"}),
        )
        .await;

    // A second message while replying is refused.
    let (status, busy) = h
        .call(
            reqwest::Method::POST,
            &format!("/conversations/{conv_id}/messages"),
            serde_json::json!({"content": "And another thing"}),
        )
        .await;
    assert_eq!((status, busy["code"].as_str()), (409, Some("busy")));

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    h.call(
        reqwest::Method::POST,
        &format!("/conversations/{conv_id}/cancel"),
        serde_json::Value::Null,
    )
    .await;
    let assistant_id = sent["assistant_message"]["id"].as_str().unwrap().to_owned();
    let (reply, _) = h.wait_for_reply(&assistant_id).await;
    assert_eq!(reply.status, hearth_protocol::MessageStatus::Cancelled);
    assert!(reply.content.starts_with("word"));
    assert!(reply.content.len() < 200 * 5);
}

#[tokio::test]
async fn sending_without_a_model_explains_why() {
    let h = Harness::new().await;
    let (_, conv) = h
        .call(
            reqwest::Method::POST,
            "/conversations",
            serde_json::json!({}),
        )
        .await;
    let (status, err) = h
        .call(
            reqwest::Method::POST,
            &format!("/conversations/{}/messages", conv["id"].as_str().unwrap()),
            serde_json::json!({"content": "Hello?"}),
        )
        .await;
    assert_eq!((status, err["code"].as_str()), (400, Some("no_model")));
}

// Web interface: login links, cookie sessions, and the Host/Origin rules.

mod web {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use tower::ServiceExt;

    use super::*;

    const HOST: &str = "127.0.0.1:7437";
    const ORIGIN: &str = "http://127.0.0.1:7437";

    async fn send(app: &Router, req: Request<Body>) -> axum::response::Response {
        app.clone().oneshot(req).await.unwrap()
    }

    /// Logs a browser in through a fresh login link; returns the session cookie pair.
    async fn login(app: &Router) -> String {
        let (status, link) = call(app, Method::POST, "/v1/web/login-link", None).await;
        assert_eq!(status, StatusCode::OK);
        let url = link["url"].as_str().unwrap();
        let path = url.strip_prefix("http://127.0.0.1:7437").unwrap();
        let res = send(
            app,
            Request::get(path)
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        let cookie = res.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));

        // The same link doesn't work twice.
        let again = send(
            app,
            Request::get(path)
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(again.status(), StatusCode::UNAUTHORIZED);

        cookie.split(';').next().unwrap().to_owned()
    }

    fn with_cookie(method: Method, path: &str, cookie: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, cookie)
    }

    #[tokio::test]
    async fn cookie_session_authenticates_same_origin_requests() {
        let app = app();
        let cookie = login(&app).await;

        let res = send(
            &app,
            with_cookie(Method::GET, "/v1/status", &cookie)
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);

        let res = send(
            &app,
            with_cookie(Method::PUT, "/v1/settings", &cookie)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"assistant_name":"Ember","default_model":null}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn expired_or_unknown_codes_are_refused() {
        let state = Arc::new(AppState::for_tests(TOKEN));
        state.login_codes.insert_expired("stale");
        let app = router(state);
        for code in ["stale", "made-up"] {
            let res = send(
                &app,
                Request::get(format!("/login?code={code}"))
                    .header(header::HOST, HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
            assert!(!res.headers().contains_key(header::SET_COOKIE));
        }
    }

    #[tokio::test]
    async fn cookie_requests_from_foreign_hosts_are_rejected() {
        let app = app();
        let cookie = login(&app).await;
        for host in ["evil.example:7437", "127.0.0.1:8080"] {
            let res = send(
                &app,
                with_cookie(Method::GET, "/v1/status", &cookie)
                    .header(header::HOST, host)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "host {host}");
        }
    }

    #[tokio::test]
    async fn cross_origin_writes_and_sockets_are_rejected() {
        let app = app();
        let cookie = login(&app).await;
        for origin in [
            Some("http://evil.example"),
            Some("http://localhost:7437"),
            None,
        ] {
            let mut req = with_cookie(Method::POST, "/v1/conversations", &cookie)
                .header(header::HOST, HOST)
                .header(header::CONTENT_TYPE, "application/json");
            if let Some(origin) = origin {
                req = req.header(header::ORIGIN, origin);
            }
            let res = send(&app, req.body(Body::from("{}")).unwrap()).await;
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "origin {origin:?}");
        }
        let res = send(
            &app,
            with_cookie(Method::GET, "/v1/events", &cookie)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, "http://evil.example")
                .header(header::CONNECTION, "upgrade")
                .header(header::UPGRADE, "websocket")
                .header(header::SEC_WEBSOCKET_VERSION, "13")
                .header(header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn sessions_cannot_mint_links_and_logout_ends_them() {
        let app = app();
        let cookie = login(&app).await;
        let authed = |method: Method, path: &str| {
            with_cookie(method, path, &cookie)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap()
        };
        let res = send(&app, authed(Method::POST, "/v1/web/login-link")).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let res = send(&app, authed(Method::POST, "/v1/web/logout")).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        let res = send(&app, authed(Method::GET, "/v1/status")).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bearer_clients_ignore_host_and_origin() {
        let res = send(
            &app(),
            Request::get("/v1/status")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::HOST, "anything:1")
                .header(header::ORIGIN, "http://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serves_the_frontend_but_not_unknown_api_routes() {
        let app = app();
        let res = send(&app, Request::get("/").body(Body::empty()).unwrap()).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        // Client-side routes fall back to the app.
        let res = send(
            &app,
            Request::get("/settings/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);

        let res = send(&app, Request::get("/v1/nope").body(Body::empty()).unwrap()).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
