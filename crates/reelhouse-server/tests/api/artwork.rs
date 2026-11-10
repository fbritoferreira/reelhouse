use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use reelhouse_server::config::Config;
use serde_json::json;
use sqlx::SqlitePool;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::common::{admin_token, app_on, call};

const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4];

/// Inserts a movies library holding one movie file, without needing ffmpeg.
async fn movie(db: &SqlitePool, dir: &Path, title: &str, year: i64) -> (i64, i64) {
    let lib = sqlx::query("INSERT INTO libraries (name, kind, path) VALUES ('m', 'movies', ?)")
        .bind(dir.to_string_lossy())
        .execute(db)
        .await
        .unwrap()
        .last_insert_rowid();
    let file = dir.join(format!("{title} ({year}).mkv"));
    std::fs::write(&file, b"").unwrap();
    let id = sqlx::query("INSERT INTO items (library_id, kind, title, year, file_path, added_at) VALUES (?, 'movie', ?, ?, ?, 0)")
        .bind(lib)
        .bind(title)
        .bind(year)
        .bind(file.to_string_lossy())
        .execute(db)
        .await
        .unwrap()
        .last_insert_rowid();
    (lib, id)
}

async fn tmdb_with_heat(server: &MockServer, searches: u64) {
    Mock::given(method("GET"))
        .and(path("/3/search/movie"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"results": [{
            "id": 949, "title": "Heat", "release_date": "1995-12-15",
            "overview": "Obsessive crime.", "poster_path": "/h.jpg", "backdrop_path": "/b.jpg"}]})),
        )
        .expect(searches)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/3/movie/949/external_ids"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"imdb_id": "tt0113277"})))
        .mount(server)
        .await;
    for p in ["/img/w500/h.jpg", "/img/w1280/b.jpg"] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/jpeg")
                    .set_body_bytes(JPEG),
            )
            .mount(server)
            .await;
    }
}

fn config_for(server: &MockServer) -> Config {
    let mut config = Config::for_tests(std::env::temp_dir());
    config.tmdb_base = Some(format!("{}/", server.uri()).parse().unwrap());
    config.tmdb_image_base = Some(format!("{}/img/", server.uri()).parse().unwrap());
    config
}

async fn scan(app: &axum::Router, token: &str, lib: i64) {
    let (status, _) = call(
        app,
        "POST",
        &format!("/api/v1/libraries/{lib}/scan"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn artwork_is_stored_and_served_locally() {
    let server = MockServer::start().await;
    tmdb_with_heat(&server, 1).await;
    let db = reelhouse_server::db::open_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (lib, id) = movie(&db, dir.path(), "Heat", 1995).await;
    let app = app_on(db.clone(), config_for(&server));
    let token = admin_token(&app).await;
    scan(&app, &token, lib).await;

    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM images WHERE item_id = ?")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 2, "poster and backdrop stored in SQLite");
    let (_, item) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(item["poster_url"], format!("/api/v1/items/{id}/poster"));
    assert_eq!(item["backdrop_url"], format!("/api/v1/items/{id}/backdrop"));

    let req = Request::get(format!("/api/v1/items/{id}/poster?token={token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/jpeg");
    assert_eq!(
        res.into_body().collect().await.unwrap().to_bytes().as_ref(),
        JPEG
    );
}

#[tokio::test]
async fn rescan_fills_missing_metadata() {
    let server = MockServer::start().await;
    tmdb_with_heat(&server, 1).await;
    let db = reelhouse_server::db::open_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (lib, id) = movie(&db, dir.path(), "Heat", 1995).await;

    // First scan with metadata off, like a server that ran before metadata was available.
    let offline = app_on(db.clone(), Config::for_tests(std::env::temp_dir()));
    let token = admin_token(&offline).await;
    scan(&offline, &token, lib).await;
    let (_, item) = call(
        &offline,
        "GET",
        &format!("/api/v1/items/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert!(item["overview"].is_null());

    let online = app_on(db.clone(), config_for(&server));
    scan(&online, &token, lib).await;
    let (_, item) = call(
        &online,
        "GET",
        &format!("/api/v1/items/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(item["overview"], "Obsessive crime.");
}

#[tokio::test]
async fn matched_items_are_not_looked_up_again() {
    let server = MockServer::start().await;
    // `expect(1)`: the second scan must not search again.
    tmdb_with_heat(&server, 1).await;
    let db = reelhouse_server::db::open_memory().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (lib, _) = movie(&db, dir.path(), "Heat", 1995).await;
    let app = app_on(db, config_for(&server));
    let token = admin_token(&app).await;
    scan(&app, &token, lib).await;
    scan(&app, &token, lib).await;
}

#[tokio::test]
async fn artwork_requires_a_token() {
    let db = reelhouse_server::db::open_memory().await.unwrap();
    let app = app_on(db, Config::for_tests(std::env::temp_dir()));
    let (status, _) = call(&app, "GET", "/api/v1/items/1/poster", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
