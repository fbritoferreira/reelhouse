//! Live TV: channel groups beside the selected group's channels with what is on now and next,
//! and a full-screen player that changes channel with Up and Down.

use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;
use reelhouse_api::{GuideChannel, Programme};
use reelhouse_core::timefmt::{clock, left};

use crate::Route;
use crate::player::Player;
use crate::session::client;
use crate::widgets::{LoadError, describe};

/// The device's clock: Unix seconds and JavaScript's timezone offset, read through the webview so
/// web and Android agree. Re-read every 30 seconds.
#[derive(Clone, Copy, PartialEq, Default)]
struct Clock {
    now: i64,
    offset_min: i64,
}

fn use_clock() -> Signal<Clock> {
    let mut c = use_signal(Clock::default);
    use_future(move || async move {
        loop {
            let read = document::eval("return [Date.now(), new Date().getTimezoneOffset()]")
                .join::<(f64, i64)>()
                .await;
            if let Ok((ms, offset_min)) = read {
                c.set(Clock {
                    now: (ms / 1000.0) as i64,
                    offset_min,
                });
            }
            futures_timer::Delay::new(Duration::from_secs(30)).await;
        }
    });
    c
}

/// What is on at `now`, and what follows it.
fn now_next(programmes: &[Programme], now: i64) -> (Option<&Programme>, Option<&Programme>) {
    let current = programmes.iter().find(|p| p.start <= now && now < p.stop);
    let next = programmes
        .iter()
        .find(|p| p.start >= current.map_or(now, |c| c.stop));
    (current, next)
}

fn group_label(name: &Option<String>) -> String {
    name.clone().unwrap_or_else(|| "Other channels".into())
}

#[component]
pub fn LiveTv() -> Element {
    let groups = use_resource(|| async move { client().channel_groups().await });
    let mut chosen = use_signal(|| None::<String>);
    // Counts focus moves, so a pending group switch can tell it has been overtaken.
    let mut focused = use_signal(|| 0u64);
    let time = use_clock();
    let list = match &*groups.read() {
        None => {
            return rsx! { div { class: "page page-top", p { class: "quiet", "Loading channels" } } };
        }
        Some(Err(e)) => {
            return rsx! { div { class: "page page-top", LoadError { error: describe(e) } } };
        }
        Some(Ok(list)) => list.clone(),
    };
    if list.is_empty() {
        return rsx! {
            div { class: "page page-top",
                header { class: "page-head", h1 { "Live TV" } }
                p { class: "empty", "No channels yet. In Manage on the web, add your M3U playlist or Xtream login." }
            }
        };
    }
    // The ungrouped are asked for with an empty group name.
    let key = |name: &Option<String>| name.clone().unwrap_or_default();
    let selected = chosen().unwrap_or_else(|| key(&list[0].name));
    let total: i64 = list.iter().map(|g| g.channels).sum();
    rsx! {
        div { class: "page page-top live",
            header { class: "page-head",
                h1 { "Live TV" }
                p { class: "quiet", "{total} channels" }
            }
            div { class: "live-body",
                nav { class: "live-groups", "aria-label": "Channel groups",
                    for g in list {
                        button {
                            key: "{key(&g.name)}",
                            class: if key(&g.name) == selected { "live-group current" } else { "live-group" },
                            "aria-current": key(&g.name) == selected,
                            onclick: {
                                let k = key(&g.name);
                                move |_| chosen.set(Some(k.clone()))
                            },
                            // Resting on a group shows its channels, as on a TV guide; passing
                            // over it on the way elsewhere does not load anything.
                            onfocus: {
                                let k = key(&g.name);
                                move |_| {
                                    focused += 1;
                                    let mine = focused();
                                    let k = k.clone();
                                    spawn(async move {
                                        futures_timer::Delay::new(Duration::from_millis(300)).await;
                                        if focused() == mine {
                                            chosen.set(Some(k));
                                        }
                                    });
                                }
                            },
                            span { class: "live-group-name", "{group_label(&g.name)}" }
                            span { class: "live-group-count", "{g.channels}" }
                        }
                    }
                }
                GroupGuide { key: "{selected}", group: selected.clone(), time }
            }
        }
    }
}

#[component]
fn GroupGuide(group: String, time: Signal<Clock>) -> Element {
    // Reactive on `group`: a key on a lone child does not remount it, so the resource must
    // re-run itself when another group is chosen.
    let guide = use_resource(use_reactive!(|group| async move {
        client().guide(&group).await
    }));
    match &*guide.read() {
        None => rsx! { p { class: "quiet live-channels", "Loading the guide" } },
        Some(Err(e)) => rsx! { div { class: "live-channels", LoadError { error: describe(e) } } },
        Some(Ok(channels)) => rsx! {
            ol { class: "live-channels",
                for c in channels.clone() {
                    ChannelRow { key: "{c.channel.id}", entry: c, group: group.clone(), time: time() }
                }
            }
        },
    }
}

#[component]
fn ChannelRow(entry: GuideChannel, group: String, time: Clock) -> Element {
    let c = &entry.channel;
    let (current, next) = now_next(&entry.programmes, time.now);
    let number = c.number.map(|n| n.to_string()).unwrap_or_default();
    rsx! {
        li {
            Link { class: "channel", to: Route::Watch { id: c.id, group },
                span { class: "channel-number", "{number}" }
                span { class: "channel-logo",
                    if let Some(url) = c.logo_url.clone() {
                        img { src: url, alt: "", loading: "lazy" }
                    }
                }
                span { class: "channel-text",
                    span { class: "channel-name", "{c.name}" }
                    if let Some(p) = current {
                        span { class: "channel-now", "{p.title}" }
                        span { class: "channel-meter",
                            span { class: "progress-track",
                                span {
                                    class: "progress-fill",
                                    style: "width: {fraction(p, time.now) * 100.0}%",
                                }
                            }
                            span { class: "quiet", "{left(time.now, p.stop)}" }
                        }
                    }
                }
                if let Some(n) = next.filter(|_| time.now > 0) {
                    span { class: "channel-next quiet",
                        span { class: "channel-next-time", "{clock(n.start, time.offset_min)}" }
                        span { "{n.title}" }
                    }
                }
            }
        }
    }
}

fn fraction(p: &Programme, now: i64) -> f64 {
    let span = (p.stop - p.start).max(1) as f64;
    ((now - p.start) as f64 / span).clamp(0.0, 1.0)
}

/// Full-screen live channel. Each channel gets a fresh `Tuned` so changing channel restarts it.
#[component]
pub fn Watch(id: i64, group: String) -> Element {
    rsx! { Tuned { key: "{id}", id, group } }
}

/// Polls of 250 ms before the channel banner fades (4 s).
const BANNER_FOR: u32 = 16;

#[component]
fn Tuned(id: i64, group: String) -> Element {
    let player = use_hook(|| Rc::new(Player::new()));
    let mut error = use_signal(|| None::<String>);
    let mut shown = use_signal(|| 0u32);
    let time = use_clock();
    let nav = navigator();
    let g = group.clone();
    let guide = use_resource(move || {
        let g = g.clone();
        async move { client().guide(&g).await.unwrap_or_default() }
    });

    let p = player.clone();
    use_future(move || {
        let p = p.clone();
        async move {
            let c = client();
            match c.play_channel(id).await {
                Ok(plan) => {
                    p.load(&c.media_url(&plan.url));
                    p.play();
                }
                Err(e) => return error.set(Some(describe(&e))),
            }
            loop {
                futures_timer::Delay::new(Duration::from_millis(250)).await;
                shown += 1;
                if let Some(e) = p.error() {
                    error.set(Some(e));
                }
            }
        }
    });

    let channels = guide.read().clone().unwrap_or_default();
    let at = channels.iter().position(|c| c.channel.id == id);
    let zap = {
        let channels = channels.clone();
        let p = player.clone();
        move |step: isize| {
            let (Some(i), n) = (at, channels.len() as isize) else {
                return;
            };
            if n == 0 {
                return;
            }
            let to = channels[(i as isize + step).rem_euclid(n) as usize]
                .channel
                .id;
            p.pause();
            nav.replace(Route::Watch {
                id: to,
                group: group.clone(),
            });
        }
    };
    let leave = {
        let p = player.clone();
        move || {
            p.pause();
            nav.go_back();
        }
    };
    crate::tv::use_back({
        let leave = leave.clone();
        move || {
            if shown() < BANNER_FOR {
                shown.set(BANNER_FOR);
            } else {
                leave()
            }
        }
    });
    let on_key = move |e: KeyboardEvent| match e.key() {
        Key::ArrowUp | Key::ChannelUp => {
            e.prevent_default();
            zap(-1);
        }
        Key::ArrowDown | Key::ChannelDown => {
            e.prevent_default();
            zap(1);
        }
        Key::Escape | Key::GoBack | Key::BrowserBack => leave(),
        _ => shown.set(0),
    };

    let entry = at.map(|i| channels[i].clone());
    let (current, next) = entry
        .as_ref()
        .map(|e| now_next(&e.programmes, time().now))
        .unwrap_or((None, None));
    let banner = shown() < BANNER_FOR || error().is_some();
    rsx! {
        div {
            class: if banner { "player live-player" } else { "player live-player idle" },
            "data-rh-keys": "true",
            tabindex: "0",
            autofocus: true,
            onkeydown: on_key,
            onmousemove: move |_| shown.set(0),
            {crate::player::surface()}
            div { class: "live-banner",
                if let Some(e) = &entry {
                    div { class: "live-banner-channel",
                        if let Some(n) = e.channel.number {
                            span { class: "channel-number", "{n}" }
                        }
                        span { class: "channel-logo",
                            if let Some(url) = e.channel.logo_url.clone() {
                                img { src: url, alt: "" }
                            }
                        }
                        span { class: "live-banner-name", "{e.channel.name}" }
                    }
                }
                if let Some(p) = current {
                    p { class: "live-banner-now", "{p.title}" }
                    div { class: "channel-meter",
                        span { class: "progress-track",
                            span { class: "progress-fill", style: "width: {fraction(p, time().now) * 100.0}%" }
                        }
                        span { "{clock(p.start, time().offset_min)} to {clock(p.stop, time().offset_min)}" }
                    }
                }
                if let Some(n) = next {
                    p { class: "quiet", "Next at {clock(n.start, time().offset_min)}: {n.title}" }
                }
                if let Some(msg) = error() {
                    p { class: "notice", "This channel isn't playing: {msg}" }
                }
                p { class: "quiet live-banner-hint", "Up and Down change channel" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(id: i64, start: i64, stop: i64) -> Programme {
        Programme {
            id,
            channel_id: 1,
            start,
            stop,
            title: format!("p{id}"),
            subtitle: None,
            description: None,
        }
    }

    #[test]
    fn now_and_next() {
        let list = [prog(1, 0, 100), prog(2, 100, 200), prog(3, 200, 300)];
        let (now, next) = now_next(&list, 150);
        assert_eq!((now.map(|p| p.id), next.map(|p| p.id)), (Some(2), Some(3)));
        // In a gap there is nothing on now, and next is the coming programme.
        let gap = [prog(1, 0, 100), prog(2, 200, 300)];
        let (now, next) = now_next(&gap, 150);
        assert_eq!((now.map(|p| p.id), next.map(|p| p.id)), (None, Some(2)));
    }
}
