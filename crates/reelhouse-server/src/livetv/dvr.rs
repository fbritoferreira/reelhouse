//! The DVR: turns rules and the guide into recordings, and runs ffmpeg for them.

use reelhouse_api::{RecordingStatus, RuleKind};

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: i64,
    pub kind: RuleKind,
    pub channel_id: i64,
    pub programme_id: Option<i64>,
    pub series_title: Option<String>,
    pub series_key: Option<String>,
    pub padding_before_s: i64,
    pub padding_after_s: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Programme {
    pub id: i64,
    pub channel_id: i64,
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub series_key: Option<String>,
}

/// A row of `recordings`; `start` and `stop` are padded.
#[derive(Debug, Clone, PartialEq)]
pub struct Existing {
    pub id: i64,
    pub rule_id: i64,
    pub channel_id: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub start: i64,
    pub stop: i64,
    pub status: RecordingStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewRecording {
    pub rule_id: i64,
    pub channel_id: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub start: i64,
    pub stop: i64,
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub create: Vec<NewRecording>,
    /// Scheduled recordings whose window has begun.
    pub start: Vec<i64>,
    /// Running recordings whose window has ended.
    pub stop: Vec<i64>,
    /// Scheduled recordings whose window ended before they could start.
    pub missed: Vec<i64>,
}

/// Series match on the guide's series key when both sides have one, else on the exact title.
fn in_series(rule: &Rule, p: &Programme) -> bool {
    match (&rule.series_key, &p.series_key) {
        (Some(a), Some(b)) => a == b,
        _ => rule.series_title.as_deref() == Some(p.title.as_str()),
    }
}

/// Whether `rec` already covers `p` for its rule: the same airing, or (for series with
/// episode names) the same episode recorded or scheduled before.
fn covers(rec: &NewRecording, status: RecordingStatus, rule: &Rule, p: &Programme) -> bool {
    let same_airing =
        rec.channel_id == p.channel_id && rec.start == p.start - rule.padding_before_s;
    let same_episode = p.subtitle.is_some()
        && status != RecordingStatus::Failed
        && rec.title == p.title
        && rec.subtitle == p.subtitle;
    rec.rule_id == rule.id && (same_airing || same_episode)
}

/// What the DVR should do at `now`, given every rule, the guide and the existing recordings.
pub fn plan(rules: &[Rule], programmes: &[Programme], recordings: &[Existing], now: i64) -> Plan {
    let mut out = Plan::default();
    let mut known: Vec<(NewRecording, RecordingStatus)> = recordings
        .iter()
        .map(|r| {
            (
                NewRecording {
                    rule_id: r.rule_id,
                    channel_id: r.channel_id,
                    title: r.title.clone(),
                    subtitle: r.subtitle.clone(),
                    start: r.start,
                    stop: r.stop,
                },
                r.status,
            )
        })
        .collect();
    for rule in rules {
        let wanted = programmes.iter().filter(|p| {
            p.channel_id == rule.channel_id
                && p.stop + rule.padding_after_s > now
                && match rule.kind {
                    RuleKind::Once => rule.programme_id == Some(p.id),
                    RuleKind::Series => in_series(rule, p),
                }
        });
        for p in wanted {
            let done_once =
                rule.kind == RuleKind::Once && known.iter().any(|(r, _)| r.rule_id == rule.id);
            if done_once || known.iter().any(|(r, s)| covers(r, *s, rule, p)) {
                continue;
            }
            let rec = NewRecording {
                rule_id: rule.id,
                channel_id: p.channel_id,
                title: p.title.clone(),
                subtitle: p.subtitle.clone(),
                start: p.start - rule.padding_before_s,
                stop: p.stop + rule.padding_after_s,
            };
            known.push((rec.clone(), RecordingStatus::Scheduled));
            out.create.push(rec);
        }
    }
    for r in recordings {
        match r.status {
            RecordingStatus::Scheduled if r.stop <= now => out.missed.push(r.id),
            RecordingStatus::Scheduled if r.start <= now => out.start.push(r.id),
            RecordingStatus::Recording if r.stop <= now => out.stop.push(r.id),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn rule(kind: RuleKind) -> Rule {
        Rule {
            id: 1,
            kind,
            channel_id: 10,
            programme_id: None,
            series_title: None,
            series_key: None,
            padding_before_s: 60,
            padding_after_s: 120,
        }
    }

    fn prog(id: i64, start: i64, title: &str, subtitle: Option<&str>) -> Programme {
        Programme {
            id,
            channel_id: 10,
            start,
            stop: start + 1800,
            title: title.into(),
            subtitle: subtitle.map(Into::into),
            series_key: None,
        }
    }

    fn existing(id: i64, p: &Programme, status: RecordingStatus) -> Existing {
        Existing {
            id,
            rule_id: 1,
            channel_id: p.channel_id,
            title: p.title.clone(),
            subtitle: p.subtitle.clone(),
            start: p.start - 60,
            stop: p.stop + 120,
            status,
        }
    }

    fn created_starts(plan: &Plan) -> Vec<i64> {
        plan.create.iter().map(|c| c.start + 60).collect()
    }

    #[test]
    fn once_rule_creates_padded_recording() {
        let p = prog(5, NOW + 3600, "Film", Some("Part 1"));
        let r = Rule {
            programme_id: Some(5),
            ..rule(RuleKind::Once)
        };
        assert_eq!(
            plan(&[r], &[p.clone()], &[], NOW).create,
            vec![NewRecording {
                rule_id: 1,
                channel_id: 10,
                title: "Film".into(),
                subtitle: Some("Part 1".into()),
                start: p.start - 60,
                stop: p.stop + 120,
            }]
        );
    }

    #[test]
    fn once_rule_with_recording_creates_nothing() {
        let p = prog(5, NOW + 3600, "Film", None);
        let r = Rule {
            programme_id: Some(5),
            ..rule(RuleKind::Once)
        };
        let e = existing(1, &p, RecordingStatus::Failed);
        assert!(plan(&[r], &[p], &[e], NOW).create.is_empty());
    }

    #[test]
    fn ended_programme_is_not_scheduled() {
        let p = prog(5, NOW - 4000, "Film", None);
        let r = Rule {
            programme_id: Some(5),
            ..rule(RuleKind::Once)
        };
        assert!(plan(&[r], &[p], &[], NOW).create.is_empty());
    }

    #[test]
    fn airing_programme_is_scheduled_late() {
        let p = prog(5, NOW - 600, "Film", None);
        let r = Rule {
            programme_id: Some(5),
            ..rule(RuleKind::Once)
        };
        assert_eq!(plan(&[r], &[p], &[], NOW).create.len(), 1);
    }

    #[test]
    fn series_matches_key_on_its_channel_only() {
        let r = Rule {
            series_title: Some("Show".into()),
            series_key: Some("EP1".into()),
            ..rule(RuleKind::Series)
        };
        let a = Programme {
            series_key: Some("EP1".into()),
            ..prog(1, NOW + 100, "Show: Special", Some("a"))
        };
        let other_key = Programme {
            series_key: Some("EP2".into()),
            ..prog(2, NOW + 2000, "Show", Some("b"))
        };
        let other_channel = Programme {
            channel_id: 11,
            series_key: Some("EP1".into()),
            ..prog(3, NOW + 4000, "Show", Some("c"))
        };
        let got = plan(&[r], &[a.clone(), other_key, other_channel], &[], NOW);
        assert_eq!(created_starts(&got), vec![a.start]);
    }

    #[test]
    fn series_falls_back_to_exact_title() {
        let r = Rule {
            series_title: Some("Show".into()),
            series_key: Some("EP1".into()),
            ..rule(RuleKind::Series)
        };
        let progs = [
            prog(1, NOW + 100, "Show", Some("a")),
            prog(2, NOW + 2000, "Show (HD)", Some("b")),
        ];
        assert_eq!(
            created_starts(&plan(&[r], &progs, &[], NOW)),
            vec![NOW + 100]
        );
    }

    #[test]
    fn series_skips_episodes_already_recorded() {
        let r = Rule {
            series_title: Some("Show".into()),
            ..rule(RuleKind::Series)
        };
        let first = prog(1, NOW - 86_400, "Show", Some("Pilot"));
        let rerun = prog(2, NOW + 100, "Show", Some("Pilot"));
        let rerun_again = prog(3, NOW + 4000, "Show", Some("Pilot"));
        let new = prog(4, NOW + 8000, "Show", Some("Second"));
        let got = plan(
            &[r],
            &[rerun, rerun_again, new.clone()],
            &[existing(1, &first, RecordingStatus::Done)],
            NOW,
        );
        assert_eq!(created_starts(&got), vec![new.start]);
    }

    #[test]
    fn series_retries_an_episode_that_failed() {
        let r = Rule {
            series_title: Some("Show".into()),
            ..rule(RuleKind::Series)
        };
        let first = prog(1, NOW - 86_400, "Show", Some("Pilot"));
        let rerun = prog(2, NOW + 100, "Show", Some("Pilot"));
        let got = plan(
            &[r],
            &[rerun.clone()],
            &[existing(1, &first, RecordingStatus::Failed)],
            NOW,
        );
        assert_eq!(created_starts(&got), vec![rerun.start]);
    }

    #[test]
    fn series_without_subtitles_records_each_slot_once() {
        let r = Rule {
            series_title: Some("News".into()),
            ..rule(RuleKind::Series)
        };
        let a = prog(1, NOW + 100, "News", None);
        let b = prog(2, NOW + 4000, "News", None);
        let got = plan(
            &[r],
            &[a.clone(), b.clone()],
            &[existing(1, &a, RecordingStatus::Scheduled)],
            NOW,
        );
        assert_eq!(created_starts(&got), vec![b.start]);
    }

    #[test]
    fn starts_stops_and_misses_by_time() {
        let due = existing(1, &prog(1, NOW, "A", None), RecordingStatus::Scheduled);
        let later = existing(
            2,
            &prog(2, NOW + 600, "B", None),
            RecordingStatus::Scheduled,
        );
        let missed = existing(
            3,
            &prog(3, NOW - 9000, "C", None),
            RecordingStatus::Scheduled,
        );
        let over = existing(
            4,
            &prog(4, NOW - 9000, "D", None),
            RecordingStatus::Recording,
        );
        let running = existing(5, &prog(5, NOW - 60, "E", None), RecordingStatus::Recording);
        let got = plan(&[], &[], &[due, later, missed, over, running], NOW);
        assert_eq!(got.start, vec![1]);
        assert_eq!(got.missed, vec![3]);
        assert_eq!(got.stop, vec![4]);
    }
}
