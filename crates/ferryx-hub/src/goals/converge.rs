//! Blind convergence check. Before a lane is called complete, a separate model call grades
//! every requirement from the contract's requirements, their verifiers and the evidence
//! bundle ONLY. It never sees the gap assessment, earlier verdicts, stored statuses or the
//! agent's claim text; the signature and [`ConvergeInput`] make that structural.
//!
//! Ferryx then checks each `met: true` verdict and turns it into `met: false` unless:
//! - it cites at least one Ferryx-attested ref present in the bundle that did not fail;
//! - every attested ref it cites exists in the bundle (no fabricated `sig:999`);
//! - no ref it cites is failing evidence (`passed == Some(false)` or a failure sentinel).
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::contract::Contract;
use super::evidence::{is_attested_ref, EvidenceBundle, EvidenceItem};
use super::prompts::retry_prompt;
use super::types::{EvidenceSource, GoalError};
use crate::judge::{Judge, Purpose};

/// Ref ids that only ever report a failure or an unknown, so they never prove a requirement.
pub const FAILURE_REFS: [&str; 5] = [
    "git:dirty",
    "git:unknown",
    "observer:no-pane",
    "observer:unreadable",
    "sig:invalid",
];

/// One requirement's convergence verdict, after Ferryx's evidence check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementVerdict {
    pub req_id: String,
    pub met: bool,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ConvergenceOutcome {
    /// Every requirement is met with passing attested evidence.
    Converged { verdicts: Vec<RequirementVerdict> },
    /// At least one requirement is not met, or its `met` lacked passing attested evidence.
    Diverged {
        verdicts: Vec<RequirementVerdict>,
        unmet_ids: Vec<String>,
    },
}

/// Everything the convergence model sees. Requirements are id, text and verifier only (no
/// status, no stored evidence); an agent claim appears as its ref label with no text.
#[derive(Debug, Clone, Serialize)]
pub struct ConvergeInput {
    pub requirements: Vec<ConvergeRequirement>,
    pub evidence: Vec<ConvergeEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConvergeRequirement {
    pub req_id: String,
    pub text: String,
    pub verifier: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConvergeEvidence {
    #[serde(rename = "ref")]
    pub reference: String,
    pub source: EvidenceSource,
    pub attested: bool,
    pub passed: Option<bool>,
    /// `None` for an agent claim: its text never reaches the convergence check.
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelReply {
    requirements: Vec<RequirementVerdict>,
}

fn is_claim(item: &EvidenceItem) -> bool {
    item.source == EvidenceSource::AgentClaim
}

impl ConvergeInput {
    pub fn new(contract: &Contract, evidence: &EvidenceBundle) -> Self {
        Self {
            requirements: contract
                .requirements
                .iter()
                .map(|requirement| ConvergeRequirement {
                    req_id: requirement.id.clone(),
                    text: requirement.text.clone(),
                    verifier: requirement.verifier.clone(),
                })
                .collect(),
            evidence: evidence
                .items
                .iter()
                .map(|item| ConvergeEvidence {
                    reference: item.reference.clone(),
                    source: item.source,
                    attested: !is_claim(item) && is_attested_ref(&item.reference),
                    passed: item.passed,
                    summary: (!is_claim(item)).then(|| item.detail.clone()),
                })
                .collect(),
        }
    }
}

/// Builds the convergence prompt from [`ConvergeInput`] alone.
pub fn build_prompt(input: &ConvergeInput) -> String {
    let input = serde_json::to_string_pretty(input).unwrap_or_default();
    format!(
        "You independently check whether a coding lane's goal is complete. You see only the requirements, their verifiers and the evidence Ferryx collected.\n\
Rules:\n\
1. Set met to true only when a cited evidence ref with attested true and passed not false shows the verifier holds.\n\
2. An unattested ref (an agent claim) never proves a requirement.\n\
3. Cite only refs listed in the evidence. Never invent refs.\n\
4. Return one entry per requirement, each requirement exactly once.\n\
\n\
Input:\n{input}\n\
\n\
Reply with strict JSON only, no prose and no code fence:\n\
{{\"requirements\": [{{\"req_id\": \"<id>\", \"met\": true, \"evidence_refs\": [\"<ref>\"]}}]}}\n"
    )
}

fn strip_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

/// Parses the model reply as strict JSON. Every contract requirement must appear exactly
/// once and no unknown requirement may appear. Verdicts come back in contract order.
pub fn parse_verdicts(
    text: &str,
    contract: &Contract,
) -> Result<Vec<RequirementVerdict>, GoalError> {
    let reply: ModelReply = serde_json::from_str(strip_fence(text))
        .map_err(|error| GoalError::InvalidDecision(format!("convergence: {error}")))?;
    let known: HashSet<&str> = contract
        .requirements
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect();
    let mut seen = HashSet::new();
    for verdict in &reply.requirements {
        if !known.contains(verdict.req_id.as_str()) {
            return Err(GoalError::InvalidDecision(format!(
                "convergence: unknown requirement {}",
                verdict.req_id
            )));
        }
        if !seen.insert(verdict.req_id.clone()) {
            return Err(GoalError::InvalidDecision(format!(
                "convergence: requirement {} appears twice",
                verdict.req_id
            )));
        }
    }
    let mut ordered = Vec::with_capacity(contract.requirements.len());
    for requirement in &contract.requirements {
        let verdict = reply
            .requirements
            .iter()
            .find(|verdict| verdict.req_id == requirement.id)
            .ok_or_else(|| {
                GoalError::InvalidDecision(format!(
                    "convergence: requirement {} has no verdict",
                    requirement.id
                ))
            })?;
        ordered.push(verdict.clone());
    }
    Ok(ordered)
}

/// True when the item can never prove a requirement: a failed check or a failure sentinel.
fn is_failing(item: &EvidenceItem) -> bool {
    item.passed == Some(false) || FAILURE_REFS.contains(&item.reference.as_str())
}

/// True when `refs` prove a requirement: at least one passing attested ref present in the
/// bundle, no fabricated attested ref and no failing cited ref.
pub fn refs_prove_met(refs: &[String], evidence: &EvidenceBundle) -> bool {
    let mut has_proof = false;
    for reference in refs {
        match evidence.get(reference) {
            None if is_attested_ref(reference) => return false,
            None => {}
            Some(item) if is_failing(item) => return false,
            Some(item) => {
                if !is_claim(item) && is_attested_ref(reference) {
                    has_proof = true;
                }
            }
        }
    }
    has_proof
}

/// Applies the evidence rule to model verdicts and decides the outcome.
pub fn decide(verdicts: Vec<RequirementVerdict>, evidence: &EvidenceBundle) -> ConvergenceOutcome {
    let verdicts: Vec<RequirementVerdict> = verdicts
        .into_iter()
        .map(|verdict| RequirementVerdict {
            met: verdict.met && refs_prove_met(&verdict.evidence_refs, evidence),
            ..verdict
        })
        .collect();
    let unmet_ids: Vec<String> = verdicts
        .iter()
        .filter(|verdict| !verdict.met)
        .map(|verdict| verdict.req_id.clone())
        .collect();
    if unmet_ids.is_empty() && !verdicts.is_empty() {
        ConvergenceOutcome::Converged { verdicts }
    } else {
        ConvergenceOutcome::Diverged {
            verdicts,
            unmet_ids,
        }
    }
}

/// Runs the blind convergence check on the judge's structured model turn. Takes the
/// contract and evidence only, never an assessment. A malformed reply is retried once with
/// the parse error appended; a second malformed reply, a model failure, or a spent cap is
/// [`GoalError::JudgeUnavailable`]. Writes nothing to the contract.
pub async fn converge(
    judge: &Judge,
    contract: &Contract,
    evidence: &EvidenceBundle,
) -> Result<ConvergenceOutcome, GoalError> {
    if contract.requirements.is_empty() {
        return Err(GoalError::EmptyContract);
    }
    let prompt = build_prompt(&ConvergeInput::new(contract, evidence));
    let reply = judge.structured(Purpose::ConvergenceCheck, &prompt).await?;
    let verdicts = match parse_verdicts(&reply, contract) {
        Ok(verdicts) => verdicts,
        Err(error) => {
            tracing::warn!(%error, lane = %contract.lane_key, "convergence reply refused; retrying once");
            let retry = retry_prompt(&prompt, &error.to_string());
            let reply = judge.structured(Purpose::ConvergenceCheck, &retry).await?;
            parse_verdicts(&reply, contract).map_err(|error| {
                tracing::warn!(%error, lane = %contract.lane_key, "convergence reply refused twice");
                GoalError::JudgeUnavailable
            })?
        }
    };
    Ok(decide(verdicts, evidence))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use sqlx::SqlitePool;

    use super::*;
    use crate::goals::contract::Requirement;
    use crate::goals::evidence::CLAIM_REF;
    use crate::goals::types::{GoalMode, RequirementStatus};
    use crate::lanes::kibitzer::ModelTurn;

    const CLAIM_TEXT: &str = "[CLAIM_DONE] lane-a | everything is finished";

    /// Answers each turn with the next scripted reply; `None` is a failed turn.
    struct ScriptedModel {
        replies: Mutex<VecDeque<Option<String>>>,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ModelTurn for ScriptedModel {
        async fn complete(&self, prompt: &str) -> crate::error::Result<String> {
            self.prompts.lock().unwrap().push(prompt.to_owned());
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .flatten()
                .ok_or_else(|| crate::error::HubError::message("offline"))
        }
    }

    struct Harness {
        judge: Judge,
        pool: SqlitePool,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    impl Harness {
        async fn new(replies: Vec<Option<String>>) -> Self {
            let pool = crate::storage::memory().await.unwrap();
            let prompts = Arc::new(Mutex::new(Vec::new()));
            let model = ScriptedModel {
                replies: Mutex::new(replies.into()),
                prompts: prompts.clone(),
            };
            let judge = Judge::bare()
                .with_pool(pool.clone())
                .with_fallback(Arc::new(model));
            Self {
                judge,
                pool,
                prompts,
            }
        }

        async fn replying(text: &str) -> Self {
            Self::new(vec![Some(text.to_owned())]).await
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().unwrap().clone()
        }

        async fn convergence_calls(&self) -> i64 {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM judge_calls WHERE purpose = 'convergence_check' AND kind = 'structured'",
            )
            .fetch_one(&self.pool)
            .await
            .unwrap()
        }
    }

    fn contract(ids: &[&str]) -> Contract {
        Contract {
            lane_key: "lane-a".into(),
            mode: GoalMode::Supervised,
            turn_limit: 20,
            turns_used: 0,
            revision: 1,
            stop_reason: None,
            stop_detail: None,
            proposal: None,
            requirements: ids
                .iter()
                .map(|id| Requirement {
                    id: (*id).into(),
                    text: format!("requirement {id}"),
                    verifier: format!("check {id}"),
                    status: RequirementStatus::Met,
                    evidence: vec!["sig:7".into()],
                    updated_at: "2026-10-24T00:00:00Z".into(),
                })
                .collect(),
            updated_at: "2026-10-24T00:00:00Z".into(),
        }
    }

    fn item(reference: &str, source: EvidenceSource, passed: Option<bool>) -> EvidenceItem {
        EvidenceItem {
            reference: reference.into(),
            source,
            passed,
            detail: format!("detail of {reference}"),
        }
    }

    fn evidence() -> EvidenceBundle {
        EvidenceBundle {
            lane_key: "lane-a".into(),
            items: vec![
                item("observer:state", EvidenceSource::FerryxObserver, None),
                item("sig:0", EvidenceSource::FerryxSignal, Some(true)),
                item("sig:1", EvidenceSource::FerryxSignal, Some(false)),
                item("git:clean", EvidenceSource::FerryxGit, Some(true)),
                item("git:dirty", EvidenceSource::FerryxGit, Some(false)),
                item("git:unknown", EvidenceSource::FerryxGit, None),
                item("observer:no-pane", EvidenceSource::FerryxObserver, Some(false)),
                item("sig:invalid", EvidenceSource::FerryxSignal, None),
                EvidenceItem {
                    reference: CLAIM_REF.into(),
                    source: EvidenceSource::AgentClaim,
                    passed: None,
                    detail: CLAIM_TEXT.into(),
                },
            ],
        }
    }

    fn reply(verdicts: &[(&str, bool, &[&str])]) -> String {
        let requirements: Vec<serde_json::Value> = verdicts
            .iter()
            .map(|(id, met, refs)| {
                serde_json::json!({"req_id": id, "met": met, "evidence_refs": refs})
            })
            .collect();
        serde_json::json!({ "requirements": requirements }).to_string()
    }

    async fn run(verdicts: &[(&str, bool, &[&str])]) -> ConvergenceOutcome {
        let ids: Vec<&str> = verdicts.iter().map(|(id, _, _)| *id).collect();
        let harness = Harness::replying(&reply(verdicts)).await;
        converge(&harness.judge, &contract(&ids), &evidence())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn all_met_with_signal_refs_converges() {
        let outcome = run(&[
            ("r1", true, &["sig:0"]),
            ("r2", true, &["git:clean", CLAIM_REF]),
        ])
        .await;
        let ConvergenceOutcome::Converged { verdicts } = outcome else {
            panic!("expected Converged, got {outcome:?}");
        };
        assert!(verdicts.iter().all(|verdict| verdict.met));
        assert_eq!(verdicts[1].evidence_refs, ["git:clean", CLAIM_REF]);
    }

    async fn assert_downgraded(refs: &[&str]) {
        let outcome = run(&[("r1", true, &["sig:0"]), ("r2", true, refs)]).await;
        let ConvergenceOutcome::Diverged {
            verdicts,
            unmet_ids,
        } = outcome
        else {
            panic!("refs {refs:?} must diverge, got {outcome:?}");
        };
        assert_eq!(unmet_ids, ["r2"], "refs {refs:?}");
        assert!(verdicts[0].met);
        assert!(!verdicts[1].met);
        assert_eq!(verdicts[1].evidence_refs, refs);
    }

    #[tokio::test]
    async fn a_met_citing_only_the_claim_is_downgraded() {
        assert_downgraded(&[CLAIM_REF]).await;
        assert_downgraded(&[]).await;
    }

    #[tokio::test]
    async fn a_fabricated_ref_is_downgraded() {
        assert_downgraded(&["sig:999"]).await;
        assert_downgraded(&["git:nonexistent"]).await;
        assert_downgraded(&["sig:0", "sig:999"]).await;
    }

    #[tokio::test]
    async fn failing_evidence_is_downgraded() {
        assert_downgraded(&["sig:1"]).await;
        assert_downgraded(&["git:dirty"]).await;
        assert_downgraded(&["sig:0", "git:dirty"]).await;
        assert_downgraded(&["git:unknown"]).await;
        assert_downgraded(&["observer:no-pane"]).await;
        assert_downgraded(&["sig:invalid"]).await;
    }

    #[tokio::test]
    async fn a_model_unmet_verdict_diverges() {
        let outcome = run(&[("r1", false, &["sig:0"]), ("r2", true, &["git:clean"])]).await;
        assert!(matches!(
            outcome,
            ConvergenceOutcome::Diverged { ref unmet_ids, .. } if unmet_ids == &["r1"]
        ));
    }

    #[tokio::test]
    async fn the_prompt_carries_no_assessment_verdicts_or_claim_text() {
        let harness = Harness::replying(&reply(&[("r1", true, &["sig:0"])])).await;
        converge(&harness.judge, &contract(&["r1"]), &evidence())
            .await
            .unwrap();
        let prompts = harness.prompts();
        assert_eq!(prompts.len(), 1);
        let prompt = &prompts[0];
        assert!(!prompt.contains("\"decision\""));
        assert!(!prompt.contains("\"gaps\""));
        assert!(!prompt.contains(CLAIM_TEXT));
        assert!(prompt.contains(CLAIM_REF));

        let input = serde_json::to_value(ConvergeInput::new(&contract(&["r1"]), &evidence()))
            .unwrap();
        let mut keys: Vec<&str> = input.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["evidence", "requirements"]);
        let mut requirement_keys: Vec<&str> = input["requirements"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        requirement_keys.sort_unstable();
        assert_eq!(requirement_keys, ["req_id", "text", "verifier"]);
        let claim = input["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["ref"] == CLAIM_REF)
            .unwrap();
        assert_eq!(claim["attested"], false);
        assert!(claim["summary"].is_null());
        let signal = input["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["ref"] == "sig:0")
            .unwrap();
        assert_eq!(signal["attested"], true);
        assert_eq!(signal["passed"], true);
    }

    #[tokio::test]
    async fn twice_malformed_or_incomplete_replies_are_judge_unavailable() {
        let contract = contract(&["r1", "r2"]);
        let replies = [
            "not json".to_owned(),
            reply(&[("r1", true, &["sig:0"])]),
            reply(&[("r1", true, &["sig:0"]), ("r1", true, &["sig:0"]), ("r2", true, &["sig:0"])]),
            reply(&[("r1", true, &["sig:0"]), ("r2", true, &["sig:0"]), ("r9", true, &["sig:0"])]),
            r#"{"requirements":[{"req_id":"r1","met":true,"evidence_refs":[],"decision":"complete"}]}"#
                .to_owned(),
        ];
        for text in replies {
            let harness = Harness::new(vec![Some(text.clone()), Some(text.clone())]).await;
            assert!(
                matches!(
                    converge(&harness.judge, &contract, &evidence()).await,
                    Err(GoalError::JudgeUnavailable)
                ),
                "{text}"
            );
            assert_eq!(harness.prompts().len(), 2, "{text}");
            assert_eq!(harness.convergence_calls().await, 2, "{text}");
        }
    }

    #[tokio::test]
    async fn a_malformed_reply_succeeds_on_the_retry() {
        let harness = Harness::new(vec![
            Some("not json".to_owned()),
            Some(reply(&[("r1", true, &["sig:0"])])),
        ])
        .await;
        let outcome = converge(&harness.judge, &contract(&["r1"]), &evidence())
            .await
            .unwrap();
        assert!(matches!(outcome, ConvergenceOutcome::Converged { .. }));
        let prompts = harness.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].starts_with(&prompts[0]));
        assert!(prompts[1].len() > prompts[0].len());
        assert_eq!(harness.convergence_calls().await, 2);
    }

    #[tokio::test]
    async fn a_model_failure_is_judge_unavailable() {
        let harness = Harness::new(vec![None]).await;
        assert!(matches!(
            converge(&harness.judge, &contract(&["r1"]), &evidence()).await,
            Err(GoalError::JudgeUnavailable)
        ));
        assert_eq!(harness.prompts().len(), 1, "a failed turn is not retried");
    }

    #[tokio::test]
    async fn no_fallback_model_is_judge_unavailable() {
        assert!(matches!(
            converge(&Judge::bare(), &contract(&["r1"]), &evidence()).await,
            Err(GoalError::JudgeUnavailable)
        ));
    }

    #[tokio::test]
    async fn a_fenced_reply_is_accepted_and_ordered_by_the_contract() {
        let text = format!("```json\n{}\n```", reply(&[("r2", true, &["git:clean"]), ("r1", true, &["sig:0"])]));
        let harness = Harness::replying(&text).await;
        let outcome = converge(&harness.judge, &contract(&["r1", "r2"]), &evidence())
            .await
            .unwrap();
        let ConvergenceOutcome::Converged { verdicts } = outcome else {
            panic!("expected Converged");
        };
        let ids: Vec<&str> = verdicts.iter().map(|verdict| verdict.req_id.as_str()).collect();
        assert_eq!(ids, ["r1", "r2"]);
    }
}
