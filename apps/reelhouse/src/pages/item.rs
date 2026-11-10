use dioxus::prelude::*;
use reelhouse_api::{Item, ItemKind};

use crate::Route;
use crate::session::client;
use crate::widgets::{
    Facts, Hero, LoadError, PlayButton, art_url, describe, episode_name, is_watched, runtime,
    watched_fraction,
};

#[component]
pub fn ItemPage(id: i64) -> Element {
    let data = use_resource(move || async move { client().item(id).await });
    match &*data.read() {
        None => rsx! { div { class: "page", p { class: "quiet", "Loading" } } },
        Some(Err(e)) => rsx! { div { class: "page", LoadError { error: describe(e) } } },
        Some(Ok(item)) => match item.kind {
            ItemKind::Show => {
                rsx! { ShowView { key: "{item.id}", show: item.clone(), season: None } }
            }
            // A season opens its show with that season selected.
            ItemKind::Season => match item.show_id {
                Some(show_id) => {
                    rsx! { SeasonOfShow { key: "{item.id}", show_id, season: item.id } }
                }
                None => rsx! { ShowView { show: item.clone(), season: Some(item.id) } },
            },
            _ => rsx! { Playable { key: "{item.id}", item: item.clone() } },
        },
    }
}

/// A film or an episode.
#[component]
fn Playable(item: Item) -> Element {
    let above = match (&item.show_title, item.show_id) {
        (Some(show), Some(show_id)) => rsx! {
            Link { class: "hero-kicker", to: Route::ItemPage { id: show_id }, "{show}" }
        },
        _ => rsx! {},
    };
    let title = if item.kind == ItemKind::Episode {
        episode_name(&item)
    } else {
        item.title.clone()
    };
    rsx! {
        Hero { backdrop: item.backdrop_url.clone(), title, above,
            Facts { item: item.clone() }
            div { class: "hero-actions",
                PlayButton { item: item.clone() }
                if cfg!(not(target_arch = "wasm32")) {
                    DownloadButton { item: item.clone() }
                }
            }
            if let Some(o) = item.overview.clone() {
                p { class: "overview", "{o}" }
            }
        }
    }
}

#[component]
fn SeasonOfShow(show_id: i64, season: i64) -> Element {
    let show = use_resource(move || async move { client().item(show_id).await });
    match &*show.read() {
        Some(Ok(show)) => rsx! { ShowView { show: show.clone(), season: Some(season) } },
        Some(Err(e)) => rsx! { div { class: "page", LoadError { error: describe(e) } } },
        None => rsx! { div { class: "page", p { class: "quiet", "Loading" } } },
    }
}

/// A show: hero, a season switcher and the selected season's episodes.
#[component]
fn ShowView(show: Item, season: Option<i64>) -> Element {
    let show_id = show.id;
    let seasons = use_resource(move || async move { client().children(show_id).await });
    let mut chosen = use_signal(|| season);
    let list = seasons.read();
    let seasons_list: Vec<Item> = match &*list {
        Some(Ok(s)) => s.clone(),
        _ => Vec::new(),
    };
    let selected = chosen().or_else(|| seasons_list.first().map(|s| s.id));
    rsx! {
        Hero { backdrop: show.backdrop_url.clone(), title: show.title.clone(), above: rsx! {},
            Facts { item: show.clone() }
            if let Some(o) = show.overview.clone() {
                p { class: "overview", "{o}" }
            }
        }
        div { class: "page",
            if seasons_list.len() > 1 {
                div { class: "seasons", role: "tablist",
                    for s in seasons_list.clone() {
                        button {
                            key: "{s.id}",
                            role: "tab",
                            class: if Some(s.id) == selected { "season active" } else { "season" },
                            "aria-selected": Some(s.id) == selected,
                            onclick: move |_| chosen.set(Some(s.id)),
                            "{s.title}"
                        }
                    }
                }
            }
            if let Some(season_id) = selected {
                Episodes { key: "{season_id}", season: season_id, fallback_art: show.backdrop_url.clone() }
            }
        }
    }
}

#[component]
fn Episodes(season: i64, fallback_art: Option<String>) -> Element {
    let eps = use_resource(move || async move { client().children(season).await });
    match &*eps.read() {
        None => rsx! { p { class: "quiet", "Loading episodes" } },
        Some(Err(e)) => rsx! { LoadError { error: describe(e) } },
        Some(Ok(eps)) => rsx! {
            ol { class: "episodes",
                for ep in eps.clone() {
                    EpisodeRow { key: "{ep.id}", ep: ep.clone(), fallback_art: fallback_art.clone() }
                }
            }
        },
    }
}

#[component]
fn EpisodeRow(ep: Item, fallback_art: Option<String>) -> Element {
    let still = art_url(ep.backdrop_url.clone().or(fallback_art));
    let number = ep.episode_no.map(|n| n.to_string()).unwrap_or_default();
    let length = ep
        .duration_s
        .filter(|d| *d > 0.0)
        .map(runtime)
        .unwrap_or_default();
    rsx! {
        li {
            Link { class: "episode", to: Route::Play { id: ep.id },
                div { class: "episode-still",
                    if let Some(url) = still {
                        img { src: url, alt: "", loading: "lazy" }
                    }
                    if let Some(f) = watched_fraction(&ep) {
                        span { class: "progress-track",
                            span { class: "progress-fill", style: "width: {f * 100.0}%" }
                        }
                    }
                }
                div { class: "episode-text",
                    p { class: "episode-head",
                        span { class: "episode-number", "{number}" }
                        span { class: "episode-name", "{episode_name(&ep)}" }
                        if is_watched(&ep) {
                            span { class: "watched-inline", "aria-label": "Watched", "✓" }
                        }
                    }
                    if !length.is_empty() {
                        p { class: "quiet episode-length", "{length}" }
                    }
                    if let Some(o) = ep.overview.clone() {
                        p { class: "episode-overview", "{o}" }
                    }
                }
            }
        }
    }
}

/// Download, its progress, or Remove once it is on the device.
#[cfg(not(target_arch = "wasm32"))]
#[component]
fn DownloadButton(item: Item) -> Element {
    {
        use crate::downloads::{self, State};
        let id = item.id;
        let mut state = use_signal(move || downloads::state(id));
        use_future(move || async move {
            loop {
                futures_timer::Delay::new(std::time::Duration::from_secs(1)).await;
                let now = downloads::state(id);
                if *state.peek() != now {
                    state.set(now);
                }
            }
        });
        let start = {
            let item = item.clone();
            move |_| {
                downloads::start(client(), item.clone());
                state.set(downloads::state(id));
            }
        };
        match state() {
            State::NotDownloaded => {
                rsx! { button { class: "secondary big", onclick: start, "Download" } }
            }
            State::Running { fraction } => rsx! {
                button { class: "secondary big", disabled: true, "Downloading {(fraction * 100.0).round()}%" }
            },
            State::Paused { fraction } => rsx! {
                button { class: "secondary big", onclick: start, "Resume download ({(fraction * 100.0).round()}%)" }
            },
            State::Failed(e) => rsx! {
                button { class: "secondary big", onclick: start, "Download failed, try again" }
                p { class: "notice", "{e}" }
            },
            State::Done => rsx! {
                button {
                    class: "secondary big",
                    onclick: move |_| {
                        downloads::remove(id);
                        state.set(State::NotDownloaded);
                    },
                    "Downloaded \u{2713}  Remove"
                }
            },
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[component]
fn DownloadButton(item: Item) -> Element {
    let _ = item;
    rsx! {}
}
