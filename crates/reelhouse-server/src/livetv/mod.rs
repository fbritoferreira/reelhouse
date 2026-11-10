//! IPTV sources (M3U and Xtream Codes), the XMLTV guide and the DVR.

pub mod dvr;
pub mod m3u;
pub mod routes;
pub mod xmltv;
pub mod xtream;

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use reelhouse_api::RefreshReport;
use sqlx::SqlitePool;

use self::xtream::Xtream;

/// A channel as a source lists it, before it is stored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceChannel {
    pub number: Option<i64>,
    pub name: String,
    pub logo_url: Option<String>,
    pub stream_url: String,
    pub epg_channel_id: Option<String>,
    pub group_title: Option<String>,
}

/// The body of an `http(s)` URL, or the contents of a local file for anything else.
pub async fn fetch_text(url: &str) -> Result<String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(reqwest::get(url).await?.error_for_status()?.text().await?)
    } else {
        Ok(tokio::fs::read_to_string(url).await?)
    }
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    kind: String,
    url: String,
    username: Option<String>,
    password: Option<String>,
    epg_url: Option<String>,
}

/// Reloads a source's channels and replaces their guide. Channels found again keep their ids.
pub async fn refresh(db: &SqlitePool, source_id: i64) -> Result<RefreshReport> {
    let src: SourceRow = sqlx::query_as(
        "SELECT kind, url, username, password, epg_url FROM tv_sources WHERE id = ?",
    )
    .bind(source_id)
    .fetch_one(db)
    .await?;
    let (channels, epg_url) = if src.kind == "xtream" {
        let xc = Xtream::new(
            &src.url,
            src.username.as_deref().unwrap_or_default(),
            src.password.as_deref().unwrap_or_default(),
        );
        let epg = src.epg_url.unwrap_or_else(|| xc.xmltv_url());
        (xc.channels().await?, Some(epg))
    } else {
        let text = fetch_text(&src.url)
            .await
            .with_context(|| format!("reading {}", src.url))?;
        (m3u::parse(&text), src.epg_url)
    };
    let guide = match &epg_url {
        Some(url) => xmltv::parse(
            &fetch_text(url)
                .await
                .with_context(|| format!("reading guide {url}"))?,
        )?,
        None => xmltv::Guide::default(),
    };

    let mut tx = db.begin().await?;
    let mut ids = HashSet::new();
    // Guide channel id -> our channel ids; several feeds (SD, HD) may share one guide id.
    let mut by_epg: HashMap<String, Vec<i64>> = HashMap::new();
    let names: HashMap<String, &str> = guide
        .channels
        .iter()
        .filter_map(|c| Some((c.name.as_ref()?.to_lowercase(), c.id.as_str())))
        .collect();
    for (i, c) in channels.iter().enumerate() {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO channels (source_id, number, name, logo_url, stream_url, epg_channel_id, group_title) \
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (source_id, stream_url) DO UPDATE SET \
             number = excluded.number, name = excluded.name, logo_url = excluded.logo_url, \
             epg_channel_id = excluded.epg_channel_id, group_title = excluded.group_title RETURNING id",
        )
        .bind(source_id)
        .bind(c.number.unwrap_or(i as i64 + 1))
        .bind(&c.name)
        .bind(&c.logo_url)
        .bind(&c.stream_url)
        .bind(&c.epg_channel_id)
        .bind(&c.group_title)
        .fetch_one(&mut *tx)
        .await?;
        ids.insert(id);
        // Without a tvg-id, fall back to the guide's display name.
        let epg_id = c
            .epg_channel_id
            .as_deref()
            .or_else(|| names.get(&c.name.to_lowercase()).copied());
        if let Some(e) = epg_id {
            by_epg.entry(e.to_string()).or_default().push(id);
        }
    }
    let stored: Vec<(i64,)> = sqlx::query_as("SELECT id FROM channels WHERE source_id = ?")
        .bind(source_id)
        .fetch_all(&mut *tx)
        .await?;
    for (id,) in stored.into_iter().filter(|(id,)| !ids.contains(id)) {
        sqlx::query("DELETE FROM channels WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "DELETE FROM programmes WHERE channel_id IN (SELECT id FROM channels WHERE source_id = ?)",
    )
    .bind(source_id)
    .execute(&mut *tx)
    .await?;
    let mut programmes = 0;
    for p in &guide.programmes {
        for channel_id in by_epg.get(&p.channel).into_iter().flatten() {
            sqlx::query(
                "INSERT INTO programmes (channel_id, start, stop, title, subtitle, description, series_key) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(channel_id)
            .bind(p.start)
            .bind(p.stop)
            .bind(&p.title)
            .bind(&p.subtitle)
            .bind(&p.description)
            .bind(&p.series_key)
            .execute(&mut *tx)
            .await?;
            programmes += 1;
        }
    }
    tx.commit().await?;
    Ok(RefreshReport {
        channels: ids.len() as u64,
        programmes,
    })
}
