//! Fixtures follow the `MediaContainer` shapes in the community Plex OpenAPI spec
//! (github.com/LukeHagar/plex-api-spec, `plex-api-spec.yaml`).

use super::common::{admin_token, app_and_db, call};
use axum::http::StatusCode;
use reelhouse_server::config::Config;
use reelhouse_server::import::plex::{Kind, Plex, PlexEntry};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const TOKEN: &str = "plex-secret";

pub fn sections() -> Value {
    json!({"MediaContainer": {"size": 3, "Directory": [
        {"key": "1", "type": "movie", "title": "Movies"},
        {"key": "2", "type": "show", "title": "TV Shows"},
        {"key": "3", "type": "artist", "title": "Music"}
    ]}})
}

pub fn movies() -> Value {
    json!({"MediaContainer": {"size": 2, "Metadata": [
        {"ratingKey": "10", "type": "movie", "title": "Heat", "year": 1995,
         "viewOffset": 4_500_000, "lastViewedAt": 1_700_000_000,
         "Guid": [{"id": "imdb://tt0113277"}, {"id": "tmdb://949"}],
         "Media": [{"id": 1, "Part": [{"id": 1, "file": "/data/movies/Heat (1995)/Heat.mkv"}]}]},
        {"ratingKey": "11", "type": "movie", "title": "Unwatched", "year": 2001,
         "Media": [{"id": 2, "Part": [{"id": 2, "file": "/data/movies/Unwatched.mkv"}]}]}
    ]}})
}

pub fn episodes() -> Value {
    json!({"MediaContainer": {"size": 1, "Metadata": [
        {"ratingKey": "150", "type": "episode", "title": "The Illusion of Truth",
         "grandparentTitle": "Babylon 5", "parentIndex": 4, "index": 8, "viewCount": 1,
         "Media": [{"id": 376, "Part": [{"id": 872,
            "file": "/data/tv/Babylon 5/Season 04/Babylon 5 S04E08.mkv"}]}]}
    ]}})
}

/// Mounts sections plus one movie and one TV library, each requiring the token and JSON.
pub async fn mount_library(server: &MockServer) {
    for (p, body, ty) in [
        ("/library/sections", sections(), None),
        ("/library/sections/1/all", movies(), Some("1")),
        ("/library/sections/2/all", episodes(), Some("4")),
    ] {
        let mut mock = Mock::given(method("GET"))
            .and(path(p))
            .and(header("X-Plex-Token", TOKEN))
            .and(header("Accept", "application/json"));
        if let Some(ty) = ty {
            mock = mock
                .and(query_param("type", ty))
                .and(query_param("includeGuids", "1"));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
}

fn plex(server: &MockServer) -> Plex {
    Plex::new(Url::parse(&server.uri()).unwrap(), TOKEN.into())
}

#[tokio::test]
async fn reads_movies_and_episodes() {
    let server = MockServer::start().await;
    mount_library(&server).await;
    let entries = plex(&server).entries().await.unwrap();
    assert_eq!(
        entries,
        vec![
            PlexEntry {
                kind: Kind::Movie,
                label: "Heat (1995)".into(),
                files: vec!["/data/movies/Heat (1995)/Heat.mkv".into()],
                guids: vec!["imdb://tt0113277".into(), "tmdb://949".into()],
                view_offset_ms: 4_500_000,
                view_count: 0,
                last_viewed_at: Some(1_700_000_000),
            },
            PlexEntry {
                kind: Kind::Movie,
                label: "Unwatched (2001)".into(),
                files: vec!["/data/movies/Unwatched.mkv".into()],
                guids: vec![],
                view_offset_ms: 0,
                view_count: 0,
                last_viewed_at: None,
            },
            PlexEntry {
                kind: Kind::Episode,
                label: "Babylon 5 S04E08".into(),
                files: vec!["/data/tv/Babylon 5/Season 04/Babylon 5 S04E08.mkv".into()],
                guids: vec![],
                view_offset_ms: 0,
                view_count: 1,
                last_viewed_at: None,
            },
        ]
    );
}

#[tokio::test]
async fn bad_token_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let err = plex(&server).entries().await.unwrap_err();
    assert!(!format!("{err:#}").contains(TOKEN));
}

async fn insert(db: &SqlitePool, kind: &str, file: &str, tmdb: Option<i64>) -> i64 {
    sqlx::query(
        "INSERT OR IGNORE INTO libraries (id, name, kind, path) VALUES (1, 'm', 'movies', '/m')",
    )
    .execute(db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO items (library_id, kind, title, file_path, tmdb_id, duration_s, added_at) VALUES (1, ?, 't', ?, ?, 10000, 0)")
        .bind(kind)
        .bind(file)
        .bind(tmdb)
        .execute(db)
        .await
        .unwrap()
        .last_insert_rowid()
}

async fn progress(db: &SqlitePool, item: i64) -> (f64, bool) {
    sqlx::query_as("SELECT position_s, played FROM progress WHERE item_id = ?")
        .bind(item)
        .fetch_one(db)
        .await
        .unwrap()
}

fn request(server: &MockServer) -> Value {
    json!({"url": server.uri(), "token": TOKEN, "user_id": 1,
           "path_rewrites": [{"from": "/data", "to": "/media"}]})
}

async fn import(app: &axum::Router, admin: &str, body: Value) -> (StatusCode, Value) {
    call(app, "POST", "/api/v1/import/plex", Some(admin), Some(body)).await
}

#[tokio::test]
async fn imports_watch_state_and_reruns_cleanly() {
    let server = MockServer::start().await;
    mount_library(&server).await;
    Mock::given(method("GET"))
        .and(path("/livetv/dvrs"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"MediaContainer": {"size": 1, "DVR": [{"key": "1"}]}})),
        )
        .mount(&server)
        .await;
    let (app, db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let heat = insert(&db, "movie", "/somewhere/else.mkv", Some(949)).await;
    let ep = insert(
        &db,
        "episode",
        "/media/tv/Babylon 5/Season 04/Babylon 5 S04E08.mkv",
        None,
    )
    .await;

    let (status, report) = import(&app, &admin, request(&server)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["matched"], 2);
    assert_eq!(report["progress_written"], 2);
    assert_eq!(report["unmatched"], json!([]));
    let notes = report["notes"].to_string();
    assert!(notes.contains("1 DVR"), "{notes}");
    assert!(notes.contains("Plex Home"), "{notes}");
    assert_eq!(progress(&db, heat).await, (4500.0, false));
    assert_eq!(progress(&db, ep).await, (0.0, true));

    let (_, again) = import(&app, &admin, request(&server)).await;
    assert_eq!(again["matched"], 2);
    assert_eq!(again["progress_written"], 0);

    // Watched further in Reelhouse since: a re-run must not rewind or unplay.
    sqlx::query("UPDATE progress SET position_s = 6000, played = 1 WHERE item_id = ?")
        .bind(heat)
        .execute(&db)
        .await
        .unwrap();
    import(&app, &admin, request(&server)).await;
    assert_eq!(progress(&db, heat).await, (6000.0, true));
}

#[tokio::test]
async fn unmatched_items_are_reported_not_guessed() {
    let server = MockServer::start().await;
    mount_library(&server).await;
    let (app, _db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let (_, report) = import(&app, &admin, request(&server)).await;
    assert_eq!(report["matched"], 0);
    assert_eq!(report["progress_written"], 0);
    let unmatched = report["unmatched"].as_array().unwrap();
    assert_eq!(unmatched.len(), 2, "unwatched Plex items are not listed");
    assert!(unmatched[0].as_str().unwrap().starts_with("Heat (1995)"));
}

#[tokio::test]
async fn import_is_admin_only_and_validated() {
    let server = MockServer::start().await;
    let (app, _db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/import/plex",
        None,
        Some(request(&server)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let admin = admin_token(&app).await;
    call(
        &app,
        "POST",
        "/api/v1/users",
        Some(&admin),
        Some(json!({"name": "kid", "password": "password3"})),
    )
    .await;
    let (_, login) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"name": "kid", "password": "password3"})),
    )
    .await;
    let kid = login["token"].as_str().unwrap();
    let (status, _) = import(&app, kid, request(&server)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let mut bad = request(&server);
    bad["user_id"] = json!(99);
    assert_eq!(import(&app, &admin, bad).await.0, StatusCode::NOT_FOUND);
    let mut bad = request(&server);
    bad["url"] = json!("not a url");
    assert_eq!(import(&app, &admin, bad).await.0, StatusCode::BAD_REQUEST);
    // Plex rejects the token: a client error, not a server fault.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    assert_eq!(
        import(&app, &admin, request(&server)).await.0,
        StatusCode::BAD_GATEWAY
    );
}

#[tokio::test]
async fn counts_live_tv_objects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/livetv/dvrs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"MediaContainer": {"size": 1, "DVR": [{"key": "1", "uuid": "abc"}]}}),
        ))
        .mount(&server)
        .await;
    assert_eq!(plex(&server).count("livetv/dvrs", "DVR").await.unwrap(), 1);
    Mock::given(method("GET"))
        .and(path("/media/subscriptions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"MediaContainer": {"size": 0}})),
        )
        .mount(&server)
        .await;
    assert_eq!(
        plex(&server)
            .count("media/subscriptions", "MediaSubscription")
            .await
            .unwrap(),
        0
    );
}
