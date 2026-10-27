//! Work admission and the initial dispatcher: one request becomes one goal and one initial
//! dispatch, replays and edits never start a second worker, questions and ambiguous scopes
//! launch nothing, a grant never exceeds what the request declared, adoption never widens a
//! legacy goal, goals on one project start in their own worktrees, and every effect is fenced
//! by the claim. Signals are awaited with a bounded deadline; nothing waits on wall-clock time.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;

use crate::agent_control::{
    pty_run_id, AgentControl, AgentOp, Delivery, FixedLauncher, RecordingControl, StartReceipt,
    TaskLauncher, WorkerSend,
};
use crate::goals::contract::{
    contract_of, set_contract, set_mode, set_proposal_state, RequirementSpec,
};
use crate::goals::loop_driver::{approve_proposal, on_turn_settled, LoopOutcome};
use crate::goals::types::{GoalMode, ProposalState, SendOutcome, TurnOrigin};
use crate::lanes::kibitzer::ModelTurn;
use crate::lanes::state::{set_pane, transition, LaneStatus};
use crate::supervisor::admission::{
    adopt_work, handle_message, is_ordinary_question, submit_work, ActorContext, AdmittedWork,
    AdoptWork, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork, LEGACY_GOAL_OFF,
    LEGACY_LANE_LIVE, LEGACY_TERMS_CHANGED, SCOPE_NEEDS_OWNER,
};
use crate::supervisor::dispatcher::{
    DispatchResult, Dispatcher, DISPATCH_FAILED, GRANT_REVOKED, OWNER_APPROVAL_REQUIRED,
    SEND_REFUSED,
};
use crate::supervisor::{
    admit_request, admit_request_with, bind_project_host, claim_dispatch, create_project,
    ensure_identity, execution_binding_of, goal_of, identity_of, ready_dispatches,
    refuse_legacy_goal_action, supervisor_owns_lane, transition_goal, AdmitRequest, AdmitTerms,
    ChannelProvider, ExecutionKind, ExecutionTarget, LegacyGoalRead, RunState,
    CLAIM_UNCONFIRMED_AT_RESTART, DEFAULT_TURN_LIMIT, SUPERVISOR_OWNED,
};

const OWNER: &str = "slack:T1:U-owner";
const STRANGER: &str = "slack:T1:U-stranger";
const HOST: &str = "host-a";
const REPO: &str = "/home/owner/code/ferryx";
const BODY: &str = "Create result.txt containing READY. Run the verifier.";
const PANE: &str = "pane-sv-1";
/// Bound on every awaited signal. Reaching it fails the test; it never makes one pass.
const DEADLINE: Duration = Duration::from_secs(10);

async fn owner_pool() -> SqlitePool {
    let pool = crate::storage::memory().await.unwrap();
    let identity = ensure_identity(&pool, OWNER).await.unwrap();
    let project = create_project(
        &pool,
        &identity.supervisor_id,
        "ferryx",
        "github.com/example/ferryx",
    )
    .await
    .unwrap();
    bind_project_host(&pool, &project.project_id, HOST, &format!("{REPO}/.git"))
        .await
        .unwrap();
    pool
}

fn actor(message_id: &str, body: &str) -> ActorContext {
    ActorContext {
        principal: OWNER.into(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: message_id.into(),
        source_body: body.into(),
        channel_binding_id: None,
        choices: OwnerChoices::default(),
    }
}

fn work() -> SubmitWork {
    SubmitWork {
        project_id: None,
        host_id: None,
        kind: None,
        title: "Write result.txt".into(),
        instruction: "Create result.txt containing READY and run the verifier.".into(),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        cwd: None,
        operations: OperationClass::BASELINE.to_vec(),
        turn_limit: None,
    }
}

fn admitted(outcome: SubmitOutcome) -> AdmittedWork {
    match outcome {
        SubmitOutcome::Admitted(work) => *work,
        other => panic!("expected an admitted goal, got {other:?}"),
    }
}

fn awaiting(outcome: SubmitOutcome) -> AdmittedWork {
    match outcome {
        SubmitOutcome::AwaitingOwner(work) => *work,
        other => panic!("expected a goal awaiting the owner, got {other:?}"),
    }
}

/// The text of an instruction's grant line, up to its first period.
fn allowed_operations(instruction: &str) -> String {
    instruction
        .split("Allowed operations:")
        .nth(1)
        .expect("a grant line")
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// A git repository with one commit at `<tmp>/ferryx-sv-<name>-<pid>/app`. Returns the
/// scratch directory and the repository root, both canonical (and plain on Windows).
fn git_repo(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("ferryx-sv-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("app");
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "sv@example.com"],
        &["config", "user.name", "sv"],
    ] {
        git(&root, args);
    }
    std::fs::write(root.join("a.txt"), "a").unwrap();
    git(&root, &["add", "a.txt"]);
    git(&root, &["-c", "commit.gpgsign=false", "commit", "-qm", "a"]);
    let plain = |path: &Path| {
        crate::lanes::worktree::without_verbatim_prefix(std::fs::canonicalize(path).unwrap())
    };
    (plain(&base), plain(&root))
}

fn git(dir: &Path, args: &[&str]) {
    let output = crate::child_env::std_command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A legacy lane at [`REPO`] with a goal contract in `mode` and `turn_limit`.
async fn legacy_lane(pool: &SqlitePool, key: &str, mode: GoalMode, turn_limit: u32) {
    crate::lanes::state::insert_lane(pool, key, &format!("Legacy {key}"), "lead", REPO, "file marker")
        .await
        .unwrap();
    set_contract(
        pool,
        key,
        &[RequirementSpec {
            id: format!("r-{key}"),
            text: "reconcile runs".into(),
            verifier: "cargo test reconcile".into(),
        }],
        mode,
        turn_limit,
    )
    .await
    .unwrap();
}

fn adopt_of(lane_key: &str) -> AdoptWork {
    AdoptWork {
        lane_key: lane_key.into(),
        project_id: None,
        host_id: None,
        requirements: Vec::new(),
    }
}

/// What `adopt_work` reads of a legacy lane's goal, for an adoption built by hand.
async fn goal_read(pool: &SqlitePool, lane_key: &str) -> Option<LegacyGoalRead> {
    let contract = contract_of(pool, lane_key).await.unwrap();
    Some(contract.map(|contract| (contract.revision, contract.turns_used)))
}

fn dispatcher(
    pool: &SqlitePool,
    control: Arc<dyn AgentControl>,
    launcher: Arc<dyn TaskLauncher>,
) -> Dispatcher {
    Dispatcher::new(pool.clone(), control, launcher, HOST)
}

/// (host, session, request id) of every send the control received.
fn sends(control: &RecordingControl) -> Vec<(Option<String>, String, String)> {
    control
        .ops()
        .into_iter()
        .filter_map(|(host, op)| match op {
            AgentOp::Send {
                session_id,
                request_id,
                ..
            } => Some((host, session_id, request_id)),
            _ => None,
        })
        .collect()
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

async fn dispatch_state(pool: &SqlitePool, request_id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE request_id = ?")
        .bind(request_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn run_state(pool: &SqlitePool, goal_id: &str) -> RunState {
    goal_of(pool, goal_id).await.unwrap().unwrap().run_state
}

/// The legacy lane-goal actions of the desktop and daemon path (`supervisor_goal`) as they
/// reach the store: the fence first, exactly as `hub_runtime::supervisor_goal` calls it, then
/// the legacy write the action makes.
async fn legacy_goal_action(
    pool: &SqlitePool,
    control: &RecordingControl,
    lane_key: &str,
    action: &str,
) -> Result<(), String> {
    refuse_legacy_goal_action(pool, lane_key, action != "show")
        .await
        .map_err(|error| error.to_string())?;
    let written = match action {
        "show" => contract_of(pool, lane_key).await.map(drop),
        "set" => {
            let other = RequirementSpec {
                id: "r-other".into(),
                text: "something else".into(),
                verifier: "true".into(),
            };
            set_contract(pool, lane_key, &[other], GoalMode::Supervised, 5)
                .await
                .map(drop)
        }
        "mode" => set_mode(pool, lane_key, GoalMode::Off).await.map(drop),
        "approve" => approve_proposal(pool, control, lane_key).await.map(drop),
        "discard" => set_proposal_state(
            pool,
            lane_key,
            ProposalState::Discarded,
            Some(&[ProposalState::AwaitingApproval]),
        )
        .await
        .map(drop),
        other => panic!("unknown legacy goal action {other}"),
    };
    written.map_err(|error| error.to_string())
}

type GoalSnapshot = (String, i64, i64, Option<String>, Vec<(String, String, String)>);

/// Mode, budget, revision, proposal state and requirements of a lane's goal contract.
async fn lane_goal_snapshot(pool: &SqlitePool, lane_key: &str) -> GoalSnapshot {
    let (mode, turn_limit, revision, proposal): (String, i64, i64, Option<String>) =
        sqlx::query_as(
            "SELECT mode, turn_limit, revision, proposal_state FROM lane_goal WHERE lane_key = ?",
        )
        .bind(lane_key)
        .fetch_one(pool)
        .await
        .unwrap();
    let requirements: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT req_id, text, verifier FROM lane_goal_requirements WHERE lane_key = ? ORDER BY rowid",
    )
    .bind(lane_key)
    .fetch_all(pool)
    .await
    .unwrap();
    (mode, turn_limit, revision, proposal, requirements)
}

/// Every mutating legacy goal action on `lane_key` is refused with SUPERVISOR_OWNED, and show
/// still answers.
async fn assert_legacy_goal_actions_refused(
    pool: &SqlitePool,
    control: &RecordingControl,
    lane_key: &str,
) {
    for action in ["set", "mode", "approve", "discard"] {
        let refused = timeout(DEADLINE, legacy_goal_action(pool, control, lane_key, action))
            .await
            .expect("a legacy goal action finishes")
            .expect_err(action);
        assert!(refused.starts_with(SUPERVISOR_OWNED), "{action}: {refused}");
    }
    legacy_goal_action(pool, control, lane_key, "show")
        .await
        .expect("show is never fenced");
}

/// Pauses the goal from inside the pane launch, i.e. after the claim and before transmit.
struct PausingLauncher {
    pool: SqlitePool,
    goal_id: String,
}

#[async_trait]
impl TaskLauncher for PausingLauncher {
    async fn launch_pane(&self, _cwd: &str, _command: &[String]) -> crate::error::Result<String> {
        let goal = goal_of(&self.pool, &self.goal_id).await.unwrap().unwrap();
        transition_goal(
            &self.pool,
            &self.goal_id,
            goal.revision,
            RunState::Dispatching,
            RunState::Paused,
            Some("owner pause"),
        )
        .await
        .unwrap();
        Ok(PANE.to_owned())
    }
}

/// Launches `pane-1`, `pane-2`, ... so each goal gets its own pane, and records each cwd.
#[derive(Default)]
struct SequenceLauncher {
    launched: AtomicUsize,
    cwds: Mutex<Vec<String>>,
}

#[async_trait]
impl TaskLauncher for SequenceLauncher {
    async fn launch_pane(&self, cwd: &str, _command: &[String]) -> crate::error::Result<String> {
        self.cwds.lock().unwrap().push(cwd.to_owned());
        let number = self.launched.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(format!("pane-{number}"))
    }
}

/// Breaks a table the post-claim grant check reads, from inside the pane launch: a store
/// error after the claim and before the transmit.
struct BreakingLauncher {
    pool: SqlitePool,
}

#[async_trait]
impl TaskLauncher for BreakingLauncher {
    async fn launch_pane(&self, _cwd: &str, _command: &[String]) -> crate::error::Result<String> {
        sqlx::query("ALTER TABLE supervisor_project_hosts RENAME TO supervisor_project_hosts_gone")
            .execute(&self.pool)
            .await
            .unwrap();
        Ok(PANE.to_owned())
    }
}

/// Reports every transmitted request id on a channel the test awaits, and answers with the
/// target's start receipt for exactly that request.
struct SignalControl {
    sent: mpsc::UnboundedSender<String>,
}

/// A transport that refuses every send: a person is typing in the pane. Nothing is typed.
struct RefusingControl;

#[async_trait]
impl AgentControl for RefusingControl {
    async fn execute(&self, _host_id: Option<&str>, _op: AgentOp) -> crate::error::Result<Value> {
        Err(crate::error::HubError::message(
            "CONTROL_CONFLICT a person typed in this pane",
        ))
    }

    async fn deliver(&self, _send: &WorkerSend) -> Delivery {
        Delivery::Refused {
            code: "CONTROL_CONFLICT".into(),
        }
    }
}

#[async_trait]
impl AgentControl for SignalControl {
    async fn execute(&self, _host_id: Option<&str>, op: AgentOp) -> crate::error::Result<Value> {
        if let AgentOp::Send { request_id, .. } = op {
            let _ = self.sent.send(request_id);
        }
        Ok(json!({"ok": true}))
    }

    async fn deliver(&self, send: &WorkerSend) -> Delivery {
        let _ = self.sent.send(send.request_id.clone());
        Delivery::Started(StartReceipt {
            request_id: send.request_id.clone(),
            session_id: send.session_id.clone(),
            run_id: pty_run_id(&send.session_id, RecordingControl::EPOCH, 1),
        })
    }
}

#[tokio::test]
async fn one_request_admits_one_goal_and_starts_one_worker_without_a_drive_call() {
    let pool = owner_pool().await;
    let work = admitted(
        submit_work(&pool, &actor("m-1", BODY), &work())
            .await
            .unwrap(),
    );
    assert!(!work.duplicate);
    assert_eq!(work.run_state, RunState::Ready);
    assert_eq!(work.mode, GoalMode::Full, "an owner execution request continues on its own");
    assert_eq!(
        (work.turn_limit, work.turns_remaining),
        (DEFAULT_TURN_LIMIT, DEFAULT_TURN_LIMIT)
    );
    assert_eq!(work.grant, OperationClass::BASELINE.to_vec());
    assert!(
        work.acknowledgment.contains(&work.receipt.goal_id),
        "{}",
        work.acknowledgment
    );
    let request_id = work.receipt.dispatch_request_id.clone();
    assert_eq!(
        crate::goals::turns::origin_for_request_id(Some(&request_id)),
        Some(TurnOrigin::SubmittedRequest)
    );
    assert!(crate::goals::turns::records_own_turn(Some(&request_id)));

    let control = Arc::new(RecordingControl::observing());
    let launcher = Arc::new(FixedLauncher::new(PANE));
    let dispatcher = dispatcher(&pool, control.clone(), launcher.clone());
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Started,
        }]
    );
    assert_eq!(
        launcher.launched.lock().unwrap().clone(),
        vec![(REPO.to_owned(), Vec::<String>::new())]
    );
    assert_eq!(
        sends(&control),
        vec![(None, PANE.to_owned(), request_id.clone())]
    );
    assert_eq!(run_state(&pool, &work.receipt.goal_id).await, RunState::Running);
    let binding = execution_binding_of(&pool, &work.receipt.goal_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.provider_session_id.as_deref(), Some(PANE));
    assert_eq!(
        binding.run_id,
        Some(pty_run_id(PANE, RecordingControl::EPOCH, 1)),
        "the run is the one the target's start receipt named, not the request id"
    );
    assert_eq!(dispatch_state(&pool, &request_id).await, "started");
    let (status, pane): (String, Option<String>) =
        sqlx::query_as("SELECT status, omo_thread_id FROM lanes WHERE key = ?")
            .bind(&work.receipt.lane_key)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "working");
    assert_eq!(pane, None, "the pane is bound through the execution binding only");
    let turns = crate::goals::turns::turns_of(&pool, &work.receipt.lane_key)
        .await
        .unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].origin, TurnOrigin::SubmittedRequest);

    assert!(dispatcher.drain().await.is_empty(), "nothing is left to send");
    assert_eq!(sends(&control).len(), 1);
}

#[tokio::test]
async fn a_replayed_source_message_returns_the_same_receipt_and_starts_no_second_worker() {
    let pool = owner_pool().await;
    let first = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let control = Arc::new(RecordingControl::observing());
    let launcher = Arc::new(FixedLauncher::new(PANE));
    let dispatcher = dispatcher(&pool, control.clone(), launcher.clone());

    let before = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    assert!(before.duplicate);
    assert_eq!(before.receipt, first.receipt);
    assert_eq!(dispatcher.drain().await.len(), 1);
    let after = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    assert!(after.duplicate);
    assert_eq!(after.receipt, first.receipt);
    assert_eq!(after.run_state, RunState::Running);
    assert!(dispatcher.drain().await.is_empty());

    assert_eq!(sends(&control).len(), 1);
    assert_eq!(launcher.launched.lock().unwrap().len(), 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_dispatches").await, 1);
}

#[tokio::test]
async fn a_changed_body_for_the_same_source_is_a_conflict_and_starts_nothing() {
    let pool = owner_pool().await;
    admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let control = Arc::new(RecordingControl::observing());
    let dispatcher = dispatcher(&pool, control.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert_eq!(dispatcher.drain().await.len(), 1);

    let edited = actor("m-1", "Create result.txt containing DONE instead.");
    let error = submit_work(&pool, &edited, &work()).await.unwrap_err();
    assert_eq!(error.code(), "REQUEST_CONFLICT", "{error}");
    assert!(dispatcher.drain().await.is_empty());
    assert_eq!(sends(&control).len(), 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_dispatches").await, 1);
}

#[tokio::test]
async fn a_replay_returns_the_admitted_goal_even_after_a_second_project_appears() {
    let pool = owner_pool().await;
    let first = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let identity = identity_of(&pool, OWNER).await.unwrap().unwrap();
    let other = create_project(&pool, &identity.supervisor_id, "mhc", "github.com/example/mhc")
        .await
        .unwrap();
    bind_project_host(&pool, &other.project_id, HOST, "/home/owner/code/mhc/.git")
        .await
        .unwrap();

    // The dedupe row answers before the scope is resolved again.
    let replay = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    assert!(replay.duplicate);
    assert_eq!(replay.receipt, first.receipt);
    assert_eq!(
        replay.grant,
        OperationClass::BASELINE.to_vec(),
        "a replay reports the persisted grant"
    );
    let edited = actor("m-1", "Create result.txt containing DONE instead.");
    let error = submit_work(&pool, &edited, &work()).await.unwrap_err();
    assert_eq!(error.code(), "REQUEST_CONFLICT", "{error}");
    // A new message is resolved afresh and now needs the owner's pick.
    let outcome = submit_work(&pool, &actor("m-2", BODY), &work()).await.unwrap();
    assert!(
        matches!(outcome, SubmitOutcome::NeedsChoice { ref missing, .. } if missing == "project"),
        "{outcome:?}"
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_requests").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_dispatches").await, 1);
}

#[tokio::test]
async fn a_read_only_request_is_granted_no_write_operation() {
    let pool = owner_pool().await;
    let mut asked = work();
    asked.title = "Investigate red CI".into();
    asked.instruction = "Find out why CI is red and report the cause.".into();
    asked.operations = Vec::new();
    let body = "Investigate why CI is red. Do not change any files.";
    let read_only = admitted(submit_work(&pool, &actor("m-ci", body), &asked).await.unwrap());
    assert_eq!(read_only.run_state, RunState::Ready);
    assert!(read_only.grant.is_empty(), "{:?}", read_only.grant);
    let (instruction, stored): (String, String) = sqlx::query_as(
        "SELECT d.instruction, g.operation_grant FROM supervisor_dispatches d
         JOIN supervisor_goals g ON g.goal_id = d.goal_id WHERE d.request_id = ?",
    )
    .bind(&read_only.receipt.dispatch_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, "[]");
    let allowed = allowed_operations(&instruction);
    assert!(
        !allowed.contains("edit") && !allowed.contains("commit") && !allowed.contains("test"),
        "{instruction}"
    );

    // A declared subset is granted exactly: running tests grants no edit or commit.
    let mut tests_only = work();
    tests_only.operations = vec![OperationClass::Test];
    let tested = admitted(
        submit_work(&pool, &actor("m-test", "Run the test suite and report."), &tests_only)
            .await
            .unwrap(),
    );
    assert_eq!(tested.grant, vec![OperationClass::Test]);
    let instruction: String =
        sqlx::query_scalar("SELECT instruction FROM supervisor_dispatches WHERE request_id = ?")
            .bind(&tested.receipt.dispatch_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(allowed_operations(&instruction), "test", "{instruction}");
}

#[tokio::test]
async fn two_goals_on_one_project_start_in_their_own_worktrees_not_the_checkout() {
    crate::lanes::worktree::isolate_test_env();
    let (base, root) = git_repo("two-goals");
    let checkout = root.to_string_lossy().into_owned();
    let pool = crate::storage::memory().await.unwrap();
    let identity = ensure_identity(&pool, OWNER).await.unwrap();
    let project = create_project(&pool, &identity.supervisor_id, "app", "github.com/example/app")
        .await
        .unwrap();
    bind_project_host(&pool, &project.project_id, HOST, &format!("{checkout}/.git"))
        .await
        .unwrap();
    let first = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let second = admitted(
        submit_work(&pool, &actor("m-2", "Create notes.txt containing DONE."), &work())
            .await
            .unwrap(),
    );

    let control = Arc::new(RecordingControl::observing());
    let launcher = Arc::new(SequenceLauncher::default());
    let results = dispatcher(&pool, control.clone(), launcher.clone()).drain().await;
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results.iter().all(|result| matches!(
            result,
            DispatchResult::Transmitted {
                outcome: SendOutcome::Started,
                ..
            }
        )),
        "{results:?}"
    );
    let lanes_dir = root.parent().unwrap().join("app-lanes");
    let mut expected: Vec<String> = [&first, &second]
        .iter()
        .map(|goal| {
            lanes_dir
                .join(&goal.receipt.lane_key)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    expected.sort();
    let mut launched = launcher.cwds.lock().unwrap().clone();
    launched.sort();
    assert_eq!(launched, expected, "each goal starts in its own task-owned worktree");
    assert!(!launched.contains(&checkout), "no worker starts in the owner's checkout");
    for goal in [&first, &second] {
        let cwd: String = sqlx::query_scalar("SELECT cwd FROM lanes WHERE key = ?")
            .bind(&goal.receipt.lane_key)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            Path::new(&cwd),
            lanes_dir.join(&goal.receipt.lane_key),
            "the lane records its worktree"
        );
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn a_store_error_after_the_claim_releases_it_and_sends_nothing() {
    let pool = owner_pool().await;
    let work = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let control = Arc::new(RecordingControl::observing());
    let launcher = Arc::new(BreakingLauncher { pool: pool.clone() });
    let results = dispatcher(&pool, control.clone(), launcher).drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { state: RunState::Degraded, detail, .. }]
                if detail.starts_with(DISPATCH_FAILED)
        ),
        "{results:?}"
    );
    assert!(sends(&control).is_empty());
    assert_eq!(
        dispatch_state(&pool, &work.receipt.dispatch_request_id).await,
        "released",
        "no claim is left behind"
    );
    assert_eq!(run_state(&pool, &work.receipt.goal_id).await, RunState::Degraded);
}

#[tokio::test]
async fn an_ordinary_question_creates_no_goal() {
    let pool = owner_pool().await;
    let question = actor("m-q", "What can you do?");
    let outcome = handle_message(&pool, &question, None).await.unwrap();
    assert!(matches!(outcome, SubmitOutcome::Conversational { .. }), "{outcome:?}");
    // A model that calls submit_work on a question still launches nothing.
    let outcome = submit_work(&pool, &question, &work()).await.unwrap();
    assert!(matches!(outcome, SubmitOutcome::Conversational { .. }), "{outcome:?}");
    assert!(is_ordinary_question("What can you do?"));
    assert!(!is_ordinary_question("Can you fix the build?"));
    assert!(!is_ordinary_question("Why is CI red? Fix it?"));

    let control = Arc::new(RecordingControl::observing());
    assert!(dispatcher(&pool, control.clone(), Arc::new(FixedLauncher::new(PANE)))
        .drain()
        .await
        .is_empty());
    assert!(control.ops().is_empty());
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_requests").await, 0);
}

#[tokio::test]
async fn a_scope_beyond_the_grant_waits_for_the_owner_and_dispatches_nothing() {
    let pool = owner_pool().await;
    let mut asked = work();
    asked.operations = vec![OperationClass::Publish];
    asked.turn_limit = Some(500);
    let outcome = submit_work(&pool, &actor("m-pub", "Fix the docs and publish them."), &asked)
        .await
        .unwrap();
    let held = match outcome {
        SubmitOutcome::AwaitingOwner(held) => *held,
        other => panic!("expected awaiting_owner, got {other:?}"),
    };
    assert_eq!(held.run_state, RunState::AwaitingOwner);
    assert_eq!(held.mode, GoalMode::Supervised);
    assert_eq!(held.revision, 2, "the hold supersedes the admitted intent");
    assert_eq!(held.turn_limit, DEFAULT_TURN_LIMIT, "a model cannot raise the budget");
    assert!(
        held.state_detail.as_deref().unwrap_or("").starts_with(SCOPE_NEEDS_OWNER),
        "the specific hold reason is committed with the goal: {:?}",
        held.state_detail
    );
    assert!(held.state_detail.as_deref().unwrap_or("").contains("publish"));
    assert!(held.acknowledgment.contains("Nothing has been sent"));

    let mut outside = work();
    outside.cwd = Some("/etc".into());
    let outcome = submit_work(&pool, &actor("m-etc", "Tidy up the config."), &outside)
        .await
        .unwrap();
    assert!(matches!(outcome, SubmitOutcome::AwaitingOwner(_)), "{outcome:?}");

    let control = Arc::new(RecordingControl::observing());
    let launcher = Arc::new(FixedLauncher::new(PANE));
    let dispatcher = dispatcher(&pool, control.clone(), launcher.clone());
    assert!(dispatcher.drain().await.is_empty());
    assert!(control.ops().is_empty());
    assert!(launcher.launched.lock().unwrap().is_empty());
    assert_eq!(dispatch_state(&pool, &held.receipt.dispatch_request_id).await, "superseded");

    // Only the owner's explicit choice widens a grant.
    let mut chosen = actor("m-pub-2", "Fix the docs and publish them to the site.");
    chosen.choices.operations = vec![OperationClass::Publish];
    let granted = admitted(submit_work(&pool, &chosen, &asked).await.unwrap());
    assert_eq!(granted.run_state, RunState::Ready);
    assert!(granted.grant.contains(&OperationClass::Publish));

    // With two projects and no channel or choice naming one, the owner picks first.
    let identity = identity_of(&pool, OWNER).await.unwrap().unwrap();
    let other = create_project(&pool, &identity.supervisor_id, "mhc", "github.com/example/mhc")
        .await
        .unwrap();
    bind_project_host(&pool, &other.project_id, HOST, "/home/owner/code/mhc/.git")
        .await
        .unwrap();
    let goals = count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await;
    let outcome = submit_work(&pool, &actor("m-which", "Run the tests and fix failures."), &work())
        .await
        .unwrap();
    assert!(
        matches!(outcome, SubmitOutcome::NeedsChoice { ref missing, .. } if missing == "project"),
        "{outcome:?}"
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, goals);
}

#[tokio::test]
async fn an_owner_pause_between_claim_and_transmit_sends_nothing() {
    let pool = owner_pool().await;
    let work = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let control = Arc::new(RecordingControl::observing());
    let launcher = Arc::new(PausingLauncher {
        pool: pool.clone(),
        goal_id: work.receipt.goal_id.clone(),
    });
    let results = dispatcher(&pool, control.clone(), launcher).drain().await;
    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0], DispatchResult::Released { code, .. } if code == "SUPERSEDED"),
        "{results:?}"
    );
    assert!(sends(&control).is_empty(), "a paused goal is never transmitted to");
    assert_eq!(
        dispatch_state(&pool, &work.receipt.dispatch_request_id).await,
        "released"
    );
    assert_eq!(run_state(&pool, &work.receipt.goal_id).await, RunState::Paused);
    let again = dispatcher(&pool, control.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert!(again.drain().await.is_empty());
    assert!(sends(&control).is_empty());
}

#[tokio::test]
async fn a_committed_ready_intent_is_drained_exactly_once_across_dispatchers() {
    let pool = owner_pool().await;
    let work = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let control = Arc::new(RecordingControl::observing());
    let launcher = Arc::new(FixedLauncher::new(PANE));
    let first = dispatcher(&pool, control.clone(), launcher.clone());
    let second = dispatcher(&pool, control.clone(), launcher.clone());
    let (one, two) = tokio::join!(first.drain(), second.drain());
    let transmitted = one
        .iter()
        .chain(two.iter())
        .filter(|result| matches!(result, DispatchResult::Transmitted { .. }))
        .count();
    assert_eq!(transmitted, 1, "{one:?} {two:?}");
    assert_eq!(
        sends(&control),
        vec![(None, PANE.to_owned(), work.receipt.dispatch_request_id.clone())]
    );
    assert_eq!(launcher.launched.lock().unwrap().len(), 1);
    assert!(first.drain().await.is_empty());
    assert!(second.drain().await.is_empty());
}

#[tokio::test]
async fn a_refused_send_is_held_for_the_owner_and_never_resent() {
    let pool = owner_pool().await;
    let work = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let refusing = dispatcher(&pool, Arc::new(RefusingControl), Arc::new(FixedLauncher::new(PANE)));
    let results = refusing.drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { state: RunState::AwaitingOwner, detail, .. }]
                if detail.starts_with(SEND_REFUSED) && detail.contains("CONTROL_CONFLICT")
        ),
        "{results:?}"
    );
    // Nothing was typed, so the claim is given back, not left uncertain.
    let (state, code): (String, Option<String>) = sqlx::query_as(
        "SELECT state, uncertain_code FROM supervisor_dispatches WHERE request_id = ?",
    )
    .bind(&work.receipt.dispatch_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((state.as_str(), code.as_deref()), ("released", None));
    assert_eq!(run_state(&pool, &work.receipt.goal_id).await, RunState::AwaitingOwner);
    let control = Arc::new(RecordingControl::observing());
    let again = dispatcher(&pool, control.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert!(again.drain().await.is_empty());
    assert!(sends(&control).is_empty(), "a refused dispatch waits for the owner's resolution");
}

#[tokio::test]
async fn the_dispatcher_drains_at_start_and_subscribes_before_the_next_trigger() {
    let pool = owner_pool().await;
    // Committed before any dispatcher exists: the startup drain must pick it up.
    let early = admitted(submit_work(&pool, &actor("m-early", BODY), &work()).await.unwrap());
    let (sent_tx, mut sent_rx) = mpsc::unbounded_channel();
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel();
    let wake = Arc::new(Notify::new());
    let dispatcher = Dispatcher::new(
        pool.clone(),
        Arc::new(SignalControl { sent: sent_tx }),
        Arc::new(SequenceLauncher::default()),
        HOST,
    )
    // The rescan is an hour away, so only the wake can deliver the second intent in time.
    .with_rescan(Duration::from_secs(3600))
    .with_drain_probe(probe_tx);
    let task = tokio::spawn(dispatcher.run(Arc::clone(&wake)));

    let sent = timeout(DEADLINE, sent_rx.recv()).await.expect("startup drain");
    assert_eq!(sent.as_deref(), Some(early.receipt.dispatch_request_id.as_str()));
    assert_eq!(timeout(DEADLINE, probe_rx.recv()).await.expect("probe"), Some(1));

    // The loop enabled its wake subscription before that drain, so this trigger is kept
    // whether or not the loop is already waiting.
    let late = admitted(
        submit_work(&pool, &actor("m-late", "Create notes.txt containing DONE."), &work())
            .await
            .unwrap(),
    );
    wake.notify_one();
    let sent = timeout(DEADLINE, sent_rx.recv()).await.expect("woken drain");
    assert_eq!(sent.as_deref(), Some(late.receipt.dispatch_request_id.as_str()));
    assert_eq!(timeout(DEADLINE, probe_rx.recv()).await.expect("probe"), Some(1));
    task.abort();
    assert!(sent_rx.try_recv().is_err(), "each intent was sent exactly once");
    assert_eq!(run_state(&pool, &early.receipt.goal_id).await, RunState::Running);
    assert_eq!(run_state(&pool, &late.receipt.goal_id).await, RunState::Running);
}

#[tokio::test]
async fn a_claim_left_by_a_stopped_dispatcher_is_held_uncertain_and_never_resent() {
    let pool = owner_pool().await;
    let work = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let binding = execution_binding_of(&pool, &work.receipt.goal_id)
        .await
        .unwrap()
        .unwrap();
    // A previous dispatcher claimed the intent and stopped before any receipt; whether its
    // bytes left cannot be known.
    claim_dispatch(
        &pool,
        &work.receipt.dispatch_request_id,
        work.revision,
        binding.generation,
    )
    .await
    .unwrap();
    let (sent_tx, mut sent_rx) = mpsc::unbounded_channel();
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel();
    let restarted = Dispatcher::new(
        pool.clone(),
        Arc::new(SignalControl { sent: sent_tx }),
        Arc::new(SequenceLauncher::default()),
        HOST,
    )
    .with_rescan(Duration::from_secs(3600))
    .with_drain_probe(probe_tx);
    let task = tokio::spawn(restarted.run(Arc::new(Notify::new())));
    // The startup drain runs after reconciliation and finds nothing it may send.
    assert_eq!(timeout(DEADLINE, probe_rx.recv()).await.expect("probe"), Some(0));
    task.abort();
    assert!(sent_rx.try_recv().is_err(), "the unconfirmed claim is not resent");
    let goal = goal_of(&pool, &work.receipt.goal_id).await.unwrap().unwrap();
    assert_eq!(goal.run_state, RunState::DeliveryUncertain);
    assert_eq!(goal.state_detail.as_deref(), Some(CLAIM_UNCONFIRMED_AT_RESTART));
    let (state, code): (String, Option<String>) = sqlx::query_as(
        "SELECT state, uncertain_code FROM supervisor_dispatches WHERE request_id = ?",
    )
    .bind(&work.receipt.dispatch_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (state.as_str(), code.as_deref()),
        ("uncertain", Some(CLAIM_UNCONFIRMED_AT_RESTART))
    );
    let control = Arc::new(RecordingControl::observing());
    let again = dispatcher(&pool, control.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert!(again.drain().await.is_empty());
    assert!(sends(&control).is_empty());
}

#[tokio::test]
async fn a_stranger_cannot_submit_or_adopt_and_model_arguments_cannot_name_an_actor() {
    let pool = owner_pool().await;
    let mut stranger = actor("m-x", BODY);
    stranger.principal = STRANGER.into();
    let error = submit_work(&pool, &stranger, &work()).await.unwrap_err();
    assert_eq!(error.code(), "UNAUTHORIZED");
    let adopt = AdoptWork {
        lane_key: "ledger".into(),
        project_id: None,
        host_id: None,
        requirements: Vec::new(),
    };
    let error = adopt_work(&pool, &stranger, &adopt).await.unwrap_err();
    assert_eq!(error.code(), "UNAUTHORIZED");
    let injected = serde_json::from_value::<SubmitWork>(json!({
        "title": "t", "instruction": "i", "requirements": [], "principal": OWNER
    }));
    assert!(injected.is_err(), "unknown model fields are refused");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 0);
}

#[tokio::test]
async fn adopting_a_legacy_lane_keeps_its_terms_and_happens_once() {
    let pool = owner_pool().await;
    legacy_lane(&pool, "ledger", GoalMode::Supervised, 3).await;
    sqlx::query("UPDATE lane_goal SET turns_used = 1 WHERE lane_key = 'ledger'")
        .execute(&pool)
        .await
        .unwrap();
    let adopt = adopt_of("ledger");
    // A supervised legacy goal is never widened: it waits for the owner with what it had left.
    let first = awaiting(adopt_work(&pool, &actor("m-adopt", "adopt ledger"), &adopt).await.unwrap());
    assert_eq!(first.mode, GoalMode::Supervised);
    assert_eq!(first.turn_limit, 2, "the legacy budget left, not a fresh one");
    assert!(
        first
            .state_detail
            .as_deref()
            .unwrap_or("")
            .starts_with(OWNER_APPROVAL_REQUIRED),
        "{:?}",
        first.state_detail
    );
    let mut desktop = actor("d-adopt", "adopt ledger");
    desktop.source_adapter = ChannelProvider::Desktop;
    let again = awaiting(adopt_work(&pool, &desktop, &adopt).await.unwrap());
    assert!(again.duplicate);
    assert_eq!(again.receipt, first.receipt);
    let contract = contract_of(&pool, &first.receipt.lane_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(contract.requirements[0].id, "r-ledger");

    let refused = AdoptWork {
        lane_key: first.receipt.lane_key.clone(),
        ..adopt
    };
    let error = adopt_work(&pool, &actor("m-adopt-2", "adopt it"), &refused)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST");

    let control = Arc::new(RecordingControl::observing());
    let dispatcher = dispatcher(&pool, control.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert!(dispatcher.drain().await.is_empty(), "a held adoption is not dispatched");
    assert!(sends(&control).is_empty());
    let legacy_pane: Option<String> =
        sqlx::query_scalar("SELECT omo_thread_id FROM lanes WHERE key = 'ledger'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(legacy_pane, None, "the legacy lane is left as history");

    // A full legacy goal stays full, with at most the default budget, and starts once.
    legacy_lane(&pool, "nightly", GoalMode::Full, 50).await;
    let full = admitted(
        adopt_work(&pool, &actor("m-nightly", "adopt nightly"), &adopt_of("nightly"))
            .await
            .unwrap(),
    );
    assert_eq!(full.mode, GoalMode::Full);
    assert_eq!(full.turn_limit, DEFAULT_TURN_LIMIT);
    assert_eq!(dispatcher.drain().await.len(), 1);
    assert_eq!(
        sends(&control),
        vec![(None, PANE.to_owned(), full.receipt.dispatch_request_id.clone())]
    );

    // An off legacy goal and a lane the legacy path still drives are refused.
    legacy_lane(&pool, "dormant", GoalMode::Off, 5).await;
    let error = adopt_work(&pool, &actor("m-dormant", "adopt dormant"), &adopt_of("dormant"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), LEGACY_GOAL_OFF, "{error}");
    legacy_lane(&pool, "busy", GoalMode::Full, 5).await;
    set_pane(&pool, "busy", "pane-legacy").await.unwrap();
    transition(&pool, "busy", LaneStatus::Working, Some("launched"))
        .await
        .unwrap();
    let error = adopt_work(&pool, &actor("m-busy", "adopt busy"), &adopt_of("busy"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), LEGACY_LANE_LIVE, "{error}");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 2);
    assert!(dispatcher.drain().await.is_empty());
    assert_eq!(sends(&control).len(), 1);
}

#[tokio::test]
async fn a_legacy_goal_action_on_a_supervisor_lane_is_refused_and_changes_nothing() {
    let pool = owner_pool().await;
    let work = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    let lane = work.receipt.lane_key.clone();
    // A pane and a proposal awaiting approval, so an unfenced approve or discard would act.
    set_pane(&pool, &lane, "pane-legacy").await.unwrap();
    sqlx::query(
        "UPDATE lane_goal SET proposal_state = 'awaiting_approval', proposal_text = 'Run the verifier.',
                proposal_gap_ids = '[]', proposal_at = ? WHERE lane_key = ?",
    )
    .bind(crate::storage::now_rfc3339())
    .bind(&lane)
    .execute(&pool)
    .await
    .unwrap();
    let before = lane_goal_snapshot(&pool, &lane).await;
    assert_eq!(before.0, "full");

    let control = RecordingControl::default();
    assert_legacy_goal_actions_refused(&pool, &control, &lane).await;
    assert_eq!(lane_goal_snapshot(&pool, &lane).await, before);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM lane_goal_revisions").await, 0);
    assert!(control.ops().is_empty(), "nothing reached a pane");
    let goal = goal_of(&pool, &work.receipt.goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Ready, work.revision));

    // A lane the supervisor does not own keeps the legacy APIs.
    legacy_lane(&pool, "ledger", GoalMode::Supervised, 3).await;
    legacy_goal_action(&pool, &control, "ledger", "mode").await.unwrap();
    assert_eq!(lane_goal_snapshot(&pool, "ledger").await.0, "off");
}

#[tokio::test]
async fn an_adopted_legacy_lane_is_fenced_from_the_legacy_goal_apis() {
    let pool = owner_pool().await;
    legacy_lane(&pool, "nightly", GoalMode::Full, 50).await;
    assert!(!supervisor_owns_lane(&pool, "nightly").await.unwrap());
    refuse_legacy_goal_action(&pool, "nightly", true).await.unwrap();
    let adopted = admitted(
        adopt_work(&pool, &actor("m-nightly", "adopt nightly"), &adopt_of("nightly"))
            .await
            .unwrap(),
    );
    assert!(supervisor_owns_lane(&pool, "nightly").await.unwrap());
    assert!(supervisor_owns_lane(&pool, &adopted.receipt.lane_key).await.unwrap());

    let before = lane_goal_snapshot(&pool, "nightly").await;
    let control = RecordingControl::default();
    assert_legacy_goal_actions_refused(&pool, &control, "nightly").await;
    assert_eq!(
        lane_goal_snapshot(&pool, "nightly").await,
        before,
        "the legacy contract cannot start a second worker on the adopted work"
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM lane_goal_revisions").await, 0);
    assert!(control.ops().is_empty());
}

#[tokio::test]
async fn an_unreadable_operation_grant_holds_the_goal_and_sends_nothing() {
    let pool = owner_pool().await;
    let work = admitted(submit_work(&pool, &actor("m-1", BODY), &work()).await.unwrap());
    sqlx::query("UPDATE supervisor_goals SET operation_grant = '[\"root\"]' WHERE goal_id = ?")
        .bind(&work.receipt.goal_id)
        .execute(&pool)
        .await
        .unwrap();
    let control = Arc::new(RecordingControl::default());
    let launcher = Arc::new(FixedLauncher::new(PANE));
    let results = dispatcher(&pool, control.clone(), launcher.clone()).drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { state: RunState::AwaitingOwner, detail, .. }]
                if detail.starts_with(GRANT_REVOKED)
        ),
        "{results:?}"
    );
    assert!(sends(&control).is_empty());
    assert!(launcher.launched.lock().unwrap().is_empty(), "no pane is launched");
    assert_eq!(
        dispatch_state(&pool, &work.receipt.dispatch_request_id).await,
        "superseded"
    );
    assert_eq!(run_state(&pool, &work.receipt.goal_id).await, RunState::AwaitingOwner);
}

#[tokio::test]
async fn an_adoption_rechecks_its_legacy_lane_inside_the_admission_transaction() {
    let pool = owner_pool().await;
    legacy_lane(&pool, "busy", GoalMode::Full, 5).await;
    set_pane(&pool, "busy", "pane-legacy").await.unwrap();
    transition(&pool, "busy", LaneStatus::Working, Some("launched"))
        .await
        .unwrap();
    let identity = identity_of(&pool, OWNER).await.unwrap().unwrap();
    let project_id: String = sqlx::query_scalar("SELECT project_id FROM supervisor_projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    let request = AdmitRequest {
        supervisor_id: identity.supervisor_id.clone(),
        owner_principal: OWNER.into(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: "adopt:busy".into(),
        body: "Adopt legacy lane busy: Legacy busy".into(),
        project_id,
        channel_binding_id: None,
        target: ExecutionTarget {
            kind: ExecutionKind::Pty,
            host_id: HOST.into(),
        },
        title: "Legacy busy".into(),
        cwd: REPO.into(),
        instruction: "Continue the work of legacy lane busy.".into(),
        requirements: work().requirements,
        mode: GoalMode::Full,
        turn_limit: 5,
        operation_grant: OperationClass::BASELINE.to_vec(),
    };
    let adopts = AdmitTerms {
        owner_hold: None,
        adopts_lane: Some("busy".into()),
        legacy_goal_read: goal_read(&pool, "busy").await,
    };
    // The store itself refuses while the legacy worker is attached, under its write lock. The
    // other order, an adoption committing during a legacy launch, is refused by the legacy
    // attach (an_adoption_during_a_legacy_launch_...).
    let error = admit_request_with(&pool, &request, &adopts).await.unwrap_err();
    assert_eq!(error.code(), LEGACY_LANE_LIVE, "{error}");
    for table in ["supervisor_goals", "supervisor_requests", "supervisor_dispatches"] {
        assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM {table}")).await, 0, "{table}");
    }
    // `adopt:` ids are reserved for adoption, and an adoption names its lane in the id.
    let error = admit_request(&pool, &request).await.unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST", "{error}");
    let elsewhere = AdmitTerms {
        owner_hold: None,
        adopts_lane: Some("ledger".into()),
        legacy_goal_read: Some(None),
    };
    let error = admit_request_with(&pool, &request, &elsewhere).await.unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST", "{error}");
    // An adoption always carries the legacy goal it read; without it nothing is re-checked.
    let unread = AdmitTerms {
        legacy_goal_read: None,
        ..adopts.clone()
    };
    let error = admit_request_with(&pool, &request, &unread).await.unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST", "{error}");
    assert!(!supervisor_owns_lane(&pool, "busy").await.unwrap());

    // Once the legacy worker is gone the adoption commits, and a held one is committed
    // directly on hold: never `ready`, so nothing can claim it.
    sqlx::query("UPDATE lanes SET omo_thread_id = NULL WHERE key = 'busy'")
        .execute(&pool)
        .await
        .unwrap();
    let detail = format!("{OWNER_APPROVAL_REQUIRED}: the owner decides first");
    let held = AdmitTerms {
        owner_hold: Some(detail.clone()),
        adopts_lane: Some("busy".into()),
        legacy_goal_read: goal_read(&pool, "busy").await,
    };
    let admission = admit_request_with(&pool, &request, &held).await.unwrap();
    assert!(!admission.duplicate);
    assert_eq!(admission.receipt.revision, 1, "the receipt names the admitted revision");
    let goal = goal_of(&pool, &admission.receipt.goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::AwaitingOwner, 2));
    assert_eq!(goal.state_detail.as_deref(), Some(detail.as_str()));
    assert_eq!(
        dispatch_state(&pool, &admission.receipt.dispatch_request_id).await,
        "superseded"
    );
    assert!(ready_dispatches(&pool, 16).await.unwrap().is_empty());
    assert!(supervisor_owns_lane(&pool, "busy").await.unwrap());
}

/// The admission an adoption of legacy lane `lane_key` makes, as `adopt_work` builds it.
async fn adoption_request(pool: &SqlitePool, lane_key: &str, adapter: ChannelProvider) -> AdmitRequest {
    let identity = identity_of(pool, OWNER).await.unwrap().unwrap();
    let project_id: String = sqlx::query_scalar("SELECT project_id FROM supervisor_projects")
        .fetch_one(pool)
        .await
        .unwrap();
    AdmitRequest {
        supervisor_id: identity.supervisor_id,
        owner_principal: OWNER.into(),
        source_adapter: adapter,
        source_message_id: format!("adopt:{lane_key}"),
        body: format!("Adopt legacy lane {lane_key}: Legacy {lane_key}"),
        project_id,
        channel_binding_id: None,
        target: ExecutionTarget {
            kind: ExecutionKind::Pty,
            host_id: HOST.into(),
        },
        title: format!("Legacy {lane_key}"),
        cwd: REPO.into(),
        instruction: format!("Continue the work of legacy lane {lane_key}."),
        requirements: work().requirements,
        mode: GoalMode::Full,
        turn_limit: 5,
        operation_grant: OperationClass::BASELINE.to_vec(),
    }
}

#[tokio::test]
async fn two_adapters_adopting_one_lane_admit_one_goal() {
    let pool = owner_pool().await;
    legacy_lane(&pool, "nightly", GoalMode::Full, 5).await;
    let adopts = AdmitTerms {
        owner_hold: None,
        adopts_lane: Some("nightly".into()),
        legacy_goal_read: goal_read(&pool, "nightly").await,
    };
    // Both adoptions reach the store as first ones, as when Slack and the desktop both pass
    // adopt_work's lookup before either commits.
    let slack = adoption_request(&pool, "nightly", ChannelProvider::Slack).await;
    let desktop = AdmitRequest {
        source_adapter: ChannelProvider::Desktop,
        ..slack.clone()
    };
    let first = admit_request_with(&pool, &slack, &adopts).await.unwrap();
    assert!(!first.duplicate);
    let second = admit_request_with(&pool, &desktop, &adopts).await.unwrap();
    assert!(second.duplicate, "the second adapter replays the first adoption");
    assert_eq!(second.receipt, first.receipt);
    // A different body from a third adapter is a typed refusal, never a second goal.
    let edited = AdmitRequest {
        source_adapter: ChannelProvider::Phone,
        body: "Adopt legacy lane nightly: renamed".into(),
        ..slack.clone()
    };
    let error = admit_request_with(&pool, &edited, &adopts).await.unwrap_err();
    assert_eq!(error.code(), "REQUEST_CONFLICT", "{error}");
    for table in ["supervisor_goals", "supervisor_requests", "supervisor_dispatches"] {
        assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM {table}")).await, 1, "{table}");
    }
}

#[tokio::test]
async fn an_adoption_that_finds_the_legacy_goal_turned_off_is_refused_at_commit() {
    let pool = owner_pool().await;
    legacy_lane(&pool, "nightly", GoalMode::Full, 5).await;
    let request = adoption_request(&pool, "nightly", ChannelProvider::Slack).await;
    let adopts = AdmitTerms {
        owner_hold: None,
        adopts_lane: Some("nightly".into()),
        legacy_goal_read: goal_read(&pool, "nightly").await,
    };
    // The owner narrowed the legacy goal after adopt_work read it as full.
    set_mode(&pool, "nightly", GoalMode::Supervised).await.unwrap();
    let error = admit_request_with(&pool, &request, &adopts).await.unwrap_err();
    assert_eq!(error.code(), LEGACY_TERMS_CHANGED, "{error}");
    set_mode(&pool, "nightly", GoalMode::Off).await.unwrap();
    let error = admit_request_with(&pool, &request, &adopts).await.unwrap_err();
    assert_eq!(error.code(), LEGACY_TERMS_CHANGED, "{error}");
    for table in ["supervisor_goals", "supervisor_requests", "supervisor_dispatches"] {
        assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM {table}")).await, 0, "{table}");
    }
    // A supervised adoption of a supervised legacy goal is not narrower, so it commits.
    set_mode(&pool, "nightly", GoalMode::Supervised).await.unwrap();
    let held = AdmitRequest {
        mode: GoalMode::Supervised,
        ..request
    };
    let terms = AdmitTerms {
        owner_hold: Some(format!("{OWNER_APPROVAL_REQUIRED}: the owner decides first")),
        adopts_lane: Some("nightly".into()),
        legacy_goal_read: goal_read(&pool, "nightly").await,
    };
    assert!(!admit_request_with(&pool, &held, &terms).await.unwrap().duplicate);
}

/// Adopts the legacy lane from inside the legacy pane launch: the adoption commits after the
/// legacy drive passed its checks and created its worktree, and before it attaches the pane.
struct AdoptingLauncher {
    pool: SqlitePool,
    lane_key: &'static str,
    adopted: Mutex<Option<AdmittedWork>>,
}

#[async_trait]
impl TaskLauncher for AdoptingLauncher {
    async fn launch_pane(&self, _cwd: &str, _command: &[String]) -> crate::error::Result<String> {
        let outcome = adopt_work(&self.pool, &actor("m-race", "adopt it"), &adopt_of(self.lane_key))
            .await
            .unwrap();
        *self.adopted.lock().unwrap() = Some(admitted(outcome));
        Ok("pane-legacy".to_owned())
    }
}

#[tokio::test]
async fn an_adoption_during_a_legacy_launch_wins_and_the_legacy_drive_sends_nothing() {
    use crate::lanes::guard::{GuardThresholds, HostLoad};
    use crate::lanes::runner::LaneRunner;

    let pool = owner_pool().await;
    legacy_lane(&pool, "nightly", GoalMode::Full, 50).await;
    let launcher = Arc::new(AdoptingLauncher {
        pool: pool.clone(),
        lane_key: "nightly",
        adopted: Mutex::new(None),
    });
    let control = Arc::new(RecordingControl::default());
    let runner = LaneRunner {
        pool: pool.clone(),
        control: control.clone(),
        launcher: launcher.clone(),
        judge: crate::judge::Judge::fallback_yes(),
        load: HostLoad {
            load: 0.1,
            mem_free_pct: 50,
            disk_free_gb: 100,
            active_lanes: 0,
        },
        thresholds: GuardThresholds {
            load_alert: 10.0,
            mem_free_min_pct: 10,
            disk_free_min_gb: 1,
            lanes_max: 4,
        },
    };
    let refused = timeout(DEADLINE, runner.drive("nightly"))
        .await
        .expect("the legacy drive finishes")
        .expect_err("an adopted lane is not driven");
    assert!(refused.to_string().starts_with(SUPERVISOR_OWNED), "{refused}");
    assert!(
        refused.to_string().contains("pane-legacy"),
        "the unused pane is named so the owner can close it: {refused}"
    );
    assert!(control.ops().is_empty(), "no brief was sent to the legacy pane");
    let (pane, status): (Option<String>, String) =
        sqlx::query_as("SELECT omo_thread_id, status FROM lanes WHERE key = 'nightly'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pane, None, "the legacy pane is not attached");
    assert_eq!(status, "queued", "the legacy lane did not start");

    // The adopting goal is the one worker.
    let adopted = launcher.adopted.lock().unwrap().take().expect("the adoption committed");
    let worker = Arc::new(RecordingControl::default());
    let dispatcher = dispatcher(&pool, worker.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert_eq!(dispatcher.drain().await.len(), 1);
    assert_eq!(
        sends(&worker),
        vec![(None, PANE.to_owned(), adopted.receipt.dispatch_request_id)]
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
}

/// The legacy paths that reopen an ended lane and send to its pane leave an adopted lane to
/// its supervisor goal: an owner reply in its Slack thread falls through to normal routing, a
/// tracker move is refused, the reopen statement itself refuses it, and the legacy goal loop
/// never sends to its pane. The adopting goal is the one worker.
#[tokio::test]
async fn an_adopted_lane_is_not_reopened_or_sent_to_by_the_legacy_paths() {
    use crate::lanes::owner_replies::{intercept, OwnerReply};
    use crate::lanes::state::reopen_unless_supervised;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let pool = owner_pool().await;
    legacy_lane(&pool, "nightly", GoalMode::Full, 10).await;
    set_pane(&pool, "nightly", "pane-legacy").await.unwrap();
    transition(&pool, "nightly", LaneStatus::Working, Some("launched"))
        .await
        .unwrap();
    transition(&pool, "nightly", LaneStatus::Failed, Some("forced: test"))
        .await
        .unwrap();
    sqlx::query("UPDATE lanes SET thread_channel_id = 'slack:C1:1.1' WHERE key = 'nightly'")
        .execute(&pool)
        .await
        .unwrap();
    // Ended, so not live: the adoption commits with the legacy pane still attached.
    let adopted = admitted(
        adopt_work(&pool, &actor("m-nightly", "adopt nightly"), &adopt_of("nightly"))
            .await
            .unwrap(),
    );
    let lane_state = || async {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT status, omo_thread_id FROM lanes WHERE key = 'nightly'",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    let ended = ("failed".to_owned(), Some("pane-legacy".to_owned()));

    // The owner's reply in the lane's thread is not consumed by the legacy lane.
    let legacy = RecordingControl::default();
    let reply = OwnerReply {
        channel: "C1",
        thread_ts: "1.1",
        author: "UOWNER",
        owner: "UOWNER",
        message_id: "r1",
        text: "keep going",
    };
    let consumed = intercept(&pool, &legacy, &reply).await.unwrap();
    assert!(!consumed, "the reply is left to normal routing");
    assert!(legacy.ops().is_empty(), "nothing reached the legacy pane");
    assert_eq!(lane_state().await, ended);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM lane_owner_replies").await, 0);

    // The reopen statement refuses it by itself, as when the adoption commits after a lookup.
    let reopened = reopen_unless_supervised(&pool, "nightly", Some("note: reopened"))
        .await
        .unwrap();
    assert!(!reopened);
    assert_eq!(lane_state().await, ended);

    // Moving its tracker issue to started is refused, not a reopen.
    for sql in [
        "INSERT INTO tracker_users(id, name, display_name, avatar_color, kind, is_me, created_at)
         VALUES('u1', 'ada', 'Ada', '#000', 'human', 1, '2020-01-01T00:00:00Z')",
        "INSERT INTO tracker_teams(id, key, name, icon, color, created_at)
         VALUES('t1', 'AGT', 'Agents', 'bot', '#fff', '2020-01-01T00:00:00Z')",
        "INSERT INTO tracker_states(id, team_id, name, type, color)
         VALUES('s-todo', 't1', 'Todo', 'unstarted', '#fff')",
        "INSERT INTO tracker_states(id, team_id, name, type, color)
         VALUES('s-started', 't1', 'In Progress', 'started', '#fff')",
        "INSERT INTO tracker_issues(id, team_id, number, identifier, title, state_id, priority,
                sort_order, lane_key, creator_id, created_at, updated_at)
         VALUES('i1', 't1', 1, 'AGT-1', 'Nightly', 's-todo', 0, 0, 'nightly', 'u1', '2020', '2020')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    let moved = crate::tracker_linear::router()
        .with_state(pool.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/issues/i1")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"state_id":"s-started"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(moved.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(moved.into_body(), 64 * 1024).await.unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains(SUPERVISOR_OWNED),
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(lane_state().await, ended);

    // Even with the lane forced back to work, the legacy goal loop sends nothing to its pane.
    sqlx::query("UPDATE lanes SET status = 'working' WHERE key = 'nightly'")
        .execute(&pool)
        .await
        .unwrap();
    let before = lane_goal_snapshot(&pool, "nightly").await;
    let settled = timeout(
        DEADLINE,
        crate::goals::loop_driver::on_turn_settled(
            &pool,
            &crate::judge::Judge::fallback_yes(),
            &legacy,
            "nightly",
            true,
        ),
    )
    .await
    .expect("the loop pass finishes")
    .unwrap();
    assert_eq!(settled, None);
    assert!(legacy.ops().is_empty(), "the legacy loop did not touch the legacy pane");
    assert_eq!(lane_goal_snapshot(&pool, "nightly").await, before);

    // The adopting goal is the one worker.
    let worker = Arc::new(RecordingControl::default());
    let dispatcher = dispatcher(&pool, worker.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert_eq!(dispatcher.drain().await.len(), 1);
    assert_eq!(
        sends(&worker),
        vec![(None, PANE.to_owned(), adopted.receipt.dispatch_request_id)]
    );
    assert!(legacy.ops().is_empty());
}

#[tokio::test]
async fn an_adoption_that_finds_the_legacy_terms_changed_is_refused_at_commit() {
    let pool = owner_pool().await;
    legacy_lane(&pool, "nightly", GoalMode::Full, 5).await;
    let request = adoption_request(&pool, "nightly", ChannelProvider::Slack).await;
    let read_now = || async {
        AdmitTerms {
            owner_hold: None,
            adopts_lane: Some("nightly".into()),
            legacy_goal_read: goal_read(&pool, "nightly").await,
        }
    };

    // The owner lowered the turn limit after adopt_work read the legacy goal.
    let read = read_now().await;
    let requirement = RequirementSpec {
        id: "r-nightly".into(),
        text: "reconcile runs".into(),
        verifier: "cargo test reconcile".into(),
    };
    set_contract(&pool, "nightly", &[requirement], GoalMode::Full, 2)
        .await
        .unwrap();
    let error = admit_request_with(&pool, &request, &read).await.unwrap_err();
    assert_eq!(error.code(), LEGACY_TERMS_CHANGED, "{error}");
    // A legacy turn counted since the read is a change too.
    let read = read_now().await;
    sqlx::query("UPDATE lane_goal SET turns_used = turns_used + 1 WHERE lane_key = 'nightly'")
        .execute(&pool)
        .await
        .unwrap();
    let error = admit_request_with(&pool, &request, &read).await.unwrap_err();
    assert_eq!(error.code(), LEGACY_TERMS_CHANGED, "{error}");
    // So is a legacy goal the lane did not have when it was read.
    let without = AdmitTerms {
        legacy_goal_read: Some(None),
        ..read
    };
    let error = admit_request_with(&pool, &request, &without).await.unwrap_err();
    assert_eq!(error.code(), LEGACY_TERMS_CHANGED, "{error}");
    for table in ["supervisor_goals", "supervisor_requests", "supervisor_dispatches"] {
        assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM {table}")).await, 0, "{table}");
    }

    // Read again, the adoption takes the current terms and commits.
    let current = read_now().await;
    assert!(!admit_request_with(&pool, &request, &current).await.unwrap().duplicate);
    assert!(supervisor_owns_lane(&pool, "nightly").await.unwrap());
}

/// A legacy lane `key` its legacy goal loop drives: a `mode` goal with ten turns, the legacy
/// pane attached, working.
async fn driven_legacy_lane(pool: &SqlitePool, key: &str, mode: GoalMode) {
    legacy_lane(pool, key, mode, 10).await;
    set_pane(pool, key, "pane-legacy").await.unwrap();
    transition(pool, key, LaneStatus::Working, Some("launched"))
        .await
        .unwrap();
}

/// A valid `continue` assessment of a [`legacy_lane`] goal.
fn legacy_continue(lane_key: &str) -> String {
    json!({
        "gaps": [{
            "id": "g1",
            "kind": "work",
            "requirement_ids": [format!("r-{lane_key}")],
            "summary": "tests not run"
        }],
        "decision": {
            "decision": "continue",
            "instruction": "run the tests",
            "gap_ids": ["g1"],
            "expected_change": "tests pass",
            "verification": "sig:0"
        }
    })
    .to_string()
}

/// The legacy goal loop's judge model. With `before_adopting`, its first answer comes only
/// after that statement ran on the legacy lane and the owner adopted the lane: the adoption
/// commits after the loop pass checked ownership and before it proposes and sends.
struct LegacyJudge {
    pool: SqlitePool,
    lane_key: &'static str,
    before_adopting: Option<&'static str>,
    adopted: Mutex<bool>,
}

#[async_trait]
impl ModelTurn for LegacyJudge {
    async fn complete(&self, _prompt: &str) -> crate::error::Result<String> {
        let adopt = self
            .before_adopting
            .filter(|_| !*self.adopted.lock().unwrap());
        if let Some(sql) = adopt {
            sqlx::query(sql)
                .bind(self.lane_key)
                .execute(&self.pool)
                .await?;
            adopt_work(
                &self.pool,
                &actor("m-judge", "adopt it"),
                &adopt_of(self.lane_key),
            )
            .await
            .unwrap();
            *self.adopted.lock().unwrap() = true;
        }
        Ok(legacy_continue(self.lane_key))
    }
}

fn legacy_judge(
    pool: &SqlitePool,
    lane_key: &'static str,
    before_adopting: Option<&'static str>,
) -> crate::judge::Judge {
    crate::judge::Judge::bare()
        .with_pool(pool.clone())
        .with_fallback(Arc::new(LegacyJudge {
            pool: pool.clone(),
            lane_key,
            before_adopting,
            adopted: Mutex::new(false),
        }))
}

/// A legacy goal pass checks ownership, then waits on its judge. Meanwhile the lane is ended
/// (a tracker cancel, a forced close) or loses its pane, and the owner adopts it. The pass
/// then proposes, and its send re-checks ownership after the proposal is committed `pending`:
/// nothing reaches the legacy pane, and the adopting goals are the only workers.
#[tokio::test]
async fn a_legacy_goal_pass_never_sends_to_a_lane_adopted_during_its_judge_call() {
    let pool = owner_pool().await;
    let legacy = RecordingControl::default();
    for (lane_key, before_adopting) in [
        (
            "nightly",
            "UPDATE lanes SET status = 'failed' WHERE key = ?",
        ),
        // Not ended, only detached: the ownership re-check alone refuses the send.
        (
            "detached",
            "UPDATE lanes SET omo_thread_id = NULL WHERE key = ?",
        ),
    ] {
        driven_legacy_lane(&pool, lane_key, GoalMode::Full).await;
        let judge = legacy_judge(&pool, lane_key, Some(before_adopting));
        let settled = timeout(
            DEADLINE,
            on_turn_settled(&pool, &judge, &legacy, lane_key, true),
        )
        .await
        .expect("the loop pass finishes")
        .unwrap();
        assert_eq!(
            settled,
            Some(LoopOutcome {
                lane_key: lane_key.to_owned(),
                stop_reason: None,
                awaiting_approval: false,
                sent: false,
            }),
            "{lane_key}"
        );
        assert!(supervisor_owns_lane(&pool, lane_key).await.unwrap());
        assert_eq!(
            lane_goal_snapshot(&pool, lane_key).await.3.as_deref(),
            Some(ProposalState::Superseded.as_str()),
            "{lane_key}: the proposal is superseded, never sent"
        );
    }
    assert!(sends(&legacy).is_empty(), "nothing reached a legacy pane");

    let worker = Arc::new(RecordingControl::default());
    let dispatcher = dispatcher(&pool, worker.clone(), Arc::new(FixedLauncher::new(PANE)));
    assert_eq!(dispatcher.drain().await.len(), 2);
    let panes: Vec<String> = sends(&worker)
        .into_iter()
        .map(|(_, pane, _)| pane)
        .collect();
    assert_eq!(panes, [PANE, PANE]);
    assert!(sends(&legacy).is_empty());
}

/// An approval whose caller checked ownership before the adoption committed: the claim
/// commits the proposal `pending`, and the send's re-check sends nothing.
#[tokio::test]
async fn an_approval_racing_an_adoption_sends_nothing_to_the_legacy_pane() {
    let pool = owner_pool().await;
    driven_legacy_lane(&pool, "ledger", GoalMode::Supervised).await;
    sqlx::query(
        "UPDATE lane_goal SET proposal_state = 'awaiting_approval', proposal_text = 'Run the verifier.',
                proposal_gap_ids = '[]', proposal_at = ? WHERE lane_key = 'ledger'",
    )
    .bind(crate::storage::now_rfc3339())
    .execute(&pool)
    .await
    .unwrap();
    // A proposal awaiting approval is not in flight, so once the lane ends the adoption commits.
    sqlx::query("UPDATE lanes SET status = 'failed' WHERE key = 'ledger'")
        .execute(&pool)
        .await
        .unwrap();
    adopt_work(
        &pool,
        &actor("m-ledger", "adopt ledger"),
        &adopt_of("ledger"),
    )
    .await
    .unwrap();
    assert!(supervisor_owns_lane(&pool, "ledger").await.unwrap());

    let legacy = RecordingControl::default();
    let outcome = timeout(DEADLINE, approve_proposal(&pool, &legacy, "ledger"))
        .await
        .expect("the approval finishes")
        .unwrap();
    assert!(!outcome.sent);
    assert!(legacy.ops().is_empty(), "nothing reached the legacy pane");
    assert_eq!(
        lane_goal_snapshot(&pool, "ledger").await.3.as_deref(),
        Some(ProposalState::Superseded.as_str())
    );
}

/// Tries to adopt the legacy lane from inside the legacy goal loop's send, after the pass
/// committed its proposal `pending` and re-checked ownership. The lane is ended first, so only
/// the pending turn stands between the adoption and a second worker. Records the refusal's
/// code, then sends.
struct AdoptOnSend {
    pool: SqlitePool,
    inner: RecordingControl,
    refusal: Mutex<Option<&'static str>>,
}

#[async_trait]
impl AgentControl for AdoptOnSend {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> crate::error::Result<Value> {
        self.inner.execute(host_id, op).await
    }

    async fn send_confirmed(
        &self,
        host_id: Option<&str>,
        session_id: &str,
        text: &str,
        request_id: &str,
    ) -> SendOutcome {
        sqlx::query("UPDATE lanes SET status = 'failed' WHERE key = 'nightly'")
            .execute(&self.pool)
            .await
            .unwrap();
        let refused = adopt_work(
            &self.pool,
            &actor("m-send", "adopt it"),
            &adopt_of("nightly"),
        )
        .await
        .expect_err("a pending legacy turn refuses the adoption");
        *self.refusal.lock().unwrap() = Some(refused.code());
        self.inner
            .send_confirmed(host_id, session_id, text, request_id)
            .await
    }
}

/// The other order of the goal-loop race: the legacy pass commits its proposal `pending`
/// first, so the adoption is refused with `LEGACY_LANE_LIVE` and the pass sends its one turn.
#[tokio::test]
async fn a_pending_legacy_turn_refuses_an_adoption_that_comes_after_it() {
    let pool = owner_pool().await;
    driven_legacy_lane(&pool, "nightly", GoalMode::Full).await;
    let control = AdoptOnSend {
        pool: pool.clone(),
        inner: RecordingControl::default(),
        refusal: Mutex::new(None),
    };
    let judge = legacy_judge(&pool, "nightly", None);
    let settled = timeout(
        DEADLINE,
        on_turn_settled(&pool, &judge, &control, "nightly", true),
    )
    .await
    .expect("the loop pass finishes")
    .unwrap()
    .expect("the pass changed the lane");
    assert!(
        settled.sent,
        "the legacy pass won the race and sent its one turn"
    );
    let refusal = *control.refusal.lock().unwrap();
    assert_eq!(refusal, Some(LEGACY_LANE_LIVE));
    let sent = sends(&control.inner);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "pane-legacy");
    // The sent turn refuses it too, and nothing was admitted.
    let error = adopt_work(&pool, &actor("m-after", "adopt it"), &adopt_of("nightly"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), LEGACY_LANE_LIVE, "{error}");
    assert!(!supervisor_owns_lane(&pool, "nightly").await.unwrap());
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await,
        0
    );
}

/// An owner reply whose lane is adopted between intercept's lookup and its apply. Simulated
/// by hiding a committed adoption's `adopt:` row from the lookup and restoring it when the
/// reply is claimed, which is what the apply then sees. Nothing is reopened, closed or sent,
/// the claim is forgotten, and the reply (and a redelivery) takes normal routing.
#[tokio::test]
async fn an_owner_reply_racing_an_adoption_is_left_to_normal_routing() {
    use crate::lanes::owner_replies::{intercept, OwnerReply};

    let pool = owner_pool().await;
    let lanes = [
        ("nightly", "1.1", "failed"),
        ("shipped", "2.2", "verified_done"),
    ];
    for (lane_key, thread_ts, status) in lanes {
        driven_legacy_lane(&pool, lane_key, GoalMode::Full).await;
        sqlx::query("UPDATE lanes SET status = ?, thread_channel_id = ? WHERE key = ?")
            .bind(status)
            .bind(format!("slack:C1:{thread_ts}"))
            .bind(lane_key)
            .execute(&pool)
            .await
            .unwrap();
        adopt_work(&pool, &actor(lane_key, "adopt it"), &adopt_of(lane_key))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE supervisor_requests SET source_message_id = 'hidden:' || ?
             WHERE source_message_id = 'adopt:' || ?",
        )
        .bind(lane_key)
        .bind(lane_key)
        .execute(&pool)
        .await
        .unwrap();
        assert!(!supervisor_owns_lane(&pool, lane_key).await.unwrap());
    }
    sqlx::query(
        "CREATE TRIGGER adoption_after_lookup AFTER INSERT ON lane_owner_replies BEGIN
           UPDATE supervisor_requests SET source_message_id = 'adopt:' || NEW.lane_key
            WHERE source_message_id = 'hidden:' || NEW.lane_key;
         END",
    )
    .execute(&pool)
    .await
    .unwrap();

    let legacy = RecordingControl::default();
    for (lane_key, thread_ts, status) in lanes {
        let reply = OwnerReply {
            channel: "C1",
            thread_ts,
            author: "UOWNER",
            owner: "UOWNER",
            message_id: lane_key,
            text: "keep going",
        };
        for attempt in ["first", "redelivered"] {
            let consumed = intercept(&pool, &legacy, &reply).await.unwrap();
            assert!(
                !consumed,
                "{lane_key} {attempt}: the reply is left to normal routing"
            );
        }
        assert!(supervisor_owns_lane(&pool, lane_key).await.unwrap());
        let lane: (String, Option<String>) =
            sqlx::query_as("SELECT status, omo_thread_id FROM lanes WHERE key = ?")
                .bind(lane_key)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            lane,
            (status.to_owned(), Some("pane-legacy".to_owned())),
            "{lane_key}: neither reopened nor closed"
        );
    }
    assert!(legacy.ops().is_empty(), "nothing reached a legacy pane");
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM lane_owner_replies").await,
        0,
        "the claims are forgotten"
    );
}

/// A lane close never removes the worktree an adopted lane hands its supervisor goal: a close
/// in progress refuses the adoption, a close whose caller checked ownership before the
/// adoption committed is refused by its own claim, and the tracker's close and cancel moves
/// and lane actions are refused with `SUPERVISOR_OWNED`. The tree stays registered on disk.
#[tokio::test]
async fn an_adopted_lanes_worktree_is_never_removed_by_a_legacy_close() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    crate::lanes::worktree::isolate_test_env();
    let (base, root) = git_repo("adopted-close");
    let pool = owner_pool().await;
    legacy_lane(&pool, "nightly", GoalMode::Full, 10).await;
    let tree = crate::lanes::worktree::create(&pool, "nightly", &root.to_string_lossy())
        .await
        .unwrap()
        .expect("a lane at a repository root gets its own worktree");
    sqlx::query("UPDATE lanes SET status = 'verified_done' WHERE key = 'nightly'")
        .execute(&pool)
        .await
        .unwrap();
    let tree_state = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM lane_owned_worktrees WHERE lane_key = 'nightly'",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };

    // A close that claimed the tree first refuses the adoption until it finishes or fails.
    sqlx::query("UPDATE lane_owned_worktrees SET state = 'removing' WHERE lane_key = 'nightly'")
        .execute(&pool)
        .await
        .unwrap();
    let error = adopt_work(&pool, &actor("m-1", "adopt it"), &adopt_of("nightly"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), LEGACY_LANE_LIVE, "{error}");
    assert!(!supervisor_owns_lane(&pool, "nightly").await.unwrap());
    sqlx::query("UPDATE lane_owned_worktrees SET state = 'registered' WHERE lane_key = 'nightly'")
        .execute(&pool)
        .await
        .unwrap();
    adopt_work(&pool, &actor("m-2", "adopt it"), &adopt_of("nightly"))
        .await
        .unwrap();
    assert!(supervisor_owns_lane(&pool, "nightly").await.unwrap());

    // A close whose caller checked ownership before the adoption committed: its claim refuses.
    let refused = crate::lanes::worktree::close(&pool, "nightly", None)
        .await
        .unwrap_err();
    assert!(refused.to_string().contains(SUPERVISOR_OWNED), "{refused}");
    assert!(Path::new(&tree).is_dir());
    assert_eq!(tree_state().await, "registered");

    for sql in [
        "INSERT INTO tracker_users(id, name, display_name, avatar_color, kind, is_me, created_at)
         VALUES('u1', 'ada', 'Ada', '#000', 'human', 1, '2020-01-01T00:00:00Z')",
        "INSERT INTO tracker_teams(id, key, name, icon, color, created_at)
         VALUES('t1', 'AGT', 'Agents', 'bot', '#fff', '2020-01-01T00:00:00Z')",
        "INSERT INTO tracker_states(id, team_id, name, type, color)
         VALUES('s-todo', 't1', 'Todo', 'unstarted', '#fff')",
        "INSERT INTO tracker_states(id, team_id, name, type, color)
         VALUES('s-done', 't1', 'Done', 'completed', '#fff')",
        "INSERT INTO tracker_states(id, team_id, name, type, color)
         VALUES('s-canceled', 't1', 'Canceled', 'canceled', '#fff')",
        "INSERT INTO tracker_issues(id, team_id, number, identifier, title, state_id, priority,
                sort_order, lane_key, creator_id, created_at, updated_at)
         VALUES('i1', 't1', 1, 'AGT-1', 'Nightly', 's-todo', 0, 0, 'nightly', 'u1', '2020', '2020')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    let router = crate::tracker_linear::router().with_state(pool.clone());
    let calls = [
        (
            "PATCH",
            "/issues/i1",
            json!({"state_id": "s-done", "force_note": "done"}),
        ),
        ("PATCH", "/issues/i1", json!({"state_id": "s-canceled"})),
        (
            "POST",
            "/issues/i1/lane-action",
            json!({"action": "close", "force": true}),
        ),
        (
            "POST",
            "/issues/i1/lane-action",
            json!({"action": "reopen"}),
        ),
        (
            "POST",
            "/issues/i1/lane-action",
            json!({"action": "send", "note": "go on"}),
        ),
        (
            "POST",
            "/issues/i1/lane-action",
            json!({"action": "object", "note": "wait"}),
        ),
    ];
    for (method, uri, body) in calls {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let text = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&text);
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{method} {uri} {body}: {text}"
        );
        assert!(
            text.contains(SUPERVISOR_OWNED),
            "{method} {uri} {body}: {text}"
        );
    }
    let (status, state_id): (String, String) = sqlx::query_as(
        "SELECT l.status, i.state_id FROM lanes l JOIN tracker_issues i ON i.lane_key = l.key
         WHERE l.key = 'nightly'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (status.as_str(), state_id.as_str()),
        ("verified_done", "s-todo")
    );
    assert!(
        Path::new(&tree).is_dir(),
        "the adopted lane's worktree is intact"
    );
    assert_eq!(tree_state().await, "registered");
    let _ = std::fs::remove_dir_all(&base);
}
