//! Walks a library folder and syncs `items` with what is on disk.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use reelhouse_api::{Library, LibraryKind, ScanReport};
use sqlx::SqlitePool;

use super::naming::{Parsed, VIDEO_EXTENSIONS, parse_episode, parse_movie};
use super::probe::probe;
use crate::auth::now;
use crate::metadata::tmdb::{Meta, Tmdb};

fn video_files(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        })
        .collect()
}

/// Returns the id of the `kind` item titled `title` under `parent`, creating it if needed.
async fn parent_item(
    db: &SqlitePool,
    lib: i64,
    kind: &str,
    parent: Option<i64>,
    title: &str,
    season: Option<u16>,
) -> Result<i64> {
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM items WHERE library_id = ? AND kind = ? AND parent_id IS ? AND title = ?",
    )
    .bind(lib)
    .bind(kind)
    .bind(parent)
    .bind(title)
    .fetch_optional(db)
    .await?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    Ok(sqlx::query("INSERT INTO items (library_id, kind, parent_id, title, season_no, added_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(lib)
        .bind(kind)
        .bind(parent)
        .bind(title)
        .bind(season)
        .bind(now())
        .execute(db)
        .await?
        .last_insert_rowid())
}

pub async fn scan_library(
    db: &SqlitePool,
    lib: &Library,
    tmdb: Option<&Tmdb>,
    images: bool,
) -> Result<ScanReport> {
    let mut report = ScanReport {
        added: 0,
        updated: 0,
        removed: 0,
    };
    let files = video_files(Path::new(&lib.path));
    let on_disk: HashSet<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    for path in &files {
        let path_str = path.to_string_lossy().into_owned();
        let known: Option<(i64,)> = sqlx::query_as("SELECT id FROM items WHERE file_path = ?")
            .bind(&path_str)
            .fetch_optional(db)
            .await?;
        if known.is_some() {
            continue;
        }
        let info = probe(path).await.unwrap_or_default();
        let (kind, parent, title, year, season, episode) = match lib.kind {
            LibraryKind::Movies => match parse_movie(path) {
                Parsed::Movie { title, year } => ("movie", None, title, year, None, None),
                Parsed::Episode { .. } => unreachable!("parse_movie always returns Movie"),
            },
            LibraryKind::Tv => match parse_episode(path) {
                Some(Parsed::Episode {
                    show,
                    season,
                    episode,
                }) => {
                    let show_id = parent_item(db, lib.id, "show", None, &show, None).await?;
                    let season_id = parent_item(
                        db,
                        lib.id,
                        "season",
                        Some(show_id),
                        &format!("Season {season}"),
                        Some(season),
                    )
                    .await?;
                    let title = format!("{show} S{season:02}E{episode:02}");
                    (
                        "episode",
                        Some(season_id),
                        title,
                        None,
                        Some(season),
                        Some(episode),
                    )
                }
                _ => continue,
            },
        };
        sqlx::query(
            "INSERT INTO items (library_id, kind, parent_id, title, year, season_no, episode_no, file_path, \
             duration_s, container, video_codec, audio_codec, height, added_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(lib.id)
        .bind(kind)
        .bind(parent)
        .bind(&title)
        .bind(year)
        .bind(season)
        .bind(episode)
        .bind(&path_str)
        .bind(info.duration_s)
        .bind(&info.container)
        .bind(&info.video_codec)
        .bind(&info.audio_codec)
        .bind(info.height)
        .bind(now())
        .execute(db)
        .await?;
        report.added += 1;
    }

    let stored: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, file_path FROM items WHERE library_id = ? AND file_path IS NOT NULL",
    )
    .bind(lib.id)
    .fetch_all(db)
    .await?;
    for (id, path) in stored {
        if !on_disk.contains(&path) {
            sqlx::query("DELETE FROM items WHERE id = ?")
                .bind(id)
                .execute(db)
                .await?;
            report.removed += 1;
        }
    }
    // Seasons, then shows, left without children.
    for kind in ["season", "show"] {
        sqlx::query(
            "DELETE FROM items WHERE library_id = ? AND kind = ? AND NOT EXISTS (SELECT 1 FROM items c WHERE c.parent_id = items.id)",
        )
        .bind(lib.id)
        .bind(kind)
        .execute(db)
        .await?;
    }
    super::enrich::enrich(db, lib.id, tmdb, images).await?;
    Ok(report)
}

/// Fills metadata columns. Episode titles are prefixed with their SxxEyy code.
pub(super) async fn apply_meta(db: &SqlitePool, id: i64, meta: &Meta, episode: bool) -> Result<()> {
    let title = match (&meta.title, episode) {
        (Some(t), true) => {
            let (code,): (String,) = sqlx::query_as(
                "SELECT printf('S%02dE%02d', season_no, episode_no) FROM items WHERE id = ?",
            )
            .bind(id)
            .fetch_one(db)
            .await?;
            Some(format!("{code} · {t}"))
        }
        (t, _) => t.clone(),
    };
    sqlx::query(
        "UPDATE items SET title = COALESCE(?, title), year = COALESCE(?, year), tmdb_id = COALESCE(?, tmdb_id), \
         imdb_id = COALESCE(?, imdb_id), tvdb_id = COALESCE(?, tvdb_id), overview = COALESCE(?, overview), \
         poster_url = COALESCE(?, poster_url), backdrop_url = COALESCE(?, backdrop_url) WHERE id = ?",
    )
    .bind(title)
    .bind(meta.year)
    .bind(meta.tmdb_id)
    .bind(&meta.imdb_id)
    .bind(meta.tvdb_id)
    .bind(&meta.overview)
    .bind(&meta.poster_url)
    .bind(&meta.backdrop_url)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}
