//! One ffmpeg process per HLS session, writing fMP4 segments to a temp directory.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use reelhouse_api::PlaybackMethod;
use tokio::process::{Child, Command};

struct Session {
    dir: tempfile::TempDir,
    child: Child,
    last_access: Instant,
}

#[derive(Default)]
pub struct Sessions {
    inner: Mutex<HashMap<String, Session>>,
}

impl Sessions {
    /// Starts ffmpeg for `input` (a file path or a stream URL) and returns the session id.
    /// `start_s` > 0 begins the conversion that far in.
    pub fn start(
        &self,
        input: &str,
        method: PlaybackMethod,
        max_height: u32,
        start_s: f64,
    ) -> Result<String> {
        let dir = tempfile::Builder::new()
            .prefix("reelhouse-hls-")
            .tempdir()?;
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if start_s > 0.0 {
            // Before -i: a fast keyframe seek on the input.
            cmd.args(["-ss", &format!("{start_s:.3}")]);
        }
        cmd.args(["-i", input])
            .args(["-map", "0:v:0", "-map", "0:a:0?"]);
        match method {
            PlaybackMethod::Transcode => {
                let scale = format!("scale=-2:'min(ih,{max_height})'");
                // 8-bit 4:2:0: browsers and most TV decoders cannot play 10-bit H.264.
                cmd.args([
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "21", "-pix_fmt", "yuv420p",
                    "-vf", &scale,
                ]);
            }
            _ => {
                cmd.args(["-c:v", "copy"]);
            }
        }
        cmd.args([
            "-c:a",
            "aac",
            "-ac",
            "2",
            "-f",
            "hls",
            "-hls_time",
            "6",
            "-hls_playlist_type",
            "event",
        ])
        .args(["-hls_segment_type", "fmp4"])
        .arg(dir.path().join("index.m3u8"))
        // Kept beside the segments so a failed start can say why.
        .stderr(std::fs::File::create(dir.path().join("ffmpeg.log"))?)
        .kill_on_drop(true);
        let child = cmd.spawn()?;
        let id = crate::auth::new_token();
        self.inner.lock().unwrap().insert(
            id.clone(),
            Session {
                dir,
                child,
                last_access: Instant::now(),
            },
        );
        Ok(id)
    }

    /// Waits until the session's playlist exists. If ffmpeg gives up first (a dead channel, a
    /// provider refusing the connection) the session is dropped and its last error returned.
    pub async fn ready(&self, id: &str, timeout: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let failed = {
                let mut map = self.inner.lock().unwrap();
                let Some(s) = map.get_mut(id) else {
                    return Err("the stream stopped".into());
                };
                if s.dir.path().join("index.m3u8").exists() {
                    return Ok(());
                }
                match s.child.try_wait() {
                    Ok(Some(_)) => {
                        let log = std::fs::read_to_string(s.dir.path().join("ffmpeg.log"))
                            .unwrap_or_default();
                        map.remove(id);
                        Some(last_error(&log))
                    }
                    _ => None,
                }
            };
            if let Some(reason) = failed {
                return Err(reason);
            }
            if started.elapsed() > timeout {
                self.inner.lock().unwrap().remove(id);
                return Err(format!("nothing arrived in {} seconds", timeout.as_secs()));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Path of `name` inside the session directory, if the session exists.
    /// The session id is the credential: 256 random bits, issued only to a signed-in user, and
    /// players fetch segments by relative URL, which drops any `?token=` query.
    pub fn file(&self, id: &str, name: &str) -> Option<PathBuf> {
        if name.contains('/') || name.contains("..") {
            return None;
        }
        let mut map = self.inner.lock().unwrap();
        let s = map.get_mut(id)?;
        s.last_access = Instant::now();
        Some(s.dir.path().join(name))
    }

    /// Drops sessions idle longer than `idle`; dropping kills ffmpeg and removes the directory.
    pub fn reap(&self, idle: Duration) {
        self.inner
            .lock()
            .unwrap()
            .retain(|_, s| s.last_access.elapsed() < idle);
    }

    pub fn spawn_reaper(self: &Arc<Self>, idle: Duration) {
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval((idle / 4).max(Duration::from_millis(250)));
            loop {
                tick.tick().await;
                let Some(s) = me.upgrade() else { return };
                s.reap(idle);
            }
        });
    }

    #[doc(hidden)]
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The last meaningful line of ffmpeg's log, without the `[https @ 0x…]` prefix.
fn last_error(log: &str) -> String {
    log.lines()
        .rev()
        .map(|l| match l.find("] ") {
            Some(i) if l.starts_with('[') => &l[i + 2..],
            _ => l,
        })
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("Error opening input file"))
        .unwrap_or("the stream would not open")
        .to_string()
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::last_error;

    #[test]
    fn last_error_skips_noise() {
        let log = "[https @ 0x5d3a] HTTP error 407 Proxy Authentication Required\n\
                   Error opening input file https://x/y.\n";
        assert_eq!(
            last_error(log),
            "HTTP error 407 Proxy Authentication Required"
        );
        assert_eq!(last_error(""), "the stream would not open");
    }
}
