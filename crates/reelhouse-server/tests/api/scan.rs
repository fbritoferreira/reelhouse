use std::path::Path;
use std::process::Command;

use super::common::{admin_token, app, call};
use axum::http::StatusCode;
use serde_json::json;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn make_video(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=d=1:s=320x240",
            "-f",
            "lavfi",
            "-i",
            "sine=d=1",
            "-shortest",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
        ])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

async fn library(app: &axum::Router, token: &str, kind: &str, dir: &Path) -> i64 {
    let (status, lib) = call(
        app,
        "POST",
        "/api/v1/libraries",
        Some(token),
        Some(json!({"name": kind, "kind": kind, "path": dir})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    lib["id"].as_i64().unwrap()
}

#[tokio::test]
async fn scans_movies() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    make_video(
        &dir.path()
            .join("Blade Runner (1982)/Blade Runner (1982).mp4"),
    );
    let app = app().await;
    let token = admin_token(&app).await;
    let lib = library(&app, &token, "movies", dir.path()).await;
    let (_, report) = call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(report["added"], 1);
    let (_, items) = call(
        &app,
        "GET",
        &format!("/api/v1/libraries/{lib}/items"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(items[0]["title"], "Blade Runner");
    assert_eq!(items[0]["year"], 1982);
    assert!((items[0]["duration_s"].as_f64().unwrap() - 1.0).abs() < 0.2);
}

#[tokio::test]
async fn scans_tv_into_show_season_episode() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    make_video(
        &dir.path()
            .join("The Bear/Season 02/The Bear - S02E07 - Forks.mp4"),
    );
    let app = app().await;
    let token = admin_token(&app).await;
    let lib = library(&app, &token, "tv", dir.path()).await;
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    let (_, shows) = call(
        &app,
        "GET",
        &format!("/api/v1/libraries/{lib}/items"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(shows[0]["kind"], "show");
    let show = shows[0]["id"].as_i64().unwrap();
    let (_, seasons) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{show}/children"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(seasons[0]["season_no"], 2);
    let season = seasons[0]["id"].as_i64().unwrap();
    let (_, eps) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{season}/children"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(eps[0]["episode_no"], 7);
}

#[tokio::test]
async fn rescan_removes_missing_files() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Show/Season 1/Show - S01E01.mp4");
    make_video(&file);
    let app = app().await;
    let token = admin_token(&app).await;
    let lib = library(&app, &token, "tv", dir.path()).await;
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    std::fs::remove_file(&file).unwrap();
    let (_, report) = call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(report["removed"], 1);
    let (_, shows) = call(
        &app,
        "GET",
        &format!("/api/v1/libraries/{lib}/items"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(shows.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn rescan_is_idempotent() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    make_video(&dir.path().join("Heat (1995).mp4"));
    let app = app().await;
    let token = admin_token(&app).await;
    let lib = library(&app, &token, "movies", dir.path()).await;
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    let (_, report) = call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(report["added"], 0);
}

#[tokio::test]
async fn scan_fills_tmdb_metadata() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    if !have_ffmpeg() {
        return;
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/3/search/movie"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [{"id": 949, "title": "Heat", "release_date": "1995-12-15", "overview": "Obsessive crime.", "poster_path": "/h.jpg"}]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/3/movie/949/external_ids"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"imdb_id": "tt0113277"})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    make_video(&dir.path().join("Heat (1995).mp4"));
    let mut config = reelhouse_server::config::Config::for_tests(std::env::temp_dir());
    config.tmdb_api_key = Some("k".into());
    config.tmdb_base = Some(format!("{}/", server.uri()).parse().unwrap());
    let app = super::common::app_with(config).await;
    let token = admin_token(&app).await;
    let lib = library(&app, &token, "movies", dir.path()).await;
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
    assert_eq!(items[0]["overview"], "Obsessive crime.");
    assert_eq!(
        items[0]["poster_url"],
        "https://image.tmdb.org/t/p/w500/h.jpg"
    );
}
