use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use rusqlite::Connection;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Dedicated lock file that serializes database initialization inside an account dir.
pub const INIT_LOCK_FILENAME: &str = "account-store.init.lock";

/// Serializes the one-time initialization window - the rollback-journal to WAL transition plus
/// the initial schema DDL - across threads of this process and across processes.
///
/// SQLite cannot be asked to serialize this window: changing into WAL is refused from inside a
/// transaction (`cannot change into wal mode from within a transaction`), and the transition
/// takes its exclusive lock through `pagerExclusiveLock`, which returns SQLITE_BUSY directly
/// instead of consulting the busy handler. Two connections that open the same *fresh* database
/// at the same time therefore fail with `database is locked` even with a busy timeout set. The
/// window is serialized here instead: a process-wide mutex for threads of this process and an
/// exclusive lock on a dedicated file for other processes. The file lock dies with its holder,
/// so a crashed process cannot wedge initialization.
///
/// Holding order: this is the innermost account-store lock. Nothing acquired inside the
/// initialization window may take `account-store.lock`.
pub fn lock_initialization(dir: &Path) -> Result<InitLockGuard, String> {
    let in_process = INIT_MUTEX.lock().map_err(|_| {
        "Account store initialization lock is poisoned: a previous initialization panicked while \
         holding it"
            .to_string()
    })?;
    let path = dir.join(INIT_LOCK_FILENAME);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| {
            format!(
                "Failed to open account store init lock {}: {error}",
                path.display()
            )
        })?;
    file.lock().map_err(|error| {
        format!(
            "Failed to lock account store init lock {}: {error}",
            path.display()
        )
    })?;
    Ok(InitLockGuard {
        _file: file,
        _in_process: in_process,
    })
}

static INIT_MUTEX: Mutex<()> = Mutex::new(());

/// Holds both initialization locks. Dropping it releases the file lock first, then the in-process
/// mutex.
///
/// The fields are private on purpose: the guard is RAII only, so a caller can hold the lock and
/// drop it, but cannot take a lock out of the struct or keep one alive past the guard.
pub struct InitLockGuard {
    /// Cross-process exclusive lock on [`INIT_LOCK_FILENAME`], released on close or drop.
    _file: File,
    /// Process-wide mutex covering threads of this process, released on drop.
    _in_process: MutexGuard<'static, ()>,
}

pub fn configure_connection(conn: &Connection) -> Result<(), String> {
    conn.busy_timeout(Duration::from_millis(5000))
        .map_err(|error| format!("Failed to set busy timeout: {error}"))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| format!("Failed to set journal_mode WAL: {error}"))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| format!("Failed to enable foreign_keys: {error}"))?;
    Ok(())
}

pub fn init_schema(conn: &mut Connection) -> Result<(), String> {
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("Failed to query PRAGMA user_version: {error}"))?;

    if user_version == 0 {
        let tx = conn
            .transaction()
            .map_err(|error| format!("Failed to begin schema transaction: {error}"))?;

        tx.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                migration_key TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS users (
                user_id TEXT PRIMARY KEY,
                email TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS users_email_unique ON users(email);

            CREATE TABLE IF NOT EXISTS sessions (
                token_hash TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS sessions_user_id ON sessions(user_id);
            CREATE INDEX IF NOT EXISTS sessions_expires_at ON sessions(expires_at);

            CREATE TABLE IF NOT EXISTS login_codes (
                code_hash TEXT PRIMARY KEY,
                email TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                consumed_at INTEGER,
                login_handle_hash TEXT
            );
            CREATE INDEX IF NOT EXISTS login_codes_expires_at ON login_codes(expires_at);

            CREATE TABLE IF NOT EXISTS enrollment_codes (
                code_hash TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                account_origin TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS enrollment_codes_expires_at ON enrollment_codes(expires_at);

            CREATE TABLE IF NOT EXISTS machines (
                machine_record_id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                machine_id TEXT NOT NULL,
                display_name TEXT NOT NULL,
                public_key TEXT NOT NULL,
                attach_public_key TEXT NOT NULL,
                relay_origin TEXT NOT NULL,
                platform TEXT NOT NULL,
                enrollment_epoch INTEGER NOT NULL,
                enrolled_at INTEGER NOT NULL,
                last_seen_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE UNIQUE INDEX IF NOT EXISTS machines_machine_id_unique ON machines(machine_id);
            CREATE INDEX IF NOT EXISTS machines_user_id ON machines(user_id);

            CREATE TABLE IF NOT EXISTS grants (
                grant_id TEXT PRIMARY KEY,
                machine_record_id TEXT NOT NULL REFERENCES machines(machine_record_id) ON DELETE CASCADE,
                owner_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                pairing_token_hash TEXT NOT NULL,
                grant_scope TEXT NOT NULL,
                device_attach_public_key TEXT NOT NULL,
                installation_id TEXT NOT NULL,
                issued_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS grants_expires_at ON grants(expires_at);

            CREATE TABLE IF NOT EXISTS device_auths (
                device_code_hash TEXT PRIMARY KEY,
                user_code TEXT NOT NULL,
                email TEXT NOT NULL,
                email_token_hash TEXT NOT NULL,
                enrollment_code TEXT,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS device_auths_user_code ON device_auths(user_code);

            CREATE TABLE IF NOT EXISTS subscriptions (
                subscription_id TEXT PRIMARY KEY,
                owner_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                org_id TEXT,
                plan_key TEXT NOT NULL,
                seats INTEGER NOT NULL,
                host_packs INTEGER NOT NULL,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                ends_at INTEGER,
                ls_customer_id TEXT,
                ls_updated_at INTEGER NOT NULL,
                manage_url TEXT
            );
            CREATE INDEX IF NOT EXISTS subscriptions_owner ON subscriptions(owner_user_id);
            CREATE INDEX IF NOT EXISTS subscriptions_org ON subscriptions(org_id);
            CREATE INDEX IF NOT EXISTS subscriptions_status ON subscriptions(status);

            CREATE TABLE IF NOT EXISTS billing_states (
                owner_key TEXT PRIMARY KEY,
                grace_started_at INTEGER,
                stopped_at INTEGER,
                last_notice TEXT
            );

            CREATE TABLE IF NOT EXISTS payment_events (
                event_key TEXT PRIMARY KEY,
                event_name TEXT NOT NULL,
                received_at INTEGER NOT NULL,
                applied_at INTEGER,
                outcome TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS payment_events_received ON payment_events(received_at);

            CREATE TABLE IF NOT EXISTS orgs (
                org_id TEXT PRIMARY KEY,
                owner_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS orgs_owner ON orgs(owner_user_id);

            CREATE TABLE IF NOT EXISTS org_members (
                org_id TEXT NOT NULL REFERENCES orgs(org_id) ON DELETE CASCADE,
                user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                joined_at INTEGER NOT NULL,
                PRIMARY KEY (org_id, user_id)
            );
            CREATE INDEX IF NOT EXISTS org_members_user ON org_members(user_id);

            CREATE TABLE IF NOT EXISTS org_invites (
                token_hash TEXT PRIMARY KEY,
                org_id TEXT NOT NULL REFERENCES orgs(org_id) ON DELETE CASCADE,
                email TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS org_invites_org ON org_invites(org_id);
            CREATE INDEX IF NOT EXISTS org_invites_expires_at ON org_invites(expires_at);
            "#,
        )
        .map_err(|error| format!("Failed to create schema tables: {error}"))?;

        tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
            .map_err(|error| format!("Failed to set user_version to {CURRENT_SCHEMA_VERSION}: {error}"))?;

        tx.commit()
            .map_err(|error| format!("Failed to commit schema initialization: {error}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::TryLockError;

    /// Env vars that turn this test binary into the cross-process probe helper.
    const PROBE_PATH_VAR: &str = "FERRYX_INIT_LOCK_PROBE_PATH";
    const PROBE_WANT_VAR: &str = "FERRYX_INIT_LOCK_PROBE_WANT";
    /// Verdict sentinel the helper prints. The parent asserts on it, so a test filter that matches
    /// nothing (no verdict) can never look like a pass.
    const PROBE_SENTINEL: &str = "FERRYX_INIT_LOCK_PROBE:";

    /// Bound for every wait in these tests: event waits only, never sleeps or polling.
    const WAIT_LIMIT: Duration = Duration::from_secs(10);

    fn run_probe(lock_path: &Path, want: &str) -> String {
        let exe = std::env::current_exe().expect("test binary path");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("probe runtime");
        let mut command = tokio::process::Command::new(exe);
        command
            .args(["init_lock_probe_helper", "--nocapture"])
            .env(PROBE_PATH_VAR, lock_path)
            .env(PROBE_WANT_VAR, want)
            // A helper that wedges must not hang the suite: when the timeout drops the `output`
            // future, the child is killed instead of being waited on forever.
            .kill_on_drop(true);
        let output = runtime
            .block_on(async { tokio::time::timeout(WAIT_LIMIT, command.output()).await })
            .unwrap_or_else(|_| {
                panic!(
                    "the init lock probe helper must finish within {:?}",
                    WAIT_LIMIT
                )
            })
            .expect("run the init lock probe helper");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            output.status.success(),
            "init lock probe helper failed (want={want}): status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status
        );
        assert!(
            stdout.contains(PROBE_SENTINEL),
            "init lock probe helper printed no verdict (want={want}): check the test filter\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        stdout
    }

    /// Helper half of `initialization_lock_excludes_a_contending_process`: the parent runs this same
    /// test binary with the probe env vars set. Without them this test is a no-op.
    #[test]
    fn init_lock_probe_helper() {
        let Ok(lock_path) = std::env::var(PROBE_PATH_VAR) else {
            return;
        };
        let want = std::env::var(PROBE_WANT_VAR).expect("probe expectation");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .expect("helper opens the init lock file");
        // `WouldBlock` is matched exactly: any other error (a real I/O or permission failure) must
        // fail the helper loudly instead of being reported as contention. Dropping `file` releases
        // the probe lock.
        match file.try_lock() {
            Ok(()) => {
                println!("{}acquired", PROBE_SENTINEL);
                assert_eq!(want, "acquired", "helper took the init lock while it was held");
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                println!("{}blocked", PROBE_SENTINEL);
                assert_eq!(want, "blocked", "helper could not take the free init lock");
            }
            Err(std::fs::TryLockError::Error(error)) => {
                panic!("init lock probe failed with a non-contention error: {error}");
            }
        }
    }

    /// Cross-process half of the exclusion proof: while this process holds the production guard, a
    /// real second process must not be able to take the lock file, and must be able to take it once
    /// the guard is dropped. `try_lock` means no sleeps anywhere.
    #[test]
    fn initialization_lock_excludes_a_contending_process() {
        if std::env::var(PROBE_PATH_VAR).is_ok() {
            // Running inside the helper process: never spawn another probe from here.
            return;
        }
        let dir = tempfile::tempdir().expect("temp");
        let lock_path = dir.path().join(INIT_LOCK_FILENAME);
        let guard = lock_initialization(dir.path()).expect("hold the init lock");

        let held = run_probe(&lock_path, "blocked");
        assert!(
            held.contains("blocked"),
            "a contending process took the init lock while it was held:\n{held}"
        );

        drop(guard);
        let released = run_probe(&lock_path, "acquired");
        assert!(
            released.contains("acquired"),
            "a contending process could not take the init lock after the guard was dropped:\n{released}"
        );
    }

    /// Same-process regression: the production initialization lock must keep a contender out of
    /// `open_sqlite_connection` until the guard is dropped. The "still blocked" half is asserted
    /// through the exact lock API (a non-blocking second acquisition that must be refused), never by
    /// waiting for silence.
    #[test]
    fn initialization_lock_blocks_a_contender_until_release() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().to_path_buf();

        // The guard is taken before the contender exists, so the contender cannot have reached the
        // store before the lock is held.
        let guard = lock_initialization(&path).expect("hold the init lock");

        // Both channels are armed before the contender can start, so neither event can be missed.
        let (attempting_tx, attempting_rx) = mpsc::channel::<()>();
        let (opened_tx, opened_rx) = mpsc::channel::<()>();
        let contender_path = path.clone();
        let contender = std::thread::spawn(move || {
            attempting_tx.send(()).expect("contender announces its attempt");
            let conn = crate::account::store::store_sqlite::open_sqlite_connection(&contender_path)
                .expect("contender opens the store once the init lock is released");
            let version: u32 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .expect("contender reads the schema version");
            assert_eq!(version, CURRENT_SCHEMA_VERSION);
            opened_tx.send(()).expect("contender announces the open");
            drop(conn);
        });

        attempting_rx
            .recv_timeout(WAIT_LIMIT)
            .expect("the contender must reach its open attempt within 10 s");

        // Exact-API sentinel: this thread owns the production mutex through the guard, so a second
        // acquisition must be refused with WouldBlock - std's `Mutex` is not reentrant and its
        // `try_lock` returns WouldBlock (never a deadlock, never Poisoned) for an already-locked
        // mutex. While the mutex is owned here, no other thread can be inside the window, so the
        // contender cannot have opened the store: that is the blocking claim, with no timing in it.
        match INIT_MUTEX.try_lock() {
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Poisoned(_)) => panic!("the init mutex must not be poisoned here"),
            Ok(_) => panic!("the init mutex must be held by the guard while the guard is alive"),
        }

        drop(guard);
        opened_rx
            .recv_timeout(WAIT_LIMIT)
            .expect("the contender must open the store once the init lock is released");
        contender.join().expect("contender thread joins");

        // The lock stays usable afterwards; this is a blocking acquire, so another test holding the
        // mutex for a moment cannot be misread as a failure.
        let guard_again = lock_initialization(&path).expect("the init lock is reusable");
        drop(guard_again);
    }
}
