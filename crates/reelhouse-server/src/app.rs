use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};

use crate::config::Config;
use crate::stream::hls::Sessions;
use crate::stream::routes as stream;
use crate::{import, libraries, progress, users};

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

pub const FEATURES: &[&str] = &["libraries", "users", "hls", "plex_import"];

pub fn router(state: AppState) -> Router {
    let web_dir = state.config.web_dir.clone();
    let api = Router::new()
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
        .route("/api/v1/items/{id}/progress", put(progress::put))
        .route("/api/v1/continue", get(progress::continue_watching))
        .route("/api/v1/import/plex", post(import::plex_import))
        .with_state(state);
    match web_dir {
        // Unknown paths fall back to index.html so the app's own routes load on refresh.
        Some(dir) => api.fallback_service(
            ServeDir::new(&dir).not_found_service(ServeFile::new(dir.join("index.html"))),
        ),
        None => api,
    }
}
