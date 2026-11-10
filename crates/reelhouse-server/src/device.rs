//! Sign-in by code for TVs: the device shows a short code, a signed-in user approves it from a
//! browser, and the device collects its own session with a secret only it holds.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use reelhouse_api::{DeviceApprove, DeviceCode, DevicePoll, LoginResponse, User};

use crate::app::AppState;
use crate::auth::{CurrentUser, new_token, now, token_hash};
use crate::error::{ApiError, ApiResult};
use crate::users::new_session;

/// Codes last ten minutes.
const LIFETIME_S: i64 = 600;
/// No 0/O or 1/I/L, so codes read cleanly off a TV.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

fn new_code() -> String {
    let raw: String = (0..8)
        .map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char)
        .collect();
    format!("{}-{}", &raw[..4], &raw[4..])
}

/// Accepts codes as people type them: any case, with or without the dash or spaces.
fn normalise(code: &str) -> String {
    let raw: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    if raw.len() == 8 {
        format!("{}-{}", &raw[..4], &raw[4..])
    } else {
        raw
    }
}

pub async fn start(State(state): State<AppState>) -> ApiResult<Json<DeviceCode>> {
    sqlx::query("DELETE FROM device_codes WHERE expires_at < ?")
        .bind(now())
        .execute(&state.db)
        .await?;
    let poll_token = new_token();
    let code = new_code();
    sqlx::query("INSERT INTO device_codes (code, poll_hash, expires_at) VALUES (?, ?, ?)")
        .bind(&code)
        .bind(token_hash(&poll_token))
        .bind(now() + LIFETIME_S)
        .execute(&state.db)
        .await?;
    Ok(Json(DeviceCode {
        code,
        poll_token,
        expires_in: LIFETIME_S as u64,
    }))
}

pub async fn approve(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<DeviceApprove>,
) -> ApiResult<StatusCode> {
    let done = sqlx::query("UPDATE device_codes SET user_id = ? WHERE code = ? AND user_id IS NULL AND expires_at >= ?")
        .bind(user.id)
        .bind(normalise(&req.code))
        .bind(now())
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 202 while waiting, 200 with a session once approved (the code is then used up), 410 if expired.
pub async fn poll(
    State(state): State<AppState>,
    Json(req): Json<DevicePoll>,
) -> ApiResult<Response> {
    let row: Option<(String, Option<i64>, i64)> =
        sqlx::query_as("SELECT code, user_id, expires_at FROM device_codes WHERE poll_hash = ?")
            .bind(token_hash(&req.poll_token))
            .fetch_optional(&state.db)
            .await?;
    let Some((code, user_id, expires_at)) = row else {
        return Ok(StatusCode::GONE.into_response());
    };
    match user_id {
        _ if expires_at < now() => Ok(StatusCode::GONE.into_response()),
        None => Ok(StatusCode::ACCEPTED.into_response()),
        Some(user_id) => {
            sqlx::query("DELETE FROM device_codes WHERE code = ?")
                .bind(&code)
                .execute(&state.db)
                .await?;
            let (name, is_admin): (String, bool) =
                sqlx::query_as("SELECT name, is_admin FROM users WHERE id = ?")
                    .bind(user_id)
                    .fetch_one(&state.db)
                    .await?;
            let token = new_session(&state.db, user_id).await?;
            Ok(Json(LoginResponse {
                token,
                user: User {
                    id: user_id,
                    name,
                    is_admin,
                },
            })
            .into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_readable() {
        let code = new_code();
        assert_eq!(code.len(), 9);
        assert_eq!(&code[4..5], "-");
        assert!(
            code.chars()
                .all(|c| c == '-' || ALPHABET.contains(&(c as u8)))
        );
    }

    #[test]
    fn normalises_typed_codes() {
        assert_eq!(normalise("k7qm 4xpd"), "K7QM-4XPD");
        assert_eq!(normalise("K7QM-4XPD"), "K7QM-4XPD");
    }
}
