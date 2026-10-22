use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[path = "store_sqlite/mod.rs"]
pub mod store_sqlite;

pub const LOGIN_CODE_TTL: Duration = Duration::from_secs(600);
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const ENROLLMENT_CODE_TTL: Duration = Duration::from_secs(600);
pub const DEFAULT_LOGIN_REQUESTS_PER_HOUR: u32 = 5;
pub const DEFAULT_MAX_BODY_BYTES: usize = 4096;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

pub fn token_hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserRecord {
    pub user_id: String,
    pub email: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub user_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoginCodeRecord {
    pub email: String,
    pub expires_at: u64,
    #[serde(default)]
    pub consumed_at: Option<u64>,
    #[serde(default)]
    pub login_handle_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentCodeRecord {
    pub user_id: String,
    pub account_origin: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MachineRecord {
    pub machine_record_id: String,
    pub owner_user_id: String,
    pub machine_id: String,
    pub display_name: String,
    pub public_key: String,
    pub attach_public_key: String,
    pub relay_origin: String,
    pub platform: String,
    pub enrollment_epoch: u64,
    pub enrolled_at: u64,
    #[serde(default)]
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrantRecord {
    pub grant_id: String,
    pub machine_record_id: String,
    pub owner_user_id: String,
    pub pairing_token_hash: String,
    pub grant_scope: String,
    pub device_attach_public_key: String,
    pub installation_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthRecord {
    pub device_code_hash: String,
    pub user_code: String,
    pub email: String,
    pub email_token_hash: String,
    pub enrollment_code: Option<String>,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionRecord {
    pub subscription_id: String,
    pub owner_user_id: String,
    #[serde(default)]
    pub org_id: Option<String>,
    pub plan_key: String,
    pub seats: u32,
    pub host_packs: u32,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub ends_at: Option<u64>,
    #[serde(default)]
    pub ls_customer_id: Option<String>,
    pub ls_updated_at: u64,
    #[serde(default)]
    pub manage_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BillingStateRecord {
    pub owner_key: String,
    #[serde(default)]
    pub grace_started_at: Option<u64>,
    #[serde(default)]
    pub stopped_at: Option<u64>,
    #[serde(default)]
    pub last_notice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentEventRecord {
    pub event_key: String,
    pub event_name: String,
    pub received_at: u64,
    #[serde(default)]
    pub applied_at: Option<u64>,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrgRecord {
    pub org_id: String,
    pub owner_user_id: String,
    pub name: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrgMemberRecord {
    pub org_id: String,
    pub user_id: String,
    pub role: String,
    pub joined_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrgInviteRecord {
    pub token_hash: String,
    pub org_id: String,
    pub email: String,
    pub expires_at: u64,
}

pub const ACCOUNT_SIGNING_KEY_FILE: &str = "account-signing-key.json";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSigningKeyRecord {
    pub public_key: String,
    pub private_key: String,
}

impl std::fmt::Debug for AccountSigningKeyRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountSigningKeyRecord")
            .field("public_key", &self.public_key)
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountStore {
    #[serde(default)]
    pub users: BTreeMap<String, UserRecord>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionRecord>,
    #[serde(default)]
    pub login_codes: BTreeMap<String, LoginCodeRecord>,
    #[serde(default)]
    pub enrollment_codes: BTreeMap<String, EnrollmentCodeRecord>,
    #[serde(default)]
    pub machines: BTreeMap<String, MachineRecord>,
    #[serde(default)]
    pub grants: BTreeMap<String, GrantRecord>,
    #[serde(default)]
    pub device_auths: BTreeMap<String, DeviceAuthRecord>,
    #[serde(default)]
    pub subscriptions: BTreeMap<String, SubscriptionRecord>,
    #[serde(default)]
    pub billing_states: BTreeMap<String, BillingStateRecord>,
    #[serde(default)]
    pub payment_events: BTreeMap<String, PaymentEventRecord>,
    #[serde(default)]
    pub orgs: BTreeMap<String, OrgRecord>,
    #[serde(default)]
    pub org_members: BTreeMap<String, OrgMemberRecord>,
    #[serde(default)]
    pub org_invites: BTreeMap<String, OrgInviteRecord>,
}

impl AccountStore {
    pub fn load(dir: &Path) -> Result<Self, String> {
        self::store_sqlite::load_sqlite_store(dir)
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        self::store_sqlite::save_sqlite_store(dir, self)
    }

    pub fn mutate_transaction<T, E, F>(dir: &Path, mutate_fn: F) -> Result<T, E>
    where
        E: From<String>,
        F: FnOnce(&mut AccountStore) -> Result<T, E>,
    {
        self::store_sqlite::mutate_transaction(dir, mutate_fn)
    }

    pub fn purge_expired(&mut self, now: u64) {
        self.enrollment_codes
            .retain(|_, code| code.expires_at > now);
        self.sessions.retain(|_, session| session.expires_at > now);
        self.grants.retain(|_, grant| grant.expires_at > now);
        self.device_auths.retain(|_, auth| auth.expires_at > now);
        self.org_invites.retain(|_, invite| invite.expires_at > now);
    }

    pub fn user_by_email(&self, email: &str) -> Option<&UserRecord> {
        self.users.values().find(|user| user.email == email)
    }

    pub fn user_for_session(&self, bearer: &str, now: u64) -> Option<&UserRecord> {
        let session = self.sessions.get(&token_hash(bearer))?;
        if session.expires_at <= now {
            return None;
        }
        self.users.get(&session.user_id)
    }
}

pub fn store_path(dir: &Path) -> PathBuf {
    dir.join("account-store.json")
}

pub fn signing_key_path(dir: &Path) -> PathBuf {
    dir.join(ACCOUNT_SIGNING_KEY_FILE)
}

pub fn lock_account_dir(dir: &Path) -> Result<File, String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("Failed to create account data dir: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Failed to restrict account data dir: {error}"))?;
    }
    let path = dir.join("account-store.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| format!("Failed to open account lock: {error}"))?;
    file.lock()
        .map_err(|error| format!("Failed to lock account store: {error}"))?;
    Ok(file)
}

pub(crate) fn write_private_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        use std::io::Write;
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_store_round_trips_without_secrets() {
        let dir = tempfile::tempdir().expect("temp");
        let mut store = AccountStore::default();
        let token = random_token();
        store
            .login_codes
            .insert(token_hash(&token), LoginCodeRecord {
                email: "a@b.co".into(),
                expires_at: now_secs() + 600,
                consumed_at: None,
                login_handle_hash: None,
            });
        store.save(dir.path()).expect("save");
        let raw = std::fs::read_to_string(store_path(dir.path())).unwrap_or_default();
        assert!(!raw.contains(&token), "raw login token must not be persisted");
        let loaded = AccountStore::load(dir.path()).expect("load");
        assert_eq!(loaded.login_codes.len(), 1);

        store.login_codes.clear();
        store.login_codes.insert(token_hash("expired"), LoginCodeRecord {
            email: "a@b.co".into(),
            expires_at: now_secs() - 1,
            consumed_at: None,
            login_handle_hash: None,
        });
        store.sessions.insert(
            "session-hash".into(),
            SessionRecord {
                user_id: "u1".into(),
                expires_at: now_secs() - 1,
            },
        );
        store.enrollment_codes.insert(
            "code-hash".into(),
            EnrollmentCodeRecord {
                user_id: "u1".into(),
                account_origin: "https://account.example".into(),
                expires_at: now_secs() - 1,
            },
        );
        store.purge_expired(now_secs());
        assert!(store.sessions.is_empty(), "expired sessions are dropped");
        assert!(store.enrollment_codes.is_empty(), "expired enrollment codes are dropped");
        assert_eq!(
            store.login_codes.len(),
            1,
            "login codes stay so the consume path can report expiry instead of reuse"
        );
    }

    #[test]
    fn json_store_imports_once_and_is_renamed() {
        let dir = tempfile::tempdir().expect("temp");
        let json_file = dir.path().join("account-store.json");

        let mut initial_store = AccountStore::default();
        let user = UserRecord {
            user_id: "u_migrated".into(),
            email: "migrated@example.com".into(),
            created_at: 1000,
        };
        initial_store.users.insert(user.user_id.clone(), user);
        let machine = MachineRecord {
            machine_record_id: "mrec_1".into(),
            owner_user_id: "u_migrated".into(),
            machine_id: "m_id_1".into(),
            display_name: "Machine 1".into(),
            public_key: "pk".into(),
            attach_public_key: "apk".into(),
            relay_origin: "https://relay.example.com".into(),
            platform: "linux".into(),
            enrollment_epoch: 1,
            enrolled_at: 1000,
            last_seen_at: 1005,
        };
        initial_store
            .machines
            .insert(machine.machine_record_id.clone(), machine);

        let serialized = serde_json::to_vec_pretty(&initial_store).expect("json");
        std::fs::write(&json_file, serialized).expect("write json");
        assert!(json_file.exists());

        let loaded = AccountStore::load(dir.path()).expect("load should import legacy json");
        assert_eq!(loaded.users.len(), 1);
        assert_eq!(loaded.machines.len(), 1);
        assert!(loaded.users.contains_key("u_migrated"));
        assert!(loaded.machines.contains_key("mrec_1"));

        assert!(!json_file.exists(), "original json file must be renamed");
        let mut found_migrated = false;
        for entry in std::fs::read_dir(dir.path()).expect("read dir") {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("account-store.json.migrated-") {
                found_migrated = true;
                break;
            }
        }
        assert!(found_migrated, "migrated backup file must exist");

        let loaded_again = AccountStore::load(dir.path()).expect("second load from sqlite");
        assert_eq!(loaded_again.users.len(), 1);
        assert_eq!(loaded_again.machines.len(), 1);
    }

    #[test]
    fn legacy_corrupt_json_fails_and_preserves_file() {
        let dir = tempfile::tempdir().expect("temp");
        let json_file = dir.path().join("account-store.json");
        let bad_bytes = b"{\"users\": {corrupt_json";
        std::fs::write(&json_file, bad_bytes).expect("write corrupt json");

        let err = AccountStore::load(dir.path()).expect_err("must fail on corrupt json");
        assert!(err.contains("STORE_CORRUPT"), "error should be STORE_CORRUPT: {err}");
        assert!(json_file.exists(), "corrupt json file must be preserved untouched");
    }

    #[test]
    fn concurrent_mutations_serialize() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().to_path_buf();

        AccountStore::mutate_transaction::<(), String, _>(&path, |store| {
            store.users.insert(
                "counter_user".into(),
                UserRecord {
                    user_id: "counter_user".into(),
                    email: "counter@example.com".into(),
                    created_at: 0,
                },
            );
            Ok(())
        })
        .expect("init user");

        // Deterministic start + per-round rendezvous, built only from bounded event waits
        // (no sleeps, no polling, no barriers): thread 2 cannot receive round N's
        // announcement until thread 1 has reached round N, and thread 1 cannot enter round
        // N's `mutate_transaction` until thread 2 has answered, so neither writer can run
        // its 100 increments while the other sits unscheduled. The handshake synchronizes
        // the *attempt* only: whether the two immediate transactions then overlap is
        // SQLite's decision, checked directly by
        // `immediate_transaction_blocks_contending_writer_connection`. Every wait is bounded
        // (10 s) so a dead or broken peer fails the test instead of hanging it.
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();

        let path1 = path.clone();
        let handle1 = std::thread::spawn(move || {
            for _ in 0..100 {
                ready_tx.send(()).expect("thread 1 announces its round");
                go_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .expect("thread 1 must see thread 2's answer for this round within 10 s");
                AccountStore::mutate_transaction::<(), String, _>(&path1, |store| {
                    if let Some(user) = store.users.get_mut("counter_user") {
                        user.created_at += 1;
                    }
                    Ok(())
                })
                .expect("thread 1 mutate");
            }
        });

        let path2 = path.clone();
        let handle2 = std::thread::spawn(move || {
            for _ in 0..100 {
                ready_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .expect("thread 2 must see thread 1's round announcement within 10 s");
                go_tx.send(()).expect("thread 2 answers thread 1's round");
                AccountStore::mutate_transaction::<(), String, _>(&path2, |store| {
                    if let Some(user) = store.users.get_mut("counter_user") {
                        user.created_at += 1;
                    }
                    Ok(())
                })
                .expect("thread 2 mutate");
            }
        });

        handle1.join().expect("thread 1 finish");
        handle2.join().expect("thread 2 finish");

        let final_store = AccountStore::load(&path).expect("final load");
        assert_eq!(
            final_store.users.len(),
            1,
            "serialized mutations must update the existing row instead of duplicating it"
        );
        let user = final_store.users.get("counter_user").expect("counter_user exists");
        assert_eq!(user.email, "counter@example.com");
        assert_eq!(
            user.created_at, 200,
            "two threads of 100 increments must serialize to exactly 200: each immediate \
             transaction must serialize with its contending writer instead of losing a \
             read-modify-write"
        );
    }

    #[test]
    fn billing_records_round_trip() {
        let dir = tempfile::tempdir().expect("temp");
        let mut store = AccountStore::default();

        let user = UserRecord {
            user_id: "u_billing".into(),
            email: "billing@example.com".into(),
            created_at: 1000,
        };
        store.users.insert(user.user_id.clone(), user.clone());

        let org = OrgRecord {
            org_id: "org_1".into(),
            owner_user_id: "u_billing".into(),
            name: "Test Org".into(),
            created_at: 1001,
        };
        store.orgs.insert(org.org_id.clone(), org.clone());

        let member = OrgMemberRecord {
            org_id: "org_1".into(),
            user_id: "u_billing".into(),
            role: "owner".into(),
            joined_at: 1002,
        };
        store.org_members.insert("org_1:u_billing".into(), member.clone());

        let invite = OrgInviteRecord {
            token_hash: "invite_hash_1".into(),
            org_id: "org_1".into(),
            email: "invited@example.com".into(),
            expires_at: now_secs() + 3600,
        };
        store.org_invites.insert(invite.token_hash.clone(), invite.clone());

        let sub = SubscriptionRecord {
            subscription_id: "sub_1".into(),
            owner_user_id: "u_billing".into(),
            org_id: Some("org_1".into()),
            plan_key: "team_monthly".into(),
            seats: 5,
            host_packs: 2,
            kind: "base".into(),
            status: "active".into(),
            ends_at: Some(now_secs() + 86400 * 30),
            ls_customer_id: Some("ls_cust_123".into()),
            ls_updated_at: 1003,
            manage_url: Some("https://billing.example.com/manage".into()),
        };
        store.subscriptions.insert(sub.subscription_id.clone(), sub.clone());

        let bstate = BillingStateRecord {
            owner_key: "org:org_1".into(),
            grace_started_at: Some(1004),
            stopped_at: None,
            last_notice: Some("grace_started".into()),
        };
        store.billing_states.insert(bstate.owner_key.clone(), bstate.clone());

        let pevent = PaymentEventRecord {
            event_key: "pevent_hash_123".into(),
            event_name: "subscription_created".into(),
            received_at: 1005,
            applied_at: Some(1006),
            outcome: "applied".into(),
        };
        store.payment_events.insert(pevent.event_key.clone(), pevent.clone());

        store.save(dir.path()).expect("save sqlite");

        let loaded = AccountStore::load(dir.path()).expect("load sqlite");
        assert_eq!(loaded.users.len(), 1);
        assert_eq!(loaded.orgs.len(), 1);
        assert_eq!(loaded.org_members.len(), 1);
        assert_eq!(loaded.org_invites.len(), 1);
        assert_eq!(loaded.subscriptions.len(), 1);
        assert_eq!(loaded.billing_states.len(), 1);
        assert_eq!(loaded.payment_events.len(), 1);

        // Full-record equality: a codec that drops, coerces, or reorders any field
        // (nullable columns, org linkage, billing metadata, timestamps) must fail here even
        // though the per-table row counts above still match.
        assert_eq!(loaded.users.get("u_billing"), Some(&user));
        assert_eq!(loaded.orgs.get("org_1"), Some(&org));
        assert_eq!(loaded.org_members.get("org_1:u_billing"), Some(&member));
        assert_eq!(loaded.org_invites.get("invite_hash_1"), Some(&invite));
        assert_eq!(loaded.subscriptions.get("sub_1"), Some(&sub));
        assert_eq!(loaded.billing_states.get("org:org_1"), Some(&bstate));
        assert_eq!(loaded.payment_events.get("pevent_hash_123"), Some(&pevent));

        let mut expired_store = loaded;
        expired_store.org_invites.insert(
            "expired_invite".into(),
            OrgInviteRecord {
                token_hash: "expired_invite".into(),
                org_id: "org_1".into(),
                email: "expired@example.com".into(),
                expires_at: now_secs() - 10,
            },
        );
        expired_store.purge_expired(now_secs());
        assert!(!expired_store.org_invites.contains_key("expired_invite"));
        assert!(expired_store.org_invites.contains_key("invite_hash_1"));
    }

    #[test]
    fn concurrent_imports_do_not_duplicate_or_corrupt() {
        let dir = tempfile::tempdir().expect("temp");
        let json_file = dir.path().join("account-store.json");

        let mut initial_store = AccountStore::default();
        let user = UserRecord {
            user_id: "u_concurrent_import".into(),
            email: "concurrent@example.com".into(),
            created_at: 1000,
        };
        initial_store.users.insert(user.user_id.clone(), user);
        let serialized = serde_json::to_vec_pretty(&initial_store).expect("json");
        std::fs::write(&json_file, &serialized).expect("write json");

        // Deterministic start from bounded event waits: thread 2 cannot run its import until
        // thread 1 announced readiness, and thread 1 cannot run its import until thread 2 has
        // answered, so both imports are attempted together instead of one `load` finishing
        // first. The handshake synchronizes the attempt; serializing the import itself is the
        // production transaction's job, which the assertions below then check.
        let (first_ready_tx, first_ready_rx) = std::sync::mpsc::channel::<()>();
        let (second_go_tx, second_go_rx) = std::sync::mpsc::channel::<()>();

        let path1 = dir.path().to_path_buf();
        let path2 = dir.path().to_path_buf();

        let handle1 = std::thread::spawn(move || {
            first_ready_tx.send(()).expect("first importer announces readiness");
            second_go_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("first importer must be released within 10 s");
            AccountStore::load(&path1)
        });
        let handle2 = std::thread::spawn(move || {
            first_ready_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("second importer must see the first importer's readiness within 10 s");
            second_go_tx.send(()).expect("second importer releases the first");
            AccountStore::load(&path2)
        });

        let res1 = handle1.join().expect("t1 join").expect("t1 load");
        let res2 = handle2.join().expect("t2 join").expect("t2 load");

        assert_eq!(res1.users.len(), 1);
        assert_eq!(res2.users.len(), 1);
        assert_eq!(res1.users.get("u_concurrent_import").unwrap().email, "concurrent@example.com");
        assert_eq!(res2.users.get("u_concurrent_import").unwrap().email, "concurrent@example.com");

        assert!(!json_file.exists(), "the legacy json file must be consumed by the import");

        let mut migrated: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(dir.path()).expect("read dir") {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("account-store.json.migrated-") {
                migrated.push(name);
            }
        }
        assert_eq!(
            migrated.len(),
            1,
            "concurrent imports must consume the legacy json exactly once, got: {migrated:?}"
        );
        let migrated_bytes =
            std::fs::read(dir.path().join(&migrated[0])).expect("read migrated backup");
        assert_eq!(
            migrated_bytes, serialized,
            "the migrated backup must preserve the original legacy bytes"
        );

        // Durable state after both importers finished: a fresh load and a raw row count must
        // both see exactly one user.
        let settled = AccountStore::load(dir.path()).expect("fresh load after concurrent imports");
        assert_eq!(settled.users.len(), 1);
        assert!(settled.users.contains_key("u_concurrent_import"));

        let conn = store_sqlite::open_sqlite_connection(dir.path()).expect("raw connection");
        let user_rows: i64 = conn
            .query_row("SELECT count(*) FROM users", [], |row| row.get(0))
            .expect("count users rows");
        assert_eq!(
            user_rows, 1,
            "the import must not duplicate rows even when two loads race"
        );
    }

    // Deterministic serialization proof with no sleeps: while the production
    // `AccountStore::mutate_transaction` is inside its closure - i.e. while it holds the
    // immediate transaction - a separate connection that refuses to wait (busy timeout 0)
    // must be rejected with SQLITE_BUSY/SQLITE_LOCKED. Once the production transaction
    // commits, that same connection must acquire the write lock and its write must land,
    // together with the row the production transaction wrote. The lock under test therefore
    // belongs to the production API, not to a hand-rolled transaction: changing
    // `mutate_transaction` from `Immediate` to a deferred transaction lets the contender
    // acquire the lock while the closure runs, which turns this test RED.
    #[test]
    fn immediate_transaction_blocks_contending_writer_connection() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().to_path_buf();

        // Initialize schema and WAL before any lock exists, so the probe can only ever
        // report lock contention and never a setup failure.
        AccountStore::load(&path).expect("initialize sqlite store");

        let mut probe_conn =
            store_sqlite::open_sqlite_connection(&path).expect("contender connection");
        probe_conn
            .busy_timeout(std::time::Duration::from_millis(0))
            .expect("the contender must not wait for the write lock");

        let (holding_tx, holding_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();

        let holder_path = path.clone();
        let holder = std::thread::spawn(move || {
            // The holder is the production mutation path: the closure runs inside
            // `BEGIN IMMEDIATE .. COMMIT`, so the write lock is held for the whole closure.
            AccountStore::mutate_transaction::<(), String, _>(&holder_path, move |store| {
                store.users.insert(
                    "holder_user".into(),
                    UserRecord {
                        user_id: "holder_user".into(),
                        email: "holder@example.com".into(),
                        created_at: 1,
                    },
                );
                holding_tx
                    .send(())
                    .expect("production transaction signals that it holds the write lock");
                // The production transaction waits only for the probe verdict, and the
                // contender cannot block either (zero busy timeout), so no cycle exists. The
                // wait is bounded, which keeps a broken rendezvous from hanging the suite.
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .expect("production transaction must be released within 10 s");
                Ok(())
            })
            .expect("holder mutate_transaction commits");
        });

        holding_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("production transaction must hold the write lock within 10 s");

        {
            // The borrow of `probe_conn` is confined to this block so the post-release
            // attempt below cannot collide with a still-live `Result<Transaction<'_>, _>`.
            let contended =
                probe_conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate);
            match contended {
                Ok(_) => panic!(
                    "contender acquired the write lock while the production immediate \
                     transaction held it"
                ),
                Err(rusqlite::Error::SqliteFailure(inner, _))
                    if matches!(
                        inner.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    ) => {}
                Err(other) => panic!(
                    "contender must fail with SQLITE_BUSY/SQLITE_LOCKED while the production \
                     transaction holds the write lock, got: {other:?}"
                ),
            }
        }

        release_tx.send(()).expect("release the production transaction");
        holder.join().expect("holder thread joins");

        // The other half of the guarantee: the same contender connection must now acquire
        // the write lock, and both the production row and the contending row must be
        // readable afterwards.
        let tx = probe_conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .expect("contender acquires the write lock once the production transaction committed");
        tx.execute(
            "INSERT INTO users (user_id, email, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params!["contender_user", "contender@example.com", 2_i64],
        )
        .expect("contender writes after the lock is released");
        tx.commit().expect("contender commits");

        let loaded = AccountStore::load(&path).expect("load after the probe");
        let holder_user = loaded
            .users
            .get("holder_user")
            .expect("the production transaction's row must be visible");
        assert_eq!(holder_user.email, "holder@example.com");
        assert_eq!(holder_user.created_at, 1);
        assert!(
            loaded.users.contains_key("contender_user"),
            "the contender's post-release write must be visible"
        );
        assert_eq!(loaded.users.len(), 2);
    }

    #[test]
    fn crash_resume_after_db_commit_does_not_overwrite_newer_records() {
        let dir = tempfile::tempdir().expect("temp");
        let json_file = dir.path().join("account-store.json");

        let mut legacy_store = AccountStore::default();
        let old_user = UserRecord {
            user_id: "u_legacy".into(),
            email: "old@example.com".into(),
            created_at: 1000,
        };
        legacy_store.users.insert(old_user.user_id.clone(), old_user);
        let serialized = serde_json::to_vec_pretty(&legacy_store).expect("json");
        std::fs::write(&json_file, &serialized).expect("write json");

        let first_load = AccountStore::load(dir.path()).expect("first load imports legacy");
        assert_eq!(first_load.users.len(), 1);

        AccountStore::mutate_transaction::<(), String, _>(dir.path(), |store| {
            store.users.insert(
                "u_newer".into(),
                UserRecord {
                    user_id: "u_newer".into(),
                    email: "newer@example.com".into(),
                    created_at: 2000,
                },
            );
            Ok(())
        })
        .expect("mutate to add newer user");

        std::fs::write(&json_file, serialized).expect("simulate unrenamed legacy file or restore");

        let resumed_load = AccountStore::load(dir.path()).expect("resumed load must not overwrite");
        assert_eq!(resumed_load.users.len(), 2, "newer user in sqlite must not be wiped by unrenamed legacy file");
        assert!(resumed_load.users.contains_key("u_legacy"));
        assert!(resumed_load.users.contains_key("u_newer"));
        assert_eq!(resumed_load.users.get("u_newer").unwrap().email, "newer@example.com");

        assert!(!json_file.exists(), "stray json file should be cleaned up / renamed");
    }

    #[test]
    fn unrenamed_legacy_json_does_not_overwrite_already_initialized_db() {
        let dir = tempfile::tempdir().expect("temp");
        let json_file = dir.path().join("account-store.json");

        AccountStore::mutate_transaction::<(), String, _>(dir.path(), |store| {
            store.users.insert(
                "fresh_db_user".into(),
                UserRecord {
                    user_id: "fresh_db_user".into(),
                    email: "fresh@example.com".into(),
                    created_at: 5000,
                },
            );
            Ok(())
        })
        .expect("create fresh db directly");

        let mut legacy_store = AccountStore::default();
        let old_user = UserRecord {
            user_id: "stray_legacy_user".into(),
            email: "stray@example.com".into(),
            created_at: 10,
        };
        legacy_store.users.insert(old_user.user_id.clone(), old_user);
        let serialized = serde_json::to_vec_pretty(&legacy_store).expect("json");
        std::fs::write(&json_file, serialized).expect("drop stray legacy file next to active db");

        let loaded = AccountStore::load(dir.path()).expect("load should protect active db");
        assert_eq!(loaded.users.len(), 1, "active DB must never be overwritten by stray json");
        assert!(loaded.users.contains_key("fresh_db_user"));
        assert!(!loaded.users.contains_key("stray_legacy_user"));
        assert!(!json_file.exists(), "stray json file should be renamed away");
    }

    #[test]
    fn account_signing_key_record_debug_redacts_private_key() {
        let record = AccountSigningKeyRecord {
            public_key: "pub".into(),
            private_key: "secret".into(),
        };
        let formatted = format!("{record:?}");
        assert!(!formatted.contains("secret"));
        assert!(formatted.contains("[REDACTED]"));
    }
}
