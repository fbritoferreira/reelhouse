//! The Reelhouse media server.

use anyhow::Result;
use reelhouse_server::app::{AppState, router};
use reelhouse_server::config::Config;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let config = Config::from_env()?;
    let db = reelhouse_server::db::open(&config.data_dir).await?;
    let bind = config.bind;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("reelhouse-server {} listening on http://{bind}", env!("CARGO_PKG_VERSION"));
    axum::serve(listener, router(AppState::new(db, config)))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
