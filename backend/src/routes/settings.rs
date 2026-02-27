use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    auth::AuthUser,
    error::{AppError, Result},
    models::Setting,
    state::AppState,
};

/// GET /api/settings
pub async fn list_settings(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Setting>>> {
    let settings: Vec<Setting> = sqlx::query_as("SELECT * FROM settings ORDER BY key")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(settings))
}

/// GET /api/settings/:key
pub async fn get_setting(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<Setting>> {
    let setting: Option<Setting> = sqlx::query_as("SELECT * FROM settings WHERE key=?")
        .bind(&key)
        .fetch_optional(&state.db)
        .await?;

    setting.map(Json).ok_or(AppError::NotFound)
}

#[derive(Debug, Deserialize)]
pub struct SetSettingRequest {
    pub value: String,
}

/// PUT /api/settings/:key
pub async fn set_setting(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<SetSettingRequest>,
) -> Result<Json<Setting>> {
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
    )
    .bind(&key)
    .bind(&req.value)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let setting: Setting = sqlx::query_as("SELECT * FROM settings WHERE key=?")
        .bind(&key)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(setting))
}

/// PATCH /api/settings  — bulk update
pub async fn bulk_update_settings(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
    Json(updates): Json<std::collections::HashMap<String, String>>,
) -> Result<StatusCode> {
    let now = chrono::Utc::now().to_rfc3339();
    for (key, value) in updates {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        )
        .bind(&key)
        .bind(&value)
        .bind(&now)
        .execute(&state.db)
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
