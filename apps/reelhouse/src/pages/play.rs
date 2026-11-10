use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;
use reelhouse_api::Progress;
use reelhouse_core::timefmt::{format_time, needs_restart, seek_fraction};

use crate::player::{Player, capabilities};
use crate::session::client;
use crate::widgets::describe;

const SKIP_SECONDS: f64 = 10.0;
const REPORT_EVERY: u32 = 40; // polls of 250 ms = 10 s
/// Controls fade out after this many quiet polls of 250 ms while playing (3 s).
const HIDE_AFTER: u32 = 12;

#[component]
pub fn Play(id: i64) -> Element {
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
    let nav = navigator();

    let p = player.clone();
    use_future(move || {
        let p = p.clone();
        async move {
            let c = client();
            let local = downloaded(id);
            // The server knows the latest position; a downloaded copy plays without it.
            let item = match (c.item(id).await, &local) {
                (Ok(i), _) => i,
                (Err(_), Some((_, saved))) => saved.clone(),
                (Err(e), None) => return error.set(Some(describe(&e))),
            };
            // A live-converted HLS stream only reports what is converted so far; the scan knows the full length.
            let known = item.duration_s.unwrap_or(0.0);
            let resume_at = item
                .progress
                .filter(|pr| !pr.played)
                .map(|pr| pr.position_s)
                .unwrap_or(0.0);
            match &local {
                Some((file, _)) => {
                    p.load(file);
                    p.seek(resume_at);
                }
                None => {
                    // A conversion starts where we resume; a direct file seeks there itself.
                    let plan = match c.playback(id, &capabilities(), resume_at).await {
                        Ok(plan) => plan,
                        Err(e) => return error.set(Some(describe(&e))),
                    };
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
                    error.set(Some(e));
                }
                ticks += 1;
                if ticks % REPORT_EVERY == 0 && p.is_playing() {
                    let _ = c
                        .put_progress(
                            id,
                            &Progress {
                                position_s: offset() + p.position(),
                                played: false,
                            },
                        )
                        .await;
                }
            }
        }
    });

    let save_and_leave = {
        let p = player.clone();
        move || {
            let pos = offset() + p.position();
            p.pause();
            spawn(async move {
                let _ = client()
                    .put_progress(
                        id,
                        &Progress {
                            position_s: pos,
                            played: false,
                        },
                    )
                    .await;
            });
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
                match c.playback(id, &capabilities(), target).await {
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

    rsx! {
        div {
            class: if controls_shown() { "player" } else if instant() { "player idle instant" } else { "player idle" },
            "data-rh-seek": "true",
            onkeydown: on_key,
            onmousemove: move |_| quiet.set(0),
            {crate::player::surface()}
            div { class: "player-top",
                button { onclick: move |_| leave(), "Back" }
            }
            div { class: "player-controls",
                if let Some(msg) = error() {
                    p { class: "notice", "Playback stopped: {msg}" }
                }
                div { class: "player-row",
                    button { class: "primary", autofocus: true, onclick: toggle, if playing() { "Pause" } else { "Play" } }
                    span { class: "time", "{format_time(position())} / {format_time(duration())}" }
                }
                input {
                    r#type: "range",
                    class: "seek",
                    "aria-label": "Position",
                    min: 0,
                    max: 1000,
                    value: (fraction * 1000.0).round(),
                    oninput: seek_to,
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
