//! Acceptance tests of todo 7. Time is an injected clock: nothing sleeps.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sqlx::SqlitePool;

use super::*;
use crate::lanes::evidence::{self, Receipt, RemoteVerifier, ReportError, ReportInput, ReportKind, Reporter};
use crate::lanes::state::{insert_lane, set_pane, transition, LaneStatus};

const T0: i64 = 1_800_000_000_000;

struct FakeClock(AtomicI64);

impl crate::supervisor::recovery::Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

impl FakeClock {
    fn at_minute(&self, minute: i64) {
        self.0.store(T0 + minute * MINUTE_MS, Ordering::SeqCst);
    }
}

struct FixedStates(Mutex<(Backend, AgentState)>);

#[async_trait]
impl LaneStates for FixedStates {
    async fn read(&self, lanes: &[LaneRef]) -> HashMap<String, (Backend, AgentState)> {
        let state = *self.0.lock().unwrap();
        lanes.iter().map(|lane| (lane.key.clone(), state)).collect()
    }
}

#[derive(Default)]
struct Recorded {
    reminders: Mutex<Vec<String>>,
    interrupts: Mutex<Vec<String>>,
}

#[async_trait]
impl OversightEffects for Recorded {
    async fn remind(&self, _lane: &LaneRef, text: &str) -> bool {
        self.reminders.lock().unwrap().push(text.to_owned());
        true
    }
    async fn interrupt(&self, lane: &LaneRef) {
        self.interrupts.lock().unwrap().push(lane.key.clone());
    }
}

/// Accepts exactly the receipts it was told the remote has.
struct Remote(Vec<Receipt>);

#[async_trait]
impl RemoteVerifier for Remote {
    async fn verify(&self, _cwd: &std::path::Path, receipt: &Receipt) -> bool {
        self.0.contains(receipt)
    }
}

async fn lane(pool: &SqlitePool, key: &str, pane: &str, cwd: &str) {
    insert_lane(pool, key, key, "lead", cwd, "file marker").await.unwrap();
    transition(pool, key, LaneStatus::Working, Some("launched")).await.unwrap();
    set_pane(pool, key, pane).await.unwrap();
}

fn worker(pane: &str) -> Reporter {
    Reporter { host_id: "local".into(), session_id: pane.into() }
}

struct Rig {
    pool: SqlitePool,
    clock: Arc<FakeClock>,
    states: Arc<FixedStates>,
    effects: Arc<Recorded>,
    oversight: Oversight,
}

async fn rig(backend: Backend, state: AgentState) -> Rig {
    let pool = crate::storage::memory().await.unwrap();
    lane(&pool, "ledger", "pane-1", "/tmp").await;
    let clock = Arc::new(FakeClock(AtomicI64::new(T0)));
    let states = Arc::new(FixedStates(Mutex::new((backend, state))));
    let effects = Arc::new(Recorded::default());
    let oversight = Oversight {
        pool: pool.clone(),
        clock: clock.clone(),
        states: states.clone(),
        effects: effects.clone(),
    };
    Rig { pool, clock, states, effects, oversight }
}

impl Rig {
    /// One pass per minute from `from` through `to`, inclusive.
    async fn minutes(&self, from: i64, to: i64) {
        for minute in from..=to {
            self.clock.at_minute(minute);
            self.oversight.tick().await.unwrap();
        }
    }

    fn reminders(&self) -> Vec<String> {
        self.effects.reminders.lock().unwrap().clone()
    }

    async fn events(&self, kind: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT body FROM lane_events WHERE lane_key = 'ledger' AND kind = ? ORDER BY id")
            .bind(kind)
            .fetch_all(&self.pool)
            .await
            .unwrap()
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ferryx-oversight-{name}-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn a_restated_report_does_not_move_progress() {
    let dir = scratch("restated");
    let pool = crate::storage::memory().await.unwrap();
    lane(&pool, "ledger", "pane-1", dir.to_str().unwrap()).await;
    std::fs::write(dir.join("out.txt"), b"first").unwrap();
    let files = vec!["out.txt".to_owned()];
    let send = |text: &'static str, now: i64| {
        let pool = pool.clone();
        let files = files.clone();
        async move {
            let input = ReportInput { lane_key: "ledger", kind: ReportKind::Progress, text, files: &files, receipts: &[] };
            evidence::report(&pool, &worker("pane-1"), &Remote(vec![]), &input, now).await.unwrap()
        }
    };

    let first = send("wrote out.txt", T0 + MINUTE_MS).await;
    assert_eq!(first.new_items, vec!["out.txt".to_owned()]);
    assert_eq!(first.last_progress_at_ms, T0 + MINUTE_MS);

    // Same bytes, new words: a restatement.
    let again = send("really made progress on out.txt", T0 + 5 * MINUTE_MS).await;
    assert!(again.new_items.is_empty());
    assert_eq!(again.repeated, vec!["out.txt".to_owned()]);
    assert_eq!(again.last_progress_at_ms, T0 + MINUTE_MS);

    // A report with no evidence at all moves nothing either.
    let bare = ReportInput { lane_key: "ledger", kind: ReportKind::Progress, text: "still going", files: &[], receipts: &[] };
    let bare = evidence::report(&pool, &worker("pane-1"), &Remote(vec![]), &bare, T0 + 6 * MINUTE_MS).await.unwrap();
    assert_eq!(bare.last_progress_at_ms, T0 + MINUTE_MS);

    std::fs::write(dir.join("out.txt"), b"second").unwrap();
    let changed = send("wrote out.txt", T0 + 9 * MINUTE_MS).await;
    assert_eq!(changed.new_items, vec!["out.txt".to_owned()]);
    assert_eq!(changed.last_progress_at_ms, T0 + 9 * MINUTE_MS);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn an_unverifiable_receipt_is_not_progress() {
    let pool = crate::storage::memory().await.unwrap();
    lane(&pool, "ledger", "pane-1", "/tmp").await;
    let pushed = Receipt::Commit("abcdef1234567".into());
    let claimed = Receipt::Commit("1234567abcdef".into());
    let remote = Remote(vec![pushed.clone()]);

    let input = ReportInput { lane_key: "ledger", kind: ReportKind::Progress, text: "pushed", files: &[], receipts: std::slice::from_ref(&claimed) };
    let outcome = evidence::report(&pool, &worker("pane-1"), &remote, &input, T0 + MINUTE_MS).await.unwrap();
    assert!(outcome.new_items.is_empty());
    assert_eq!(outcome.unverified, vec![claimed.identity()]);
    assert_eq!(outcome.last_progress_at_ms, T0 + MINUTE_MS, "the run starts at the report; nothing moved it");

    // A tool-call id is not a receipt the hub can attest.
    let refused = Receipt::parse(&serde_json::json!({"toolCallId": "call_123"}));
    assert!(matches!(refused, Err(ReportError::Unattestable(_))));

    let input = ReportInput { lane_key: "ledger", kind: ReportKind::Progress, text: "pushed", files: &[], receipts: std::slice::from_ref(&pushed) };
    let verified = evidence::report(&pool, &worker("pane-1"), &remote, &input, T0 + 3 * MINUTE_MS).await.unwrap();
    assert_eq!(verified.new_items, vec![pushed.identity()]);
    assert_eq!(verified.last_progress_at_ms, T0 + 3 * MINUTE_MS);
}

#[tokio::test]
async fn a_report_from_another_lane_is_refused() {
    let pool = crate::storage::memory().await.unwrap();
    lane(&pool, "ledger", "pane-1", "/tmp").await;
    lane(&pool, "other", "pane-2", "/tmp").await;
    let input = ReportInput { lane_key: "ledger", kind: ReportKind::Progress, text: "not mine", files: &[], receipts: &[] };
    let refused = evidence::report(&pool, &worker("pane-2"), &Remote(vec![]), &input, T0).await;
    assert!(matches!(refused, Err(ReportError::NotLaneWorker { .. })), "{refused:?}");
    let remote_host = Reporter { host_id: "host-b".into(), session_id: "pane-1".into() };
    let refused = evidence::report(&pool, &remote_host, &Remote(vec![]), &input, T0).await;
    assert!(matches!(refused, Err(ReportError::NotLaneWorker { .. })), "{refused:?}");
    let written: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_reports").fetch_one(&pool).await.unwrap();
    assert_eq!(written, 0);
}

#[tokio::test]
async fn reminders_at_10_and_20_then_one_alert_at_40() {
    let rig = rig(Backend::Claude, AgentState::Idle).await;
    rig.minutes(0, 9).await;
    assert!(rig.reminders().is_empty());
    rig.minutes(10, 10).await;
    assert_eq!(rig.reminders(), vec![reminder_text("ledger", 10)]);
    rig.minutes(11, 39).await;
    assert_eq!(rig.reminders(), vec![reminder_text("ledger", 10), reminder_text("ledger", 20)]);
    assert!(rig.events(STALL_EVENT).await.is_empty());
    rig.minutes(40, 90).await;
    assert_eq!(rig.reminders().len(), 2);
    assert_eq!(rig.events(STALL_EVENT).await.len(), 1);
}

#[tokio::test]
async fn no_reminder_while_working_or_waiting_on_owner() {
    let rig = rig(Backend::Codex, AgentState::Working).await;
    rig.minutes(0, 30).await;
    assert!(rig.reminders().is_empty(), "a working agent is never nudged");

    *rig.states.0.lock().unwrap() = (Backend::Codex, AgentState::AwaitingOwner);
    rig.minutes(31, 60).await;
    assert!(rig.reminders().is_empty(), "an agent asking the owner is never nudged");
    assert!(rig.events(STALL_EVENT).await.is_empty(), "a lane waiting on the owner is not stalled");

    // Idle again but its last report says it waits on the owner.
    let input = ReportInput { lane_key: "ledger", kind: ReportKind::Waiting, text: "need the owner's pick", files: &[], receipts: &[] };
    evidence::report(&rig.pool, &worker("pane-1"), &Remote(vec![]), &input, T0 + 61 * MINUTE_MS).await.unwrap();
    *rig.states.0.lock().unwrap() = (Backend::Codex, AgentState::Idle);
    rig.minutes(61, 120).await;
    assert!(rig.reminders().is_empty());
    assert!(rig.events(STALL_EVENT).await.is_empty());
}

#[tokio::test]
async fn an_observe_only_backend_gets_no_reminders() {
    for backend in [Backend::PairedHost, Backend::MhcEngine, Backend::Unknown] {
        let rig = rig(backend, AgentState::Idle).await;
        rig.minutes(0, 300).await;
        assert!(rig.reminders().is_empty(), "{backend:?}");
        assert!(rig.events(STALL_EVENT).await.is_empty(), "{backend:?}");
        assert!(rig.events(OBSTACLE_EVENT).await.is_empty(), "{backend:?}");
        assert!(!crate::supervisor::hold::lane_held(&rig.pool, "ledger").await.unwrap());
        let shown = status(&rig.pool, "ledger", T0 + 300 * MINUTE_MS).await.unwrap().unwrap();
        assert_eq!(shown.mode, "observe_only", "{backend:?}");
        assert_eq!(shown.deadline_at_ms, None);
    }
}

#[tokio::test]
async fn the_wall_ceiling_holds_the_lane() {
    let rig = rig(Backend::MhcPane, AgentState::Working).await;
    start_run(&rig.pool, "ledger", Ceilings { wall_minutes: Some(30), tokens: None, cost_micros: None }, T0).await.unwrap();
    rig.minutes(0, 29).await;
    assert!(!crate::supervisor::hold::lane_held(&rig.pool, "ledger").await.unwrap());
    rig.minutes(30, 30).await;
    assert!(crate::supervisor::hold::lane_held(&rig.pool, "ledger").await.unwrap());
    assert_eq!(rig.events(OBSTACLE_EVENT).await.len(), 1);
    assert_eq!(*rig.effects.interrupts.lock().unwrap(), vec!["ledger".to_owned()]);
    // Held: nothing more is sent, and later passes never release it.
    *rig.states.0.lock().unwrap() = (Backend::MhcPane, AgentState::Idle);
    rig.minutes(31, 120).await;
    assert!(rig.reminders().is_empty());
    assert_eq!(rig.events(OBSTACLE_EVENT).await.len(), 1);
    assert!(crate::supervisor::hold::lane_held(&rig.pool, "ledger").await.unwrap());
}

#[tokio::test]
async fn an_unknown_cost_is_reported_unknown() {
    let rig = rig(Backend::Claude, AgentState::Working).await;
    start_run(&rig.pool, "ledger", Ceilings { wall_minutes: None, tokens: Some(1_000), cost_micros: Some(5_000_000) }, T0).await.unwrap();
    let shown = status(&rig.pool, "ledger", T0).await.unwrap().unwrap();
    assert_eq!(shown.cost_micros, CeilingView::Unknown { limit: 5_000_000 });
    assert_eq!(shown.tokens, CeilingView::Unknown { limit: 1_000 });
    assert_eq!(shown.wall, CeilingView::Off);

    // Tokens known, cost left out of the record: cost stays unknown, never "under".
    record_usage(&rig.pool, "ledger", Some(10), None, T0).await.unwrap();
    let shown = status(&rig.pool, "ledger", T0).await.unwrap().unwrap();
    assert_eq!(shown.tokens, CeilingView::Known { used: 10, limit: 1_000 });
    assert_eq!(shown.cost_micros, CeilingView::Unknown { limit: 5_000_000 });
    rig.minutes(0, 5).await;
    assert!(!crate::supervisor::hold::lane_held(&rig.pool, "ledger").await.unwrap());

    // Once a record leaves the cost out, a later known cost does not make the total known.
    record_usage(&rig.pool, "ledger", Some(10), Some(9_000_000), T0).await.unwrap();
    let shown = status(&rig.pool, "ledger", T0).await.unwrap().unwrap();
    assert_eq!(shown.cost_micros, CeilingView::Unknown { limit: 5_000_000 });
}
