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

use crate::agent_control::{AgentControl, AgentOp, FixedLauncher, RecordingControl, TaskLauncher};
use crate::goals::contract::{contract_of, set_contract, RequirementSpec};
use crate::goals::types::{GoalMode, SendOutcome, TurnOrigin};
use crate::lanes::state::{set_pane, transition, LaneStatus};
use crate::supervisor::admission::{
    adopt_work, handle_message, is_ordinary_question, submit_work, ActorContext, AdmittedWork,
    AdoptWork, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork, LEGACY_GOAL_OFF,
    LEGACY_LANE_LIVE,
};
use crate::supervisor::dispatcher::{
    DispatchResult, Dispatcher, DISPATCH_FAILED, OWNER_APPROVAL_REQUIRED,
};
use crate::supervisor::{
    bind_project_host, claim_dispatch, create_project, ensure_identity, execution_binding_of,
    goal_of, identity_of, transition_goal, ChannelProvider, RunState,
    CLAIM_UNCONFIRMED_AT_RESTART, DEFAULT_TURN_LIMIT,
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
/// scratch directory and the repository root, both canonical.
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
    git(&root, &["commit", "-qm", "a"]);
    (
        std::fs::canonicalize(&base).unwrap(),
        std::fs::canonicalize(&root).unwrap(),
    )
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

/// Reports every transmitted request id on a channel the test awaits.
struct SignalControl {
    sent: mpsc::UnboundedSender<String>,
}

/// A transport that refuses every send: a person is typing in the pane.
struct RefusingControl;

#[async_trait]
impl AgentControl for RefusingControl {
    async fn execute(&self, _host_id: Option<&str>, _op: AgentOp) -> crate::error::Result<Value> {
        Err(crate::error::HubError::message(
            "CONTROL_CONFLICT a person typed in this pane",
        ))
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

    let control = Arc::new(RecordingControl::default());
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
    assert_eq!(binding.run_id.as_deref(), Some(request_id.as_str()));
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
    let control = Arc::new(RecordingControl::default());
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
    let control = Arc::new(RecordingControl::default());
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

    let control = Arc::new(RecordingControl::default());
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
    let control = Arc::new(RecordingControl::default());
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

    let control = Arc::new(RecordingControl::default());
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
    assert!(held.state_detail.as_deref().unwrap_or("").contains("publish"));
    assert!(held.acknowledgment.contains("Nothing has been sent"));

    let mut outside = work();
    outside.cwd = Some("/etc".into());
    let outcome = submit_work(&pool, &actor("m-etc", "Tidy up the config."), &outside)
        .await
        .unwrap();
    assert!(matches!(outcome, SubmitOutcome::AwaitingOwner(_)), "{outcome:?}");

    let control = Arc::new(RecordingControl::default());
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
    let control = Arc::new(RecordingControl::default());
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
    let control = Arc::new(RecordingControl::default());
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
    assert_eq!(
        refusing.drain().await,
        vec![DispatchResult::Transmitted {
            request_id: work.receipt.dispatch_request_id.clone(),
            outcome: SendOutcome::Refused {
                code: "CONTROL_CONFLICT".into()
            },
        }]
    );
    let (state, code): (String, Option<String>) = sqlx::query_as(
        "SELECT state, uncertain_code FROM supervisor_dispatches WHERE request_id = ?",
    )
    .bind(&work.receipt.dispatch_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (state.as_str(), code.as_deref()),
        ("uncertain", Some("SEND_REFUSED:CONTROL_CONFLICT"))
    );
    assert_eq!(run_state(&pool, &work.receipt.goal_id).await, RunState::AwaitingOwner);
    let control = Arc::new(RecordingControl::default());
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
    let control = Arc::new(RecordingControl::default());
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

    let control = Arc::new(RecordingControl::default());
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
