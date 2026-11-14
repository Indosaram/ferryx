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

/// Forward-only v3 migration: a nullable `relay_fallback_origins` column on `machines`, holding
/// the JSON array of fallback relay origins the machine published at enrollment. Existing rows
/// keep NULL, which reads back as an empty list.
pub fn migrate_to_v3(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("Failed to begin v3 migration transaction: {error}"))?;
    // Additive and idempotent: only a `machines` table that lacks the column is altered.
    let missing: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'machines') \
             AND NOT EXISTS(SELECT 1 FROM pragma_table_info('machines') WHERE name = 'relay_fallback_origins')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed to inspect machines for the v3 migration: {error}"))?;
    if missing {
        tx.execute_batch("ALTER TABLE machines ADD COLUMN relay_fallback_origins TEXT;")
            .map_err(|error| format!("Failed to apply v3 relay fallback migration: {error}"))?;
    }
    tx.pragma_update(None, "user_version", 3)
        .map_err(|error| format!("Failed to set user_version to 3: {error}"))?;
    tx.commit()
        .map_err(|error| format!("Failed to commit v3 relay fallback migration: {error}"))
}

/// Rewrites machine records saved on the retired relay origin to the current relay, keeping any
/// path (exact origin match only). Runs on the writable paths, never on a read-only standby;
/// idempotent, so a migrated store is left unchanged. Logs the deprecation once per rewrite.
pub fn migrate_retired_relay_origins(conn: &Connection) -> Result<usize, String> {
    use crate::remote::relay_client::RETIRED_RELAY_ORIGIN;
    let changed = conn
        .execute(
            "UPDATE machines SET relay_origin = ?2 || substr(relay_origin, length(?1) + 1) \
             WHERE relay_origin = ?1 OR substr(relay_origin, 1, length(?1) + 1) = ?1 || '/'",
            params![RETIRED_RELAY_ORIGIN, crate::remote::state::DEFAULT_RELAY_URL],
        )
        .map_err(|err| format!("Failed to migrate retired relay origins: {err}"))?;
    if changed > 0 {
        crate::remote::relay_client::warn_retired_relay_migrated(&format!(
            "{changed} account machine record(s)"
        ));
    }
    Ok(changed)
}

#[cfg(test)]
mod retired_relay_origin_tests {
    use super::*;

    #[test]
    fn machine_records_on_the_retired_origin_move_to_the_current_relay_once() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE machines (machine_record_id TEXT PRIMARY KEY, relay_origin TEXT NOT NULL);
             INSERT INTO machines VALUES ('a', 'https://relay.checka.cc');
             INSERT INTO machines VALUES ('b', 'https://relay.checka.cc/host/m-1');
             INSERT INTO machines VALUES ('c', 'https://relay.checka.cc.example');
             INSERT INTO machines VALUES ('d', 'https://relay.ferryx.dev');",
        )
        .unwrap();
        assert_eq!(migrate_retired_relay_origins(&conn).unwrap(), 2);
        let origins: Vec<String> = conn
            .prepare("SELECT relay_origin FROM machines ORDER BY machine_record_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            origins,
            [
                "https://relay.ferryx.dev",
                "https://relay.ferryx.dev/host/m-1",
                "https://relay.checka.cc.example",
                "https://relay.ferryx.dev",
            ]
        );
        assert_eq!(migrate_retired_relay_origins(&conn).unwrap(), 0, "idempotent");
    }
}

#[cfg(test)]
mod relay_fallback_origin_tests {
    use crate::account::store::{AccountStore, MachineRecord, UserRecord};
    use crate::account::store::store_sqlite::{
        load_sqlite_store, open_sqlite_connection, save_sqlite_store, schema::CURRENT_SCHEMA_VERSION,
    };

    fn machine(id: &str, fallbacks: Vec<String>) -> MachineRecord {
        MachineRecord {
            machine_record_id: format!("rec-{id}"),
            owner_user_id: "u1".into(),
            machine_id: id.into(),
            display_name: id.into(),
            public_key: "pk".into(),
            attach_public_key: "apk".into(),
            relay_origin: "https://relay.test".into(),
            relay_fallback_origins: fallbacks,
            platform: "linux".into(),
            enrollment_epoch: 1,
            enrolled_at: 1,
            last_seen_at: 1,
        }
    }

    #[test]
    fn machine_record_fallback_origins_round_trip_through_sqlite_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = AccountStore::default();
        store.users.insert(
            "u1".into(),
            UserRecord { user_id: "u1".into(), email: "u1@test.local".into(), created_at: 1 },
        );
        let fallbacks = vec![
            "https://standby-b.example.com".to_string(),
            "https://standby-a.example.com".to_string(),
        ];
        store.machines.insert("rec-m1".into(), machine("m1", fallbacks.clone()));
        store.machines.insert("rec-m2".into(), machine("m2", Vec::new()));
        save_sqlite_store(dir.path(), &store).unwrap();
        let loaded = load_sqlite_store(dir.path()).unwrap();
        assert_eq!(loaded.machines["rec-m1"].relay_fallback_origins, fallbacks);
        assert!(loaded.machines["rec-m2"].relay_fallback_origins.is_empty());
        // An empty list is stored as NULL, like a row written before the column existed.
        let conn = open_sqlite_connection(dir.path()).unwrap();
        let raw: Option<String> = conn
            .query_row(
                "SELECT relay_fallback_origins FROM machines WHERE machine_record_id = 'rec-m2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw, None);
    }

    /// A v2 database (no fallback column) upgrades in place: its machine rows load with no
    /// fallbacks, and a fallback list written afterwards round-trips.
    #[test]
    fn a_v2_database_without_the_column_migrates_and_old_rows_still_load() {
        let dir = tempfile::tempdir().unwrap();
        {
            // Build a real current database, then drop the v3 column to get the v2 shape.
            let conn = open_sqlite_connection(dir.path()).unwrap();
            conn.execute_batch(
                "ALTER TABLE machines DROP COLUMN relay_fallback_origins;
                 PRAGMA user_version = 2;
                 INSERT INTO users (user_id, email, created_at) VALUES ('u1', 'u1@test.local', 1);
                 INSERT INTO machines (machine_record_id, user_id, machine_id, display_name, public_key,
                   attach_public_key, relay_origin, platform, enrollment_epoch, enrolled_at, last_seen_at)
                   VALUES ('rec-old', 'u1', 'old', 'old', 'pk', 'apk', 'https://relay.test', 'linux', 1, 1, 1);",
            )
            .unwrap();
            let columns: Vec<String> = conn
                .prepare("SELECT name FROM pragma_table_info('machines')")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert!(!columns.iter().any(|c| c == "relay_fallback_origins"), "seeded as v2");
        }
        let mut store = load_sqlite_store(dir.path()).expect("a v2 store loads after migration");
        let conn = open_sqlite_connection(dir.path()).unwrap();
        let version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        assert!(store.machines["rec-old"].relay_fallback_origins.is_empty());

        store.machines.get_mut("rec-old").unwrap().relay_fallback_origins =
            vec!["https://standby.example.com".into()];
        save_sqlite_store(dir.path(), &store).unwrap();
        assert_eq!(
            load_sqlite_store(dir.path()).unwrap().machines["rec-old"].relay_fallback_origins,
            ["https://standby.example.com"]
        );
    }

    #[test]
    fn an_old_json_machine_record_without_the_field_deserializes() {
        let json = r#"{"machineRecordId":"r","ownerUserId":"u","machineId":"m","displayName":"d",
            "publicKey":"pk","attachPublicKey":"apk","relayOrigin":"https://relay.test",
            "platform":"linux","enrollmentEpoch":1,"enrolledAt":1}"#;
        let record: MachineRecord = serde_json::from_str(json).unwrap();
        assert!(record.relay_fallback_origins.is_empty());
        assert!(!serde_json::to_string(&record).unwrap().contains("relayFallbackOrigins"));
    }
}
