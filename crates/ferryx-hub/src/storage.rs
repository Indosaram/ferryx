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
///
/// Opening and migrating run under an exclusive lock on `<path>.open-lock`. Switching a fresh
/// (rollback-journal) file to WAL upgrades a read transaction to a write one, and SQLite never
/// calls the busy handler for that upgrade: two processes opening a new hub database at once
/// failed with code 5 "database is locked" despite `BUSY_TIMEOUT` (2026-10-28, q28: 9 of 10
/// runs on Linux). The lock also keeps two openers from migrating at once.
pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let db_path = path.to_owned();
    let _open_lock = tokio::task::spawn_blocking(move || lock_open(&db_path))
        .await
        .map_err(|error| {
            crate::error::HubError::message(format!("hub database open lock: {error}"))
        })??;
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

/// Takes the exclusive `<db_path>.open-lock` that [`connect`] holds while it opens and
/// migrates; released when the returned file drops. Blocking.
pub fn lock_open(db_path: &Path) -> std::io::Result<std::fs::File> {
    let mut name = db_path.as_os_str().to_owned();
    name.push(".open-lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(std::path::PathBuf::from(name))?;
    file.lock()?;
    Ok(file)
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

pub(crate) async fn migrate(pool: &SqlitePool) -> Result<()> {
    // Embedded at build time: an installed binary has no source tree to read them from.
    let migrator = sqlx::migrate!("./migrations");
    // sqlx's SQLite migrator takes no lock (its `lock()` is a no-op): two processes opening a
    // fresh or older hub database at once (the daemon's hub start and a `ferryx-cli supervisor`
    // command) both read the applied list, and the one that loses the write lock then re-runs
    // a migration the other committed ("duplicate column name", a duplicate version row) and
    // the hub does not open. A failed run is retried while the applied count still grows (the
    // winner's commits, or this run's own progress); a failure with no progress is a real one.
    // Bounded: the count cannot exceed the number of migrations.
    let mut applied = applied_migrations(pool).await;
    loop {
        match migrator.run(pool).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                let now = applied_migrations(pool).await;
                if now <= applied {
                    return Err(error.into());
                }
                tracing::info!(%error, applied = now, "another process migrated the hub database concurrently; migrating again");
                applied = now;
            }
        }
    }
}

/// Rows in sqlx's migration table; 0 before the first migration created it.
async fn applied_migrations(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap_or(0)
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

    /// Several processes may open a fresh hub database at once (the daemon's hub start beside
    /// a `ferryx-cli supervisor` command, 2026-10-28 q24): every opener gets the full schema.
    #[tokio::test]
    async fn concurrent_openers_of_a_fresh_database_all_migrate_it() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.sqlite");
        let (a, b, c, d) = tokio::join!(
            connect(&path),
            connect(&path),
            connect(&path),
            connect(&path)
        );
        for pool in [a, b, c, d] {
            let pool = pool.expect("every concurrent opener migrates");
            let held: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pragma_table_info('supervisor_goals') WHERE name = 'held_from'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(held, 1);
            pool.close().await;
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The q28 failure needed real parallelism: eight openers on worker threads, several fresh
    /// databases in a row, every one of them must open with the full schema.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn many_parallel_openers_of_fresh_databases_all_open_them() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-race8-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for round in 0..6 {
            let path = dir.join(format!("hub-{round}.sqlite"));
            let openers: Vec<_> = (0..8)
                .map(|_| {
                    let path = path.clone();
                    tokio::spawn(async move { connect(&path).await })
                })
                .collect();
            for opener in openers {
                let pool = opener.await.unwrap().unwrap_or_else(|error| {
                    panic!("round {round}: every parallel opener opens: {error:?}")
                });
                let held: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM pragma_table_info('supervisor_goals') WHERE name = 'held_from'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(held, 1, "round {round}");
                pool.close().await;
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

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
