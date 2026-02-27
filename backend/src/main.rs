mod auth;
mod config;
mod db;
mod error;
mod iplayer;
mod models;
mod queue;
mod routes;
mod state;

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{models::WsEvent, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Logging ──────────────────────────────────────────────────────────────
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tapedeck=info,tower_http=info".into()),
        )
        .with(fmt::layer())
        .init();

    // ── Config ───────────────────────────────────────────────────────────────
    let config = config::AppConfig::from_env()?;
    let config = Arc::new(config);
    info!("Starting tapedeck, binding to {}", config.bind);

    // Ensure output directory exists
    tokio::fs::create_dir_all(&config.output_dir).await?;

    // ── Database ─────────────────────────────────────────────────────────────
    let db = db::connect(&config).await?;
    db::seed_admin(&db, &config).await?;

    // ── WebSocket broadcast channel ───────────────────────────────────────────
    let (events_tx, _) = broadcast::channel::<WsEvent>(256);

    // ── Download worker pool ──────────────────────────────────────────────────
    let queue = queue::start_worker_pool(db.clone(), Arc::clone(&config), events_tx.clone());

    // ── Application state ─────────────────────────────────────────────────────
    let state = AppState {
        db,
        config: Arc::clone(&config),
        queue,
        events: events_tx,
    };

    // ── Scheduled-item watcher ────────────────────────────────────────────────
    // Every minute, check for items whose scheduled_at has passed and enqueue them.
    {
        let state_clone = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                enqueue_scheduled(&state_clone).await;
            }
        });
    }

    // ── Cache refresh (every hour, not on startup) ────────────────────────────
    // The local cache is only a fallback when BBC web search is unavailable;
    // refreshing it at startup delays the server for no practical benefit.
    {
        let iplayer_path = config.get_iplayer_path.clone();
        let cache_dir = config.iplayer_cache_dir.clone();
        tokio::spawn(async move {
            let start = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3600);
            let mut interval =
                tokio::time::interval_at(start, tokio::time::Duration::from_secs(3600));
            loop {
                interval.tick().await;
                info!("Refreshing get_iplayer TV cache…");
                if let Err(e) = iplayer::refresh_cache(&iplayer_path, "tv", &cache_dir).await {
                    tracing::warn!("TV cache refresh failed: {e:#}");
                } else {
                    info!("TV cache refresh complete");
                }
                info!("Refreshing get_iplayer radio cache…");
                if let Err(e) = iplayer::refresh_cache(&iplayer_path, "radio", &cache_dir).await {
                    tracing::warn!("Radio cache refresh failed: {e:#}");
                } else {
                    info!("Radio cache refresh complete");
                }
            }
        });
    }

    // ── HTTP server ───────────────────────────────────────────────────────────
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "/app/ui/dist".to_string());
    let router = routes::build_router(state, &static_dir);

    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    info!("Listening on http://{}", config.bind);

    axum::serve(listener, router).await?;

    Ok(())
}

/// Enqueue any queue items whose `scheduled_at` is in the past and whose status
/// is still `queued`.
async fn enqueue_scheduled(state: &AppState) {
    let now = chrono::Utc::now().to_rfc3339();

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM queue_items \
         WHERE status='queued' AND scheduled_at IS NOT NULL AND scheduled_at <= ?",
    )
    .bind(&now)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for (id,) in rows {
        tracing::info!("Enqueuing scheduled item {id}");
        state.queue.enqueue(id);
    }
}
