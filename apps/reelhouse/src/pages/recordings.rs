//! Recorded: what the DVR is recording now, what it will record, and what it has, to play or
//! delete. Finished recordings also live in the Recordings library.

use dioxus::prelude::*;
use reelhouse_api::{Recording, RecordingStatus};
use reelhouse_core::timefmt::clock;

use super::live::{Mode, ModeToggle};
use crate::Route;
use crate::session::client;
use crate::widgets::{LoadError, describe};

#[component]
pub fn Recordings() -> Element {
    let mut recs = use_resource(|| async move { client().recordings().await });
    // The device's timezone offset, for times of day.
    let offset = use_resource(|| async move {
        document::eval("return new Date().getTimezoneOffset()")
            .join::<i64>()
            .await
            .unwrap_or(0)
    });
    // While something records or is about to, its state moves on by itself.
    use_future(move || async move {
        loop {
            futures_timer::Delay::new(std::time::Duration::from_secs(15)).await;
            recs.restart();
        }
    });
    let mut note = use_signal(|| None::<String>);
    let list: Vec<Recording> = match &*recs.read() {
        Some(Ok(l)) => l.clone(),
        Some(Err(e)) => {
            return rsx! { div { class: "page page-top", LoadError { error: describe(e) } } };
        }
        None => Vec::new(),
    };
    let off = offset.read().unwrap_or(0);
    let when = move |r: &Recording| format!("{} to {}", clock(r.start, off), clock(r.stop, off));
    let now: Vec<_> = list
        .iter()
        .filter(|r| r.status == RecordingStatus::Recording)
        .cloned()
        .collect();
    let soon: Vec<_> = list
        .iter()
        .filter(|r| r.status == RecordingStatus::Scheduled)
        .cloned()
        .collect();
    let mut kept: Vec<_> = list
        .iter()
        .filter(|r| r.status == RecordingStatus::Done && r.item_id.is_some())
        .cloned()
        .collect();
    kept.reverse();
    let failed: Vec<_> = list
        .iter()
        .filter(|r| r.status == RecordingStatus::Failed)
        .cloned()
        .collect();
    let name = |r: &Recording| match &r.subtitle {
        Some(s) => format!("{}, {s}", r.title),
        None => r.title.clone(),
    };
    let cancel = move |rule: i64| async move {
        match client().delete_rule(rule).await {
            Ok(()) => note.set(Some("Cancelled.".into())),
            Err(e) => note.set(Some(describe(&e))),
        }
        recs.restart();
    };
    let delete = move |id: i64| async move {
        match client().delete_recording(id).await {
            Ok(()) => note.set(Some("Recording deleted.".into())),
            Err(e) => note.set(Some(describe(&e))),
        }
        recs.restart();
    };
    rsx! {
        div { class: "page page-top live",
            div { class: "live-body",
                aside { class: "live-groups",
                    ModeToggle { current: Mode::Recorded }
                }
                section { class: "live-main recordings",
                    if let Some(n) = note() {
                        p { class: "toast", role: "status", "{n}" }
                    }
                    if recs.read().is_some() && list.is_empty() {
                        p { class: "empty", "Nothing recorded yet. In Live, open the Guide and choose Record on a programme." }
                    }
                    if !now.is_empty() {
                        h2 { "Recording now" }
                        ul { class: "rec-list",
                            for r in now {
                                li { key: "{r.id}", class: "rec-row",
                                    span { class: "rec-dot live", "aria-hidden": "true" }
                                    span { class: "rec-name", "{name(&r)}" }
                                    span { class: "quiet", "{when(&r)}" }
                                    button { class: "secondary", onclick: move |_| cancel(r.rule_id), "Stop" }
                                }
                            }
                        }
                    }
                    if !soon.is_empty() {
                        h2 { "Scheduled" }
                        ul { class: "rec-list",
                            for r in soon {
                                li { key: "{r.id}", class: "rec-row",
                                    span { class: "rec-dot", "aria-hidden": "true" }
                                    span { class: "rec-name", "{name(&r)}" }
                                    span { class: "quiet", "{when(&r)}" }
                                    button { class: "secondary", onclick: move |_| cancel(r.rule_id), "Cancel" }
                                }
                            }
                        }
                    }
                    if !kept.is_empty() {
                        h2 { "Recorded" }
                        ul { class: "rec-list",
                            for r in kept {
                                li { key: "{r.id}", class: "rec-row",
                                    Link { class: "rec-play", to: Route::Play { id: r.item_id.unwrap_or_default() },
                                        span { class: "play-icon", "aria-hidden": "true" }
                                        span { class: "rec-name", "{name(&r)}" }
                                    }
                                    span { class: "quiet", "{when(&r)}" }
                                    button { class: "secondary", onclick: move |_| delete(r.id), "Delete" }
                                }
                            }
                        }
                    }
                    if !failed.is_empty() {
                        h2 { "Didn't record" }
                        ul { class: "rec-list",
                            for r in failed {
                                li { key: "{r.id}", class: "rec-row",
                                    span { class: "rec-name quiet", "{name(&r)}" }
                                    span { class: "quiet", "{when(&r)}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
