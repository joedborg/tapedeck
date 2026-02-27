use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;

use crate::{
    auth::{hash_password, AuthUser},
    error::{AppError, Result},
    models::{CreateUserRequest, User},
    state::AppState,
};

#[derive(Serialize)]
pub struct UserView {
    pub id: String,
    pub username: String,
    pub created_at: String,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        UserView {
            id: u.id,
            username: u.username,
            created_at: u.created_at,
        }
    }
}

/// GET /api/users  (admin: lists all users)
pub async fn list_users(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserView>>> {
    let users: Vec<User> = sqlx::query_as("SELECT * FROM users ORDER BY created_at")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(users.into_iter().map(UserView::from).collect()))
}

/// GET /api/users/me
pub async fn get_me(AuthUser(user): AuthUser) -> Json<UserView> {
    Json(UserView::from(user))
}

/// POST /api/users
pub async fn create_user(
    AuthUser(_caller): AuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserView>)> {
    if req.username.trim().is_empty() {
        return Err(AppError::BadRequest("username cannot be empty".into()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }

    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM users WHERE username=?")
            .bind(&req.username)
            .fetch_optional(&state.db)
            .await?;
    if existing.is_some() {
        return Err(AppError::Conflict(format!(
            "username '{}' already exists",
            req.username
        )));
    }

    let id = User::new_id();
    let hash = hash_password(&req.password)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query("INSERT INTO users (id, username, password) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(&req.username)
        .bind(&hash)
        .execute(&state.db)
        .await?;

    let user: User = sqlx::query_as("SELECT * FROM users WHERE id=?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    Ok((StatusCode::CREATED, Json(UserView::from(user))))
}

/// DELETE /api/users/:id
pub async fn delete_user(
    AuthUser(caller): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    if caller.id == id {
        return Err(AppError::BadRequest("cannot delete yourself".into()));
    }

    let result = sqlx::query("DELETE FROM users WHERE id=?")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/users/:id/password
#[derive(serde::Deserialize)]
pub struct ChangePasswordRequest {
    pub new_password: String,
}

pub async fn change_password(
    AuthUser(caller): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<StatusCode> {
    // Users can only change their own password (extend with admin role if needed)
    if caller.id != id {
        return Err(AppError::Forbidden);
    }
    if req.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }

    let hash = hash_password(&req.new_password)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query("UPDATE users SET password=?, updated_at=datetime('now') WHERE id=?")
        .bind(&hash)
        .bind(&id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}
