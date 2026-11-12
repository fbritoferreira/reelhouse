//! The guide: channels down, time across, as on a TV guide. Focus a programme to see what it is
//! and record it, once or every episode; select one that is on now to watch it.

use dioxus::prelude::*;
use reelhouse_api::{
    GuideChannel, NewRecordingRule, Programme, Recording, RecordingStatus, RuleKind,
};
use reelhouse_core::timefmt::clock;

use super::live::Clock;
use crate::Route;
use crate::live_cache::LIVE;
use crate::session::client;
use crate::widgets::{LoadError, describe};

/// How much time the grid shows, from the half hour before now.
const WINDOW_S: i64 = 3 * 3600 + 1800;
const HALF_HOUR: i64 = 1800;
/// Channel rows drawn at a time.
const ROWS: usize = 30;

/// Where a programme sits in the window, as percentages of its width: `(left, width)`.
/// `None` when it falls outside.
fn span(start: i64, stop: i64, from: i64, to: i64) -> Option<(f64, f64)> {
    let (a, b) = (start.max(from), stop.min(to));
    if b <= a {
        return None;
    }
    let len = (to - from) as f64;
    Some((
        (a - from) as f64 / len * 100.0,
        (b - a) as f64 / len * 100.0,
    ))
}

/// The scheduled or running recording of `p`, if any. Recordings are padded, so one covers its
/// programme whole and only touches the neighbours.
fn recording_of<'a>(p: &Programme, recs: &'a [Recording]) -> Option<&'a Recording> {
    recs.iter().find(|r| {
        r.channel_id == p.channel_id
            && matches!(
                r.status,
                RecordingStatus::Scheduled | RecordingStatus::Recording
            )
            && r.start <= p.start
            && p.stop <= r.stop
    })
}

#[component]
pub fn GuideGrid(group: String, time: Signal<Clock>) -> Element {
    let now = time().now;
    // The window starts at the half hour before now and moves on as time passes.
    let from = now - now.rem_euclid(HALF_HOUR);
    let to = from + WINDOW_S;
    let guide = use_resource(use_reactive!(|group, from| async move {
        if from == 0 {
            return Ok(Vec::new());
        }
        client().guide_window(&group, from, from + WINDOW_S).await
    }));
    let mut recs = use_resource(|| async move { client().recordings().await.unwrap_or_default() });
    let mut picked = use_signal(|| None::<(GuideChannel, Programme)>);
    let mut rows = use_signal(|| ROWS);
    let mut busy = use_signal(|| false);
    let mut note = use_signal(|| None::<String>);

    let channels: Vec<GuideChannel> = match &*guide.read() {
        Some(Ok(c)) => c.clone(),
        Some(Err(e)) => return rsx! { LoadError { error: describe(e) } },
        None => {
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
    let recordings: Vec<Recording> = recs.read().clone().unwrap_or_default();
    let limit = rows().min(channels.len());
    let ticks: Vec<i64> = (0..WINDOW_S / HALF_HOUR)
        .map(|i| from + i * HALF_HOUR)
        .collect();
    let now_at = span(now, now + 1, from, to).map(|(l, _)| l);

    let record = move |kind: RuleKind, p: Programme| async move {
        busy.set(true);
        let rule = NewRecordingRule {
            kind,
            programme_id: Some(p.id),
            channel_id: None,
            series_title: None,
            padding_before_s: 60,
            padding_after_s: 300,
            keep_last: None,
        };
        match client().create_rule(&rule).await {
            Ok(_) => {
                note.set(Some(match kind {
                    RuleKind::Once => format!("{} will be recorded.", p.title),
                    RuleKind::Series => format!("Every episode of {} will be recorded.", p.title),
                }));
                // The recorder plans from rules every few seconds; look again shortly.
                futures_timer::Delay::new(std::time::Duration::from_secs(12)).await;
                recs.restart();
            }
            Err(e) => note.set(Some(describe(&e))),
        }
        busy.set(false);
    };
    let cancel = move |rule_id: i64| async move {
        busy.set(true);
        match client().delete_rule(rule_id).await {
            Ok(()) => {
                note.set(Some("Recording cancelled.".into()));
                recs.restart();
            }
            Err(e) => note.set(Some(describe(&e))),
        }
        busy.set(false);
    };

    let detail = picked().or_else(|| {
        // Until something is focused, describe what is on now on the first channel with a guide.
        channels.iter().find_map(|c| {
            c.programmes
                .iter()
                .find(|p| p.start <= now && now < p.stop)
                .map(|p| (c.clone(), p.clone()))
        })
    });

    rsx! {
        if let Some((c, p)) = detail {
            {
                let rec = recording_of(&p, &recordings).cloned();
                let on_now = p.start <= now && now < p.stop;
                let ended = p.stop <= now;
                rsx! {
                    div { class: "on-air guide-detail",
                        div { class: "on-air-text",
                            p { class: "on-air-channel",
                                if let Some(n) = c.channel.number {
                                    span { class: "channel-number", "{n}" }
                                }
                                span { "{c.channel.name}" }
                            }
                            p { class: "on-air-now",
                                if on_now {
                                    span { class: "lamp", "aria-label": "On now" }
                                }
                                "{p.title}"
                            }
                            p { class: "quiet",
                                "{clock(p.start, time().offset_min)} to {clock(p.stop, time().offset_min)}"
                                if let Some(s) = p.subtitle.clone() {
                                    ", {s}"
                                }
                            }
                            if let Some(d) = p.description.clone() {
                                p { class: "on-air-desc", "{d}" }
                            }
                            if let Some(n) = note() {
                                p { class: "notice", role: "status", "{n}" }
                            }
                        }
                        div { class: "guide-actions",
                            if on_now {
                                Link { class: "primary", to: Route::Watch { id: c.channel.id, group: group.clone().into() },
                                    span { class: "play-icon", "aria-hidden": "true" }
                                    "Watch"
                                }
                            }
                            match (rec, ended) {
                                (Some(r), _) => rsx! {
                                    button { class: "secondary rec-on", disabled: busy(), onclick: move |_| cancel(r.rule_id),
                                        span { class: "rec-dot", "aria-hidden": "true" }
                                        if r.status == RecordingStatus::Recording { "Recording, stop" } else { "Cancel recording" }
                                    }
                                },
                                (None, false) => rsx! {
                                    button { class: "secondary", disabled: busy(), onclick: {
                                            let p = p.clone();
                                            move |_| record(RuleKind::Once, p.clone())
                                        },
                                        span { class: "rec-dot", "aria-hidden": "true" }
                                        "Record"
                                    }
                                    button { class: "secondary", disabled: busy(), onclick: {
                                            let p = p.clone();
                                            move |_| record(RuleKind::Series, p.clone())
                                        },
                                        "Record every episode"
                                    }
                                },
                                (None, true) => rsx! {},
                            }
                        }
                    }
                }
            }
        }
        div { class: "guide", role: "grid", "aria-label": "Guide",
            div { class: "guide-row guide-times", role: "row",
                span { class: "guide-channel" }
                div { class: "guide-track",
                    for t in ticks {
                        span {
                            key: "{t}",
                            class: "guide-tick",
                            style: "left: {span(t, t + 1, from, to).map_or(0.0, |s| s.0)}%",
                            "{clock(t, time().offset_min)}"
                        }
                    }
                }
            }
            for (i, c) in channels[..limit].iter().cloned().enumerate() {
                div { key: "{c.channel.id}", class: "guide-row", role: "row",
                    onfocusin: move |_| {
                        if i + 4 >= rows() {
                            rows += ROWS;
                        }
                    },
                    Link {
                        class: "guide-channel",
                        to: Route::Watch { id: c.channel.id, group: group.clone().into() },
                        span { class: "channel-number", "{c.channel.number.map(|n| n.to_string()).unwrap_or_default()}" }
                        span { class: "guide-channel-name", "{c.channel.name}" }
                    }
                    div { class: "guide-track",
                        if let Some(x) = now_at {
                            span { class: "guide-now", style: "left: {x}%" }
                        }
                        if c.programmes.is_empty() {
                            span { class: "guide-empty", "No guide for this channel" }
                        }
                        for p in c.programmes.clone() {
                            if let Some((left, width)) = span(p.start, p.stop, from, to) {
                                {
                                    let recorded = recording_of(&p, &recordings).is_some();
                                    let on_now = p.start <= now && now < p.stop;
                                    let class = match (on_now, recorded) {
                                        (true, true) => "guide-prog now rec",
                                        (true, false) => "guide-prog now",
                                        (false, true) => "guide-prog rec",
                                        (false, false) => "guide-prog",
                                    };
                                    let entry = (c.clone(), p.clone());
                                    let nav = navigator();
                                    let channel_id = c.channel.id;
                                    let g = group.clone();
                                    rsx! {
                                        button {
                                            key: "{p.id}",
                                            class,
                                            role: "gridcell",
                                            style: "left: {left}%; width: {width}%",
                                            onfocus: move |_| {
                                                note.set(None);
                                                picked.set(Some(entry.clone()));
                                            },
                                            onclick: move |_| {
                                                if on_now {
                                                    LIVE.write().place.returning = true;
                                                    nav.push(Route::Watch { id: channel_id, group: g.clone().into() });
                                                }
                                            },
                                            if recorded {
                                                span { class: "rec-dot", "aria-label": "Recording" }
                                            }
                                            span { class: "guide-prog-title", "{p.title}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if limit < channels.len() {
                button { class: "secondary live-more", onclick: move |_| rows += ROWS,
                    "Show {(channels.len() - limit).min(ROWS)} more channels"
                }
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
            title: "NCIS".into(),
            subtitle: None,
            description: None,
        }
    }

    #[test]
    fn a_padded_recording_marks_only_its_programme() {
        // 23:00 to 00:00 recorded with a minute before and five after.
        let rec = Recording {
            id: 1,
            rule_id: 1,
            channel_id: 1,
            title: "NCIS".into(),
            subtitle: None,
            start: 1000 - 60,
            stop: 4600 + 300,
            status: RecordingStatus::Scheduled,
            item_id: None,
            failure: None,
        };
        let recs = [rec];
        assert!(recording_of(&prog(2, 1000, 4600), &recs).is_some());
        assert!(recording_of(&prog(1, -2600, 1000), &recs).is_none());
        assert!(recording_of(&prog(3, 4600, 8200), &recs).is_none());
    }

    #[test]
    fn programmes_are_placed_and_clipped_to_the_window() {
        // Window of 1000 s.
        assert_eq!(span(100, 300, 0, 1000), Some((10.0, 20.0)));
        // Started before the window: clipped at its left edge.
        assert_eq!(span(-500, 200, 0, 1000), Some((0.0, 20.0)));
        // Runs past the end: clipped at the right.
        assert_eq!(span(900, 5000, 0, 1000), Some((90.0, 10.0)));
        // Outside entirely.
        assert_eq!(span(1000, 1200, 0, 1000), None);
        assert_eq!(span(-200, 0, 0, 1000), None);
    }
}
