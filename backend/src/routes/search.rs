use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;

use crate::{
    auth::AuthUser,
    error::Result,
    iplayer::{self, EpisodesOptions, SearchOptions},
    models::SearchResult,
    state::AppState,
};

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_type")]
    pub r#type: String,
}

fn default_type() -> String {
    "tv".to_string()
}

/// GET /api/search?q=...&type=tv|radio
pub async fn search(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>> {
    let opts = SearchOptions {
        query: &params.q,
        media_type: &params.r#type,
        get_iplayer_path: &state.config.get_iplayer_path,
        cache_dir: &state.config.iplayer_cache_dir,
        proxy: state.config.proxy.as_deref(),
    };

    let results = iplayer::search(opts)
        .await
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;

    Ok(Json(results))
}

/// GET /api/search/episodes?pid=...&type=tv|radio
/// Lists all episodes for a brand/series PID via get_iplayer --pid-recursive-list.
#[derive(Deserialize)]
pub struct EpisodesQuery {
    pub pid: String,
    #[serde(default = "default_type")]
    pub r#type: String,
}

pub async fn list_episodes(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<EpisodesQuery>,
) -> Result<Json<Vec<SearchResult>>> {
    let opts = EpisodesOptions {
        pid: &params.pid,
        media_type: &params.r#type,
        get_iplayer_path: &state.config.get_iplayer_path,
        cache_dir: &state.config.iplayer_cache_dir,
        proxy: state.config.proxy.as_deref(),
    };

    let results = iplayer::list_episodes(opts)
        .await
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;

    Ok(Json(results))
}

/// POST /api/search/refresh  — refresh the get_iplayer programme cache
#[derive(Deserialize)]
pub struct RefreshBody {
    #[serde(default = "default_type")]
    pub r#type: String,
}

pub async fn refresh_cache(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<RefreshBody>,
) -> Result<axum::http::StatusCode> {
    let path = state.config.get_iplayer_path.clone();
    let media_type = body.r#type.clone();
    let cache_dir = state.config.iplayer_cache_dir.clone();

    // Run in background — returns 202 Accepted immediately
    tokio::spawn(async move {
        if let Err(e) = iplayer::refresh_cache(&path, &media_type, &cache_dir).await {
            tracing::warn!("Cache refresh failed: {e:#}");
        }
    });

    Ok(axum::http::StatusCode::ACCEPTED)
}
