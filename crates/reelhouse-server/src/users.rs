use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use reelhouse_api::{LoginRequest, LoginResponse, NewUser, ServerInfo, User};

use crate::app::{AppState, FEATURES};
use crate::auth::{
    AdminUser, CurrentUser, hash_password, new_token, now, token_hash, verify_password,
};
use crate::error::{ApiError, ApiResult};

pub async fn server_info() -> Json<ServerInfo> {
    Json(ServerInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        features: FEATURES.iter().map(|f| (*f).into()).collect(),
    })
}

fn validate(new: &NewUser) -> ApiResult<()> {
    if new.name.trim().is_empty() || new.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "name required and password at least 8 characters".into(),
        ));
    }
    Ok(())
}

fn unique(e: sqlx::Error) -> ApiError {
    match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => ApiError::Conflict("user exists"),
        e => e.into(),
    }
}

async fn insert_user(state: &AppState, new: &NewUser, is_admin: bool) -> ApiResult<User> {
    validate(new)?;
    let id = sqlx::query(
        "INSERT INTO users (name, password_hash, is_admin, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(new.name.trim())
    .bind(hash_password(&new.password)?)
    .bind(is_admin)
    .bind(now())
    .execute(&state.db)
    .await
    .map_err(unique)?
    .last_insert_rowid();
    Ok(User {
        id,
        name: new.name.trim().into(),
        is_admin,
    })
}

/// First-run setup: creates the admin. One conditional insert, so two racing setups cannot both win.
pub async fn setup(
    State(state): State<AppState>,
    Json(new): Json<NewUser>,
) -> ApiResult<Json<User>> {
    validate(&new)?;
    let done = sqlx::query(
        "INSERT INTO users (name, password_hash, is_admin, created_at) \
         SELECT ?, ?, 1, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(new.name.trim())
    .bind(hash_password(&new.password)?)
    .bind(now())
    .execute(&state.db)
    .await
    .map_err(unique)?;
    if done.rows_affected() == 0 {
        return Err(ApiError::Conflict("setup already done"));
    }
    Ok(Json(User {
        id: done.last_insert_rowid(),
        name: new.name.trim().into(),
        is_admin: true,
    }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    let row: Option<(i64, String, String, bool)> =
        sqlx::query_as("SELECT id, name, password_hash, is_admin FROM users WHERE name = ?")
            .bind(&req.name)
            .fetch_optional(&state.db)
            .await?;
    let (id, name, hash, is_admin) = row.ok_or(ApiError::Unauthorized)?;
    if !verify_password(&req.password, &hash) {
        return Err(ApiError::Unauthorized);
    }
    let token = new_token();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, created_at) VALUES (?, ?, ?)")
        .bind(token_hash(&token))
        .bind(id)
        .bind(now())
        .execute(&state.db)
        .await?;
    Ok(Json(LoginResponse {
        token,
        user: User { id, name, is_admin },
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    _user: CurrentUser,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash(token))
            .execute(&state.db)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(CurrentUser(user): CurrentUser) -> Json<User> {
    Json(user)
}

pub async fn list(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<User>>> {
    let rows: Vec<(i64, String, bool)> =
        sqlx::query_as("SELECT id, name, is_admin FROM users ORDER BY name")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, is_admin)| User { id, name, is_admin })
            .collect(),
    ))
}

pub async fn create(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(new): Json<NewUser>,
) -> ApiResult<(StatusCode, Json<User>)> {
    Ok((
        StatusCode::CREATED,
        Json(insert_user(&state, &new, new.is_admin).await?),
    ))
}

pub async fn remove(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if id == admin.id {
        return Err(ApiError::BadRequest("cannot delete yourself".into()));
    }
    let done = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
