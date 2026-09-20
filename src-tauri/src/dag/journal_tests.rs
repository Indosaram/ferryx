use super::journal::*;
use std::path::Path;

const FIXTURE_F107_JSON: &str =
    include_str!("testdata/dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");
const FIXTURE_081E_JSON: &str =
    include_str!("testdata/dag_081e597f-0aa8-4a20-a826-4e3d045aacef.json");
const FIXTURE_F107_JSONL: &str =
    include_str!("testdata/dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.jsonl");

#[test]
fn test_parse_run_checkpoint_f107_real_fixture() {
    let snapshot = parse_run_checkpoint(FIXTURE_F107_JSON).expect("parse f107 checkpoint");

    assert_eq!(snapshot.name, "Ferryx native terminal rendering fixes");
    assert_eq!(snapshot.status, DagRunStatus::Cancelled);
    assert_eq!(snapshot.nodes.len(), 6);
    assert_eq!(snapshot.waves.len(), 4);
    assert!(snapshot.waves[0].node_ids.iter().any(|id| id == "extract"));

    for wave in &snapshot.waves {
        for node_id in &wave.node_ids {
            assert!(
                snapshot.nodes.iter().any(|n| &n.id == node_id),
                "wave node {node_id} must exist in nodes"
            );
        }
    }

    assert_eq!(
        snapshot.root_session_id.as_deref(),
        Some("01a04819-fbfa-7fd7-b6be-3fe9bfed7bf1")
    );
    assert_eq!(
        snapshot.parent_session_id.as_deref(),
        Some("01a04819-fbfa-7fd7-b6be-3fe9bfed7bf1")
    );

    assert_eq!(snapshot.counts.completed, 1);
    assert_eq!(snapshot.counts.total, 6);
    assert_eq!(snapshot.counts.cancelled, 5);
    assert_eq!(snapshot.amend_count, 1);

    assert_eq!(
        &snapshot.critical_path[..4],
        &["extract", "render", "d5-fix", "verify"]
    );
    assert_eq!(snapshot.bottlenecks[0].node_id, "extract");
    assert_eq!(snapshot.bottlenecks[0].blocked_count, 3);
}

#[test]
fn test_parse_run_checkpoint_081e_real_fixture() {
    let snapshot = parse_run_checkpoint(FIXTURE_081E_JSON).expect("parse 081e checkpoint");

    assert_eq!(snapshot.status, DagRunStatus::Failed);
    assert_eq!(snapshot.nodes.len(), 5);

    let backend_node = snapshot
        .nodes
        .iter()
        .find(|n| n.id == "backend-bootstrap")
        .expect("backend-bootstrap node exists");

    assert_eq!(backend_node.state, DagNodeState::Failed);
    let err = backend_node.error.as_ref().expect("error present");
    assert_eq!(err.code, "task_cancelled");
    assert!(err.node_id.as_deref() == Some("backend-bootstrap"));
}

#[test]
fn test_state_tolerance_fallback_variant() {
    let mutated =
        FIXTURE_081E_JSON.replace("\"state\":\"completed\"", "\"state\":\"zombie-state\"");
    let snapshot = parse_run_checkpoint(&mutated).expect("parse mutated checkpoint");
    let arch_node = snapshot
        .nodes
        .iter()
        .find(|n| n.id == "architecture")
        .expect("architecture node exists");

    assert_eq!(arch_node.state, DagNodeState::Unknown);
}

#[test]
fn test_waves_unknown_node_id_fails_with_typed_error() {
    let mutated = FIXTURE_081E_JSON.replace(
        "\"nodeIds\":[\"architecture\"",
        "\"nodeIds\":[\"non-existent-node\"",
    );
    let result = parse_run_checkpoint(&mutated);
    match result {
        Err(DagJournalError::UnknownWaveNode {
            node_id,
            wave_index,
        }) => {
            assert_eq!(node_id, "non-existent-node");
            assert_eq!(wave_index, 0);
        }
        other => panic!("expected UnknownWaveNode error, got: {other:?}"),
    }
}

#[test]
fn test_parse_events_jsonl_stream() {
    let events = parse_events(FIXTURE_F107_JSONL).expect("parse jsonl events");
    assert!(!events.is_empty());

    let mut prev_seq = 0u64;
    for event in &events {
        assert!(
            event.seq > prev_seq,
            "seq {} must be strictly greater than prev_seq {}",
            event.seq,
            prev_seq
        );
        prev_seq = event.seq;
    }

    let transition_event = events
        .iter()
        .find(|e| {
            e.event_type == "dag.node.transitioned" && e.node_id.as_deref() == Some("extract")
        })
        .expect("transitioned event found for extract");

    assert_eq!(transition_event.from, Some(DagNodeState::Pending));
    assert_eq!(transition_event.to, Some(DagNodeState::Scheduled));
}

#[test]
fn test_list_run_summaries_from_testdata_dir() {
    let testdata_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/dag/testdata");
    let summaries = list_run_summaries(&testdata_dir).expect("list run summaries");

    assert!(summaries.len() >= 2);
    // Verified sorted updatedAt desc
    for pair in summaries.windows(2) {
        let a = pair[0].updated_at.as_deref().unwrap_or("");
        let b = pair[1].updated_at.as_deref().unwrap_or("");
        assert!(
            a >= b,
            "summaries must be sorted by updatedAt desc: {a} >= {b}"
        );
    }
}

#[test]
fn test_parse_run_checkpoint_full_information_fields() {
    let json = r#"{
        "runId": "dag_sample_123",
        "runKey": "sample-key",
        "name": "Full Information DAG",
        "status": "completed",
        "startedAt": "2026-09-17T10:00:00.000Z",
        "completedAt": "2026-09-17T10:05:00.000Z",
        "amendCount": 1,
        "amendHistory": [
            {
                "at": "2026-09-17T10:02:00.000Z",
                "previousFingerprint": "prev_fp",
                "fingerprint": "curr_fp",
                "changedNodeIds": ["step_1"],
                "addedNodeIds": [],
                "invalidatedNodeIds": ["step_1", "step_2"]
            }
        ],
        "diagnostics": [{"type": "warning", "message": "High memory usage"}],
        "nodes": [
            {
                "id": "step_1",
                "label": "First Step",
                "prompt": "TASK: Execute command. DELIVERABLE: result.txt. SCOPE: /tmp. VERIFY: check. STOP WHEN: done.",
                "state": "completed",
                "dependsOn": [],
                "attempt": 1,
                "route": {"kind": "category", "category": "quick"},
                "startedAt": "2026-09-17T10:00:01.000Z",
                "completedAt": "2026-09-17T10:02:00.000Z",
                "taskId": "st_01a0sample",
                "runStats": {
                    "runtimeMs": 119000,
                    "turns": 4,
                    "toolCalls": 3,
                    "inputTokens": 12000,
                    "outputTokens": 450,
                    "totalTokens": 12450,
                    "generationMs": 350,
                    "tokensPerSecond": 720.5,
                    "costUsd": 0.0025,
                    "cacheReadTokens": 8000,
                    "cacheWriteTokens": 1000
                },
                "resultArtifact": {
                    "relativePath": "dag/results/dag_sample_123/step_1.txt",
                    "sha256": "abcdef123456",
                    "bytes": 2048
                }
            }
        ],
        "edges": [],
        "waves": [{"index": 0, "nodeIds": ["step_1"]}],
        "criticalPath": ["step_1"],
        "bottlenecks": []
    }"#;

    let snapshot = parse_run_checkpoint(json).expect("parse full checkpoint");
    assert_eq!(snapshot.run_id, "dag_sample_123");
    assert_eq!(snapshot.amend_count, 1);

    let amend_history = snapshot
        .amend_history
        .as_ref()
        .expect("amend history present");
    assert_eq!(amend_history.len(), 1);
    assert_eq!(amend_history[0].changed_node_ids, vec!["step_1"]);
    assert_eq!(
        amend_history[0].invalidated_node_ids,
        vec!["step_1", "step_2"]
    );

    let diagnostics = snapshot.diagnostics.as_ref().expect("diagnostics present");
    assert_eq!(diagnostics.len(), 1);

    let node = &snapshot.nodes[0];
    assert_eq!(
        node.prompt.as_deref(),
        Some("TASK: Execute command. DELIVERABLE: result.txt. SCOPE: /tmp. VERIFY: check. STOP WHEN: done.")
    );
    assert_eq!(node.task_id.as_deref(), Some("st_01a0sample"));

    let stats = node.run_stats.as_ref().expect("runStats present");
    assert_eq!(stats.runtime_ms, Some(119000));
    assert_eq!(stats.turns, Some(4));
    assert_eq!(stats.tool_calls, Some(3));
    assert_eq!(stats.input_tokens, Some(12000));
    assert_eq!(stats.output_tokens, Some(450));
    assert_eq!(stats.total_tokens, Some(12450));
    assert_eq!(stats.tokens_per_second, Some(720.5));
    assert_eq!(stats.cost_usd, Some(0.0025));
    assert_eq!(stats.cache_read_tokens, Some(8000));

    let artifact = node
        .result_artifact
        .as_ref()
        .expect("resultArtifact present");
    assert_eq!(
        artifact.relative_path,
        "dag/results/dag_sample_123/step_1.txt"
    );
    assert_eq!(artifact.sha256.as_deref(), Some("abcdef123456"));
    assert_eq!(artifact.bytes, Some(2048));
}
