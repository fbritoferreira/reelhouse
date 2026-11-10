use std::time::{Duration, Instant};

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::response::{IntoResponse, Response};
use reelhouse_api::{Capabilities, PlaybackMethod, PlaybackPlan};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::decide::decide;
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::scan::probe::MediaInfo;

#[derive(sqlx::FromRow)]
struct MediaRow {
    file_path: Option<String>,
    duration_s: Option<f64>,
    container: Option<String>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    height: Option<i64>,
}

async fn media(state: &AppState, id: i64) -> ApiResult<(String, MediaInfo)> {
    let r: MediaRow = sqlx::query_as(
        "SELECT file_path, duration_s, container, video_codec, audio_codec, height FROM items WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let path = r.file_path.ok_or(ApiError::NotFound)?;
    let info = MediaInfo {
        duration_s: r.duration_s,
        container: r.container,
        video_codec: r.video_codec,
        audio_codec: r.audio_codec,
        height: r.height,
    };
    Ok((path, info))
}

async fn serve_file(path: std::path::PathBuf, req: Request) -> ApiResult<Response> {
    let res = ServeFile::new(path)
        .oneshot(req)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
    Ok(res.map(Body::new).into_response())
}

pub async fn playback(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    Json(caps): Json<Capabilities>,
) -> ApiResult<Json<PlaybackPlan>> {
    let (path, info) = media(&state, id).await?;
    let method = decide(&info, &caps);
    let url = match method {
        PlaybackMethod::Direct => format!("/api/v1/items/{id}/file"),
        _ => {
            let session = state.hls.start(
                std::path::Path::new(&path),
                method,
                caps.max_height,
                user.id,
            )?;
            format!("/api/v1/hls/{session}/index.m3u8")
        }
    };
    Ok(Json(PlaybackPlan { method, url }))
}

/// The original file, with HTTP range support (tower-http answers 206 and 416).
pub async fn file(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
    req: Request,
) -> ApiResult<Response> {
    let (path, _) = media(&state, id).await?;
    serve_file(path.into(), req).await
}

/// A playlist or segment from a running HLS session; waits up to 15 s for ffmpeg to write it.
pub async fn hls_file(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((session, name)): Path<(String, String)>,
    req: Request,
) -> ApiResult<Response> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let path = state
            .hls
            .file(&session, &name, user.id)
            .ok_or(ApiError::NotFound)?;
        if path.exists() {
            return serve_file(path, req).await;
        }
        if Instant::now() > deadline {
            return Err(ApiError::NotFound);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
