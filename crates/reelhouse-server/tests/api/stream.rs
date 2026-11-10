use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::common::{admin_token, call};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use reelhouse_server::config::Config;
use serde_json::json;
use tower::ServiceExt;

pub fn have_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn make_video(path: &Path, secs: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let d = format!("testsrc=d={secs}:s=320x240");
    let a = format!("sine=d={secs}");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &d,
                "-f",
                "lavfi",
                "-i",
                &a,
                "-shortest",
                "-c:v",
                "libx264",
                "-c:a",
                "aac"
            ])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

/// App with one scanned movie; returns (app, token, item id, tempdir guard).
async fn setup(idle: Duration) -> (axum::Router, String, i64, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    make_video(&dir.path().join("Clip (2020).mkv"), 12);
    let mut config = Config::for_tests(std::env::temp_dir());
    config.hls_idle = idle;
    let app = super::common::app_with(config).await;
    let token = admin_token(&app).await;
    let (_, lib) = call(
        &app,
        "POST",
        "/api/v1/libraries",
        Some(&token),
        Some(json!({"name": "m", "kind": "movies", "path": dir.path()})),
    )
    .await;
    let lib = lib["id"].as_i64().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    let (_, items) = call(
        &app,
        "GET",
        &format!("/api/v1/libraries/{lib}/items"),
        Some(&token),
        None,
    )
    .await;
    (app, token, items[0]["id"].as_i64().unwrap(), dir)
}

pub async fn get(
    app: &axum::Router,
    uri: &str,
    range: Option<&str>,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut req = Request::builder().uri(uri);
    if let Some(r) = range {
        req = req.header("range", r);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let cr = res
        .headers()
        .get("content-range")
        .map(|v| v.to_str().unwrap().to_string());
    (
        status,
        cr,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

#[tokio::test]
async fn file_supports_ranges() {
    if !have_ffmpeg() {
        return;
    }
    let (app, token, id, _dir) = setup(Duration::from_secs(60)).await;
    let (status, cr, body) = get(
        &app,
        &format!("/api/v1/items/{id}/file?token={token}"),
        Some("bytes=0-99"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert!(cr.unwrap().starts_with("bytes 0-99/"));
    assert_eq!(body.len(), 100);
}

#[tokio::test]
async fn range_past_end_is_416() {
    if !have_ffmpeg() {
        return;
    }
    let (app, token, id, _dir) = setup(Duration::from_secs(60)).await;
    let (status, _, _) = get(
        &app,
        &format!("/api/v1/items/{id}/file?token={token}"),
        Some("bytes=999999999-"),
    )
    .await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
}

async fn start_hls(app: &axum::Router, token: &str, id: i64) -> String {
    let caps = json!({"containers": ["mp4"], "video_codecs": ["h264"], "audio_codecs": ["aac"], "max_height": 1080});
    let (status, plan) = call(
        app,
        "POST",
        &format!("/api/v1/items/{id}/playback"),
        Some(token),
        Some(caps),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plan["method"], "remux");
    plan["url"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn hls_playlist_appears() {
    if !have_ffmpeg() {
        return;
    }
    let (app, token, id, _dir) = setup(Duration::from_secs(60)).await;
    let url = start_hls(&app, &token, id).await;
    let (status, _, body) = get(&app, &format!("{url}?token={token}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).starts_with("#EXTM3U"));
}

#[tokio::test]
async fn hls_can_start_part_way_through() {
    if !have_ffmpeg() {
        return;
    }
    let (app, token, id, _dir) = setup(Duration::from_secs(60)).await;
    let caps = json!({"containers": ["mp4"], "video_codecs": ["h264"], "audio_codecs": ["aac"], "max_height": 1080});
    let (status, plan) = call(
        &app,
        "POST",
        &format!("/api/v1/items/{id}/playback?start=1.5"),
        Some(&token),
        Some(caps),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plan["method"], "remux");
    assert_eq!(plan["start_s"], 1.5);
}

#[tokio::test]
async fn idle_hls_session_is_reaped() {
    if !have_ffmpeg() {
        return;
    }
    let (app, token, id, _dir) = setup(Duration::from_secs(2)).await;
    let url = start_hls(&app, &token, id).await;
    assert_eq!(
        get(&app, &format!("{url}?token={token}"), None).await.0,
        StatusCode::OK
    );
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(
        get(&app, &format!("{url}?token={token}"), None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn file_requires_token() {
    let app = super::common::app().await;
    let (status, _, _) = get(&app, "/api/v1/items/1/file", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn hls_segments_load_without_token_query() {
    if !have_ffmpeg() {
        return;
    }
    let (app, token, id, _dir) = setup(Duration::from_secs(60)).await;
    let url = start_hls(&app, &token, id).await;
    // Wait for the playlist, then fetch the init segment the way a player does: relative, no token.
    assert_eq!(get(&app, &url, None).await.0, StatusCode::OK);
    let init = url.replace("index.m3u8", "init.mp4");
    assert_eq!(get(&app, &init, None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn unknown_hls_session_is_404() {
    let app = super::common::app().await;
    let (status, _, _) = get(&app, "/api/v1/hls/not-a-session/index.m3u8", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A film with English and Spanish soundtracks.
fn make_bilingual(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    assert!(
        Command::new("ffmpeg")
            .args(["-y", "-v", "error"])
            .args(["-f", "lavfi", "-i", "testsrc=d=8:s=320x240"])
            .args(["-f", "lavfi", "-i", "sine=f=440:d=8"])
            .args(["-f", "lavfi", "-i", "sine=f=880:d=8"])
            .args([
                "-map", "0", "-map", "1", "-map", "2", "-c:v", "libx264", "-c:a", "aac"
            ])
            .args([
                "-metadata:s:a:0",
                "language=eng",
                "-metadata:s:a:1",
                "language=spa"
            ])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn every_soundtrack_reaches_hls() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    make_bilingual(&dir.path().join("Two (2020).mkv"));
    let app = super::common::app_with(Config::for_tests(std::env::temp_dir())).await;
    let token = admin_token(&app).await;
    let (_, lib) = call(
        &app,
        "POST",
        "/api/v1/libraries",
        Some(&token),
        Some(json!({"name": "m", "kind": "movies", "path": dir.path()})),
    )
    .await;
    let lib = lib["id"].as_i64().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    let (_, items) = call(
        &app,
        "GET",
        &format!("/api/v1/libraries/{lib}/items"),
        Some(&token),
        None,
    )
    .await;
    let id = items[0]["id"].as_i64().unwrap();
    let url = start_hls(&app, &token, id).await;
    // The master playlist appears once ffmpeg has started.
    let mut master = String::new();
    for _ in 0..50 {
        let (status, _, body) = get(&app, &format!("{url}?token={token}"), None).await;
        if status == StatusCode::OK {
            master = String::from_utf8_lossy(&body).into_owned();
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(master.matches("TYPE=AUDIO").count(), 2, "{master}");
    assert!(master.contains("LANGUAGE=\"spa\""), "{master}");
}

#[tokio::test]
async fn offline_copies_come_as_files_or_mp4() {
    if !have_ffmpeg() {
        return;
    }
    let (app, token, id, _dir) = setup(Duration::from_secs(60)).await;
    // An mkv: a browser that plays only mp4 gets a remuxed mp4 stream.
    let mp4_only = json!({"containers": ["mp4"], "video_codecs": ["h264"], "audio_codecs": ["aac"], "max_height": 1080});
    let (status, plan) = call(
        &app,
        "POST",
        &format!("/api/v1/items/{id}/offline"),
        Some(&token),
        Some(mp4_only),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["resumable"], false);
    let url = plan["url"].as_str().unwrap().to_string();
    assert!(url.contains("/mp4"), "{url}");
    let sep = if url.contains('?') { '&' } else { '?' };
    let (status, _, body) = get(&app, &format!("{url}{sep}token={token}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[4..8], b"ftyp", "an mp4 starts with its ftyp box");

    // One that plays mkv gets the file itself, resumable.
    let mkv = json!({"containers": ["mkv"], "video_codecs": ["h264"], "audio_codecs": ["aac"], "max_height": 1080});
    let (_, plan) = call(
        &app,
        "POST",
        &format!("/api/v1/items/{id}/offline"),
        Some(&token),
        Some(mkv),
    )
    .await;
    assert_eq!(plan["resumable"], true);
    assert_eq!(plan["url"], format!("/api/v1/items/{id}/file"));
}

#[tokio::test]
async fn text_subtitles_come_as_webvtt() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let film = dir.path().join("Subs (2020)/Subs (2020).mkv");
    std::fs::create_dir_all(film.parent().unwrap()).unwrap();
    let srt = dir.path().join("en.srt");
    std::fs::write(&srt, "1\n00:00:01,000 --> 00:00:03,000\nHello there\n\n2\n00:00:05,000 --> 00:00:07,000\nSecond line\n").unwrap();
    assert!(
        Command::new("ffmpeg")
            .args(["-y", "-v", "error"])
            .args(["-f", "lavfi", "-i", "testsrc=d=8:s=320x240"])
            .args(["-f", "lavfi", "-i", "sine=d=8"])
            .arg("-i")
            .arg(&srt)
            .args([
                "-map", "0", "-map", "1", "-map", "2", "-c:v", "libx264", "-c:a", "aac", "-c:s",
                "srt"
            ])
            .args(["-metadata:s:s:0", "language=eng"])
            .arg(&film)
            .status()
            .unwrap()
            .success()
    );
    let app = super::common::app_with(Config::for_tests(std::env::temp_dir())).await;
    let token = admin_token(&app).await;
    let (_, lib) = call(
        &app,
        "POST",
        "/api/v1/libraries",
        Some(&token),
        Some(json!({"name": "m", "kind": "movies", "path": dir.path()})),
    )
    .await;
    let lib = lib["id"].as_i64().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    let (_, items) = call(
        &app,
        "GET",
        &format!("/api/v1/libraries/{lib}/items"),
        Some(&token),
        None,
    )
    .await;
    let id = items[0]["id"].as_i64().unwrap();

    let (status, subs) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{id}/subtitles"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        subs,
        json!([{"index": 0, "language": "eng", "title": null}])
    );

    let (status, _, body) = get(
        &app,
        &format!("/api/v1/items/{id}/subtitles/0?token={token}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let vtt = String::from_utf8_lossy(&body);
    assert!(vtt.starts_with("WEBVTT"), "{vtt}");
    assert!(vtt.contains("Hello there"), "{vtt}");

    // Starting part-way in, the cues move to the stream's clock.
    let (_, _, body) = get(
        &app,
        &format!("/api/v1/items/{id}/subtitles/0?start=4&token={token}"),
        None,
    )
    .await;
    let vtt = String::from_utf8_lossy(&body);
    assert!(!vtt.contains("Hello there"), "{vtt}");
    assert!(vtt.contains("00:01.000 --> 00:03.000"), "{vtt}");
}
