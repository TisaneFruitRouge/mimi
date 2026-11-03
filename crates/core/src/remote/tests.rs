//! Phones over iroh, end to end on this machine: a real daemon endpoint and fake phones,
//! with relays and address lookup off, so nothing leaves the machine.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode};
use mimi_protocol::{Event, Paired, PairingOffer, RemoteAccess};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::wire::{RequestHead, ResponseHead, read_head, write_head};
use super::*;

const TOKEN: &str = "local-token";

fn daemon() -> Arc<AppState> {
    let state = Arc::new(AppState::for_tests(TOKEN));
    state.remote.local_only_for_tests();
    state
}

async fn new_phone() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .unwrap()
}

async fn dial(state: &AppState, phone: &Endpoint) -> Option<Connection> {
    let daemon = state.remote.endpoint().await.expect("running");
    let port = daemon
        .bound_sockets()
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .unwrap()
        .port();
    let addr =
        EndpointAddr::new(daemon.id()).with_ip_addr(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    phone.connect(addr, ALPN).await.ok()
}

struct Reply {
    status: u16,
    body: Value,
}

async fn send(
    conn: &Connection,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> anyhow::Result<Reply> {
    let (mut tx, mut rx) = conn.open_bi().await?;
    let mut headers = BTreeMap::new();
    if let Some(token) = token {
        headers.insert("authorization".to_owned(), format!("Bearer {token}"));
    }
    if body.is_some() {
        headers.insert("content-type".to_owned(), "application/json".to_owned());
    }
    let head = RequestHead {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
    };
    write_head(&mut tx, &head).await?;
    if let Some(body) = body {
        tx.write_all(&serde_json::to_vec(&body)?).await?;
    }
    tx.finish()?;
    let head: ResponseHead = read_head(&mut rx).await?;
    let bytes = rx.read_to_end(1 << 20).await?;
    Ok(Reply {
        status: head.status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    })
}

/// A request from this computer (the desktop app's token).
async fn local(
    state: &Arc<AppState>,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
        .unwrap();
    let res = crate::api::router(state.clone())
        .oneshot(req)
        .await
        .unwrap();
    let status = res.status();
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn code_of(offer: &PairingOffer) -> String {
    let url = url::Url::parse(&offer.link).unwrap();
    url.query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned()
}

#[tokio::test]
async fn a_phone_pairs_and_uses_the_api_with_its_own_token_only() {
    let state = daemon();
    let (status, offer) = local(&state, "POST", "/v1/remote/pairing", None).await;
    assert_eq!(status, StatusCode::OK, "{offer}");
    let offer: PairingOffer = serde_json::from_value(offer).unwrap();
    assert!(offer.link.starts_with("mimi://pair?id="));
    assert!(offer.qr_svg.contains("<svg"));
    let code = code_of(&offer);

    let phone = new_phone().await;
    let conn = dial(&state, &phone)
        .await
        .expect("connects while a code waits");

    // Nothing without a token, and this computer's own token means nothing here.
    assert_eq!(
        send(&conn, "GET", "/v1/status", None, None)
            .await
            .unwrap()
            .status,
        401
    );
    assert_eq!(
        send(&conn, "GET", "/v1/status", Some(TOKEN), None)
            .await
            .unwrap()
            .status,
        401
    );
    // Only the API: no web pages, no login links.
    assert_eq!(
        send(&conn, "GET", "/login?code=x", None, None)
            .await
            .unwrap()
            .status,
        404
    );

    let wrong = send(
        &conn,
        "POST",
        "/v1/remote/pair",
        None,
        Some(json!({"code": "nope", "name": "Phone"})),
    )
    .await
    .unwrap();
    assert_eq!(wrong.status, 401);
    assert_eq!(wrong.body["code"], "pairing_expired");

    let paired = send(
        &conn,
        "POST",
        "/v1/remote/pair",
        None,
        Some(json!({"code": code, "name": " Sam's iPhone "})),
    )
    .await
    .unwrap();
    assert_eq!(paired.status, 200, "{}", paired.body);
    let paired: Paired = serde_json::from_value(paired.body).unwrap();
    assert_eq!(paired.device.name, "Sam's iPhone");
    assert!(paired.device.this_device);

    // The code works once.
    let again = send(
        &conn,
        "POST",
        "/v1/remote/pair",
        None,
        Some(json!({"code": code, "name": "Again"})),
    )
    .await
    .unwrap();
    assert_eq!(again.status, 401);

    let token = paired.token.as_str();
    assert_eq!(
        send(&conn, "GET", "/v1/status", Some(token), None)
            .await
            .unwrap()
            .status,
        200
    );
    let remote = send(&conn, "GET", "/v1/remote", Some(token), None)
        .await
        .unwrap();
    let remote: RemoteAccess = serde_json::from_value(remote.body).unwrap();
    assert!(remote.running);
    assert_eq!(remote.devices.len(), 1);
    assert!(remote.devices[0].this_device);
    assert_eq!(
        remote.devices[0].connection,
        Some(mimi_protocol::DevicePath::Direct)
    );

    // Phones can't make codes for other phones, move the relay, or mint browser links.
    for (method, path, body) in [
        ("POST", "/v1/remote/pairing", None),
        ("PUT", "/v1/remote/relay", Some(json!({"kind": "default"}))),
        ("POST", "/v1/web/login-link", None),
    ] {
        let reply = send(&conn, method, path, Some(token), body).await.unwrap();
        assert_eq!(reply.status, 403, "{method} {path}");
    }

    // The pairing route is only for phones.
    let (status, _) = local(
        &state,
        "POST",
        "/v1/remote/pair",
        Some(json!({"code": "x", "name": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A second phone can't borrow the first one's token.
    let thief = new_phone().await;
    local(&state, "POST", "/v1/remote/pairing", None).await;
    let thief_conn = dial(&state, &thief).await.unwrap();
    assert_eq!(
        send(&thief_conn, "GET", "/v1/status", Some(token), None)
            .await
            .unwrap()
            .status,
        401
    );

    // Once no code waits, strangers are turned away at the door.
    local(&state, "DELETE", "/v1/remote/pairing", None).await;
    let stranger = new_phone().await;
    let refused = match dial(&state, &stranger).await {
        None => true,
        Some(conn) => send(&conn, "GET", "/health", None, None).await.is_err(),
    };
    assert!(refused);

    // A phone can't remove another.
    let other = local(&state, "GET", "/v1/remote", None).await.1;
    assert_eq!(other["devices"].as_array().unwrap().len(), 1);
    let not_mine = send(
        &conn,
        "DELETE",
        &format!("/v1/remote/devices/{}", uuid::Uuid::now_v7()),
        Some(token),
        None,
    )
    .await
    .unwrap();
    assert_eq!(not_mine.status, 403);

    // Removing the phone cuts it off at once.
    let id = paired.device.id;
    let (status, _) = local(&state, "DELETE", &format!("/v1/remote/devices/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::timeout(Duration::from_secs(5), conn.closed())
        .await
        .expect("connection closed");
    let (_, remote) = local(&state, "GET", "/v1/remote", None).await;
    let remote: RemoteAccess = serde_json::from_value(remote).unwrap();
    assert!(remote.devices.is_empty());
    // With no phone left and no code waiting, phone access stops.
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.remote.endpoint().await.is_some() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("phone access stopped");
}

#[tokio::test]
async fn phones_hear_events_as_lines() {
    let state = daemon();
    let (_, offer) = local(&state, "POST", "/v1/remote/pairing", None).await;
    let offer: PairingOffer = serde_json::from_value(offer).unwrap();
    let phone = new_phone().await;
    let conn = dial(&state, &phone).await.unwrap();
    let paired = send(
        &conn,
        "POST",
        "/v1/remote/pair",
        None,
        Some(json!({"code": code_of(&offer), "name": "Phone"})),
    )
    .await
    .unwrap();
    let token = paired.body["token"].as_str().unwrap().to_owned();

    let (mut tx, mut rx) = conn.open_bi().await.unwrap();
    let mut headers = BTreeMap::new();
    headers.insert("authorization".to_owned(), format!("Bearer {token}"));
    headers.insert("accept".to_owned(), "application/x-ndjson".to_owned());
    write_head(
        &mut tx,
        &RequestHead {
            method: "GET".into(),
            path: "/v1/events".into(),
            headers,
        },
    )
    .await
    .unwrap();
    tx.finish().unwrap();
    let head: ResponseHead = read_head(&mut rx).await.unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.headers["content-type"], "application/x-ndjson");

    state.events.publish(Event::MemoryChanged);
    let mut buf = Vec::new();
    let line = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut chunk = [0u8; 256];
            let n = rx.read(&mut chunk).await.unwrap().expect("open");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(line) = buf
                .split(|b| *b == b'\n')
                .find(|l| !l.is_empty() && buf.ends_with(b"\n"))
            {
                return String::from_utf8(line.to_vec()).unwrap();
            }
        }
    })
    .await
    .unwrap();
    let event: Event = serde_json::from_str(&line).unwrap();
    assert_eq!(event, Event::MemoryChanged);
}

#[test]
fn relay_addresses_are_checked() {
    use mimi_protocol::Relay;
    assert!(
        check_relay(&Relay::Custom {
            url: "https://relay.example.org".into()
        })
        .is_ok()
    );
    assert!(
        check_relay(&Relay::Custom {
            url: "http://127.0.0.1:3340".into()
        })
        .is_ok()
    );
    assert!(
        check_relay(&Relay::Custom {
            url: "http://relay.example.org".into()
        })
        .is_err()
    );
    assert!(
        check_relay(&Relay::Custom {
            url: "relay".into()
        })
        .is_err()
    );
}

#[test]
fn pairing_links_carry_a_custom_relay_only() {
    use mimi_protocol::Relay;
    let plain = pairing_link("abc", "123", &Relay::Default);
    assert_eq!(plain, "mimi://pair?id=abc&code=123");
    let own = pairing_link(
        "abc",
        "123",
        &Relay::Custom {
            url: "https://r.example/".into(),
        },
    );
    assert_eq!(
        own,
        "mimi://pair?id=abc&code=123&relay=https%3A%2F%2Fr.example%2F"
    );
}

#[test]
fn secret_keys_round_trip() {
    let key = SecretKey::generate();
    let back = parse_secret(&hex(&key.to_bytes())).unwrap();
    assert_eq!(back.public(), key.public());
    assert!(parse_secret("zz").is_none());
}

/// Pairs like the iPhone app does, through number 0's address lookup and relays, with a
/// running daemon: `MIMI_TEST_PAIRING_LINK='mimi://pair?…' cargo test -p mimi-core
/// live_pairing -- --ignored` (make the link with `POST /v1/remote/pairing`).
#[tokio::test]
#[ignore]
async fn live_pairing() {
    let link = std::env::var("MIMI_TEST_PAIRING_LINK").expect("MIMI_TEST_PAIRING_LINK");
    let url = url::Url::parse(&link).unwrap();
    let get = |k: &str| {
        url.query_pairs()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.into_owned())
    };
    let id: iroh::EndpointId = get("id").unwrap().parse().unwrap();
    let phone = Endpoint::bind(presets::N0).await.unwrap();
    let started = std::time::Instant::now();
    let conn = phone.connect(id, ALPN).await.expect("reaches the daemon");
    println!("connected in {:?}", started.elapsed());
    let paired = send(
        &conn,
        "POST",
        "/v1/remote/pair",
        None,
        Some(json!({"code": get("code").unwrap(), "name": "Live test"})),
    )
    .await
    .unwrap();
    assert_eq!(paired.status, 200, "{}", paired.body);
    let token = paired.body["token"].as_str().unwrap().to_owned();
    let status = send(&conn, "GET", "/v1/status", Some(&token), None)
        .await
        .unwrap();
    assert_eq!(status.status, 200);
    tokio::time::sleep(Duration::from_secs(3)).await;
    let paths: Vec<_> = conn
        .paths()
        .iter()
        .map(|p| (p.is_selected(), p.is_ip()))
        .collect();
    println!("paths (selected, direct): {paths:?}");
    let remote = send(&conn, "GET", "/v1/remote", Some(&token), None)
        .await
        .unwrap();
    println!("{}", remote.body);
    // Leave no trace on the daemon.
    let id = paired.body["device"]["id"].as_str().unwrap();
    let removed = send(
        &conn,
        "DELETE",
        &format!("/v1/remote/devices/{id}"),
        Some(&token),
        None,
    )
    .await
    .unwrap();
    assert_eq!(removed.status, 200);
}
