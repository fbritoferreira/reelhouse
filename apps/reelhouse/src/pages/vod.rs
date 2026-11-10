//! On demand: the films and series Live TV sources offer, browsed like the channels beside
//! them. Groups down the left (gathered by country), posters on the right, drawn a page at a time.

use dioxus::prelude::*;
use reelhouse_api::{Progress, VodEpisode, VodGroup, VodMovie, VodShow as Show, VodTitle};

use super::live::{ModeToggle, families_of, split_prefix};
use crate::Route;
use crate::session::client;
use crate::widgets::{LoadError, describe};

const KIND_KEY: &str = "reelhouse.vod.kind";
/// The group key standing for "Continue watching" (no real group can start with a NUL).
const CONTINUE: &str = "\u{0}continue";

/// How far into a title, for its progress bar; nothing once finished or before the length is known.
fn watched(progress: &Option<Progress>, duration: Option<f64>) -> Option<f64> {
    match (progress, duration) {
        (Some(p), Some(d)) if !p.played && d > 0.0 && p.position_s > 0.0 => {
            Some((p.position_s / d).clamp(0.0, 1.0))
        }
        _ => None,
    }
}
/// Posters drawn at a time.
const PAGE: usize = 48;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Movies,
    Series,
}

impl Kind {
    fn id(self) -> &'static str {
        match self {
            Kind::Movies => "movies",
            Kind::Series => "series",
        }
    }
    fn saved() -> Self {
        match crate::session::load(KIND_KEY).as_deref() {
            Some("series") => Kind::Series,
            _ => Kind::Movies,
        }
    }
}

/// Where you were in On demand, kept above the page so Back from a title returns to it.
#[derive(Clone, Default, PartialEq)]
struct VodPlace {
    group: Option<String>,
    shown: usize,
}

static PLACE: GlobalSignal<VodPlace> = Signal::global(VodPlace::default);

#[component]
pub fn VodBrowser() -> Element {
    let mut kind = use_signal(Kind::saved);
    let groups = use_resource(move || async move { client().vod_groups(kind().id()).await });
    let started = use_resource(|| async move { client().vod_continue().await.unwrap_or_default() });
    let started: Vec<VodTitle> = started.read().clone().unwrap_or_default();
    let list: Vec<VodGroup> = match &*groups.read() {
        Some(Ok(g)) => g.clone(),
        Some(Err(e)) => {
            return rsx! { div { class: "page page-top", LoadError { error: describe(e) } } };
        }
        None => Vec::new(),
    };
    let key = |n: &Option<String>| n.clone().unwrap_or_default();
    let selected = PLACE
        .read()
        .group
        .clone()
        .filter(|g| {
            list.iter().any(|l| key(&l.name) == *g) || (g == CONTINUE && !started.is_empty())
        })
        .or_else(|| list.first().map(|g| key(&g.name)));
    let choose = |k: String| {
        let mut p = PLACE.write();
        if p.group.as_deref() != Some(k.as_str()) {
            *p = VodPlace {
                group: Some(k),
                shown: PAGE,
            };
        }
    };
    let pairs: Vec<(Option<String>, i64)> =
        list.iter().map(|g| (g.name.clone(), g.titles)).collect();
    rsx! {
        div { class: "page page-top live",
            div { class: "live-body",
                aside { class: "live-groups",
                    ModeToggle { on_demand: true }
                    div { class: "live-sort", role: "radiogroup", "aria-label": "Films or series",
                        for (k, label) in [(Kind::Movies, "Films"), (Kind::Series, "Series")] {
                            button {
                                key: "{k.id()}",
                                role: "radio",
                                class: if kind() == k { "live-sort-option current" } else { "live-sort-option" },
                                "aria-checked": kind() == k,
                                onclick: move |_| {
                                    kind.set(k);
                                    crate::session::save(KIND_KEY, Some(k.id()));
                                    *PLACE.write() = VodPlace::default();
                                },
                                "{label}"
                            }
                        }
                    }
                    if groups.read().is_none() {
                        for i in 0..9 {
                            span { key: "{i}", class: "skeleton skeleton-line" }
                        }
                    }
                    nav { class: "live-group-list", "aria-label": "Groups",
                        if !started.is_empty() {
                            button {
                                class: if selected.as_deref() == Some(CONTINUE) { "live-group continue current" } else { "live-group continue" },
                                "aria-current": selected.as_deref() == Some(CONTINUE),
                                onclick: move |_| choose(CONTINUE.to_string()),
                                span { class: "live-group-name", "Continue watching" }
                                span { class: "live-group-count", "{started.len()}" }
                            }
                        }
                        for fam in families_of(&pairs) {
                            if let Some(country) = fam.name.clone() {
                                p { key: "{country}", class: "live-family-label", "{country}" }
                            }
                            for (name, count, label) in fam.members {
                                {
                                    let k = key(&name);
                                    let current = Some(&k) == selected.as_ref();
                                    let nested = fam.name.is_some();
                                    rsx! {
                                        button {
                                            key: "{k}",
                                            class: match (current, nested) {
                                                (true, true) => "live-group nested current",
                                                (true, false) => "live-group current",
                                                (false, true) => "live-group nested",
                                                (false, false) => "live-group",
                                            },
                                            "aria-current": current,
                                            onclick: move |_| choose(k.clone()),
                                            span { class: "live-group-name", "{label}" }
                                            span { class: "live-group-count", "{count}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                section { class: "live-main",
                    if selected.as_deref() == Some(CONTINUE) {
                        header { class: "live-main-head", h2 { "Continue watching" } }
                        div { class: "grid vod-grid",
                            for t in started.clone() {
                                VodPoster {
                                    key: "{t.id}",
                                    to: Route::PlayVod { id: t.id },
                                    title: t.show_title.clone().unwrap_or(t.title.clone()),
                                    caption: match (t.show_title.is_some(), t.season_no, t.episode_no) {
                                        (true, Some(s), Some(e)) => format!("S{s} E{e}  {}", t.title),
                                        _ => String::new(),
                                    },
                                    poster: t.poster_url.clone(),
                                    near_end: false,
                                    fraction: watched(&t.progress, t.duration_s),
                                }
                            }
                        }
                    } else if let Some(g) = selected.clone() {
                        {
                            let (country, short) = match split_prefix(&g) {
                                Some((c, s)) => (Some(c.to_string()), s.to_string()),
                                None => (None, if g.is_empty() { "Other titles".to_string() } else { g.clone() }),
                            };
                            rsx! {
                                header { class: "live-main-head",
                                    h2 { "{short}" }
                                    if let Some(c) = country {
                                        span { class: "quiet", "{c}" }
                                    }
                                }
                                if kind() == Kind::Movies {
                                    Films { key: "m{g}", group: g.clone() }
                                } else {
                                    Shows { key: "s{g}", group: g.clone() }
                                }
                            }
                        }
                    } else if groups.read().is_some() {
                        p { class: "empty", "Your sources offer nothing on demand. Films and series in an M3U playlist appear here after a refresh." }
                    }
                }
            }
        }
    }
}

fn shown() -> usize {
    PLACE.read().shown.max(PAGE)
}

fn grow() {
    let mut p = PLACE.write();
    p.shown = p.shown.max(PAGE) + PAGE;
}

#[component]
fn PosterGrid(total: usize, children: Element) -> Element {
    let limit = shown().min(total);
    rsx! {
        div { class: "grid vod-grid", {children} }
        if limit < total {
            button { class: "secondary live-more", onclick: move |_| grow(),
                "Show {(total - limit).min(PAGE)} more of {total - limit}"
            }
        }
    }
}

#[component]
fn Films(group: String) -> Element {
    let films = use_resource(use_reactive!(|group| async move {
        client().vod_movies(&group).await
    }));
    let list: Vec<VodMovie> = match &*films.read() {
        Some(Ok(f)) => f.clone(),
        Some(Err(e)) => return rsx! { LoadError { error: describe(e) } },
        None => return rsx! { PosterSkeleton {} },
    };
    let limit = shown().min(list.len());
    rsx! {
        PosterGrid { total: list.len(),
            for (i, f) in list[..limit].iter().cloned().enumerate() {
                VodPoster {
                    key: "{f.id}",
                    to: Route::PlayVod { id: f.id },
                    title: f.title.clone(),
                    caption: f.year.map(|y| y.to_string()).unwrap_or_default(),
                    poster: f.poster_url.clone(),
                    near_end: i + 8 >= limit,
                    fraction: watched(&f.progress, f.duration_s),
                }
            }
        }
    }
}

#[component]
fn Shows(group: String) -> Element {
    let shows = use_resource(use_reactive!(|group| async move {
        client().vod_series(&group).await
    }));
    let list: Vec<Show> = match &*shows.read() {
        Some(Ok(s)) => s.clone(),
        Some(Err(e)) => return rsx! { LoadError { error: describe(e) } },
        None => return rsx! { PosterSkeleton {} },
    };
    let limit = shown().min(list.len());
    rsx! {
        PosterGrid { total: list.len(),
            for (i, s) in list[..limit].iter().cloned().enumerate() {
                VodPoster {
                    key: "{s.title}",
                    to: Route::VodShow { title: s.title.clone() },
                    title: s.title.clone(),
                    caption: if s.seasons > 1 { format!("{} seasons", s.seasons) } else if s.episodes == 1 { "1 episode".into() } else { format!("{} episodes", s.episodes) },
                    poster: s.poster_url.clone(),
                    near_end: i + 8 >= limit,
                    fraction: None,
                }
            }
        }
    }
}

#[component]
fn VodPoster(
    to: Route,
    title: String,
    caption: String,
    poster: Option<String>,
    near_end: bool,
    fraction: Option<f64>,
) -> Element {
    rsx! {
        // Link takes no focus handler; the wrapper hears its focus.
        div { class: "vod-cell", onfocusin: move |_| if near_end { grow() },
        Link {
            class: "poster",
            to,
            div { class: "poster-art",
                if let Some(url) = poster {
                    img { src: url, alt: "", loading: "lazy", decoding: "async" }
                } else {
                    span { class: "poster-fallback", "{initials(&title)}" }
                }
                if let Some(f) = fraction {
                    span { class: "progress-track",
                        span { class: "progress-fill", style: "width: {f * 100.0}%" }
                    }
                }
            }
            span { class: "poster-title", "{title}" }
            if !caption.is_empty() {
                span { class: "poster-caption", "{caption}" }
            }
        }
        }
    }
}

fn initials(title: &str) -> String {
    title
        .split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

#[component]
fn PosterSkeleton() -> Element {
    rsx! {
        div { class: "grid vod-grid", "aria-busy": "true",
            for i in 0..12 {
                span { key: "{i}", class: "skeleton skeleton-poster" }
            }
        }
    }
}

/// A series' episodes, season by season.
#[component]
pub fn VodShow(title: String) -> Element {
    let eps = use_resource(use_reactive!(|title| async move {
        client().vod_episodes(&title).await
    }));
    let list: Vec<VodEpisode> = match &*eps.read() {
        Some(Ok(e)) => e.clone(),
        Some(Err(e)) => {
            return rsx! { div { class: "page page-top", LoadError { error: describe(e) } } };
        }
        None => return rsx! { div { class: "page page-top", PosterSkeleton {} } },
    };
    let mut seasons: Vec<Option<i64>> = list.iter().map(|e| e.season_no).collect();
    seasons.dedup();
    let poster = list.iter().find_map(|e| e.poster_url.clone());
    rsx! {
        div { class: "page page-top vod-show",
            header { class: "vod-show-head",
                if let Some(url) = poster {
                    img { class: "vod-show-poster", src: url, alt: "" }
                }
                div {
                    h1 { "{title}" }
                    p { class: "quiet",
                        if seasons.len() > 1 { "{seasons.len()} seasons, {list.len()} episodes" } else { "{list.len()} episodes" }
                    }
                }
            }
            for s in seasons {
                section { key: "{s:?}", class: "vod-season",
                    h2 { match s { Some(n) => format!("Season {n}"), None => "Episodes".to_string() } }
                    ol { class: "vod-episodes",
                        for e in list.iter().filter(|e| e.season_no == s).cloned() {
                            li { key: "{e.id}",
                                Link { class: "vod-episode", to: Route::PlayVod { id: e.id },
                                    span { class: "episode-number", "{e.episode_no.map(|n| n.to_string()).unwrap_or_default()}" }
                                    span { class: "episode-name", "{e.title}" }
                                    if let Some(f) = watched(&e.progress, e.duration_s) {
                                        span { class: "progress-track vod-episode-progress",
                                            span { class: "progress-fill", style: "width: {f * 100.0}%" }
                                        }
                                    } else if e.progress.as_ref().is_some_and(|p| p.played) {
                                        span { class: "watched-inline", "aria-label": "Watched", "✓" }
                                    }
                                    span { class: "play-icon", "aria-hidden": "true" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
