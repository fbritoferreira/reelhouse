//! Video playback. Web drives a `<video>` element (hls.js for HLS); native backends arrive with the TV app.

use dioxus::prelude::*;
use reelhouse_api::Capabilities;

fn list(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlVideoElement;

    pub const VIDEO_ID: &str = "reelhouse-video";

    pub struct Player;

    impl Player {
        pub fn new() -> Self {
            Player
        }

        fn video(&self) -> Option<HtmlVideoElement> {
            web_sys::window()?
                .document()?
                .get_element_by_id(VIDEO_ID)?
                .dyn_into()
                .ok()
        }

        pub fn load(&self, url: &str) {
            // hls.js loads asynchronously; wait for it rather than falling back to native HLS,
            // which Chrome claims to support but fails on fMP4 streams.
            let js = format!(
                "(() => {{ const url = {url:?}; const v = document.getElementById({VIDEO_ID:?}); \
                 if (window.__hls) {{ window.__hls.destroy(); window.__hls = null; }} \
                 if (!url.includes('.m3u8')) {{ v.src = url; return; }} \
                 const go = () => {{ \
                   if (window.Hls && Hls.isSupported()) {{ window.__hls = new Hls(); window.__hls.loadSource(url); window.__hls.attachMedia(v); }} \
                   else {{ v.src = url; }} }}; \
                 if (window.Hls) {{ go(); return; }} \
                 let waited = 0; const t = setInterval(() => {{ waited += 50; \
                   if (window.Hls || waited > 5000) {{ clearInterval(t); go(); }} }}, 50); }})()"
            );
            let _ = js_sys::eval(&js);
        }

        pub fn play(&self) {
            if let Some(v) = self.video() {
                let _ = v.play();
            }
        }

        pub fn pause(&self) {
            if let Some(v) = self.video() {
                let _ = v.pause();
            }
        }

        pub fn seek(&self, seconds: f64) {
            if let Some(v) = self.video() {
                v.set_current_time(seconds);
            }
        }

        pub fn position(&self) -> f64 {
            self.video().map_or(0.0, |v| v.current_time())
        }

        pub fn duration(&self) -> f64 {
            self.video()
                .map(|v| v.duration())
                .filter(|d| d.is_finite())
                .unwrap_or(0.0)
        }

        pub fn is_playing(&self) -> bool {
            self.video().is_some_and(|v| !v.paused())
        }

        pub fn error(&self) -> Option<String> {
            let e = self.video()?.error()?;
            Some(format!(
                "the browser could not play this file (code {})",
                e.code()
            ))
        }
    }
}

#[cfg(target_os = "android")]
#[path = "player_android.rs"]
mod imp;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod imp {
    /// Desktop builds have no player yet.
    pub struct Player;

    impl Player {
        pub fn new() -> Self {
            Player
        }
        pub fn load(&self, _url: &str) {}
        pub fn play(&self) {}
        pub fn pause(&self) {}
        pub fn seek(&self, _seconds: f64) {}
        pub fn position(&self) -> f64 {
            0.0
        }
        pub fn duration(&self) -> f64 {
            0.0
        }
        pub fn is_playing(&self) -> bool {
            false
        }
        pub fn error(&self) -> Option<String> {
            Some("playback is not available on this platform yet".into())
        }
    }
}

pub use imp::Player;
#[cfg(target_os = "android")]
pub use imp::files_dir;

/// What this client plays without the server's help.
#[cfg(not(target_os = "android"))]
pub fn capabilities() -> Capabilities {
    Capabilities {
        containers: list(&["mp4", "m4v", "webm", "mov"]),
        video_codecs: list(&["h264", "vp9", "av1"]),
        audio_codecs: list(&["aac", "mp3", "opus"]),
        max_height: 2160,
    }
}

/// The element the video draws into.
pub fn surface() -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        rsx! {
            document::Script { src: "https://cdn.jsdelivr.net/npm/hls.js@1.5.17/dist/hls.min.js" }
            video { id: imp::VIDEO_ID, class: "video", playsinline: true }
        }
    }
    // Native: the video draws in a native view behind the transparent webview.
    #[cfg(not(target_arch = "wasm32"))]
    {
        rsx! { div { class: "video native" } }
    }
}

/// ExoPlayer on Google TV hardware: MKV and HEVC play directly.
#[cfg(target_os = "android")]
pub fn capabilities() -> Capabilities {
    Capabilities {
        containers: list(&["mp4", "m4v", "mkv", "webm", "mov", "ts"]),
        video_codecs: list(&["h264", "hevc", "vp9", "av1"]),
        audio_codecs: list(&["aac", "ac3", "eac3", "mp3", "opus", "flac"]),
        max_height: 2160,
    }
}
