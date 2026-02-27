use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::{
    auth::AuthUser,
    error::{AppError, Result},
    models::{
        AddQueueItemRequest, DownloadStatus, PaginatedResponse, QueueItem, QueueQuery, WsEvent,
    },
    state::AppState,
};

/// GET /api/queue
pub async fn list_queue(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Query(q): Query<QueueQuery>,
) -> Result<Json<PaginatedResponse<QueueItem>>> {
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(25).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let (items, total) = if let Some(status) = &q.status {
        let items: Vec<QueueItem> = sqlx::query_as(
            "SELECT * FROM queue_items WHERE status=? ORDER BY priority ASC, added_at ASC LIMIT ? OFFSET ?",
        )
        .bind(status)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM queue_items WHERE status=?")
            .bind(status)
            .fetch_one(&state.db)
            .await?;

        (items, total)
    } else {
        let items: Vec<QueueItem> = sqlx::query_as(
            "SELECT * FROM queue_items ORDER BY priority ASC, added_at ASC LIMIT ? OFFSET ?",
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM queue_items")
            .fetch_one(&state.db)
            .await?;

        (items, total)
    };

    Ok(Json(PaginatedResponse {
        data: items,
        total,
        page,
        per_page,
    }))
}

/// GET /api/queue/:id
pub async fn get_queue_item(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<QueueItem>> {
    let item: Option<QueueItem> = sqlx::query_as("SELECT * FROM queue_items WHERE id=?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?;

    item.map(Json).ok_or(AppError::NotFound)
}

/// POST /api/queue
pub async fn add_to_queue(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(req): Json<AddQueueItemRequest>,
) -> Result<(StatusCode, Json<QueueItem>)> {
    // Reject duplicate PIDs that are already queued or downloading
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM queue_items WHERE pid=? AND status IN ('queued','downloading')",
    )
    .bind(&req.pid)
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::Conflict(format!(
            "PID {} is already queued",
            req.pid
        )));
    }

    let id = QueueItem::new_id();
    let now = chrono::Utc::now().to_rfc3339();
    let scheduled = req.scheduled_at.map(|t| t.to_rfc3339());

    sqlx::query(
        "INSERT INTO queue_items \
         (id, pid, title, series, episode, channel, media_type, thumbnail_url, \
          added_at, scheduled_at, priority, status, quality, subtitles, metadata, user_id) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&req.pid)
    .bind(&req.title)
    .bind(&req.series)
    .bind(&req.episode)
    .bind(&req.channel)
    .bind(&req.media_type)
    .bind(&req.thumbnail_url)
    .bind(&now)
    .bind(&scheduled)
    .bind(req.priority)
    .bind(DownloadStatus::Queued.to_string())
    .bind(&req.quality)
    .bind(req.subtitles)
    .bind("{}")
    .bind(&user.id)
    .execute(&state.db)
    .await?;

    let item: QueueItem = sqlx::query_as("SELECT * FROM queue_items WHERE id=?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    // Notify the worker (only enqueue immediately if no scheduled time)
    if req.scheduled_at.is_none() {
        state.queue.enqueue(id.clone());
    }

    let _ = state.events.send(WsEvent::ItemAdded { item: item.clone() });

    Ok((StatusCode::CREATED, Json(item)))
}

/// DELETE /api/queue/:id   — cancel and remove
pub async fn remove_from_queue(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let item: Option<QueueItem> = sqlx::query_as("SELECT * FROM queue_items WHERE id=?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?;

    let item = item.ok_or(AppError::NotFound)?;

    // If actively downloading, mark cancelled so the worker task notices;
    // the worker is responsible for cleaning up the partial file.
    if item.status == DownloadStatus::Downloading.to_string() {
        sqlx::query("UPDATE queue_items SET status='cancelled' WHERE id=?")
            .bind(&id)
            .execute(&state.db)
            .await?;
    } else {
        // Delete the output file from disk if present
        if let Some(ref path) = item.output_path {
            if !path.is_empty() {
                if let Err(e) = tokio::fs::remove_file(path).await {
                    // Not fatal — file may have already been moved or deleted
                    tracing::warn!("Could not delete output file {path}: {e}");
                }
            }
        }
        sqlx::query("DELETE FROM queue_items WHERE id=?")
            .bind(&id)
            .execute(&state.db)
            .await?;
    }

    let _ = state.events.send(WsEvent::ItemRemoved { id });
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/queue/:id/retry
pub async fn retry_queue_item(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<QueueItem>> {
    sqlx::query(
        "UPDATE queue_items \
         SET status='queued', error=NULL, progress=0, started_at=NULL, completed_at=NULL \
         WHERE id=? AND status IN ('failed','cancelled')",
    )
    .bind(&id)
    .execute(&state.db)
    .await?;

    let item: Option<QueueItem> = sqlx::query_as("SELECT * FROM queue_items WHERE id=?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?;

    let item = item.ok_or(AppError::NotFound)?;
    state.queue.enqueue(id);
    Ok(Json(item))
}

/// POST /api/queue/reorder  — body: [{ id, priority }]
#[derive(serde::Deserialize)]
pub struct ReorderEntry {
    pub id: String,
    pub priority: i64,
}

pub async fn reorder_queue(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Json(entries): Json<Vec<ReorderEntry>>,
) -> Result<StatusCode> {
    for entry in entries {
        sqlx::query("UPDATE queue_items SET priority=? WHERE id=?")
            .bind(entry.priority)
            .bind(&entry.id)
            .execute(&state.db)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
