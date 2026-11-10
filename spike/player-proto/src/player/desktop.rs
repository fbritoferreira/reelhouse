//! macOS libmpv backend: mpv renders into an NSView inserted below the transparent WKWebView.

use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
use libmpv2::Mpv;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSRect;

const NS_VIEW_WIDTH_SIZABLE: usize = 2;
const NS_VIEW_HEIGHT_SIZABLE: usize = 16;
const NS_WINDOW_BELOW: isize = -1;

pub struct Player {
    mpv: Result<Mpv, String>,
}

/// Adds an autoresizing NSView at the back of the window's content view and returns it.
fn video_view() -> *mut AnyObject {
    let window = dioxus::desktop::window();
    let content = window.window.ns_view() as *mut AnyObject;
    unsafe {
        let bounds: NSRect = msg_send![content, bounds];
        let view: *mut AnyObject = msg_send![class!(NSView), alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: bounds];
        let _: () =
            msg_send![view, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
        let _: () = msg_send![content, addSubview: view, positioned: NS_WINDOW_BELOW, relativeTo: std::ptr::null_mut::<AnyObject>()];
        view
    }
}

impl Player {
    pub fn new() -> Player {
        let view = video_view();
        let mpv = Mpv::with_initializer(|init| {
            init.set_property("wid", view as i64)?;
            init.set_property("keep-open", "yes")?;
            if std::env::var("PROTO_MPV_LOG").is_ok() {
                init.set_property("terminal", "yes")?;
                init.set_property("msg-level", "all=v")?;
            }
            Ok(())
        })
        .map_err(|e| format!("mpv init failed: {e}"));
        Player { mpv }
    }

    pub fn load(&self, url: &str) {
        if let Ok(mpv) = &self.mpv {
            let _ = mpv.command("loadfile", &[url]);
        }
    }

    pub fn play(&self) {
        if let Ok(mpv) = &self.mpv {
            let _ = mpv.set_property("pause", false);
        }
    }

    pub fn pause(&self) {
        if let Ok(mpv) = &self.mpv {
            let _ = mpv.set_property("pause", true);
        }
    }

    pub fn seek(&self, seconds: f64) {
        if let Ok(mpv) = &self.mpv {
            let _ = mpv.command("seek", &[&seconds.to_string(), "absolute"]);
        }
    }

    pub fn position(&self) -> f64 {
        self.mpv
            .as_ref()
            .ok()
            .and_then(|m| m.get_property::<f64>("time-pos").ok())
            .unwrap_or(0.0)
    }

    pub fn duration(&self) -> f64 {
        self.mpv
            .as_ref()
            .ok()
            .and_then(|m| m.get_property::<f64>("duration").ok())
            .unwrap_or(0.0)
    }

    pub fn is_playing(&self) -> bool {
        self.mpv
            .as_ref()
            .ok()
            .and_then(|m| m.get_property::<bool>("pause").ok())
            .is_some_and(|paused| !paused && self.duration() > 0.0)
    }

    pub fn error(&self) -> Option<String> {
        self.mpv.as_ref().err().cloned()
    }
}
