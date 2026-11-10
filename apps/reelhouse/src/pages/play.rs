use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;
use reelhouse_api::Progress;
use reelhouse_core::timefmt::{format_time, seek_fraction};

use crate::player::{Player, capabilities};
use crate::session::client;
use crate::widgets::describe;

const SKIP_SECONDS: f64 = 10.0;
const REPORT_EVERY: u32 = 40; // polls of 250 ms = 10 s

#[component]
pub fn Play(id: i64) -> Element {
    let player = use_hook(|| Rc::new(Player::new()));
    let mut error = use_signal(|| None::<String>);
    let mut position = use_signal(|| 0.0);
    let mut duration = use_signal(|| 0.0);
    let mut playing = use_signal(|| false);
    let nav = navigator();

    let p = player.clone();
    use_future(move || {
        let p = p.clone();
        async move {
            let c = client();
            let item = match c.item(id).await {
                Ok(i) => i,
                Err(e) => return error.set(Some(describe(&e))),
            };
            // A live-converted HLS stream only reports what is converted so far; the scan knows the full length.
            let known = item.duration_s.unwrap_or(0.0);
            let plan = match c.playback(id, &capabilities()).await {
                Ok(plan) => plan,
                Err(e) => return error.set(Some(describe(&e))),
            };
            p.load(&c.media_url(&plan.url));
            if let Some(at) = item
                .progress
                .filter(|pr| !pr.played)
                .map(|pr| pr.position_s)
            {
                p.seek(at);
            }
            p.play();
            let mut ticks = 0;
            loop {
                futures_timer::Delay::new(Duration::from_millis(250)).await;
                position.set(p.position());
                duration.set(p.duration().max(known));
                playing.set(p.is_playing());
                if let Some(e) = p.error() {
                    error.set(Some(e));
                }
                ticks += 1;
                if ticks % REPORT_EVERY == 0 && p.is_playing() {
                    let _ = c
                        .put_progress(
                            id,
                            &Progress {
                                position_s: p.position(),
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
            let pos = p.position();
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
    let skip = {
        let p = player.clone();
        move |delta: f64| {
            let d = duration().max(p.duration());
            if d.is_finite() && d > 0.0 {
                p.seek(seek_fraction((p.position() + delta) / d, d));
            }
        }
    };
    let on_key = {
        let skip = skip.clone();
        let leave = save_and_leave.clone();
        move |e: KeyboardEvent| match e.key() {
            Key::ArrowLeft => {
                e.prevent_default();
                skip(-SKIP_SECONDS);
            }
            Key::ArrowRight => {
                e.prevent_default();
                skip(SKIP_SECONDS);
            }
            Key::Escape | Key::GoBack | Key::BrowserBack => leave(),
            _ => {}
        }
    };
    let toggle = {
        let p = player.clone();
        move |_| if p.is_playing() { p.pause() } else { p.play() }
    };
    let seek_to = {
        let p = player.clone();
        move |e: FormEvent| {
            if let Ok(v) = e.value().parse::<f64>() {
                p.seek(seek_fraction(v / 1000.0, duration().max(p.duration())));
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
        div { class: "player", onkeydown: on_key,
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
