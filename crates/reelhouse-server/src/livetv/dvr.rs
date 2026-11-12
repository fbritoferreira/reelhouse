//! The DVR: turns rules and the guide into recordings, and runs ffmpeg for them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use reelhouse_api::{RecordingStatus, RuleKind};
use sqlx::sqlite::SqliteRow;
use sqlx::{FromRow, Row, SqlitePool};
use tokio::process::{Child, Command};

use super::xmltv::civil_from_days;
use crate::auth::now;
use crate::scan::probe::probe;

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

#[derive(Debug, Clone, Default, PartialEq, FromRow)]
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

pub fn status_str(s: RecordingStatus) -> &'static str {
    match s {
        RecordingStatus::Scheduled => "scheduled",
        RecordingStatus::Recording => "recording",
        RecordingStatus::Done => "done",
        RecordingStatus::Failed => "failed",
    }
}

pub fn parse_status(s: &str) -> RecordingStatus {
    match s {
        "recording" => RecordingStatus::Recording,
        "done" => RecordingStatus::Done,
        "failed" => RecordingStatus::Failed,
        _ => RecordingStatus::Scheduled,
    }
}

pub fn parse_kind(s: &str) -> RuleKind {
    if s == "series" {
        RuleKind::Series
    } else {
        RuleKind::Once
    }
}

impl FromRow<'_, SqliteRow> for Rule {
    fn from_row(r: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Rule {
            id: r.try_get("id")?,
            kind: parse_kind(r.try_get("kind")?),
            channel_id: r.try_get("channel_id")?,
            programme_id: r.try_get("programme_id")?,
            series_title: r.try_get("series_title")?,
            series_key: r.try_get("series_key")?,
            padding_before_s: r.try_get("padding_before_s")?,
            padding_after_s: r.try_get("padding_after_s")?,
        })
    }
}

impl FromRow<'_, SqliteRow> for Existing {
    fn from_row(r: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Existing {
            id: r.try_get("id")?,
            rule_id: r.try_get("rule_id")?,
            channel_id: r.try_get("channel_id")?,
            title: r.try_get("title")?,
            subtitle: r.try_get("subtitle")?,
            start: r.try_get("start")?,
            stop: r.try_get("stop")?,
            status: parse_status(r.try_get("status")?),
        })
    }
}

/// Plans at `now`, stores the new recordings and fails the missed ones.
/// Returns the plan so the caller can start and stop ffmpeg.
pub async fn schedule(db: &SqlitePool, now: i64) -> Result<Plan> {
    let rules: Vec<Rule> = sqlx::query_as("SELECT * FROM recording_rules")
        .fetch_all(db)
        .await?;
    // A day of slack covers any padding after a programme's end.
    let programmes: Vec<Programme> = sqlx::query_as(
        "SELECT id, channel_id, start, stop, title, subtitle, series_key FROM programmes \
         WHERE stop > ? AND channel_id IN (SELECT channel_id FROM recording_rules)",
    )
    .bind(now - 86_400)
    .fetch_all(db)
    .await?;
    // ponytail: loads every recording each tick for duplicate checks; filter by rule if history grows large.
    let recordings: Vec<Existing> = sqlx::query_as("SELECT * FROM recordings")
        .fetch_all(db)
        .await?;
    let plan = plan(&rules, &programmes, &recordings, now);
    for c in &plan.create {
        sqlx::query(
            "INSERT OR IGNORE INTO recordings (rule_id, channel_id, title, subtitle, start, stop, status) \
             VALUES (?, ?, ?, ?, ?, ?, 'scheduled')",
        )
        .bind(c.rule_id)
        .bind(c.channel_id)
        .bind(&c.title)
        .bind(&c.subtitle)
        .bind(c.start)
        .bind(c.stop)
        .execute(db)
        .await?;
    }
    for id in &plan.missed {
        fail(
            db,
            *id,
            "the server wasn't running when it was due to start",
        )
        .await?;
    }
    Ok(plan)
}

/// `<Title>/<Title> - YYYY-MM-DD HH-MM.ts`, in UTC, with characters filesystems reject replaced.
pub fn relative_path(title: &str, start: i64) -> PathBuf {
    let safe: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let safe = match safe.trim().trim_start_matches('.') {
        "" => "Recording",
        s => s,
    };
    let (y, m, d) = civil_from_days(start.div_euclid(86_400));
    let secs = start.rem_euclid(86_400);
    let name = format!(
        "{safe} - {y:04}-{m:02}-{d:02} {:02}-{:02}.ts",
        secs / 3600,
        secs / 60 % 60
    );
    Path::new(safe).join(name)
}

/// Runs ffmpeg for due recordings and files finished ones into the Recordings library.
pub struct Recorder {
    db: SqlitePool,
    dir: PathBuf,
    running: HashMap<i64, Child>,
}

impl Recorder {
    pub fn new(db: SqlitePool, dir: PathBuf) -> Self {
        Self {
            db,
            dir,
            running: HashMap::new(),
        }
    }

    pub fn spawn(mut self, every: Duration) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            loop {
                tick.tick().await;
                if let Err(e) = self.tick(now()).await {
                    tracing::warn!("dvr: {e:#}");
                }
            }
        });
    }

    pub async fn tick(&mut self, now: i64) -> Result<()> {
        let plan = schedule(&self.db, now).await?;
        // Rows deleted with their rule are cancelled; dropping the child kills ffmpeg.
        let live: HashSet<i64> =
            sqlx::query_as::<_, (i64,)>("SELECT id FROM recordings WHERE status = 'recording'")
                .fetch_all(&self.db)
                .await?
                .into_iter()
                .map(|(id,)| id)
                .collect();
        self.running.retain(|id, _| live.contains(id));
        for id in plan.start {
            if let Err(e) = self.start(id, now).await {
                tracing::warn!("dvr: recording {id} failed to start: {e:#}");
                fail(&self.db, id, "it could not be started on the server").await?;
            }
        }
        for id in plan.stop {
            if let Some(mut child) = self.running.remove(&id) {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            self.finish(id).await?;
        }
        Ok(())
    }

    async fn start(&mut self, id: i64, now: i64) -> Result<()> {
        let (url, title, start, stop): (String, String, i64, i64) = sqlx::query_as(
            "SELECT c.stream_url, r.title, r.start, r.stop FROM recordings r \
             JOIN channels c ON c.id = r.channel_id WHERE r.id = ?",
        )
        .bind(id)
        .fetch_one(&self.db)
        .await?;
        let mut path = self.dir.join(relative_path(&title, start));
        // Two rules on one airing would otherwise share a file.
        let (taken,): (bool,) =
            sqlx::query_as("SELECT EXISTS (SELECT 1 FROM recordings WHERE file_path = ?)")
                .bind(path.to_string_lossy())
                .fetch_one(&self.db)
                .await?;
        if taken || path.exists() {
            path.set_extension(format!("{id}.ts"));
        }
        std::fs::create_dir_all(path.parent().unwrap_or(&self.dir))?;
        let child = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-y", "-i", &url, "-c", "copy"])
            .args(["-t", &(stop - now).max(1).to_string(), "-f", "mpegts"])
            .arg(&path)
            .kill_on_drop(true)
            .spawn()?;
        self.running.insert(id, child);
        sqlx::query("UPDATE recordings SET status = 'recording', file_path = ? WHERE id = ?")
            .bind(path.to_string_lossy())
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// Marks the recording done and adds it to the library, or failed if nothing was written.
    async fn finish(&self, id: i64) -> Result<()> {
        let (rule_id, title, subtitle, file, url): (
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
        ) = sqlx::query_as(
            "SELECT r.rule_id, r.title, r.subtitle, r.file_path, c.stream_url FROM recordings r \
             JOIN channels c ON c.id = r.channel_id WHERE r.id = ?",
        )
        .bind(id)
        .fetch_one(&self.db)
        .await?;
        let Some(file) = file.filter(|f| std::fs::metadata(f).is_ok_and(|m| m.len() > 0)) else {
            // Nothing written: ask the provider why, now that ffmpeg has let go of the stream.
            let why = super::provider_status(&url)
                .await
                .and_then(|s| super::refusal(s, "channel"))
                .unwrap_or_else(|| "the channel sent nothing to record".into());
            return fail(&self.db, id, &why).await;
        };
        let library = self.library().await?;
        let info = probe(Path::new(&file)).await.unwrap_or_default();
        let name = match &subtitle {
            Some(s) => format!("{title} - {s}"),
            None => Path::new(&file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or(title),
        };
        let (item_id,): (i64,) = sqlx::query_as(
            "INSERT INTO items (library_id, kind, title, file_path, duration_s, container, video_codec, \
             audio_codec, height, added_at) VALUES (?, 'movie', ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (file_path) DO UPDATE SET title = excluded.title, duration_s = excluded.duration_s, \
             container = excluded.container, video_codec = excluded.video_codec, \
             audio_codec = excluded.audio_codec, height = excluded.height RETURNING id",
        )
        .bind(library)
        .bind(&name)
        .bind(&file)
        .bind(info.duration_s)
        .bind(&info.container)
        .bind(&info.video_codec)
        .bind(&info.audio_codec)
        .bind(info.height)
        .bind(now())
        .fetch_one(&self.db)
        .await?;
        sqlx::query("UPDATE recordings SET status = 'done', item_id = ? WHERE id = ?")
            .bind(item_id)
            .bind(id)
            .execute(&self.db)
            .await?;
        self.keep_last(rule_id).await
    }

    /// The "Recordings" movie library over the recordings folder, created on first use.
    async fn library(&self) -> Result<i64> {
        let found: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM libraries WHERE name = 'Recordings'")
                .fetch_optional(&self.db)
                .await?;
        if let Some((id,)) = found {
            return Ok(id);
        }
        std::fs::create_dir_all(&self.dir)?;
        Ok(sqlx::query(
            "INSERT INTO libraries (name, kind, path) VALUES ('Recordings', 'movies', ?)",
        )
        .bind(self.dir.to_string_lossy())
        .execute(&self.db)
        .await?
        .last_insert_rowid())
    }

    /// Deletes the files and items of a rule's recordings beyond its newest `keep_last`.
    /// The rows stay, so a series rule does not record those episodes again.
    async fn keep_last(&self, rule_id: i64) -> Result<()> {
        let old: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
            "SELECT r.id, r.file_path, r.item_id FROM recordings r JOIN recording_rules u ON u.id = r.rule_id \
             WHERE r.rule_id = ? AND u.keep_last IS NOT NULL AND r.status = 'done' AND r.file_path IS NOT NULL \
             ORDER BY r.start DESC LIMIT -1 OFFSET (SELECT keep_last FROM recording_rules WHERE id = ?)",
        )
        .bind(rule_id)
        .bind(rule_id)
        .fetch_all(&self.db)
        .await?;
        for (id, file, item) in old {
            let _ = std::fs::remove_file(&file);
            sqlx::query("DELETE FROM items WHERE id = ?")
                .bind(item)
                .execute(&self.db)
                .await?;
            sqlx::query("UPDATE recordings SET file_path = NULL, item_id = NULL WHERE id = ?")
                .bind(id)
                .execute(&self.db)
                .await?;
        }
        Ok(())
    }
}

/// Marks a recording failed, with why in plain words.
async fn fail(db: &SqlitePool, id: i64, why: &str) -> Result<()> {
    sqlx::query("UPDATE recordings SET status = 'failed', failure = ? WHERE id = ?")
        .bind(why)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
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

    #[test]
    fn recording_path_is_title_and_utc_start() {
        // 2026-11-10 19:05 UTC
        assert_eq!(
            relative_path("Doctor Who", 1_794_337_500),
            Path::new("Doctor Who/Doctor Who - 2026-11-10 19-05.ts")
        );
        assert_eq!(
            relative_path("../A/B: C?", 0),
            Path::new("_A_B_ C_/_A_B_ C_ - 1970-01-01 00-00.ts")
        );
        assert_eq!(
            relative_path(" ", 0),
            Path::new("Recording/Recording - 1970-01-01 00-00.ts")
        );
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
            plan(&[r], std::slice::from_ref(&p), &[], NOW).create,
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
            std::slice::from_ref(&rerun),
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
