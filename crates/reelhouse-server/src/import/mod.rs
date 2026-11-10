//! Importing users' watch state from other media servers.

pub mod plex;

use axum::Json;
use axum::extract::State;
use reelhouse_api::{ImportReport, PlexImportRequest};
use url::Url;

use crate::app::AppState;
use crate::auth::{AdminUser, now};
use crate::error::{ApiError, ApiResult};
use plex::{Kind, LocalItem, Plex, match_entries};

/// Plex objects Reelhouse has no model for yet: (path, `MediaContainer` field, name).
const NOT_IMPORTED: &[(&str, &str, &str)] = &[
    ("livetv/dvrs", "DVR", "DVR"),
    ("media/subscriptions", "MediaSubscription", "recording rule"),
];

type ItemRow = (
    i64,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i64>,
);

/// Imports the Plex token owner's watch state into one Reelhouse user. The token is used
/// for this request only: never stored, never logged.
pub async fn plex_import(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<PlexImportRequest>,
) -> ApiResult<Json<ImportReport>> {
    let base =
        Url::parse(&req.url).map_err(|e| ApiError::BadRequest(format!("invalid Plex url: {e}")))?;
    sqlx::query("SELECT 1 FROM users WHERE id = ?")
        .bind(req.user_id)
        .fetch_one(&state.db)
        .await?;
    let plex = Plex::new(base, req.token);
    let all = plex
        .entries()
        .await
        .map_err(|e| ApiError::BadGateway(format!("Plex request failed: {e:#}")))?;
    let (entries, unwatched): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|e| e.view_offset_ms > 0 || e.view_count > 0);

    let rows: Vec<ItemRow> = sqlx::query_as(
        "SELECT id, kind, file_path, tmdb_id, imdb_id, tvdb_id FROM items WHERE kind IN ('movie', 'episode')",
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<LocalItem> = rows
        .into_iter()
        .map(
            |(id, kind, file_path, tmdb_id, imdb_id, tvdb_id)| LocalItem {
                id,
                kind: if kind == "movie" {
                    Kind::Movie
                } else {
                    Kind::Episode
                },
                file_path,
                tmdb_id,
                imdb_id,
                tvdb_id,
            },
        )
        .collect();
    let m = match_entries(&entries, &req.path_rewrites, &items);

    // Re-runs only move forward: the larger position wins and played is never cleared.
    let mut tx = state.db.begin().await?;
    let mut written = 0;
    for (item, e) in &m.matched {
        written += sqlx::query(
            "INSERT INTO progress (user_id, item_id, position_s, played, updated_at) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (user_id, item_id) DO UPDATE SET position_s = MAX(position_s, excluded.position_s), \
             played = MAX(played, excluded.played), updated_at = excluded.updated_at \
             WHERE excluded.position_s > progress.position_s OR excluded.played > progress.played",
        )
        .bind(req.user_id)
        .bind(item)
        .bind(e.view_offset_ms as f64 / 1000.0)
        .bind(e.view_count > 0)
        .bind(e.last_viewed_at.unwrap_or_else(now))
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    tx.commit().await?;

    let mut notes = vec![
        "Imported the Plex token owner's watch state only. Plex Home and shared users are not \
         imported: their watch state needs a per-user token, which this import does not request."
            .to_string(),
        format!(
            "{} Plex items had no watch state and were skipped.",
            unwatched.len()
        ),
    ];
    for (path, field, name) in NOT_IMPORTED {
        match plex.count(path, field).await {
            Ok(0) => {}
            Ok(n) => notes.push(format!(
                "Plex has {n} {name}(s); not imported: Reelhouse has no Live TV yet."
            )),
            Err(_) => notes.push(format!("Could not read Plex {name}s; none imported.")),
        }
    }
    Ok(Json(ImportReport {
        matched: m.matched.len() as u64,
        unmatched: m.unmatched,
        progress_written: written,
        notes,
    }))
}
