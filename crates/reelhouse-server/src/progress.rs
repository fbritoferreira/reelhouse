use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use reelhouse_api::{Item, Progress};

use crate::app::AppState;
use crate::auth::{CurrentUser, now};
use crate::error::ApiResult;
use crate::libraries::{ITEM_SELECT, ItemRow};

/// Played once 90% watched.
const PLAYED_FRACTION: f64 = 0.9;

pub async fn put(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    Json(p): Json<Progress>,
) -> ApiResult<StatusCode> {
    let (duration,): (Option<f64>,) = sqlx::query_as("SELECT duration_s FROM items WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let played =
        p.played || duration.is_some_and(|d| d > 0.0 && p.position_s >= PLAYED_FRACTION * d);
    sqlx::query(
        "INSERT INTO progress (user_id, item_id, position_s, played, updated_at) VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (user_id, item_id) DO UPDATE SET position_s = excluded.position_s, played = excluded.played, \
         updated_at = excluded.updated_at",
    )
    .bind(user.id)
    .bind(id)
    .bind(p.position_s.max(0.0))
    .bind(played)
    .bind(now())
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn continue_watching(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> ApiResult<Json<Vec<Item>>> {
    let rows: Vec<ItemRow> = sqlx::query_as(&format!(
        "{ITEM_SELECT} WHERE p.position_s > 0 AND p.played = 0 ORDER BY p.updated_at DESC, p.rowid DESC LIMIT 20"
    ))
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Item::from).collect()))
}
