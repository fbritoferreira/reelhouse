//! Search across the libraries' titles and Live TV channel names.

use dioxus::prelude::*;
use reelhouse_api::Item;

use crate::Route;
use crate::session::client;
use crate::widgets::Poster;

#[component]
pub fn Search() -> Element {
    let mut query = use_signal(String::new);
    // Every library's top-level titles, loaded once and filtered as you type.
    let titles = use_resource(|| async move {
        let c = client();
        let mut all: Vec<Item> = Vec::new();
        for lib in c.libraries().await.unwrap_or_default() {
            all.extend(c.library_items(lib.id).await.unwrap_or_default());
        }
        all
    });
    let channels = use_resource(move || async move {
        let q = query();
        if q.trim().chars().count() < 2 {
            return Vec::new();
        }
        // Typing fast replaces this request before it is sent.
        futures_timer::Delay::new(std::time::Duration::from_millis(250)).await;
        client().search_channels(q.trim()).await.unwrap_or_default()
    });
    let on_demand = use_resource(move || async move {
        let q = query();
        if q.trim().chars().count() < 2 {
            return reelhouse_api::VodSearch::default();
        }
        futures_timer::Delay::new(std::time::Duration::from_millis(250)).await;
        client().vod_search(q.trim()).await.unwrap_or_default()
    });
    let vod = on_demand.read().clone().unwrap_or_default();
    let q = query().trim().to_lowercase();
    let matches: Vec<Item> = match (&*titles.read(), q.is_empty()) {
        (Some(all), false) => all
            .iter()
            .filter(|i| i.title.to_lowercase().contains(&q))
            .take(30)
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let chans = channels.read().clone().unwrap_or_default();
    rsx! {
        div { class: "page page-top search",
            input {
                class: "search-field",
                r#type: "search",
                placeholder: "Films, shows and channels",
                "aria-label": "Search",
                autofocus: true,
                value: "{query}",
                oninput: move |e| query.set(e.value()),
            }
            if q.is_empty() {
                p { class: "quiet", "Type a title or a channel name." }
            } else if matches.is_empty() && chans.is_empty() && vod.films.is_empty() && vod.shows.is_empty() {
                p { class: "quiet", "Nothing matches \u{201c}{query}\u{201d}." }
            }
            if !matches.is_empty() {
                section { class: "row",
                    h2 { "Films and shows" }
                    div { class: "grid",
                        for item in matches {
                            Poster { key: "{item.id}", item }
                        }
                    }
                }
            }
            if !vod.films.is_empty() {
                section { class: "row",
                    h2 { "On demand films" }
                    div { class: "grid",
                        for f in vod.films.clone() {
                            Link { key: "{f.id}", class: "poster", to: Route::VodFilm { id: f.id },
                                div { class: "poster-art",
                                    if let Some(url) = f.poster_url.clone() {
                                        img { src: url, alt: "", loading: "lazy" }
                                    }
                                }
                                span { class: "poster-title", "{f.title}" }
                                if let Some(y) = f.year {
                                    span { class: "poster-caption", "{y}" }
                                }
                            }
                        }
                    }
                }
            }
            if !vod.shows.is_empty() {
                section { class: "row",
                    h2 { "On demand series" }
                    div { class: "grid",
                        for s in vod.shows.clone() {
                            Link { key: "{s.title}", class: "poster", to: Route::VodShow { title: s.title.clone() },
                                div { class: "poster-art",
                                    if let Some(url) = s.poster_url.clone() {
                                        img { src: url, alt: "", loading: "lazy" }
                                    }
                                }
                                span { class: "poster-title", "{s.title}" }
                                span { class: "poster-caption",
                                    if s.seasons > 1 { "{s.seasons} seasons" } else { "{s.episodes} episodes" }
                                }
                            }
                        }
                    }
                }
            }
            if !chans.is_empty() {
                section { class: "row",
                    h2 { "Channels" }
                    ul { class: "search-channels",
                        for c in chans {
                            li { key: "{c.id}",
                                Link {
                                    class: if crate::widgets::channel_trouble(&c).is_some() { "channel refused" } else { "channel" },
                                    title: c.refused.clone().or(crate::widgets::channel_trouble(&c).map(str::to_string)),
                                    to: Route::Watch { id: c.id, group: c.group_title.clone().unwrap_or_default().into() },
                                    span { class: "channel-number", "{c.number.map(|n| n.to_string()).unwrap_or_default()}" }
                                    span { class: "channel-logo",
                                        if let Some(url) = c.logo_url.clone() {
                                            img { src: url, alt: "", loading: "lazy" }
                                        }
                                    }
                                    span { class: "channel-text",
                                        span { class: "channel-name", "{c.name}" }
                                        if let Some(trouble) = crate::widgets::channel_trouble(&c) {
                                            span { class: "channel-now quiet", "{trouble}" }
                                        } else if let Some(g) = c.group_title.clone() {
                                            span { class: "channel-now", "{g}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
