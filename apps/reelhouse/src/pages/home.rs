use dioxus::prelude::*;
use reelhouse_api::{Item, VodTitle};

use crate::Route;
use crate::live_cache::LIVE;
use crate::session::client;
use crate::widgets::{Facts, Hero, LoadError, PlayButton, Poster, Row, WideCard, describe};

#[component]
pub fn Home() -> Element {
    let data = use_resource(|| async move {
        let c = client();
        let cont = c.continue_watching().await?;
        let added = c.recently_added().await.unwrap_or_default();
        let libs = c.libraries().await?;
        let mut rows = Vec::new();
        for lib in libs {
            let items = c.library_items(lib.id).await?;
            rows.push((lib, items));
        }
        Ok::<_, reelhouse_core::ClientError>((cont, added, rows))
    });
    let on_demand =
        use_resource(|| async move { client().vod_continue().await.unwrap_or_default() });
    let started: Vec<VodTitle> = on_demand.read().clone().unwrap_or_default();
    let recent = LIVE.read().recent.clone();
    match &*data.read() {
        None => rsx! { HomeSkeleton {} },
        Some(Err(e)) => rsx! { div { class: "page", LoadError { error: describe(e) } } },
        Some(Ok((cont, added, rows))) => {
            // The hero is what you are in the middle of, else the newest title with artwork, else
            // any title with artwork.
            let featured: Option<Item> = cont
                .iter()
                .chain(added)
                .chain(rows.iter().flat_map(|(_, items)| items))
                .find(|i| i.backdrop_url.is_some())
                .cloned();
            rsx! {
                if let Some(item) = featured {
                    Hero {
                        backdrop: item.backdrop_url.clone(),
                        title: item.show_title.clone().unwrap_or(item.title.clone()),
                        above: rsx! {},
                        Facts { item: item.clone() }
                        div { class: "hero-actions",
                            PlayButton { item: item.clone() }
                            Link { class: "secondary big", to: Route::ItemPage { id: item.show_id.unwrap_or(item.id) }, "More about it" }
                        }
                        if let Some(o) = item.overview.clone() {
                            p { class: "overview", "{o}" }
                        }
                    }
                }
                div { class: "page rows",
                    if !cont.is_empty() {
                        Row { title: "Continue watching", to: None,
                            for item in cont.clone() {
                                WideCard { key: "{item.id}", item }
                            }
                        }
                    }
                    if !started.is_empty() {
                        Row { title: "On demand, continue watching", to: Some(Route::LiveTv {}),
                            for t in started.clone() {
                                Link { key: "{t.id}", class: "poster", to: Route::PlayVod { id: t.id },
                                    div { class: "poster-art",
                                        if let Some(url) = t.poster_url.clone() {
                                            img { src: url, alt: "", loading: "lazy" }
                                        }
                                        if let (Some(p), Some(d)) = (&t.progress, t.duration_s.filter(|d| *d > 0.0)) {
                                            span { class: "progress-track",
                                                span { class: "progress-fill", style: "width: {(p.position_s / d * 100.0).min(100.0)}%" }
                                            }
                                        }
                                    }
                                    span { class: "poster-title", "{t.show_title.clone().unwrap_or(t.title.clone())}" }
                                    if let (Some(s), Some(e)) = (t.season_no, t.episode_no) {
                                        span { class: "poster-caption", "S{s} E{e}" }
                                    }
                                }
                            }
                        }
                    }
                    if !recent.is_empty() {
                        Row { title: "Recent channels", to: Some(Route::LiveTv {}),
                            for c in recent.clone() {
                                Link {
                                    key: "{c.id}",
                                    class: "channel-tile",
                                    to: Route::Watch { id: c.id, group: c.group.clone().into() },
                                    span { class: "channel-tile-art",
                                        if let Some(url) = c.logo_url.clone() {
                                            img { src: url, alt: "", loading: "lazy" }
                                        } else {
                                            span { class: "channel-monogram", "{monogram(&c.name)}" }
                                        }
                                        span { class: "lamp", "aria-hidden": "true" }
                                    }
                                    span { class: "poster-title", "{c.name}" }
                                    if let Some(n) = c.number {
                                        span { class: "poster-caption", "{n}" }
                                    }
                                }
                            }
                        }
                    }
                    if rows.is_empty() {
                        div { class: "empty",
                            h2 { "No libraries yet" }
                            p { "Add your Movies and TV folders, and Reelhouse finds artwork and details for everything in them." }
                            Link { class: "primary big", to: Route::Admin {}, "Add a library" }
                        }
                    }
                    if !added.is_empty() {
                        Row { title: "Recently added", to: None,
                            for item in added.clone() {
                                Poster { key: "{item.id}", item }
                            }
                        }
                    }
                    for (lib, items) in rows.clone() {
                        Row { key: "{lib.id}", title: lib.name.clone(), to: Some(Route::LibraryPage { id: lib.id }),
                            for item in items.into_iter().take(30) {
                                Poster { key: "{item.id}", item }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Two letters for a channel without a logo: `UK: BBC One` gives `BO`.
fn monogram(name: &str) -> String {
    let name = name.split_once(": ").map_or(name, |(_, rest)| rest);
    name.split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// Home's shape while it loads: a hero and two rows, so nothing jumps when they arrive.
#[component]
fn HomeSkeleton() -> Element {
    rsx! {
        div { "aria-busy": "true",
            div { class: "skeleton skeleton-hero" }
            div { class: "page rows",
                for r in 0..2 {
                    section { key: "{r}", class: "row",
                        span { class: "skeleton skeleton-heading" }
                        div { class: "skeleton-row-posters",
                            for i in 0..7 {
                                span { key: "{i}", class: "skeleton skeleton-poster" }
                            }
                        }
                    }
                }
            }
        }
    }
}
