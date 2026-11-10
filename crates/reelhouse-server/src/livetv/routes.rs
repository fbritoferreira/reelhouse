use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use reelhouse_api::{
    Channel, GuideChannel, NewTvSource, PlaybackMethod, PlaybackPlan, Programme, RefreshReport,
    TvSource, TvSourceKind,
};
use serde::Deserialize;
use sqlx::FromRow;

use crate::app::AppState;
use crate::auth::{AdminUser, CurrentUser, now};
use crate::error::{ApiError, ApiResult};

#[derive(FromRow)]
struct SourceRow {
    id: i64,
    name: String,
    kind: String,
    url: String,
    username: Option<String>,
    epg_url: Option<String>,
}

impl From<SourceRow> for TvSource {
    fn from(r: SourceRow) -> Self {
        TvSource {
            id: r.id,
            name: r.name,
            kind: if r.kind == "xtream" {
                TvSourceKind::Xtream
            } else {
                TvSourceKind::M3u
            },
            url: r.url,
            username: r.username,
            epg_url: r.epg_url,
        }
    }
}

pub async fn list_sources(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<TvSource>>> {
    let rows: Vec<SourceRow> = sqlx::query_as(
        "SELECT id, name, kind, url, username, epg_url FROM tv_sources ORDER BY name",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(TvSource::from).collect()))
}

pub async fn create_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(new): Json<NewTvSource>,
) -> ApiResult<(StatusCode, Json<TvSource>)> {
    if new.url.trim().is_empty() {
        return Err(ApiError::BadRequest("url is required".into()));
    }
    let kind = match new.kind {
        TvSourceKind::M3u => "m3u",
        TvSourceKind::Xtream => {
            if new.username.is_none() || new.password.is_none() {
                return Err(ApiError::BadRequest(
                    "xtream sources need a username and password".into(),
                ));
            }
            "xtream"
        }
    };
    let id = sqlx::query(
        "INSERT INTO tv_sources (name, kind, url, username, password, epg_url, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&new.name)
    .bind(kind)
    .bind(&new.url)
    .bind(&new.username)
    .bind(&new.password)
    .bind(&new.epg_url)
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    Ok((
        StatusCode::CREATED,
        Json(TvSource {
            id,
            name: new.name,
            kind: new.kind,
            url: new.url,
            username: new.username,
            epg_url: new.epg_url,
        }),
    ))
}

pub async fn refresh_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<RefreshReport>> {
    sqlx::query("SELECT 1 FROM tv_sources WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    // A source that cannot be read is the admin's to fix, so say why.
    super::refresh(&state.db, id)
        .await
        .map(Json)
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))
}

#[derive(FromRow)]
struct ChannelRow {
    id: i64,
    source_id: i64,
    number: Option<i64>,
    name: String,
    logo_url: Option<String>,
    group_title: Option<String>,
}

impl From<ChannelRow> for Channel {
    fn from(r: ChannelRow) -> Self {
        Channel {
            id: r.id,
            source_id: r.source_id,
            number: r.number,
            name: r.name,
            logo_url: r.logo_url,
            group_title: r.group_title,
        }
    }
}

async fn all_channels(state: &AppState) -> ApiResult<Vec<Channel>> {
    let rows: Vec<ChannelRow> = sqlx::query_as(
        "SELECT id, source_id, number, name, logo_url, group_title FROM channels \
         ORDER BY number IS NULL, number, name",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(Channel::from).collect())
}

pub async fn channels(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> ApiResult<Json<Vec<Channel>>> {
    Ok(Json(all_channels(&state).await?))
}

#[derive(Deserialize)]
pub struct Window {
    start: Option<i64>,
    end: Option<i64>,
}

#[derive(FromRow)]
struct ProgrammeRow {
    id: i64,
    channel_id: i64,
    start: i64,
    stop: i64,
    title: String,
    subtitle: Option<String>,
    description: Option<String>,
}

/// Every channel with the programmes that overlap `[start, end)`; defaults to the next 3 hours.
pub async fn guide(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(w): Query<Window>,
) -> ApiResult<Json<Vec<GuideChannel>>> {
    let start = w.start.unwrap_or_else(now);
    let end = w.end.unwrap_or(start + 3 * 3600);
    let rows: Vec<ProgrammeRow> = sqlx::query_as(
        "SELECT id, channel_id, start, stop, title, subtitle, description FROM programmes \
         WHERE stop > ? AND start < ? ORDER BY start",
    )
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;
    let mut out: Vec<GuideChannel> = all_channels(&state)
        .await?
        .into_iter()
        .map(|channel| GuideChannel {
            channel,
            programmes: Vec::new(),
        })
        .collect();
    let index: HashMap<i64, usize> = out
        .iter()
        .enumerate()
        .map(|(i, g)| (g.channel.id, i))
        .collect();
    for r in rows {
        if let Some(&i) = index.get(&r.channel_id) {
            out[i].programmes.push(Programme {
                id: r.id,
                channel_id: r.channel_id,
                start: r.start,
                stop: r.stop,
                title: r.title,
                subtitle: r.subtitle,
                description: r.description,
            });
        }
    }
    Ok(Json(out))
}

/// Remuxes the channel's stream into an HLS session.
pub async fn play(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<PlaybackPlan>> {
    let (url,): (String,) = sqlx::query_as("SELECT stream_url FROM channels WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let method = PlaybackMethod::Remux;
    let session = state.hls.start(&url, method, u32::MAX)?;
    Ok(Json(PlaybackPlan {
        method,
        url: format!("/api/v1/hls/{session}/index.m3u8"),
    }))
}
