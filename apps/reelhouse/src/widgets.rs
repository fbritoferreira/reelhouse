//! Building blocks shared by the pages: artwork, posters, wide cards, the hero and play buttons.

use dioxus::prelude::*;
use reelhouse_api::{Item, ItemKind};
use reelhouse_core::ClientError;

use crate::Route;

/// Artwork stored on the server needs the session token, since `<img>` cannot send headers.
/// Remote fallback URLs are used as they are.
pub fn art_url(url: Option<String>) -> Option<String> {
    let url = url?;
    if !url.starts_with('/') {
        return Some(url);
    }
    // Native apps go through their own origin (see art.rs); the web app is served by the server.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(local) = crate::art::local_path(&url) {
        return Some(local);
    }
    Some(crate::session::client().media_url(&url))
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

/// "4K", "1080p", "720p" or "SD" for a picture `height` pixels tall. Widescreen films are
/// letterboxed short (a 2.39:1 film is 1600 tall in 4K, 800 in 1080p), so each step starts at its
/// most letterboxed height, which stays above the full height of the step below.
pub fn quality(height: Option<i64>) -> Option<&'static str> {
    Some(match height? {
        h if h >= 1500 => "4K",
        h if h >= 780 => "1080p",
        h if h >= 520 => "720p",
        h if h > 0 => "SD",
        _ => return None,
    })
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
            if let Some(q) = quality(item.height) {
                span { class: "quality", "{q}" }
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
                if let Some(q) = quality(item.height).filter(|q| *q == "4K") {
                    span { class: "quality corner", "{q}" }
                }
                Peek { item: item.clone() }
            }
            span { class: "poster-title", "{item.title}" }
            if !year.is_empty() {
                span { class: "poster-caption", "{year}" }
            }
        }
    }
}

/// What a poster is, shown over it after a moment's hover (pointer devices only; see CSS).
#[component]
fn Peek(item: Item) -> Element {
    let nav = navigator();
    let id = item.id;
    let playable = matches!(item.kind, ItemKind::Movie | ItemKind::Episode);
    rsx! {
        span { class: "peek", "aria-hidden": "true",
            span { class: "peek-title", "{item.title}" }
            span { class: "peek-facts",
                for fact in facts(&item) {
                    span { "{fact}" }
                }
            }
            if let Some(o) = item.overview.clone() {
                span { class: "peek-overview", "{o}" }
            }
            if playable {
                span {
                    class: "peek-play",
                    onclick: move |e| {
                        // Inside the poster's link: play instead of opening the page.
                        e.prevent_default();
                        e.stop_propagation();
                        nav.push(Route::Play { id });
                    },
                    span { class: "play-icon" }
                    if watched_fraction(&item).is_some() { "Resume" } else { "Play" }
                }
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
        (ItemKind::Episode, Some(s), Some(e)) => match time_left(&item) {
            Some(left) => format!("S{s} E{e}  {left}"),
            // Not started: Continue watching offers it as the one after a finished episode.
            None => format!("Next episode, S{s} E{e}"),
        },
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

/// Marks a title watched or not. On-demand titles keep their own progress (`on_demand`).
/// `compact` is the round tick on episode rows; otherwise a labelled button for a title's page.
#[component]
pub fn WatchedToggle(
    id: i64,
    watched: bool,
    #[props(default)] compact: bool,
    #[props(default)] on_demand: bool,
) -> Element {
    let mut on = use_signal(|| watched);
    // A refreshed list brings the server's word.
    use_effect(use_reactive!(|watched| on.set(watched)));
    let toggle = move |_| async move {
        let now = !on();
        on.set(now);
        let p = reelhouse_api::Progress {
            position_s: 0.0,
            played: now,
        };
        let c = crate::session::client();
        let saved = if on_demand {
            c.put_vod_progress(id, &p).await
        } else {
            c.put_progress(id, &p).await
        };
        if saved.is_err() {
            on.set(!now);
        }
    };
    let label = if on() {
        "Mark unwatched"
    } else {
        "Mark watched"
    };
    if compact {
        rsx! {
            button {
                class: if on() { "watched-toggle on" } else { "watched-toggle" },
                "aria-pressed": on(),
                "aria-label": label,
                title: label,
                onclick: toggle,
                svg { view_box: "0 0 24 24", "aria-hidden": "true",
                    path { d: "M5 12.5l4.5 4.5L19 7.5" }
                }
            }
        }
    } else {
        rsx! {
            button {
                class: if on() { "secondary big watched-button on" } else { "secondary big watched-button" },
                "aria-pressed": on(),
                title: label,
                onclick: toggle,
                svg { view_box: "0 0 24 24", "aria-hidden": "true",
                    path { d: "M5 12.5l4.5 4.5L19 7.5" }
                }
                if on() { "Watched" } else { "Mark watched" }
            }
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
        // The server words these for people; the status code adds nothing.
        ClientError::Status(400 | 409 | 502, why) if !why.trim().is_empty() => why.clone(),
        e => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_reasons_are_shown_as_written() {
        let e = ClientError::Status(502, "your provider no longer carries this channel".into());
        assert_eq!(describe(&e), "your provider no longer carries this channel");
        let e = ClientError::Status(500, "boom".into());
        assert_eq!(describe(&e), "server returned 500: boom");
    }

    #[test]
    fn runtime_reads_naturally() {
        assert_eq!(runtime(6329.0), "1h 45m");
        assert_eq!(runtime(2700.0), "45m");
        assert_eq!(runtime(7200.0), "2h");
        assert_eq!(runtime(20.0), "1m");
    }

    #[test]
    fn quality_steps() {
        // Heights found in a real library, letterboxed ones included.
        for (h, q) in [
            (2160, "4K"),
            (2072, "4K"),
            (1606, "4K"),
            (1080, "1080p"),
            (960, "1080p"),
            (816, "1080p"),
            (800, "1080p"),
            (720, "720p"),
            (718, "720p"),
            (480, "SD"),
        ] {
            assert_eq!(quality(Some(h)), Some(q), "{h}");
        }
        assert_eq!(quality(None), None);
    }

    #[test]
    fn initials_skip_punctuation() {
        assert_eq!(initials("Alien: Earth"), "AE");
        assert_eq!(initials("365 Days"), "3D");
    }
}
