//! Archive on first standby, adoptable listing and owner-triggered adoption (plan todo 13).
//! Files live in per-test temp directories; nothing waits on time.
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::*;
use crate::checkpoint::{export, ExportRequest, SUPERVISOR_IDENTITY_MISMATCH};
use crate::checkpoint_import::{default_private_writer, import, ImportOptions};
use crate::supervisor::provision_owner;
use crate::supervisor::request::{self, RequestContext};
use crate::supervisor::ChannelProvider;

const OWNER: &str = "owner:u1";
const REPO: &str = "github.com/example/ferryx";
const NOW: i64 = 1_800_000_000;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferryx-adopt-{name}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn context() -> RequestContext {
    RequestContext {
        principal: OWNER.into(),
        adapter: ChannelProvider::Desktop,
        channel_binding_id: None,
        control: None,
        memory: None,
    }
}

async fn provisioned(pool: &SqlitePool, host: &str) -> String {
    let (identity, _, _) = provision_owner(pool, OWNER, "ferryx", REPO, host, "/repo/.git")
        .await
        .unwrap();
    identity.supervisor_id
}

/// Submits one goal through the shared request service and returns its goal id.
async fn submit(pool: &SqlitePool, request_id: &str, text: &str) -> String {
    let answer = request::handle(
        pool,
        &context(),
        json!({"requestId": request_id, "action": "submit", "payload": {
            "request": text,
            "work": {"title": format!("Goal {request_id}"), "instruction": text,
                "requirements": [{"id": "r1", "text": "result.txt contains READY", "verifier": "verify.sh exits 0"}],
                "operations": ["edit", "test"]}}}),
    )
    .await
    .unwrap();
    assert_eq!(answer["outcome"], "admitted", "{answer}");
    answer["receipt"]["goal_id"].as_str().unwrap().to_owned()
}

/// A second machine's hub with its own identity: one open goal and one completed goal.
async fn second_machine(dir: &Path) -> (String, String, String) {
    let pool = crate::storage::connect(&dir.join(LIVE_DB)).await.unwrap();
    let supervisor_id = provisioned(&pool, "local").await;
    let open = submit(&pool, "slack-1", "Create result.txt containing READY.").await;
    let done = submit(&pool, "slack-2", "Write the release notes.").await;
    sqlx::query("UPDATE supervisor_goals SET run_state = 'completed' WHERE goal_id = ?")
        .bind(&done)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    (supervisor_id, open, done)
}

async fn goal_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_goals")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn fingerprint(path: &Path) -> (Vec<u8>, std::time::SystemTime) {
    (
        fs::read(path).unwrap(),
        fs::metadata(path).unwrap().modified().unwrap(),
    )
}

#[tokio::test]
async fn standby_archives_once_and_lists_only_open_goals() {
    let dir = temp_dir("archive");
    let (supervisor_id, open, done) = second_machine(&dir).await;
    let live_before = fs::read(dir.join(LIVE_DB)).unwrap();

    let archive = archive_before_replace(&dir, NOW).unwrap().expect("first standby archives");
    assert_eq!(archive, dir.join(format!("{ARCHIVE_PREFIX}{NOW}")));
    assert_eq!(fs::read(&archive).unwrap(), live_before);
    assert!(dir.join(LIVE_DB).exists(), "the live database is never moved or deleted");
    // A second standby (another daemon start) is a no-op.
    assert_eq!(archive_before_replace(&dir, NOW + 60).unwrap(), None);
    assert!(!dir.join(format!("{ARCHIVE_PREFIX}{}", NOW + 60)).exists());

    let listed = adoptable_goals(&dir, "Laptop B").await.unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].goal_id, open);
    assert_eq!(listed[0].from_machine, "Laptop B");
    assert_eq!(listed[0].supervisor_id, supervisor_id);
    assert_eq!(listed[0].title, "Goal slack-1");
    assert!(listed.iter().all(|goal| goal.goal_id != done));
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_hub_without_a_database_or_archive_has_nothing_to_archive_or_adopt() {
    let dir = temp_dir("empty");
    assert_eq!(archive_before_replace(&dir, NOW).unwrap(), None);
    assert!(adoptable_goals(&dir, "Laptop B").await.unwrap().is_empty());
    assert_eq!(
        adopt_archived(&dir, &AdoptSelection::All, |_| async { Ok(json!({})) })
            .await
            .unwrap_err()
            .code(),
        "NOT_FOUND"
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// Adopt sends each open goal once under `archived:<supervisor>:<goal>`; a re-adopt returns
/// the holder's same goal (the source id is the idempotency key) and the archive is never
/// written. Dropping the source id from the envelope makes the re-adopt admit a second goal.
#[tokio::test]
async fn adopt_submits_each_goal_once_and_re_adopt_is_idempotent() {
    let dir = temp_dir("adopt");
    let (supervisor_id, open, _) = second_machine(&dir).await;
    let archive = archive_before_replace(&dir, NOW).unwrap().unwrap();
    let archived = fingerprint(&archive);

    let holder = crate::storage::memory().await.unwrap();
    let holder_supervisor = provisioned(&holder, "machine:laptop-a").await;
    assert_ne!(holder_supervisor, supervisor_id, "two identities before adoption");
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let send = |envelope: Value| {
        let pool = holder.clone();
        let sent = std::sync::Arc::clone(&sent);
        async move {
            sent.lock().unwrap().push(envelope.clone());
            request::handle(&pool, &context(), envelope).await
        }
    };

    let first = adopt_archived(&dir, &AdoptSelection::All, send).await.unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].adopted, "{:?}", first[0]);
    let source = format!("archived:{supervisor_id}:{open}");
    assert_eq!(first[0].source_id, source);
    assert_eq!(sent.lock().unwrap()[0]["requestId"], source.as_str());
    let holder_goal = first[0].answer.as_ref().unwrap()["receipt"]["goal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(goal_count(&holder).await, 1);
    let (body, owner_supervisor): (String, String) = sqlx::query_as(
        "SELECT body, supervisor_id FROM supervisor_requests WHERE source_message_id = ?",
    )
    .bind(&source)
    .fetch_one(&holder)
    .await
    .unwrap();
    assert_eq!(body, "Create result.txt containing READY.", "the original request text");
    assert_eq!(owner_supervisor, holder_supervisor, "adopted into the holder's identity");

    // Recorded in the ledger, so no longer adoptable; `--all` sends nothing again.
    let ledger = adopted(&dir).unwrap();
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].holder_goal_id.as_deref(), Some(holder_goal.as_str()));
    assert!(adoptable_goals(&dir, "Laptop B").await.unwrap().is_empty());
    let again = adopt_archived(&dir, &AdoptSelection::All, send).await.unwrap();
    assert!(again.is_empty());

    // An explicit re-adopt reaches the holder and returns the same goal.
    let re = adopt_archived(&dir, &AdoptSelection::Goal(open.clone()), send)
        .await
        .unwrap();
    assert_eq!(re.len(), 1);
    let answer = re[0].answer.as_ref().unwrap();
    assert_eq!(answer["duplicate"], true, "{answer}");
    assert_eq!(answer["receipt"]["goal_id"], holder_goal.as_str());
    assert_eq!(goal_count(&holder).await, 1, "a re-adopt admits nothing new");
    assert_eq!(adopted(&dir).unwrap().len(), 1);

    assert_eq!(fingerprint(&archive), archived, "the archive is never written");
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_failed_send_is_not_recorded_and_stays_adoptable() {
    let dir = temp_dir("failed");
    let (_, open, _) = second_machine(&dir).await;
    archive_before_replace(&dir, NOW).unwrap().unwrap();
    let refused = adopt_archived(&dir, &AdoptSelection::All, |_| async {
        Err(crate::holder::standby_failure(Some("laptop-a"), Some("Laptop A")))
    })
    .await
    .unwrap();
    assert_eq!(refused.len(), 1);
    assert!(!refused[0].adopted);
    assert_eq!(refused[0].error.as_ref().unwrap()["code"], "SUPERVISOR_STANDBY");
    assert!(adopted(&dir).unwrap().is_empty());
    let listed = adoptable_goals(&dir, "Laptop B").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].goal_id, open);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn only_archived_source_ids_parse() {
    assert_eq!(
        parse_archived_source("archived:sup-1:goal-1"),
        Some(("sup-1".to_owned(), "goal-1".to_owned()))
    );
    for other in ["slack-1", "archived:", "archived:sup", "archived::g", "archived:s:g:x"] {
        assert_eq!(parse_archived_source(other), None, "{other}");
    }
}

/// The `--replace-local` move path: a target holding another supervisor refuses the import
/// with `SUPERVISOR_IDENTITY_MISMATCH` until it archives itself; after
/// `archive_before_replace` the same import stages.
#[tokio::test]
async fn replace_local_archives_then_imports() {
    let root = temp_dir("replace");
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    let pool = crate::storage::connect(&source.join(LIVE_DB)).await.unwrap();
    provisioned(&pool, "local").await;
    let exported = export(
        &pool,
        &ExportRequest::new(
            source.clone(),
            "machine-a".into(),
            root.join("checkpoint.tar.zst"),
            default_private_writer(),
        ),
    )
    .await
    .unwrap();
    pool.close().await;

    let target = root.join("target");
    fs::create_dir_all(&target).unwrap();
    second_machine(&target).await;
    let options = || ImportOptions {
        memory_root: root.join("memory-agents"),
        env_file: target.join("env"),
        profile_omo_jsonc: None,
        now_ts: NOW,
        write_private: default_private_writer(),
    };
    let refused = import(&exported.path, &target, "machine-b", &[], options())
        .await
        .unwrap_err();
    assert_eq!(refused.code(), SUPERVISOR_IDENTITY_MISMATCH);

    let archive = archive_before_replace(&target, NOW).unwrap().unwrap();
    import(&exported.path, &target, "machine-b", &[], options())
        .await
        .unwrap();
    assert!(target.join("hub.sqlite.incoming").exists(), "the checkpoint is staged");
    assert!(archive.exists(), "the archive stays");
    assert_eq!(adoptable_goals(&target, "Laptop B").await.unwrap().len(), 1);
    fs::remove_dir_all(&root).unwrap();
}
