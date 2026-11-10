/// Host build with no platform feature (used by `cargo test`): plays nothing.
pub struct Player;

impl Player {
    pub fn new() -> Player {
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
        Some("no player backend in this build".into())
    }
}
