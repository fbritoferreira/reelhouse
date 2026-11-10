use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use reelhouse_api::{Item, ItemKind, Library, LibraryKind, NewLibrary, Progress, ScanReport};
use sqlx::FromRow;

use crate::app::AppState;
use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::metadata::tmdb::Tmdb;
use crate::scan::scanner::scan_library;

fn kind_str(k: LibraryKind) -> &'static str {
    match k {
        LibraryKind::Movies => "movies",
        LibraryKind::Tv => "tv",
    }
}

pub async fn get_library(state: &AppState, id: i64) -> ApiResult<Library> {
    let (id, name, kind, path): (i64, String, String, String) =
        sqlx::query_as("SELECT id, name, kind, path FROM libraries WHERE id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let kind = if kind == "tv" {
        LibraryKind::Tv
    } else {
        LibraryKind::Movies
    };
    Ok(Library {
        id,
        name,
        kind,
        path,
    })
}

pub async fn list(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> ApiResult<Json<Vec<Library>>> {
    let ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM libraries ORDER BY name")
        .fetch_all(&state.db)
        .await?;
    let mut out = Vec::new();
    for (id,) in ids {
        out.push(get_library(&state, id).await?);
    }
    Ok(Json(out))
}

pub async fn create(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(new): Json<NewLibrary>,
) -> ApiResult<(StatusCode, Json<Library>)> {
    if !std::path::Path::new(&new.path).is_dir() {
        return Err(ApiError::BadRequest(format!(
            "{} is not a folder",
            new.path
        )));
    }
    let id = sqlx::query("INSERT INTO libraries (name, kind, path) VALUES (?, ?, ?)")
        .bind(&new.name)
        .bind(kind_str(new.kind))
        .bind(&new.path)
        .execute(&state.db)
        .await?
        .last_insert_rowid();
    Ok((
        StatusCode::CREATED,
        Json(Library {
            id,
            name: new.name,
            kind: new.kind,
            path: new.path,
        }),
    ))
}

pub async fn scan(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<ScanReport>> {
    let lib = get_library(&state, id).await?;
    let config = &state.config;
    let tmdb = config.tmdb_base.clone().map(|base| {
        let t = Tmdb::new(base, config.tmdb_api_key.clone());
        match &config.tmdb_image_base {
            Some(images) => t.with_image_base(images.clone()),
            None => t,
        }
    });
    let images = config.tmdb_image_base.is_some();
    // A detached task: if the browser leaves the page, the request is dropped but the scan finishes.
    let db = state.db.clone();
    let report = tokio::spawn(async move { scan_library(&db, &lib, tmdb.as_ref(), images).await })
        .await
        .map_err(|e| anyhow::anyhow!("scan task failed: {e}"))??;
    Ok(Json(report))
}

#[derive(FromRow)]
pub struct ItemRow {
    id: i64,
    kind: String,
    parent_id: Option<i64>,
    title: String,
    year: Option<i64>,
    season_no: Option<i64>,
    episode_no: Option<i64>,
    duration_s: Option<f64>,
    overview: Option<String>,
    poster_url: Option<String>,
    backdrop_url: Option<String>,
    position_s: Option<f64>,
    played: Option<bool>,
    has_poster: bool,
    has_backdrop: bool,
}

impl From<ItemRow> for Item {
    fn from(r: ItemRow) -> Self {
        let kind = match r.kind.as_str() {
            "show" => ItemKind::Show,
            "season" => ItemKind::Season,
            "episode" => ItemKind::Episode,
            _ => ItemKind::Movie,
        };
        Item {
            id: r.id,
            kind,
            parent_id: r.parent_id,
            title: r.title,
            year: r.year,
            season_no: r.season_no,
            episode_no: r.episode_no,
            duration_s: r.duration_s,
            overview: r.overview,
            // Stored artwork is served by this server; the TMDB URL is only a fallback.
            poster_url: if r.has_poster {
                Some(format!("/api/v1/items/{}/poster", r.id))
            } else {
                r.poster_url
            },
            backdrop_url: if r.has_backdrop {
                Some(format!("/api/v1/items/{}/backdrop", r.id))
            } else {
                r.backdrop_url
            },
            progress: r.position_s.map(|position_s| Progress {
                position_s,
                played: r.played.unwrap_or(false),
            }),
        }
    }
}

pub const ITEM_SELECT: &str = "SELECT i.id, i.kind, i.parent_id, i.title, i.year, i.season_no, i.episode_no, i.duration_s, \
     i.overview, i.poster_url, i.backdrop_url, p.position_s, p.played, \
     EXISTS (SELECT 1 FROM images WHERE item_id = i.id AND kind = 'poster') AS has_poster, \
     EXISTS (SELECT 1 FROM images WHERE item_id = i.id AND kind = 'backdrop') AS has_backdrop \
     FROM items i LEFT JOIN progress p ON p.item_id = i.id AND p.user_id = ?";

pub async fn items(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<Item>>> {
    let rows: Vec<ItemRow> = sqlx::query_as(&format!(
        "{ITEM_SELECT} WHERE i.library_id = ? AND i.parent_id IS NULL ORDER BY i.title"
    ))
    .bind(user.id)
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Item::from).collect()))
}

pub async fn item(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Item>> {
    let row: ItemRow = sqlx::query_as(&format!("{ITEM_SELECT} WHERE i.id = ?"))
        .bind(user.id)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(row.into()))
}

pub async fn children(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<Item>>> {
    let rows: Vec<ItemRow> = sqlx::query_as(&format!(
        "{ITEM_SELECT} WHERE i.parent_id = ? ORDER BY i.season_no, i.episode_no, i.title"
    ))
    .bind(user.id)
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Item::from).collect()))
}

async fn artwork(state: &AppState, id: i64, kind: &str) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let (content_type, bytes): (String, Vec<u8>) =
        sqlx::query_as("SELECT content_type, bytes FROM images WHERE item_id = ? AND kind = ?")
            .bind(id)
            .bind(kind)
            .fetch_one(&state.db)
            .await?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (
                axum::http::header::CACHE_CONTROL,
                "private, max-age=604800".to_string(),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub async fn poster(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<axum::response::Response> {
    artwork(&state, id, "poster").await
}

pub async fn backdrop(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<axum::response::Response> {
    artwork(&state, id, "backdrop").await
}
