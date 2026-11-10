mod controls;
mod player;
mod timefmt;

use std::rc::Rc;

use dioxus::prelude::*;

use controls::Controls;
use player::Player;

const MEDIA_BASE: &str = match option_env!("MEDIA_BASE") {
    Some(base) => base,
    None => "http://localhost:8765",
};
const HLS_URL: &str = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";

const STYLE: &str = r#"
html, body, #main { margin: 0; height: 100%; background: transparent; color: #fff; font-family: sans-serif; }
video { position: fixed; inset: 0; width: 100%; height: 100%; background: #000; z-index: -1; }
.pip { position: fixed; top: 16px; right: 16px; }
.sources { position: fixed; top: 16px; left: 16px; display: flex; gap: 8px; }
.controls { position: fixed; left: 16px; right: 16px; bottom: 16px; padding: 12px; border-radius: 8px; background: rgba(0,0,0,0.55); }
.row { display: flex; align-items: center; gap: 12px; }
.seek { width: 100%; margin-top: 8px; }
.error { color: #ff8080; margin-bottom: 8px; }
button { font-size: 18px; padding: 8px 16px; }
:focus { outline: 3px solid #4da3ff; }
"#;

#[cfg(target_os = "ios")]
fn start_pip(p: &Player) {
    p.start_pip();
}
#[cfg(not(target_os = "ios"))]
fn start_pip(_: &Player) {}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let player = use_context_provider(|| Rc::new(Player::new()));
    // Test hook: PROTO_AUTOPLAY=<url> plays at launch (set via SIMCTL_CHILD_PROTO_AUTOPLAY on the simulator).
    let auto = player.clone();
    use_hook(move || {
        if let Ok(url) = std::env::var("PROTO_AUTOPLAY") {
            auto.load(&url);
            auto.play();
        }
    });
    let sources = [
        ("HLS", HLS_URL.to_string()),
        ("MP4 HEVC", format!("{MEDIA_BASE}/hevc10.mp4")),
        ("MKV HEVC", format!("{MEDIA_BASE}/hevc10.mkv")),
    ];

    rsx! {
        document::Style { {STYLE} }
        if cfg!(target_arch = "wasm32") {
            document::Script { src: "https://cdn.jsdelivr.net/npm/hls.js@1.5.17/dist/hls.min.js" }
            video { id: "v", playsinline: true }
        }
        div { class: "sources",
            for (label, url) in sources {
                button {
                    tabindex: 0,
                    onclick: {
                        let p = player.clone();
                        move |_| {
                            p.load(&url);
                            p.play();
                        }
                    },
                    "{label}"
                }
            }
        }
        if cfg!(target_os = "ios") {
            div { class: "pip",
                button {
                    tabindex: 0,
                    onclick: {
                        let p = player.clone();
                        move |_| start_pip(&p)
                    },
                    "PiP"
                }
            }
        }
        Controls {}
    }
}
