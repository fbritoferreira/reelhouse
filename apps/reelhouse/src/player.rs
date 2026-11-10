//! Video playback. Web drives a `<video>` element (hls.js for HLS); native backends arrive with the TV app.

use dioxus::prelude::*;
use reelhouse_api::Capabilities;

/// An audio or subtitle track the player can switch to.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct Track {
    /// "audio" or "text".
    pub kind: String,
    pub group: i32,
    pub index: i32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub channels: i32,
    #[serde(default)]
    pub selected: bool,
}

impl Track {
    /// "English 5.1", "Português", "Commentary".
    pub fn name(&self) -> String {
        let lang = language_name(&self.language);
        let mut name = match (self.label.is_empty(), lang) {
            (false, _) => self.label.clone(),
            (true, Some(l)) => l.to_string(),
            (true, None) if !self.language.is_empty() => self.language.to_uppercase(),
            _ => format!("Track {}", self.index + 1),
        };
        if self.kind == "audio" {
            match self.channels {
                6 => name.push_str(" 5.1"),
                8 => name.push_str(" 7.1"),
                _ => {}
            }
        }
        name
    }
}

pub(crate) fn language_name(code: &str) -> Option<&'static str> {
    Some(
        match code.split(['-', '_']).next()?.to_ascii_lowercase().as_str() {
            "en" | "eng" => "English",
            "pt" | "por" => "Português",
            "es" | "spa" => "Español",
            "fr" | "fre" | "fra" => "Français",
            "de" | "ger" | "deu" => "Deutsch",
            "it" | "ita" => "Italiano",
            "nl" | "dut" | "nld" => "Nederlands",
            "ar" | "ara" => "العربية",
            "ru" | "rus" => "Русский",
            "pl" | "pol" => "Polski",
            "tr" | "tur" => "Türkçe",
            "ja" | "jpn" => "日本語",
            "und" | "" => return None,
            _ => return None,
        },
    )
}

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
                 if (url.startsWith('opfs:')) {{ navigator.storage.getDirectory() \
                   .then(r => r.getFileHandle('dl-' + url.slice(5))).then(h => h.getFile()) \
                   .then(f => {{ if (v.__rhBlob) URL.revokeObjectURL(v.__rhBlob); \
                     v.__rhBlob = URL.createObjectURL(f); v.src = v.__rhBlob; v.play(); }}); return; }} \
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

        /// The stream's soundtracks (from hls.js) and the subtitle tracks attached to the video.
        pub fn tracks(&self) -> Vec<super::Track> {
            let js = format!(
                "JSON.stringify([].concat( \
                (window.__hls && window.__hls.audioTracks || []).map((t, i) => ({{ \
                  kind: 'audio', group: 0, index: i, label: '', language: t.lang || '', \
                  channels: 0, selected: i === window.__hls.audioTrack }})), \
                Array.from((document.getElementById({VIDEO_ID:?}) || {{ textTracks: [] }}).textTracks) \
                  .map((t, i) => ({{ kind: 'text', group: 0, index: i, label: t.label || '', \
                    language: t.language || '', channels: 0, selected: t.mode === 'showing' }}))))"
            );
            let js = js.as_str();
            js_sys::eval(js)
                .ok()
                .and_then(|v| v.as_string())
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        }

        pub fn select_track(&self, t: &super::Track) {
            if t.kind == "audio" {
                let _ = js_sys::eval(&format!(
                    "window.__hls && (window.__hls.audioTrack = {})",
                    t.index
                ));
            } else {
                let _ = js_sys::eval(&format!(
                    "Array.from(document.getElementById({VIDEO_ID:?}).textTracks) \
                     .forEach((t, i) => t.mode = i === {} ? 'showing' : 'disabled')",
                    t.index
                ));
            }
        }

        pub fn subtitles_off(&self) {
            let _ = js_sys::eval(&format!(
                "Array.from(document.getElementById({VIDEO_ID:?}).textTracks) \
                 .forEach(t => t.mode = 'disabled')"
            ));
        }

        /// Replaces the video's subtitle tracks with `(label, language, url)`; none shows until
        /// chosen, except the one already showing under the same label.
        pub fn set_subtitles(&self, tracks: &[(String, String, String)]) {
            let list = serde_json::to_string(tracks).unwrap_or_else(|_| "[]".into());
            let _ = js_sys::eval(&format!(
                "(() => {{ const v = document.getElementById({VIDEO_ID:?}); if (!v) return; \
                 const showing = Array.from(v.textTracks).find(t => t.mode === 'showing'); \
                 const keep = showing && showing.label; \
                 v.querySelectorAll('track').forEach(t => t.remove()); \
                 for (const [label, lang, src] of {list}) {{ \
                   const t = document.createElement('track'); t.kind = 'subtitles'; t.label = label; \
                   t.srclang = lang; t.src = src; v.appendChild(t); \
                   if (label === keep) t.track.mode = 'showing'; }} }})()"
            ));
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
        pub fn tracks(&self) -> Vec<super::Track> {
            Vec::new()
        }
        pub fn select_track(&self, _t: &super::Track) {}
        pub fn subtitles_off(&self) {}
        pub fn set_subtitles(&self, _tracks: &[(String, String, String)]) {}
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

/// [`capabilities`] limited to audio every output takes: when surround audio will not open (a
/// Bluetooth speaker, an old receiver), the server keeps the video and converts the sound.
pub fn stereo_capabilities() -> Capabilities {
    Capabilities {
        audio_codecs: list(&["aac", "mp3"]),
        ..capabilities()
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

#[cfg(test)]
mod tests {
    use super::Track;

    fn track(kind: &str, label: &str, language: &str, channels: i32) -> Track {
        Track {
            kind: kind.into(),
            group: 0,
            index: 0,
            label: label.into(),
            language: language.into(),
            channels,
            selected: false,
        }
    }

    #[test]
    fn tracks_read_as_language_and_layout() {
        assert_eq!(track("audio", "", "eng", 6).name(), "English 5.1");
        assert_eq!(track("audio", "Commentary", "en", 2).name(), "Commentary");
        assert_eq!(track("text", "", "pt-BR", 0).name(), "Português");
        assert_eq!(track("text", "", "xx", 0).name(), "XX");
        assert_eq!(track("text", "", "", 0).name(), "Track 1");
    }
}
