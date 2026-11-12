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

/// What to pick up: titles part-watched, and for each show whose latest finished episode has
/// another after it (next season included) that is not started yet, that next one. Most recent
/// first, at most 20.
pub async fn continue_watching(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> ApiResult<Json<Vec<Item>>> {
    // (item, when, rowid): saves in the same second order by which was written last.
    let mut picks: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT item_id, updated_at, rowid FROM progress WHERE user_id = ? AND position_s > 0 AND NOT played",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let next_up: Vec<(i64, i64, i64)> = sqlx::query_as(
        "WITH fin AS ( \
           SELECT s.parent_id AS show, e.season_no AS sn, e.episode_no AS en, p.updated_at AS at, p.rowid AS rid, \
             ROW_NUMBER() OVER (PARTITION BY s.parent_id ORDER BY p.updated_at DESC, p.rowid DESC) AS r \
           FROM progress p JOIN items e ON e.id = p.item_id JOIN items s ON s.id = e.parent_id \
           WHERE p.user_id = ?1 AND p.played AND e.kind = 'episode'), \
         next AS ( \
           SELECT (SELECT n.id FROM items n JOIN items ns ON ns.id = n.parent_id \
             WHERE ns.parent_id = fin.show AND n.kind = 'episode' \
             AND (n.season_no > fin.sn OR (n.season_no = fin.sn AND n.episode_no > fin.en)) \
             ORDER BY n.season_no, n.episode_no LIMIT 1) AS id, fin.at AS at, fin.rid AS rid \
           FROM fin WHERE r = 1) \
         SELECT id, at, rid FROM next WHERE id IS NOT NULL AND NOT EXISTS ( \
           SELECT 1 FROM progress q WHERE q.user_id = ?1 AND q.item_id = next.id \
           AND (q.played OR q.position_s > 0))",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    picks.extend(next_up);
    picks.sort_by(|a, b| (b.1, b.2).cmp(&(a.1, a.2)));
    let mut out = Vec::new();
    for (id, ..) in picks.into_iter().take(20) {
        let row: Option<ItemRow> = sqlx::query_as(&format!("{ITEM_SELECT} WHERE i.id = ?"))
            .bind(user.id)
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
        out.extend(row.map(Item::from));
    }
    Ok(Json(out))
}

/// Films and shows, newest first; a show counts as new when an episode of it is. At most 20.
pub async fn recently_added(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> ApiResult<Json<Vec<Item>>> {
    let rows: Vec<ItemRow> = sqlx::query_as(&format!(
        "{ITEM_SELECT} WHERE i.parent_id IS NULL ORDER BY ( \
           SELECT MAX(d.added_at) FROM items d WHERE d.id = i.id OR d.parent_id = i.id \
           OR d.parent_id IN (SELECT s.id FROM items s WHERE s.parent_id = i.id)) DESC, \
         i.id DESC LIMIT 20"
    ))
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Item::from).collect()))
}
