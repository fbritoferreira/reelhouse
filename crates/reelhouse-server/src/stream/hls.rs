//! One ffmpeg process per HLS session, writing fMP4 segments to a temp directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    /// Starts ffmpeg for `file` and returns the session id.
    pub fn start(&self, file: &Path, method: PlaybackMethod, max_height: u32) -> Result<String> {
        let dir = tempfile::Builder::new()
            .prefix("reelhouse-hls-")
            .tempdir()?;
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin", "-i"])
            .arg(file)
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

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
