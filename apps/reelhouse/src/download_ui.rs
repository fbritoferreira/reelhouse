//! Download controls. One mark everywhere: a reel ring that winds up in marquee amber as the
//! download progresses, and fills solid once the item is on this TV. Web builds render nothing.

use dioxus::prelude::*;
use reelhouse_api::Item;

/// What the ring shows.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq)]
pub enum Reel {
    Idle,
    Queued,
    /// 0.0 to 1.0.
    Winding(f64),
    Paused(f64),
    Full,
    Failed,
}

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Ring(reel: Reel) -> Element {
    let pct = match reel {
        Reel::Winding(f) | Reel::Paused(f) => (f * 100.0).clamp(0.0, 100.0),
        Reel::Full => 100.0,
        _ => 0.0,
    };
    let class = match reel {
        Reel::Idle => "reel",
        Reel::Queued => "reel queued",
        Reel::Winding(_) => "reel winding",
        Reel::Paused(_) => "reel paused",
        Reel::Full => "reel full",
        Reel::Failed => "reel failed",
    };
    rsx! {
        svg { class, view_box: "0 0 36 36", "aria-hidden": "true",
            circle { class: "reel-track", cx: "18", cy: "18", r: "15.9155" }
            circle {
                class: "reel-wind",
                cx: "18",
                cy: "18",
                r: "15.9155",
                stroke_dasharray: "{pct} 100",
                transform: "rotate(-90 18 18)",
            }
            match reel {
                Reel::Full => rsx! { path { class: "reel-glyph", d: "M11.5 18.5l4.2 4.2 8.8-9.4" } },
                Reel::Winding(_) => rsx! {
                    text { class: "reel-pct", x: "18", y: "18", dy: "0.35em", text_anchor: "middle", "{pct.round()}" }
                },
                Reel::Failed => rsx! { path { class: "reel-glyph", d: "M18 10.5v9.5M18 24.5v1" } },
                _ => rsx! { path { class: "reel-glyph", d: "M18 10.5v13M12.5 18.5l5.5 5.5 5.5-5.5" } },
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::time::Duration;

    use super::*;
    use crate::downloads::{self, State};
    use crate::session::client;

    pub fn reel_of(state: &State) -> Reel {
        match state {
            State::NotDownloaded => Reel::Idle,
            State::Queued => Reel::Queued,
            State::Running { fraction } => Reel::Winding(*fraction),
            State::Paused { fraction } => Reel::Paused(*fraction),
            State::Done => Reel::Full,
            State::Failed(_) => Reel::Failed,
        }
    }

    /// Re-reads the download state every second.
    fn use_state_of(id: i64) -> Signal<State> {
        let mut state = use_signal(move || downloads::state(id));
        use_future(move || async move {
            loop {
                futures_timer::Delay::new(Duration::from_secs(1)).await;
                let now = downloads::state(id);
                if *state.peek() != now {
                    state.set(now);
                }
            }
        });
        state
    }

    /// The hero's download action for a film or an episode.
    #[component]
    pub fn DownloadButton(item: Item) -> Element {
        let id = item.id;
        let mut state = use_state_of(id);
        let s = state();
        let label = match &s {
            State::NotDownloaded => "Download".to_string(),
            State::Queued => "Waiting to download".to_string(),
            State::Running { fraction } => format!("Downloading {}%", (fraction * 100.0).round()),
            State::Paused { fraction } => {
                format!("Paused at {}%, resume", (fraction * 100.0).round())
            }
            State::Done => "On this TV".to_string(),
            State::Failed(_) => "Download stopped, try again".to_string(),
        };
        let item_for_start = item.clone();
        rsx! {
            div { class: "download-action",
                button {
                    class: if s == State::Done { "reel-button done" } else { "reel-button" },
                    onclick: move |_| {
                        if *state.peek() == State::Done {
                            downloads::remove(id);
                        } else {
                            downloads::start(client(), item_for_start.clone());
                        }
                        state.set(downloads::state(id));
                    },
                    Ring { reel: reel_of(&s) }
                    span { "{label}" }
                }
                if s == State::Done {
                    span { class: "quiet download-hint", "Select again to remove it" }
                }
                if let State::Failed(e) = &s {
                    span { class: "notice download-hint", "{e}" }
                }
            }
        }
    }

    /// The compact ring at the end of an episode row: its own focus stop.
    #[component]
    pub fn EpisodeDownload(ep: Item) -> Element {
        let id = ep.id;
        let mut state = use_state_of(id);
        let s = state();
        let name = crate::widgets::episode_name(&ep);
        let label = match &s {
            State::Done => format!("Remove {name} from this TV"),
            State::Running { .. } | State::Queued => format!("{name} is downloading"),
            _ => format!("Download {name}"),
        };
        rsx! {
            button {
                class: "reel-icon",
                "aria-label": "{label}",
                title: "{label}",
                onclick: move |_| {
                    match *state.peek() {
                        State::Done => downloads::remove(id),
                        State::Running { .. } | State::Queued => {}
                        _ => downloads::start(client(), ep.clone()),
                    }
                    state.set(downloads::state(id));
                },
                Ring { reel: reel_of(&s) }
            }
        }
    }

    /// Downloads every episode of a season (`scope` = the season) or a whole show.
    #[component]
    pub fn CollectionDownload(scope: Item) -> Element {
        let scope_id = scope.id;
        let is_show = scope.kind == reelhouse_api::ItemKind::Show;
        let episodes = use_resource(use_reactive!(|scope_id, is_show| async move {
            let c = client();
            let load = async {
                if !is_show {
                    return Ok((c.children(scope_id).await?, 1));
                }
                let seasons = c.children(scope_id).await?;
                let mut eps = Vec::new();
                for s in &seasons {
                    eps.extend(c.children(s.id).await?);
                }
                Ok::<_, reelhouse_core::ClientError>((eps, seasons.len()))
            };
            load.await.map_err(|e| e.to_string())
        }));
        let mut tick = use_signal(|| 0u32);
        use_future(move || async move {
            loop {
                futures_timer::Delay::new(Duration::from_secs(1)).await;
                tick += 1;
            }
        });
        let _ = tick();
        let Some(Ok((eps, season_count))) = episodes.read().clone() else {
            return rsx! {};
        };
        if eps.is_empty() {
            return rsx! {};
        }
        let ids: Vec<i64> = eps.iter().map(|e| e.id).collect();
        let (done, busy) = downloads::summary(&ids);
        let total = eps.len();
        let what = if is_show {
            if season_count == 1 {
                "the whole show".to_string()
            } else {
                format!("all {season_count} seasons")
            }
        } else {
            scope.title.to_lowercase()
        };
        let count = if total == 1 {
            "1 episode".to_string()
        } else {
            format!("{total} episodes")
        };
        let (reel, label) = if done == total {
            (Reel::Full, format!("{} on this TV", capitalise(&what)))
        } else if busy {
            (
                Reel::Winding(done as f64 / total as f64),
                format!("Downloading {done} of {total}"),
            )
        } else if done > 0 {
            (
                Reel::Paused(done as f64 / total as f64),
                format!("Download the other {} of {count}", total - done),
            )
        } else {
            (Reel::Idle, format!("Download {what}, {count}"))
        };
        rsx! {
            button {
                class: if done == total { "reel-button done" } else { "reel-button" },
                disabled: busy || done == total,
                onclick: move |_| downloads::start_all(client(), eps.clone()),
                Ring { reel }
                span { "{label}" }
            }
        }
    }

    fn capitalise(s: &str) -> String {
        let mut c = s.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{CollectionDownload, DownloadButton, EpisodeDownload, reel_of};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;

    #[component]
    pub fn DownloadButton(item: Item) -> Element {
        let _ = item;
        rsx! {}
    }

    #[component]
    pub fn EpisodeDownload(ep: Item) -> Element {
        let _ = ep;
        rsx! {}
    }

    #[component]
    pub fn CollectionDownload(scope: Item) -> Element {
        let _ = scope;
        rsx! {}
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::{CollectionDownload, DownloadButton, EpisodeDownload};
