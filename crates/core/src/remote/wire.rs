//! The `mimi/1` protocol: plain API requests over iroh, one QUIC stream per request.
//!
//! The phone opens a bidirectional stream and writes a head (4-byte big-endian length,
//! then JSON `{"method", "path", "headers"}`), then the body, then finishes its side. The
//! daemon answers the same way (`{"status", "headers"}`, then the body) and finishes. A
//! streamed response (the event feed) simply doesn't finish until the phone stops it.
//! Requests go through the same router as local ones, tagged with the phone's endpoint
//! id, so every route keeps its own checks.

use std::collections::BTreeMap;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Method, Request, StatusCode, Uri};
use futures::StreamExt;
use iroh::endpoint::{RecvStream, SendStream};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

use super::Peer;

/// Heads are small: a path and a few headers.
const MAX_HEAD: usize = 64 * 1024;
/// Bodies are JSON, or at most a forwarded draft.
const MAX_BODY: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHead {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseHead {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

pub async fn read_head<T: DeserializeOwned>(recv: &mut RecvStream) -> anyhow::Result<T> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    anyhow::ensure!(len <= MAX_HEAD, "head too large");
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf)?)
}

pub async fn write_head<T: Serialize>(send: &mut SendStream, head: &T) -> anyhow::Result<()> {
    let json = serde_json::to_vec(head)?;
    let mut frame = Vec::with_capacity(json.len() + 4);
    frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
    frame.extend_from_slice(&json);
    send.write_all(&frame).await?;
    Ok(())
}

/// Serves one request stream.
pub async fn serve(router: Router, peer: Peer, mut send: SendStream, mut recv: RecvStream) {
    let response = match read_request(&peer, &mut recv).await {
        Ok(req) => match router.oneshot(req).await {
            Ok(res) => res,
            Err(never) => match never {},
        },
        Err(status) => {
            let _ = write_head(
                &mut send,
                &ResponseHead {
                    status: status.as_u16(),
                    headers: BTreeMap::new(),
                },
            )
            .await;
            let _ = send.finish();
            return;
        }
    };
    let (parts, body) = response.into_parts();
    let headers = parts
        .headers
        .iter()
        .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
        .collect();
    let head = ResponseHead {
        status: parts.status.as_u16(),
        headers,
    };
    if write_head(&mut send, &head).await.is_err() {
        return;
    }
    let mut body = body.into_data_stream();
    while let Some(chunk) = body.next().await {
        let Ok(chunk) = chunk else {
            // The response broke off midway: say so rather than finish cleanly.
            let _ = send.reset(1u32.into());
            return;
        };
        if send.write_all(&chunk).await.is_err() {
            // The phone stopped reading (it left the screen, or went away).
            return;
        }
    }
    let _ = send.finish();
}

async fn read_request(peer: &Peer, recv: &mut RecvStream) -> Result<Request<Body>, StatusCode> {
    let head: RequestHead = read_head(recv).await.map_err(|_| StatusCode::BAD_REQUEST)?;
    // Only the API: the web interface's pages and login links are for this computer.
    let path_only = head.path.split('?').next().unwrap_or_default();
    if !(path_only == "/health" || path_only.starts_with(mimi_protocol::API_PREFIX)) {
        return Err(StatusCode::NOT_FOUND);
    }
    let method = Method::from_bytes(head.method.as_bytes()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let uri: Uri = head.path.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let body = recv.read_to_end(MAX_BODY).await.map_err(|e| match e {
        iroh::endpoint::ReadToEndError::TooLong => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::BAD_REQUEST,
    })?;
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body))
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    for (name, value) in &head.headers {
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) else {
            return Err(StatusCode::BAD_REQUEST);
        };
        // Browser credentials mean nothing here; only the phone's own token does.
        if name == axum::http::header::COOKIE {
            continue;
        }
        req.headers_mut().append(name, value);
    }
    req.extensions_mut().insert(peer.clone());
    Ok(req)
}
