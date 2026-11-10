use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use reelhouse_server::app::{AppState, router};
use reelhouse_server::config::Config;
use serde_json::{Value, json};
use tower::ServiceExt;

pub async fn app_with(config: Config) -> Router {
    let db = reelhouse_server::db::open_memory().await.unwrap();
    router(AppState {
        db,
        config: Arc::new(config),
    })
}

pub async fn app() -> Router {
    app_with(Config::for_tests(std::env::temp_dir())).await
}

pub async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub async fn admin_token(app: &Router) -> String {
    call(
        app,
        "POST",
        "/api/v1/setup",
        None,
        Some(json!({"name": "admin", "password": "correct horse"})),
    )
    .await;
    let (_, v) = call(
        app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"name": "admin", "password": "correct horse"})),
    )
    .await;
    v["token"].as_str().unwrap().to_string()
}
