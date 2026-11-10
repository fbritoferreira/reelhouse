mod common;

use axum::http::StatusCode;
use common::{admin_token, app, call};
use serde_json::json;

#[tokio::test]
async fn setup_only_once() {
    let app = app().await;
    let first = call(
        &app,
        "POST",
        "/api/v1/setup",
        None,
        Some(json!({"name": "a", "password": "password1"})),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    let second = call(
        &app,
        "POST",
        "/api/v1/setup",
        None,
        Some(json!({"name": "b", "password": "password2"})),
    )
    .await;
    assert_eq!(second.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn login_rejects_bad_password() {
    let app = app().await;
    admin_token(&app).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"name": "admin", "password": "wrong password"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_requires_token() {
    let app = app().await;
    let token = admin_token(&app).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/me", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, me) = call(&app, "GET", "/api/v1/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["name"], "admin");
}

#[tokio::test]
async fn non_admin_cannot_create_users() {
    let app = app().await;
    let admin = admin_token(&app).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        Some(&admin),
        Some(json!({"name": "kid", "password": "password3"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, login) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"name": "kid", "password": "password3"})),
    )
    .await;
    let kid = login["token"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        Some(kid),
        Some(json!({"name": "x", "password": "password4"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn server_info_lists_features() {
    let app = app().await;
    let (_, info) = call(&app, "GET", "/api/v1/server", None, None).await;
    assert!(
        info["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "libraries")
    );
}
