use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ── User ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub created_at: String,
    pub updated_at: String,
}

impl User {
    pub fn new_id() -> String {
        Uuid::new_v4().to_string()
    }
}

// ── Download queue item ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DownloadStatus::Queued => "queued",
            DownloadStatus::Downloading => "downloading",
            DownloadStatus::Done => "done",
            DownloadStatus::Failed => "failed",
            DownloadStatus::Cancelled => "cancelled",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for DownloadStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(DownloadStatus::Queued),
            "downloading" => Ok(DownloadStatus::Downloading),
            "done" => Ok(DownloadStatus::Done),
            "failed" => Ok(DownloadStatus::Failed),
            "cancelled" => Ok(DownloadStatus::Cancelled),
            other => Err(anyhow::anyhow!("unknown status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct QueueItem {
    pub id: String,
    pub pid: String,
    pub title: String,
    pub series: Option<String>,
    pub episode: Option<String>,
    pub channel: Option<String>,
    pub media_type: String,
    pub thumbnail_url: Option<String>,
    pub added_at: String,
    pub scheduled_at: Option<String>,
    pub priority: i64,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<String>,
    pub output_path: Option<String>,
    pub progress: f64,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub file_size: Option<i64>,
    pub quality: String,
    pub subtitles: bool,
    pub metadata: String, // JSON blob
    pub user_id: String,
}

impl QueueItem {
    pub fn new_id() -> String {
        Uuid::new_v4().to_string()
    }
}

// ── Request / Response DTOs ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddQueueItemRequest {
    pub pid: String,
    pub title: String,
    pub series: Option<String>,
    pub episode: Option<String>,
    pub channel: Option<String>,
    #[serde(default = "default_media_type")]
    pub media_type: String,
    pub thumbnail_url: Option<String>,
    pub scheduled_at: Option<DateTime<Utc>>,
    #[serde(default = "default_priority")]
    pub priority: i64,
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default = "default_subtitles")]
    pub subtitles: bool,
}

fn default_media_type() -> String {
    "tv".to_string()
}
fn default_priority() -> i64 {
    5
}
fn default_quality() -> String {
    "best".to_string()
}
fn default_subtitles() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Deserialize, Default)]
pub struct QueueQuery {
    pub status: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

/// Live progress update broadcast via WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    Progress {
        id: String,
        progress: f64,
        speed: Option<String>,
        eta: Option<String>,
    },
    StatusChange {
        id: String,
        status: String,
    },
    ItemAdded {
        item: QueueItem,
    },
    ItemRemoved {
        id: String,
    },
    Error {
        id: String,
        message: String,
    },
}

/// Simplified search result returned from get_iplayer --search
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchResult {
    pub pid: String,
    pub title: String,
    pub series: Option<String>,
    pub episode: Option<String>,
    pub channel: Option<String>,
    pub media_type: String,
    pub thumbnail_url: Option<String>,
    pub available_until: Option<String>,
    pub duration: Option<String>,
    pub description: Option<String>,
}

/// Key/value settings pair
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}
