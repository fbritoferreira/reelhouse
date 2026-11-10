//! IPTV sources (M3U and Xtream Codes), the XMLTV guide and the DVR.

pub mod m3u;

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
