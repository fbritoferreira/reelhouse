use wasm_bindgen::JsCast;
use web_sys::HtmlVideoElement;

pub const VIDEO_ID: &str = "v";

/// Drives the `<video id="v">` element rendered by `main.rs`.
pub struct Player;

impl Player {
    pub fn new() -> Player {
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
        let Some(video) = self.video() else { return };
        if url.ends_with(".m3u8") {
            // hls.js first: Chrome 157 reports canPlayType "maybe" for HLS but fails on fMP4 streams.
            // Native HLS only where hls.js has no MSE (iOS Safari).
            let _ = js_sys::eval(&format!(
                "if (window.__hls) {{ window.__hls.destroy(); window.__hls = null; }} \
                 const v = document.getElementById({VIDEO_ID:?}); \
                 if (window.Hls && Hls.isSupported()) {{ window.__hls = new Hls(); window.__hls.loadSource({url:?}); window.__hls.attachMedia(v); }} \
                 else {{ v.src = {url:?}; }}"
            ));
        } else {
            let _ =
                js_sys::eval("if (window.__hls) { window.__hls.destroy(); window.__hls = null; }");
            video.set_src(url);
        }
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
        self.video().map_or(0.0, |v| v.duration())
    }

    pub fn is_playing(&self) -> bool {
        self.video().is_some_and(|v| !v.paused())
    }

    pub fn error(&self) -> Option<String> {
        let e = self.video()?.error()?;
        Some(format!("media error {}: {}", e.code(), e.message()))
    }
}
