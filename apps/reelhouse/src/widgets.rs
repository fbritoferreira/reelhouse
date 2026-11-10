//! Posters, rows and status messages shared by the pages.

use dioxus::prelude::*;
use reelhouse_api::{Item, ItemKind};
use reelhouse_core::ClientError;

use crate::Route;

/// Artwork stored on the server needs the session token, since `<img>` cannot send headers.
/// Remote fallback URLs are used as they are.
pub fn art_url(url: Option<String>) -> Option<String> {
    let url = url?;
    Some(if url.starts_with('/') {
        crate::session::client().media_url(&url)
    } else {
        url
    })
}

pub fn caption(item: &Item) -> String {
    match (item.kind, item.year, item.season_no, item.episode_no) {
        (ItemKind::Episode, _, Some(s), Some(e)) => format!("Season {s}, episode {e}"),
        (ItemKind::Season, _, Some(s), _) => format!("Season {s}"),
        (_, Some(y), _, _) => y.to_string(),
        _ => String::new(),
    }
}

fn target(item: &Item) -> Route {
    match item.kind {
        ItemKind::Movie | ItemKind::Episode => Route::ItemPage { id: item.id },
        ItemKind::Show | ItemKind::Season => Route::ItemPage { id: item.id },
    }
}

#[component]
pub fn Poster(item: Item) -> Element {
    let watched = item.progress.as_ref().is_some_and(|p| p.played);
    let fraction = match (&item.progress, item.duration_s) {
        (Some(p), Some(d)) if d > 0.0 && !p.played => Some((p.position_s / d).clamp(0.0, 1.0)),
        _ => None,
    };
    let cap = caption(&item);
    rsx! {
        Link { class: "poster", to: target(&item),
            div { class: "poster-art",
                if let Some(url) = art_url(item.poster_url.clone().or(item.backdrop_url.clone())) {
                    img { src: url, alt: "", loading: "lazy" }
                } else {
                    span { class: "poster-fallback", "{item.title}" }
                }
                if watched {
                    span { class: "watched", "Watched" }
                }
                if let Some(f) = fraction {
                    span { class: "progress", style: "width: {f * 100.0}%" }
                }
            }
            span { class: "poster-title", "{item.title}" }
            if !cap.is_empty() {
                span { class: "poster-caption", "{cap}" }
            }
        }
    }
}

#[component]
pub fn Row(title: String, items: Vec<Item>) -> Element {
    rsx! {
        section { class: "row",
            h2 { "{title}" }
            div { class: "row-scroll",
                for item in items {
                    Poster { key: "{item.id}", item: item.clone() }
                }
            }
        }
    }
}

/// Renders a load error; signs out on an expired session.
#[component]
pub fn LoadError(error: String) -> Element {
    rsx! { p { class: "notice", "Couldn't load this page: {error}" } }
}

pub fn describe(e: &ClientError) -> String {
    match e {
        ClientError::Unauthorized => "your session has ended. Sign in again.".into(),
        e => e.to_string(),
    }
}
