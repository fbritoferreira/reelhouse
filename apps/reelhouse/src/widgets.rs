//! Building blocks shared by the pages: artwork, posters, wide cards, the hero and play buttons.

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

/// "1h 45m" or "45m".
pub fn runtime(seconds: f64) -> String {
    // Never "0m": anything shorter than a minute reads as a minute.
    let minutes = ((seconds / 60.0).round() as u64).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Fraction watched, for items started but not finished.
pub fn watched_fraction(item: &Item) -> Option<f64> {
    match (&item.progress, item.duration_s) {
        (Some(p), Some(d)) if d > 0.0 && !p.played && p.position_s > 0.0 => {
            Some((p.position_s / d).clamp(0.0, 1.0))
        }
        _ => None,
    }
}

/// "32 min left" for started items.
pub fn time_left(item: &Item) -> Option<String> {
    let p = item.progress.as_ref()?;
    let left = item.duration_s? - p.position_s;
    (watched_fraction(item).is_some() && left > 0.0).then(|| format!("{} left", runtime(left)))
}

pub fn is_watched(item: &Item) -> bool {
    item.progress.as_ref().is_some_and(|p| p.played)
}

/// The episode's own name: metadata stores it as "S01E05 · Name"; files give "Show S01E05".
pub fn episode_name(item: &Item) -> String {
    match item.title.split_once(" · ") {
        Some((code, name)) if code.starts_with('S') => name.to_string(),
        _ => match item.episode_no {
            Some(e) => format!("Episode {e}"),
            None => item.title.clone(),
        },
    }
}

/// Facts shown under a title, in reading order.
pub fn facts(item: &Item) -> Vec<String> {
    let mut out = Vec::new();
    match item.kind {
        ItemKind::Episode => {
            if let (Some(s), Some(e)) = (item.season_no, item.episode_no) {
                out.push(format!("Season {s}, episode {e}"));
            }
        }
        ItemKind::Season => {
            if let Some(s) = item.season_no {
                out.push(format!("Season {s}"));
            }
        }
        _ => {
            if let Some(y) = item.year {
                out.push(y.to_string());
            }
        }
    }
    if let Some(d) = item.duration_s.filter(|d| *d > 0.0) {
        out.push(runtime(d));
    }
    out
}

#[component]
pub fn Facts(item: Item) -> Element {
    rsx! {
        p { class: "facts",
            for fact in facts(&item) {
                span { "{fact}" }
            }
        }
    }
}

/// Initials for a title without artwork, set large on a gradient instead of repeating the title.
fn initials(title: &str) -> String {
    title
        .split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

#[component]
fn Progress(fraction: f64) -> Element {
    rsx! {
        span { class: "progress-track",
            span { class: "progress-fill", style: "width: {fraction * 100.0}%" }
        }
    }
}

#[component]
pub fn Poster(item: Item) -> Element {
    let art = art_url(item.poster_url.clone());
    let year = item.year.map(|y| y.to_string()).unwrap_or_default();
    rsx! {
        Link { class: "poster", to: Route::ItemPage { id: item.id },
            div { class: "poster-art",
                if let Some(url) = art {
                    img { src: url, alt: "", loading: "lazy" }
                } else {
                    span { class: "poster-fallback", "{initials(&item.title)}" }
                }
                if is_watched(&item) {
                    span { class: "watched", "aria-label": "Watched", "✓" }
                }
                if let Some(f) = watched_fraction(&item) {
                    Progress { fraction: f }
                }
            }
            span { class: "poster-title", "{item.title}" }
            if !year.is_empty() {
                span { class: "poster-caption", "{year}" }
            }
        }
    }
}

/// A 16:9 card for things in progress: the frame you stopped on, and how much is left.
#[component]
pub fn WideCard(item: Item) -> Element {
    let art = art_url(item.backdrop_url.clone().or(item.poster_url.clone()));
    let heading = item.show_title.clone().unwrap_or(item.title.clone());
    let detail = match (item.kind, item.season_no, item.episode_no) {
        (ItemKind::Episode, Some(s), Some(e)) => {
            let mut d = format!("S{s} E{e}");
            if let Some(left) = time_left(&item) {
                d.push_str(&format!("  {left}"));
            }
            d
        }
        _ => time_left(&item).unwrap_or_default(),
    };
    rsx! {
        Link { class: "wide", to: Route::Play { id: item.id },
            div { class: "wide-art",
                if let Some(url) = art {
                    img { src: url, alt: "", loading: "lazy" }
                } else {
                    span { class: "poster-fallback", "{initials(&heading)}" }
                }
                if let Some(f) = watched_fraction(&item) {
                    Progress { fraction: f }
                }
            }
            span { class: "poster-title", "{heading}" }
            if !detail.is_empty() {
                span { class: "poster-caption", "{detail}" }
            }
        }
    }
}

/// A titled, horizontally scrolling row.
#[component]
pub fn Row(title: String, to: Option<Route>, children: Element) -> Element {
    rsx! {
        section { class: "row",
            h2 {
                if let Some(to) = to {
                    Link { to, "{title}" }
                } else {
                    "{title}"
                }
            }
            div { class: "row-scroll", {children} }
        }
    }
}

/// Play, or Resume with how far in and how much is left.
#[component]
pub fn PlayButton(item: Item) -> Element {
    let fraction = watched_fraction(&item);
    rsx! {
        div { class: "play-group",
            Link { class: "primary big", autofocus: true, to: Route::Play { id: item.id },
                span { class: "play-icon", "aria-hidden": "true" }
                if fraction.is_some() { "Resume" } else { "Play" }
            }
            if let (Some(f), Some(left)) = (fraction, time_left(&item)) {
                div { class: "resume-meter",
                    Progress { fraction: f }
                    span { "{left}" }
                }
            }
        }
    }
}

/// Full-bleed backdrop with the title over a scrim. `above` sits over the title (a show link),
/// `children` below the facts (buttons, overview).
#[component]
pub fn Hero(backdrop: Option<String>, title: String, above: Element, children: Element) -> Element {
    rsx! {
        section { class: "hero",
            if let Some(url) = art_url(backdrop) {
                img { class: "hero-art", src: url, alt: "" }
            }
            div { class: "hero-scrim" }
            div { class: "hero-body",
                {above}
                h1 { class: "hero-title", "{title}" }
                {children}
            }
        }
    }
}

/// Renders a load error.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_reads_naturally() {
        assert_eq!(runtime(6329.0), "1h 45m");
        assert_eq!(runtime(2700.0), "45m");
        assert_eq!(runtime(7200.0), "2h");
        assert_eq!(runtime(20.0), "1m");
    }

    #[test]
    fn initials_skip_punctuation() {
        assert_eq!(initials("Alien: Earth"), "AE");
        assert_eq!(initials("365 Days"), "3D");
    }
}
