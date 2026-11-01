//! Verified completion and owner precedence (todo 10): a settled run drives the existing
//! assessment and blind convergence check; a fabricated done never completes a goal; a decision
//! made against an older revision never overwrites an owner correction; a verifier failure
//! becomes a corrective run within the turn budget without owner input; an owner pause that
//! lands while a claimed send waits for its pane stops the bytes; cancel is not pause; routine
//! prompts are answered only from a current decision naming the exact prompt and label.
//! No test sleeps: virtual time goes to `tick`, and gated fakes are awaited on exact signals
//! under a bounded deadline.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;

use crate::agent_control::{
    AgentControl, AgentOp, Delivery, FixedLauncher, RecordingControl, StartReceipt, WorkerSend,
};
use crate::error::{HubError, Result};
use crate::goals::contract::{contract_of, RequirementSpec};
use crate::goals::evidence::VERIFIER_UNPINNED;
use crate::goals::types::RequirementStatus;
use crate::judge::Judge;
use crate::lanes::kibitzer::ModelTurn;
use crate::supervisor::admission::{
    submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
};
use crate::supervisor::answers::{
    routine_answer, AnswerDecision, DecisionSource, OwnerReason, PromptDecision, PromptKind,
    WorkerPrompt,
};
use crate::supervisor::completion::{
    CompletionDriver, CompletionHandler, Verification, GRANT_EXCEEDED, JUDGE_UNAVAILABLE,
    PANE_NOT_IDLE, REMOTE_CHECK_UNAVAILABLE, RETRY_NOT_DUE, REVISION_CONFLICT, VERIFIED,
    VERIFIER_FAILED, VERIFIER_NOT_RUNNABLE,
};
use crate::supervisor::delivery::check_before_typing;
use crate::supervisor::dispatcher::{DispatchResult, Dispatcher, LANE_PAUSED, TURN_LIMIT};
use crate::supervisor::receipts::{record_pty_settle, PtySettle};
use crate::supervisor::wakes::WakeScheduler;
use crate::supervisor::{
    bind_project_host, create_project, enqueue_dispatch, ensure_identity, execution_binding_of,
    goal_of, ready_dispatches, revise_goal, transition_goal, ChannelProvider, RunState,
    SupervisorError,
};

const OWNER: &str = "slack:T1:U-owner";
const HOST: &str = "host-a";
const REPO: &str = "/home/owner/code/ferryx";
const BODY: &str = "Create result.txt containing READY. Run the verifier.";
const PANE: &str = "pane-sv-1";
const EPOCH: &str = RecordingControl::EPOCH;
/// Bound on every awaited signal. Reaching it fails the test; it never makes one pass.
const DEADLINE: Duration = Duration::from_secs(10);

const ASSESS_COMPLETE: &str = r#"{"gaps":[],"decision":{"decision":"complete"}}"#;
const ASSESS_CONTINUE: &str = r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r1"],"summary":"verify.sh failed"}],"decision":{"decision":"continue","instruction":"fix result.txt and rerun verify.sh","gap_ids":["g1"],"expected_change":"verify.sh exits 0","verification":"git:clean"}}"#;
/// A continue whose instruction asks for an operation the baseline grant withholds.
const ASSESS_PUSH: &str = r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r1"],"summary":"not on origin"}],"decision":{"decision":"continue","instruction":"push the fix to origin main","gap_ids":["g1"],"expected_change":"pushed","verification":"git:clean"}}"#;
/// Cites the check Ferryx ran from r1's verifier.
const CONVERGED: &str =
    r#"{"requirements":[{"req_id":"r1","met":true,"evidence_refs":["git:clean","sig:req:r1"]}]}"#;
/// The worker's own claim and a check Ferryx never ran.
const FABRICATED: &str =
    r#"{"requirements":[{"req_id":"r1","met":true,"evidence_refs":["claim:latest","sig:7"]}]}"#;
/// Only worker-controlled screen text and the observer state: attested prefixes, no check.
const SCREEN_ONLY: &str = r#"{"requirements":[{"req_id":"r1","met":true,"evidence_refs":["observer:state","screen:r1"]}]}"#;

/// Answers judge turns from a script, recording every prompt. When `gate` is set, the first
/// turn reports itself on `entered` and waits for the gate before answering.
struct ScriptedModel {
    answers: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<String>>,
    gate: Option<(mpsc::UnboundedSender<()>, Arc<Notify>)>,
}

impl ScriptedModel {
    fn new(answers: &[&str]) -> Arc<Self> {
        Arc::new(Self::with(answers, None))
    }

    fn with(answers: &[&str], gate: Option<(mpsc::UnboundedSender<()>, Arc<Notify>)>) -> Self {
        Self {
            answers: Mutex::new(answers.iter().map(|text| (*text).to_owned()).collect()),
            prompts: Mutex::new(Vec::new()),
            gate,
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

#[async_trait]
impl ModelTurn for ScriptedModel {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let first = {
            let mut prompts = self.prompts.lock().unwrap();
            prompts.push(prompt.to_owned());
            prompts.len() == 1
        };
        if let (true, Some((entered, gate))) = (first, &self.gate) {
            let _ = entered.send(());
            gate.notified().await;
        }
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| HubError::message("no scripted answer"))
    }
}

fn judge(pool: &SqlitePool, model: Arc<ScriptedModel>) -> Arc<Judge> {
    Arc::new(Judge::bare().with_pool(pool.clone()).with_fallback(model))
}

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

fn actor(message_id: &str) -> ActorContext {
    ActorContext {
        principal: OWNER.into(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: message_id.into(),
        source_body: BODY.into(),
        channel_binding_id: None,
        choices: OwnerChoices::default(),
    }
}

/// Admits one full-mode goal with one requirement; returns (goal id, lane key).
async fn admit(pool: &SqlitePool, message_id: &str, turn_limit: Option<u32>) -> (String, String) {
    let work = SubmitWork {
        project_id: None,
        host_id: None,
        kind: None,
        title: "Write result.txt".into(),
        instruction: "Create result.txt containing READY.".into(),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        cwd: None,
        operations: OperationClass::BASELINE.to_vec(),
        turn_limit,
    };
    match submit_work(pool, &actor(message_id), &work).await.unwrap() {
        SubmitOutcome::Admitted(admitted) => {
            let goal_id = admitted.receipt.goal_id;
            let lane = goal_of(pool, &goal_id).await.unwrap().unwrap().lane_key;
            (goal_id, lane)
        }
        other => panic!("expected an admitted goal, got {other:?}"),
    }
}

/// A temporary git repository, removed when dropped.
struct TempRepo(std::path::PathBuf);

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = crate::child_env::std_command("git")
        .args([
            "-c",
            "user.name=qa",
            "-c",
            "user.email=qa@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A clean git repository (`git:clean`: nothing uncommitted, its one commit on a remote
/// branch) whose admitted commit holds the requirement's verifier `verify.sh`, which exits 0
/// only for READY, and an untracked `result.txt` with `result`. Returns the admitted commit.
fn clean_repo(result: &str) -> (TempRepo, String) {
    let root = std::env::temp_dir().join(format!("ferryx-hub-completion-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let repo = TempRepo(root.clone());
    git(&root, &["init", "-q"]);
    std::fs::write(
        root.join("verify.sh"),
        "test \"$(cat result.txt)\" = READY\n",
    )
    .unwrap();
    git(&root, &["add", "verify.sh"]);
    git(&root, &["commit", "-q", "-m", "verifier"]);
    git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    std::fs::write(root.join("result.txt"), result).unwrap();
    let head = git(&root, &["rev-parse", "HEAD"]);
    (repo, head)
}

/// Points the lane at a clean repository whose result.txt holds `result`, after its pane was
/// launched, recorded as the lane's task-owned worktree started at the admitted commit (as
/// the dispatcher's auto worktree records it). Keep the returned guard alive for the test.
async fn use_clean_repo(pool: &SqlitePool, lane: &str, result: &str) -> TempRepo {
    let (repo, head) = clean_repo(result);
    let path = repo.0.to_string_lossy().into_owned();
    sqlx::query("UPDATE lanes SET cwd = ? WHERE key = ?")
        .bind(&path)
        .bind(lane)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO lane_owned_worktrees(lane_key, path, state, repo, branch, start_commit, origin_cwd)
         VALUES (?, ?, 'registered', ?, ?, ?, ?)",
    )
    .bind(lane)
    .bind(&path)
    .bind(&path)
    .bind(format!("lane/{lane}"))
    .bind(&head)
    .bind(&path)
    .execute(pool)
    .await
    .unwrap();
    repo
}

/// A target that reports a causal start for every delivery and reads its panes idle, with no
/// prompt and a screen whose (worker-controlled) text claims success.
fn idle_control() -> Arc<RecordingControl> {
    let control = RecordingControl::observing();
    *control.result.lock().unwrap() = json!({
        "session": {
            "state": "idle",
            "stateSource": "provider",
            "tier": "provider",
            "pending": null,
            "screen": {"revision": "r1", "text": "$ sh verify.sh\nall checks passed, READY"}
        }
    });
    Arc::new(control)
}

fn dispatcher(pool: &SqlitePool, control: Arc<dyn AgentControl>, pane: &str) -> Dispatcher {
    Dispatcher::new(
        pool.clone(),
        control,
        Arc::new(FixedLauncher::new(pane)),
        HOST,
    )
}

fn sends(control: &RecordingControl) -> Vec<String> {
    control
        .ops()
        .into_iter()
        .filter_map(|(_, op)| match op {
            AgentOp::Send { request_id, .. } => Some(request_id),
            _ => None,
        })
        .collect()
}

fn sends_text(control: &RecordingControl) -> Vec<String> {
    control
        .ops()
        .into_iter()
        .filter_map(|(_, op)| match op {
            AgentOp::Send { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// The hub observer's idle reading of `pane` that ends its live run.
async fn settle(pool: &SqlitePool, pane: &str, sequence: u64) {
    let settled = record_pty_settle(pool, HOST, pane, EPOCH, sequence)
        .await
        .unwrap();
    assert!(matches!(settled, PtySettle::Settled(_)), "{settled:?}");
}

fn epoch() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-27T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn completion_scheduler(
    pool: &SqlitePool,
    model: Arc<ScriptedModel>,
    control: Arc<dyn AgentControl>,
) -> WakeScheduler {
    let driver = CompletionDriver::new(pool.clone(), judge(pool, model), control, HOST);
    WakeScheduler::new(
        pool.clone(),
        Arc::new(CompletionHandler::new(pool.clone(), driver)),
    )
}

async fn goal_state(pool: &SqlitePool, goal_id: &str) -> (RunState, String) {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    (goal.run_state, goal.state_detail.unwrap_or_default())
}

async fn requirement_status(pool: &SqlitePool, lane: &str) -> (RequirementStatus, Vec<String>) {
    let contract = contract_of(pool, lane).await.unwrap().unwrap();
    let requirement = &contract.requirements[0];
    (requirement.status, requirement.evidence.clone())
}

async fn turns_used(pool: &SqlitePool, lane: &str) -> u32 {
    contract_of(pool, lane).await.unwrap().unwrap().turns_used
}

#[tokio::test]
async fn a_fabricated_done_never_completes_and_becomes_a_corrective_run() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    crate::lanes::state::set_claim(&pool, &lane, "[CLAIM_DONE] all done, verify.sh passed")
        .await
        .unwrap();
    settle(&pool, PANE, 1_000).await;

    let model = ScriptedModel::new(&[ASSESS_COMPLETE, FABRICATED]);
    let scheduler = completion_scheduler(&pool, model.clone(), control.clone());
    let tick = scheduler.tick(epoch()).await.unwrap();
    assert_eq!(
        tick.delivered.len(),
        1,
        "the durable settle drives verification"
    );

    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::Ready, "{detail}");
    assert!(detail.starts_with(VERIFIER_FAILED), "{detail}");
    assert_eq!(
        requirement_status(&pool, &lane).await.0,
        RequirementStatus::Pending,
        "a claim and an unrun check prove nothing"
    );
    // The blind check ran in its own context: the contract and evidence, not the assessment.
    let prompts = model.prompts();
    assert_eq!(prompts.len(), 2);
    assert!(!prompts[1].contains("\"decision\""), "{}", prompts[1]);
    let ready = ready_dispatches(&pool, 10).await.unwrap();
    assert_eq!(ready.len(), 1, "one corrective run, without the owner");
    assert!(
        ready[0].instruction.contains("r1"),
        "{}",
        ready[0].instruction
    );
    assert_eq!(turns_used(&pool, &lane).await, 1);
}

/// Observer state and the screen are attested prefixes, but the screen is the worker's own
/// text: citing them without the check Ferryx ran never completes a goal, even when that
/// check would pass.
#[tokio::test]
async fn the_screen_and_observer_state_alone_never_prove_a_requirement() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;

    let model = ScriptedModel::new(&[ASSESS_COMPLETE, SCREEN_ONLY]);
    completion_scheduler(&pool, model, control)
        .tick(epoch())
        .await
        .unwrap();
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::Ready, "{detail}");
    assert!(detail.starts_with(VERIFIER_FAILED), "{detail}");
    assert_eq!(
        requirement_status(&pool, &lane).await.0,
        RequirementStatus::Pending
    );
}

/// The judge cites the right check, but the check Ferryx ran failed (wrong bytes): no
/// completion, a correction instead.
/// The worker rewrites the verifier to `exit 0` (and commits it) instead of fixing the work.
/// Ferryx runs verify.sh as it was at the admitted revision, so the check still fails on the
/// wrong bytes and the goal never completes, however the judge reads it.
#[tokio::test]
async fn a_worker_rewritten_verifier_never_completes_the_goal() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let repo = use_clean_repo(&pool, &lane, "NOT READY").await;
    std::fs::write(repo.0.join("verify.sh"), "exit 0\n").unwrap();
    git(&repo.0, &["commit", "-q", "-am", "make the check pass"]);
    git(&repo.0, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let rewritten = crate::child_env::std_command("sh")
        .arg("verify.sh")
        .current_dir(&repo.0)
        .status()
        .unwrap();
    assert!(
        rewritten.success(),
        "the worktree's own verify.sh now passes"
    );
    settle(&pool, PANE, 1_000).await;

    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    completion_scheduler(&pool, model, control)
        .tick(epoch())
        .await
        .unwrap();
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_ne!(state, RunState::Completed, "{detail}");
    assert_eq!(state, RunState::Ready, "{detail}");
    assert!(detail.starts_with(VERIFIER_FAILED), "{detail}");
    assert_eq!(
        requirement_status(&pool, &lane).await.0,
        RequirementStatus::Pending
    );
    let bundle = crate::goals::evidence::collect_for_run(
        &pool,
        &RecordingControl::default(),
        &lane,
        None,
        None,
        &contract_of(&pool, &lane)
            .await
            .unwrap()
            .unwrap()
            .requirements,
    )
    .await
    .unwrap();
    assert_eq!(
        bundle.get("sig:req:r1").unwrap().passed,
        Some(false),
        "{:?}",
        bundle.items
    );
}

/// A verifier the admitted revision does not have (the worker added it) is never run; a lane
/// with no admitted revision has nothing to pin it to. Both wait for the owner.
#[tokio::test]
async fn a_verifier_missing_from_the_admitted_revision_waits_for_the_owner() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let repo = use_clean_repo(&pool, &lane, "READY").await;
    sqlx::query(
        "UPDATE lane_goal_requirements SET verifier = 'check.sh exits 0' WHERE lane_key = ?",
    )
    .bind(&lane)
    .execute(&pool)
    .await
    .unwrap();
    std::fs::write(repo.0.join("check.sh"), "exit 0\n").unwrap();
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    let driver = CompletionDriver::new(pool.clone(), judge(&pool, model.clone()), control, HOST);
    let held = driver.verify(&goal).await.unwrap();
    assert!(
        matches!(&held, Verification::Held { state: RunState::AwaitingOwner, detail } if detail.starts_with(VERIFIER_UNPINNED)),
        "{held:?}"
    );
    assert!(model.prompts().is_empty(), "nothing graded");

    // No admitted revision at all: the original verify.sh is not run as the worker left it.
    let unpinned = crate::goals::evidence::unpinned_verifiers(
        &pool,
        &lane,
        &[crate::goals::contract::Requirement {
            verifier: "verify.sh exits 0".into(),
            ..contract_of(&pool, &lane)
                .await
                .unwrap()
                .unwrap()
                .requirements[0]
                .clone()
        }],
    )
    .await
    .unwrap();
    assert!(
        unpinned.is_empty(),
        "verify.sh is in the admitted revision: {unpinned:?}"
    );
    sqlx::query("DELETE FROM lane_owned_worktrees WHERE lane_key = ?")
        .bind(&lane)
        .execute(&pool)
        .await
        .unwrap();
    let unpinned = crate::goals::evidence::unpinned_verifiers(
        &pool,
        &lane,
        &contract_of(&pool, &lane)
            .await
            .unwrap()
            .unwrap()
            .requirements,
    )
    .await
    .unwrap();
    assert!(
        unpinned.len() == 1 && unpinned[0].starts_with(VERIFIER_UNPINNED),
        "{unpinned:?}"
    );
}

/// The judge cites the right check, but the check Ferryx ran failed (wrong bytes): no
/// completion, a correction instead.
#[tokio::test]
async fn a_failing_ferryx_check_never_completes_even_when_cited() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "NOT READY").await;
    settle(&pool, PANE, 1_000).await;

    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    completion_scheduler(&pool, model, control)
        .tick(epoch())
        .await
        .unwrap();
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::Ready, "{detail}");
    assert!(detail.starts_with(VERIFIER_FAILED), "{detail}");
}

#[tokio::test]
async fn a_verifier_failure_runs_a_correction_without_the_owner_and_stops_at_the_budget() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", Some(1)).await;
    let control = idle_control();
    let dispatch = dispatcher(&pool, control.clone(), PANE);
    dispatch.drain().await;
    let _repo = use_clean_repo(&pool, &lane, "NOT READY").await;
    let model = ScriptedModel::new(&[ASSESS_CONTINUE, ASSESS_CONTINUE]);
    let scheduler = completion_scheduler(&pool, model.clone(), control.clone());

    // First verifier failure: a corrective run goes out with no owner input.
    settle(&pool, PANE, 1_000).await;
    scheduler.tick(epoch()).await.unwrap();
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::Ready, "{detail}");
    assert!(detail.starts_with(VERIFIER_FAILED), "{detail}");
    let results = dispatch.drain().await;
    assert!(
        matches!(results.as_slice(), [DispatchResult::Transmitted { .. }]),
        "{results:?}"
    );
    assert_eq!(sends(&control).len(), 2, "the first run and one correction");
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Running);
    assert!(sends(&control)[1] != sends(&control)[0]);
    // The correction carries the persisted grant, not just the judge's words.
    let correction = sends_text(&control).pop().unwrap();
    assert!(
        correction.contains("Allowed operations: edit, test, commit"),
        "{correction}"
    );
    assert!(
        correction.contains("Ask the owner before any of: push"),
        "{correction}"
    );

    // Second failure with the budget spent: a visible hold, never success, nothing sent.
    settle(&pool, PANE, 2_000).await;
    scheduler
        .tick(epoch() + chrono::Duration::seconds(1))
        .await
        .unwrap();
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::AwaitingOwner, "{detail}");
    assert!(detail.starts_with(TURN_LIMIT), "{detail}");
    assert!(dispatch.drain().await.is_empty());
    assert_eq!(sends(&control).len(), 2);
    assert_eq!(turns_used(&pool, &lane).await, 1);
    // One assessment per settled run; the unconverged goal is then reflected on once (through
    // typesafe-judge or the model, whichever this machine has).
    let assessments = model
        .prompts()
        .iter()
        .filter(|prompt| !prompt.contains("lesson_reflect"))
        .count();
    assert_eq!(assessments, 2, "one assessment per settled run");
    let reflections: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM judge_calls WHERE purpose = 'lesson_reflect'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(reflections >= 1, "the unconverged goal is reflected on");
}

/// A corrective instruction that names a withheld operation is held for the owner.
#[tokio::test]
async fn a_correction_naming_a_withheld_operation_waits_for_the_owner() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    let dispatch = dispatcher(&pool, control.clone(), PANE);
    dispatch.drain().await;
    let _repo = use_clean_repo(&pool, &lane, "NOT READY").await;
    settle(&pool, PANE, 1_000).await;
    let model = ScriptedModel::new(&[ASSESS_PUSH]);
    completion_scheduler(&pool, model, control.clone())
        .tick(epoch())
        .await
        .unwrap();
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::AwaitingOwner, "{detail}");
    assert!(detail.starts_with(GRANT_EXCEEDED), "{detail}");
    assert!(dispatch.drain().await.is_empty());
    assert_eq!(sends(&control).len(), 1, "nothing beyond the first run");
    assert_eq!(turns_used(&pool, &lane).await, 0);
}

/// A pause and resume while the run goes bumps the goal revision but asks the worker nothing
/// new: the settled run is still graded and completes the goal.
#[tokio::test]
async fn a_run_that_outlived_a_pause_is_still_graded() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    let paused = transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Paused,
        Some("owner paused"),
    )
    .await
    .unwrap();
    transition_goal(
        &pool,
        &goal,
        paused.revision,
        RunState::Paused,
        RunState::Running,
        Some("owner resumed"),
    )
    .await
    .unwrap();
    settle(&pool, PANE, 1_000).await;

    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    let resumed = goal_of(&pool, &goal).await.unwrap().unwrap();
    assert!(
        resumed.revision > running.revision,
        "the hold bumped the revision"
    );
    transition_goal(
        &pool,
        &goal,
        resumed.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    let driver = CompletionDriver::new(pool.clone(), judge(&pool, model.clone()), control, HOST);
    let graded = driver.verify(&goal).await.unwrap();
    assert!(matches!(graded, Verification::Completed(_)), "{graded:?}");
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::Completed, "{detail}");
    assert_eq!(model.prompts().len(), 2, "graded, not redone");
    assert_eq!(turns_used(&pool, &lane).await, 0);
}

/// A verify-time owner hold bumps the revision; the owner's approval releases the goal to
/// `verifying` (T13 `release_target`: the latest run settled) without queueing anything. The
/// settled run still answers what the worker was asked, so it is graded and completes the goal
/// at the approved revision. The approval's write is reproduced here as one row update until
/// T13's `approve` is on this branch; after the rebase this drives `request::handle`.
#[tokio::test]
async fn an_approved_verify_time_hold_grades_the_settled_run() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    let verifying = transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    let held = transition_goal(
        &pool,
        &goal,
        verifying.revision,
        RunState::Verifying,
        RunState::AwaitingOwner,
        Some("OWNER_APPROVAL_REQUIRED: unproven r1; a supervised goal does not continue without the owner"),
    )
    .await
    .unwrap();
    assert!(
        held.revision > verifying.revision,
        "the hold bumped the revision"
    );
    // The approval: awaiting_owner -> verifying at the hold's revision, nothing queued.
    let approved = sqlx::query(
        "UPDATE supervisor_goals SET run_state = 'verifying', state_detail = 'OWNER_APPROVED: by owner'
          WHERE goal_id = ? AND revision = ? AND run_state = 'awaiting_owner'",
    )
    .bind(&goal)
    .bind(held.revision)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(approved.rows_affected(), 1);

    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    let driver = CompletionDriver::new(
        pool.clone(),
        judge(&pool, model.clone()),
        control.clone(),
        HOST,
    );
    let graded = driver.verify(&goal).await.unwrap();
    let Verification::Completed(record) = &graded else {
        panic!("the settled run was not graded: {graded:?}");
    };
    assert_eq!(record.goal_revision, held.revision);
    assert_eq!(record.request_id, sends(&control)[0]);
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Completed);
    assert_eq!(model.prompts().len(), 2, "graded, not redone");
    assert_eq!(sends(&control).len(), 1, "no new run");
    assert_eq!(turns_used(&pool, &lane).await, 0);
}

/// A lane pause that commits while the judge decides: the decision writes nothing.
#[tokio::test]
async fn a_lane_pause_during_grading_writes_nothing() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();

    let (entered_tx, mut entered) = mpsc::unbounded_channel();
    let gate = Arc::new(Notify::new());
    let model = Arc::new(ScriptedModel::with(
        &[ASSESS_COMPLETE, CONVERGED],
        Some((entered_tx, gate.clone())),
    ));
    let driver = Arc::new(CompletionDriver::new(
        pool.clone(),
        judge(&pool, model),
        control,
        HOST,
    ));
    let grading = {
        let driver = driver.clone();
        let goal = goal.clone();
        tokio::spawn(async move { driver.verify(&goal).await })
    };
    timeout(DEADLINE, entered.recv())
        .await
        .expect("the judge was asked")
        .unwrap();
    assert!(crate::lanes::pause::pause(&pool, &lane, "owner")
        .await
        .unwrap());
    gate.notify_one();
    let result = timeout(DEADLINE, grading).await.unwrap().unwrap().unwrap();
    assert_eq!(
        result,
        Verification::Skipped {
            code: LANE_PAUSED.into()
        }
    );
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Verifying);
    assert_eq!(
        requirement_status(&pool, &lane).await.0,
        RequirementStatus::Pending
    );
}

/// A lane pause that commits while the judge decides a correction: no corrective run, no
/// counted turn.
#[tokio::test]
async fn a_lane_pause_during_a_correction_decision_writes_nothing() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "NOT READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();

    let (entered_tx, mut entered) = mpsc::unbounded_channel();
    let gate = Arc::new(Notify::new());
    let model = Arc::new(ScriptedModel::with(
        &[ASSESS_CONTINUE],
        Some((entered_tx, gate.clone())),
    ));
    let driver = Arc::new(CompletionDriver::new(
        pool.clone(),
        judge(&pool, model),
        control.clone(),
        HOST,
    ));
    let grading = {
        let driver = driver.clone();
        let goal = goal.clone();
        tokio::spawn(async move { driver.verify(&goal).await })
    };
    timeout(DEADLINE, entered.recv())
        .await
        .expect("the judge was asked")
        .unwrap();
    assert!(crate::lanes::pause::pause(&pool, &lane, "owner")
        .await
        .unwrap());
    gate.notify_one();
    let result = timeout(DEADLINE, grading).await.unwrap().unwrap().unwrap();
    assert_eq!(
        result,
        Verification::Skipped {
            code: LANE_PAUSED.into()
        }
    );
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Verifying);
    assert!(
        ready_dispatches(&pool, 10).await.unwrap().is_empty(),
        "no corrective run"
    );
    assert_eq!(turns_used(&pool, &lane).await, 0);
    assert_eq!(sends(&control).len(), 1);
}

/// The binding moved to a new generation while the judge decided: the old run never
/// completes the goal.
#[tokio::test]
async fn a_run_of_an_old_generation_never_completes_the_goal() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();

    let (entered_tx, mut entered) = mpsc::unbounded_channel();
    let gate = Arc::new(Notify::new());
    let model = Arc::new(ScriptedModel::with(
        &[ASSESS_COMPLETE, CONVERGED],
        Some((entered_tx, gate.clone())),
    ));
    let driver = Arc::new(CompletionDriver::new(
        pool.clone(),
        judge(&pool, model),
        control,
        HOST,
    ));
    let grading = {
        let driver = driver.clone();
        let goal = goal.clone();
        tokio::spawn(async move { driver.verify(&goal).await })
    };
    timeout(DEADLINE, entered.recv())
        .await
        .expect("the judge was asked")
        .unwrap();
    let binding = execution_binding_of(&pool, &goal).await.unwrap().unwrap();
    crate::supervisor::advance_generation(&pool, &goal, binding.generation, Some(PANE))
        .await
        .unwrap();
    gate.notify_one();
    let result = timeout(DEADLINE, grading).await.unwrap().unwrap().unwrap();
    assert_eq!(
        result,
        Verification::Skipped {
            code: REVISION_CONFLICT.into()
        }
    );
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Verifying);
    assert_eq!(
        requirement_status(&pool, &lane).await.0,
        RequirementStatus::Pending
    );
}

/// A requirement whose verifier Ferryx cannot run (prose) is never graded by the model alone:
/// the owner waives it or names a runnable check.
#[tokio::test]
async fn a_requirement_without_a_runnable_check_waits_for_the_owner() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    sqlx::query(
        "UPDATE lane_goal_requirements SET verifier = 'the output looks right' WHERE lane_key = ?",
    )
    .bind(&lane)
    .execute(&pool)
    .await
    .unwrap();
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    let driver = CompletionDriver::new(pool.clone(), judge(&pool, model.clone()), control, HOST);
    let held = driver.verify(&goal).await.unwrap();
    assert!(
        matches!(&held, Verification::Held { state: RunState::AwaitingOwner, detail } if detail.starts_with(VERIFIER_NOT_RUNNABLE)),
        "{held:?}"
    );
    assert!(model.prompts().is_empty());
    assert_eq!(
        requirement_status(&pool, &lane).await.0,
        RequirementStatus::Pending
    );
}

/// A run bound on another host gets no local git or local checks; with requirements to check
/// it is held for the owner, never completed from this hub's checkout.
#[tokio::test]
async fn a_remote_run_is_never_checked_against_the_local_checkout() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    // This driver is on another machine than the binding's host.
    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    let driver = CompletionDriver::new(
        pool.clone(),
        judge(&pool, model.clone()),
        control,
        "hub-other",
    );
    let held = driver.verify(&goal).await.unwrap();
    assert!(
        matches!(&held, Verification::Held { state: RunState::AwaitingOwner, detail } if detail.starts_with(REMOTE_CHECK_UNAVAILABLE)),
        "{held:?}"
    );
    assert!(
        model.prompts().is_empty(),
        "nothing graded from the wrong checkout"
    );
    let bundle = crate::goals::evidence::collect_for_run(
        &pool,
        &RecordingControl::default(),
        &lane,
        Some(HOST),
        Some(PANE.into()),
        &contract_of(&pool, &lane)
            .await
            .unwrap()
            .unwrap()
            .requirements,
    )
    .await
    .unwrap();
    assert!(
        bundle
            .items
            .iter()
            .all(|item| !item.reference.starts_with("git:") && !item.reference.starts_with("sig:")),
        "{:?}",
        bundle.refs()
    );
}

/// A settle whose pane is not idle when it is about to be graded (a quiet-timer idle) is not
/// graded; the judge is never asked.
#[tokio::test]
async fn an_early_settle_on_a_busy_pane_is_not_graded() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    *control.result.lock().unwrap() = json!({"session": {"state": "working"}});
    let model = ScriptedModel::new(&[]);
    let driver = CompletionDriver::new(pool.clone(), judge(&pool, model.clone()), control, HOST);
    assert_eq!(
        driver.verify(&goal).await.unwrap(),
        Verification::Skipped {
            code: PANE_NOT_IDLE.into()
        }
    );
    assert!(model.prompts().is_empty());
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Verifying);
}

/// A judge failure keeps the goal verifying with a visible reason and backs off: the next
/// pass does not ask the judge again before the retry is due.
#[tokio::test]
async fn a_judge_failure_backs_off_instead_of_retrying_every_tick() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    // No scripted answers: every judge turn fails, twice per assessment (one retry).
    let model = ScriptedModel::new(&[]);
    let driver = CompletionDriver::new(pool.clone(), judge(&pool, model.clone()), control, HOST);
    assert_eq!(
        driver.verify(&goal).await.unwrap(),
        Verification::Skipped {
            code: JUDGE_UNAVAILABLE.into()
        }
    );
    let asked = model.prompts().len();
    assert!(asked >= 1);
    let (state, detail) = goal_state(&pool, &goal).await;
    assert_eq!(state, RunState::Verifying);
    assert!(detail.starts_with(JUDGE_UNAVAILABLE), "{detail}");
    assert_eq!(
        driver.verify(&goal).await.unwrap(),
        Verification::Skipped {
            code: RETRY_NOT_DUE.into()
        }
    );
    assert_eq!(
        model.prompts().len(),
        asked,
        "no judge call before the retry is due"
    );
}

#[tokio::test]
async fn a_stale_decision_cannot_overwrite_an_owner_correction() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let control = idle_control();
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let _repo = use_clean_repo(&pool, &lane, "READY").await;
    settle(&pool, PANE, 1_000).await;
    let running = goal_of(&pool, &goal).await.unwrap().unwrap();
    transition_goal(
        &pool,
        &goal,
        running.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();

    let (entered_tx, mut entered) = mpsc::unbounded_channel();
    let gate = Arc::new(Notify::new());
    let model = Arc::new(ScriptedModel::with(
        &[ASSESS_COMPLETE, CONVERGED],
        Some((entered_tx, gate.clone())),
    ));
    let driver = Arc::new(CompletionDriver::new(
        pool.clone(),
        judge(&pool, model.clone()),
        control.clone(),
        HOST,
    ));
    let grading = {
        let driver = driver.clone();
        let goal = goal.clone();
        tokio::spawn(async move { driver.verify(&goal).await })
    };
    timeout(DEADLINE, entered.recv())
        .await
        .expect("the judge was asked")
        .unwrap();

    // The owner corrects while the judge is deciding.
    let verifying = goal_of(&pool, &goal).await.unwrap().unwrap();
    let corrected = revise_goal(&pool, &goal, verifying.revision).await.unwrap();
    let instruction = enqueue_dispatch(&pool, &goal, corrected.revision, "Write DONE instead.")
        .await
        .unwrap();
    gate.notify_one();
    let stale = timeout(DEADLINE, grading).await.unwrap().unwrap().unwrap();
    assert_eq!(
        stale,
        Verification::Skipped {
            code: REVISION_CONFLICT.into()
        }
    );
    let (state, _) = goal_state(&pool, &goal).await;
    assert_eq!(
        state,
        RunState::Verifying,
        "the stale complete wrote nothing"
    );
    assert_eq!(
        requirement_status(&pool, &lane).await.0,
        RequirementStatus::Pending
    );

    // Graded again, the pre-correction run is never evidence: the correction goes out.
    let next = driver.verify(&goal).await.unwrap();
    assert_eq!(
        next,
        Verification::Resumed {
            request_id: instruction.request_id.clone()
        }
    );
    let ready = ready_dispatches(&pool, 10).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].request_id, instruction.request_id);
    assert_eq!(ready[0].goal_revision, corrected.revision);
    assert_eq!(model.prompts().len(), 2, "no judge call for a stale settle");
}

#[tokio::test]
async fn independent_goals_complete_independently_with_attached_evidence() {
    let pool = owner_pool().await;
    let control = idle_control();
    let (first, first_lane) = admit(&pool, "m-1", None).await;
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let (second, second_lane) = admit(&pool, "m-2", None).await;
    dispatcher(&pool, control.clone(), "pane-sv-2")
        .drain()
        .await;
    let _repo = use_clean_repo(&pool, &first_lane, "READY").await;
    settle(&pool, PANE, 1_000).await;

    let model = ScriptedModel::new(&[ASSESS_COMPLETE, CONVERGED]);
    let scheduler = completion_scheduler(&pool, model, control.clone());
    scheduler.tick(epoch()).await.unwrap();

    let (state, detail) = goal_state(&pool, &first).await;
    assert_eq!(state, RunState::Completed, "{detail}");
    let record: Value = serde_json::from_str(
        detail
            .strip_prefix(VERIFIED)
            .and_then(|rest| rest.strip_prefix(": "))
            .unwrap(),
    )
    .unwrap();
    let goal = goal_of(&pool, &first).await.unwrap().unwrap();
    assert_eq!(record["projectId"], json!(goal.project_id));
    assert_eq!(record["hostId"], json!(HOST));
    assert_eq!(record["goalRevision"], json!(goal.revision));
    assert!(record["runId"]
        .as_str()
        .unwrap()
        .starts_with(&format!("pty:{PANE}:")));
    assert_eq!(record["evidenceRefs"], json!(["git:clean", "sig:req:r1"]));
    let (status, evidence) = requirement_status(&pool, &first_lane).await;
    assert_eq!(status, RequirementStatus::Met);
    assert!(
        evidence.iter().any(|item| item.starts_with("run:")),
        "{evidence:?}"
    );

    assert_eq!(goal_state(&pool, &second).await.0, RunState::Running);
    assert_eq!(
        requirement_status(&pool, &second_lane).await.0,
        RequirementStatus::Pending
    );
    assert!(execution_binding_of(&pool, &second)
        .await
        .unwrap()
        .unwrap()
        .run_id
        .is_some());
}

/// A local target that waits for its pane after the claim was confirmed, then asks the typing
/// fence inside its input step, as the daemon's delivery does.
struct PaneWait {
    entered: mpsc::UnboundedSender<String>,
    ready: Arc<Notify>,
    typed: Mutex<Vec<String>>,
}

#[async_trait]
impl AgentControl for PaneWait {
    async fn execute(&self, _host_id: Option<&str>, _op: AgentOp) -> Result<Value> {
        Ok(json!({}))
    }

    async fn deliver(&self, send: &WorkerSend) -> Delivery {
        let _ = self.entered.send(send.request_id.clone());
        self.ready.notified().await;
        if check_before_typing(&send.request_id).is_err() {
            return Delivery::Refused {
                code: "CONTROL_CONFLICT".into(),
            };
        }
        self.typed.lock().unwrap().push(send.request_id.clone());
        Delivery::Started(StartReceipt {
            request_id: send.request_id.clone(),
            session_id: send.session_id.clone(),
            run_id: crate::agent_control::pty_run_id(&send.session_id, EPOCH, 1),
        })
    }
}

async fn claimed_send_waiting_for_its_pane(
    pool: &SqlitePool,
) -> (
    Arc<PaneWait>,
    Arc<Notify>,
    String,
    tokio::task::JoinHandle<Vec<DispatchResult>>,
) {
    let (entered_tx, mut entered) = mpsc::unbounded_channel();
    let ready = Arc::new(Notify::new());
    let control = Arc::new(PaneWait {
        entered: entered_tx,
        ready: ready.clone(),
        typed: Mutex::new(Vec::new()),
    });
    let dispatch = dispatcher(pool, control.clone(), PANE);
    let drain = tokio::spawn(async move { dispatch.drain().await });
    let request_id = timeout(DEADLINE, entered.recv())
        .await
        .expect("the claimed send reached the pane wait")
        .unwrap();
    (control, ready, request_id, drain)
}

async fn dispatch_state(pool: &SqlitePool, request_id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE request_id = ?")
        .bind(request_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_owner_pause_stops_a_claimed_send_that_waits_for_its_pane() {
    let pool = owner_pool().await;
    let (goal, _) = admit(&pool, "m-1", None).await;
    let (control, ready, request_id, drain) = claimed_send_waiting_for_its_pane(&pool).await;
    let claimed = goal_of(&pool, &goal).await.unwrap().unwrap();
    assert_eq!(claimed.run_state, RunState::Dispatching);
    transition_goal(
        &pool,
        &goal,
        claimed.revision,
        RunState::Dispatching,
        RunState::Paused,
        Some("owner paused"),
    )
    .await
    .unwrap();
    ready.notify_one();
    let results = timeout(DEADLINE, drain).await.unwrap().unwrap();
    assert!(
        matches!(results.as_slice(), [DispatchResult::Released { code, .. }] if code == "SUPERSEDED"),
        "{results:?}"
    );
    assert!(
        control.typed.lock().unwrap().is_empty(),
        "nothing was typed"
    );
    assert_eq!(dispatch_state(&pool, &request_id).await, "released");
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Paused);
    assert!(
        check_before_typing(&request_id).is_ok(),
        "the fence entry is dropped"
    );
}

#[tokio::test]
async fn a_lane_pause_stops_a_claimed_send_that_waits_for_its_pane() {
    let pool = owner_pool().await;
    let (goal, lane) = admit(&pool, "m-1", None).await;
    let (control, ready, request_id, drain) = claimed_send_waiting_for_its_pane(&pool).await;
    assert!(crate::lanes::pause::pause(&pool, &lane, "owner")
        .await
        .unwrap());
    ready.notify_one();
    let results = timeout(DEADLINE, drain).await.unwrap().unwrap();
    assert!(
        matches!(results.as_slice(), [DispatchResult::Held { state: RunState::Paused, detail, .. }] if detail.starts_with(LANE_PAUSED)),
        "{results:?}"
    );
    assert!(
        control.typed.lock().unwrap().is_empty(),
        "nothing was typed"
    );
    assert_eq!(dispatch_state(&pool, &request_id).await, "released");
    assert_eq!(goal_state(&pool, &goal).await.0, RunState::Paused);
}

#[tokio::test]
async fn cancel_is_not_pause() {
    let pool = owner_pool().await;
    let control = Arc::new(RecordingControl::observing());
    let (paused, paused_lane) = admit(&pool, "m-1", None).await;
    dispatcher(&pool, control.clone(), PANE).drain().await;
    let (cancelled, _) = admit(&pool, "m-2", None).await;
    dispatcher(&pool, control.clone(), "pane-sv-2")
        .drain()
        .await;

    // Pause gates new sends and decisions; the running worker is left alone and resumes.
    let running = goal_of(&pool, &paused).await.unwrap().unwrap();
    let held = transition_goal(
        &pool,
        &paused,
        running.revision,
        RunState::Running,
        RunState::Paused,
        Some("owner paused"),
    )
    .await
    .unwrap();
    assert!(execution_binding_of(&pool, &paused)
        .await
        .unwrap()
        .unwrap()
        .run_id
        .is_some());
    assert!(
        !control
            .ops()
            .iter()
            .any(|(_, op)| matches!(op, AgentOp::Stop { .. })),
        "a pause never stops a worker"
    );
    transition_goal(
        &pool,
        &paused,
        held.revision,
        RunState::Paused,
        RunState::Running,
        Some("owner resumed"),
    )
    .await
    .unwrap();
    assert!(held.revision > running.revision);

    // A paused lane gates an ongoing decision: no judge call, nothing written.
    let resumed = goal_of(&pool, &paused).await.unwrap().unwrap();
    settle(&pool, PANE, 1_000).await;
    transition_goal(
        &pool,
        &paused,
        resumed.revision,
        RunState::Running,
        RunState::Verifying,
        None,
    )
    .await
    .unwrap();
    crate::lanes::pause::pause(&pool, &paused_lane, "owner")
        .await
        .unwrap();
    let model = ScriptedModel::new(&[]);
    let driver = CompletionDriver::new(
        pool.clone(),
        judge(&pool, model.clone()),
        control.clone(),
        HOST,
    );
    assert_eq!(
        driver.verify(&paused).await.unwrap(),
        Verification::Skipped {
            code: LANE_PAUSED.into()
        }
    );
    assert!(model.prompts().is_empty());

    // Cancel is terminal: it never resumes and is never graded.
    let live = goal_of(&pool, &cancelled).await.unwrap().unwrap();
    let ended = transition_goal(
        &pool,
        &cancelled,
        live.revision,
        RunState::Running,
        RunState::Cancelled,
        Some("owner cancelled"),
    )
    .await
    .unwrap();
    let resume = transition_goal(
        &pool,
        &cancelled,
        ended.revision,
        RunState::Cancelled,
        RunState::Running,
        None,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(resume, SupervisorError::IllegalTransition { .. }),
        "{resume}"
    );
    assert_eq!(
        driver.verify(&cancelled).await.unwrap(),
        Verification::Skipped {
            code: "cancelled".into()
        }
    );
}

fn question(prompt_id: &str, options: &[&str]) -> WorkerPrompt {
    WorkerPrompt {
        prompt_id: prompt_id.into(),
        kind: PromptKind::Question,
        screen_readable: true,
        options: options.iter().map(|label| (*label).to_owned()).collect(),
    }
}

fn decided(revision: i64, prompt_id: &str, label: &str, source: DecisionSource) -> PromptDecision {
    PromptDecision {
        goal_revision: revision,
        prompt_id: prompt_id.into(),
        option_label: label.into(),
        source,
    }
}

#[test]
fn a_routine_question_is_answered_only_from_a_current_decision_with_its_exact_prompt_and_label() {
    let prompt = question("p-7", &["Use tabs", "Use spaces"]);
    let decisions = [decided(3, "p-7", "Use spaces", DecisionSource::Contract)];
    assert_eq!(
        routine_answer(&prompt, 3, &decisions),
        AnswerDecision::Answer {
            prompt_id: "p-7".into(),
            option_index: 1,
            option_label: "Use spaces".into(),
            source: DecisionSource::Contract,
        }
    );
    // An owner correction moved the revision: the old decision no longer answers.
    assert_eq!(
        routine_answer(&prompt, 4, &decisions),
        AnswerDecision::Owner {
            reason: OwnerReason::NoCurrentDecision
        }
    );
    // Another prompt id, or a label that is not exactly offered, goes to the owner.
    assert_eq!(
        routine_answer(&question("p-8", &["Use tabs", "Use spaces"]), 3, &decisions),
        AnswerDecision::Owner {
            reason: OwnerReason::NoCurrentDecision
        }
    );
    assert_eq!(
        routine_answer(&question("p-7", &["Use tabs", "use spaces"]), 3, &decisions),
        AnswerDecision::Owner {
            reason: OwnerReason::LabelNotOffered
        }
    );
    let split = [
        decided(3, "p-7", "Use spaces", DecisionSource::Contract),
        decided(3, "p-7", "Use tabs", DecisionSource::Owner),
    ];
    assert_eq!(
        routine_answer(&prompt, 3, &split),
        AnswerDecision::Owner {
            reason: OwnerReason::ConflictingDecisions
        }
    );
    let unreadable = WorkerPrompt {
        screen_readable: false,
        ..prompt.clone()
    };
    assert_eq!(
        routine_answer(&unreadable, 3, &decisions),
        AnswerDecision::Owner {
            reason: OwnerReason::Unreadable
        }
    );
}

#[test]
fn a_permission_prompt_always_goes_to_the_owner() {
    for kind in [PromptKind::Permission, PromptKind::Approval] {
        let prompt = WorkerPrompt {
            kind,
            ..question("p-9", &["Allow", "Deny"])
        };
        let decisions = [decided(1, "p-9", "Allow", DecisionSource::Owner)];
        assert_eq!(
            routine_answer(&prompt, 1, &decisions),
            AnswerDecision::Owner {
                reason: OwnerReason::PermissionRequired
            },
            "{kind:?}"
        );
    }
}
