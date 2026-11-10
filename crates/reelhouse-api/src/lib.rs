//! HTTP API types shared by the Reelhouse server and clients.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServerInfo {
    pub version: String,
    pub features: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LoginRequest {
    pub name: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LoginResponse {
    pub token: String,
    pub user: User,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NewUser {
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Movies,
    Tv,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Library {
    pub id: i64,
    pub name: String,
    pub kind: LibraryKind,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NewLibrary {
    pub name: String,
    pub kind: LibraryKind,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Movie,
    Show,
    Season,
    Episode,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    pub id: i64,
    pub kind: ItemKind,
    pub parent_id: Option<i64>,
    pub title: String,
    pub year: Option<i64>,
    pub season_no: Option<i64>,
    pub episode_no: Option<i64>,
    pub duration_s: Option<f64>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub progress: Option<Progress>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Progress {
    pub position_s: f64,
    pub played: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ScanReport {
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
}

/// What a client can play without help. Codec names follow ffprobe (`h264`, `hevc`, `aac`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Capabilities {
    pub containers: Vec<String>,
    pub video_codecs: Vec<String>,
    pub audio_codecs: Vec<String>,
    pub max_height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMethod {
    Direct,
    Remux,
    Transcode,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PlaybackPlan {
    pub method: PlaybackMethod,
    pub url: String,
}

/// Rewrites Plex file paths before matching, for when the two servers mount media differently.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PathRewrite {
    pub from: String,
    pub to: String,
}

/// Imports the Plex token owner's watch state into `user_id`.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct PlexImportRequest {
    pub url: String,
    pub token: String,
    pub user_id: i64,
    #[serde(default)]
    pub path_rewrites: Vec<PathRewrite>,
}

/// Hand-written so the Plex token never reaches a log.
impl std::fmt::Debug for PlexImportRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlexImportRequest")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("user_id", &self.user_id)
            .field("path_rewrites", &self.path_rewrites)
            .finish()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ImportReport {
    pub matched: u64,
    pub unmatched: Vec<String>,
    pub progress_written: u64,
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plex_request_debug_hides_token() {
        let req = PlexImportRequest {
            url: "http://plex:32400".into(),
            token: "s3cret".into(),
            user_id: 1,
            path_rewrites: vec![],
        };
        assert!(!format!("{req:?}").contains("s3cret"));
    }
}
