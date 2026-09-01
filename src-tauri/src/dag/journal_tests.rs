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
