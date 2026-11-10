use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;
use reelhouse_api::Progress;
use reelhouse_core::timefmt::{format_time, needs_restart, seek_fraction};

use crate::Route;
use crate::player::{Player, capabilities, stereo_capabilities};
use crate::session::client;
use crate::widgets::describe;

const SKIP_SECONDS: f64 = 10.0;
const REPORT_EVERY: u32 = 40; // polls of 250 ms = 10 s
/// Controls fade out after this many quiet polls of 250 ms while playing (3 s).
const HIDE_AFTER: u32 = 12;

/// A library title.
#[component]
pub fn Play(id: i64) -> Element {
    rsx! { PlayerScreen { id, on_demand: false } }
}

/// A film or episode a Live TV source offers on demand.
#[component]
pub fn PlayVod(id: i64) -> Element {
    rsx! { PlayerScreen { id, on_demand: true } }
}

/// Saves where playback is; failures are left for the next report.
async fn save_progress(c: &reelhouse_core::Client, id: i64, on_demand: bool, position_s: f64) {
    let p = Progress {
        position_s,
        played: false,
    };
    let _ = if on_demand {
        c.put_vod_progress(id, &p).await
    } else {
        c.put_progress(id, &p).await
    };
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
    let nav = navigator();

    let p = player.clone();
    use_future(move || {
        let p = p.clone();
        async move {
            let c = client();
            let local = if on_demand { None } else { downloaded(id) };
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
                            t.progress
                                .filter(|p| !p.played)
                                .map(|p| p.position_s)
                                .unwrap_or(0.0),
                        )
                    }
                    Err(_) => (0.0, 0.0),
                }
            } else {
                let item = match (c.item(id).await, &local) {
                    (Ok(i), _) => i,
                    (Err(_), Some((_, saved))) => saved.clone(),
                    (Err(e), None) => return error.set(Some(describe(&e))),
                };
                // A live-converted HLS stream only reports what is converted so far; the scan knows
                // the full length.
                let resume = item
                    .progress
                    .as_ref()
                    .filter(|pr| !pr.played)
                    .map(|pr| pr.position_s)
                    .unwrap_or(0.0);
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
                                if plan.start_s == 0.0 {
                                    p.seek(at);
                                }
                                p.play();
                            }
                            Err(e2) => error.set(Some(describe(&e2))),
                        }
                    }
                }
                ticks += 1;
                if ticks % REPORT_EVERY == 0 && p.is_playing() {
                    save_progress(&c, id, on_demand, offset() + p.position()).await;
                }
            }
        }
    });

    let save_and_leave = {
        let p = player.clone();
        move || {
            let pos = offset() + p.position();
            p.pause();
            spawn(async move { save_progress(&client(), id, on_demand, pos).await });
            nav.go_back();
        }
    };
    let controls_shown = move || !playing() || quiet() < HIDE_AFTER || error().is_some();
    // The remote's Back first hides the controls; with them hidden it saves and returns.
    crate::tv::use_back({
        let leave = save_and_leave.clone();
        move || {
            if controls_shown() && playing() {
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
    let on_key = {
        let mut skip = skip.clone();
        let leave = save_and_leave.clone();
        move |e: KeyboardEvent| {
            if e.key() == Key::Escape || e.key() == Key::GoBack || e.key() == Key::BrowserBack {
                return leave();
            }
            // With the controls hidden, the first press only brings them back.
            let was_hidden = !controls_shown();
            quiet.set(0);
            instant.set(false);
            if was_hidden {
                e.prevent_default();
                return;
            }
            match e.key() {
                Key::ArrowLeft => {
                    e.prevent_default();
                    skip(-SKIP_SECONDS);
                }
                Key::ArrowRight => {
                    e.prevent_default();
                    skip(SKIP_SECONDS);
                }
                _ => {}
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
            onmousemove: move |_| quiet.set(0),
            {crate::player::surface()}
            div { class: "player-top",
                button { class: "player-back", "aria-label": "Back", onclick: move |_| leave(),
                    span { class: "chevron-left", "aria-hidden": "true" }
                }
                if let Some((title, sub)) = heading() {
                    div { class: "player-heading",
                        p { class: "player-title", "{title}" }
                        if !sub.is_empty() {
                            p { class: "player-sub", "{sub}" }
                        }
                    }
                }
            }
            if let Some((to, label)) = up_next().filter(|_| near_end) {
                div { class: "up-next",
                    span { class: "quiet", "Up next" }
                    Link { class: "primary", to, "{label}" }
                }
            }
            div { class: "player-controls",
                if let Some(msg) = error() {
                    p { class: "notice", "Playback stopped: {msg}" }
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
fn downloaded(_id: i64) -> Option<(String, reelhouse_api::Item)> {
    None
}
