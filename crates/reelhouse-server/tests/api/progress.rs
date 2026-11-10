use super::common::{admin_token, app_and_db, call};
use axum::http::StatusCode;
use reelhouse_server::config::Config;
use serde_json::json;
use sqlx::SqlitePool;

async fn movie(db: &SqlitePool, title: &str, duration: f64) -> i64 {
    sqlx::query(
        "INSERT OR IGNORE INTO libraries (id, name, kind, path) VALUES (1, 'm', 'movies', '/m')",
    )
    .execute(db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO items (library_id, kind, title, file_path, duration_s, added_at) VALUES (1, 'movie', ?, ?, ?, 0)")
        .bind(title)
        .bind(format!("/m/{title}.mkv"))
        .bind(duration)
        .execute(db)
        .await
        .unwrap()
        .last_insert_rowid()
}

async fn second_user(app: &axum::Router, admin: &str) -> String {
    call(
        app,
        "POST",
        "/api/v1/users",
        Some(admin),
        Some(json!({"name": "kid", "password": "password3"})),
    )
    .await;
    let (_, login) = call(
        app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"name": "kid", "password": "password3"})),
    )
    .await;
    login["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn progress_is_per_user() {
    let (app, db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let kid = second_user(&app, &admin).await;
    let id = movie(&db, "Heat", 100.0).await;
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/v1/items/{id}/progress"),
        Some(&admin),
        Some(json!({"position_s": 30.0, "played": false})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, mine) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(mine["progress"]["position_s"], 30.0);
    let (_, theirs) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{id}"),
        Some(&kid),
        None,
    )
    .await;
    assert!(theirs["progress"].is_null());
}

#[tokio::test]
async fn ninety_percent_marks_played() {
    let (app, db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let id = movie(&db, "Heat", 100.0).await;
    call(
        &app,
        "PUT",
        &format!("/api/v1/items/{id}/progress"),
        Some(&admin),
        Some(json!({"position_s": 91.0, "played": false})),
    )
    .await;
    let (_, item) = call(
        &app,
        "GET",
        &format!("/api/v1/items/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(item["progress"]["played"], true);
    let (_, cont) = call(&app, "GET", "/api/v1/continue", Some(&admin), None).await;
    assert_eq!(cont.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn continue_watching_orders_by_recent() {
    let (app, db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let a = movie(&db, "Alien", 100.0).await;
    let b = movie(&db, "Brazil", 100.0).await;
    call(
        &app,
        "PUT",
        &format!("/api/v1/items/{a}/progress"),
        Some(&admin),
        Some(json!({"position_s": 10.0, "played": false})),
    )
    .await;
    call(
        &app,
        "PUT",
        &format!("/api/v1/items/{b}/progress"),
        Some(&admin),
        Some(json!({"position_s": 10.0, "played": false})),
    )
    .await;
    let (_, cont) = call(&app, "GET", "/api/v1/continue", Some(&admin), None).await;
    assert_eq!(cont[0]["title"], "Brazil");
    assert_eq!(cont[1]["title"], "Alien");
}

#[tokio::test]
async fn recently_added_puts_newest_titles_and_shows_with_new_episodes_first() {
    let (app, db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let token = admin_token(&app).await;
    sqlx::query(
        "INSERT INTO libraries (id, name, kind, path) VALUES (1, 'm', 'movies', '/m'), (2, 't', 'tv', '/t')",
    )
    .execute(&db)
    .await
    .unwrap();
    let insert = |sql: &'static str| sqlx::query(sql).execute(&db);
    insert("INSERT INTO items (id, library_id, kind, title, file_path, added_at) VALUES (1, 1, 'movie', 'Old film', '/m/a', 100)").await.unwrap();
    insert("INSERT INTO items (id, library_id, kind, title, file_path, added_at) VALUES (2, 1, 'movie', 'New film', '/m/b', 300)").await.unwrap();
    // A show added long ago that just got a new episode.
    insert("INSERT INTO items (id, library_id, kind, title, added_at) VALUES (3, 2, 'show', 'Show', 50)").await.unwrap();
    insert("INSERT INTO items (id, library_id, kind, parent_id, title, season_no, added_at) VALUES (4, 2, 'season', 3, 'Season 1', 1, 50)").await.unwrap();
    insert("INSERT INTO items (id, library_id, kind, parent_id, title, season_no, episode_no, file_path, added_at) VALUES (5, 2, 'episode', 4, 'Pilot', 1, 1, '/t/1', 50)").await.unwrap();
    insert("INSERT INTO items (id, library_id, kind, parent_id, title, season_no, episode_no, file_path, added_at) VALUES (6, 2, 'episode', 4, 'Next', 1, 2, '/t/2', 500)").await.unwrap();

    let (status, recent) = call(&app, "GET", "/api/v1/recent", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let titles: Vec<&str> = recent
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Show", "New film", "Old film"]);
}
