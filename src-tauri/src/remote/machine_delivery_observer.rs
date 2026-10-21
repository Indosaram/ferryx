//! Authoritative host-side physical delivery observer for paired machine terminal sessions.
//!
//! Tracks actual WebSocket ingress and physical delivery completion (PTY master write / OS signal)
//! on the host daemon under `#[cfg(feature = "local-split-qa")]`.
//!
//! Architectural Invariants:
//! 1. Concrete Target Fencing: Auditing and interval tracking strictly filter on the full
//!    `RemoteTerminalTarget` identity (machine_id, daemon_epoch string Epoch, session_id).
//! 2. Event-Sourced Immutable Records: Ingress and Delivery are separate immutable append-only
//!    events with their own monotonic sequence numbers. Late completions cannot mutate sealed evidence.
//! 3. Explicit Armed + Sealed Finite Interval: Fault intervals are armed with `start_delivery_seq`
//!    and sealed once with `end_delivery_seq`. Pre-arm ingress delivered during the interval is
//!    captured by its delivery sequence. Unresolved intents at seal are explicitly recorded.
//! 4. Borrow-Free Seal Semantics: `seal_fence` inspects the fence immutably first, computes the
//!    unresolved snapshot across events, and re-acquires the mutable fence to seal in place.
//! 5. Cross-Generation Replay Detection: Nonced writes and nonced signals are compared against
//!    pre-interval deliveries to catch retransmission across reconnect generation boundaries.
//! 6. Signal Intent Association: Optional wire intent_id is associated from ingress to completion.
//!    Signals carrying unique intent_id are verified against cross-generation and duplicate replay;
//!    fixed legacy signals without intent_id are bounded by expected multiplicity.
//! 7. Serial Test Lock: Unit tests targeting static `STATE` are serialized using an explicit
//!    test mutex guard held across the entire test case to eliminate parallel race conditions.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use crate::remote::machine_protocol::RemoteTerminalTarget;
use crate::scoped_contracts::Epoch;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcreteTarget {
    pub machine_id: String,
    pub daemon_epoch: Epoch,
    pub session_id: String,
}

impl From<&RemoteTerminalTarget> for ConcreteTarget {
    fn from(t: &RemoteTerminalTarget) -> Self {
        Self {
            machine_id: t.machine_id.clone(),
            daemon_epoch: t.daemon_epoch,
            session_id: t.session_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngressEvent {
    pub ingress_seq: u64,
    pub target: ConcreteTarget,
    pub generation: Epoch,
    pub operation: String, // "write" | "signal"
    pub payload_hash: String,
    pub payload_len: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_id: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryEvent {
    pub delivery_seq: u64,
    pub ingress_seq: u64,
    pub target: ConcreteTarget,
    pub generation: Epoch,
    pub operation: String,
    pub payload_hash: String,
    pub payload_len: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedIngress {
    pub ingress_seq: u64,
    pub target: ConcreteTarget,
    pub generation: Epoch,
    pub operation: String,
    pub payload_hash: String,
    pub payload_len: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_id: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FenceRecord {
    pub fence_id: String,
    pub target: ConcreteTarget,
    pub generation: Epoch,
    pub armed_at_ms: u64,
    pub start_delivery_seq: u64,
    pub start_ingress_seq: u64,
    pub sealed_at_ms: Option<u64>,
    pub end_delivery_seq: Option<u64>,
    pub end_ingress_seq: Option<u64>,
    pub status: String, // "armed" | "sealed"
    pub unresolved_at_seal: Vec<UnresolvedIngress>,
    pub expected_write_multiplicity: Option<usize>,
    pub expected_signal_multiplicity: Option<usize>,
}

#[derive(Default)]
pub struct ObserverState {
    next_ingress_seq: u64,
    next_delivery_seq: u64,
    next_fence_id: u64,
    ingress_events: Vec<IngressEvent>,
    delivery_events: Vec<DeliveryEvent>,
    fences: HashMap<String, FenceRecord>, // keyed by fence_id
}

static STATE: LazyLock<Mutex<ObserverState>> = LazyLock::new(|| Mutex::new(ObserverState::default()));

/// Record ingress of an incoming frame at the host WebSocket receiver.
/// Preserve payload hash and optional intent identity as separate evidence.
pub fn record_ingress(
    target: &RemoteTerminalTarget,
    generation: u64,
    operation: &str,
    payload: &[u8],
    intent_id: Option<&str>,
) -> u64 {
    let mut state = STATE.lock().unwrap();
    state.next_ingress_seq += 1;
    let seq = state.next_ingress_seq;
    let payload_hash = sha256_hex(payload);
    let event = IngressEvent {
        ingress_seq: seq,
        target: ConcreteTarget::from(target),
        generation: Epoch(generation),
        operation: operation.to_owned(),
        payload_hash,
        payload_len: payload.len(),
        intent_id: intent_id.map(|s| s.to_owned()),
        timestamp_ms: now_ms(),
    };
    state.ingress_events.push(event);
    seq
}

/// Record completion of PTY write or OS signal delivery on the host as an immutable append-only event.
/// Associates the exact same intent_id from ingress to delivery completion.
pub fn record_delivery(ingress_seq: u64, success: bool, error: Option<String>) -> u64 {
    let mut state = STATE.lock().unwrap();
    let ingress = state
        .ingress_events
        .iter()
        .find(|i| i.ingress_seq == ingress_seq)
        .cloned();

    state.next_delivery_seq += 1;
    let delivery_seq = state.next_delivery_seq;

    if let Some(i) = ingress {
        let event = DeliveryEvent {
            delivery_seq,
            ingress_seq,
            target: i.target,
            generation: i.generation,
            operation: i.operation,
            payload_hash: i.payload_hash,
            payload_len: i.payload_len,
            intent_id: i.intent_id,
            success,
            error,
            timestamp_ms: now_ms(),
        };
        state.delivery_events.push(event);
    }
    delivery_seq
}

/// Arm a fault interval fence for a concrete target and controller generation.
pub fn arm_fence(
    target: &RemoteTerminalTarget,
    generation: Epoch,
    expected_write_multiplicity: Option<usize>,
    expected_signal_multiplicity: Option<usize>,
) -> Result<String, &'static str> {
    if target.session_id.is_empty() || target.machine_id.is_empty() {
        return Err("INVALID_TARGET");
    }
    let mut state = STATE.lock().unwrap();
    state.next_fence_id += 1;
    let fence_id = format!("fence-{}-{}-{}", target.session_id, generation.0, state.next_fence_id);
    let start_delivery_seq = state.next_delivery_seq;
    let start_ingress_seq = state.next_ingress_seq;

    let fence = FenceRecord {
        fence_id: fence_id.clone(),
        target: ConcreteTarget::from(target),
        generation,
        armed_at_ms: now_ms(),
        start_delivery_seq,
        start_ingress_seq,
        sealed_at_ms: None,
        end_delivery_seq: None,
        end_ingress_seq: None,
        status: "armed".to_owned(),
        unresolved_at_seal: Vec::new(),
        expected_write_multiplicity,
        expected_signal_multiplicity,
    };
    state.fences.insert(fence_id.clone(), fence);
    Ok(fence_id)
}

/// Explicitly seal a previously armed fault interval fence once only.
/// Borrows fence immutably first to snapshot boundaries, then computes unresolved intents,
/// and re-borrows mutably to complete seal without E0502 borrow checker conflict.
pub fn seal_fence(fence_id: &str, target: &RemoteTerminalTarget) -> Result<FenceRecord, &'static str> {
    let mut state = STATE.lock().unwrap();
    let concrete_target = ConcreteTarget::from(target);

    // 1. Immutable read and boundary snapshot
    let (_start_ingress, _start_delivery) = {
        let fence = state.fences.get(fence_id).ok_or("FENCE_NOT_FOUND")?;
        if fence.target != concrete_target {
            return Err("FENCE_TARGET_MISMATCH");
        }
        if fence.status == "sealed" {
            return Err("ALREADY_SEALED");
        }
        (fence.start_ingress_seq, fence.start_delivery_seq)
    };

    // 2. Snapshot sequence ceiling and unresolved intents without active fence borrow
    let end_delivery_seq = state.next_delivery_seq;
    let end_ingress_seq = state.next_ingress_seq;

    let delivered_ingress_seqs: std::collections::HashSet<u64> = state
        .delivery_events
        .iter()
        .map(|d| d.ingress_seq)
        .collect();

    let unresolved: Vec<UnresolvedIngress> = state
        .ingress_events
        .iter()
        .filter(|i| {
            i.target == concrete_target
                && i.ingress_seq <= end_ingress_seq
                && !delivered_ingress_seqs.contains(&i.ingress_seq)
        })
        .map(|i| UnresolvedIngress {
            ingress_seq: i.ingress_seq,
            target: i.target.clone(),
            generation: i.generation,
            operation: i.operation.clone(),
            payload_hash: i.payload_hash.clone(),
            payload_len: i.payload_len,
            intent_id: i.intent_id.clone(),
            timestamp_ms: i.timestamp_ms,
        })
        .collect();

    // 3. Re-acquire mutable borrow on target fence and seal
    let fence = state.fences.get_mut(fence_id).ok_or("FENCE_NOT_FOUND")?;
    fence.sealed_at_ms = Some(now_ms());
    fence.end_delivery_seq = Some(end_delivery_seq);
    fence.end_ingress_seq = Some(end_ingress_seq);
    fence.status = "sealed".to_owned();
    fence.unresolved_at_seal = unresolved;
    Ok(fence.clone())
}

/// Audit physical deliveries across a sealed fault interval for a concrete target and required fence_id.
pub fn audit(target: &RemoteTerminalTarget, fence_id: &str) -> Result<Value, &'static str> {
    let state = STATE.lock().unwrap();
    let concrete_target = ConcreteTarget::from(target);

    let fence = state.fences.get(fence_id).cloned().ok_or("FENCE_NOT_FOUND")?;
    if fence.target != concrete_target {
        return Err("FENCE_TARGET_MISMATCH");
    }
    if fence.status != "sealed" || fence.end_delivery_seq.is_none() {
        return Err("FENCE_NOT_SEALED");
    }

    let start_delivery = fence.start_delivery_seq;
    let end_delivery = fence.end_delivery_seq.unwrap();

    // Deliveries strictly matching exact concrete target
    let target_deliveries: Vec<DeliveryEvent> = state
        .delivery_events
        .iter()
        .filter(|d| d.target == concrete_target)
        .cloned()
        .collect();

    let pre_interval: Vec<DeliveryEvent> = target_deliveries
        .iter()
        .filter(|d| d.delivery_seq <= start_delivery && d.success)
        .cloned()
        .collect();

    let interval_deliveries: Vec<DeliveryEvent> = target_deliveries
        .iter()
        .filter(|d| d.delivery_seq > start_delivery && d.delivery_seq <= end_delivery)
        .cloned()
        .collect();

    let interval_delivered_writes: Vec<DeliveryEvent> = interval_deliveries
        .iter()
        .filter(|d| d.operation == "write" && d.success)
        .cloned()
        .collect();

    let interval_delivered_signals: Vec<DeliveryEvent> = interval_deliveries
        .iter()
        .filter(|d| d.operation == "signal" && d.success)
        .cloned()
        .collect();

    let interval_failed_count = interval_deliveries.iter().filter(|d| !d.success).count();

    // Cross-generation replay detection:
    // 1. Writes: Compare nonced write payloads against pre-interval deliveries.
    let mut replay_violations = Vec::new();
    for delivered_write in &interval_delivered_writes {
        if let Some(pre) = pre_interval.iter().find(|p| p.operation == "write" && p.payload_hash == delivered_write.payload_hash) {
            replay_violations.push(json!({
                "payloadHash": delivered_write.payload_hash,
                "operation": "write",
                "initialGeneration": pre.generation.0,
                "initialDeliverySeq": pre.delivery_seq,
                "replayedGeneration": delivered_write.generation.0,
                "replayedDeliverySeq": delivered_write.delivery_seq,
                "type": "cross_generation_write_replay",
            }));
        }
    }

    // 2. Signals with unique intent_id: Compare intent_id against pre-interval signal deliveries.
    for delivered_signal in &interval_delivered_signals {
        if let Some(ref intent) = delivered_signal.intent_id {
            if let Some(pre) = pre_interval.iter().find(|p| p.operation == "signal" && p.intent_id.as_deref() == Some(intent.as_str())) {
                replay_violations.push(json!({
                    "intentId": intent,
                    "operation": "signal",
                    "initialGeneration": pre.generation.0,
                    "initialDeliverySeq": pre.delivery_seq,
                    "replayedGeneration": delivered_signal.generation.0,
                    "replayedDeliverySeq": delivered_signal.delivery_seq,
                    "type": "cross_generation_signal_replay",
                }));
            }
        }
    }

    // Intra-interval duplicate write detection:
    let mut seen_writes = HashMap::new();
    for w in &interval_delivered_writes {
        *seen_writes.entry(&w.payload_hash).or_insert(0usize) += 1;
    }
    for (hash, count) in seen_writes {
        if count > 1 {
            replay_violations.push(json!({
                "payloadHash": hash,
                "operation": "write",
                "count": count,
                "type": "intra_interval_duplicate_write",
            }));
        }
    }

    // Intra-interval duplicate signal intent detection:
    let mut seen_signal_intents = HashMap::new();
    for s in &interval_delivered_signals {
        if let Some(ref intent) = s.intent_id {
            *seen_signal_intents.entry(intent.as_str()).or_insert(0usize) += 1;
        }
    }
    for (intent, count) in seen_signal_intents {
        if count > 1 {
            replay_violations.push(json!({
                "intentId": intent,
                "operation": "signal",
                "count": count,
                "type": "intra_interval_duplicate_signal",
            }));
        }
    }

    // Fixed control signal multiplicity check:
    // If expected multiplicity is specified, bound total delivered signals in interval.
    let signal_multiplicity_violation = if let Some(expected) = fence.expected_signal_multiplicity {
        interval_delivered_signals.len() > expected
    } else {
        false
    };

    let signal_deficit = if let Some(expected) = fence.expected_signal_multiplicity {
        interval_delivered_signals.len() < expected
    } else {
        false
    };

    let write_multiplicity_violation = if let Some(expected) = fence.expected_write_multiplicity {
        interval_delivered_writes.len() > expected
    } else {
        false
    };

    Ok(json!({
        "target": concrete_target,
        "fence": fence,
        "interval": {
            "startDeliverySeq": start_delivery,
            "endDeliverySeq": end_delivery,
            "totalDelivered": interval_deliveries.len(),
            "deliveredWrites": interval_delivered_writes.len(),
            "deliveredSignals": interval_delivered_signals.len(),
            "failedCount": interval_failed_count,
            "unresolvedAtSeal": fence.unresolved_at_seal.len(),
            "replayViolations": replay_violations,
            "signalMultiplicityViolation": signal_multiplicity_violation,
            "signalDeficit": signal_deficit,
            "writeMultiplicityViolation": write_multiplicity_violation,
        },
        "records": interval_deliveries,
        "unresolvedAtSeal": fence.unresolved_at_seal,
        "preIntervalDeliveries": pre_interval,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn test_seal_immutability_and_once_only() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let t = RemoteTerminalTarget {
            machine_id: "m-test-seal".into(),
            daemon_epoch: Epoch(1),
            session_id: "s-test-seal".into(),
        };
        let fence_id = arm_fence(&t, Epoch(1), Some(1), Some(1)).unwrap();
        let in_seq = record_ingress(&t, 1, "write", b"test-payload-1", None);
        record_delivery(in_seq, true, None);
        let sealed = seal_fence(&fence_id, &t).unwrap();
        assert_eq!(sealed.status, "sealed");

        // Once-only seal: repeated seal returns typed error
        assert_eq!(seal_fence(&fence_id, &t).unwrap_err(), "ALREADY_SEALED");

        // Completion after seal receives delivery_seq > end_seq and cannot alter sealed evidence
        let late_in = record_ingress(&t, 1, "write", b"late-payload", None);
        record_delivery(late_in, true, None);
        let aud = audit(&t, &fence_id).unwrap();
        assert_eq!(aud["interval"]["deliveredWrites"], 1);
        assert_eq!(aud["interval"]["totalDelivered"], 1);
    }

    #[test]
    fn test_delayed_completion_pre_arm_captured() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let t = RemoteTerminalTarget {
            machine_id: "m-test-delayed".into(),
            daemon_epoch: Epoch(1),
            session_id: "s-test-delayed".into(),
        };
        let pre_in = record_ingress(&t, 1, "write", b"pre-arm-nonced-write", None);
        let fence_id = arm_fence(&t, Epoch(1), Some(1), None).unwrap();
        record_delivery(pre_in, true, None);
        seal_fence(&fence_id, &t).unwrap();
        let aud = audit(&t, &fence_id).unwrap();
        assert_eq!(aud["interval"]["deliveredWrites"], 1);
    }

    #[test]
    fn test_pre_arm_unresolved_completion_cannot_change_sealed_audit() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let target = RemoteTerminalTarget {
            machine_id: "pre-arm-unresolved".into(),
            daemon_epoch: Epoch(1),
            session_id: "pre-arm-unresolved".into(),
        };
        let ingress = record_ingress(&target, 1, "signal", b"interrupt", Some("intent-before-arm"));
        let fence = arm_fence(&target, Epoch(1), None, None).unwrap();
        seal_fence(&fence, &target).unwrap();
        let before = audit(&target, &fence).unwrap();
        assert_eq!(before["unresolvedAtSeal"][0]["intentId"], "intent-before-arm");
        record_delivery(ingress, true, None);
        assert_eq!(audit(&target, &fence).unwrap(), before);
    }

    #[test]
    fn test_fence_mismatch_and_required() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let t = RemoteTerminalTarget {
            machine_id: "m-test-mismatch".into(),
            daemon_epoch: Epoch(1),
            session_id: "s-test-mismatch".into(),
        };
        let other = RemoteTerminalTarget {
            machine_id: "m-test-mismatch-other".into(),
            daemon_epoch: Epoch(1),
            session_id: "s-test-mismatch".into(),
        };
        let fence_id = arm_fence(&t, Epoch(1), None, None).unwrap();
        assert_eq!(seal_fence(&fence_id, &other).unwrap_err(), "FENCE_TARGET_MISMATCH");
        assert_eq!(audit(&t, "nonexistent-fence").unwrap_err(), "FENCE_NOT_FOUND");
    }

    #[test]
    fn test_signal_intent_replay_detection() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let t = RemoteTerminalTarget {
            machine_id: "m-test-sigintent".into(),
            daemon_epoch: Epoch(1),
            session_id: "s-test-sigintent".into(),
        };
        let s1 = record_ingress(&t, 1, "signal", b"interrupt", Some("nonce-sig-1"));
        record_delivery(s1, true, None);

        let fence_id = arm_fence(&t, Epoch(2), None, None).unwrap();

        // Replay of the exact same signal intent in next generation
        let s1_replay = record_ingress(&t, 2, "signal", b"interrupt", Some("nonce-sig-1"));
        record_delivery(s1_replay, true, None);

        seal_fence(&fence_id, &t).unwrap();
        let aud = audit(&t, &fence_id).unwrap();

        let replays = aud["interval"]["replayViolations"].as_array().unwrap();
        assert_eq!(replays.len(), 1);
        assert_eq!(replays[0]["type"], "cross_generation_signal_replay");
        assert_eq!(replays[0]["intentId"], "nonce-sig-1");
    }

    #[test]
    fn test_legitimate_identical_signals_vs_duplicates_across_generations() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let t = RemoteTerminalTarget {
            machine_id: "m-test-signals".into(),
            daemon_epoch: Epoch(1),
            session_id: "s-test-signals".into(),
        };
        let s1 = record_ingress(&t, 1, "signal", b"interrupt", Some("sig-intent-1"));
        record_delivery(s1, true, None);
        let w1 = record_ingress(&t, 1, "write", b"unique-nonce-101", None);
        record_delivery(w1, true, None);

        let fence_id = arm_fence(&t, Epoch(2), Some(1), Some(1)).unwrap();

        // Distinct legitimate signal with new unique intent
        let s2 = record_ingress(&t, 2, "signal", b"interrupt", Some("sig-intent-2"));
        record_delivery(s2, true, None);

        let w1_replay = record_ingress(&t, 2, "write", b"unique-nonce-101", None);
        record_delivery(w1_replay, true, None);

        seal_fence(&fence_id, &t).unwrap();
        let aud = audit(&t, &fence_id).unwrap();

        assert_eq!(aud["interval"]["deliveredSignals"], 1);
        assert_eq!(aud["interval"]["signalMultiplicityViolation"], false);

        let replays = aud["interval"]["replayViolations"].as_array().unwrap();
        assert_eq!(replays.len(), 1);
        assert_eq!(replays[0]["type"], "cross_generation_write_replay");
        assert_eq!(replays[0]["operation"], "write");
    }
}

