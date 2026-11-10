use dioxus::prelude::*;

/// Everything downloaded to this device, playable without the server. Grouped by show, with
/// anything still arriving first.
#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Downloads() -> Element {
    use crate::Route;
    use crate::download_ui::{Ring, reel_of};
    use crate::downloads::{self, State};
    use crate::widgets::episode_name;

    let mut tick = use_signal(|| 0u32);
    use_future(move || async move {
        loop {
            futures_timer::Delay::new(std::time::Duration::from_secs(1)).await;
            tick += 1;
        }
    });
    let _ = tick();
    let list = downloads::list();
    // (heading, entries); films share one group, episodes group under their show.
    let mut groups: Vec<(String, Vec<(downloads::Download, State)>)> = Vec::new();
    for d in list {
        let heading = d.item.show_title.clone().unwrap_or_else(|| "Films".into());
        let state = downloads::state(d.item.id);
        match groups.iter_mut().find(|(h, _)| *h == heading) {
            Some((_, entries)) => entries.push((d, state)),
            None => groups.push((heading, vec![(d, state)])),
        }
    }
    let unfinished = |s: &State| *s != State::Done;
    for (_, entries) in &mut groups {
        entries.sort_by_key(|(d, s)| (!unfinished(s), d.item.season_no, d.item.episode_no));
    }
    groups.sort_by_key(|(_, entries)| !entries.iter().any(|(_, s)| unfinished(s)));
    rsx! {
        div { class: "page page-top",
            header { class: "page-head",
                h1 { "Downloads" }
                p { class: "quiet", "On this TV. They play without your server." }
            }
            if groups.is_empty() {
                p { class: "empty", "Nothing downloaded yet. Open a film, an episode or a show and choose Download." }
            }
            for (heading, entries) in groups {
                section { key: "{heading}", class: "download-group",
                    h2 { "{heading}" }
                    ul { class: "downloads",
                        for (d, state) in entries {
                            li { key: "{d.item.id}",
                                Link { class: "wide", to: Route::Play { id: d.item.id },
                                    div { class: "wide-art",
                                        img { src: "/art/{d.item.id}/backdrop", alt: "" }
                                    }
                                    span { class: "poster-title",
                                        if d.item.show_title.is_some() { "{episode_name(&d.item)}" } else { "{d.item.title}" }
                                    }
                                    span { class: "poster-caption download-caption",
                                        Ring { reel: reel_of(&state) }
                                        {caption(&d, &state)}
                                    }
                                }
                                button {
                                    class: "secondary",
                                    onclick: move |_| {
                                        downloads::remove(d.item.id);
                                        tick += 1;
                                    },
                                    "Remove"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn caption(d: &crate::downloads::Download, state: &crate::downloads::State) -> String {
    use crate::downloads::State;
    let code = match (d.item.season_no, d.item.episode_no) {
        (Some(s), Some(e)) => format!("S{s} E{e}, "),
        _ => String::new(),
    };
    let what = match state {
        State::Done => size(d.bytes),
        State::Queued => "Waiting to download".into(),
        State::Running { fraction } => format!("Downloading, {}%", (fraction * 100.0).round()),
        State::Paused { fraction } => {
            format!("Stopped at {}%, play to resume", (fraction * 100.0).round())
        }
        State::Failed(_) => "Download stopped".into(),
        State::NotDownloaded => String::new(),
    };
    format!("{code}{what}")
}

#[cfg(target_arch = "wasm32")]
#[component]
pub fn Downloads() -> Element {
    rsx! {
        div { class: "page page-top", p { class: "quiet", "Downloads are available in the TV and mobile apps." } }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn size(bytes: u64) -> String {
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}
