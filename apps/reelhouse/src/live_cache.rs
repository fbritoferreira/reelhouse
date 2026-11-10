//! Live TV's memory: the groups, each group's guide, and where you were in them. It lives above
//! the pages, so leaving a channel lands you back on the same row, and it is mirrored to device
//! storage, so the next launch opens on cached channels while fresh ones load.

use std::collections::HashMap;

use dioxus::prelude::*;
use reelhouse_api::{ChannelGroup, GuideChannel};
use serde::{Deserialize, Serialize};

/// Cached lists older than this are refetched in the background (still shown meanwhile).
pub const FRESH_FOR_S: i64 = 300;
/// Guides kept on the device; the most recently loaded win.
const GUIDES_KEPT: usize = 6;
const DISK_KEY: &str = "reelhouse.live.cache";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Cached<T> {
    pub at: i64,
    pub value: T,
}

impl<T> Cached<T> {
    pub fn fresh(&self) -> bool {
        now_s() - self.at < FRESH_FOR_S
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveMemory {
    pub groups: Option<Cached<Vec<ChannelGroup>>>,
    pub guides: HashMap<String, Cached<Vec<GuideChannel>>>,
    /// Where you were; not written to disk.
    #[serde(skip)]
    pub place: Place,
}

#[derive(Clone, Default, PartialEq)]
pub struct Place {
    pub group: Option<String>,
    /// The focused channel's index in its group.
    pub spotlight: usize,
    /// Rows drawn.
    pub shown: usize,
    /// Set when a channel opens, so coming back puts focus on it again.
    pub returning: bool,
}

pub static LIVE: GlobalSignal<LiveMemory> = Signal::global(|| {
    crate::session::load(DISK_KEY)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
});

pub fn now_s() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

pub fn store_groups(groups: Vec<ChannelGroup>) {
    LIVE.write().groups = Some(Cached {
        at: now_s(),
        value: groups,
    });
    persist();
}

pub fn store_guide(group: &str, guide: Vec<GuideChannel>) {
    LIVE.write().guides.insert(
        group.to_string(),
        Cached {
            at: now_s(),
            value: guide,
        },
    );
    persist();
}

/// Writes the cache, keeping only the newest guides.
fn persist() {
    let mut copy = LIVE.peek().clone();
    let mut by_age: Vec<(String, i64)> =
        copy.guides.iter().map(|(k, v)| (k.clone(), v.at)).collect();
    by_age.sort_by_key(|(_, at)| -at);
    for (k, _) in by_age.into_iter().skip(GUIDES_KEPT) {
        copy.guides.remove(&k);
    }
    if let Ok(json) = serde_json::to_string(&copy) {
        crate::session::save(DISK_KEY, Some(&json));
    }
}

/// Forgets everything, e.g. when another user signs in.
pub fn clear() {
    *LIVE.write() = LiveMemory::default();
    crate::session::save(DISK_KEY, None);
}
