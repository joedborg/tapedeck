use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{config::AppConfig, db::Db, models::WsEvent, queue::QueueHandle};

/// Shared application state injected into every Axum handler.
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Arc<AppConfig>,
    pub queue: QueueHandle,
    /// Broadcast channel for real-time WebSocket events.
    pub events: broadcast::Sender<WsEvent>,
}
