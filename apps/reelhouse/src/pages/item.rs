use dioxus::prelude::*;
use reelhouse_api::{Item, ItemKind};

use crate::Route;
use crate::download_ui::{CollectionDownload, DownloadButton, EpisodeDownload};
use crate::session::client;
use crate::widgets::{
    Facts, Hero, LoadError, PlayButton, WatchedToggle, art_url, describe, episode_name, is_watched,
    runtime, watched_fraction,
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
                DownloadButton { item: item.clone() }
                WatchedToggle { id: item.id, watched: is_watched(&item) }
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
            div { class: "hero-actions", CollectionDownload { scope: show.clone() } }
            if let Some(o) = show.overview.clone() {
                p { class: "overview", "{o}" }
            }
        }
        div { class: "page",
            div { class: "season-bar",
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
                // Only worth a separate control when the show has more than one season.
                if let Some(season) = seasons_list.iter().find(|s| Some(s.id) == selected).filter(|_| seasons_list.len() > 1) {
                    CollectionDownload { key: "{season.id}", scope: season.clone() }
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
    let mut eps = use_resource(use_reactive!(|season| async move {
        client().children(season).await
    }));
    let mut marking = use_signal(|| false);
    match &*eps.read() {
        None => rsx! { p { class: "quiet", "Loading episodes" } },
        Some(Err(e)) => rsx! { LoadError { error: describe(e) } },
        Some(Ok(list)) => rsx! {
            {
                let all_watched = !list.is_empty() && list.iter().all(is_watched);
                let ids: Vec<i64> = list.iter().map(|e| e.id).collect();
                rsx! {
                    div { class: "episodes-head",
                        button {
                            class: "secondary",
                            disabled: marking(),
                            onclick: move |_| {
                                let ids = ids.clone();
                                async move {
                                    marking.set(true);
                                    let c = client();
                                    let p = reelhouse_api::Progress { position_s: 0.0, played: !all_watched };
                                    for id in ids {
                                        let _ = c.put_progress(id, &p).await;
                                    }
                                    marking.set(false);
                                    eps.restart();
                                }
                            },
                            if all_watched { "Mark season unwatched" } else { "Mark season watched" }
                        }
                    }
                }
            }
            ol { class: "episodes",
                for ep in list.clone() {
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
        li { class: "episode-item",
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

                    }
                    if !length.is_empty() {
                        p { class: "quiet episode-length", "{length}"
                            if let Some(q) = crate::widgets::quality(ep.height) {
                                span { class: "quality", "{q}" }
                            }
                        }
                    }
                    if let Some(o) = ep.overview.clone() {
                        p { class: "episode-overview", "{o}" }
                    }
                }
            }
            WatchedToggle { id: ep.id, watched: is_watched(&ep), compact: true }
            EpisodeDownload { ep: ep.clone() }
        }
    }
}
