use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

const TMDB_BASE: &str = "https://api.themoviedb.org/";
const TMDB_IMAGES: &str = "https://image.tmdb.org/t/p/";
/// Shared proxy holding the project's TMDB key, cached at Cloudflare's edge (workers/metadata).
const METADATA_PROXY: &str = "https://reelhouse-metadata.fbritoferreira.workers.dev/";

fn metadata_base(have_key: bool) -> anyhow::Result<Option<url::Url>> {
    match std::env::var("REELHOUSE_METADATA_URL")
        .ok()
        .filter(|u| !u.is_empty())
    {
        Some(u) if u == "off" => Ok(None),
        Some(u) => Ok(Some(u.parse()?)),
        None if have_key => Ok(Some(TMDB_BASE.parse()?)),
        None => Ok(Some(METADATA_PROXY.parse()?)),
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    /// With a key, the server calls TMDB directly; without one it uses the Reelhouse metadata proxy.
    pub tmdb_api_key: Option<String>,
    /// Where metadata lookups go; `None` turns metadata off (`REELHOUSE_METADATA_URL=off`).
    pub tmdb_base: Option<url::Url>,
    /// Where artwork is downloaded from; `None` skips downloads (tests).
    pub tmdb_image_base: Option<url::Url>,
    /// Built web app to serve at `/`, if any.
    pub web_dir: Option<PathBuf>,
    /// HLS sessions with no request for this long are stopped.
    pub hls_idle: Duration,
    /// How often the DVR checks its schedule; `None` leaves the DVR off.
    pub dvr_tick: Option<Duration>,
    /// Wait before a failed job's first retry; it doubles for each retry after.
    pub job_backoff: Duration,
    /// Live TV sources are refreshed when their last refresh is older than this; `None` leaves
    /// it to the Refresh button.
    pub source_refresh: Option<Duration>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let tmdb_api_key = std::env::var("TMDB_API_KEY").ok().filter(|k| !k.is_empty());
        Ok(Self {
            bind: std::env::var("REELHOUSE_BIND")
                .unwrap_or_else(|_| "0.0.0.0:8420".into())
                .parse()?,
            data_dir: std::env::var("REELHOUSE_DATA_DIR")
                .unwrap_or_else(|_| "./data".into())
                .into(),
            tmdb_api_key: tmdb_api_key.clone(),
            tmdb_base: metadata_base(tmdb_api_key.is_some())?,
            tmdb_image_base: Some(TMDB_IMAGES.parse()?),
            web_dir: std::env::var("REELHOUSE_WEB_DIR").ok().map(PathBuf::from),
            hls_idle: Duration::from_secs(60),
            dvr_tick: Some(Duration::from_secs(10)),
            job_backoff: Duration::from_secs(30),
            source_refresh: Some(Duration::from_secs(12 * 3600)),
        })
    }

    pub fn for_tests(data_dir: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir,
            tmdb_api_key: None,
            tmdb_base: None,
            tmdb_image_base: None,
            web_dir: None,
            hls_idle: Duration::from_secs(60),
            dvr_tick: None,
            job_backoff: Duration::from_millis(10),
            source_refresh: None,
        }
    }
}
