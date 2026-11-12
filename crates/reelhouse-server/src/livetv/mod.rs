//! IPTV sources (M3U and Xtream Codes), the XMLTV guide and the DVR.

pub mod dvr;
pub mod m3u;
pub mod routes;
pub mod vod;
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
    let mut vod = Vec::new();
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
        let epg = src.epg_url.or_else(|| m3u::epg_url(&text));
        vod = m3u::parse_vod(&text);
        (m3u::parse(&text), epg)
    };
    // Providers under load sometimes send a playlist cut short. Replacing the list with it would
    // drop most channels (and the choices made about them), so refuse and let the job retry.
    let (before,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM channels WHERE source_id = ?")
        .bind(source_id)
        .fetch_one(db)
        .await?;
    anyhow::ensure!(
        before < 10 || channels.len() as i64 * 2 >= before,
        "the source sent {} channels, against {before} last time; keeping the old list",
        channels.len()
    );
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
    // On-demand titles keep their ids across refreshes (people's progress hangs off them):
    // upsert by URL, then drop the ones this refresh did not list.
    let seen = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    for v in &vod {
        sqlx::query(
            "INSERT INTO vod_items (source_id, kind, title, year, show_title, season_no, episode_no, \
             poster_url, group_title, stream_url, seen) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (source_id, stream_url) DO UPDATE SET kind = excluded.kind, title = excluded.title, \
             year = excluded.year, show_title = excluded.show_title, season_no = excluded.season_no, \
             episode_no = excluded.episode_no, poster_url = excluded.poster_url, \
             group_title = excluded.group_title, seen = excluded.seen",
        )
        .bind(source_id)
        .bind(match v.kind {
            m3u::VodKind::Movie => "movie",
            m3u::VodKind::Episode => "episode",
        })
        .bind(&v.title)
        .bind(v.year)
        .bind(&v.show)
        .bind(v.season)
        .bind(v.episode)
        .bind(&v.poster_url)
        .bind(&v.group_title)
        .bind(&v.stream_url)
        .bind(seen)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM vod_items WHERE source_id = ? AND seen != ?")
        .bind(source_id)
        .bind(seen)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(RefreshReport {
        channels: ids.len() as u64,
        programmes,
        on_demand: vod.len() as u64,
    })
}

/// Queues a refresh of every source with none in the last `every`, so the guide keeps ahead and
/// new channels and titles appear without anyone pressing Refresh. Returns how many it queued.
pub async fn refresh_stale(
    state: &crate::app::AppState,
    every: std::time::Duration,
) -> crate::error::ApiResult<usize> {
    let since = crate::auth::now() - every.as_secs() as i64;
    let stale: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM tv_sources s WHERE NOT EXISTS (SELECT 1 FROM jobs j \
         WHERE j.kind = 'refresh_source' AND j.target_id = s.id AND j.created_at > ?)",
    )
    .bind(since)
    .fetch_all(&state.db)
    .await?;
    for (id,) in &stale {
        crate::jobs::enqueue(state, reelhouse_api::JobKind::RefreshSource, *id).await?;
    }
    Ok(stale.len())
}

/// Checks for stale sources hourly (see `refresh_stale`).
pub fn spawn_refresher(state: crate::app::AppState, every: std::time::Duration) {
    tokio::spawn(async move {
        loop {
            match refresh_stale(&state, every).await {
                Ok(0) => {}
                Ok(n) => tracing::info!("source refresh: queued {n} not refreshed lately"),
                Err(e) => tracing::error!("source refresh: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

/// The HTTP status a provider answers for `url`, without reading the stream. `None` for local
/// files and when it does not answer at all.
pub(crate) async fn provider_status(url: &str) -> Option<u16> {
    if !url.starts_with("http") {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;
    client
        .get(url)
        .send()
        .await
        .ok()
        .map(|r| r.status().as_u16())
}

/// Plain words for a provider's refusal of a stream, by HTTP status; `what` is "channel" or
/// "title".
pub(crate) fn refusal(status: u16, what: &str) -> Option<String> {
    match status {
        401 | 403 | 407 => Some(format!(
            "your provider won't play this {what} on your account"
        )),
        404 | 410 => Some(format!("your provider no longer carries this {what}")),
        429 => Some(
            "your provider's limit on streams at once is reached; stop another stream and try again"
                .into(),
        ),
        500..=599 => Some("your provider's server isn't answering; try again in a minute".into()),
        _ => None,
    }
}

#[cfg(test)]
mod refusal_tests {
    use super::refusal;

    #[test]
    fn provider_refusals_read_plainly() {
        assert_eq!(
            refusal(407, "channel").as_deref(),
            Some("your provider won't play this channel on your account")
        );
        assert_eq!(
            refusal(403, "title").as_deref(),
            Some("your provider won't play this title on your account")
        );
        assert_eq!(
            refusal(404, "title").as_deref(),
            Some("your provider no longer carries this title")
        );
        assert_eq!(
            refusal(429, "channel").as_deref(),
            Some(
                "your provider's limit on streams at once is reached; stop another stream and try again"
            )
        );
        assert_eq!(
            refusal(503, "channel").as_deref(),
            Some("your provider's server isn't answering; try again in a minute")
        );
        assert_eq!(refusal(200, "channel"), None);
        assert_eq!(refusal(206, "title"), None);
    }
}
