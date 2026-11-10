//! Second scan pass: looks up metadata for items that have never been looked up, then downloads
//! artwork into SQLite so the server can serve it without TMDB.

use anyhow::Result;
use futures::StreamExt;
use sqlx::SqlitePool;

use super::scanner::apply_meta;
use crate::auth::now;
use crate::metadata::tmdb::Tmdb;

/// Concurrent lookups and downloads; the proxy and TMDB both cope well with this.
const PARALLEL: usize = 8;
/// Artwork larger than this is not stored.
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

#[derive(sqlx::FromRow)]
struct Pending {
    id: i64,
    kind: String,
    title: String,
    year: Option<i64>,
    season_no: Option<i64>,
    episode_no: Option<i64>,
}

pub async fn enrich(
    db: &SqlitePool,
    library_id: i64,
    tmdb: Option<&Tmdb>,
    images: bool,
) -> Result<()> {
    if let Some(t) = tmdb {
        // Shows and movies first: episodes need their show's TMDB id.
        for kinds in [&["show", "movie", "season"][..], &["episode"][..]] {
            let placeholders = vec!["?"; kinds.len()].join(", ");
            let sql = format!(
                "SELECT id, kind, title, year, season_no, episode_no FROM items \
                 WHERE library_id = ? AND metadata_at IS NULL AND kind IN ({placeholders})"
            );
            let mut q = sqlx::query_as::<_, Pending>(&sql).bind(library_id);
            for k in kinds {
                q = q.bind(*k);
            }
            let pending = q.fetch_all(db).await?;
            futures::stream::iter(pending)
                .for_each_concurrent(PARALLEL, |item| async move {
                    if let Err(e) = look_up(db, t, &item).await {
                        tracing::warn!("metadata for {} ({}): {e:#}", item.title, item.id);
                    }
                })
                .await;
        }
    }
    if images {
        download_artwork(db, library_id).await?;
    }
    Ok(())
}

/// Looks one item up. Marks it as looked up unless the lookup itself failed, so network errors are
/// retried on the next scan but "no match" is not.
async fn look_up(db: &SqlitePool, t: &Tmdb, item: &Pending) -> Result<()> {
    let meta = match item.kind.as_str() {
        "movie" => {
            t.movie(&item.title, item.year.and_then(|y| u16::try_from(y).ok()))
                .await?
        }
        "show" => t.show(&item.title).await?,
        "episode" => {
            let (show_tmdb,): (Option<i64>,) = sqlx::query_as(
                "SELECT show.tmdb_id FROM items ep JOIN items season ON season.id = ep.parent_id \
                 JOIN items show ON show.id = season.parent_id WHERE ep.id = ?",
            )
            .bind(item.id)
            .fetch_one(db)
            .await?;
            match (show_tmdb, item.season_no, item.episode_no) {
                (Some(show), Some(s), Some(e)) => {
                    t.episode(show, u16::try_from(s)?, u16::try_from(e)?)
                        .await?
                }
                _ => None,
            }
        }
        _ => None,
    };
    if let Some(meta) = meta {
        apply_meta(db, item.id, &meta, item.kind == "episode").await?;
    }
    sqlx::query("UPDATE items SET metadata_at = ? WHERE id = ?")
        .bind(now())
        .bind(item.id)
        .execute(db)
        .await?;
    Ok(())
}

async fn download_artwork(db: &SqlitePool, library_id: i64) -> Result<()> {
    let missing: Vec<(i64, &'static str, String)> = {
        let mut out = Vec::new();
        for (kind, column) in [("poster", "poster_url"), ("backdrop", "backdrop_url")] {
            let sql = format!(
                "SELECT id, {column} FROM items i WHERE library_id = ? AND {column} IS NOT NULL \
                 AND NOT EXISTS (SELECT 1 FROM images WHERE item_id = i.id AND kind = ?)"
            );
            let rows: Vec<(i64, String)> = sqlx::query_as(&sql)
                .bind(library_id)
                .bind(kind)
                .fetch_all(db)
                .await?;
            out.extend(rows.into_iter().map(|(id, url)| (id, kind, url)));
        }
        out
    };
    let http = reqwest::Client::new();
    futures::stream::iter(missing)
        .for_each_concurrent(PARALLEL, |(id, kind, url)| {
            let http = http.clone();
            async move {
                if let Err(e) = store_image(db, &http, id, kind, &url).await {
                    tracing::warn!("artwork {url}: {e:#}");
                }
            }
        })
        .await;
    Ok(())
}

async fn store_image(
    db: &SqlitePool,
    http: &reqwest::Client,
    id: i64,
    kind: &str,
    url: &str,
) -> Result<()> {
    let res = http.get(url).send().await?.error_for_status()?;
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    anyhow::ensure!(
        content_type.starts_with("image/"),
        "not an image: {content_type}"
    );
    let bytes = res.bytes().await?;
    anyhow::ensure!(
        bytes.len() <= MAX_IMAGE_BYTES,
        "image too large: {} bytes",
        bytes.len()
    );
    sqlx::query(
        "INSERT OR REPLACE INTO images (item_id, kind, content_type, bytes, source_url, fetched_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(kind)
    .bind(content_type)
    .bind(bytes.as_ref())
    .bind(url)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}
