use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;

use crate::player::Player;
use crate::timefmt::{format_time, seek_fraction};

const SKIP_SECONDS: f64 = 10.0;

#[component]
pub fn Controls() -> Element {
    let player = use_context::<Rc<Player>>();
    let mut position = use_signal(|| 0.0);
    let mut duration = use_signal(|| 0.0);
    let mut playing = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let poll = player.clone();
    use_future(move || {
        let p = poll.clone();
        async move {
            loop {
                position.set(p.position());
                duration.set(p.duration());
                playing.set(p.is_playing());
                error.set(p.error());
                futures_timer::Delay::new(Duration::from_millis(250)).await;
            }
        }
    });

    let skip = {
        let p = player.clone();
        move |delta: f64| {
            // Read the backend, not the 250 ms poll, so repeated key presses accumulate.
            let d = p.duration();
            if d.is_finite() && d > 0.0 {
                p.seek(seek_fraction((p.position() + delta) / d, d));
            }
        }
    };
    let on_arrows = {
        let skip = skip.clone();
        move |e: KeyboardEvent| match e.key() {
            Key::ArrowLeft => {
                e.prevent_default();
                skip(-SKIP_SECONDS)
            }
            Key::ArrowRight => {
                e.prevent_default();
                skip(SKIP_SECONDS)
            }
            _ => {}
        }
    };
    let toggle = {
        let p = player.clone();
        move || if playing() { p.pause() } else { p.play() }
    };
    let seek_input = player.clone();
    let fraction = if duration() > 0.0 {
        position() / duration()
    } else {
        0.0
    };

    rsx! {
        div { class: "controls",
            if let Some(message) = error() {
                div { class: "error", "{message}" }
            }
            div { class: "row",
                button {
                    tabindex: 0,
                    // Enter and the D-pad centre button activate a focused <button> natively.
                    onclick: move |_| toggle(),
                    if playing() { "Pause" } else { "Play" }
                }
                span { class: "time", "{format_time(position())} / {format_time(duration())}" }
            }
            input {
                r#type: "range",
                class: "seek",
                tabindex: 0,
                min: 0,
                max: 1000,
                value: (fraction * 1000.0).round(),
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        seek_input.seek(seek_fraction(v / 1000.0, duration()));
                    }
                },
                onkeydown: on_arrows,
            }
        }
    }
}
