//! SQLite pool with migrations applied.

use std::path::Path;
use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

pub async fn open(data_dir: &Path) -> sqlx::Result<SqlitePool> {
    std::fs::create_dir_all(data_dir)?;
    let opts = SqliteConnectOptions::new()
        .filename(data_dir.join("reelhouse.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().connect_with(opts).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

pub async fn open_memory() -> sqlx::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    // One connection: each in-memory connection would otherwise be its own empty database.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn migrations_apply() {
        let pool = super::open_memory().await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
