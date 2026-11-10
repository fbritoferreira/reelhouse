use std::time::{Duration, Instant};

use super::common::{admin_token, app, app_with, call, user_token};
use super::livetv::{add_m3u, guide, now, write_source};
use super::stream::{have_ffmpeg, make_video};
use axum::Router;
use axum::http::StatusCode;
use reelhouse_server::config::Config;
use serde_json::{Value, json};

/// An app with a News channel airing "Show" twice (Pilot, then Second) and "Other".
async fn setup() -> (Router, String, Value, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let t = now() + 3600;
    let (m3u, epg) = write_source(
        dir.path(),
        "http://example.invalid/news.ts",
        &guide(&[
            (t, t + 1800, "Show", "Pilot"),
            (t + 1800, t + 3600, "Other", "x"),
            (t + 3600, t + 5400, "Show", "Second"),
        ]),
    );
    let app = app().await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, g) = call(
        &app,
        "GET",
        &format!("/api/v1/livetv/guide?start={t}&end={}", t + 5400),
        Some(&admin),
        None,
    )
    .await;
    (app, admin, g[0].clone(), dir)
}

async fn add_rule(app: &Router, token: &str, body: Value) -> (StatusCode, Value) {
    call(app, "POST", "/api/v1/livetv/rules", Some(token), Some(body)).await
}

async fn recordings(app: &Router, token: &str) -> Vec<Value> {
    let (status, v) = call(app, "GET", "/api/v1/livetv/recordings", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    v.as_array().unwrap().clone()
}

#[tokio::test]
async fn once_rule_schedules_padded_recording() {
    let (app, admin, news, _dir) = setup().await;
    let pilot = &news["programmes"][0];
    let (status, rule) = add_rule(
        &app,
        &admin,
        json!({"kind": "once", "programme_id": pilot["id"], "padding_before_s": 60, "padding_after_s": 300}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(rule["channel_id"], news["channel"]["id"]);
    let recs = recordings(&app, &admin).await;
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["title"], "Show");
    assert_eq!(recs[0]["subtitle"], "Pilot");
    assert_eq!(recs[0]["status"], "scheduled");
    assert_eq!(recs[0]["rule_id"], rule["id"]);
    assert_eq!(recs[0]["start"], pilot["start"].as_i64().unwrap() - 60);
    assert_eq!(recs[0]["stop"], pilot["stop"].as_i64().unwrap() + 300);
}

#[tokio::test]
async fn series_rule_from_programme_takes_every_episode() {
    let (app, admin, news, _dir) = setup().await;
    let (status, rule) = add_rule(
        &app,
        &admin,
        json!({"kind": "series", "programme_id": news["programmes"][0]["id"], "keep_last": 5}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(rule["series_title"], "Show");
    let subtitles: Vec<Value> = recordings(&app, &admin)
        .await
        .iter()
        .map(|r| r["subtitle"].clone())
        .collect();
    assert_eq!(subtitles, [json!("Pilot"), json!("Second")]);
}

#[tokio::test]
async fn series_rule_by_title() {
    let (app, admin, news, _dir) = setup().await;
    let (status, _) = add_rule(
        &app,
        &admin,
        json!({"kind": "series", "channel_id": news["channel"]["id"], "series_title": "Other"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let recs = recordings(&app, &admin).await;
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["title"], "Other");
}

#[tokio::test]
async fn incomplete_rules_are_rejected() {
    let (app, admin, news, _dir) = setup().await;
    for body in [
        json!({"kind": "once"}),
        json!({"kind": "once", "programme_id": 999_999}),
        json!({"kind": "series", "channel_id": news["channel"]["id"]}),
        json!({"kind": "series", "series_title": "Show"}),
        json!({"kind": "once", "programme_id": news["programmes"][0]["id"], "padding_before_s": -5}),
        json!({"kind": "series", "programme_id": news["programmes"][0]["id"], "keep_last": 0}),
    ] {
        let (status, _) = add_rule(&app, &admin, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
}

#[tokio::test]
async fn rules_and_recordings_are_per_user() {
    let (app, admin, news, _dir) = setup().await;
    let (_, rule) = add_rule(
        &app,
        &admin,
        json!({"kind": "once", "programme_id": news["programmes"][0]["id"]}),
    )
    .await;
    let kid = user_token(&app, &admin, "kid").await;
    let (status, rules) = call(&app, "GET", "/api/v1/livetv/rules", Some(&kid), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rules, json!([]));
    assert!(recordings(&app, &kid).await.is_empty());
    let uri = format!("/api/v1/livetv/rules/{}", rule["id"]);
    let (status, _) = call(&app, "DELETE", &uri, Some(&kid), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, rules) = call(&app, "GET", "/api/v1/livetv/rules", Some(&admin), None).await;
    assert_eq!(rules[0], rule);
}

#[tokio::test]
async fn dvr_records_into_the_recordings_library_and_keeps_last() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let stream = dir.path().join("live.ts");
    make_video(&stream, 10);
    let t = now();
    let (m3u, epg) = write_source(
        dir.path(),
        &stream.to_string_lossy(),
        &guide(&[
            // Pilot must still be airing once setup finishes, which takes seconds on slow CI
            // runners and more when the whole suite runs at once.
            (t - 10, t + 12, "Show", "Pilot"),
            (t + 12, t + 14, "Show", "Second"),
        ]),
    );
    let data = dir.path().join("data");
    let mut config = Config::for_tests(data.clone());
    config.dvr_tick = Some(Duration::from_millis(200));
    let app = app_with(config).await;
    let admin = admin_token(&app).await;
    add_m3u(&app, &admin, &m3u, &epg).await;
    let (_, g) = call(
        &app,
        "GET",
        &format!("/api/v1/livetv/guide?start={t}"),
        Some(&admin),
        None,
    )
    .await;
    let (status, _) = add_rule(
        &app,
        &admin,
        json!({"kind": "series", "programme_id": g[0]["programmes"][0]["id"], "keep_last": 1}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let deadline = Instant::now() + Duration::from_secs(45);
    let recs = loop {
        let recs = recordings(&app, &admin).await;
        if recs.len() == 2 && recs.iter().all(|r| r["status"] == "done") {
            break recs;
        }
        assert!(Instant::now() < deadline, "not recorded: {recs:?}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    // keep_last = 1: the older episode's file and item are gone.
    assert_eq!(recs[0]["item_id"], Value::Null);
    let item = recs[1]["item_id"].as_i64().unwrap();

    let (_, libs) = call(&app, "GET", "/api/v1/libraries", Some(&admin), None).await;
    assert_eq!(libs[0]["name"], "Recordings");
    assert_eq!(libs[0]["kind"], "movies");
    let (_, items) = call(
        &app,
        "GET",
        &format!("/api/v1/libraries/{}/items", libs[0]["id"]),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["id"], item);
    assert_eq!(items[0]["title"], "Show - Second");
    assert!(items[0]["duration_s"].as_f64().unwrap() > 0.0);
    let files: Vec<_> = std::fs::read_dir(data.join("recordings").join("Show"))
        .unwrap()
        .collect();
    assert_eq!(files.len(), 1);

    // Deleting a finished recording removes its file and library item; the record stays, so the
    // series rule does not record that episode again.
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/livetv/recordings/{}", recs[1]["id"]),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let recs = recordings(&app, &admin).await;
    assert_eq!(recs[1]["item_id"], Value::Null);
    let files: Vec<_> = std::fs::read_dir(data.join("recordings").join("Show"))
        .unwrap()
        .collect();
    assert_eq!(files.len(), 0);
    let (status, _) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{item}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_rule_cancels_its_recordings() {
    let (app, admin, news, _dir) = setup().await;
    let (_, rule) = add_rule(
        &app,
        &admin,
        json!({"kind": "series", "programme_id": news["programmes"][0]["id"]}),
    )
    .await;
    let uri = format!("/api/v1/livetv/rules/{}", rule["id"]);
    let (status, _) = call(&app, "DELETE", &uri, Some(&admin), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(recordings(&app, &admin).await.is_empty());
}
