use super::common::{admin_token, app, call};
use super::stream::{have_ffmpeg, make_video};
use axum::http::StatusCode;
use serde_json::json;

/// A playlist with one live channel, two films and two episodes of one show; returns its path.
fn playlist(dir: &std::path::Path, film: &str) -> String {
    let path = dir.join("list.m3u");
    std::fs::write(
        &path,
        format!(
            "#EXTM3U\n\
             #EXTINF:-1 group-title=\"News\",Live News\nhttp://example.invalid/live/1\n\
             #EXTINF:-1 tvg-logo=\"http://p/heat.jpg\" group-title=\"Crime\",Heat (1995)\n{film}\n\
             #EXTINF:-1 group-title=\"Crime\",Ronin (1998)\nhttp://example.invalid/movie/u/p/2.mp4\n\
             #EXTINF:-1 tvg-logo=\"http://p/wire.jpg\" group-title=\"Drama\",The Wire S01 The Wire - S01E01 - The Target\nhttp://example.invalid/series/u/p/3.mkv\n\
             #EXTINF:-1 group-title=\"Drama\",The Wire S01 The Wire - S01E02 - The Detail\nhttp://example.invalid/series/u/p/4.mkv\n"
        ),
    )
    .unwrap();
    path.to_string_lossy().into_owned()
}

async fn add(app: &axum::Router, token: &str, m3u: &str) {
    let (_, src) = call(
        app,
        "POST",
        "/api/v1/livetv/sources",
        Some(token),
        Some(json!({"name": "P", "kind": "m3u", "url": m3u})),
    )
    .await;
    let (status, report) = call(
        app,
        "POST",
        &format!("/api/v1/livetv/sources/{}/refresh", src["id"]),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["channels"], 1);
    assert_eq!(report["on_demand"], 4);
}

#[tokio::test]
async fn films_and_series_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = playlist(dir.path(), "http://example.invalid/movie/u/p/1.mp4");
    let app = app().await;
    let token = admin_token(&app).await;
    add(&app, &token, &m3u).await;

    let (_, groups) = call(
        &app,
        "GET",
        "/api/v1/vod/groups?kind=movies",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(groups, json!([{"name": "Crime", "titles": 2}]));
    let (_, groups) = call(
        &app,
        "GET",
        "/api/v1/vod/groups?kind=series",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(groups, json!([{"name": "Drama", "titles": 1}]));

    let (_, films) = call(
        &app,
        "GET",
        "/api/v1/vod/movies?group=Crime",
        Some(&token),
        None,
    )
    .await;
    let titles: Vec<&str> = films
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Heat", "Ronin"]);
    assert_eq!(films[0]["year"], 1995);
    assert_eq!(films[0]["poster_url"], "http://p/heat.jpg");
    assert!(
        films[0].get("stream_url").is_none(),
        "stream URLs carry credentials"
    );

    let (_, shows) = call(
        &app,
        "GET",
        "/api/v1/vod/series?group=Drama",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        shows,
        json!([{"title": "The Wire", "poster_url": "http://p/wire.jpg", "seasons": 1, "episodes": 2}])
    );
    let (_, eps) = call(
        &app,
        "GET",
        "/api/v1/vod/episodes?show=The%20Wire",
        Some(&token),
        None,
    )
    .await;
    let names: Vec<&str> = eps
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["title"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["The Target", "The Detail"]);
    assert_eq!(eps[1]["episode_no"], 2);
}

#[tokio::test]
async fn a_film_plays_through_hls() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let film = dir.path().join("heat.mp4");
    make_video(&film, 8);
    let m3u = playlist(dir.path(), &film.to_string_lossy());
    let app = app().await;
    let token = admin_token(&app).await;
    add(&app, &token, &m3u).await;
    let (_, films) = call(
        &app,
        "GET",
        "/api/v1/vod/movies?group=Crime",
        Some(&token),
        None,
    )
    .await;
    let caps = json!({"containers": ["mp4"], "video_codecs": ["h264"], "audio_codecs": ["aac"], "max_height": 1080});
    let (status, plan) = call(
        &app,
        "POST",
        &format!("/api/v1/vod/{}/playback?start=2", films[0]["id"]),
        Some(&token),
        Some(caps),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert!(plan["url"].as_str().unwrap().starts_with("/api/v1/hls/"));
    assert_eq!(plan["start_s"], 2.0);
    let (status, _) = call(
        &app,
        "GET",
        plan["url"].as_str().unwrap(),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
