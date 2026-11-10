use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};

use crate::config::Config;
use crate::livetv::dvr::Recorder;
use crate::livetv::routes as livetv;
use crate::stream::hls::Sessions;
use crate::stream::routes as stream;
use crate::{import, libraries, progress, users};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Arc<Config>,
    pub hls: Arc<Sessions>,
    /// Wakes the job worker when work is queued.
    pub jobs: Arc<tokio::sync::Notify>,
}

impl AppState {
    /// Builds the state and starts the HLS reaper and the job worker (needs a tokio runtime).
    pub fn new(db: SqlitePool, config: Config) -> Self {
        let hls = Arc::new(Sessions::default());
        hls.spawn_reaper(config.hls_idle);
        if let Some(every) = config.dvr_tick {
            Recorder::new(db.clone(), config.data_dir.join("recordings")).spawn(every);
        }
        let config = Arc::new(config);
        let jobs = Arc::new(tokio::sync::Notify::new());
        crate::jobs::spawn_worker(db.clone(), config.clone(), jobs.clone());
        Self {
            db,
            config,
            hls,
            jobs,
        }
    }
}

pub const FEATURES: &[&str] = &["libraries", "users", "hls", "plex_import", "livetv", "dvr"];

pub fn router(state: AppState) -> Router {
    let web_dir = state.config.web_dir.clone();
    let api = Router::new()
        .route("/api/v1/server", get(users::server_info))
        .route("/api/v1/setup", post(users::setup))
        .route("/api/v1/auth/login", post(users::login))
        .route("/api/v1/auth/logout", post(users::logout))
        .route("/api/v1/auth/device", post(crate::device::start))
        .route("/api/v1/auth/device/approve", post(crate::device::approve))
        .route("/api/v1/auth/device/poll", post(crate::device::poll))
        .route("/api/v1/me", get(users::me))
        .route("/api/v1/users", get(users::list).post(users::create))
        .route("/api/v1/users/{id}", delete(users::remove))
        .route(
            "/api/v1/libraries",
            get(libraries::list).post(libraries::create),
        )
        .route("/api/v1/libraries/{id}", delete(libraries::remove))
        .route("/api/v1/libraries/{id}/scan", post(libraries::scan))
        .route("/api/v1/libraries/{id}/items", get(libraries::items))
        .route("/api/v1/items/{id}", get(libraries::item))
        .route("/api/v1/items/{id}/children", get(libraries::children))
        .route("/api/v1/items/{id}/poster", get(libraries::poster))
        .route("/api/v1/items/{id}/backdrop", get(libraries::backdrop))
        .route("/api/v1/items/{id}/playback", post(stream::playback))
        .route("/api/v1/items/{id}/file", get(stream::file))
        .route("/api/v1/items/{id}/offline", post(stream::offline))
        .route("/api/v1/items/{id}/subtitles", get(stream::subtitles))
        .route(
            "/api/v1/items/{id}/subtitles/{index}",
            get(stream::subtitle),
        )
        .route("/api/v1/items/{id}/mp4", get(stream::mp4))
        .route("/api/v1/hls/{session}/{name}", get(stream::hls_file))
        .route("/api/v1/items/{id}/progress", put(progress::put))
        .route("/api/v1/continue", get(progress::continue_watching))
        .route("/api/v1/jobs", get(crate::jobs::list))
        .route("/api/v1/jobs/{id}", get(crate::jobs::get))
        .route(
            "/api/v1/livetv/sources",
            get(livetv::list_sources).post(livetv::create_source),
        )
        .route("/api/v1/livetv/sources/{id}", delete(livetv::delete_source))
        .route(
            "/api/v1/livetv/sources/{id}/groups",
            get(livetv::source_groups).put(livetv::hide_groups),
        )
        .route(
            "/api/v1/livetv/sources/{id}/refresh",
            post(livetv::refresh_source),
        )
        .route("/api/v1/livetv/channels", get(livetv::channels))
        .route("/api/v1/livetv/channels/{id}/play", post(livetv::play))
        .route("/api/v1/livetv/guide", get(livetv::guide))
        .route("/api/v1/livetv/groups", get(livetv::groups))
        .route("/api/v1/vod/groups", get(crate::livetv::vod::groups))
        .route("/api/v1/vod/movies", get(crate::livetv::vod::movies))
        .route("/api/v1/vod/series", get(crate::livetv::vod::series))
        .route("/api/v1/vod/episodes", get(crate::livetv::vod::episodes))
        .route(
            "/api/v1/vod/{id}/playback",
            post(crate::livetv::vod::playback),
        )
        .route(
            "/api/v1/vod/continue",
            get(crate::livetv::vod::continue_watching),
        )
        .route("/api/v1/vod/search", get(crate::livetv::vod::search))
        .route("/api/v1/vod/{id}", get(crate::livetv::vod::title))
        .route("/api/v1/vod/{id}/file", get(crate::livetv::vod::file))
        .route(
            "/api/v1/vod/{id}/offline",
            post(crate::livetv::vod::offline),
        )
        .route("/api/v1/vod/{id}/mp4", get(crate::livetv::vod::mp4))
        .route(
            "/api/v1/vod/{id}/progress",
            put(crate::livetv::vod::put_progress),
        )
        .route(
            "/api/v1/livetv/rules",
            get(livetv::rules).post(livetv::create_rule),
        )
        .route("/api/v1/livetv/rules/{id}", delete(livetv::delete_rule))
        .route("/api/v1/livetv/recordings", get(livetv::recordings))
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
