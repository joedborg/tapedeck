use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::{
    extract::{FromRequestParts, State},
    http::{HeaderMap, StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};

use crate::{
    models::{LoginRequest, LoginResponse, User},
    state::AppState,
};

// ── Password hashing ───────────────────────────────────────────────────────────

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt_bytes: [u8; 16] = rand::random();
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| anyhow::anyhow!("salt: {e}"))?;
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hash_password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed = PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("parse hash: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

// ── Session token ──────────────────────────────────────────────────────────────
// We use a simple HMAC-SHA256 token: base64(user_id + ":" + timestamp + ":" + hmac).
// For production you'd swap this for JWT / sessions stored in DB.

fn make_token(user_id: &str, secret: &str) -> String {
    let payload = format!("{user_id}:{}", chrono::Utc::now().timestamp());
    let mac = hmac_sha256(secret, &payload);
    let token = format!("{payload}:{mac}");
    STANDARD.encode(token)
}

fn hmac_sha256(secret: &str, data: &str) -> String {
    use sha2::{Digest, Sha256};
    // Simple keyed hash: SHA256(secret || data)
    let mut h = Sha256::new();
    h.update(secret.as_bytes());
    h.update(b":");
    h.update(data.as_bytes());
    hex::encode(h.finalize())
}

pub fn verify_token(token: &str, secret: &str) -> Option<String> {
    let decoded = STANDARD.decode(token).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    if parts.len() != 3 {
        return None;
    }
    let (user_id, ts, sig) = (parts[0], parts[1], parts[2]);
    let payload = format!("{user_id}:{ts}");
    let expected = hmac_sha256(secret, &payload);
    if sig != expected {
        return None;
    }
    Some(user_id.to_string())
}

// ── Login handler ──────────────────────────────────────────────────────────────

pub async fn login_handler(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<LoginRequest>,
) -> crate::error::Result<axum::Json<LoginResponse>> {
    let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&state.db)
        .await?;

    let user = user.ok_or(crate::error::AppError::Unauthorized)?;

    let valid = verify_password(&req.password, &user.password)
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;
    if !valid {
        return Err(crate::error::AppError::Unauthorized);
    }

    let token = make_token(&user.id, &state.config.secret);
    Ok(axum::Json(LoginResponse {
        token,
        user_id: user.id,
        username: user.username,
    }))
}

// ── Extractor: authenticated user ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Accept token from Authorization: Bearer <token> or X-Auth-Token header.
        let token = extract_bearer(&parts.headers)
            .or_else(|| {
                parts
                    .headers
                    .get("x-auth-token")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            })
            .ok_or(AuthRejection)?;

        let user_id = verify_token(&token, &state.config.secret).ok_or(AuthRejection)?;

        let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE id = ?")
            .bind(&user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| AuthRejection)?;

        user.map(AuthUser).ok_or(AuthRejection)
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get("authorization")?.to_str().ok()?;
    v.strip_prefix("Bearer ").map(|s| s.to_string())
}

#[derive(Debug)]
pub struct AuthRejection;

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response()
    }
}
