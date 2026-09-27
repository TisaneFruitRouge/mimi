use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use mimi_protocol::Event;
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;

pub async fn subscribe(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| forward(socket, rx))
}

async fn forward(mut socket: WebSocket, mut rx: tokio::sync::broadcast::Receiver<Event>) {
    loop {
        tokio::select! {
            event = rx.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(RecvError::Lagged(_)) => Event::Resync,
                    Err(RecvError::Closed) => return,
                };
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
