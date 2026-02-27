use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("not found")]
    NotFound,

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Db(e) => {
                tracing::error!("DB error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database error".to_string(),
                )
            }
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            AppError::Internal(m) => {
                tracing::error!("Internal error: {m}");
                (StatusCode::INTERNAL_SERVER_ERROR, m.clone())
            }
            AppError::Anyhow(e) => {
                tracing::error!("Unhandled error: {e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type Result<T, E = AppError> = std::result::Result<T, E>;
