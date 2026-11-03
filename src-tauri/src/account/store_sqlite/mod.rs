pub mod codec;
pub mod migration;
pub mod schema;

use std::path::{Path, PathBuf};
use rusqlite::Connection;

use crate::account::store::{now_secs, AccountStore};

pub const STORE_SQLITE_FILENAME: &str = "account-store.sqlite3";
pub const STORE_JSON_FILENAME: &str = "account-store.json";

/// Marker file that turns this data directory into a read-only replica.
///
/// A warm-standby relay restores the primary's account database from the Litestream replica
/// and must never write it: two writable account databases would fork the account state.
/// The marker is created before the first restore and removed only by a promotion, so the
/// read-only state survives restarts and needs no environment variable to stay in force.
pub const STANDBY_MARKER_FILENAME: &str = "standby.readonly";

/// Prefix of the error every refused write returns, so callers can branch on it.
pub const READ_ONLY_ERROR_CODE: &str = "ACCOUNT_STORE_READ_ONLY";

pub fn is_read_only(dir: &Path) -> bool {
    dir.join(STANDBY_MARKER_FILENAME).exists()
}

fn ensure_writable(dir: &Path) -> Result<(), String> {
    if is_read_only(dir) {
        return Err(format!(
            "{READ_ONLY_ERROR_CODE}: this relay is a warm standby; its account store is a \
             read-only replica until a failover promotes it"
        ));
    }
    Ok(())
}

pub fn sqlite_store_path(dir: &Path) -> PathBuf {
    dir.join(STORE_SQLITE_FILENAME)
}

/// Serializes this process's writers of one account store.
///
/// Every write rewrites the whole store inside `BEGIN IMMEDIATE`, so concurrent writers of one
/// relay used to queue on SQLite's busy handler, and under slow disk commits a waiter gave up
/// after the busy timeout with "database is locked" (a sign-in poll failed that way in
/// production). Writers of this process now wait here instead, without a deadline; the busy
/// timeout only covers other processes. Stores in different directories never wait on each other.
fn store_write_lock(dir: &Path) -> std::sync::Arc<parking_lot::Mutex<()>> {
    static LOCKS: std::sync::LazyLock<
        parking_lot::Mutex<std::collections::HashMap<PathBuf, std::sync::Arc<parking_lot::Mutex<()>>>>,
    > = std::sync::LazyLock::new(Default::default);
    std::sync::Arc::clone(LOCKS.lock().entry(dir.to_path_buf()).or_default())
}

pub fn json_store_path(dir: &Path) -> PathBuf {
    dir.join(STORE_JSON_FILENAME)
}

pub fn open_sqlite_connection(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("Failed to create account store directory: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Failed to set permissions on account store directory: {error}"))?;
    }

    // Serialize the one-time initialization window (the rollback-journal to WAL transition plus
    // the initial schema DDL) across threads of this process and across processes;
    // `schema::lock_initialization` documents why SQLite cannot serialize it for us. Every
    // connection takes this lock, so no other connection can hold a lock against this
    // connection's WAL transition either.
    let init_lock = schema::lock_initialization(dir)?;

    let db_path = sqlite_store_path(dir);
    let mut conn = Connection::open(&db_path)
        .map_err(|error| format!("Failed to open sqlite database {}: {error}", db_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Failed to set permissions on sqlite database file: {error}"))?;
    }

    schema::configure_connection(&conn)?;
    schema::init_schema(&mut conn, is_read_only(dir))?;

    // The connection outlives the initialization window, so the lock is released explicitly
    // (an error above drops it on the way out).
    drop(init_lock);

    Ok(conn)
}

pub fn mutate_transaction<T, E, F>(dir: &Path, mutate_fn: F) -> Result<T, E>
where
    E: From<String>,
    F: FnOnce(&mut AccountStore) -> Result<T, E>,
{
    ensure_writable(dir).map_err(E::from)?;
    let write_lock = store_write_lock(dir);
    let _writer = write_lock.lock();
    let mut conn = open_sqlite_connection(dir).map_err(E::from)?;
    migration::maybe_import_legacy_json(&mut conn, dir).map_err(E::from)?;

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| E::from(format!("Failed to begin immediate transaction: {error}")))?;

    let mut store = codec::load_store_from_connection(&tx).map_err(E::from)?;
    let now = now_secs();
    store.purge_expired(now);

    let result = mutate_fn(&mut store)?;

    codec::save_store_to_transaction(&tx, &store).map_err(E::from)?;

    tx.commit()
        .map_err(|error| E::from(format!("Failed to commit mutate transaction: {error}")))?;

    Ok(result)
}

/// Runs `lease_fn` inside one `BEGIN IMMEDIATE` transaction without loading or saving the
/// account store: supervisor lease writes are targeted SQL on the lease tables only, so a renew
/// every few seconds per machine never rewrites the whole store. Refusals returned by
/// `lease_fn` roll the transaction back. A read-only replica refuses like every other write.
pub fn lease_transaction<T, E, F>(dir: &Path, lease_fn: F) -> Result<T, E>
where
    E: From<String>,
    F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T, E>,
{
    ensure_writable(dir).map_err(E::from)?;
    let write_lock = store_write_lock(dir);
    let _writer = write_lock.lock();
    let mut conn = open_sqlite_connection(dir).map_err(E::from)?;
    migration::maybe_import_legacy_json(&mut conn, dir).map_err(E::from)?;

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| E::from(format!("Failed to begin immediate lease transaction: {error}")))?;
    let result = lease_fn(&tx)?;
    tx.commit()
        .map_err(|error| E::from(format!("Failed to commit lease transaction: {error}")))?;
    Ok(result)
}

/// Runs a lease read on a plain connection; works on a read-only replica too.
pub fn lease_read<T, E, F>(dir: &Path, read_fn: F) -> Result<T, E>
where
    E: From<String>,
    F: FnOnce(&Connection) -> Result<T, E>,
{
    let conn = open_sqlite_connection(dir).map_err(E::from)?;
    read_fn(&conn)
}

pub fn load_sqlite_store(dir: &Path) -> Result<AccountStore, String> {
    let mut conn = open_sqlite_connection(dir)?;
    migration::maybe_import_legacy_json(&mut conn, dir)?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("Failed to begin read transaction: {error}"))?;
    let store = codec::load_store_from_connection(&tx)?;
    tx.commit()
        .map_err(|error| format!("Failed to commit read transaction: {error}"))?;
    Ok(store)
}

pub fn save_sqlite_store(dir: &Path, store: &AccountStore) -> Result<(), String> {
    ensure_writable(dir)?;
    let write_lock = store_write_lock(dir);
    let _writer = write_lock.lock();
    let mut conn = open_sqlite_connection(dir)?;
    migration::maybe_import_legacy_json(&mut conn, dir)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| format!("Failed to begin immediate transaction: {error}"))?;
    codec::save_store_to_transaction(&tx, store)?;
    tx.commit()
        .map_err(|error| format!("Failed to commit store save: {error}"))?;
    Ok(())
}
