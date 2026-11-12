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

#[tokio::test]
async fn progress_is_kept_and_listed_and_survives_a_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = playlist(dir.path(), "http://example.invalid/movie/u/p/1.mp4");
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
    let heat = films[0]["id"].as_i64().unwrap();

    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/v1/vod/{heat}/progress"),
        Some(&token),
        Some(json!({"position_s": 1234.0, "played": false})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, title) = call(
        &app,
        "GET",
        &format!("/api/v1/vod/{heat}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(title["title"], "Heat");
    assert_eq!(title["progress"]["position_s"], 1234.0);
    let (_, cont) = call(&app, "GET", "/api/v1/vod/continue", Some(&token), None).await;
    assert_eq!(cont[0]["id"], heat);

    // Refreshing the same playlist keeps the title, and so its progress.
    let (_, sources) = call(&app, "GET", "/api/v1/livetv/sources", Some(&token), None).await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/livetv/sources/{}/refresh", sources[0]["id"]),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, films) = call(
        &app,
        "GET",
        "/api/v1/vod/movies?group=Crime",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(films[0]["id"], heat);
    assert_eq!(films[0]["progress"]["position_s"], 1234.0);
}

#[tokio::test]
async fn a_title_downloads_through_the_server_with_ranges() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let provider = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/movie/u/p/1.mp4"))
        .and(header("range", "bytes=6-"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 6-10/11")
                .set_body_bytes("world"),
        )
        .mount(&provider)
        .await;
    Mock::given(method("GET"))
        .and(path("/movie/u/p/1.mp4"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes("hello world"))
        .mount(&provider)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let m3u = playlist(dir.path(), &format!("{}/movie/u/p/1.mp4", provider.uri()));
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
    let id = films[0]["id"].as_i64().unwrap();

    let file = format!("/api/v1/vod/{id}/file?token={token}");
    let (status, _, body) = super::stream::get(&app, &file, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"hello world");
    let (status, range, body) = super::stream::get(&app, &file, Some("bytes=6-")).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(range.as_deref(), Some("bytes 6-10/11"));
    assert_eq!(&body[..], b"world");
}

#[tokio::test]
async fn films_and_shows_can_be_searched() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = playlist(dir.path(), "http://example.invalid/movie/u/p/1.mp4");
    let app = app().await;
    let token = admin_token(&app).await;
    add(&app, &token, &m3u).await;
    let (status, found) = call(&app, "GET", "/api/v1/vod/search?q=wir", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["films"], json!([]));
    assert_eq!(found["shows"][0]["title"], "The Wire");
    let (_, found) = call(&app, "GET", "/api/v1/vod/search?q=heat", Some(&token), None).await;
    assert_eq!(found["films"][0]["title"], "Heat");
    assert_eq!(found["shows"], json!([]));
}

#[tokio::test]
async fn an_on_demand_title_can_be_kept_as_mp4() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let film = dir.path().join("heat.mkv");
    make_video(&film, 4);
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
    let id = films[0]["id"].as_i64().unwrap();
    let caps = json!({"containers": ["mp4"], "video_codecs": ["h264"], "audio_codecs": ["aac"], "max_height": 1080});
    let (status, plan) = call(
        &app,
        "POST",
        &format!("/api/v1/vod/{id}/offline"),
        Some(&token),
        Some(caps),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["resumable"], false);
    let url = plan["url"].as_str().unwrap();
    let (status, _, body) = super::stream::get(&app, &format!("{url}&token={token}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[4..8], b"ftyp");
}

#[tokio::test]
async fn a_title_the_provider_lost_says_why() {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let provider = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&provider)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let m3u = playlist(dir.path(), &format!("{}/movie/u/p/1.mp4", provider.uri()));
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
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/vod/{}/playback", films[0]["id"]))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(caps.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&body);
    assert_eq!(body, "your provider no longer carries this title");
}

#[tokio::test]
async fn titles_keep_their_ids_and_progress_when_the_provider_changes_its_urls() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = playlist(dir.path(), "http://example.invalid/movie/u/p/1.mp4");
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
    let heat = films[0]["id"].as_i64().unwrap();
    call(
        &app,
        "PUT",
        &format!("/api/v1/vod/{heat}/progress"),
        Some(&token),
        Some(json!({"position_s": 600.0, "played": false})),
    )
    .await;
    let (_, eps) = call(
        &app,
        "GET",
        "/api/v1/vod/episodes?show=The%20Wire",
        Some(&token),
        None,
    )
    .await;

    // The provider moves everything to new URLs.
    let text = std::fs::read_to_string(&m3u)
        .unwrap()
        .replace("/u/p/", "/u2/p2/");
    std::fs::write(&m3u, text).unwrap();
    let (_, sources) = call(&app, "GET", "/api/v1/livetv/sources", Some(&token), None).await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/livetv/sources/{}/refresh", sources[0]["id"]),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, films) = call(
        &app,
        "GET",
        "/api/v1/vod/movies?group=Crime",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(films[0]["id"], heat);
    assert_eq!(films[0]["progress"]["position_s"], 600.0);
    let (_, eps_after) = call(
        &app,
        "GET",
        "/api/v1/vod/episodes?show=The%20Wire",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(eps_after[0]["id"], eps[0]["id"]);
    assert_eq!(eps_after[1]["id"], eps[1]["id"]);
}
