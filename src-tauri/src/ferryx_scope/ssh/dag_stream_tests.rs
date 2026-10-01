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

fn canonical_checkpoint(run_id: &str, status: &str) -> Value {
    json!({
        "runId": run_id,
        "runKey": format!("key-{run_id}"),
        "name": format!("name-{run_id}"),
        "status": status,
        "nodes": [
            {
                "id": "node-1",
                "label": "Node 1",
                "prompt": "Prompt 1",
                "state": status,
                "route": { "kind": "category", "category": "deep-low" }
            }
        ]
    })
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
        &canonical_checkpoint("run-a", "running"),
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
        &canonical_checkpoint("run-b", "running"),
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
        &canonical_checkpoint("same-run", "running"),
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
        &canonical_checkpoint("same-run", "completed"),
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
    let snapshot = canonical_checkpoint("dedup-run", "running");
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
        &canonical_checkpoint("dedup-run", "failed"),
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
        &canonical_checkpoint("overlap-run", "running"),
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
        &canonical_checkpoint("small-run", "running"),
    );
    let huge = json!({
        "runId": "huge-run",
        "runKey": "key-huge",
        "name": "huge",
        "status": "running",
        "nodes": [],
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
                "runKey": format!("key-{index:03}"),
                "name": format!("name-{index:03}"),
                "status": "running",
                "nodes": [
                    {
                        "id": "node-1",
                        "prompt": "x".repeat(50_000),
                        "state": "running",
                        "route": { "kind": "category", "category": "quick" }
                    }
                ]
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

#[test]
fn dag_stream_projects_large_raw_checkpoint_with_padding_definition() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-large-projected");
    let dir = runs_dir(project_dir.path());

    // Construct a checkpoint with >300KiB of raw padding definition (e.g. 618KiB total),
    // which previously exceeded the 256KiB raw limit.
    // The projection via parse_run_checkpoint strips `definition` / extra unmodeled fields,
    // preserving id, prompt, run_stats, amends, while producing a compact frame <300KiB.
    let large_raw_definition = "A".repeat(400 * 1024);
    let checkpoint = json!({
        "runId": "large-run-618k",
        "runKey": "key-large",
        "name": "large-test-run",
        "status": "running",
        "definition": large_raw_definition,
        "nodes": [
            {
                "id": "node-1",
                "label": "First Node",
                "prompt": "Analyze repository and produce plan",
                "state": "completed",
                "route": { "kind": "category", "category": "deep-low" },
                "runStats": { "turns": 3, "totalTokens": 15000 }
            },
            {
                "id": "node-2",
                "label": "Second Node",
                "prompt": "Execute verification and regression tests",
                "state": "running",
                "route": { "kind": "category", "category": "quick" },
                "dependsOn": ["node-1"]
            }
        ],
        "edges": [{ "from": "node-1", "to": "node-2" }],
        "waves": [
            { "index": 0, "nodeIds": ["node-1"] },
            { "index": 1, "nodeIds": ["node-2"] }
        ]
    });

    write_checkpoint(&dir, "large_run.json", &checkpoint);

    let raw_bytes = std::fs::metadata(dir.join("large_run.json")).unwrap().len();
    assert!(
        raw_bytes > 256 * 1024,
        "raw checkpoint must be >256KiB, was {raw_bytes}"
    );
    assert!(
        raw_bytes < super::MAX_INPUT_FILE_BYTES,
        "raw checkpoint must be within input limit"
    );

    let frame = call(
        &runtime,
        "tok-large-projected",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");

    assert_eq!(
        run_ids(&frame),
        vec!["large-run-618k".to_string()],
        "projected snapshot must be delivered in runs"
    );
    assert!(
        frame["dropped"].as_array().unwrap().is_empty(),
        "checkpoint should not be dropped: {:?}",
        frame["dropped"]
    );

    let run = &frame["runs"][0];
    assert_eq!(run["runId"], "large-run-618k");
    // Definition must be stripped by projection
    assert!(
        run.get("definition").is_none(),
        "raw definition field must be omitted in projected frame"
    );
    // Node fields must be preserved
    let nodes = run["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["prompt"], "Analyze repository and produce plan");
    assert_eq!(nodes[0]["runStats"]["totalTokens"], 15000);
    assert_eq!(nodes[1]["prompt"], "Execute verification and regression tests");

    // Frame serialized size must be compact (<300KiB) and well within 512KiB frame budget
    let frame_bytes = serde_json::to_vec(&frame).unwrap().len();
    assert!(
        frame_bytes < 300 * 1024,
        "projected frame should be compact (<300KiB), was {frame_bytes} bytes"
    );

    call(
        &runtime,
        "tok-large-projected",
        "dag.unsubscribe",
        json!({ "subscriptionId": frame["subscriptionId"].as_str().unwrap() }),
    )
    .expect("unsubscribe");
}

#[test]
fn dag_stream_rejects_oversized_projected_frame() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-oversize-projected");
    let dir = runs_dir(project_dir.path());

    // Construct a checkpoint where the PROJECTED fields themselves exceed 512KiB
    let huge_prompt = "P".repeat(550 * 1024);
    let checkpoint = json!({
        "runId": "huge-projected-run",
        "runKey": "key-huge",
        "name": "huge-projected",
        "status": "running",
        "nodes": [
            {
                "id": "node-huge",
                "prompt": huge_prompt,
                "state": "running",
                "route": { "kind": "category", "category": "deep-low" }
            }
        ]
    });

    write_checkpoint(&dir, "huge_projected.json", &checkpoint);

    let frame = call(
        &runtime,
        "tok-oversize-projected",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");

    assert!(
        run_ids(&frame).is_empty(),
        "oversized projected run must not be included in runs"
    );
    let dropped = frame["dropped"].as_array().expect("dropped array");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["file"], "huge_projected.json");
    assert_eq!(dropped[0]["error"], "SNAPSHOT_TOO_LARGE");
    assert!(dropped[0]["bytes"].as_u64().unwrap() > super::MAX_PROJECTED_SNAPSHOT_BYTES as u64);

    call(
        &runtime,
        "tok-oversize-projected",
        "dag.unsubscribe",
        json!({ "subscriptionId": frame["subscriptionId"].as_str().unwrap() }),
    )
    .expect("unsubscribe");
}

fn exact_sized_checkpoint(run_id: &str, target_projected_bytes: usize) -> Value {
    let mut checkpoint = json!({
        "runId": run_id,
        "runKey": format!("key-{run_id}"),
        "name": format!("name-{run_id}"),
        "status": "running",
        "nodes": [
            {
                "id": "node-1",
                "label": "Node 1",
                "prompt": "",
                "state": "running",
                "route": { "kind": "category", "category": "deep-low" }
            }
        ]
    });

    let raw_str = serde_json::to_string(&checkpoint).unwrap();
    let snapshot = super::super::dag_journal::parse_run_checkpoint(&raw_str).unwrap();
    let base_val = serde_json::to_value(&snapshot).unwrap();
    let base_bytes = serde_json::to_vec(&base_val).unwrap().len();

    assert!(
        target_projected_bytes >= base_bytes,
        "target bytes {target_projected_bytes} must be >= base {base_bytes}"
    );

    let padding_len = target_projected_bytes - base_bytes;
    checkpoint["nodes"][0]["prompt"] = Value::String("X".repeat(padding_len));

    // Verify exact projected byte length matches target
    let raw_str = serde_json::to_string(&checkpoint).unwrap();
    let snapshot = super::super::dag_journal::parse_run_checkpoint(&raw_str).unwrap();
    let projected_val = serde_json::to_value(&snapshot).unwrap();
    let actual_projected_bytes = serde_json::to_vec(&projected_val).unwrap().len();
    assert_eq!(actual_projected_bytes, target_projected_bytes);

    checkpoint
}

#[test]
fn dag_stream_strictly_bounds_complete_frame_with_near_cap_run_and_drops() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-near-cap");
    let dir = runs_dir(project_dir.path());

    // 1) Test single projected snapshot where snapshot len is MAX_BATCH_BYTES - 50.
    // The snapshot itself is 524,238 bytes <= MAX_PROJECTED_SNAPSHOT_BYTES (524,288 bytes).
    // However, the JSON envelope wrapper (subscriptionId, sequence, runs, dropped, etc.)
    // is > 100 bytes, which pushes the entire candidate frame > MAX_BATCH_BYTES (512 KiB).
    // Because it is a single item that alone cannot fit in any valid frame, it must be
    // reported as dropped with SNAPSHOT_TOO_LARGE and NO runs delivered.
    let target_bytes = super::MAX_BATCH_BYTES - 50;
    let single_overflow_checkpoint = exact_sized_checkpoint("single-near-cap", target_bytes);
    write_checkpoint(&dir, "00-single-near-cap.json", &single_overflow_checkpoint);

    let frame = call(
        &runtime,
        "tok-near-cap",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");

    assert!(
        run_ids(&frame).is_empty(),
        "run exceeding envelope budget alone must not be delivered in runs"
    );
    let dropped = frame["dropped"].as_array().expect("dropped array");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["file"], "00-single-near-cap.json");
    assert_eq!(dropped[0]["error"], "SNAPSHOT_TOO_LARGE");
    assert_eq!(dropped[0]["bytes"], target_bytes as u64);

    let frame_bytes = serde_json::to_vec(&frame).unwrap().len();
    assert!(
        frame_bytes <= super::MAX_BATCH_BYTES,
        "delivered frame must not exceed 512KiB budget, was {frame_bytes} bytes"
    );

    call(
        &runtime,
        "tok-near-cap",
        "dag.unsubscribe",
        json!({ "subscriptionId": frame["subscriptionId"].as_str().unwrap() }),
    )
    .expect("unsubscribe");
}

#[test]
fn dag_stream_defers_dropped_record_when_addition_overflows_frame() {
    let (runtime, _runtime_dir, project_dir) = runtime_with_project("tok-defer-drop");
    let dir = runs_dir(project_dir.path());

    // Size run so that candidate frame with run alone is valid, but adding a dropped record
    // (~80-120 bytes) causes the total frame to exceed MAX_BATCH_BYTES (512 KiB).
    // First subscribe with a dummy run to discover the exact wrapper overhead.
    let dummy = exact_sized_checkpoint("probe-run", 1000);
    write_checkpoint(&dir, "probe.json", &dummy);
    let probe_frame = call(
        &runtime,
        "tok-defer-drop",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .unwrap();
    let sub_id = probe_frame["subscriptionId"].as_str().unwrap();
    let probe_frame_bytes = serde_json::to_vec(&probe_frame).unwrap().len();
    let wrapper_overhead = probe_frame_bytes - 1000;

    call(
        &runtime,
        "tok-defer-drop",
        "dag.unsubscribe",
        json!({ "subscriptionId": sub_id }),
    )
    .unwrap();
    std::fs::remove_file(dir.join("probe.json")).unwrap();

    // Create run that leaves only 30 bytes remaining before MAX_BATCH_BYTES.
    // That fits in the frame alone, but adding any dropped entry (>50 bytes) will overflow.
    let exact_run_bytes = super::MAX_BATCH_BYTES - wrapper_overhead - 30;
    let run_checkpoint = exact_sized_checkpoint("run-fit", exact_run_bytes);
    write_checkpoint(&dir, "00-run-fit.json", &run_checkpoint);

    // Create second file that is invalid / oversized, producing a dropped record
    let huge = json!({
        "runId": "huge-file",
        "runKey": "key-huge",
        "name": "huge",
        "status": "running",
        "nodes": [],
        "filler": "x".repeat(super::MAX_INPUT_FILE_BYTES as usize + 1),
    });
    write_checkpoint(&dir, "01-huge.json", &huge);

    let frame = call(
        &runtime,
        "tok-defer-drop",
        "dag.subscribe",
        json!({ "projectId": "project" }),
    )
    .expect("subscribe");

    let sub_id = frame["subscriptionId"].as_str().unwrap().to_string();
    assert_eq!(run_ids(&frame), vec!["run-fit".to_string()]);
    assert!(
        frame["dropped"].as_array().unwrap().is_empty(),
        "dropped record must be deferred to next frame because adding it exceeds 512KiB"
    );
    assert_eq!(
        frame["more"], true,
        "more must be true to indicate deferred items"
    );

    let frame_bytes = serde_json::to_vec(&frame).unwrap().len();
    assert!(
        frame_bytes <= super::MAX_BATCH_BYTES,
        "frame must be within 512KiB, was {frame_bytes} bytes"
    );

    // Second frame delivers the deferred dropped record
    let next_frame = call(
        &runtime,
        "tok-defer-drop",
        "dag.next",
        json!({ "subscriptionId": sub_id, "waitMs": 0 }),
    )
    .expect("next frame");

    assert!(run_ids(&next_frame).is_empty());
    let next_dropped = next_frame["dropped"].as_array().expect("dropped in next");
    assert_eq!(next_dropped.len(), 1);
    assert_eq!(next_dropped[0]["file"], "01-huge.json");
    assert_eq!(next_dropped[0]["error"], "SNAPSHOT_TOO_LARGE");

    call(
        &runtime,
        "tok-defer-drop",
        "dag.unsubscribe",
        json!({ "subscriptionId": sub_id }),
    )
    .expect("unsubscribe");
}
