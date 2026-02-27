use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::{auth::verify_token, models::WsEvent, state::AppState};

#[derive(Deserialize)]
pub struct WsQuery {
    /// Auth token passed as a query param (convenient for browser WebSocket API
    /// which can't set custom headers).
    pub token: Option<String>,
}

/// GET /ws  — real-time event stream
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> Response {
    // Validate token before upgrading
    let authed = query
        .token
        .as_deref()
        .and_then(|t| verify_token(t, &state.config.secret))
        .is_some();

    if !authed {
        // Return 401 without upgrading
        return axum::response::IntoResponse::into_response((
            axum::http::StatusCode::UNAUTHORIZED,
            "unauthorized",
        ));
    }

    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, rx))
}

async fn handle_socket(socket: WebSocket, mut rx: broadcast::Receiver<WsEvent>) {
    let (mut sink, mut stream) = socket.split();

    // Task: forward broadcast events → client
    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(e) => {
                            warn!("WS serialise error: {e}");
                            continue;
                        }
                    };
                    if sink.send(Message::Text(json.into())).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WS subscriber lagged by {n} messages");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Keep reading from the client (ping/pong and close frames are handled
    // automatically; we don't currently process any client→server messages).
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(m) => {
                debug!("WS recv (ignored): {m:?}");
            }
        }
    }

    send_task.abort();
}
