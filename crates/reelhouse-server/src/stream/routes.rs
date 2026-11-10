use std::time::{Duration, Instant};

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
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

#[derive(serde::Deserialize)]
pub struct StartAt {
    start: Option<f64>,
}

pub async fn playback(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
    Query(at): Query<StartAt>,
    Json(caps): Json<Capabilities>,
) -> ApiResult<Json<PlaybackPlan>> {
    let (path, info) = media(&state, id).await?;
    let method = decide(&info, &caps);
    let start_s = at
        .start
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(0.0);
    let (url, start_s) = match method {
        PlaybackMethod::Direct => (format!("/api/v1/items/{id}/file"), 0.0),
        _ => {
            // A file whose soundtracks cannot be read still plays its first.
            let audio = crate::scan::probe::audio_languages(&path)
                .await
                .unwrap_or_default();
            let session = state
                .hls
                .start(&path, method, caps.max_height, start_s, &audio)?;
            (format!("/api/v1/hls/{session}/index.m3u8"), start_s)
        }
    };
    Ok(Json(PlaybackPlan {
        method,
        url,
        start_s,
        duration_s: None,
    }))
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
    Path((session, name)): Path<(String, String)>,
    req: Request,
) -> ApiResult<Response> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let path = state.hls.file(&session, &name).ok_or(ApiError::NotFound)?;
        if path.exists() {
            return serve_file(path, req).await;
        }
        if Instant::now() > deadline {
            return Err(ApiError::NotFound);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// How a client should fetch a copy of `id` to keep: the file itself when it plays it as is,
/// otherwise an MP4 made for it (see [`mp4`]).
pub async fn offline(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
    Json(caps): Json<Capabilities>,
) -> ApiResult<Json<reelhouse_api::OfflineSource>> {
    let (_, info) = media(&state, id).await?;
    Ok(Json(match decide(&info, &caps) {
        PlaybackMethod::Direct => reelhouse_api::OfflineSource {
            url: format!("/api/v1/items/{id}/file"),
            resumable: true,
        },
        method => reelhouse_api::OfflineSource {
            url: format!(
                "/api/v1/items/{id}/mp4?transcode={}&height={}",
                u8::from(method == PlaybackMethod::Transcode),
                caps.max_height
            ),
            resumable: false,
        },
    }))
}

#[derive(serde::Deserialize)]
pub struct Mp4Query {
    #[serde(default)]
    transcode: u8,
    height: Option<u32>,
}

/// The title as a fragmented MP4 made as it streams, for clients that keep copies offline but
/// cannot play the original.
pub async fn mp4(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
    Query(q): Query<Mp4Query>,
) -> ApiResult<Response> {
    let (path, _) = media(&state, id).await?;
    mp4_response(&path, q.transcode == 1, q.height.unwrap_or(2160))
}

/// Runs ffmpeg to turn `input` into a fragmented MP4 on stdout and streams it; the process stops
/// when the response is dropped.
pub fn mp4_response(input: &str, transcode: bool, max_height: u32) -> ApiResult<Response> {
    use futures::StreamExt;
    use tokio::io::AsyncReadExt;
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.args([
        "-v", "error", "-nostdin", "-i", input, "-map", "0:v:0", "-map", "0:a:0?",
    ]);
    if transcode {
        let scale = format!("scale=-2:'min(ih,{max_height})'");
        cmd.args([
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "21", "-pix_fmt", "yuv420p", "-vf",
            &scale,
        ]);
    } else {
        cmd.args(["-c:v", "copy"]);
    }
    cmd.args(["-c:a", "aac", "-ac", "2"])
        .args([
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-f",
            "mp4",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("starting ffmpeg: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("ffmpeg has no output")))?;
    // The child rides along with the stream, so dropping the response stops ffmpeg.
    let body = futures::stream::unfold((child, stdout), |(child, mut out)| async move {
        let mut buf = vec![0u8; 256 * 1024];
        match out.read(&mut buf).await {
            Ok(0) | Err(_) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)),
                    (child, out),
                ))
            }
        }
    })
    .boxed();
    Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "video/mp4")
        .body(Body::from_stream(body))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))
}
