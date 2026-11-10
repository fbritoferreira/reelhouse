use axum::http::StatusCode;
use reelhouse_server::config::Config;
use serde_json::json;

use super::common::{admin_token, app_and_db, call};

async fn start(app: &axum::Router) -> (String, String) {
    let (status, body) = call(app, "POST", "/api/v1/auth/device", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["expires_in"], 600);
    (
        body["code"].as_str().unwrap().to_string(),
        body["poll_token"].as_str().unwrap().to_string(),
    )
}

async fn poll(app: &axum::Router, poll_token: &str) -> (StatusCode, serde_json::Value) {
    call(
        app,
        "POST",
        "/api/v1/auth/device/poll",
        None,
        Some(json!({"poll_token": poll_token})),
    )
    .await
}

#[tokio::test]
async fn approved_device_signs_in_once() {
    let (app, _) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let (code, poll_token) = start(&app).await;
    assert_eq!(code.len(), 9, "shown as XXXX-XXXX");

    assert_eq!(
        poll(&app, &poll_token).await.0,
        StatusCode::ACCEPTED,
        "pending before approval"
    );
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/auth/device/approve",
        Some(&admin),
        Some(json!({"code": code})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, login) = poll(&app, &poll_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login["user"]["name"], "admin");
    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/me",
        Some(login["token"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the device's token works");

    assert_eq!(
        poll(&app, &poll_token).await.0,
        StatusCode::GONE,
        "single use"
    );
}

#[tokio::test]
async fn approval_accepts_how_people_type_codes() {
    let (app, _) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let (code, _) = start(&app).await;
    let typed = code.replace('-', " ").to_lowercase();
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/auth/device/approve",
        Some(&admin),
        Some(json!({"code": typed})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn unknown_code_is_not_found() {
    let (app, _) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/auth/device/approve",
        Some(&admin),
        Some(json!({"code": "AAAA-AAAA"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn approving_needs_a_signed_in_user() {
    let (app, _) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let (code, _) = start(&app).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/auth/device/approve",
        None,
        Some(json!({"code": code})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_code_is_gone() {
    let (app, db) = app_and_db(Config::for_tests(std::env::temp_dir())).await;
    let admin = admin_token(&app).await;
    let (code, poll_token) = start(&app).await;
    sqlx::query("UPDATE device_codes SET expires_at = 0")
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/auth/device/approve",
        Some(&admin),
        Some(json!({"code": code})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(poll(&app, &poll_token).await.0, StatusCode::GONE);
}
