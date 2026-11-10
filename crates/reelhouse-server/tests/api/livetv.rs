use std::path::Path;

use super::common::{admin_token, app, call, user_token};
use super::stream::{have_ffmpeg, make_video};
use axum::Router;
use axum::http::StatusCode;
use reelhouse_server::livetv::xmltv::civil_from_days;
use serde_json::{Value, json};
use wiremock::MockServer;

pub fn xmltv_time(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!(
        "{y:04}{m:02}{d:02}{:02}{:02}{:02} +0000",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

pub fn now() -> i64 {
    reelhouse_server::auth::now()
}

/// An XMLTV guide for `news.uk` with one programme per `(start, stop, title, subtitle)`.
pub fn guide(progs: &[(i64, i64, &str, &str)]) -> String {
    let mut xml =
        String::from(r#"<tv><channel id="news.uk"><display-name>News</display-name></channel>"#);
    for (start, stop, title, sub) in progs {
        xml += &format!(
            r#"<programme start="{}" stop="{}" channel="news.uk"><title>{title}</title><sub-title>{sub}</sub-title></programme>"#,
            xmltv_time(*start),
            xmltv_time(*stop)
        );
    }
    xml + "</tv>"
}

/// Writes an M3U with `news.uk` (streaming `stream`) and a channel without a guide, plus `epg`.
pub fn write_source(dir: &Path, stream: &str, epg: &str) -> (String, String) {
    let m3u = dir.join("list.m3u");
    std::fs::write(
        &m3u,
        format!(
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"news.uk\" tvg-chno=\"5\" group-title=\"UK\",News\n{stream}\n\
             #EXTINF:-1 tvg-chno=\"9\",Music\nhttp://example.invalid/music.ts\n"
        ),
    )
    .unwrap();
    let xml = dir.join("guide.xml");
    std::fs::write(&xml, epg).unwrap();
    (
        m3u.to_string_lossy().into_owned(),
        xml.to_string_lossy().into_owned(),
    )
}

/// Adds and refreshes an M3U source; returns the refresh report.
pub async fn add_m3u(app: &Router, token: &str, m3u: &str, epg: &str) -> Value {
    let (status, src) = call(
        app,
        "POST",
        "/api/v1/livetv/sources",
        Some(token),
        Some(json!({"name": "Local", "kind": "m3u", "url": m3u, "epg_url": epg})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = src["id"].as_i64().unwrap();
    let (status, report) = call(
        app,
        "POST",
        &format!("/api/v1/livetv/sources/{id}/refresh"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    report
}

#[tokio::test]
async fn sources_are_admin_only() {
    let app = app().await;
    let admin = admin_token(&app).await;
    let kid = user_token(&app, &admin, "kid").await;
    let (status, _) = call(&app, "GET", "/api/v1/livetv/sources", Some(&kid), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/livetv/sources",
        Some(&kid),
        Some(json!({"name": "x", "kind": "m3u", "url": "http://x/list.m3u"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn xtream_source_needs_credentials() {
    let app = app().await;
    let admin = admin_token(&app).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/livetv/sources",
        Some(&admin),
        Some(json!({"name": "x", "kind": "xtream", "url": "http://panel"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn m3u_refresh_loads_channels_and_guide() {
    let dir = tempfile::tempdir().unwrap();
    let t = now();
    let (m3u, epg) = write_source(
        dir.path(),
        "http://example.invalid/news.ts",
        &guide(&[
            (t, t + 1800, "Early", "a"),
            (t + 1800, t + 3600, "Late", "b"),
        ]),
    );
    let app = app().await;
    let admin = admin_token(&app).await;
    let report = add_m3u(&app, &admin, &m3u, &epg).await;
    assert_eq!(report, json!({"channels": 2, "programmes": 2}));

    let kid = user_token(&app, &admin, "kid").await;
    let (status, chans) = call(&app, "GET", "/api/v1/livetv/channels", Some(&kid), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(chans[0]["name"], "News");
    assert_eq!(chans[0]["number"], 5);
    assert_eq!(chans[0]["group_title"], "UK");
    assert_eq!(chans[1]["name"], "Music");
    assert!(
        chans[0].get("stream_url").is_none(),
        "stream URLs carry credentials"
    );

    let (_, g) = call(
        &app,
        "GET",
        &format!("/api/v1/livetv/guide?start={}&end={}", t + 1800, t + 7200),
        Some(&kid),
        None,
    )
    .await;
    assert_eq!(g[0]["channel"]["name"], "News");
    let titles: Vec<&str> = g[0]["programmes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Late"]);
    assert_eq!(g[1]["programmes"], json!([]));
}

#[tokio::test]
async fn groups_list_counts_and_filter_the_guide() {
    let dir = tempfile::tempdir().unwrap();
    let (m3u, epg) = write_source(dir.path(), "http://example.invalid/news.ts", &guide(&[]));
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;

    let (status, groups) = call(&app, "GET", "/api/v1/livetv/groups", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        groups,
        json!([{"name": "UK", "channels": 1}, {"name": null, "channels": 1}])
    );

    let (_, uk) = call(
        &app,
        "GET",
        "/api/v1/livetv/guide?group=UK",
        Some(&admin),
        None,
    )
    .await;
    let names: Vec<&str> = uk
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["channel"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["News"]);
    // An empty group asks for the channels without one.
    let (_, rest) = call(
        &app,
        "GET",
        "/api/v1/livetv/guide?group=",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(rest[0]["channel"]["name"], "Music");
    assert_eq!(rest.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn removing_a_source_removes_its_channels() {
    let dir = tempfile::tempdir().unwrap();
    let (m3u, epg) = write_source(dir.path(), "http://example.invalid/news.ts", &guide(&[]));
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, sources) = call(&app, "GET", "/api/v1/livetv/sources", Some(&admin), None).await;
    let id = sources[0]["id"].as_i64().unwrap();

    let kid = user_token(&app, &admin, "kid").await;
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/livetv/sources/{id}"),
        Some(&kid),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/livetv/sources/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, chans) = call(&app, "GET", "/api/v1/livetv/channels", Some(&admin), None).await;
    assert_eq!(chans, json!([]));
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/livetv/sources/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn guide_can_cap_programmes_per_channel() {
    let dir = tempfile::tempdir().unwrap();
    let t = now();
    let (m3u, epg) = write_source(
        dir.path(),
        "http://example.invalid/news.ts",
        &guide(&[
            (t - 60, t + 1800, "Now", "a"),
            (t + 1800, t + 3600, "Next", "b"),
            (t + 3600, t + 5400, "Later", "c"),
        ]),
    );
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, g) = call(
        &app,
        "GET",
        "/api/v1/livetv/guide?group=UK&per_channel=2",
        Some(&admin),
        None,
    )
    .await;
    let titles: Vec<&str> = g[0]["programmes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Now", "Next"]);
}

#[tokio::test]
async fn hidden_groups_leave_live_tv() {
    let dir = tempfile::tempdir().unwrap();
    let (m3u, epg) = write_source(dir.path(), "http://example.invalid/news.ts", &guide(&[]));
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, sources) = call(&app, "GET", "/api/v1/livetv/sources", Some(&admin), None).await;
    let id = sources[0]["id"].as_i64().unwrap();
    let path = format!("/api/v1/livetv/sources/{id}/groups");

    let (status, groups) = call(&app, "GET", &path, Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        groups,
        json!([{"name": "UK", "channels": 1, "shown": true}, {"name": null, "channels": 1, "shown": true}])
    );

    let kid = user_token(&app, &admin, "kid").await;
    let (status, _) = call(
        &app,
        "PUT",
        &path,
        Some(&kid),
        Some(json!({"hidden": ["UK"]})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(
        &app,
        "PUT",
        &path,
        Some(&admin),
        Some(json!({"hidden": ["UK"]})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, groups) = call(&app, "GET", &path, Some(&admin), None).await;
    assert_eq!(groups[0]["shown"], false);
    let (_, visible) = call(&app, "GET", "/api/v1/livetv/groups", Some(&admin), None).await;
    assert_eq!(visible, json!([{"name": null, "channels": 1}]));
    let (_, chans) = call(&app, "GET", "/api/v1/livetv/channels", Some(&admin), None).await;
    assert_eq!(chans.as_array().unwrap().len(), 1);
    let (_, uk) = call(
        &app,
        "GET",
        "/api/v1/livetv/guide?group=UK",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(uk, json!([]));

    // A refresh keeps the choice.
    call(
        &app,
        "POST",
        &format!("/api/v1/livetv/sources/{id}/refresh"),
        Some(&admin),
        None,
    )
    .await;
    let (_, visible) = call(&app, "GET", "/api/v1/livetv/groups", Some(&admin), None).await;
    assert_eq!(visible, json!([{"name": null, "channels": 1}]));
}

#[tokio::test]
async fn refresh_keeps_channel_ids() {
    let dir = tempfile::tempdir().unwrap();
    let (m3u, epg) = write_source(dir.path(), "http://example.invalid/n.ts", &guide(&[]));
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, before) = call(&app, "GET", "/api/v1/livetv/channels", Some(&admin), None).await;
    let (_, sources) = call(&app, "GET", "/api/v1/livetv/sources", Some(&admin), None).await;
    let id = sources[0]["id"].as_i64().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/livetv/sources/{id}/refresh"),
        Some(&admin),
        None,
    )
    .await;
    let (_, after) = call(&app, "GET", "/api/v1/livetv/channels", Some(&admin), None).await;
    assert_eq!(before, after);
}

#[tokio::test]
async fn xtream_refresh_uses_panel_guide() {
    let server = MockServer::start().await;
    super::xtream::mount_panel(&server).await;
    let app = app().await;
    let admin = admin_token(&app).await;
    let (status, src) = call(
        &app,
        "POST",
        "/api/v1/livetv/sources",
        Some(&admin),
        Some(json!({"name": "XC", "kind": "xtream", "url": server.uri(), "username": "u", "password": "p"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(src.get("password").is_none());
    let (status, report) = call(
        &app,
        "POST",
        &format!("/api/v1/livetv/sources/{}/refresh", src["id"]),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report, json!({"channels": 2, "programmes": 1}));
}

#[tokio::test]
async fn unreachable_source_fails_refresh() {
    let app = app().await;
    let admin = admin_token(&app).await;
    let (_, src) = call(
        &app,
        "POST",
        "/api/v1/livetv/sources",
        Some(&admin),
        Some(json!({"name": "gone", "kind": "m3u", "url": "/does/not/exist.m3u"})),
    )
    .await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/livetv/sources/{}/refresh", src["id"]),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn play_channel_starts_hls() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let stream = dir.path().join("live.ts");
    make_video(&stream, 12);
    let (m3u, epg) = write_source(dir.path(), &stream.to_string_lossy(), &guide(&[]));
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, chans) = call(&app, "GET", "/api/v1/livetv/channels", Some(&admin), None).await;
    let (status, plan) = call(
        &app,
        "POST",
        &format!("/api/v1/livetv/channels/{}/play", chans[0]["id"]),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plan["method"], "remux");
    let url = plan["url"].as_str().unwrap();
    assert!(url.starts_with("/api/v1/hls/"));
    let (status, _) = call(&app, "GET", url, Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn dead_channel_fails_fast() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (m3u, epg) = write_source(dir.path(), "/does/not/exist.ts", &guide(&[]));
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, chans) = call(&app, "GET", "/api/v1/livetv/channels", Some(&admin), None).await;
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/v1/livetv/channels/{}/play", chans[0]["id"]),
        Some(&admin),
        None,
    )
    .await;
    // The plain-text reason (ffmpeg's last error) is covered by hls::tests::last_error_skips_noise.
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
}
