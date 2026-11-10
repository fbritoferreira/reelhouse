use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post};
use sqlx::SqlitePool;

use crate::config::Config;
use crate::stream::hls::Sessions;
use crate::stream::routes as stream;
use crate::{libraries, users};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Arc<Config>,
    pub hls: Arc<Sessions>,
}

impl AppState {
    /// Builds the state and starts the HLS reaper (needs a tokio runtime).
    pub fn new(db: SqlitePool, config: Config) -> Self {
        let hls = Arc::new(Sessions::default());
        hls.spawn_reaper(config.hls_idle);
        Self {
            db,
            config: Arc::new(config),
            hls,
        }
    }
}

pub const FEATURES: &[&str] = &["libraries", "users", "hls"];

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/server", get(users::server_info))
        .route("/api/v1/setup", post(users::setup))
        .route("/api/v1/auth/login", post(users::login))
        .route("/api/v1/auth/logout", post(users::logout))
        .route("/api/v1/me", get(users::me))
        .route("/api/v1/users", get(users::list).post(users::create))
        .route("/api/v1/users/{id}", delete(users::remove))
        .route(
            "/api/v1/libraries",
            get(libraries::list).post(libraries::create),
        )
        .route("/api/v1/libraries/{id}/scan", post(libraries::scan))
        .route("/api/v1/libraries/{id}/items", get(libraries::items))
        .route("/api/v1/items/{id}", get(libraries::item))
        .route("/api/v1/items/{id}/children", get(libraries::children))
        .route("/api/v1/items/{id}/playback", post(stream::playback))
        .route("/api/v1/items/{id}/file", get(stream::file))
        .route("/api/v1/hls/{session}/{name}", get(stream::hls_file))
        .with_state(state)
}
