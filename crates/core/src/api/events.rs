use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use mimi_protocol::Event;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;

/// How often the streamed feed sends a blank line when nothing happens, so a phone can
/// tell a quiet feed from a dead connection.
const HEARTBEAT: Duration = Duration::from_secs(25);

/// The event feed: a WebSocket, or for clients without one (phones over iroh) a
/// streamed response with one JSON event per line (`Accept: application/x-ndjson`).
pub async fn subscribe(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let rx = state.events.subscribe();
    match ws {
        Ok(ws) => ws.on_upgrade(move |socket| forward(state, socket, rx)),
        Err(_) if wants_lines(&headers) => lines(state, rx),
        Err(rejection) => rejection.into_response(),
    }
}

fn wants_lines(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/x-ndjson"))
}

/// The next event for the owner's clients; `None` once the daemon stops.
async fn next_event(state: &AppState, rx: &mut Receiver<Event>) -> Option<Event> {
    loop {
        match rx.recv().await {
            // A trusted person's conversation belongs to them: nothing about it reaches
            // the owner's clients.
            Ok(event) if state.access.hides(&event) => continue,
            Ok(event) => return Some(event),
            Err(RecvError::Lagged(_)) => return Some(Event::Resync),
            Err(RecvError::Closed) => return None,
        }
    }
}

fn lines(state: Arc<AppState>, rx: Receiver<Event>) -> Response {
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let stream = futures::stream::unfold(
        (state, rx, heartbeat),
        |(state, mut rx, mut heartbeat)| async move {
            let line = tokio::select! {
                event = next_event(&state, &mut rx) => {
                    let mut json = serde_json::to_vec(&event?).expect("events serialize");
                    json.push(b'\n');
                    json
                }
                // The first tick is immediate, which also sends the head straight away.
                _ = heartbeat.tick() => b"\n".to_vec(),
            };
            Some((
                Ok::<_, Infallible>(Bytes::from(line)),
                (state, rx, heartbeat),
            ))
        },
    );
    (
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

async fn forward(state: Arc<AppState>, mut socket: WebSocket, mut rx: Receiver<Event>) {
    loop {
        tokio::select! {
            event = next_event(&state, &mut rx) => {
                let Some(event) = event else { return };
                let json = serde_json::to_string(&event).expect("events serialize");
                if socket.send(Message::Text(json.into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                // Clients don't send anything meaningful; pings are answered by axum.
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}
