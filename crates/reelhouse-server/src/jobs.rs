//! Background jobs: library scans and Live TV refreshes. One worker runs them in order; a failed
//! job is retried up to [`MAX_RETRIES`] times with a doubling wait. Jobs live in SQLite, so work
//! queued when the server stops is picked up when it starts again.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use reelhouse_api::{Job, JobKind, JobStatus};
use sqlx::{FromRow, SqlitePool};
use tokio::sync::Notify;

use crate::app::AppState;
use crate::auth::{AdminUser, now};
use crate::config::Config;
use crate::error::ApiResult;

pub const MAX_RETRIES: i64 = reelhouse_api::JOB_MAX_ATTEMPTS - 1;

/// How often the worker looks for due retries when nothing wakes it.
const POLL: Duration = Duration::from_millis(500);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn kind_str(k: JobKind) -> &'static str {
    match k {
        JobKind::ScanLibrary => "scan_library",
        JobKind::RefreshSource => "refresh_source",
    }
}

#[derive(FromRow)]
struct JobRow {
    id: i64,
    kind: String,
    target_id: i64,
    status: String,
    attempts: i64,
    last_error: Option<String>,
    result: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl From<JobRow> for Job {
    fn from(r: JobRow) -> Self {
        Job {
            id: r.id,
            kind: if r.kind == "refresh_source" {
                JobKind::RefreshSource
            } else {
                JobKind::ScanLibrary
            },
            target_id: r.target_id,
            status: match r.status.as_str() {
                "running" => JobStatus::Running,
                "done" => JobStatus::Done,
                "failed" => JobStatus::Failed,
                _ => JobStatus::Queued,
            },
            attempts: r.attempts,
            last_error: r.last_error,
            result: r.result.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const SELECT: &str = "SELECT id, kind, target_id, status, attempts, last_error, result, created_at, updated_at FROM jobs";

/// Queues `kind` for `target`, or returns the job already waiting or running for it.
pub async fn enqueue(state: &AppState, kind: JobKind, target: i64) -> ApiResult<Job> {
    let existing: Option<JobRow> = sqlx::query_as(&format!(
        "{SELECT} WHERE kind = ? AND target_id = ? AND status IN ('queued', 'running')"
    ))
    .bind(kind_str(kind))
    .bind(target)
    .fetch_optional(&state.db)
    .await?;
    if let Some(job) = existing {
        return Ok(job.into());
    }
    let t = now();
    let row: JobRow = sqlx::query_as(
        "INSERT INTO jobs (kind, target_id, run_after_ms, created_at, updated_at) VALUES (?, ?, ?, ?, ?) \
         RETURNING id, kind, target_id, status, attempts, last_error, result, created_at, updated_at",
    )
    .bind(kind_str(kind))
    .bind(target)
    .bind(now_ms())
    .bind(t)
    .bind(t)
    .fetch_one(&state.db)
    .await?;
    state.jobs.notify_one();
    Ok(row.into())
}

/// Starts the worker. Jobs left running by a stopped server go back in the queue.
pub fn spawn_worker(db: SqlitePool, config: Arc<Config>, wake: Arc<Notify>) {
    tokio::spawn(async move {
        let _ = sqlx::query("UPDATE jobs SET status = 'queued' WHERE status = 'running'")
            .execute(&db)
            .await;
        loop {
            match claim(&db).await {
                Ok(Some((id, kind, target, attempts))) => {
                    let outcome = run(&db, &config, &kind, target).await;
                    finish(&db, &config, id, attempts, outcome).await;
                }
                Ok(None) => {
                    tokio::select! {
                        _ = wake.notified() => {}
                        _ = tokio::time::sleep(POLL) => {}
                    }
                }
                Err(e) => {
                    tracing::error!("job queue: {e:#}");
                    tokio::time::sleep(POLL).await;
                }
            }
        }
    });
}

/// Marks the oldest due job running and returns it.
async fn claim(db: &SqlitePool) -> sqlx::Result<Option<(i64, String, i64, i64)>> {
    sqlx::query_as(
        "UPDATE jobs SET status = 'running', attempts = attempts + 1, updated_at = ? \
         WHERE id = (SELECT id FROM jobs WHERE status = 'queued' AND run_after_ms <= ? ORDER BY id LIMIT 1) \
         RETURNING id, kind, target_id, attempts",
    )
    .bind(now())
    .bind(now_ms())
    .fetch_optional(db)
    .await
}

async fn run(
    db: &SqlitePool,
    config: &Config,
    kind: &str,
    target: i64,
) -> anyhow::Result<serde_json::Value> {
    Ok(match kind {
        "refresh_source" => serde_json::to_value(crate::livetv::refresh(db, target).await?)?,
        _ => serde_json::to_value(crate::libraries::run_scan(db, config, target).await?)?,
    })
}

async fn finish(
    db: &SqlitePool,
    config: &Config,
    id: i64,
    attempts: i64,
    outcome: anyhow::Result<serde_json::Value>,
) {
    let t = now();
    let done = match outcome {
        Ok(result) => sqlx::query(
            "UPDATE jobs SET status = 'done', result = ?, last_error = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(result.to_string())
        .bind(t)
        .bind(id)
        .execute(db)
        .await,
        Err(e) => {
            let retries_used = attempts - 1;
            let (status, wait) = if retries_used < MAX_RETRIES {
                ("queued", config.job_backoff * 2u32.pow(retries_used as u32))
            } else {
                ("failed", Duration::ZERO)
            };
            sqlx::query(
                "UPDATE jobs SET status = ?, last_error = ?, run_after_ms = ?, updated_at = ? WHERE id = ?",
            )
            .bind(status)
            .bind(format!("{e:#}"))
            .bind(now_ms() + wait.as_millis() as i64)
            .bind(t)
            .bind(id)
            .execute(db)
            .await
        }
    };
    if let Err(e) = done {
        tracing::error!("job {id}: could not record its outcome: {e}");
    }
}

/// The 50 most recent jobs, newest first.
pub async fn list(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<Job>>> {
    let rows: Vec<JobRow> = sqlx::query_as(&format!("{SELECT} ORDER BY id DESC LIMIT 50"))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(Job::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Job>> {
    let row: JobRow = sqlx::query_as(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(row.into()))
}
