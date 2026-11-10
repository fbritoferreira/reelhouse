//! Live TV: channel groups beside the selected group's channels with what is on now and next,
//! and a full-screen player that changes channel with Up and Down.

use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;
use reelhouse_api::{ChannelGroup, GuideChannel, Programme};
use reelhouse_core::timefmt::{clock, left};

use crate::Route;
use crate::live_cache::{LIVE, Place, store_groups, store_guide};
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

/// How the group list is ordered; remembered on this device.
#[derive(Clone, Copy, PartialEq)]
enum GroupSort {
    /// As the provider lists them.
    Provider,
    Name,
    Size,
}

const SORT_KEY: &str = "reelhouse.live.sort";

impl GroupSort {
    const ALL: [GroupSort; 3] = [GroupSort::Provider, GroupSort::Name, GroupSort::Size];

    fn label(self) -> &'static str {
        match self {
            GroupSort::Provider => "Provider",
            GroupSort::Name => "A to Z",
            GroupSort::Size => "Biggest",
        }
    }

    fn id(self) -> &'static str {
        match self {
            GroupSort::Provider => "provider",
            GroupSort::Name => "name",
            GroupSort::Size => "size",
        }
    }

    fn saved() -> Self {
        let saved = crate::session::load(SORT_KEY);
        Self::ALL
            .into_iter()
            .find(|s| Some(s.id()) == saved.as_deref())
            .unwrap_or(GroupSort::Provider)
    }
}

/// Orders `groups` (which arrive in provider order); the ungrouped stay last.
fn sort_groups(mut groups: Vec<ChannelGroup>, by: GroupSort) -> Vec<ChannelGroup> {
    match by {
        GroupSort::Provider => {}
        GroupSort::Name => groups.sort_by_key(|g| {
            (
                g.name.is_none(),
                g.name.clone().unwrap_or_default().to_lowercase(),
            )
        }),
        GroupSort::Size => groups.sort_by_key(|g| (g.name.is_none(), -g.channels)),
    }
    groups
}

/// Groups that share a prefix (`Germany - RTL+`, `IT | Crime`) shown together under it.
/// `name` is `None` for a group standing alone.
#[derive(Clone, PartialEq)]
struct Family {
    name: Option<String>,
    /// Each group with the label shown inside its family.
    groups: Vec<(ChannelGroup, String)>,
}

/// Splits `Germany - RTL+` into `("Germany", "RTL+")`.
fn split_prefix(name: &str) -> Option<(&str, &str)> {
    [" - ", " | ", ": "].iter().find_map(|sep| {
        let (head, tail) = name.split_once(sep)?;
        (!head.trim().is_empty() && !tail.trim().is_empty()).then(|| (head.trim(), tail.trim()))
    })
}

/// Gathers groups into families in order of first appearance, so any sort carries over.
fn families(groups: &[ChannelGroup]) -> Vec<Family> {
    fn prefix_of(g: &ChannelGroup) -> Option<&str> {
        g.name.as_deref().and_then(split_prefix).map(|(p, _)| p)
    }
    let mut out: Vec<Family> = Vec::new();
    for g in groups {
        let shared =
            prefix_of(g).filter(|p| groups.iter().filter(|o| prefix_of(o) == Some(*p)).count() > 1);
        match shared {
            Some(p) => {
                let short = split_prefix(g.name.as_deref().unwrap_or_default())
                    .map_or("", |(_, s)| s)
                    .to_string();
                match out.iter_mut().find(|f| f.name.as_deref() == Some(p)) {
                    Some(f) => f.groups.push((g.clone(), short)),
                    None => out.push(Family {
                        name: Some(p.to_string()),
                        groups: vec![(g.clone(), short)],
                    }),
                }
            }
            None => out.push(Family {
                name: None,
                groups: vec![(g.clone(), group_label(&g.name))],
            }),
        }
    }
    out
}

fn group_label(name: &Option<String>) -> String {
    name.clone().unwrap_or_else(|| "Other channels".into())
}

#[component]
pub fn LiveTv() -> Element {
    // Groups come from the cache at once; a stale or missing list is fetched behind it.
    let mut failed = use_signal(|| None::<String>);
    use_future(move || async move {
        if LIVE.peek().groups.as_ref().is_some_and(|g| g.fresh()) {
            return;
        }
        match client().channel_groups().await {
            Ok(g) => store_groups(g),
            Err(e) => failed.set(Some(describe(&e))),
        }
    });
    let mut sort = use_signal(GroupSort::saved);
    // Countries opened by hand; the one holding the current group is always open.
    let mut opened = use_signal(std::collections::HashSet::<String>::new);
    // Counts focus moves, so a pending group switch can tell it has been overtaken.
    let mut focused = use_signal(|| 0u64);
    let time = use_clock();
    let cached = LIVE.read().groups.as_ref().map(|g| g.value.clone());
    let list = match (cached, failed()) {
        (Some(list), _) => sort_groups(list, sort()),
        (None, Some(e)) => {
            return rsx! { div { class: "page page-top", LoadError { error: e } } };
        }
        (None, None) => return rsx! { LiveSkeleton {} },
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
    let remembered = LIVE.read().place.group.clone();
    let selected = remembered
        .filter(|g| list.iter().any(|l| key(&l.name) == *g))
        .unwrap_or_else(|| key(&list[0].name));
    // Choosing a group starts it from the top.
    let choose = |k: String| {
        let mut live = LIVE.write();
        if live.place.group.as_deref() != Some(k.as_str()) {
            live.place = Place {
                group: Some(k),
                spotlight: 0,
                shown: PAGE,
                returning: false,
            };
        }
    };
    let current = list.iter().find(|g| key(&g.name) == selected).cloned();
    let fams = families(&list);
    let pick = move |k: String| {
        move |_| {
            focused += 1;
            let mine = focused();
            let k = k.clone();
            // Resting on a group shows its channels, as on a TV guide; passing over it on the
            // way elsewhere loads nothing.
            spawn(async move {
                futures_timer::Delay::new(Duration::from_millis(300)).await;
                if focused() == mine {
                    choose(k);
                }
            });
        }
    };
    rsx! {
        div { class: "page page-top live",
            div { class: "live-body",
                aside { class: "live-groups",
                    h1 { class: "live-title", "Live TV" }
                    div { class: "live-sort", role: "radiogroup", "aria-label": "Sort groups",
                        for by in GroupSort::ALL {
                            button {
                                key: "{by.id()}",
                                role: "radio",
                                class: if sort() == by { "live-sort-option current" } else { "live-sort-option" },
                                "aria-checked": sort() == by,
                                onclick: move |_| {
                                    sort.set(by);
                                    crate::session::save(SORT_KEY, Some(by.id()));
                                },
                                "{by.label()}"
                            }
                        }
                    }
                    nav { class: "live-group-list", "aria-label": "Channel groups",
                        for fam in fams {
                            match fam.name.clone() {
                                None => {
                                    let (g, label) = fam.groups[0].clone();
                                    let k = key(&g.name);
                                    rsx! {
                                        GroupButton {
                                            key: "{k}",
                                            label,
                                            channels: g.channels,
                                            current: k == selected,
                                            on_pick: pick(k.clone()),
                                            on_choose: move |_| choose(k.clone()),
                                        }
                                    }
                                }
                                Some(country) => {
                                    let holds_current = fam.groups.iter().any(|(g, _)| key(&g.name) == selected);
                                    let open = holds_current || opened().contains(&country);
                                    let total: i64 = fam.groups.iter().map(|(g, _)| g.channels).sum();
                                    let c2 = country.clone();
                                    rsx! {
                                        div { key: "{country}", class: if open { "live-family open" } else { "live-family" },
                                            button {
                                                class: "live-family-head",
                                                "aria-expanded": open,
                                                onclick: move |_| {
                                                    let mut o = opened();
                                                    if !o.remove(&c2) {
                                                        o.insert(c2.clone());
                                                    }
                                                    opened.set(o);
                                                },
                                                span { class: "live-family-chevron", "aria-hidden": "true" }
                                                span { class: "live-group-name", "{country}" }
                                                span { class: "live-group-count", "{total}" }
                                            }
                                            if open {
                                                for (g, label) in fam.groups.clone() {
                                                    {
                                                        let k = key(&g.name);
                                                        rsx! {
                                                            GroupButton {
                                                                key: "{k}",
                                                                label,
                                                                channels: g.channels,
                                                                current: k == selected,
                                                                nested: true,
                                                                on_pick: pick(k.clone()),
                                                                on_choose: move |_| choose(k.clone()),
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
                }
                section { class: "live-main",
                    if let Some(g) = current {
                        {
                            // The country is in the sidebar; the heading names the group itself.
                            let label = group_label(&g.name);
                            let (country, short) = match split_prefix(&label) {
                                Some((c, s)) => (Some(c.to_string()), s.to_string()),
                                None => (None, label.clone()),
                            };
                            let count = if g.channels == 1 { "1 channel".to_string() } else { format!("{} channels", g.channels) };
                            let detail = match country {
                                Some(c) => format!("{c}, {count}"),
                                None => count,
                            };
                            rsx! {
                                header { class: "live-main-head",
                                    h2 { "{short}" }
                                    span { class: "quiet", "{detail}" }
                                }
                            }
                        }
                    }
                    GroupGuide { key: "{selected}", group: selected.clone(), time }
                }
            }
        }
    }
}

#[component]
fn GroupButton(
    label: String,
    channels: i64,
    current: bool,
    #[props(default)] nested: bool,
    on_pick: EventHandler<FocusEvent>,
    on_choose: EventHandler<MouseEvent>,
) -> Element {
    let class = match (current, nested) {
        (true, true) => "live-group nested current",
        (true, false) => "live-group current",
        (false, true) => "live-group nested",
        (false, false) => "live-group",
    };
    rsx! {
        button {
            class,
            "aria-current": current,
            onclick: move |e| on_choose.call(e),
            onfocus: move |e| on_pick.call(e),
            span { class: "live-group-name", "{label}" }
            span { class: "live-group-count", "{channels}" }
        }
    }
}

/// The page's shape while the groups load, so nothing jumps when they arrive.
#[component]
fn LiveSkeleton() -> Element {
    rsx! {
        div { class: "page page-top live", "aria-busy": "true",
            div { class: "live-body",
                aside { class: "live-groups",
                    h1 { class: "live-title", "Live TV" }
                    for i in 0..9 {
                        span { key: "{i}", class: "skeleton skeleton-line" }
                    }
                }
                section { class: "live-main",
                    span { class: "skeleton skeleton-panel" }
                    for i in 0..6 {
                        span { key: "{i}", class: "skeleton skeleton-row" }
                    }
                }
            }
        }
    }
}

#[component]
fn GroupGuide(group: String, time: Signal<Clock>) -> Element {
    let mut failed = use_signal(|| None::<String>);
    // Cached guides show at once; a stale or missing one is fetched behind it.
    use_effect(use_reactive!(|group| {
        failed.set(None);
        if LIVE.peek().guides.get(&group).is_some_and(|g| g.fresh()) {
            return;
        }
        spawn(async move {
            match client().guide(&group).await {
                Ok(g) => store_guide(&group, g),
                Err(e) => failed.set(Some(describe(&e))),
            }
        });
    }));
    let guide = LIVE.read().guides.get(&group).map(|g| g.value.clone());
    let place = LIVE.read().place.clone();
    let shown = place.shown.max(PAGE);
    let spotlight = place.spotlight;
    // Back from a channel: focus its row again, where it was.
    let returning = place.returning && guide.is_some();
    use_effect(move || {
        if returning {
            LIVE.write().place.returning = false;
            spawn(async move {
                futures_timer::Delay::new(Duration::from_millis(60)).await;
                let _ = document::eval(
                    "const r = document.querySelector('.channel.lit'); \
                     if (r) { r.focus({ preventScroll: true }); r.scrollIntoView({ block: 'nearest' }); }",
                );
            });
        }
    });
    let channels = match (guide, failed()) {
        (Some(c), _) => c,
        (None, Some(e)) => return rsx! { div { class: "live-channels", LoadError { error: e } } },
        (None, None) => {
            return rsx! {
                div { class: "live-loading", "aria-busy": "true",
                    span { class: "skeleton skeleton-panel" }
                    for i in 0..6 {
                        span { key: "{i}", class: "skeleton skeleton-row" }
                    }
                }
            };
        }
    };
    let total = channels.len();
    let limit = shown.min(total);
    let lit = channels
        .get(spotlight.min(total.saturating_sub(1)))
        .cloned();
    rsx! {
        if let Some(entry) = lit {
            OnAir { entry, time: time() }
        }
        ol { class: "live-channels",
            for (i, c) in channels[..limit].iter().cloned().enumerate() {
                ChannelRow {
                    key: "{c.channel.id}",
                    entry: c,
                    group: group.clone(),
                    time: time(),
                    lit: i == spotlight,
                    on_focus: move |_| {
                        let mut live = LIVE.write();
                        live.place.spotlight = i;
                        if i + 5 >= live.place.shown.max(PAGE) {
                            live.place.shown = live.place.shown.max(PAGE) + PAGE;
                        }
                    },
                }
            }
            if limit < total {
                li {
                    button {
                        class: "secondary live-more",
                        onclick: move |_| {
                            let mut live = LIVE.write();
                            live.place.shown = live.place.shown.max(PAGE) + PAGE;
                        },
                        "Show {(total - limit).min(PAGE)} more of {total - limit}"
                    }
                }
            }
        }
    }
}

/// Channel rows drawn at a time.
const PAGE: usize = 40;

/// Two letters for a channel without a logo: `UK: BBC One` gives `BO`.
fn monogram(name: &str) -> String {
    let name = name.split_once(": ").map_or(name, |(_, rest)| rest);
    name.split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

#[component]
fn Logo(name: String, url: Option<String>) -> Element {
    rsx! {
        span { class: "channel-logo",
            if let Some(url) = url {
                img { src: url, alt: "", loading: "lazy", decoding: "async" }
            } else {
                span { class: "channel-monogram", "{monogram(&name)}" }
            }
        }
    }
}

/// The channel in focus, large: what is on now and next. The marquee lamp marks it as live.
#[component]
fn OnAir(entry: GuideChannel, time: Clock) -> Element {
    let c = &entry.channel;
    let (current, next) = now_next(&entry.programmes, time.now);
    rsx! {
        div { class: "on-air",
            Logo { name: c.name.clone(), url: c.logo_url.clone() }
            div { class: "on-air-text",
                p { class: "on-air-channel",
                    if let Some(n) = c.number {
                        span { class: "channel-number", "{n}" }
                    }
                    span { "{c.name}" }
                }
                match current {
                    Some(p) => rsx! {
                        p { class: "on-air-now",
                            span { class: "lamp", "aria-label": "On now" }
                            "{p.title}"
                        }
                        div { class: "channel-meter",
                            span { class: "progress-track",
                                span { class: "progress-fill", style: "width: {fraction(p, time.now) * 100.0}%" }
                            }
                            span { class: "quiet", "{left(time.now, p.stop)}" }
                        }
                        if let Some(d) = p.description.clone() {
                            p { class: "on-air-desc", "{d}" }
                        }
                    },
                    None => rsx! {
                        p { class: "on-air-now",
                            span { class: "lamp", "aria-label": "Live" }
                            "Live now"
                        }
                        p { class: "quiet on-air-desc", "This provider sends no guide for this channel." }
                    },
                }
                if let Some(n) = next.filter(|_| time.now > 0) {
                    p { class: "quiet", "Next at {clock(n.start, time.offset_min)}: {n.title}" }
                }
            }
        }
    }
}

#[component]
fn ChannelRow(
    entry: GuideChannel,
    group: String,
    time: Clock,
    lit: bool,
    on_focus: EventHandler,
) -> Element {
    let c = &entry.channel;
    let (current, next) = now_next(&entry.programmes, time.now);
    let number = c.number.map(|n| n.to_string()).unwrap_or_default();
    rsx! {
        li { onfocusin: move |_| on_focus.call(()), onmouseenter: move |_| on_focus.call(()),
            Link {
                class: if lit { "channel lit" } else { "channel" },
                to: Route::Watch { id: c.id, group },
                onclick: move |_| LIVE.write().place.returning = true,
                span { class: "channel-number", "{number}" }
                Logo { name: c.name.clone(), url: c.logo_url.clone() }
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
    // The group's guide, from the cache when the list was open a moment ago.
    let g = group.clone();
    let guide = use_resource(move || {
        let g = g.clone();
        async move {
            if let Some(c) = LIVE.peek().guides.get(&g).filter(|c| c.fresh()) {
                return c.value.clone();
            }
            match client().guide(&g).await {
                Ok(list) => {
                    store_guide(&g, list.clone());
                    list
                }
                Err(_) => Vec::new(),
            }
        }
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

    fn group(name: Option<&str>, channels: i64) -> ChannelGroup {
        ChannelGroup {
            name: name.map(str::to_string),
            channels,
        }
    }

    #[test]
    fn sorts_groups_keeping_the_ungrouped_last() {
        let groups = vec![
            group(Some("UK"), 5),
            group(None, 50),
            group(Some("africa"), 9),
        ];
        let names = |by| {
            sort_groups(groups.clone(), by)
                .into_iter()
                .map(|g| g.name.unwrap_or_default())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(GroupSort::Provider), ["UK", "", "africa"]);
        assert_eq!(names(GroupSort::Name), ["africa", "UK", ""]);
        assert_eq!(names(GroupSort::Size), ["africa", "UK", ""]);
    }

    #[test]
    fn groups_gather_under_their_country() {
        let groups = vec![
            group(Some("Germany - RTL+"), 40),
            group(Some("Sport"), 9),
            group(Some("Germany - MotoGP"), 14),
            group(Some("IT | Crime"), 3),
            group(Some("IT | Anime"), 2),
            group(Some("UK - Only one"), 5),
            group(None, 1),
        ];
        let fams = families(&groups);
        let shape: Vec<(Option<&str>, Vec<&str>)> = fams
            .iter()
            .map(|f| {
                (
                    f.name.as_deref(),
                    f.groups.iter().map(|(_, short)| short.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (Some("Germany"), vec!["RTL+", "MotoGP"]),
                (None, vec!["Sport"]),
                (Some("IT"), vec!["Crime", "Anime"]),
                // A prefix used once is not a family; the group keeps its full name.
                (None, vec!["UK - Only one"]),
                (None, vec!["Other channels"]),
            ]
        );
    }

    #[test]
    fn monograms_skip_the_country_prefix() {
        assert_eq!(monogram("UK: BBC One"), "BO");
        assert_eq!(monogram("Eurosport 1"), "E1");
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
