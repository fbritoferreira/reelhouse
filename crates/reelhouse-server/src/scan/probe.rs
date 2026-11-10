//! `ffprobe` wrapper.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub duration_s: Option<f64>,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub height: Option<i64>,
}

#[derive(Deserialize)]
struct Out {
    format: Option<Format>,
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Deserialize)]
struct Format {
    format_name: Option<String>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    height: Option<i64>,
}

pub async fn probe(path: &Path) -> Result<MediaInfo> {
    let out = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .await
        .context("running ffprobe")?;
    anyhow::ensure!(
        out.status.success(),
        "ffprobe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Out = serde_json::from_slice(&out.stdout)?;
    let stream = |kind: &str| {
        parsed
            .streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some(kind))
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    Ok(MediaInfo {
        duration_s: parsed
            .format
            .as_ref()
            .and_then(|f| f.duration.as_deref()?.parse().ok()),
        // ffprobe reports "matroska,webm" or "mov,mp4,m4a,..."; the extension is what clients match on.
        container: ext.or_else(|| parsed.format.and_then(|f| f.format_name)),
        video_codec: stream("video").and_then(|s| s.codec_name.clone()),
        audio_codec: stream("audio").and_then(|s| s.codec_name.clone()),
        height: stream("video").and_then(|s| s.height),
    })
}

/// The language of each soundtrack in `input` (a path or URL), in stream order; `None` where a
/// track is untagged.
pub async fn audio_languages(input: &str) -> Result<Vec<Option<String>>> {
    let out = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream_tags=language",
            "-of",
            "csv=p=0",
        ])
        .arg(input)
        .output()
        .await
        .context("running ffprobe")?;
    anyhow::ensure!(
        out.status.success(),
        "ffprobe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| Some(l.trim().to_string()).filter(|l| !l.is_empty()))
        .collect())
}
