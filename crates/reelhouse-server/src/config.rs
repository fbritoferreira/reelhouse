use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub tmdb_api_key: Option<String>,
    /// HLS sessions with no request for this long are stopped.
    pub hls_idle: Duration,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            bind: std::env::var("REELHOUSE_BIND")
                .unwrap_or_else(|_| "0.0.0.0:8420".into())
                .parse()?,
            data_dir: std::env::var("REELHOUSE_DATA_DIR")
                .unwrap_or_else(|_| "./data".into())
                .into(),
            tmdb_api_key: std::env::var("TMDB_API_KEY").ok().filter(|k| !k.is_empty()),
            hls_idle: Duration::from_secs(60),
        })
    }

    pub fn for_tests(data_dir: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir,
            tmdb_api_key: None,
            hls_idle: Duration::from_secs(60),
        }
    }
}
