use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;
use reelhouse_core::timefmt::{format_time, needs_restart, seek_fraction};

use crate::Route;
use crate::player::{Player, capabilities, stereo_capabilities};
use crate::session::client;
use crate::widgets::describe;

const SKIP_SECONDS: f64 = 10.0;
/// The arrow keys skip less on a keyboard, as on YouTube; the remote's arrows keep the long skip.
const ARROW_SECONDS: f64 = if cfg!(target_arch = "wasm32") {
    5.0
} else {
    SKIP_SECONDS
};
const REPORT_EVERY: u32 = 40; // polls of 250 ms = 10 s
/// Controls fade out after this many quiet polls of 250 ms while playing (3 s).
const HIDE_AFTER: u32 = 12;
/// The next episode starts this many polls after one ends (10 s).
const AUTOPLAY_POLLS: u32 = 40;

/// A library title.
#[component]
pub fn Play(id: i64) -> Element {
    // A one-item keyed list, so going to another title (Up next) starts a fresh player rather
    // than keeping this one's stream and heading: keys only remount among list siblings.
    rsx! {
        for id in [id] {
            PlayerScreen { key: "{id}", id, on_demand: false }
        }
    }
}

/// A film or episode a Live TV source offers on demand.
#[component]
pub fn PlayVod(id: i64) -> Element {
    rsx! {
        for id in [id] {
            PlayerScreen { key: "{id}", id, on_demand: true }
        }
    }
}

/// Attaches a library title's text subtitles, timed for a stream that begins `start_s` in.
async fn attach_subtitles(c: &reelhouse_core::Client, p: &Player, id: i64, start_s: f64) {
    let Ok(list) = c.subtitles(id).await else {
        return;
    };
    let tracks: Vec<(String, String, String)> = list
        .iter()
        .map(|t| {
            let lang = t.language.clone().unwrap_or_default();
            let label = t
                .title
                .clone()
                .or_else(|| crate::player::language_name(&lang).map(str::to_string))
                .unwrap_or_else(|| format!("Subtitles {}", t.index + 1));
            let url = c.media_url(&format!(
                "/api/v1/items/{id}/subtitles/{}?start={start_s:.3}",
                t.index
            ));
            (label, lang, url)
        })
        .collect();
    p.set_subtitles(&tracks);
}

/// How to play `id` from `start_s` seconds in.
/// `stereo` asks for sound every output takes (see [`stereo_capabilities`]).
async fn plan_for(
    c: &reelhouse_core::Client,
    id: i64,
    on_demand: bool,
    start_s: f64,
    stereo: bool,
) -> Result<reelhouse_api::PlaybackPlan, reelhouse_core::ClientError> {
    let caps = if stereo {
        stereo_capabilities()
    } else {
        capabilities()
    };
    if on_demand {
        c.vod_playback(id, &caps, start_s).await
    } else {
        c.playback(id, &caps, start_s).await
    }
}

#[component]
fn PlayerScreen(id: i64, on_demand: bool) -> Element {
    let player = use_hook(|| Rc::new(Player::new()));
    let mut error = use_signal(|| None::<String>);
    let mut position = use_signal(|| 0.0);
    let mut duration = use_signal(|| 0.0);
    let mut playing = use_signal(|| false);
    // Polls since the last key press; controls show while this is low or while paused.
    let mut quiet = use_signal(|| 0u32);
    // Back hides the controls at once, without the fade.
    let mut instant = use_signal(|| false);
    // Where in the title the current stream begins. A live HLS conversion counts its own clock
    // from here, and can only seek within what it has converted so far.
    let mut offset = use_signal(|| 0.0);
    let mut converting = use_signal(|| false);
    // Set after a playback failure: the title is retried once with stereo sound.
    let mut stereo = use_signal(|| false);
    // What is playing, for the top of the screen: the show or film, then the episode.
    let mut heading = use_signal(|| None::<(String, String)>);
    // The episode after this one, offered as the credits roll.
    let mut up_next = use_signal(|| None::<(Route, String)>);
    // The title's resolution, shown beside what is playing.
    let mut badge = use_signal(|| None::<&'static str>);
    // What the last key changed ("Volume 60%"), shown until the controls fade.
    let mut hint = use_signal(|| None::<String>);
    // Polls left before the next episode starts by itself, once this one has ended.
    let mut countdown = use_signal(|| None::<u32>);
    let mut autoplay_off = use_signal(|| false);
    let nav = navigator();

    let p = player.clone();
    use_future(move || {
        let p = p.clone();
        async move {
            let c = client();
            // On-demand titles are kept on the device under their negated id.
            let local = downloaded(if on_demand { -id } else { id });
            // The server knows the latest position; a downloaded copy plays without it. On-demand
            // titles keep no position yet.
            let (mut known, resume_at) = if on_demand {
                match c.vod_title(id).await {
                    Ok(t) => {
                        let code = match (t.season_no, t.episode_no) {
                            (Some(s), Some(e)) => format!("S{s} E{e}  {}", t.title),
                            _ => String::new(),
                        };
                        heading.set(Some((
                            t.show_title.clone().unwrap_or(t.title.clone()),
                            code,
                        )));
                        if let Some(show) = t.show_title.clone() {
                            let eps = c.vod_episodes(&show).await.unwrap_or_default();
                            if let Some(n) = eps
                                .iter()
                                .position(|e| e.id == id)
                                .and_then(|i| eps.get(i + 1))
                            {
                                let label = match (n.season_no, n.episode_no) {
                                    (Some(s), Some(e)) => format!("S{s} E{e}  {}", n.title),
                                    _ => n.title.clone(),
                                };
                                up_next.set(Some((Route::PlayVod { id: n.id }, label)));
                            }
                        }
                        (
                            t.duration_s.unwrap_or(0.0),
                            crate::unsent::position(id, true)
                                .or_else(|| t.progress.filter(|p| !p.played).map(|p| p.position_s))
                                .unwrap_or(0.0),
                        )
                    }
                    Err(_) => (0.0, crate::unsent::position(id, true).unwrap_or(0.0)),
                }
            } else {
                let item = match (c.item(id).await, &local) {
                    (Ok(i), _) => i,
                    (Err(_), Some((_, saved))) => saved.clone(),
                    (Err(e), None) => return error.set(Some(describe(&e))),
                };
                // A live-converted HLS stream only reports what is converted so far; the scan knows
                // the full length.
                // A position watched offline and not yet sent is newer than the server's.
                let resume = crate::unsent::position(id, false)
                    .or_else(|| {
                        item.progress
                            .as_ref()
                            .filter(|pr| !pr.played)
                            .map(|pr| pr.position_s)
                    })
                    .unwrap_or(0.0);
                badge.set(crate::widgets::quality(item.height));
                heading.set(Some(
                    match (&item.show_title, item.season_no, item.episode_no) {
                        (Some(show), Some(s), Some(e)) => (
                            show.clone(),
                            format!("S{s} E{e}  {}", crate::widgets::episode_name(&item)),
                        ),
                        _ => (
                            item.title.clone(),
                            item.year.map(|y| y.to_string()).unwrap_or_default(),
                        ),
                    },
                ));
                if let (reelhouse_api::ItemKind::Episode, Some(season)) =
                    (item.kind, item.parent_id)
                {
                    let eps = c.children(season).await.unwrap_or_default();
                    if let Some(n) = eps
                        .iter()
                        .position(|e| e.id == id)
                        .and_then(|i| eps.get(i + 1))
                    {
                        let label = match (n.season_no, n.episode_no) {
                            (Some(s), Some(e)) => {
                                format!("S{s} E{e}  {}", crate::widgets::episode_name(n))
                            }
                            _ => n.title.clone(),
                        };
                        up_next.set(Some((Route::Play { id: n.id }, label)));
                    }
                }
                (item.duration_s.unwrap_or(0.0), resume)
            };
            match &local {
                Some((file, _)) => {
                    p.load(file);
                    p.seek(resume_at);
                }
                None => {
                    // A conversion starts where we resume; a direct file seeks there itself.
                    let plan = match plan_for(&c, id, on_demand, resume_at, false).await {
                        Ok(plan) => plan,
                        Err(e) => return error.set(Some(describe(&e))),
                    };
                    if let Some(d) = plan.duration_s {
                        known = known.max(d);
                    }
                    converting.set(plan.method != reelhouse_api::PlaybackMethod::Direct);
                    offset.set(plan.start_s);
                    p.load(&c.media_url(&plan.url));
                    if !on_demand {
                        attach_subtitles(&c, &p, id, plan.start_s).await;
                    }
                    if plan.start_s == 0.0 {
                        p.seek(resume_at);
                    }
                }
            }
            p.play();
            let mut ticks = 0;
            loop {
                futures_timer::Delay::new(Duration::from_millis(250)).await;
                position.set(offset() + p.position());
                duration.set((offset() + p.duration()).max(known));
                playing.set(p.is_playing());
                if p.is_playing() {
                    quiet += 1;
                    if quiet() == HIDE_AFTER {
                        hint.set(None);
                    }
                }
                if let Some(e) = p.error() {
                    if stereo() || local.is_some() {
                        error.set(Some(e));
                    } else {
                        // Often the sound output, not the file: try once more with stereo sound,
                        // from where it stopped.
                        stereo.set(true);
                        let at = offset() + p.position();
                        match plan_for(&c, id, on_demand, at, true).await {
                            Ok(plan) => {
                                converting
                                    .set(plan.method != reelhouse_api::PlaybackMethod::Direct);
                                offset.set(plan.start_s);
                                p.load(&c.media_url(&plan.url));
                                if !on_demand {
                                    attach_subtitles(&c, &p, id, plan.start_s).await;
                                }
                                if plan.start_s == 0.0 {
                                    p.seek(at);
                                }
                                p.play();
                            }
                            Err(e2) => error.set(Some(describe(&e2))),
                        }
                    }
                }
                // Ended with an episode after it: count down, then play it, as a TV box would.
                let ended = duration() > 0.0 && duration() - position() < 1.5 && !p.is_playing();
                if let Some((next, _)) = up_next().filter(|_| ended && !autoplay_off()) {
                    match countdown() {
                        None => countdown.set(Some(AUTOPLAY_POLLS)),
                        Some(0) => {
                            crate::unsent::save(&c, id, on_demand, duration()).await;
                            nav.push(next);
                            return;
                        }
                        Some(n) => countdown.set(Some(n - 1)),
                    }
                } else if countdown().is_some() {
                    countdown.set(None);
                }
                ticks += 1;
                if ticks % REPORT_EVERY == 0 && p.is_playing() {
                    crate::unsent::save(&c, id, on_demand, offset() + p.position()).await;
                }
            }
        }
    });

    let save_and_leave = {
        let p = player.clone();
        move || {
            let pos = offset() + p.position();
            p.pause();
            spawn(async move { crate::unsent::save(&client(), id, on_demand, pos).await });
            nav.go_back();
        }
    };
    // The audio and subtitle picker, and what it offers (read from the player while open).
    let mut picking = use_signal(|| false);
    let mut tracks = use_signal(Vec::<crate::player::Track>::new);
    {
        let p = player.clone();
        use_future(move || {
            let p = p.clone();
            async move {
                loop {
                    futures_timer::Delay::new(Duration::from_millis(500)).await;
                    // Read while the picker is open, and now and then otherwise, so the button
                    // shows once a title turns out to have choices.
                    if picking() || tracks.peek().is_empty() {
                        let now = p.tracks();
                        if *tracks.peek() != now {
                            tracks.set(now);
                        }
                    }
                }
            }
        });
    }
    let controls_shown =
        move || picking() || !playing() || quiet() < HIDE_AFTER || error().is_some();
    // The remote's Back first hides the controls; with them hidden it saves and returns.
    crate::tv::use_back({
        let leave = save_and_leave.clone();
        move || {
            if picking() {
                picking.set(false);
            } else if controls_shown() && playing() {
                instant.set(true);
                quiet.set(HIDE_AFTER);
            } else {
                leave()
            }
        }
    });
    // Seeks to `target` seconds into the title. Past what a conversion has reached, it starts a
    // fresh conversion there.
    let go_to = {
        let p = player.clone();
        move |target: f64| {
            if !converting() || !needs_restart(target, offset(), p.duration()) {
                p.seek(target - offset());
                return;
            }
            let p = p.clone();
            position.set(target);
            spawn(async move {
                let c = client();
                match plan_for(&c, id, on_demand, target, stereo()).await {
                    Ok(plan) => {
                        offset.set(plan.start_s);
                        p.load(&c.media_url(&plan.url));
                        if !on_demand {
                            attach_subtitles(&c, &p, id, plan.start_s).await;
                        }
                        p.play();
                    }
                    Err(e) => error.set(Some(describe(&e))),
                }
            });
        }
    };
    let skip = {
        let mut go_to = go_to.clone();
        move |delta: f64| {
            let d = duration();
            if d.is_finite() && d > 0.0 {
                go_to(seek_fraction((position() + delta) / d, d));
            }
        }
    };
    // YouTube's keys: Space or K plays and pauses, J and L skip 10 s, the arrows skip 5 s and
    // change the volume, M mutes, F fills the screen, C turns subtitles on and off, 0 to 9 jump
    // to that tenth, Home and End to the ends, < and > change the speed, Shift+N plays the next.
    let on_key = {
        let mut skip = skip.clone();
        let mut go_to = go_to.clone();
        let leave = save_and_leave.clone();
        let p = player.clone();
        move |e: KeyboardEvent| {
            if e.key() == Key::Escape && crate::player::is_fullscreen() {
                return crate::player::toggle_fullscreen();
            }
            if e.key() == Key::Escape || e.key() == Key::GoBack || e.key() == Key::BrowserBack {
                return leave();
            }
            let was_hidden = !controls_shown();
            quiet.set(0);
            instant.set(false);
            // The remote's media keys (and a keyboard's) act at once, controls shown or not.
            let media = match e.key() {
                Key::MediaPlayPause => {
                    if p.is_playing() {
                        p.pause()
                    } else {
                        p.play()
                    }
                    true
                }
                Key::MediaPlay => {
                    p.play();
                    true
                }
                Key::MediaPause => {
                    p.pause();
                    true
                }
                Key::MediaFastForward => {
                    skip(SKIP_SECONDS);
                    true
                }
                Key::MediaRewind => {
                    skip(-SKIP_SECONDS);
                    true
                }
                Key::MediaTrackNext => {
                    if let Some((to, _)) = up_next() {
                        nav.push(to);
                    }
                    true
                }
                _ => false,
            };
            if media {
                e.prevent_default();
                return;
            }
            // On the TV the first press only brings the controls back; a keyboard acts at once.
            if was_hidden && cfg!(target_os = "android") {
                e.prevent_default();
                return;
            }
            // Leave the browser's own shortcuts alone.
            if e.modifiers()
                .intersects(Modifiers::CONTROL | Modifiers::META | Modifiers::ALT)
            {
                return;
            }
            let web = cfg!(target_arch = "wasm32");
            let d = duration();
            let handled = match e.key() {
                Key::ArrowLeft => {
                    skip(-ARROW_SECONDS);
                    true
                }
                Key::ArrowRight => {
                    skip(ARROW_SECONDS);
                    true
                }
                // On the TV the up and down arrows move between buttons.
                Key::ArrowUp if web => {
                    hint.set(crate::player::nudge_volume(0.05));
                    true
                }
                Key::ArrowDown if web => {
                    hint.set(crate::player::nudge_volume(-0.05));
                    true
                }
                Key::Home if d > 0.0 => {
                    go_to(0.0);
                    true
                }
                Key::End if d > 0.0 => {
                    go_to(seek_fraction(1.0, d));
                    true
                }
                Key::Character(c) => match c.as_str() {
                    " " | "k" | "K" => {
                        if p.is_playing() {
                            p.pause()
                        } else {
                            p.play()
                        }
                        true
                    }
                    "j" | "J" => {
                        skip(-SKIP_SECONDS);
                        true
                    }
                    "l" | "L" => {
                        skip(SKIP_SECONDS);
                        true
                    }
                    "f" | "F" if web => {
                        crate::player::toggle_fullscreen();
                        true
                    }
                    "m" | "M" if web => {
                        hint.set(crate::player::toggle_mute());
                        true
                    }
                    ">" if web => {
                        hint.set(crate::player::nudge_speed(0.25));
                        true
                    }
                    "<" if web => {
                        hint.set(crate::player::nudge_speed(-0.25));
                        true
                    }
                    "c" | "C" => {
                        let text: Vec<_> = p
                            .tracks()
                            .into_iter()
                            .filter(|t| t.kind == "text")
                            .collect();
                        hint.set(Some(if text.iter().any(|t| t.selected) {
                            p.subtitles_off();
                            "Subtitles off".into()
                        } else if let Some(t) = text.first() {
                            p.select_track(t);
                            format!("Subtitles: {}", t.name())
                        } else {
                            "No subtitles for this title".into()
                        }));
                        true
                    }
                    "N" => {
                        if let Some((to, _)) = up_next() {
                            nav.push(to);
                        }
                        true
                    }
                    n => match n.parse::<u8>() {
                        Ok(tenth) if tenth <= 9 && d > 0.0 => {
                            go_to(seek_fraction(f64::from(tenth) / 10.0, d));
                            true
                        }
                        _ => false,
                    },
                },
                _ => false,
            };
            if handled {
                e.prevent_default();
            }
        }
    };
    let toggle = {
        let p = player.clone();
        move |_| if p.is_playing() { p.pause() } else { p.play() }
    };
    let seek_to = {
        let mut go_to = go_to.clone();
        move |e: FormEvent| {
            if let Ok(v) = e.value().parse::<f64>() {
                go_to(seek_fraction(v / 1000.0, duration()));
            }
        }
    };
    let fraction = if duration() > 0.0 {
        position() / duration()
    } else {
        0.0
    };
    let leave = save_and_leave.clone();
    // The last two minutes of an episode with another after it.
    let near_end = duration() > 0.0 && duration() - position() < 120.0;

    rsx! {
        div {
            class: if controls_shown() { "player" } else if instant() { "player idle instant" } else { "player idle" },
            "data-rh-seek": "true",
            onkeydown: on_key,
            // Space on a focused button would press it as well when the key comes up.
            onkeyup: move |e: KeyboardEvent| {
                if e.key() == Key::Character(" ".into()) {
                    e.prevent_default();
                }
            },
            onmousemove: move |_| quiet.set(0),
            {crate::player::surface()}
            div { class: "player-top",
                button { class: "player-back", "aria-label": "Back", onclick: move |_| leave(),
                    span { class: "chevron-left", "aria-hidden": "true" }
                }
                if let Some((title, sub)) = heading() {
                    div { class: "player-heading",
                        p { class: "player-title", "{title}" }
                        if !sub.is_empty() || badge().is_some() {
                            p { class: "player-sub", "{sub}"
                                if let Some(q) = badge() {
                                    span { class: "quality", "{q}" }
                                }
                            }
                        }
                    }
                }
            }
            if let Some((to, label)) = up_next().filter(|_| near_end) {
                div { class: "up-next",
                    span { class: "quiet",
                        match countdown() {
                            Some(n) => format!("Up next in {} s", n.div_ceil(4)),
                            None => "Up next".to_string(),
                        }
                    }
                    Link { class: "primary", to, "{label}" }
                    if countdown().is_some() {
                        button {
                            class: "secondary",
                            onclick: move |_| {
                                autoplay_off.set(true);
                                countdown.set(None);
                            },
                            "Stay here"
                        }
                    }
                }
            }
            div { class: "player-controls",
                if let Some(msg) = error() {
                    p { class: "notice", "Playback stopped: {msg}" }
                }
                if let Some(h) = hint() {
                    p { class: "player-hint", role: "status", "{h}" }
                }
                div { class: "player-scrub",
                    span { class: "time", "{format_time(position())}" }
                    input {
                        r#type: "range",
                        class: "seek",
                        "aria-label": "Position",
                        min: 0,
                        max: 1000,
                        value: (fraction * 1000.0).round(),
                        style: "--played: {fraction * 100.0}%",
                        oninput: seek_to,
                    }
                    span { class: "time", "-{format_time((duration() - position()).max(0.0))}" }
                }
                div { class: "player-row",
                    button { class: "player-skip", "aria-label": "Back 10 seconds", onclick: { let mut skip = skip.clone(); move |_| skip(-SKIP_SECONDS) },
                        "−10"
                    }
                    button { class: "primary player-toggle", autofocus: true, onclick: toggle,
                        if playing() {
                            span { class: "pause-icon", "aria-hidden": "true" }
                            "Pause"
                        } else {
                            span { class: "play-icon", "aria-hidden": "true" }
                            "Play"
                        }
                    }
                    button { class: "player-skip", "aria-label": "Forward 10 seconds", onclick: { let mut skip = skip.clone(); move |_| skip(SKIP_SECONDS) },
                        "+10"
                    }
                    div { class: "player-end",
                        if cfg!(target_os = "android")
                            || tracks().iter().any(|t| t.kind == "text")
                            || tracks().iter().filter(|t| t.kind == "audio").count() > 1
                        {
                            button {
                                class: if picking() { "player-tracks current" } else { "player-tracks" },
                                "aria-expanded": picking(),
                                onclick: {
                                    let p = player.clone();
                                    move |_| {
                                        tracks.set(p.tracks());
                                        picking.toggle();
                                    }
                                },
                                "Audio & subtitles"
                            }
                        }
                        if cfg!(target_arch = "wasm32") {
                            button {
                                class: "player-tracks",
                                "aria-label": if crate::player::is_fullscreen() { "Exit full screen" } else { "Full screen" },
                                title: "Full screen (F)",
                                onclick: move |_| crate::player::toggle_fullscreen(),
                                if crate::player::is_fullscreen() { "Exit full screen" } else { "Full screen" }
                            }
                        }
                    }
                }
                if picking() {
                    TrackPicker {
                        tracks: tracks(),
                        on_pick: {
                            let p = player.clone();
                            move |choice: Option<crate::player::Track>| match choice {
                                Some(t) => p.select_track(&t),
                                None => p.subtitles_off(),
                            }
                        },
                    }
                }
            }
        }
    }
}

/// Audio and subtitle choices for the playing title.
#[component]
/// `on_pick` gets the chosen track, or `None` for subtitles off.
fn TrackPicker(
    tracks: Vec<crate::player::Track>,
    on_pick: EventHandler<Option<crate::player::Track>>,
) -> Element {
    let audio: Vec<_> = tracks
        .iter()
        .filter(|t| t.kind == "audio")
        .cloned()
        .collect();
    let text: Vec<_> = tracks
        .iter()
        .filter(|t| t.kind == "text")
        .cloned()
        .collect();
    let subtitles_on = text.iter().any(|t| t.selected);
    rsx! {
        div { class: "track-picker",
            section {
                h3 { "Audio" }
                if audio.is_empty() {
                    p { class: "quiet", "One soundtrack." }
                }
                for t in audio {
                    button {
                        key: "a{t.group}-{t.index}",
                        class: if t.selected { "track current" } else { "track" },
                        "aria-pressed": t.selected,
                        onclick: {
                            let t = t.clone();
                            move |_| on_pick.call(Some(t.clone()))
                        },
                        "{t.name()}"
                    }
                }
            }
            // Browsers get no subtitle renditions yet; Off alone would only confuse.
            if !text.is_empty() || cfg!(target_os = "android") {
            section {
                h3 { "Subtitles" }
                button {
                    class: if subtitles_on { "track" } else { "track current" },
                    "aria-pressed": !subtitles_on,
                    onclick: move |_| on_pick.call(None),
                    "Off"
                }
                for t in text {
                    button {
                        key: "t{t.group}-{t.index}",
                        class: if t.selected { "track current" } else { "track" },
                        "aria-pressed": t.selected,
                        onclick: {
                            let t = t.clone();
                            move |_| on_pick.call(Some(t.clone()))
                        },
                        "{t.name()}"
                    }
                }
            }
            }
        }
    }
}

/// The downloaded file (as a URL the player opens) and its saved details, if this item is on the device.
#[cfg(not(target_arch = "wasm32"))]
fn downloaded(id: i64) -> Option<(String, reelhouse_api::Item)> {
    let file = crate::downloads::local_file(id)?;
    let item = crate::downloads::item(id)?;
    Some((format!("file://{}", file.display()), item))
}

#[cfg(target_arch = "wasm32")]
fn downloaded(id: i64) -> Option<(String, reelhouse_api::Item)> {
    Some((
        crate::downloads::local_url(id)?,
        crate::downloads::item(id)?,
    ))
}
