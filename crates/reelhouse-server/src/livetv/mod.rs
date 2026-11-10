//! IPTV sources (M3U and Xtream Codes), the XMLTV guide and the DVR.

pub mod dvr;
pub mod m3u;
pub mod xmltv;
pub mod xtream;

use anyhow::Result;

/// The body of an `http(s)` URL, or the contents of a local file for anything else.
pub async fn fetch_text(url: &str) -> Result<String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(reqwest::get(url).await?.error_for_status()?.text().await?)
    } else {
        Ok(tokio::fs::read_to_string(url).await?)
    }
}

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
