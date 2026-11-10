//! AVPlayer backend. Every bridge call returns `Option<String>`: manganis 0.7.10 mis-converts
//! numeric, bool and unit returns from Swift (see native/ios/Sources/PlayerBridge/PlayerBridge.swift).

#[manganis::ffi("/native/ios")]
extern "Swift" {
    pub type PlayerBridge;
    pub fn load(this: &PlayerBridge, url: &str) -> Option<String>;
    pub fn play(this: &PlayerBridge) -> Option<String>;
    pub fn pause(this: &PlayerBridge) -> Option<String>;
    pub fn seek(this: &PlayerBridge, seconds: f64) -> Option<String>;
    pub fn position(this: &PlayerBridge) -> Option<String>;
    pub fn duration(this: &PlayerBridge) -> Option<String>;
    pub fn is_playing(this: &PlayerBridge) -> Option<String>;
    pub fn error(this: &PlayerBridge) -> Option<String>;
    pub fn start_pip(this: &PlayerBridge) -> Option<String>;
}

pub struct Player {
    bridge: Result<PlayerBridge, &'static str>,
}

fn number(value: Result<Option<String>, &'static str>) -> f64 {
    value
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .filter(|v: &f64| v.is_finite())
        .unwrap_or(0.0)
}

impl Player {
    pub fn new() -> Player {
        Player {
            bridge: PlayerBridge::new(),
        }
    }

    pub fn load(&self, url: &str) {
        if let Ok(b) = &self.bridge {
            let _ = load(b, url);
        }
    }

    pub fn play(&self) {
        if let Ok(b) = &self.bridge {
            let _ = play(b);
        }
    }

    pub fn pause(&self) {
        if let Ok(b) = &self.bridge {
            let _ = pause(b);
        }
    }

    pub fn seek(&self, seconds: f64) {
        if let Ok(b) = &self.bridge {
            let _ = seek(b, seconds);
        }
    }

    pub fn position(&self) -> f64 {
        self.bridge.as_ref().map_or(0.0, |b| number(position(b)))
    }

    pub fn duration(&self) -> f64 {
        self.bridge.as_ref().map_or(0.0, |b| number(duration(b)))
    }

    pub fn is_playing(&self) -> bool {
        self.bridge
            .as_ref()
            .is_ok_and(|b| matches!(is_playing(b), Ok(Some(_))))
    }

    pub fn error(&self) -> Option<String> {
        match &self.bridge {
            Err(e) => Some((*e).to_string()),
            Ok(b) => error(b).unwrap_or_else(|e| Some(e.to_string())),
        }
    }

    pub fn start_pip(&self) {
        if let Ok(b) = &self.bridge {
            let _ = start_pip(b);
        }
    }
}
