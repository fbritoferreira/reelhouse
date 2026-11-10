//! Password hashing, session tokens and the `CurrentUser` / `AdminUser` extractors.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reelhouse_api::User;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::app::AppState;
use crate::error::{ApiError, ApiResult};

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt =
        SaltString::encode_b64(&rand::random::<[u8; 16]>()).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

pub fn new_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn user_for_token(db: &SqlitePool, token: &str) -> ApiResult<User> {
    let row: Option<(i64, String, bool)> = sqlx::query_as(
        "SELECT u.id, u.name, u.is_admin FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ?",
    )
    .bind(token_hash(token))
    .fetch_optional(db)
    .await?;
    let (id, name, is_admin) = row.ok_or(ApiError::Unauthorized)?;
    Ok(User { id, name, is_admin })
}

/// Bearer header, or `?token=` for players that cannot set headers.
fn token_from(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned)
        .or_else(|| {
            parts
                .uri
                .query()?
                .split('&')
                .find_map(|kv| kv.strip_prefix("token=").map(str::to_owned))
        })
}

pub struct CurrentUser(pub User);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let token = token_from(parts).ok_or(ApiError::Unauthorized)?;
        Ok(CurrentUser(user_for_token(&state.db, &token).await?))
    }
}

pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;
        if !user.is_admin {
            return Err(ApiError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
