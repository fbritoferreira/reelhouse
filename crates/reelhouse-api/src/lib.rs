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
    /// For seasons and episodes, the show they belong to.
    #[serde(default)]
    pub show_id: Option<i64>,
    #[serde(default)]
    pub show_title: Option<String>,
    /// The picture's height in pixels, for films and episodes (1080, 2160).
    #[serde(default)]
    pub height: Option<i64>,
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
    /// Where in the title the stream begins: an HLS conversion can start part-way through, and
    /// its own clock then counts from here. Direct play always starts at 0 and seeks itself.
    #[serde(default)]
    pub start_s: f64,
    /// The whole title's length when the server knows it (a live conversion only reports what
    /// it has converted so far).
    #[serde(default)]
    pub duration_s: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TvSourceKind {
    M3u,
    Xtream,
}

/// A text subtitle track in a title, served as WebVTT. `index` counts the file's subtitle streams.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SubtitleTrack {
    pub index: i64,
    pub language: Option<String>,
    pub title: Option<String>,
}

/// Where to fetch a copy to keep offline: the original file when the client plays it as is
/// (`resumable`, ranges allowed), else an MP4 remuxed as it streams (restart if interrupted).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct OfflineSource {
    pub url: String,
    pub resumable: bool,
}

/// An IPTV source. The password is write-only.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TvSource {
    pub id: i64,
    pub name: String,
    pub kind: TvSourceKind,
    pub url: String,
    pub username: Option<String>,
    pub epg_url: Option<String>,
    /// When its last refresh finished, Unix seconds; `None` if it has not been refreshed yet.
    #[serde(default)]
    pub refreshed_at: Option<i64>,
}

/// For `m3u`, `url` is the playlist. For `xtream`, it is the panel base URL and
/// `username`/`password` are required. `epg_url` overrides the panel's own guide.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NewTvSource {
    pub name: String,
    pub kind: TvSourceKind,
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub epg_url: Option<String>,
}

/// Runs a job gets before it is marked failed: the first and three retries.
pub const JOB_MAX_ATTEMPTS: i64 = 4;

/// Background work the server queues: scanning a library, refreshing a Live TV source.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    ScanLibrary,
    RefreshSource,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    /// Gave up after its retries.
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Job {
    pub id: i64,
    pub kind: JobKind,
    /// The library or source the job works on.
    pub target_id: i64,
    pub status: JobStatus,
    /// Runs so far, including the one under way.
    pub attempts: i64,
    pub last_error: Option<String>,
    /// The `ScanReport` or `RefreshReport` once done.
    pub result: Option<serde_json::Value>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RefreshReport {
    pub channels: u64,
    pub programmes: u64,
    /// Films and series episodes the source offers on demand.
    #[serde(default)]
    pub on_demand: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Channel {
    pub id: i64,
    pub source_id: i64,
    pub number: Option<i64>,
    pub name: String,
    pub logo_url: Option<String>,
    pub group_title: Option<String>,
    /// Why the provider refused this channel the last time it was played; `None` once it plays.
    #[serde(default)]
    pub refused: Option<String>,
}

/// Times are Unix seconds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Programme {
    pub id: i64,
    pub channel_id: i64,
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
}

/// A playlist's `group-title` and how many channels it holds; `None` collects the ungrouped.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChannelGroup {
    pub name: Option<String>,
    pub channels: i64,
}

/// A group of on-demand titles, with how many films or shows it holds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VodGroup {
    pub name: Option<String>,
    pub titles: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VodMovie {
    pub id: i64,
    pub title: String,
    pub year: Option<i64>,
    pub poster_url: Option<String>,
    /// Known once the title has been played.
    #[serde(default)]
    pub duration_s: Option<f64>,
    #[serde(default)]
    pub progress: Option<Progress>,
}

/// On-demand titles matching a search.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct VodSearch {
    pub films: Vec<VodMovie>,
    pub shows: Vec<VodShow>,
}

/// One on-demand title with where you are in it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VodTitle {
    pub id: i64,
    pub title: String,
    pub show_title: Option<String>,
    pub season_no: Option<i64>,
    pub episode_no: Option<i64>,
    pub poster_url: Option<String>,
    pub duration_s: Option<f64>,
    pub progress: Option<Progress>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VodShow {
    pub title: String,
    pub poster_url: Option<String>,
    pub seasons: i64,
    pub episodes: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VodEpisode {
    pub id: i64,
    pub title: String,
    pub season_no: Option<i64>,
    pub episode_no: Option<i64>,
    pub poster_url: Option<String>,
    #[serde(default)]
    pub duration_s: Option<f64>,
    #[serde(default)]
    pub progress: Option<Progress>,
}

/// One of a source's groups, and whether Live TV shows it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SourceGroup {
    pub name: Option<String>,
    pub channels: i64,
    pub shown: bool,
}

/// The groups of a source to hide; `None` is the ungrouped.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct HiddenGroups {
    pub hidden: Vec<Option<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GuideChannel {
    pub channel: Channel,
    pub programmes: Vec<Programme>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    Once,
    Series,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RecordingRule {
    pub id: i64,
    pub kind: RuleKind,
    pub channel_id: i64,
    pub programme_id: Option<i64>,
    pub series_title: Option<String>,
    pub padding_before_s: i64,
    pub padding_after_s: i64,
    pub keep_last: Option<i64>,
}

/// `once` needs `programme_id`. `series` needs `programme_id` (its channel and series are
/// used) or `channel_id` plus `series_title`. `keep_last` deletes older recordings of a series.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NewRecordingRule {
    pub kind: RuleKind,
    pub programme_id: Option<i64>,
    pub channel_id: Option<i64>,
    pub series_title: Option<String>,
    #[serde(default)]
    pub padding_before_s: i64,
    #[serde(default)]
    pub padding_after_s: i64,
    pub keep_last: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStatus {
    Scheduled,
    Recording,
    Done,
    Failed,
}

/// `start` and `stop` include the rule's padding. `item_id` is set once done.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Recording {
    pub id: i64,
    pub rule_id: i64,
    pub channel_id: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub start: i64,
    pub stop: i64,
    pub status: RecordingStatus,
    pub item_id: Option<i64>,
    /// Why it failed, in plain words; `None` unless it failed.
    #[serde(default)]
    pub failure: Option<String>,
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

/// A sign-in code for a device without a keyboard. `code` is shown to the user;
/// `poll_token` stays on the device and is used to collect the session once approved.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceCode {
    pub code: String,
    pub poll_token: String,
    pub expires_in: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DevicePoll {
    pub poll_token: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceApprove {
    pub code: String,
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
