pub mod codec;
pub mod migration;
pub mod schema;

use std::path::{Path, PathBuf};
use rusqlite::Connection;

use crate::account::store::{now_secs, AccountStore};

pub const STORE_SQLITE_FILENAME: &str = "account-store.sqlite3";
pub const STORE_JSON_FILENAME: &str = "account-store.json";

pub fn sqlite_store_path(dir: &Path) -> PathBuf {
    dir.join(STORE_SQLITE_FILENAME)
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
    schema::init_schema(&mut conn)?;

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
