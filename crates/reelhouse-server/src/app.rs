use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post};
use sqlx::SqlitePool;

use crate::config::Config;
use crate::users;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Arc<Config>,
}

pub const FEATURES: &[&str] = &["libraries", "users"];

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/server", get(users::server_info))
        .route("/api/v1/setup", post(users::setup))
        .route("/api/v1/auth/login", post(users::login))
        .route("/api/v1/auth/logout", post(users::logout))
        .route("/api/v1/me", get(users::me))
        .route("/api/v1/users", get(users::list).post(users::create))
        .route("/api/v1/users/{id}", delete(users::remove))
        .with_state(state)
}
