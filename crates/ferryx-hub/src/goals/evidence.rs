//! Evidence the goal evaluators may use, each item tagged by where it came from. Ferryx
//! attests the observer state, the screen, the done-signal checks and git; an agent's own
//! claim is carried for context but can never satisfy a requirement alone.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use super::types::{EvidenceSource, GoalError};
use crate::agent_control::{AgentControl, AgentOp};
use crate::lanes::signals::{check_signals, parse_signals, DoneSignal};

/// Longest screen excerpt an evidence item carries, taken from the screen's tail.
pub const SCREEN_EXCERPT_MAX_CHARS: usize = 4000;

/// Ref id of the lane's latest agent claim. Never attested.
pub const CLAIM_REF: &str = "claim:latest";

/// A requirement's verifier script cannot be run as admitted: the lane has no admitted source
/// revision, or the script is not in it. The worker could have written or rewritten it.
pub const VERIFIER_UNPINNED: &str = "VERIFIER_UNPINNED";

/// Interpreters whose first argument is the script a verifier runs.
const SCRIPT_INTERPRETERS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "python", "python3", "node", "bun", "ruby", "perl",
];

const ATTESTED_PREFIXES: [&str; 4] = ["sig:", "git:", "screen:", "observer:"];

/// True for a ref id Ferryx produced itself. Agent claims and unknown prefixes are not.
pub fn is_attested_ref(reference: &str) -> bool {
    ATTESTED_PREFIXES
        .iter()
        .any(|prefix| reference.len() > prefix.len() && reference.starts_with(prefix))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceItem {
    /// Stable id the evaluators cite: `sig:0`, `git:clean`, `screen:<rev>`, `claim:latest`.
    #[serde(rename = "ref")]
    pub reference: String,
    pub source: EvidenceSource,
    /// Whether the check passed, when the item is a check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passed: Option<bool>,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub lane_key: String,
    pub items: Vec<EvidenceItem>,
}

impl EvidenceBundle {
    pub fn refs(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|item| item.reference.clone())
            .collect()
    }

    /// Every ref Ferryx attests; `agent_claim` items are excluded.
    pub fn attested_refs(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.source != EvidenceSource::AgentClaim)
            .map(|item| item.reference.clone())
            .collect()
    }

    pub fn get(&self, reference: &str) -> Option<&EvidenceItem> {
        self.items.iter().find(|item| item.reference == reference)
    }
}

fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max)).collect()
}

fn read_items(value: &Value) -> Vec<EvidenceItem> {
    let text_at = |pointer: &str| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let state = text_at("/session/state");
    let source = text_at("/session/stateSource");
    let tier = text_at("/session/tier");
    let mut items = vec![EvidenceItem {
        reference: "observer:state".into(),
        source: EvidenceSource::FerryxObserver,
        passed: None,
        detail: format!("state={state} stateSource={source} tier={tier}"),
    }];
    let revision = text_at("/session/screen/revision");
    if !revision.is_empty() {
        let screen = text_at("/session/screen/text");
        items.push(EvidenceItem {
            reference: format!("screen:{revision}"),
            source: EvidenceSource::FerryxScreen,
            passed: None,
            detail: crate::judge::scrub_for_external(&tail_chars(
                &screen,
                SCREEN_EXCERPT_MAX_CHARS,
            )),
        });
    }
    items
}

/// Gathers a fresh, tagged evidence bundle for a lane: an agent-control read of its pane,
/// its done-signal checks, its git state and its latest claim. A source that cannot be read
/// becomes an item saying so, so the evaluators see the gap instead of a silent omission.
pub async fn collect<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    control: &C,
    lane_key: &str,
) -> Result<EvidenceBundle, GoalError> {
    let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT cwd, done_signals, omo_thread_id, claim_evidence FROM lanes WHERE key = ?",
    )
    .bind(lane_key)
    .fetch_optional(pool)
    .await?;
    let Some((cwd, done_signals, session_id, claim)) = row else {
        return Err(GoalError::Hub(crate::error::HubError::message(format!(
            "lane {lane_key} does not exist"
        ))));
    };
    let mut items = Vec::new();
    match session_id.filter(|id| !id.is_empty()) {
        Some(session_id) => match control
            .execute(
                crate::lanes::state::lane_host(pool, lane_key)
                    .await
                    .map_err(|error| GoalError::Hub(crate::error::HubError::message(error.to_string())))?
                    .as_deref(),
                AgentOp::Read { session_id },
            )
            .await
        {
            Ok(value) => items.extend(read_items(&value)),
            Err(error) => items.push(EvidenceItem {
                reference: "observer:unreadable".into(),
                source: EvidenceSource::FerryxObserver,
                passed: Some(false),
                detail: format!("the pane could not be read: {error}"),
            }),
        },
        None => items.push(EvidenceItem {
            reference: "observer:no-pane".into(),
            source: EvidenceSource::FerryxObserver,
            passed: Some(false),
            detail: "the lane has no pane".into(),
        }),
    }
    match parse_signals(&done_signals) {
        Ok(signals) => {
            let checks = check_signals(&signals, std::path::Path::new(&cwd)).await;
            for (index, check) in checks.into_iter().enumerate() {
                items.push(EvidenceItem {
                    reference: format!("sig:{index}"),
                    source: EvidenceSource::FerryxSignal,
                    passed: Some(check.passed),
                    detail: format!("{}: {}", check.signal, check.detail),
                });
            }
        }
        Err(error) => items.push(EvidenceItem {
            reference: "sig:invalid".into(),
            source: EvidenceSource::FerryxSignal,
            passed: Some(false),
            detail: format!("done signals do not parse: {error}"),
        }),
    }
    items.push(git_item(pool, lane_key, &cwd).await);
    if let Some(claim) = claim.filter(|text| !text.trim().is_empty()) {
        items.push(EvidenceItem {
            reference: CLAIM_REF.into(),
            source: EvidenceSource::AgentClaim,
            passed: None,
            detail: crate::judge::scrub_for_external(&claim),
        });
    }
    Ok(EvidenceBundle {
        lane_key: lane_key.to_owned(),
        items,
    })
}

/// The lane worktree's git state on this machine: `git:clean`, `git:dirty` or `git:unknown`.
async fn git_item(pool: &SqlitePool, lane_key: &str, cwd: &str) -> EvidenceItem {
    match crate::lanes::settle::worktree_reasons(pool, lane_key, cwd).await {
        Ok(reasons) if reasons.is_empty() => EvidenceItem {
            reference: "git:clean".into(),
            source: EvidenceSource::FerryxGit,
            passed: Some(true),
            detail: "no uncommitted tracked files or unpushed commits".into(),
        },
        Ok(reasons) => EvidenceItem {
            reference: "git:dirty".into(),
            source: EvidenceSource::FerryxGit,
            passed: Some(false),
            detail: reasons.join("; "),
        },
        Err(error) => EvidenceItem {
            reference: "git:unknown".into(),
            source: EvidenceSource::FerryxGit,
            passed: None,
            detail: format!("git state could not be read: {error}"),
        },
    }
}

/// Ref id of the check Ferryx ran for one requirement's verifier: `sig:req:<id>`.
pub fn requirement_check_ref(req_id: &str) -> String {
    format!("sig:req:{req_id}")
}

/// The check Ferryx itself can run for a requirement's verifier, when it names one:
/// - a done-signal `command [...]` or `file <path> [sha256=...]`;
/// - the shorthand `<script>.sh exits 0` (one relative path, no spaces), run as `sh <script>`.
///
/// Anything else (prose, a remote check, a network signal) is not a Ferryx-run check, so the
/// requirement can only be proven by an explicit owner waiver.
pub fn verifier_signal(verifier: &str) -> Option<DoneSignal> {
    if let Ok(mut signals) = parse_signals(verifier) {
        if signals.len() == 1
            && matches!(
                signals[0],
                DoneSignal::Command { .. } | DoneSignal::File { .. }
            )
        {
            return signals.pop();
        }
        return None;
    }
    let script = verifier.trim().strip_suffix("exits 0")?.trim();
    let plain = !script.is_empty()
        && script.ends_with(".sh")
        && !script.starts_with('-')
        && !script.starts_with('/')
        && !script.contains(char::is_whitespace)
        && !script.split('/').any(|part| part == "..");
    plain.then(|| DoneSignal::Command {
        argv: vec!["sh".to_owned(), script.to_owned()],
        stdout_regex: None,
    })
}

/// Index of the argv element naming the verifier's script: a relative path run directly
/// (`./verify.sh`), or an interpreter's first argument (`sh verify.sh`). A bare program name
/// (`cargo test`) or an inline `sh -c` names no script.
fn script_index(argv: &[String]) -> Option<usize> {
    let program = argv.first()?;
    if program.contains('/') && !program.starts_with('/') {
        return Some(0);
    }
    let name = program.rsplit('/').next().unwrap_or(program);
    let script = argv.get(1)?;
    (SCRIPT_INTERPRETERS.contains(&name) && !script.starts_with('-') && !script.starts_with('/'))
        .then_some(1)
}

/// The source revision the lane's worker started from: the commit its task-owned worktree
/// was created at, before any worker turn.
async fn admitted_revision(pool: &SqlitePool, lane_key: &str) -> Result<Option<String>, GoalError> {
    Ok(sqlx::query_scalar(
        "SELECT start_commit FROM lane_owned_worktrees
          WHERE lane_key = ? AND start_commit IS NOT NULL ORDER BY rowid LIMIT 1",
    )
    .bind(lane_key)
    .fetch_optional(pool)
    .await?)
}

/// A verifier script materialized from the admitted revision; its directory is removed on drop.
struct PinnedScript(std::path::PathBuf);

impl Drop for PinnedScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The check to run for `signal`: a command naming a script runs that script's bytes as they
/// were at `admitted` (read with `git cat-file blob`, no replace objects or filters, into a
/// run-owned temp file) in the worktree `cwd`, so a worker that rewrites the script changes
/// nothing. `Err` is a [`VERIFIER_UNPINNED`] detail. Other signals are returned as they are.
async fn pin_signal(
    cwd: &str,
    admitted: Option<&str>,
    signal: DoneSignal,
) -> Result<(DoneSignal, Option<PinnedScript>), String> {
    let (mut argv, stdout_regex) = match signal {
        DoneSignal::Command { argv, stdout_regex } => (argv, stdout_regex),
        other => return Ok((other, None)),
    };
    let Some(index) = script_index(&argv) else {
        return Ok((DoneSignal::Command { argv, stdout_regex }, None));
    };
    let script = argv[index].trim_start_matches("./").to_owned();
    if script.is_empty() || script.split('/').any(|part| part == "..") {
        return Err(format!(
            "{VERIFIER_UNPINNED}: {script} is not a path inside the worktree"
        ));
    }
    let Some(admitted) = admitted else {
        return Err(format!(
            "{VERIFIER_UNPINNED}: the lane has no admitted source revision to run {script} from"
        ));
    };
    let read = crate::child_env::command("git")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(cwd)
        .args(["cat-file", "blob"])
        .arg(format!("{admitted}:./{script}"))
        .output()
        .await
        .map_err(|error| format!("{VERIFIER_UNPINNED}: git could not read {script}: {error}"))?;
    if !read.status.success() {
        return Err(format!(
            "{VERIFIER_UNPINNED}: {script} is not in the admitted source revision {admitted}"
        ));
    }
    let dir = std::env::temp_dir().join(format!("ferryx-verifier-{}", uuid::Uuid::new_v4()));
    let pinned = PinnedScript(dir.clone());
    let name = std::path::Path::new(&script)
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "verifier".into());
    let path = dir.join(name);
    let written = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&path, &read.stdout).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).await?;
        }
        Ok::<(), std::io::Error>(())
    }
    .await;
    written
        .map_err(|error| format!("{VERIFIER_UNPINNED}: {script} was not materialized: {error}"))?;
    argv[index] = path.to_string_lossy().into_owned();
    Ok((DoneSignal::Command { argv, stdout_regex }, Some(pinned)))
}

/// The [`VERIFIER_UNPINNED`] details of the lane's non-waived requirements whose verifier
/// script cannot be run as admitted. Empty when every script check can be pinned.
pub async fn unpinned_verifiers(
    pool: &SqlitePool,
    lane_key: &str,
    requirements: &[super::contract::Requirement],
) -> Result<Vec<String>, GoalError> {
    let cwd: Option<String> = sqlx::query_scalar("SELECT cwd FROM lanes WHERE key = ?")
        .bind(lane_key)
        .fetch_optional(pool)
        .await?;
    let Some(cwd) = cwd else {
        return Ok(Vec::new());
    };
    let admitted = admitted_revision(pool, lane_key).await?;
    let mut unpinned = Vec::new();
    for requirement in requirements {
        if requirement.status == super::types::RequirementStatus::Waived {
            continue;
        }
        let Some(signal) = verifier_signal(&requirement.verifier) else {
            continue;
        };
        if let Err(detail) = pin_signal(&cwd, admitted.as_deref(), signal).await {
            unpinned.push(format!("{} ({})", detail, requirement.id));
        }
    }
    Ok(unpinned)
}

/// A fresh evidence bundle for one supervisor run: the bound worker session read on its host
/// (never `lanes.omo_thread_id`) and the latest claim; and, only when the run is on this
/// machine (`host_id` is `None`), the lane's git state and one Ferryx-run check per
/// requirement whose verifier names one ([`verifier_signal`], ref [`requirement_check_ref`]).
/// A run on another host gets no local git and no local checks: this hub's checkout at the
/// same path is not that host's. Waived requirements are not checked.
/// A verifier script runs as it was at the lane's admitted source revision ([`pin_signal`]);
/// one that cannot be pinned is a failed check.
pub async fn collect_for_run<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    control: &C,
    lane_key: &str,
    host_id: Option<&str>,
    session_id: Option<String>,
    requirements: &[super::contract::Requirement],
) -> Result<EvidenceBundle, GoalError> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT cwd, claim_evidence FROM lanes WHERE key = ?")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?;
    let Some((cwd, claim)) = row else {
        return Err(GoalError::Hub(crate::error::HubError::message(format!(
            "lane {lane_key} does not exist"
        ))));
    };
    let mut items = Vec::new();
    match session_id.filter(|id| !id.is_empty()) {
        Some(session_id) => match control.execute(host_id, AgentOp::Read { session_id }).await {
            Ok(value) => items.extend(read_items(&value)),
            Err(error) => items.push(EvidenceItem {
                reference: "observer:unreadable".into(),
                source: EvidenceSource::FerryxObserver,
                passed: Some(false),
                detail: format!("the pane could not be read: {error}"),
            }),
        },
        None => items.push(EvidenceItem {
            reference: "observer:no-pane".into(),
            source: EvidenceSource::FerryxObserver,
            passed: Some(false),
            detail: "the lane has no pane".into(),
        }),
    }
    if host_id.is_none() {
        items.push(git_item(pool, lane_key, &cwd).await);
        let admitted = admitted_revision(pool, lane_key).await?;
        for requirement in requirements {
            if requirement.status == super::types::RequirementStatus::Waived {
                continue;
            }
            let Some(signal) = verifier_signal(&requirement.verifier) else {
                continue;
            };
            let (passed, detail) = match pin_signal(&cwd, admitted.as_deref(), signal).await {
                Err(detail) => (false, detail),
                Ok((signal, pinned)) => {
                    let check =
                        check_signals(std::slice::from_ref(&signal), std::path::Path::new(&cwd))
                            .await
                            .pop();
                    drop(pinned);
                    match check {
                        Some(check) => {
                            (check.passed, format!("{}: {}", check.signal, check.detail))
                        }
                        None => (false, "the check did not run".to_owned()),
                    }
                }
            };
            items.push(EvidenceItem {
                reference: requirement_check_ref(&requirement.id),
                source: EvidenceSource::FerryxSignal,
                passed: Some(passed),
                detail: crate::judge::scrub_for_external(&detail),
            });
        }
    }
    if let Some(claim) = claim.filter(|text| !text.trim().is_empty()) {
        items.push(EvidenceItem {
            reference: CLAIM_REF.into(),
            source: EvidenceSource::AgentClaim,
            passed: None,
            detail: crate::judge::scrub_for_external(&claim),
        });
    }
    Ok(EvidenceBundle {
        lane_key: lane_key.to_owned(),
        items,
    })
}

/// The source revision (`git rev-parse HEAD`) of `cwd`, when it is a repository with a commit.
pub async fn source_revision(cwd: &str) -> Option<String> {
    let output = crate::child_env::command("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .output()
        .await
        .ok()?;
    let head = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !head.is_empty()).then_some(head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use crate::goals::contract::{contract_of, set_contract, update_requirement, RequirementSpec};
    use crate::goals::types::{GoalMode, RequirementStatus};
    use crate::lanes::state::{insert_lane, set_claim, set_pane};
    use serde_json::json;

    /// A git repository with no commits and one untracked marker: clean, nothing unpushed.
    fn clean_repo(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ferryx-goal-evidence-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let status = crate::child_env::std_command("git")
            .arg("-C")
            .arg(&root)
            .arg("init")
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(root.join("ok.txt"), "ok").unwrap();
        root
    }

    async fn lane(pool: &SqlitePool, cwd: &str) {
        insert_lane(pool, "lane-a", "A", "lead", cwd, "file ok.txt")
            .await
            .unwrap();
        set_pane(pool, "lane-a", "pane-1").await.unwrap();
        set_claim(pool, "lane-a", "[CLAIM_DONE] lane-a | all done")
            .await
            .unwrap();
    }

    fn reading_control() -> RecordingControl {
        let control = RecordingControl::default();
        *control.result.lock().unwrap() = json!({
            "session": {
                "state": "idle",
                "stateSource": "provider",
                "tier": "provider",
                "screen": {"revision": "r7", "text": "tests passed"}
            }
        });
        control
    }

    #[test]
    fn only_ferryx_refs_are_attested() {
        assert!(is_attested_ref("sig:0"));
        assert!(is_attested_ref("git:clean"));
        assert!(is_attested_ref("screen:r7"));
        assert!(is_attested_ref("observer:state"));
        assert!(!is_attested_ref(CLAIM_REF));
        assert!(!is_attested_ref("sig:"));
        assert!(!is_attested_ref("made-up:1"));
    }

    #[tokio::test]
    async fn a_read_signals_git_and_claim_become_tagged_items() {
        let root = clean_repo("tagged");
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, &root.to_string_lossy()).await;
        let control = reading_control();
        let bundle = collect(&pool, &control, "lane-a").await.unwrap();
        let source_of = |reference: &str| bundle.get(reference).map(|item| item.source);
        assert_eq!(
            source_of("observer:state"),
            Some(EvidenceSource::FerryxObserver)
        );
        assert_eq!(source_of("screen:r7"), Some(EvidenceSource::FerryxScreen));
        assert_eq!(source_of("sig:0"), Some(EvidenceSource::FerryxSignal));
        assert_eq!(source_of("git:clean"), Some(EvidenceSource::FerryxGit));
        assert_eq!(source_of(CLAIM_REF), Some(EvidenceSource::AgentClaim));
        assert_eq!(bundle.get("sig:0").unwrap().passed, Some(true));
        assert_eq!(bundle.get("screen:r7").unwrap().detail, "tests passed");
        assert!(bundle
            .get("observer:state")
            .unwrap()
            .detail
            .contains("state=idle"));
        let attested = bundle.attested_refs();
        assert!(attested.len() >= 2);
        assert!(attested.contains(&"sig:0".to_owned()));
        assert!(attested.contains(&"git:clean".to_owned()));
        assert!(!attested.contains(&CLAIM_REF.to_owned()));
        assert!(matches!(
            control.ops().as_slice(),
            [(None, AgentOp::Read { session_id })] if session_id == "pane-1"
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_screen_excerpt_keeps_the_tail() {
        let text = format!("{}END", "a".repeat(SCREEN_EXCERPT_MAX_CHARS));
        let items = read_items(&json!({
            "session": {"screen": {"revision": "r1", "text": text}}
        }));
        let detail = &items[1].detail;
        assert_eq!(detail.chars().count(), SCREEN_EXCERPT_MAX_CHARS);
        assert!(detail.ends_with("END"));
    }

    async fn contract_lane() -> (SqlitePool, i64) {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "/tmp").await;
        let contract = set_contract(
            &pool,
            "lane-a",
            &[RequirementSpec {
                id: "r1".into(),
                text: "the marker exists".into(),
                verifier: "file ok.txt".into(),
            }],
            GoalMode::Supervised,
            20,
        )
        .await
        .unwrap();
        (pool, contract.revision)
    }

    #[tokio::test]
    async fn marking_met_with_only_the_agent_claim_is_refused() {
        let (pool, revision) = contract_lane().await;
        let error = update_requirement(
            &pool,
            "lane-a",
            "r1",
            RequirementStatus::Met,
            &[CLAIM_REF.to_owned()],
            revision,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, GoalError::EvidenceRequired { ref req_id } if req_id == "r1"));
        let contract = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert_eq!(contract.requirements[0].status, RequirementStatus::Pending);
    }

    #[tokio::test]
    async fn marking_met_with_a_signal_ref_succeeds() {
        let (pool, revision) = contract_lane().await;
        update_requirement(
            &pool,
            "lane-a",
            "r1",
            RequirementStatus::Met,
            &["sig:0".to_owned(), CLAIM_REF.to_owned()],
            revision,
        )
        .await
        .unwrap();
        let contract = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert_eq!(contract.requirements[0].status, RequirementStatus::Met);
        assert_eq!(contract.requirements[0].evidence, ["sig:0", CLAIM_REF]);
    }

    #[test]
    fn only_a_command_file_or_script_verifier_is_a_ferryx_check() {
        assert_eq!(
            verifier_signal("verify.sh exits 0"),
            Some(DoneSignal::Command {
                argv: vec!["sh".into(), "verify.sh".into()],
                stdout_regex: None,
            })
        );
        assert!(matches!(
            verifier_signal("file ok.txt"),
            Some(DoneSignal::File { .. })
        ));
        for prose in [
            "the output looks right",
            "/etc/verify.sh exits 0",
            "../verify.sh exits 0",
            "my verify.sh exits 0",
            "verify.py exits 0",
        ] {
            assert_eq!(verifier_signal(prose), None, "{prose}");
        }
        assert_eq!(requirement_check_ref("r1"), "sig:req:r1");
    }
}
