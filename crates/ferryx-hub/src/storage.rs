use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::error::Result;

/// How long a connection waits for another writer before "database is locked".
pub const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Opens the hub database and applies the embedded migrations.
///
/// A memory pool uses one connection: each extra connection would be a different
/// in-memory database.
///
/// WAL lets readers run beside the writer, and `BUSY_TIMEOUT` makes a second writer wait
/// instead of failing: with rollback journaling and no timeout, concurrent watchers hit
/// "database is locked" (2026-10-24).
pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let uri = format!("sqlite://{}", path.display());
    let options = SqliteConnectOptions::from_str(&uri)
        .map_err(|error| crate::error::HubError::config(format!("hub database url: {error}")))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(BUSY_TIMEOUT);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Opens an existing database without applying migrations. Import uses this so a
/// live omo-gateway file is never rewritten by the hub's own schema.
pub async fn connect_readonly(path: &Path) -> Result<SqlitePool> {
    let uri = format!("sqlite://{}", path.display());
    let options = SqliteConnectOptions::from_str(&uri)
        .map_err(|error| crate::error::HubError::config(format!("hub database url: {error}")))?
        .read_only(true)
        .foreign_keys(false);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(crate::error::HubError::from)
}

pub async fn memory() -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .map_err(|error| crate::error::HubError::config(error.to_string()))?
        .foreign_keys(true);
    // The database lives only as long as its one connection, so that connection is never
    // retired for idling or age (a long dry-run import would otherwise lose its tables).
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<()> {
    // Embedded at build time: an installed binary has no source tree to read them from.
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

/// Every hub transaction writes, and most read first (next sequence number, existing row).
/// A deferred `BEGIN` takes its read snapshot first and fails with SQLITE_BUSY_SNAPSHOT
/// (code 517, "database is locked") when another connection commits before its first write;
/// `busy_timeout` never retries that. `BEGIN IMMEDIATE` takes the write lock up front, so a
/// second writer waits on `BUSY_TIMEOUT` instead (2026-10-27: Slack transcript rows and the
/// owner-inbox reconciliation were dropped this way).
pub(crate) trait WriteTx {
    async fn begin_write(&self) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Sqlite>>;
}

impl WriteTx for SqlitePool {
    async fn begin_write(&self) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        self.begin_with("BEGIN IMMEDIATE").await
    }
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_file_database_uses_wal_and_waits_for_a_busy_writer() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-wal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pool = connect(&dir.join("hub.sqlite")).await.unwrap();
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        let timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(timeout, BUSY_TIMEOUT.as_millis() as i64);
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read-then-write transaction must not fail when another connection commits between
    /// its read and its write: with a deferred BEGIN this returned code 517 in production.
    #[tokio::test]
    async fn a_write_transaction_survives_a_commit_between_its_read_and_its_write() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-snapshot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pool = connect(&dir.join("hub.sqlite")).await.unwrap();
        sqlx::query("CREATE TABLE t (n INTEGER)")
            .execute(&pool)
            .await
            .unwrap();

        let mut deferred = pool.begin().await.unwrap();
        let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t")
            .fetch_one(&mut *deferred)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t VALUES (1)")
            .execute(&pool)
            .await
            .unwrap();
        let stale = sqlx::query("INSERT INTO t VALUES (2)")
            .execute(&mut *deferred)
            .await
            .unwrap_err();
        assert!(stale.to_string().contains("517"), "{stale}");
        drop(deferred);

        let mut tx = pool.begin_write().await.unwrap();
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let other = tokio::spawn({
            let pool = pool.clone();
            async move { sqlx::query("INSERT INTO t VALUES (3)").execute(&pool).await }
        });
        sqlx::query("INSERT INTO t VALUES (4)")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        other.await.unwrap().unwrap();
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after, before + 2);
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
