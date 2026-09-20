//! Real-process-shaped tests for the helper-owned DAG subscription stream.
//!
//! Every wait is an event: either a condvar-backed "consumer is parked" barrier
//! or a bounded channel receive. No test sleeps.
use super::super::{Request, Runtime};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{mpsc, Arc},
    time::Duration,
};

const BOUND: Duration = Duration::from_secs(10);

fn runtime_with_project(token: &str) -> (Runtime, tempfile::TempDir, tempfile::TempDir) {
    let runtime_dir = tempfile::tempdir().expect("runtime dir");
    let project_dir = tempfile::tempdir().expect("project dir");
    let runtime = Runtime::new(
        runtime_dir.path().to_path_buf(),
        "test-host".to_string(),
        token.to_string(),
    )
    .expect("runtime");
    call(
        &runtime,
        token,
        "project.register",
        json!({ "id": "project", "path": project_dir.path().to_string_lossy() }),
    )
    .expect("register project");
    (runtime, runtime_dir, project_dir)
}

fn call(runtime: &Runtime, token: &str, op: &str, params: Value) -> Result<Value, String> {
    runtime.handle(Request {
        protocol: 1,
        token: token.to_string(),
        op: op.to_string(),
        params,
    })
}

fn runs_dir(project: &Path) -> std::path::PathBuf {
    let dir = project.join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&dir).expect("create runs dir");
    dir
}

fn write_checkpoint(dir: &Path, name: &str, body: &Value) {
    std::fs::write(dir.join(name), serde_json::to_vec(body).expect("encode")).expect("write");
}

fn run_ids(frame: &Value) -> Vec<String> {
    frame["runs"]
        .as_array()
        .expect("runs array")
        .iter()
        .map(|r| r["runId"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Spawns a blocking `dag.next` on a worker thread and returns once the helper
/// reports a parked consumer, so the mutation that follows is observed by the
/// subscription's watcher rather than raced against it.
fn park_next(
    runtime: &Arc<Runtime>,
    token: &str,
    subscription_id: &str,
    wait_ms: u64,
) -> mpsc::Receiver<Result<Value, String>> {
    let (tx, rx) = mpsc::channel();
    let worker = runtime.clone();
    let token = token.to_string();
    let id = subscription_id.to_string();
    std::thread::spawn(move || {
        let result = call(
            &worker,
            &token,
            "dag.next",
            json!({ "subscriptionId": id, "waitMs": wait_ms }),
        );
        let _ = tx.send(result);
    });
    assert!(
        runtime
            .dag_streams()
            .wait_until_waiting(subscription_id, BOUND),
        "consumer never parked inside dag.next"
    );
    rx
}

#[test]
fn dag_subscribe_streams_inventory_then_asynchronous_update() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-stream");
    let runtime = Arc::new(runtime);
    let dir = runs_dir(project_dir.path());
    write_checkpoint(
        &dir,
        "run-a.json",
        &json!({ "runId": "run-a", "status": "running" }),
    );

    let first = call(
        &runtime,
        "tok-stream",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");
    let id = first["subscriptionId"].as_str().expect("id").to_string();
    assert_eq!(first["projectId"], "project");
    assert_eq!(first["resync"], true);
    assert_eq!(run_ids(&first), vec!["run-a".to_string()]);

    // Asynchronous: the consumer blocks, the helper's own watcher wakes it.
    let pending = park_next(&runtime, "tok-stream", &id, 5_000);
    write_checkpoint(
        &dir,
        "run-b.json",
        &json!({ "runId": "run-b", "status": "running" }),
    );
    let update = pending
        .recv_timeout(BOUND)
        .expect("update frame")
        .expect("ok");
    assert_eq!(run_ids(&update), vec!["run-b".to_string()]);
    assert_eq!(update["resync"], false);
    assert!(
        update["sequence"].as_u64().unwrap() > first["sequence"].as_u64().unwrap(),
        "sequence must advance"
    );

    call(
        &runtime,
        "tok-stream",
        "dag.unsubscribe",
        json!({ "subscriptionId": id }),
    )
    .expect("unsubscribe");
}

#[test]
fn dag_subscribe_emits_same_mtime_content_change_without_known_runs() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-same-mtime");
    let runtime = Arc::new(runtime);
    let dir = runs_dir(project_dir.path());
    let path = dir.join("run.json");
    write_checkpoint(
        &dir,
        "run.json",
        &json!({ "runId": "same-run", "status": "running" }),
    );
    let original = std::fs::metadata(&path).unwrap().modified().unwrap();

    let first = call(
        &runtime,
        "tok-same-mtime",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");
    let id = first["subscriptionId"].as_str().expect("id").to_string();
    assert_eq!(first["runs"][0]["status"], "running");

    let pending = park_next(&runtime, "tok-same-mtime", &id, 5_000);
    write_checkpoint(
        &dir,
        "run.json",
        &json!({ "runId": "same-run", "status": "completed" }),
    );
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(original))
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        original,
        "fixture must keep the original mtime"
    );

    let update = pending
        .recv_timeout(BOUND)
        .expect("update frame")
        .expect("ok");
    assert_eq!(update["runs"][0]["runId"], "same-run");
    assert_eq!(update["runs"][0]["status"], "completed");

    call(
        &runtime,
        "tok-same-mtime",
        "dag.unsubscribe",
        json!({ "subscriptionId": id }),
    )
    .expect("unsubscribe");
}

#[test]
fn dag_subscribe_suppresses_unchanged_checkpoint_rewrite() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-dedup");
    let runtime = Arc::new(runtime);
    let dir = runs_dir(project_dir.path());
    let snapshot = json!({ "runId": "dedup-run", "status": "running" });
    write_checkpoint(&dir, "run.json", &snapshot);

    let first = call(
        &runtime,
        "tok-dedup",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");
    let id = first["subscriptionId"].as_str().expect("id").to_string();
    assert_eq!(run_ids(&first), vec!["dedup-run".to_string()]);

    // Byte-identical rewrite (new mtime) must not produce a frame; the parked
    // consumer times out empty instead.
    let pending = park_next(&runtime, "tok-dedup", &id, 400);
    write_checkpoint(&dir, "run.json", &snapshot);
    let idle = pending
        .recv_timeout(BOUND)
        .expect("idle frame")
        .expect("ok");
    assert!(
        idle["runs"].as_array().unwrap().is_empty(),
        "unchanged content must not be redelivered: {idle}"
    );

    // A real change still flows on the same subscription.
    let pending = park_next(&runtime, "tok-dedup", &id, 5_000);
    write_checkpoint(
        &dir,
        "run.json",
        &json!({ "runId": "dedup-run", "status": "failed" }),
    );
    let update = pending
        .recv_timeout(BOUND)
        .expect("update frame")
        .expect("ok");
    assert_eq!(update["runs"][0]["status"], "failed");

    call(
        &runtime,
        "tok-dedup",
        "dag.unsubscribe",
        json!({ "subscriptionId": id }),
    )
    .expect("unsubscribe");
}

#[test]
fn dag_unsubscribe_wakes_parked_consumer_and_releases_subscription() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-cancel");
    let runtime = Arc::new(runtime);
    runs_dir(project_dir.path());

    let first = call(
        &runtime,
        "tok-cancel",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");
    let id = first["subscriptionId"].as_str().expect("id").to_string();
    assert_eq!(runtime.dag_streams().subscription_count(), 1);

    let pending = park_next(&runtime, "tok-cancel", &id, 10_000);
    let closed = call(
        &runtime,
        "tok-cancel",
        "dag.unsubscribe",
        json!({ "subscriptionId": id }),
    )
    .expect("unsubscribe");
    assert_eq!(closed["closed"], true);

    let final_frame = pending
        .recv_timeout(BOUND)
        .expect("cancellation must wake the parked consumer")
        .expect("ok");
    assert_eq!(final_frame["closed"], true);
    assert_eq!(runtime.dag_streams().subscription_count(), 0);

    let after = call(
        &runtime,
        "tok-cancel",
        "dag.next",
        json!({ "subscriptionId": id, "waitMs": 0 }),
    );
    assert_eq!(after.unwrap_err(), "NOT_FOUND");
}

#[test]
fn dag_subscribe_rejects_unregistered_project() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(
        runtime_dir.path().to_path_buf(),
        "test-host".to_string(),
        "tok-unregistered".to_string(),
    )
    .unwrap();
    let result = call(
        &runtime,
        "tok-unregistered",
        "dag.subscribe",
        json!({ "projectId": project_dir.path().to_string_lossy() }),
    );
    assert_eq!(result.unwrap_err(), "NOT_FOUND");
}

/// Deterministic reproduction of the watcher/delivery overlap: a name that
/// delivery has already recorded is force-re-enqueued, exactly as the watcher
/// would when it rescans between the pop and the hash write. The unchanged
/// checkpoint must not be emitted a second time.
#[test]
fn dag_subscribe_does_not_redeliver_concurrently_requeued_unchanged_file() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-overlap");
    let runtime = Arc::new(runtime);
    let dir = runs_dir(project_dir.path());
    write_checkpoint(
        &dir,
        "run.json",
        &json!({ "runId": "overlap-run", "status": "running" }),
    );

    let first = call(
        &runtime,
        "tok-overlap",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");
    let id = first["subscriptionId"].as_str().expect("id").to_string();
    assert_eq!(run_ids(&first), vec!["overlap-run".to_string()]);

    // Simulate the overlapping watcher enqueue without touching the file.
    assert!(runtime.dag_streams().force_pending(&id, "run.json"));

    let frame = call(
        &runtime,
        "tok-overlap",
        "dag.next",
        json!({ "subscriptionId": id, "waitMs": 0 }),
    )
    .expect("next");
    assert!(
        frame["runs"].as_array().unwrap().is_empty(),
        "a re-enqueued unchanged file must not be redelivered: {frame}"
    );

    call(
        &runtime,
        "tok-overlap",
        "dag.unsubscribe",
        json!({ "subscriptionId": id }),
    )
    .expect("unsubscribe");
}

/// Many unreadable checkpoints must not produce an unbounded `dropped` list.
#[test]
fn dag_subscribe_bounds_dropped_reports_per_frame() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-dropped-bound");
    let dir = runs_dir(project_dir.path());
    let invalid_count = super::MAX_DROPPED_PER_FRAME * 2;
    for index in 0..invalid_count {
        std::fs::write(dir.join(format!("bad-{index:03}.json")), b"{not json").expect("write");
    }

    let frame = call(
        &runtime,
        "tok-dropped-bound",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");
    let id = frame["subscriptionId"].as_str().unwrap().to_string();
    let dropped = frame["dropped"].as_array().expect("dropped");
    assert!(
        dropped.len() <= super::MAX_DROPPED_PER_FRAME,
        "dropped list must be bounded, got {}",
        dropped.len()
    );
    assert_eq!(
        frame["more"], true,
        "remaining invalid files must stay pending: {frame}"
    );

    // The remainder drains across subsequent frames, still bounded each time.
    let mut seen = dropped.len();
    for _ in 0..8 {
        if seen >= invalid_count {
            break;
        }
        let next = call(
            &runtime,
            "tok-dropped-bound",
            "dag.next",
            json!({ "subscriptionId": id, "waitMs": 200 }),
        )
        .expect("next");
        let batch = next["dropped"].as_array().expect("dropped").len();
        assert!(batch <= super::MAX_DROPPED_PER_FRAME);
        seen += batch;
    }
    assert_eq!(seen, invalid_count, "every invalid file must be reported");

    call(
        &runtime,
        "tok-dropped-bound",
        "dag.unsubscribe",
        json!({ "subscriptionId": id }),
    )
    .expect("unsubscribe");
}

#[test]
fn dag_subscribe_reports_oversized_snapshot_instead_of_truncating() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-oversize");
    let dir = runs_dir(project_dir.path());
    write_checkpoint(
        &dir,
        "small.json",
        &json!({ "runId": "small-run", "status": "running" }),
    );
    let huge = json!({
        "runId": "huge-run",
        "status": "running",
        "filler": "x".repeat(super::MAX_SNAPSHOT_BYTES as usize + 1),
    });
    write_checkpoint(&dir, "huge.json", &huge);

    let frame = call(
        &runtime,
        "tok-oversize",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");
    assert_eq!(run_ids(&frame), vec!["small-run".to_string()]);
    let dropped = frame["dropped"].as_array().expect("dropped array");
    assert_eq!(
        dropped.len(),
        1,
        "oversized snapshot must be reported: {frame}"
    );
    assert_eq!(dropped[0]["file"], "huge.json");
    assert_eq!(dropped[0]["error"], "SNAPSHOT_TOO_LARGE");
    assert!(
        serde_json::to_vec(&frame).unwrap().len() < super::super::MAX_FRAME,
        "frame must stay inside the helper frame bound"
    );

    call(
        &runtime,
        "tok-oversize",
        "dag.unsubscribe",
        json!({ "subscriptionId": frame["subscriptionId"].as_str().unwrap() }),
    )
    .expect("unsubscribe");
}

#[test]
fn dag_inventory_drains_pending_files_before_advancing_resync_cursor() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-inventory-pages");
    let dir = runs_dir(project_dir.path());
    let expected = super::MAX_PENDING_FILES + 3;
    for index in 0..expected {
        write_checkpoint(
            &dir,
            &format!("run-{index:03}.json"),
            &json!({
                "runId": format!("run-{index:03}"),
                "status": "running",
                "name": "x".repeat(100_000),
            }),
        );
    }
    let mut frame = call(
        &runtime,
        "tok-inventory-pages",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .unwrap();
    let id = frame["subscriptionId"].as_str().unwrap().to_string();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..expected {
        seen.extend(run_ids(&frame));
        if frame["more"] != true {
            break;
        }
        frame = call(
            &runtime,
            "tok-inventory-pages",
            "dag.next",
            json!({ "subscriptionId": id, "waitMs": 0 }),
        )
        .unwrap();
    }
    call(
        &runtime,
        "tok-inventory-pages",
        "dag.unsubscribe",
        json!({ "subscriptionId": id }),
    )
    .unwrap();
    assert_eq!(
        seen.len(),
        expected,
        "paged inventory lost pending checkpoints"
    );
}
