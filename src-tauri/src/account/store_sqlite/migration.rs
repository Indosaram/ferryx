use std::path::Path;
use rusqlite::{params, Connection, OptionalExtension};

use crate::account::store::{now_secs, AccountStore};
use super::codec::save_store_to_transaction;
use super::json_store_path;

pub const LEGACY_JSON_MIGRATION_KEY: &str = "legacy_json_import";

pub fn is_legacy_migration_applied(conn: &Connection) -> Result<bool, String> {
    let mut stmt = conn
        .prepare("SELECT 1 FROM schema_migrations WHERE migration_key = ?1")
        .map_err(|err| format!("Failed to prepare migration check: {err}"))?;
    let found: Option<i32> = stmt
        .query_row(params![LEGACY_JSON_MIGRATION_KEY], |row| row.get(0))
        .optional()
        .map_err(|err| format!("Failed to query schema_migrations: {err}"))?;
    Ok(found.is_some())
}

pub fn has_existing_data(conn: &Connection) -> Result<bool, String> {
    let count: i64 = conn
        .query_row("SELECT count(*) FROM users", [], |row| row.get(0))
        .map_err(|err| format!("Failed to check user count: {err}"))?;
    if count > 0 {
        return Ok(true);
    }
    let m_count: i64 = conn
        .query_row("SELECT count(*) FROM machines", [], |row| row.get(0))
        .map_err(|err| format!("Failed to check machines count: {err}"))?;
    Ok(m_count > 0)
}

pub fn rename_legacy_json_idempotent(dir: &Path) -> Result<(), String> {
    let json_path = json_store_path(dir);
    if !json_path.exists() {
        return Ok(());
    }
    let timestamp = now_secs();
    let migrated_filename = format!("account-store.json.migrated-{timestamp}");
    let mut migrated_path = dir.join(&migrated_filename);
    let mut nonce = 1;
    while migrated_path.exists() {
        migrated_path = dir.join(format!("{migrated_filename}.{nonce}"));
        nonce += 1;
    }
    match std::fs::rename(&json_path, &migrated_path) {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "Failed to rename legacy store file after migration: {err}"
        )),
    }
}

pub fn maybe_import_legacy_json(conn: &mut Connection, dir: &Path) -> Result<(), String> {
    let json_path = json_store_path(dir);
    if !json_path.exists() {
        return Ok(());
    }

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| format!("Failed to begin migration transaction: {error}"))?;

    let mut stmt = tx
        .prepare("SELECT 1 FROM schema_migrations WHERE migration_key = ?1")
        .map_err(|err| format!("Failed to prepare migration check in tx: {err}"))?;
    let already_applied: Option<i32> = stmt
        .query_row(params![LEGACY_JSON_MIGRATION_KEY], |row| row.get(0))
        .optional()
        .map_err(|err| format!("Failed to query schema_migrations in tx: {err}"))?;
    drop(stmt);

    if already_applied.is_some() {
        tx.commit()
            .map_err(|err| format!("Failed to commit check transaction: {err}"))?;
        rename_legacy_json_idempotent(dir)?;
        return Ok(());
    }

    let user_count: i64 = tx
        .query_row("SELECT count(*) FROM users", [], |row| row.get(0))
        .map_err(|err| format!("Failed to count existing users: {err}"))?;
    let machine_count: i64 = tx
        .query_row("SELECT count(*) FROM machines", [], |row| row.get(0))
        .map_err(|err| format!("Failed to count existing machines: {err}"))?;

    if user_count > 0 || machine_count > 0 {
        tx.execute(
            "INSERT INTO schema_migrations (migration_key, applied_at) VALUES (?1, ?2) \
             ON CONFLICT(migration_key) DO UPDATE SET applied_at=excluded.applied_at",
            params![LEGACY_JSON_MIGRATION_KEY, now_secs() as i64],
        )
        .map_err(|err| format!("Failed to record schema migration marker: {err}"))?;
        tx.commit()
            .map_err(|err| format!("Failed to commit migration marker: {err}"))?;
        rename_legacy_json_idempotent(dir)?;
        return Ok(());
    }

    let bytes = match std::fs::read(&json_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            tx.commit()
                .map_err(|err| format!("Failed to commit empty migration check: {err}"))?;
            return Ok(());
        }
        Err(err) => return Err(format!("Failed to read legacy store file: {err}")),
    };

    let legacy_store: AccountStore = serde_json::from_slice(&bytes)
        .map_err(|error| format!("STORE_CORRUPT: {error}"))?;

    save_store_to_transaction(&tx, &legacy_store)?;

    tx.execute(
        "INSERT INTO schema_migrations (migration_key, applied_at) VALUES (?1, ?2) \
         ON CONFLICT(migration_key) DO UPDATE SET applied_at=excluded.applied_at",
        params![LEGACY_JSON_MIGRATION_KEY, now_secs() as i64],
    )
    .map_err(|err| format!("Failed to record schema migration marker: {err}"))?;

    tx.commit()
        .map_err(|error| format!("Failed to commit imported legacy json: {error}"))?;

    rename_legacy_json_idempotent(dir)?;

    Ok(())
}
