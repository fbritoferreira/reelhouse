//! On-demand films and series from Live TV sources: browsing by group, and playback.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use reelhouse_api::{
    Capabilities, PlaybackMethod, PlaybackPlan, Progress, VodEpisode, VodGroup, VodMovie, VodShow,
    VodTitle,
};
use serde::Deserialize;

use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::stream::decide::decide;

/// Matches titles in group `?1`; '' asks for the ungrouped.
const IN_GROUP: &str = "(group_title = ?1 OR (?1 = '' AND group_title IS NULL))";

#[derive(Deserialize)]
pub struct KindQuery {
    kind: String,
}

/// Groups of films (`kind=movies`) or of shows (`kind=series`), with how many each holds.
pub async fn groups(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<KindQuery>,
) -> ApiResult<Json<Vec<VodGroup>>> {
    let counted = match q.kind.as_str() {
        "movies" => "COUNT(*) FROM vod_items WHERE kind = 'movie'",
        "series" => "COUNT(DISTINCT show_title) FROM vod_items WHERE kind = 'episode'",
        _ => return Err(ApiError::BadRequest("kind is movies or series".into())),
    };
    let rows: Vec<(Option<String>, i64)> = sqlx::query_as(&format!(
        "SELECT group_title, {counted} GROUP BY group_title ORDER BY group_title IS NULL, MIN(id)"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(name, titles)| VodGroup { name, titles })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct GroupQuery {
    group: String,
}

pub async fn movies(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Query(q): Query<GroupQuery>,
) -> ApiResult<Json<Vec<VodMovie>>> {
    let rows: Vec<TitleRow> = sqlx::query_as(&format!(
        "{TITLE_SELECT} WHERE v.kind = 'movie' AND {IN_GROUP} ORDER BY v.id"
    ))
    .bind(&q.group)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| VodMovie {
                progress: r.progress(),
                id: r.id,
                title: r.title,
                year: r.year,
                poster_url: r.poster_url,
                duration_s: r.duration_s,
            })
            .collect(),
    ))
}

pub async fn series(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<GroupQuery>,
) -> ApiResult<Json<Vec<VodShow>>> {
    let rows: Vec<(String, Option<String>, i64, i64)> = sqlx::query_as(&format!(
        "SELECT show_title, MIN(poster_url), COUNT(DISTINCT season_no), COUNT(*) FROM vod_items \
         WHERE kind = 'episode' AND show_title IS NOT NULL AND {IN_GROUP} \
         GROUP BY show_title ORDER BY MIN(id)"
    ))
    .bind(&q.group)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(title, poster_url, seasons, episodes)| VodShow {
                title,
                poster_url,
                seasons,
                episodes,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct ShowQuery {
    show: String,
}

pub async fn episodes(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Query(q): Query<ShowQuery>,
) -> ApiResult<Json<Vec<VodEpisode>>> {
    let rows: Vec<TitleRow> = sqlx::query_as(&format!(
        "{TITLE_SELECT} WHERE v.kind = 'episode' AND v.show_title = ?1 ORDER BY v.season_no, v.episode_no, v.id"
    ))
    .bind(&q.show)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| VodEpisode {
                progress: r.progress(),
                id: r.id,
                title: r.title,
                season_no: r.season_no,
                episode_no: r.episode_no,
                poster_url: r.poster_url,
                duration_s: r.duration_s,
            })
            .collect(),
    ))
}

/// One title with where you are in it.
pub async fn title(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<VodTitle>> {
    let r: TitleRow = sqlx::query_as(&format!("{TITLE_SELECT} WHERE v.id = ?1"))
        .bind(id)
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(r.into_title()))
}

/// Titles started and not finished, most recent first.
pub async fn continue_watching(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> ApiResult<Json<Vec<VodTitle>>> {
    let rows: Vec<TitleRow> = sqlx::query_as(&format!(
        "{TITLE_SELECT} WHERE ?1 IS NULL AND p.position_s > 0 AND NOT p.played ORDER BY p.updated_at DESC LIMIT 20"
    ))
    .bind(None::<i64>)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(TitleRow::into_title).collect()))
}

/// Watched past this share of its length, a title counts as played.
const PLAYED_FRACTION: f64 = 0.92;

pub async fn put_progress(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    Json(p): Json<Progress>,
) -> ApiResult<StatusCode> {
    let (duration,): (Option<f64>,) =
        sqlx::query_as("SELECT duration_s FROM vod_items WHERE id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let played =
        p.played || duration.is_some_and(|d| d > 0.0 && p.position_s >= PLAYED_FRACTION * d);
    sqlx::query(
        "INSERT INTO vod_progress (user_id, vod_id, position_s, played, updated_at) VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (user_id, vod_id) DO UPDATE SET position_s = excluded.position_s, \
         played = excluded.played, updated_at = excluded.updated_at",
    )
    .bind(user.id)
    .bind(id)
    .bind(p.position_s.max(0.0))
    .bind(played)
    .bind(crate::auth::now())
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A title joined with the asking person's progress. Binds: `?1` the filter value, `?2` the user.
const TITLE_SELECT: &str = "SELECT v.id, v.title, v.year, v.show_title, v.season_no, v.episode_no, v.poster_url, \
     v.duration_s, p.position_s, p.played FROM vod_items v \
     LEFT JOIN vod_progress p ON p.vod_id = v.id AND p.user_id = ?2";

#[derive(sqlx::FromRow)]
struct TitleRow {
    id: i64,
    title: String,
    year: Option<i64>,
    show_title: Option<String>,
    season_no: Option<i64>,
    episode_no: Option<i64>,
    poster_url: Option<String>,
    duration_s: Option<f64>,
    position_s: Option<f64>,
    played: Option<bool>,
}

impl TitleRow {
    fn progress(&self) -> Option<Progress> {
        self.position_s.map(|position_s| Progress {
            position_s,
            played: self.played.unwrap_or(false),
        })
    }

    fn into_title(self) -> VodTitle {
        VodTitle {
            progress: self.progress(),
            id: self.id,
            title: self.title,
            show_title: self.show_title,
            season_no: self.season_no,
            episode_no: self.episode_no,
            poster_url: self.poster_url,
            duration_s: self.duration_s,
        }
    }
}

#[derive(Deserialize)]
pub struct StartAt {
    start: Option<f64>,
}

/// Plays an on-demand title through HLS: the provider's file is probed, then remuxed, or
/// converted when this client cannot decode it. It is never handed out directly, since its
/// URL carries the provider login.
pub async fn playback(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
    Query(at): Query<StartAt>,
    Json(caps): Json<Capabilities>,
) -> ApiResult<Json<PlaybackPlan>> {
    let (url,): (String,) = sqlx::query_as("SELECT stream_url FROM vod_items WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let info = crate::scan::probe::probe(std::path::Path::new(&url))
        .await
        .map_err(|e| ApiError::BadGateway(format!("the title would not open: {e:#}")))?;
    // Remembered so progress can tell when a title has been watched to the end.
    if let Some(d) = info.duration_s {
        sqlx::query("UPDATE vod_items SET duration_s = ? WHERE id = ?")
            .bind(d)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    let method = match decide(&info, &caps) {
        PlaybackMethod::Direct => PlaybackMethod::Remux,
        m => m,
    };
    let start_s = at
        .start
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(0.0);
    let session = state.hls.start(&url, method, caps.max_height, start_s)?;
    state
        .hls
        .ready(&session, std::time::Duration::from_secs(30))
        .await
        .map_err(|why| ApiError::BadGateway(format!("the title did not start: {why}")))?;
    Ok(Json(PlaybackPlan {
        method,
        url: format!("/api/v1/hls/{session}/index.m3u8"),
        start_s,
        duration_s: info.duration_s,
    }))
}
