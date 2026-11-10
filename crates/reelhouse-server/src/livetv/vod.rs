//! On-demand films and series from Live TV sources: browsing by group, and playback.

use axum::Json;
use axum::extract::{Path, Query, State};
use reelhouse_api::{
    Capabilities, PlaybackMethod, PlaybackPlan, VodEpisode, VodGroup, VodMovie, VodShow,
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
    _user: CurrentUser,
    Query(q): Query<GroupQuery>,
) -> ApiResult<Json<Vec<VodMovie>>> {
    let rows: Vec<(i64, String, Option<i64>, Option<String>)> = sqlx::query_as(&format!(
        "SELECT id, title, year, poster_url FROM vod_items WHERE kind = 'movie' AND {IN_GROUP} ORDER BY id"
    ))
    .bind(&q.group)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, title, year, poster_url)| VodMovie {
                id,
                title,
                year,
                poster_url,
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

#[derive(sqlx::FromRow)]
struct EpisodeRow {
    id: i64,
    title: String,
    season_no: Option<i64>,
    episode_no: Option<i64>,
    poster_url: Option<String>,
}

pub async fn episodes(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<ShowQuery>,
) -> ApiResult<Json<Vec<VodEpisode>>> {
    let rows: Vec<EpisodeRow> = sqlx::query_as(
        "SELECT id, title, season_no, episode_no, poster_url FROM vod_items \
         WHERE kind = 'episode' AND show_title = ? ORDER BY season_no, episode_no, id",
    )
    .bind(&q.show)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| VodEpisode {
                id: r.id,
                title: r.title,
                season_no: r.season_no,
                episode_no: r.episode_no,
                poster_url: r.poster_url,
            })
            .collect(),
    ))
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
