//! Evidence the goal evaluators may use, each item tagged by where it came from. Ferryx
//! attests the observer state, the screen, the done-signal checks and git; an agent's own
//! claim is carried for context but can never satisfy a requirement alone.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use super::types::{EvidenceSource, GoalError};
use crate::agent_control::{AgentControl, AgentOp};
use crate::lanes::signals::{check_signals, parse_signals};

/// Longest screen excerpt an evidence item carries, taken from the screen's tail.
pub const SCREEN_EXCERPT_MAX_CHARS: usize = 4000;

/// Ref id of the lane's latest agent claim. Never attested.
pub const CLAIM_REF: &str = "claim:latest";

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
        Some(session_id) => match control.execute(None, AgentOp::Read { session_id }).await {
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
    match crate::lanes::settle::worktree_reasons(pool, lane_key, &cwd).await {
        Ok(reasons) if reasons.is_empty() => items.push(EvidenceItem {
            reference: "git:clean".into(),
            source: EvidenceSource::FerryxGit,
            passed: Some(true),
            detail: "no uncommitted tracked files or unpushed commits".into(),
        }),
        Ok(reasons) => items.push(EvidenceItem {
            reference: "git:dirty".into(),
            source: EvidenceSource::FerryxGit,
            passed: Some(false),
            detail: reasons.join("; "),
        }),
        Err(error) => items.push(EvidenceItem {
            reference: "git:unknown".into(),
            source: EvidenceSource::FerryxGit,
            passed: None,
            detail: format!("git state could not be read: {error}"),
        }),
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
}
