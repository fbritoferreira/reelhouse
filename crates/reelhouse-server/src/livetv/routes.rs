use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use reelhouse_api::{
    Channel, ChannelGroup, GuideChannel, HiddenGroups, Job, JobKind, NewRecordingRule, NewTvSource,
    PlaybackMethod, PlaybackPlan, Programme, Recording, RecordingRule, RuleKind, SourceGroup,
    TvSource, TvSourceKind,
};
use serde::Deserialize;
use sqlx::FromRow;

use super::dvr;
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

/// Every group of one source, with whether Live TV shows it.
pub async fn source_groups(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<SourceGroup>>> {
    sqlx::query("SELECT 1 FROM tv_sources WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let rows: Vec<(Option<String>, i64, bool)> = sqlx::query_as(&format!(
        "SELECT group_title, COUNT(*), {VISIBLE} FROM channels WHERE source_id = ? \
         GROUP BY group_title ORDER BY group_title IS NULL, MIN(number), group_title"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(name, channels, shown)| SourceGroup {
                name,
                channels,
                shown,
            })
            .collect(),
    ))
}

/// Replaces which of a source's groups are hidden.
pub async fn hide_groups(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
    Json(body): Json<HiddenGroups>,
) -> ApiResult<StatusCode> {
    sqlx::query("SELECT 1 FROM tv_sources WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM hidden_groups WHERE source_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for name in &body.hidden {
        sqlx::query("INSERT OR IGNORE INTO hidden_groups (source_id, group_title) VALUES (?, ?)")
            .bind(id)
            .bind(name.as_deref().unwrap_or(""))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a source with its channels, guide and the recording rules on those channels.
pub async fn delete_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let done = sqlx::query("DELETE FROM tv_sources WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Queues a refresh; the job's result is the `RefreshReport`.
pub async fn refresh_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    sqlx::query("SELECT 1 FROM tv_sources WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let job = crate::jobs::enqueue(&state, JobKind::RefreshSource, id).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
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

/// Matches channels in group `?1`: every channel when it is NULL, the ungrouped when it is ''.
const IN_GROUP: &str = "(?1 IS NULL OR group_title = ?1 OR (?1 = '' AND group_title IS NULL))";

/// Leaves out channels in groups an admin has hidden (see `source_groups`).
const VISIBLE: &str = "NOT EXISTS (SELECT 1 FROM hidden_groups h WHERE h.source_id = channels.source_id \
     AND h.group_title = COALESCE(channels.group_title, ''))";

async fn all_channels(state: &AppState, group: Option<&str>) -> ApiResult<Vec<Channel>> {
    let rows: Vec<ChannelRow> = sqlx::query_as(&format!(
        "SELECT id, source_id, number, name, logo_url, group_title FROM channels \
         WHERE {IN_GROUP} AND {VISIBLE} ORDER BY number IS NULL, number, name"
    ))
    .bind(group)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(Channel::from).collect())
}

#[derive(Deserialize)]
pub struct ChannelQuery {
    /// Channels whose name contains this, at most 50.
    q: Option<String>,
}

pub async fn channels(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(query): Query<ChannelQuery>,
) -> ApiResult<Json<Vec<Channel>>> {
    let Some(q) = query.q.filter(|q| !q.trim().is_empty()) else {
        return Ok(Json(all_channels(&state, None).await?));
    };
    let rows: Vec<ChannelRow> = sqlx::query_as(&format!(
        "SELECT id, source_id, number, name, logo_url, group_title FROM channels \
         WHERE name LIKE '%' || ? || '%' AND {VISIBLE} ORDER BY number IS NULL, number, name LIMIT 50"
    ))
    .bind(q.trim())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Channel::from).collect()))
}

/// Channel groups in playlist order (by their first channel), the ungrouped last.
pub async fn groups(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> ApiResult<Json<Vec<ChannelGroup>>> {
    let rows: Vec<(Option<String>, i64)> = sqlx::query_as(&format!(
        "SELECT group_title, COUNT(*) FROM channels WHERE {VISIBLE} GROUP BY group_title \
         ORDER BY group_title IS NULL, MIN(number), group_title"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(name, channels)| ChannelGroup { name, channels })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct Window {
    start: Option<i64>,
    end: Option<i64>,
    group: Option<String>,
    /// At most this many programmes per channel, earliest first (2 = now and next).
    per_channel: Option<i64>,
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

/// Every channel (or one group's) with the programmes that overlap `[start, end)`; defaults to
/// the next 3 hours.
pub async fn guide(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(w): Query<Window>,
) -> ApiResult<Json<Vec<GuideChannel>>> {
    let start = w.start.unwrap_or_else(now);
    let end = w.end.unwrap_or(start + 3 * 3600);
    let group = w.group.as_deref();
    let rows: Vec<ProgrammeRow> = sqlx::query_as(&format!(
        "SELECT id, channel_id, start, stop, title, subtitle, description FROM ( \
           SELECT *, ROW_NUMBER() OVER (PARTITION BY channel_id ORDER BY start) AS n FROM programmes \
           WHERE stop > ?2 AND start < ?3 \
           AND channel_id IN (SELECT id FROM channels WHERE {IN_GROUP} AND {VISIBLE})) \
         WHERE ?4 IS NULL OR n <= ?4 ORDER BY start"
    ))
    .bind(group)
    .bind(start)
    .bind(end)
    .bind(w.per_channel)
    .fetch_all(&state.db)
    .await?;
    let mut out: Vec<GuideChannel> = all_channels(&state, group)
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

/// The HTTP status a provider answers for `url`, without reading the stream. `None` for local
/// files and when it does not answer at all.
async fn provider_status(url: &str) -> Option<u16> {
    if !url.starts_with("http") {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;
    client
        .get(url)
        .send()
        .await
        .ok()
        .map(|r| r.status().as_u16())
}

/// Plain words for a provider's refusal of a live stream, by HTTP status.
fn refusal(status: u16) -> Option<&'static str> {
    match status {
        401 | 403 | 407 => Some("your provider won't play this channel on your account"),
        404 | 410 => Some("your provider no longer carries this channel"),
        429 => Some(
            "your provider's limit on streams at once is reached; stop another stream and try again",
        ),
        500..=599 => Some("your provider's server isn't answering; try again in a minute"),
        _ => None,
    }
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
    let session = state.hls.start(&url, method, u32::MAX, 0.0, &[])?;
    // Live sources fail at the far end often (dead channels, connection limits); say so now
    // rather than hand the player a playlist that never appears.
    if let Err(why) = state
        .hls
        .ready(&session, std::time::Duration::from_secs(20))
        .await
    {
        // ffmpeg names few HTTP refusals ("4XX Client Error"); ask the provider once more, now
        // that nothing else holds a connection, and say what it answered.
        let why = provider_status(&url).await.and_then(refusal).map_or_else(
            || format!("the channel did not start: {why}"),
            str::to_string,
        );
        return Err(ApiError::BadGateway(why));
    }
    Ok(Json(PlaybackPlan {
        method,
        url: format!("/api/v1/hls/{session}/index.m3u8"),
        start_s: 0.0,
        duration_s: None,
    }))
}

#[derive(FromRow)]
struct RuleRow {
    id: i64,
    kind: String,
    channel_id: i64,
    programme_id: Option<i64>,
    series_title: Option<String>,
    padding_before_s: i64,
    padding_after_s: i64,
    keep_last: Option<i64>,
}

impl From<RuleRow> for RecordingRule {
    fn from(r: RuleRow) -> Self {
        RecordingRule {
            id: r.id,
            kind: dvr::parse_kind(&r.kind),
            channel_id: r.channel_id,
            programme_id: r.programme_id,
            series_title: r.series_title,
            padding_before_s: r.padding_before_s,
            padding_after_s: r.padding_after_s,
            keep_last: r.keep_last,
        }
    }
}

const RULE_SELECT: &str = "SELECT id, kind, channel_id, programme_id, series_title, padding_before_s, \
     padding_after_s, keep_last FROM recording_rules";

pub async fn rules(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> ApiResult<Json<Vec<RecordingRule>>> {
    let rows: Vec<RuleRow> =
        sqlx::query_as(&format!("{RULE_SELECT} WHERE user_id = ? ORDER BY id"))
            .bind(user.id)
            .fetch_all(&state.db)
            .await?;
    Ok(Json(rows.into_iter().map(RecordingRule::from).collect()))
}

pub async fn create_rule(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(new): Json<NewRecordingRule>,
) -> ApiResult<(StatusCode, Json<RecordingRule>)> {
    let bad = |m: &str| Err(ApiError::BadRequest(m.into()));
    if new.padding_before_s < 0 || new.padding_after_s < 0 {
        return bad("padding cannot be negative");
    }
    if new.keep_last.is_some_and(|k| k < 1) {
        return bad("keep_last must be at least 1");
    }
    let (channel_id, series_title, series_key) = match (new.programme_id, new.kind) {
        (Some(pid), kind) => {
            let p: Option<(i64, String, Option<String>)> =
                sqlx::query_as("SELECT channel_id, title, series_key FROM programmes WHERE id = ?")
                    .bind(pid)
                    .fetch_optional(&state.db)
                    .await?;
            let Some((channel_id, title, key)) = p else {
                return bad("unknown programme");
            };
            match kind {
                RuleKind::Once => (channel_id, None, None),
                RuleKind::Series => (channel_id, Some(title), key),
            }
        }
        (None, RuleKind::Series) => {
            let (Some(channel_id), Some(title)) = (new.channel_id, new.series_title.clone()) else {
                return bad("a series rule needs a programme, or a channel and a series title");
            };
            let known: Option<(i64,)> = sqlx::query_as("SELECT id FROM channels WHERE id = ?")
                .bind(channel_id)
                .fetch_optional(&state.db)
                .await?;
            if known.is_none() {
                return bad("unknown channel");
            }
            (channel_id, Some(title), None)
        }
        (None, RuleKind::Once) => return bad("a once rule needs a programme"),
    };
    let kind = match new.kind {
        RuleKind::Once => "once",
        RuleKind::Series => "series",
    };
    let id = sqlx::query(
        "INSERT INTO recording_rules (user_id, kind, channel_id, programme_id, series_title, series_key, \
         padding_before_s, padding_after_s, keep_last) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user.id)
    .bind(kind)
    .bind(channel_id)
    .bind(new.programme_id)
    .bind(&series_title)
    .bind(&series_key)
    .bind(new.padding_before_s)
    .bind(new.padding_after_s)
    .bind(new.keep_last)
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    // Schedule now: a guide refresh may replace the programme before the DVR's next tick.
    dvr::schedule(&state.db, now()).await?;
    Ok((
        StatusCode::CREATED,
        Json(RecordingRule {
            id,
            kind: new.kind,
            channel_id,
            programme_id: new.programme_id,
            series_title,
            padding_before_s: new.padding_before_s,
            padding_after_s: new.padding_after_s,
            keep_last: new.keep_last,
        }),
    ))
}

/// Deletes the rule and its recording rows; finished files stay in the Recordings library.
pub async fn delete_rule(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let done = sqlx::query("DELETE FROM recording_rules WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(FromRow)]
struct RecordingRow {
    id: i64,
    rule_id: i64,
    channel_id: i64,
    title: String,
    subtitle: Option<String>,
    start: i64,
    stop: i64,
    status: String,
    item_id: Option<i64>,
}

pub async fn recordings(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> ApiResult<Json<Vec<Recording>>> {
    let rows: Vec<RecordingRow> = sqlx::query_as(
        "SELECT r.id, r.rule_id, r.channel_id, r.title, r.subtitle, r.start, r.stop, r.status, r.item_id \
         FROM recordings r JOIN recording_rules u ON u.id = r.rule_id WHERE u.user_id = ? ORDER BY r.start",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| Recording {
                id: r.id,
                rule_id: r.rule_id,
                channel_id: r.channel_id,
                title: r.title,
                subtitle: r.subtitle,
                start: r.start,
                stop: r.stop,
                status: dvr::parse_status(&r.status),
                item_id: r.item_id,
            })
            .collect(),
    ))
}

/// Deletes a finished recording's file and its library item. The record stays, so a series rule
/// does not record the same episode again.
pub async fn delete_recording(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let found: Option<(Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT r.file_path, r.item_id FROM recordings r JOIN recording_rules u ON u.id = r.rule_id \
         WHERE r.id = ? AND u.user_id = ?",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;
    let Some((file, item)) = found else {
        return Err(ApiError::NotFound);
    };
    if let Some(file) = file {
        let _ = tokio::fs::remove_file(&file).await;
    }
    if let Some(item) = item {
        sqlx::query("DELETE FROM items WHERE id = ?")
            .bind(item)
            .execute(&state.db)
            .await?;
    }
    sqlx::query("UPDATE recordings SET file_path = NULL, item_id = NULL WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::refusal;

    #[test]
    fn provider_refusals_read_plainly() {
        assert_eq!(
            refusal(407),
            Some("your provider won't play this channel on your account")
        );
        assert_eq!(
            refusal(403),
            Some("your provider won't play this channel on your account")
        );
        assert_eq!(
            refusal(404),
            Some("your provider no longer carries this channel")
        );
        assert_eq!(
            refusal(429),
            Some(
                "your provider's limit on streams at once is reached; stop another stream and try again"
            )
        );
        assert_eq!(
            refusal(503),
            Some("your provider's server isn't answering; try again in a minute")
        );
        assert_eq!(refusal(200), None);
        assert_eq!(refusal(206), None);
    }
}
