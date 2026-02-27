/// Background download queue.
///
/// A `QueueHandle` is cloned into every Axum handler. Work items are sent over
/// a Tokio channel. A pool of worker tasks drains the channel up to
/// `max_concurrent` downloads at a time.
use std::sync::Arc;

use tokio::sync::{Semaphore, broadcast, mpsc};
use tracing::{error, info, warn};

use crate::{
    config::AppConfig,
    db::Db,
    iplayer::{self, DownloadOptions},
    models::{DownloadStatus, QueueItem, WsEvent},
};

// ── Public handle ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QueueHandle {
    tx: mpsc::UnboundedSender<String>, // sends item IDs to the worker pool
}

impl QueueHandle {
    /// Enqueue an already-persisted item by ID.
    pub fn enqueue(&self, id: String) {
        let _ = self.tx.send(id);
    }
}

// ── Worker pool startup ────────────────────────────────────────────────────────

pub fn start_worker_pool(
    db: Db,
    config: Arc<AppConfig>,
    events: broadcast::Sender<WsEvent>,
) -> QueueHandle {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let max = config.max_concurrent;

    tokio::spawn(run_pool(rx, tx.clone(), db, config, events, max));

    QueueHandle { tx }
}

async fn run_pool(
    mut rx: mpsc::UnboundedReceiver<String>,
    tx: mpsc::UnboundedSender<String>,
    db: Db,
    config: Arc<AppConfig>,
    events: broadcast::Sender<WsEvent>,
    max_concurrent: usize,
) {
    let sem = Arc::new(Semaphore::new(max_concurrent));

    // On startup, resume any items that were mid-download or still queued when
    // the service last stopped.
    requeue_interrupted(&db, &events, &tx).await;

    while let Some(id) = rx.recv().await {
        let permit = Arc::clone(&sem)
            .acquire_owned()
            .await
            .expect("semaphore closed");

        let db = db.clone();
        let config = Arc::clone(&config);
        let events = events.clone();

        tokio::spawn(async move {
            let _permit = permit; // held for the duration of the download
            run_download(id, db, config, events).await;
        });
    }
}

/// On startup: reset any interrupted-mid-download items back to `queued`, then
/// push all pending `queued` items (including any that were already queued before
/// the restart) back into the worker channel so they're actually downloaded.
async fn requeue_interrupted(
    db: &Db,
    events: &broadcast::Sender<WsEvent>,
    tx: &mpsc::UnboundedSender<String>,
) {
    // 1. Reset anything stuck in `downloading` back to `queued`
    let interrupted: Vec<(String,)> =
        match sqlx::query_as("SELECT id FROM queue_items WHERE status = 'downloading'")
            .fetch_all(db)
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                error!("Startup: failed to query interrupted downloads: {e}");
                return;
            }
        };

    info!(
        "Startup: {} item(s) were mid-download, resetting to queued",
        interrupted.len()
    );

    for (id,) in &interrupted {
        if let Err(e) = sqlx::query(
            "UPDATE queue_items SET status='queued', started_at=NULL, progress=0 WHERE id=?",
        )
        .bind(id)
        .execute(db)
        .await
        {
            error!("Startup: failed to reset interrupted item {id}: {e}");
            continue;
        }
        let _ = events.send(WsEvent::StatusChange {
            id: id.clone(),
            status: DownloadStatus::Queued.to_string(),
        });
    }

    // 2. Enqueue all items currently in `queued` state that are either
    //    unscheduled or whose scheduled time has already passed.
    //    (Future-scheduled items are handled by the minute-tick watcher.)
    let now = chrono::Utc::now().to_rfc3339();
    let queued: Vec<(String,)> = match sqlx::query_as(
        "SELECT id FROM queue_items \
         WHERE status = 'queued' \
           AND (scheduled_at IS NULL OR scheduled_at <= ?) \
         ORDER BY added_at ASC",
    )
    .bind(&now)
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            error!("Startup: failed to query queued items: {e}");
            return;
        }
    };

    info!("Startup: resuming {} queued item(s)", queued.len());

    for (id,) in queued {
        info!("Startup: re-enqueuing item {id}");
        if tx.send(id).is_err() {
            error!("Startup: worker channel closed, aborting requeue");
            break;
        }
    }
}

// ── Single download task ───────────────────────────────────────────────────────

async fn run_download(
    id: String,
    db: Db,
    config: Arc<AppConfig>,
    events: broadcast::Sender<WsEvent>,
) {
    // Fetch the item
    let item: Option<QueueItem> = sqlx::query_as("SELECT * FROM queue_items WHERE id = ?")
        .bind(&id)
        .fetch_optional(&db)
        .await
        .unwrap_or(None);

    let item = match item {
        Some(i) => i,
        None => {
            warn!("Queue item {id} not found, skipping");
            return;
        }
    };

    // Check it hasn't been cancelled since it was enqueued
    if item.status == DownloadStatus::Cancelled.to_string() {
        info!("Item {id} was cancelled before download started");
        return;
    }

    // Mark as downloading
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = sqlx::query(
        "UPDATE queue_items SET status='downloading', started_at=?, progress=0 WHERE id=?",
    )
    .bind(&now)
    .bind(&id)
    .execute(&db)
    .await
    {
        error!("Failed to mark {id} as downloading: {e}");
        return;
    }

    let _ = events.send(WsEvent::StatusChange {
        id: id.clone(),
        status: DownloadStatus::Downloading.to_string(),
    });

    info!("Starting download for PID {} (item {})", item.pid, id);

    // ── Read max_download_retries from DB settings (falls back to env config) ──
    let max_retries: u32 = {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key='max_download_retries'")
                .fetch_optional(&db)
                .await
                .unwrap_or(None);
        row.and_then(|(v,)| v.parse().ok())
            .unwrap_or(config.max_download_retries)
    };

    // ── Download with exponential-backoff retries ──────────────────────────────
    let mut attempt = 0u32;
    let final_result = loop {
        let id_clone = id.clone();
        let db_clone = db.clone();
        let events_clone = events.clone();

        let opts = DownloadOptions {
            pid: &item.pid,
            media_type: &item.media_type,
            quality: &item.quality,
            subtitles: item.subtitles,
            output_dir: &config.output_dir,
            get_iplayer_path: &config.get_iplayer_path,
            ffmpeg_path: &config.ffmpeg_path,
            cache_dir: &config.iplayer_cache_dir,
            proxy: config.proxy.as_deref(),
        };

        let result = {
            // Spawn a heartbeat that logs elapsed time every 30 s while the
            // download is running.  This keeps docker logs alive and sends WS
            // events so the UI indeterminate bar stays live.
            let hb_id = id_clone.clone();
            let hb_events = events_clone.clone();
            let start = std::time::Instant::now();
            let heartbeat = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
                ticker.tick().await; // skip the immediate first tick
                loop {
                    ticker.tick().await;
                    let elapsed = start.elapsed().as_secs();
                    info!("Download in progress for {} (elapsed: {}s)", hb_id, elapsed);
                    let _ = hb_events.send(WsEvent::Progress {
                        id: hb_id.clone(),
                        progress: 0.0,
                        speed: None,
                        eta: Some(format!("{}m elapsed", elapsed / 60)),
                    });
                }
            });

            let result = iplayer::download(opts, move |progress| {
                let id = id_clone.clone();
                let db = db_clone.clone();
                let events = events_clone.clone();

                tokio::spawn(async move {
                    let _ =
                        sqlx::query("UPDATE queue_items SET progress=?, speed=?, eta=? WHERE id=?")
                            .bind(progress.percent)
                            .bind(&progress.speed)
                            .bind(&progress.eta)
                            .bind(&id)
                            .execute(&db)
                            .await;

                    let _ = events.send(WsEvent::Progress {
                        id,
                        progress: progress.percent,
                        speed: progress.speed,
                        eta: progress.eta,
                    });
                });
            })
            .await;

            heartbeat.abort();
            result
        };

        match result {
            Ok(path) => break Ok(path),
            Err(e) => {
                if attempt >= max_retries {
                    break Err(e);
                }
                attempt += 1;
                let delay_secs = 2u64.pow(attempt);
                warn!(
                    "Download attempt {attempt}/{max_retries} failed for {id}, \
                     retrying in {delay_secs}s: {e:#}"
                );
                let error_msg = format!(
                    "Attempt {attempt}/{max_retries} failed: {e}. Retrying in {delay_secs}s\u{2026}"
                );
                let _ = sqlx::query("UPDATE queue_items SET error=? WHERE id=?")
                    .bind(&error_msg)
                    .bind(&id)
                    .execute(&db)
                    .await;
                // Push the error to the UI immediately — don't wait for the 5 s poller
                let _ = events.send(WsEvent::Error {
                    id: id.clone(),
                    message: error_msg,
                });
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                // Clear the stale error and signal a fresh attempt is starting
                let _ = sqlx::query("UPDATE queue_items SET error=NULL WHERE id=?")
                    .bind(&id)
                    .execute(&db)
                    .await;
                let _ = events.send(WsEvent::StatusChange {
                    id: id.clone(),
                    status: DownloadStatus::Downloading.to_string(),
                });
            }
        }
    };

    let completed_at = chrono::Utc::now().to_rfc3339();

    match final_result {
        Ok(output_path) => {
            info!("Download complete for {id}: {output_path}");

            // Check if it was cancelled while running
            let current: Option<(String,)> =
                sqlx::query_as("SELECT status FROM queue_items WHERE id=?")
                    .bind(&id)
                    .fetch_optional(&db)
                    .await
                    .unwrap_or(None);

            if current.map(|(s,)| s) == Some(DownloadStatus::Cancelled.to_string()) {
                // Clean up the file written during a cancelled download
                if !output_path.is_empty() {
                    if let Err(e) = tokio::fs::remove_file(&output_path).await {
                        warn!("Could not delete cancelled download file {output_path}: {e}");
                    }
                }
                return;
            }

            let _ = sqlx::query(
                "UPDATE queue_items \
                 SET status='done', completed_at=?, progress=100, output_path=?, error=NULL \
                 WHERE id=?",
            )
            .bind(&completed_at)
            .bind(if output_path.is_empty() {
                None
            } else {
                Some(output_path)
            })
            .bind(&id)
            .execute(&db)
            .await;

            let _ = events.send(WsEvent::StatusChange {
                id,
                status: DownloadStatus::Done.to_string(),
            });
        }
        Err(e) => {
            error!("Download failed for {id} after {max_retries} retries: {e:#}");

            let _ = sqlx::query(
                "UPDATE queue_items SET status='failed', completed_at=?, error=? WHERE id=?",
            )
            .bind(&completed_at)
            .bind(e.to_string())
            .bind(&id)
            .execute(&db)
            .await;

            let _ = events.send(WsEvent::Error {
                id: id.clone(),
                message: e.to_string(),
            });
            let _ = events.send(WsEvent::StatusChange {
                id,
                status: DownloadStatus::Failed.to_string(),
            });
        }
    }
}
