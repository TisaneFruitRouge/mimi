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
    let mut settings: mimi_protocol::Settings = parse(body);
    assert_eq!(settings.assistant_name, "Mimi");

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
        parse::<mimi_protocol::Settings>(body).assistant_name,
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

    let settings = mimi_protocol::Settings {
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
    let event: mimi_protocol::Event = serde_json::from_str(frame.to_text().unwrap()).unwrap();
    assert_eq!(event, mimi_protocol::Event::SettingsChanged { settings });
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
    state: Arc<AppState>,
    http: reqwest::Client,
    base: String,
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Harness {
    async fn new() -> Self {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let (port, state) = serve().await;
        let mut req = format!("ws://127.0.0.1:{port}/v1/events")
            .into_client_request()
            .unwrap();
        req.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {TOKEN}").parse().unwrap(),
        );
        let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        Self {
            state,
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
                    "assistant_name": "Mimi",
                    "default_model": {"provider_id": provider["id"], "model": "mock-model"}
                }),
            )
            .await;
        assert_eq!(status, 200);
    }

    /// Waits for the assistant message to leave `streaming`, collecting the deltas seen.
    async fn wait_for_reply(&mut self, message_id: &str) -> (mimi_protocol::Message, String) {
        let mut deltas = String::new();
        loop {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(10), self.ws.next())
                .await
                .expect("timed out waiting for the reply")
                .unwrap()
                .unwrap();
            let event: mimi_protocol::Event =
                serde_json::from_str(frame.to_text().unwrap()).unwrap();
            match event {
                mimi_protocol::Event::MessageDelta {
                    message_id: id,
                    content,
                    ..
                } if id.to_string() == message_id => deltas += &content,
                mimi_protocol::Event::MessageUpdated { message }
                    if message.id.to_string() == message_id
                        && message.status != mimi_protocol::MessageStatus::Streaming =>
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
    assert_eq!(reply.status, mimi_protocol::MessageStatus::Complete);
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
    let detail: mimi_protocol::ConversationDetail = parse(detail);
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
    assert_eq!(reply.status, mimi_protocol::MessageStatus::Cancelled);
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

// --- Tool use -----------------------------------------------------------------------

mod tool_use {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::future::BoxFuture;
    use mimi_protocol::{ActionStatus, Event, MessageStatus};
    use serde_json::{Value, json};

    use super::*;
    use crate::tools::{Tool, ToolContext, ToolSource};

    /// What the scripted model answers to one request.
    pub(super) enum Reply {
        Text(&'static str),
        Call(&'static str, Value),
        /// Some text, then a tool call, in one response.
        SayThenCall(&'static str, &'static str, Value),
        Status(u16, &'static str),
    }

    pub(super) struct ScriptedLlm {
        port: u16,
        requests: Arc<Mutex<Vec<Value>>>,
    }

    impl ScriptedLlm {
        pub(super) fn requests(&self) -> Vec<Value> {
            self.requests.lock().unwrap().clone()
        }
    }

    /// A fake OpenAI-compatible server whose reply to each request is decided by
    /// `script(request_body, how_many_requests_before)`. Records every request.
    pub(super) async fn scripted_llm(
        script: impl Fn(&Value, usize) -> Reply + Send + Sync + 'static,
    ) -> ScriptedLlm {
        use axum::routing::{get, post};
        let requests: Arc<Mutex<Vec<Value>>> = Default::default();
        let script = Arc::new(script);
        let recorded = requests.clone();
        let app = Router::new()
            .route(
                "/v1/models",
                get(|| async { Json(json!({"data": [{"id": "mock-model"}]})) }),
            )
            .route(
                "/v1/chat/completions",
                post(move |Json(body): Json<Value>| {
                    let script = script.clone();
                    let recorded = recorded.clone();
                    async move {
                        let n = {
                            let mut r = recorded.lock().unwrap();
                            r.push(body.clone());
                            r.len() - 1
                        };
                        let frames: Vec<String> = match script(&body, n) {
                            Reply::Status(code, msg) => {
                                return Response::builder()
                                    .status(code)
                                    .header(header::CONTENT_TYPE, "application/json")
                                    .body(Body::from(json!({"error": {"message": msg}}).to_string()))
                                    .unwrap();
                            }
                            Reply::Text(t) => vec![json!({"choices": [{"delta": {"content": t}}]}).to_string()],
                            Reply::Call(name, args) | Reply::SayThenCall(_, name, args) => {
                                // Arguments arrive in fragments, as real servers send them.
                                let args = args.to_string();
                                let (a, b) = args.split_at(args.len() / 2);
                                let say = match script(&body, n) {
                                    Reply::SayThenCall(t, ..) => Some(json!({"choices": [{"delta": {"content": t}}]}).to_string()),
                                    _ => None,
                                };
                                say.into_iter().chain([
                                    json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": format!("call_{n}"), "type": "function", "function": {"name": name, "arguments": a}}]}}]}).to_string(),
                                    json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": b}}]}}]}).to_string(),
                                ]).collect()
                            }
                        };
                        let body: String = frames
                            .into_iter()
                            .map(|f| format!("data: {f}\n\n"))
                            .chain(["data: [DONE]\n\n".to_owned()])
                            .collect();
                        Response::builder()
                            .header(header::CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(body))
                            .unwrap()
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        ScriptedLlm { port, requests }
    }

    struct TestTool {
        name: &'static str,
        approval: bool,
        runs: Arc<AtomicUsize>,
    }

    impl Tool for TestTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "A test tool."
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        fn needs_approval(&self, _: &Value) -> bool {
            self.approval
        }
        fn summary(&self, args: &Value) -> String {
            format!("{} with {args}", self.name)
        }
        fn result_label(&self, _: &Value, _: &Value) -> String {
            format!("ran {}", self.name)
        }
        fn run<'a>(
            &'a self,
            _: &'a ToolContext,
            args: Value,
        ) -> BoxFuture<'a, Result<Value, String>> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({"answer": 42, "args": args})) })
        }
    }

    struct Source(Vec<Arc<dyn Tool>>);

    impl ToolSource for Source {
        fn tools<'a>(&'a self, _: &'a AppState) -> BoxFuture<'a, Vec<Arc<dyn Tool>>> {
            let tools = self.0.clone();
            Box::pin(async move { tools })
        }
    }

    /// Sets up a harness whose model is `llm`, with a read tool `lookup` and an
    /// approval tool `send_note`. Returns run counters for both.
    pub(super) async fn setup(llm: &ScriptedLlm) -> (Harness, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let h = Harness::new().await;
        h.use_mock(llm.port).await;
        let (reads, writes) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        h.state.tool_sources.add(Arc::new(Source(vec![
            Arc::new(TestTool {
                name: "lookup",
                approval: false,
                runs: reads.clone(),
            }),
            Arc::new(TestTool {
                name: "send_note",
                approval: true,
                runs: writes.clone(),
            }),
        ])));
        (h, reads, writes)
    }

    impl Harness {
        /// Starts a conversation with `content`; returns (conversation id, reply id).
        async fn start(&self, content: &str) -> (String, String) {
            let (_, conv) = self
                .call(reqwest::Method::POST, "/conversations", json!({}))
                .await;
            let conv_id = conv["id"].as_str().unwrap().to_owned();
            let (status, sent) = self
                .call(
                    reqwest::Method::POST,
                    &format!("/conversations/{conv_id}/messages"),
                    json!({"content": content}),
                )
                .await;
            assert_eq!(status, 200, "{sent}");
            (
                conv_id,
                sent["assistant_message"]["id"].as_str().unwrap().to_owned(),
            )
        }

        /// Waits until the reply shows an approval card; returns the action id.
        async fn wait_for_pending(&mut self, message_id: &str) -> String {
            loop {
                let frame =
                    tokio::time::timeout(std::time::Duration::from_secs(10), self.ws.next())
                        .await
                        .expect("timed out waiting for an approval card")
                        .unwrap()
                        .unwrap();
                if let Event::MessageUpdated { message } =
                    serde_json::from_str(frame.to_text().unwrap()).unwrap()
                    && message.id.to_string() == message_id
                    && let Some(a) = message
                        .actions
                        .iter()
                        .find(|a| a.status == ActionStatus::PendingApproval)
                {
                    return a.id.to_string();
                }
            }
        }
    }

    fn has_tools(req: &Value) -> bool {
        req.get("tools")
            .is_some_and(|t| t.as_array().is_some_and(|a| !a.is_empty()))
    }

    fn tool_messages(req: &Value) -> Vec<Value> {
        req["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn read_tool_runs_and_its_result_goes_back_to_the_model() {
        let llm = scripted_llm(|_, n| match n {
            0 => Reply::Call("lookup", json!({"q": "cats"})),
            _ => Reply::Text("The answer is 42."),
        })
        .await;
        let (mut h, reads, _) = setup(&llm).await;
        let (_, id) = h.start("What's the answer?").await;
        let (reply, _) = h.wait_for_reply(&id).await;

        assert_eq!(reply.status, MessageStatus::Complete);
        assert_eq!(reply.content, "The answer is 42.");
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        let [action] = &reply.actions[..] else {
            panic!("{:?}", reply.actions)
        };
        assert_eq!(action.status, ActionStatus::Done);
        assert!(!action.requires_approval);
        assert_eq!(action.arguments, json!({"q": "cats"}));
        assert_eq!(action.result.as_deref(), Some("ran lookup"));

        let requests = llm.requests();
        assert!(has_tools(&requests[0]));
        let tools = tool_messages(&requests[1]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["tool_call_id"], "call_0");
        assert!(tools[0]["content"].as_str().unwrap().contains("42"));
        assert!(
            requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["tool_calls"][0]["function"]["name"] == "lookup")
        );

        // The next message replays the tool call and result from history.
        let (_, conv) = h
            .call(reqwest::Method::GET, "/conversations", json!(null))
            .await;
        let conv_id = conv[0]["id"].as_str().unwrap().to_owned();
        let (_, sent) = h
            .call(
                reqwest::Method::POST,
                &format!("/conversations/{conv_id}/messages"),
                json!({"content": "Thanks"}),
            )
            .await;
        h.wait_for_reply(sent["assistant_message"]["id"].as_str().unwrap())
            .await;
        let replayed = tool_messages(&llm.requests()[2]);
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0]["tool_call_id"], "call_0");
    }

    #[tokio::test]
    async fn approval_pauses_until_approved_with_edits() {
        let llm = scripted_llm(|_, n| match n {
            0 => Reply::SayThenCall("Sure. ", "send_note", json!({"to": "Sam", "text": "hi"})),
            _ => Reply::Text("Sent."),
        })
        .await;
        let (mut h, _, writes) = setup(&llm).await;
        let (_, id) = h.start("Tell Sam hi").await;
        let action = h.wait_for_pending(&id).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            writes.load(Ordering::SeqCst),
            0,
            "must not run before approval"
        );
        assert_eq!(llm.requests().len(), 1, "the turn waits");

        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/actions/{action}/approve"),
                json!({"arguments": {"to": "Sam", "text": "hello!"}}),
            )
            .await;
        assert_eq!(status, 200);
        let (reply, _) = h.wait_for_reply(&id).await;
        // Text from both rounds, a paragraph apart, with the action placed between.
        assert_eq!(reply.content, "Sure. \n\nSent.");
        assert_eq!(reply.actions[0].content_offset, 6);
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(reply.actions[0].status, ActionStatus::Done);
        assert_eq!(reply.actions[0].arguments["text"], "hello!");

        // A decided action can't be decided again.
        let (status, body) = h
            .call(
                reqwest::Method::POST,
                &format!("/actions/{action}/approve"),
                json!({}),
            )
            .await;
        assert_eq!((status, body["code"].as_str()), (409, Some("not_waiting")));
    }

    #[tokio::test]
    async fn rejected_action_does_not_run_and_the_model_is_told() {
        let llm = scripted_llm(|_, n| match n {
            0 => Reply::Call("send_note", json!({"to": "Sam"})),
            _ => Reply::Text("Okay, I won't."),
        })
        .await;
        let (mut h, _, writes) = setup(&llm).await;
        let (_, id) = h.start("Tell Sam hi").await;
        let action = h.wait_for_pending(&id).await;
        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/actions/{action}/reject"),
                json!(null),
            )
            .await;
        assert_eq!(status, 200);
        let (reply, _) = h.wait_for_reply(&id).await;
        assert_eq!(reply.status, MessageStatus::Complete);
        assert_eq!(reply.actions[0].status, ActionStatus::Rejected);
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        let tools = tool_messages(&llm.requests()[1]);
        assert!(tools[0]["content"].as_str().unwrap().contains("declined"));
    }

    #[tokio::test]
    async fn models_without_tool_support_get_plain_chat() {
        let llm = scripted_llm(|req, _| {
            if has_tools(req) {
                Reply::Status(
                    400,
                    "registry.ollama.ai/library/tiny does not support tools",
                )
            } else {
                Reply::Text("Plain answer.")
            }
        })
        .await;
        let (mut h, _, _) = setup(&llm).await;
        let (_, id) = h.start("Hello").await;
        let (reply, _) = h.wait_for_reply(&id).await;
        assert_eq!(reply.status, MessageStatus::Complete);
        assert_eq!(reply.content, "Plain answer.");
        let requests = llm.requests();
        assert_eq!(requests.len(), 2);
        assert!(!has_tools(&requests[1]));
    }

    #[tokio::test]
    async fn tool_rounds_are_capped() {
        let llm = scripted_llm(|req, _| {
            if has_tools(req) {
                Reply::Call("lookup", json!({}))
            } else {
                Reply::Text("Enough looking.")
            }
        })
        .await;
        let (mut h, reads, _) = setup(&llm).await;
        let (_, id) = h.start("Look forever").await;
        let (reply, _) = h.wait_for_reply(&id).await;
        let max = crate::chat::MAX_TOOL_ROUNDS as usize;
        assert_eq!(reply.content, "Enough looking.");
        assert_eq!(reply.actions.len(), max);
        assert_eq!(reads.load(Ordering::SeqCst), max);
        let requests = llm.requests();
        assert_eq!(requests.len(), max + 1);
        assert!(!has_tools(requests.last().unwrap()));
    }

    #[tokio::test]
    async fn cancelling_while_awaiting_approval_stops_cleanly() {
        let llm = scripted_llm(|_, _| Reply::Call("send_note", json!({"to": "Sam"}))).await;
        let (mut h, _, writes) = setup(&llm).await;
        let (conv, id) = h.start("Tell Sam hi").await;
        let action = h.wait_for_pending(&id).await;
        h.call(
            reqwest::Method::POST,
            &format!("/conversations/{conv}/cancel"),
            json!(null),
        )
        .await;
        let (reply, _) = h.wait_for_reply(&id).await;
        assert_eq!(reply.status, MessageStatus::Cancelled);
        assert_eq!(reply.actions[0].status, ActionStatus::Failed);
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/actions/{action}/approve"),
                json!({}),
            )
            .await;
        assert_eq!(status, 409);
    }

    #[tokio::test]
    async fn unknown_tools_and_bad_arguments_fail_without_running() {
        let llm = scripted_llm(|_, n| match n {
            0 => Reply::Call("does_not_exist", json!({})),
            _ => Reply::Text("Sorry."),
        })
        .await;
        let (mut h, _, _) = setup(&llm).await;
        let (_, id) = h.start("Do something").await;
        let (reply, _) = h.wait_for_reply(&id).await;
        assert_eq!(reply.actions[0].status, ActionStatus::Failed);
        assert!(
            tool_messages(&llm.requests()[1])[0]["content"]
                .as_str()
                .unwrap()
                .contains("no tool")
        );
    }

    #[tokio::test]
    async fn restart_fails_actions_that_were_in_flight() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let conv = crate::chat::new_conversation(None);
        crate::chat::store::upsert_conversation(&db, conv.clone())
            .await
            .unwrap();
        let pending = mimi_protocol::Action {
            id: uuid::Uuid::now_v7(),
            tool: "send_note".into(),
            summary: "Send".into(),
            arguments: json!({}),
            requires_approval: true,
            status: ActionStatus::PendingApproval,
            result: None,
            error: None,
            output: None,
            call_id: "call_0".into(),
            round: 0,
            content_offset: 0,
        };
        let message = mimi_protocol::Message {
            id: uuid::Uuid::now_v7(),
            conversation_id: conv.id,
            role: mimi_protocol::MessageRole::Assistant,
            content: String::new(),
            reasoning: String::new(),
            status: MessageStatus::Streaming,
            model: None,
            locality: None,
            error: None,
            created_at: 1,
            actions: vec![pending],
            mentions: Vec::new(),
        };
        crate::chat::store::upsert_message(&db, message)
            .await
            .unwrap();
        assert_eq!(crate::chat::store::mark_interrupted(&db).await.unwrap(), 1);
        let saved = crate::chat::store::messages(&db, conv.id).await.unwrap();
        assert_eq!(saved[0].status, MessageStatus::Interrupted);
        assert_eq!(saved[0].actions[0].status, ActionStatus::Failed);
    }

    #[tokio::test]
    async fn telegram_owner_approves_actions_with_buttons() {
        use super::fake_telegram::FakeTelegram;

        let llm = scripted_llm(|_, n| match n {
            0 => Reply::Call("send_note", json!({"to": "Sam"})),
            _ => Reply::Text("Done, I sent it."),
        })
        .await;
        let (h, _reads, writes) = setup(&llm).await;
        let (tg, tg_url) = FakeTelegram::start().await;
        *h.state.connections.telegram_api.lock().unwrap() = tg_url;

        let (_, conn) = h
            .call(
                reqwest::Method::POST,
                "/connections",
                json!({"integration": "telegram", "bot_token": "123:secret"}),
            )
            .await;
        let code = conn["action_url"]
            .as_str()
            .unwrap()
            .rsplit("start=")
            .next()
            .unwrap()
            .to_owned();
        tg.message(42, "Vincent", &format!("/start {code}"));
        tg.message(42, "Vincent", "Please send Sam a note");

        let mut approve = None;
        for _ in 0..200 {
            approve = tg
                .buttons_sent_to(42)
                .into_iter()
                .find(|b| b.starts_with("approve:"));
            if approve.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let approve = approve.expect("an approval button was sent");
        assert_eq!(
            writes.load(Ordering::SeqCst),
            0,
            "nothing runs before approval"
        );

        // A tap from someone else doesn't count.
        tg.tap(99, &approve);
        tg.tap(42, &approve);
        for _ in 0..200 {
            if tg.sent_to(42).iter().any(|m| m == "Done, I sent it.") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert!(
            tg.sent_to(42).iter().any(|m| m == "Done, I sent it."),
            "{:?}",
            tg.sent_to(42)
        );
        assert_eq!(writes.load(Ordering::SeqCst), 1);
    }

    // --- Memory ---------------------------------------------------------------------

    fn system_prompt(req: &Value) -> String {
        req["messages"][0]["content"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    #[tokio::test]
    async fn memories_are_recalled_into_the_prompt_and_writes_can_be_undone() {
        use crate::memory::{PROFILE_PATH, store};
        use mimi_protocol::MemorySource;

        let llm = scripted_llm(|_, n| match n {
            0 => Reply::Call(
                "memory_write",
                json!({"path": "people/sam.md", "facts": ["Sam's birthday is 3 May"]}),
            ),
            _ => Reply::Text("Noted! How about climbing gear?"),
        })
        .await;
        let mut h = Harness::new().await;
        h.use_mock(llm.port).await;
        h.state
            .tool_sources
            .add(Arc::new(crate::memory::tools::MemoryTools));
        let state = h.state.clone();
        let db = &state.db;
        store::put(
            db,
            PROFILE_PATH,
            None,
            "- The user's name is Vincent",
            MemorySource::You,
            None,
        )
        .await
        .unwrap();
        store::put(
            db,
            "people/sam.md",
            None,
            "- Sam is the user's brother\n- Sam loves climbing",
            MemorySource::Learned,
            None,
        )
        .await
        .unwrap();
        store::put(
            db,
            "places/office.md",
            None,
            "- The office is in Oerlikon",
            MemorySource::Learned,
            None,
        )
        .await
        .unwrap();

        let (conv_id, id) = h
            .start("Sam's birthday is on 3 May. What should I get my brother Sam?")
            .await;
        let (reply, _) = h.wait_for_reply(&id).await;
        assert_eq!(reply.status, MessageStatus::Complete);

        // The profile and the relevant note (and only that) went into the prompt.
        let system = system_prompt(&llm.requests()[0]);
        assert!(
            system.contains("<memory>") && system.contains("Vincent"),
            "{system}"
        );
        assert!(system.contains("Sam loves climbing"), "{system}");
        assert!(!system.contains("Oerlikon"), "{system}");

        let [action] = &reply.actions[..] else {
            panic!("{:?}", reply.actions)
        };
        assert_eq!(action.tool, "memory_write");
        assert!(!action.requires_approval);
        assert_eq!(
            action.result.as_deref(),
            Some("remembered that Sam's birthday is 3 May")
        );
        let revision = action.output.as_ref().unwrap()["revision"]
            .as_i64()
            .unwrap();
        let note = store::get(db, "people/sam.md").await.unwrap().unwrap();
        assert!(note.body.contains("3 May"));

        // Undo from the chat line: the fact is gone and the line says so.
        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/memory/undo/{revision}"),
                json!(null),
            )
            .await;
        assert_eq!(status, 200);
        let note = store::get(db, "people/sam.md").await.unwrap().unwrap();
        assert!(!note.body.contains("3 May"));
        let (_, detail) = h
            .call(
                reqwest::Method::GET,
                &format!("/conversations/{conv_id}"),
                json!(null),
            )
            .await;
        assert_eq!(
            detail["messages"][1]["actions"][0]["output"]["undone"],
            true
        );
        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/memory/undo/{revision}"),
                json!(null),
            )
            .await;
        assert_eq!(status, 409);

        // The Memory screen sees it all, and can't be used to store secrets.
        let (_, overview) = h.call(reqwest::Method::GET, "/memory", json!(null)).await;
        assert!(overview["profile"].as_str().unwrap().contains("Vincent"));
        assert_eq!(overview["notes"].as_array().unwrap().len(), 2);
        assert_eq!(overview["learning"], true);
        let (status, _) = h
            .call(
                reqwest::Method::PUT,
                "/memory/note?path=notes/bank.md",
                json!({"body": "- The bank password is hunter2"}),
            )
            .await;
        assert_eq!(status, 400);
        let (status, _) = h
            .call(reqwest::Method::POST, "/memory/forget-all", json!(null))
            .await;
        assert_eq!(status, 200);
        let (_, overview) = h.call(reqwest::Method::GET, "/memory", json!(null)).await;
        assert_eq!(overview["profile"], "");
        assert!(overview["notes"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn learning_files_what_the_user_said_and_respects_opt_outs() {
        use crate::memory::{PROFILE_PATH, store};

        const PLAN: &str = r#"{"profile": {"add": ["The user lives in Geneva", "The user's card number is 4111 1111 1111 1111"]},
            "notes": [{"path": "people/Léa", "add": ["Léa is the user's sister", "Léa is a nurse"]}]}"#;
        let llm = scripted_llm(|req, _| {
            if system_prompt(req).contains("maintain a private memory") {
                Reply::Text(PLAN)
            } else {
                Reply::Text("That's lovely!")
            }
        })
        .await;
        let mut h = Harness::new().await;
        h.use_mock(llm.port).await;

        let (conv_id, id) = h
            .start("I just moved to Geneva, and my sister Léa is a nurse here.")
            .await;
        h.wait_for_reply(&id).await;
        let (_, sent) = h
            .call(
                reqwest::Method::POST,
                &format!("/conversations/{conv_id}/messages"),
                json!({"content": "Don't remember this, but I'm seeing a therapist."}),
            )
            .await;
        h.wait_for_reply(sent["assistant_message"]["id"].as_str().unwrap())
            .await;

        let conv: uuid::Uuid = conv_id.parse().unwrap();
        let changed = crate::memory::learn::learn_from(&h.state, conv)
            .await
            .unwrap();
        assert_eq!(changed, 2);

        let learning_request = llm
            .requests()
            .into_iter()
            .find(|r| system_prompt(r).contains("maintain a private memory"))
            .unwrap();
        let shown = learning_request["messages"][1]["content"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(shown.contains("moved to Geneva"));
        assert!(!shown.contains("therapist"), "opt-out leaked: {shown}");

        let db = &h.state.db;
        let profile = store::get(db, PROFILE_PATH).await.unwrap().unwrap();
        assert!(profile.body.contains("Geneva"));
        assert!(!profile.body.contains("4111"), "secrets must be dropped");
        let lea = store::get(db, "people/léa.md").await.unwrap().unwrap();
        assert!(lea.body.contains("nurse"));
        assert_eq!(lea.source, mimi_protocol::MemorySource::Learned);

        // Nothing new since: the next pass has nothing to read.
        let before = llm.requests().len();
        assert_eq!(
            crate::memory::learn::learn_from(&h.state, conv)
                .await
                .unwrap(),
            0
        );
        assert_eq!(llm.requests().len(), before);
    }

    fn user_messages(req: &Value) -> Vec<String> {
        req["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "user")
            .map(|m| m["content"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn mentions_tell_the_model_exactly_who_was_meant() {
        let llm = scripted_llm(|_, _| Reply::Text("Noted.")).await;
        let (mut h, _, _) = setup(&llm).await;

        let (status, sam) = h
            .call(
                reqwest::Method::POST,
                "/people",
                json!({"name": "Sam Carter", "nickname": "Sammy", "handles": [
                    {"channel": "telegram", "value": "t.me/samcarter"},
                    {"channel": "email", "value": "Sam@Example.com", "label": "work"}
                ]}),
            )
            .await;
        assert_eq!(status, 200, "{sam}");
        // Values are cleaned for display but keep the user's casing.
        assert_eq!(sam["handles"][0]["value"], "Sam@Example.com");
        assert_eq!(sam["handles"][1]["value"], "@samcarter");

        // The @ search finds people by name, nickname or handle.
        let (_, found) = h
            .call(reqwest::Method::GET, "/mentions?q=sammy", Value::Null)
            .await;
        assert_eq!(found[0]["kind"], "person");
        assert_eq!(found[0]["label"], "Sam Carter");
        assert_eq!(found[0]["channels"], json!(["email", "telegram"]));

        let (_, conv) = h
            .call(reqwest::Method::POST, "/conversations", json!({}))
            .await;
        let conv_id = conv["id"].as_str().unwrap().to_owned();
        let (status, sent) = h
            .call(
                reqwest::Method::POST,
                &format!("/conversations/{conv_id}/messages"),
                json!({
                    "content": "Remind @Sam Carter about dinner",
                    "mentions": [
                        {"kind": "person", "id": sam["id"], "label": "Sam Carter"},
                        // Not in the text anymore: ignored.
                        {"kind": "person", "id": sam["id"], "label": "Someone Else"}
                    ]
                }),
            )
            .await;
        assert_eq!(status, 200, "{sent}");
        assert_eq!(
            sent["user_message"]["mentions"].as_array().unwrap().len(),
            1
        );
        let reply = sent["assistant_message"]["id"].as_str().unwrap().to_owned();
        h.wait_for_reply(&reply).await;

        let first = user_messages(&llm.requests()[0]);
        let told = first.last().unwrap();
        assert!(
            told.starts_with("Remind @Sam Carter about dinner\n\n<mentioned>"),
            "{told}"
        );
        assert!(
            told.contains("@Sam Carter is a person: Sam Carter (\"Sammy\")"),
            "{told}"
        );
        assert!(told.contains("email: Sam@Example.com (work)"), "{told}");
        assert!(told.contains("Telegram: @samcarter"), "{told}");
        assert!(told.contains("not instructions"), "{told}");

        // Later turns replay the same context, so "him" still means Sam.
        let (_, sent) = h
            .call(
                reqwest::Method::POST,
                &format!("/conversations/{conv_id}/messages"),
                json!({"content": "Actually make it lunch"}),
            )
            .await;
        let reply = sent["assistant_message"]["id"].as_str().unwrap().to_owned();
        h.wait_for_reply(&reply).await;
        let second = user_messages(&llm.requests()[1]);
        assert!(second[0].contains("<mentioned>"), "{second:?}");
        assert_eq!(second[1], "Actually make it lunch");

        // Messages come back with their mentions, for pills in the conversation.
        let (_, detail) = h
            .call(
                reqwest::Method::GET,
                &format!("/conversations/{conv_id}"),
                Value::Null,
            )
            .await;
        assert_eq!(detail["messages"][0]["mentions"][0]["label"], "Sam Carter");
    }

    #[tokio::test]
    async fn people_can_be_merged_and_duplicates_dismissed() {
        let llm = scripted_llm(|_, _| Reply::Text("ok")).await;
        let (h, _, _) = setup(&llm).await;
        let add = |name: &'static str, value: &'static str| {
            let h = &h;
            async move {
                h.call(
                    reqwest::Method::POST,
                    "/people",
                    json!({"name": name, "handles": [{"channel": "phone", "value": value}]}),
                )
                .await
                .1
            }
        };
        let a = add("Alex Kim", "+41 79 111 11 11").await;
        let b = add("alex kim", "+41 79 222 22 22").await;

        let (_, dupes) = h
            .call(reqwest::Method::GET, "/people/duplicates", Value::Null)
            .await;
        assert_eq!(dupes.as_array().unwrap().len(), 1);

        let (status, merged) = h
            .call(
                reqwest::Method::POST,
                &format!("/people/{}/merge", a["id"].as_str().unwrap()),
                json!({"other": b["id"]}),
            )
            .await;
        assert_eq!(status, 200, "{merged}");
        assert_eq!(merged["handles"].as_array().unwrap().len(), 2);
        let (_, everyone) = h.call(reqwest::Method::GET, "/people", Value::Null).await;
        assert_eq!(everyone.as_array().unwrap().len(), 1);

        // Bad input is explained, not stored.
        let (status, err) = h
            .call(
                reqwest::Method::POST,
                "/people",
                json!({"name": "X", "handles": [{"channel": "email", "value": "nope"}]}),
            )
            .await;
        assert_eq!(
            (status, err["message"].as_str()),
            (400, Some("That doesn't look like an email address."))
        );
    }
}

/// A fake Telegram Bot API: queued updates go out through getUpdates, and everything
/// sent comes back through `sent()`.
mod fake_telegram {
    use std::sync::{Arc, Mutex};

    use axum::extract::{Path, State};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{Value, json};

    #[derive(Clone, Default)]
    pub struct FakeTelegram {
        pub updates: Arc<Mutex<Vec<Value>>>,
        pub sent: Arc<Mutex<Vec<Value>>>,
        next_id: Arc<Mutex<i64>>,
    }

    impl FakeTelegram {
        pub async fn start() -> (Self, String) {
            let fake = FakeTelegram::default();
            let app = Router::new()
                .route("/{bot}/{method}", post(handle))
                .with_state(fake.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            (fake, url)
        }

        /// Queues a private message from `chat_id`.
        pub fn message(&self, chat_id: i64, name: &str, text: &str) {
            let mut id = self.next_id.lock().unwrap();
            *id += 1;
            self.updates.lock().unwrap().push(json!({
                "update_id": *id,
                "message": { "chat": { "id": chat_id, "type": "private", "first_name": name }, "text": text }
            }));
        }

        /// Queues a tap on an inline button.
        pub fn tap(&self, chat_id: i64, data: &str) {
            let mut id = self.next_id.lock().unwrap();
            *id += 1;
            self.updates.lock().unwrap().push(json!({
                "update_id": *id,
                "callback_query": {
                    "id": format!("cb{}", *id),
                    "from": { "id": chat_id },
                    "message": { "message_id": 7, "chat": { "id": chat_id, "type": "private" }, "text": "Waiting for you" },
                    "data": data
                }
            }));
        }

        /// The callback data of every button sent to `chat_id`.
        pub fn buttons_sent_to(&self, chat_id: i64) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m["chat_id"] == chat_id)
                .flat_map(|m| {
                    m["reply_markup"]["inline_keyboard"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .flat_map(|row| row.as_array().cloned().unwrap_or_default())
                        .filter_map(|b| b["callback_data"].as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
                .collect()
        }

        pub fn sent_to(&self, chat_id: i64) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m["chat_id"] == chat_id)
                .map(|m| m["text"].as_str().unwrap_or_default().to_owned())
                .collect()
        }
    }

    async fn handle(
        State(fake): State<FakeTelegram>,
        Path((bot, method)): Path<(String, String)>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        if bot != "bot123:secret" {
            return Json(json!({ "ok": false, "error_code": 401, "description": "Unauthorized" }));
        }
        let result = match method.as_str() {
            "getMe" => json!({ "username": "test_mimi_bot", "first_name": "Test" }),
            "getUpdates" => {
                let offset = body["offset"].as_i64().unwrap_or(0);
                let pending: Vec<Value> = fake
                    .updates
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|u| u["update_id"].as_i64().unwrap() >= offset)
                    .cloned()
                    .collect();
                if pending.is_empty() {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                json!(pending)
            }
            "sendMessage" => {
                fake.sent.lock().unwrap().push(body);
                json!({ "message_id": 1 })
            }
            _ => json!(true),
        };
        Json(json!({ "ok": true, "result": result }))
    }
}

#[tokio::test]
async fn telegram_bot_pairs_with_its_owner_and_relays_replies() {
    use fake_telegram::FakeTelegram;

    let llm = mock_llm(
        vec!["Hello from ", "your assistant!"],
        std::time::Duration::from_millis(1),
    )
    .await;
    let (tg, tg_url) = FakeTelegram::start().await;
    let h = Harness::new().await;
    *h.state.connections.telegram_api.lock().unwrap() = tg_url;
    h.use_mock(llm).await;

    // A wrong token is refused up front.
    let (status, err) = h
        .call(
            reqwest::Method::POST,
            "/connections",
            serde_json::json!({"integration": "telegram", "bot_token": "nope"}),
        )
        .await;
    assert_eq!(status, 400, "{err}");

    let (status, conn) = h
        .call(
            reqwest::Method::POST,
            "/connections",
            serde_json::json!({"integration": "telegram", "bot_token": "123:secret"}),
        )
        .await;
    assert_eq!(status, 200, "{conn}");
    assert_eq!(conn["status"], "needs_action");
    let link = conn["action_url"].as_str().unwrap().to_owned();
    let code = link.rsplit("start=").next().unwrap().to_owned();
    assert!(link.starts_with("https://t.me/test_mimi_bot?start="));

    // A stranger can't claim the bot, even knowing it exists.
    tg.message(99, "Mallory", "/start 000000");
    tg.message(42, "Vincent", &format!("/start {code}"));

    let wait_for = |chat: i64, n: usize| {
        let tg = tg.clone();
        async move {
            for _ in 0..200 {
                if tg.sent_to(chat).len() >= n {
                    return tg.sent_to(chat);
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            panic!(
                "timed out waiting for {n} messages to {chat}: {:?}",
                tg.sent_to(chat)
            );
        }
    };
    let welcome = wait_for(42, 1).await;
    assert!(welcome[0].contains("Hi Vincent"), "{welcome:?}");

    let (_, list) = h
        .call(
            reqwest::Method::GET,
            "/connections",
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(list[0]["status"], "ok");
    assert!(list[0]["detail"].as_str().unwrap().contains("Vincent"));

    // Only the owner gets answers.
    tg.message(99, "Mallory", "What's on Vincent's calendar?");
    tg.message(42, "Vincent", "Hi!");
    let replies = wait_for(42, 2).await;
    assert_eq!(replies[1], "Hello from your assistant!");
    assert!(tg.sent_to(99).is_empty());

    // The exchange lives in a "Telegram" conversation, visible in the app.
    let (_, conversations) = h
        .call(
            reqwest::Method::GET,
            "/conversations",
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(conversations[0]["title"], "Telegram");

    let (_, integrations) = h
        .call(
            reqwest::Method::GET,
            "/integrations",
            serde_json::Value::Null,
        )
        .await;
    let telegram = integrations
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "telegram")
        .unwrap();
    assert_eq!(telegram["status"], "connected");
}

// --- Reminders and routines ----------------------------------------------------------
mod schedule_flow {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::Harness;
    use super::fake_telegram::FakeTelegram;
    use super::tool_use::{Reply, scripted_llm, setup};
    use crate::schedule;

    const OWNER: i64 = 42;

    /// Connects the fake bot and pairs it with its owner.
    async fn pair(h: &Harness) -> FakeTelegram {
        let (tg, url) = FakeTelegram::start().await;
        *h.state.connections.telegram_api.lock().unwrap() = url;
        let (_, conn) = h
            .call(
                reqwest::Method::POST,
                "/connections",
                json!({"integration": "telegram", "bot_token": "123:secret"}),
            )
            .await;
        let code = conn["action_url"]
            .as_str()
            .unwrap()
            .rsplit("start=")
            .next()
            .unwrap()
            .to_owned();
        tg.message(OWNER, "Vincent", &format!("/start {code}"));
        wait_for(|| !tg.sent_to(OWNER).is_empty()).await;
        tg
    }

    async fn wait_for(check: impl Fn() -> bool) {
        for _ in 0..400 {
            if check() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("timed out");
    }

    /// Makes an item due now, as if its time had come, and runs the scheduler once.
    async fn make_due(h: &Harness, id: &str) {
        let mut item = schedule::store::get(&h.state.db, id.parse().unwrap())
            .await
            .unwrap()
            .unwrap();
        item.next_at = Some(crate::now_ms() - 1_000);
        schedule::store::upsert(&h.state.db, item).await.unwrap();
        schedule::tick(&h.state, jiff::Timestamp::now()).await;
    }

    async fn add(h: &Harness, body: Value) -> Value {
        let (status, item) = h.call(reqwest::Method::POST, "/schedule", body).await;
        assert_eq!(status, 200, "{item}");
        item
    }

    async fn latest_delivery(h: &Harness) -> Value {
        let (_, list) = h
            .call(reqwest::Method::GET, "/schedule/deliveries", Value::Null)
            .await;
        list[0].clone()
    }

    async fn wait_for_status(h: &Harness, status: &str) {
        for _ in 0..200 {
            if latest_delivery(h).await["status"] == status {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("no delivery became {status}: {}", latest_delivery(h).await);
    }

    fn button(tg: &FakeTelegram, prefix: &str) -> Option<String> {
        tg.buttons_sent_to(OWNER)
            .into_iter()
            .rev()
            .find(|b| b.starts_with(prefix))
    }

    #[tokio::test]
    async fn reminders_reach_telegram_with_done_and_snooze() {
        let llm = scripted_llm(|_, _| Reply::Text("ok")).await;
        let (h, _, _) = setup(&llm).await;
        let tg = pair(&h).await;

        let item = add(
            &h,
            json!({"kind": "reminder", "title": "Take out the bins", "schedule": {"type": "weekly", "days": ["mon"], "time": "20:00"}}),
        )
        .await;
        assert_eq!(item["description"], "Every Monday at 20:00");
        assert!(item["next_at"].as_i64().unwrap() > crate::now_ms());

        make_due(&h, item["id"].as_str().unwrap()).await;
        wait_for(|| button(&tg, "snooze:").is_some()).await;
        assert!(
            tg.sent_to(OWNER)
                .iter()
                .any(|m| m.contains("Take out the bins"))
        );
        wait_for_status(&h, "delivered").await;
        // The next occurrence is a week out, not replayed.
        let (_, items) = h.call(reqwest::Method::GET, "/schedule", Value::Null).await;
        assert!(items[0]["next_at"].as_i64().unwrap() > crate::now_ms() + 3_600_000);

        // Someone else's tap does nothing; the owner's snoozes it for 10 minutes.
        let snooze = button(&tg, "snooze:").unwrap();
        tg.tap(99, &snooze);
        tg.tap(OWNER, &snooze);
        wait_for_status(&h, "snoozed").await;
        let (_, items) = h.call(reqwest::Method::GET, "/schedule", Value::Null).await;
        let back = items[0]["next_at"].as_i64().unwrap() - crate::now_ms();
        assert!(
            (9 * 60_000..=10 * 60_000 + 5_000).contains(&back),
            "comes back in {back} ms"
        );

        // Done ends it (and cancels the snooze); after that it's already handled.
        tg.tap(OWNER, &button(&tg, "done:").unwrap());
        wait_for_status(&h, "done").await;
        let (_, items) = h.call(reqwest::Method::GET, "/schedule", Value::Null).await;
        assert!(items[0]["next_at"].as_i64().unwrap() > crate::now_ms() + 3_600_000);
        let id = latest_delivery(&h).await["id"].as_str().unwrap().to_owned();
        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/schedule/deliveries/{id}/done"),
                Value::Null,
            )
            .await;
        assert_eq!(status, 409);
    }

    #[tokio::test]
    async fn routines_run_in_their_conversation_and_report_back() {
        let llm = scripted_llm(|_, _| Reply::Text("Nothing planned today. **Enjoy!**")).await;
        let (h, _, _) = setup(&llm).await;
        let tg = pair(&h).await;

        let item = add(
            &h,
            json!({"kind": "routine", "title": "Morning briefing", "instruction": "Send me my day", "schedule": {"type": "daily", "time": "07:00"}}),
        )
        .await;
        make_due(&h, item["id"].as_str().unwrap()).await;
        wait_for(|| {
            tg.sent_to(OWNER)
                .iter()
                .any(|m| m.contains("Nothing planned"))
        })
        .await;
        let sent = tg
            .sent_to(OWNER)
            .into_iter()
            .find(|m| m.contains("Nothing planned"))
            .unwrap();
        assert!(sent.starts_with("<b>Morning briefing</b>"), "{sent}");
        assert!(sent.contains("<b>Enjoy!</b>"));

        wait_for_status(&h, "delivered").await;
        let delivery = latest_delivery(&h).await;
        let conversation = delivery["conversation_id"].as_str().unwrap().to_owned();
        let (_, detail) = h
            .call(
                reqwest::Method::GET,
                &format!("/conversations/{conversation}"),
                Value::Null,
            )
            .await;
        assert_eq!(detail["conversation"]["title"], "Morning briefing");
        assert_eq!(detail["messages"][0]["content"], "Send me my day");
        // The model knew it was a scheduled run; the user's chat doesn't show that.
        let prompt = llm.requests()[0]["messages"].to_string();
        assert!(prompt.contains("<routine>"), "{prompt}");
    }

    #[tokio::test]
    async fn routine_approvals_go_to_telegram_without_blocking_reminders() {
        let llm = scripted_llm(|body, _| {
            if body["messages"].to_string().contains("\"role\":\"tool\"") {
                Reply::Text("Sent the note.")
            } else {
                Reply::Call("send_note", json!({"to": "Sam"}))
            }
        })
        .await;
        let (h, _, writes) = setup(&llm).await;
        let tg = pair(&h).await;

        let routine = add(
            &h,
            json!({"kind": "routine", "title": "Friday note", "instruction": "Send Sam a note", "schedule": {"type": "weekly", "days": ["fri"], "time": "17:00"}}),
        )
        .await;
        make_due(&h, routine["id"].as_str().unwrap()).await;
        wait_for(|| button(&tg, "approve:").is_some()).await;
        assert_eq!(writes.load(Ordering::SeqCst), 0);

        // While the routine waits for its OK, reminders still go out.
        let reminder = add(
            &h,
            json!({"kind": "reminder", "title": "Water the plants", "schedule": {"type": "daily", "time": "09:00"}}),
        )
        .await;
        make_due(&h, reminder["id"].as_str().unwrap()).await;
        wait_for(|| {
            tg.sent_to(OWNER)
                .iter()
                .any(|m| m.contains("Water the plants"))
        })
        .await;

        tg.tap(OWNER, &button(&tg, "approve:").unwrap());
        wait_for(|| {
            tg.sent_to(OWNER)
                .iter()
                .any(|m| m.contains("Sent the note."))
        })
        .await;
        assert_eq!(writes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn assistant_sets_reminders_and_undo_takes_them_back() {
        let llm = scripted_llm(|_, n| match n {
            0 => Reply::Call(
                "reminder_add",
                json!({"text": "Call Léa", "in_minutes": 30}),
            ),
            _ => Reply::Text("I'll remind you."),
        })
        .await;
        let (mut h, _, _) = setup(&llm).await;
        h.state
            .tool_sources
            .add(std::sync::Arc::new(schedule::tools::ScheduleTools));

        let (_, conv) = h
            .call(reqwest::Method::POST, "/conversations", json!({}))
            .await;
        let conv_id = conv["id"].as_str().unwrap().to_owned();
        let (_, sent) = h
            .call(
                reqwest::Method::POST,
                &format!("/conversations/{conv_id}/messages"),
                json!({"content": "Remind me to call Léa in half an hour"}),
            )
            .await;
        let assistant = sent["assistant_message"]["id"].as_str().unwrap().to_owned();
        let (reply, _) = h.wait_for_reply(&assistant).await;
        let action = &reply.actions[0];
        assert_eq!(action.tool, "reminder_add");
        assert!(!action.requires_approval);
        let result = action.result.clone().unwrap();
        assert!(result.contains("Call Léa"), "{result}");
        let revision = action.output.as_ref().unwrap()["schedule_revision"]
            .as_i64()
            .unwrap();

        let (_, items) = h.call(reqwest::Method::GET, "/schedule", Value::Null).await;
        assert_eq!(items[0]["title"], "Call Léa");
        let in_ms = items[0]["next_at"].as_i64().unwrap() - crate::now_ms();
        assert!((29 * 60_000..=31 * 60_000).contains(&in_ms), "{in_ms}");

        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/schedule/undo/{revision}"),
                Value::Null,
            )
            .await;
        assert_eq!(status, 200);
        let (_, items) = h.call(reqwest::Method::GET, "/schedule", Value::Null).await;
        assert_eq!(items.as_array().unwrap().len(), 0);
        let (_, detail) = h
            .call(
                reqwest::Method::GET,
                &format!("/conversations/{conv_id}"),
                Value::Null,
            )
            .await;
        assert_eq!(
            detail["messages"][1]["actions"][0]["output"]["undone"],
            true
        );
        let (status, _) = h
            .call(
                reqwest::Method::POST,
                &format!("/schedule/undo/{revision}"),
                Value::Null,
            )
            .await;
        assert_eq!(status, 409);
    }
}
