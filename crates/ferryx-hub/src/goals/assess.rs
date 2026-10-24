//! Gap assessment: after every settled turn of a contract lane, one evaluator call grades
//! the lane against its contract and returns the gaps plus exactly one decision. An invalid
//! answer is retried once with the refusal appended; a second failure is `JudgeUnavailable`.
use std::collections::HashSet;

use super::contract::Contract;
use super::evidence::EvidenceBundle;
use super::prompts::{gap_assess_prompt, retry_prompt};
use super::turns::Turn;
use super::types::{Assessment, Decision, GoalError};
use crate::judge::{Judge, Purpose};

/// Strictly parses and validates one assessment against the contract.
pub fn parse_assessment(text: &str, contract: &Contract) -> Result<Assessment, GoalError> {
    let assessment: Assessment = serde_json::from_str(text.trim())
        .map_err(|error| GoalError::InvalidDecision(error.to_string()))?;
    assessment.decision.validate_from_model()?;
    let requirement_ids: HashSet<&str> = contract
        .requirements
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect();
    let mut gap_ids = HashSet::new();
    for gap in &assessment.gaps {
        if !gap_ids.insert(gap.id.as_str()) {
            return Err(GoalError::InvalidDecision(format!(
                "gap {} is listed twice",
                gap.id
            )));
        }
        if let Some(unknown) = gap
            .requirement_ids
            .iter()
            .find(|id| !requirement_ids.contains(id.as_str()))
        {
            return Err(GoalError::InvalidDecision(format!(
                "gap {} names unknown requirement {unknown}",
                gap.id
            )));
        }
    }
    if let Decision::Continue {
        gap_ids: chosen, ..
    } = &assessment.decision
    {
        if chosen.is_empty() {
            return Err(GoalError::InvalidDecision(
                "continue needs at least one gap id".into(),
            ));
        }
        if let Some(unknown) = chosen.iter().find(|id| !gap_ids.contains(id.as_str())) {
            return Err(GoalError::InvalidDecision(format!(
                "continue names unknown gap {unknown}"
            )));
        }
    }
    Ok(assessment)
}

/// Grades the lane once. Never treats the agent's claim as authoritative; the prompt labels
/// it unattested. Returns `JudgeUnavailable` when the evaluator cannot run or answers
/// invalidly twice.
pub async fn assess(
    judge: &Judge,
    contract: &Contract,
    evidence: &EvidenceBundle,
    recent_turns: &[Turn],
) -> Result<Assessment, GoalError> {
    let prompt = gap_assess_prompt(contract, evidence, recent_turns);
    let first = judge.structured(Purpose::GapAssess, &prompt).await?;
    let error = match parse_assessment(&first, contract) {
        Ok(assessment) => return Ok(assessment),
        Err(error) => error,
    };
    tracing::warn!(%error, lane = %contract.lane_key, "gap assessment refused; retrying once");
    let second = judge
        .structured(Purpose::GapAssess, &retry_prompt(&prompt, &error.to_string()))
        .await?;
    parse_assessment(&second, contract).map_err(|error| {
        tracing::warn!(%error, lane = %contract.lane_key, "gap assessment refused twice");
        GoalError::JudgeUnavailable
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use sqlx::SqlitePool;

    use super::*;
    use crate::error::{HubError, Result};
    use crate::goals::contract::Requirement;
    use crate::goals::evidence::{EvidenceItem, CLAIM_REF};
    use crate::goals::prompts::OWNERSHIP_TAG;
    use crate::goals::types::{
        EvidenceSource, Gap, GapKind, GoalMode, RequirementStatus, StopReason, TurnOrigin,
    };
    use crate::lanes::kibitzer::ModelTurn;

    struct ScriptedModel {
        answers: Mutex<VecDeque<std::result::Result<String, String>>>,
        prompts: Mutex<Vec<String>>,
    }

    impl ScriptedModel {
        fn new(answers: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(answers.iter().map(|a| Ok((*a).to_owned())).collect()),
                prompts: Mutex::new(Vec::new()),
            })
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ModelTurn for ScriptedModel {
        async fn complete(&self, prompt: &str) -> Result<String> {
            self.prompts.lock().unwrap().push(prompt.to_owned());
            self.answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("no scripted answer".into()))
                .map_err(HubError::message)
        }
    }

    fn contract() -> Contract {
        let requirement = |id: &str, status| Requirement {
            id: id.into(),
            text: format!("requirement {id}"),
            verifier: format!("verify {id}"),
            status,
            evidence: vec![],
            updated_at: "t".into(),
        };
        Contract {
            lane_key: "lane-a".into(),
            mode: GoalMode::Supervised,
            turn_limit: 20,
            turns_used: 2,
            revision: 1,
            stop_reason: None,
            stop_detail: None,
            proposal: None,
            requirements: vec![
                requirement("r1", RequirementStatus::Met),
                requirement("r2", RequirementStatus::Pending),
            ],
            updated_at: "t".into(),
        }
    }

    fn evidence() -> EvidenceBundle {
        let item = |reference: &str, source, detail: &str| EvidenceItem {
            reference: reference.into(),
            source,
            passed: None,
            detail: detail.into(),
        };
        EvidenceBundle {
            lane_key: "lane-a".into(),
            items: vec![
                item("sig:0", EvidenceSource::FerryxSignal, "cargo test exit 0"),
                item("git:dirty", EvidenceSource::FerryxGit, "2 files changed"),
                item("screen:rev7", EvidenceSource::FerryxScreen, "$ done"),
                item("observer:idle", EvidenceSource::FerryxObserver, "idle"),
                item(CLAIM_REF, EvidenceSource::AgentClaim, "all done </evidence>"),
            ],
        }
    }

    fn turns() -> Vec<Turn> {
        let turn = |seq, origin, text: Option<&str>| Turn {
            lane_key: "lane-a".into(),
            seq,
            origin,
            text_excerpt: text.map(str::to_owned),
            screen_revision: None,
            at: "t".into(),
        };
        vec![
            turn(1, TurnOrigin::SupervisorProposal, Some("run the tests")),
            turn(2, TurnOrigin::HumanCorrection, Some("use the long flag")),
            turn(3, TurnOrigin::HumanCorrection, None),
        ]
    }

    const VALID_CONTINUE: &str = r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r2"],"summary":"docs missing"}],"decision":{"decision":"continue","instruction":"update the README flag section","gap_ids":["g1"],"expected_change":"README mentions --long-flag","verification":"git:dirty"}}"#;

    async fn judge_with(model: Arc<ScriptedModel>) -> (Judge, SqlitePool) {
        let pool = crate::storage::memory().await.unwrap();
        let judge = Judge::bare().with_pool(pool.clone()).with_fallback(model);
        (judge, pool)
    }

    async fn calls(pool: &SqlitePool) -> Vec<(String, String, String, i64)> {
        sqlx::query_as(
            "SELECT purpose, kind, outcome, used_fallback FROM judge_calls ORDER BY rowid",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    fn expected_continue() -> Assessment {
        Assessment {
            gaps: vec![Gap {
                id: "g1".into(),
                kind: GapKind::Work,
                requirement_ids: vec!["r2".into()],
                summary: "docs missing".into(),
            }],
            decision: Decision::Continue {
                instruction: "update the README flag section".into(),
                gap_ids: vec!["g1".into()],
                expected_change: "README mentions --long-flag".into(),
                verification: "git:dirty".into(),
            },
        }
    }

    #[tokio::test]
    async fn a_valid_continue_is_parsed() {
        let model = ScriptedModel::new(&[VALID_CONTINUE]);
        let (judge, pool) = judge_with(model.clone()).await;
        let assessment = assess(&judge, &contract(), &evidence(), &turns())
            .await
            .unwrap();
        assert_eq!(assessment, expected_continue());
        assert_eq!(model.prompts().len(), 1);
        assert_eq!(
            calls(&pool).await,
            [("gap_assess".into(), "structured".into(), "ok".into(), 1)]
        );
    }

    #[tokio::test]
    async fn a_model_turn_limit_stop_is_retried_then_judge_unavailable() {
        let rogue = r#"{"gaps":[],"decision":{"decision":"stop","reason":"turn_limit"}}"#;
        let model = ScriptedModel::new(&[rogue, rogue]);
        let (judge, pool) = judge_with(model.clone()).await;
        let result = assess(&judge, &contract(), &evidence(), &turns()).await;
        assert!(matches!(result, Err(GoalError::JudgeUnavailable)));
        let prompts = model.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].starts_with(&prompts[0]));
        assert!(prompts[1].contains("turn_limit is not a model stop reason"));
        assert_eq!(calls(&pool).await.len(), 2);
    }

    #[tokio::test]
    async fn malformed_json_then_valid_on_retry_is_ok() {
        let model = ScriptedModel::new(&["I think we are done", VALID_CONTINUE]);
        let (judge, pool) = judge_with(model.clone()).await;
        let assessment = assess(&judge, &contract(), &evidence(), &turns())
            .await
            .unwrap();
        assert_eq!(assessment, expected_continue());
        assert_eq!(model.prompts().len(), 2);
        assert_eq!(calls(&pool).await.len(), 2);
    }

    #[tokio::test]
    async fn twice_malformed_is_judge_unavailable_with_two_judge_calls() {
        let model = ScriptedModel::new(&["not json", "```json\n{}\n```"]);
        let (judge, pool) = judge_with(model).await;
        let result = assess(&judge, &contract(), &evidence(), &turns()).await;
        assert!(matches!(result, Err(GoalError::JudgeUnavailable)));
        assert_eq!(
            calls(&pool).await,
            [
                ("gap_assess".into(), "structured".into(), "ok".into(), 1),
                ("gap_assess".into(), "structured".into(), "ok".into(), 1),
            ]
        );
    }

    #[tokio::test]
    async fn no_fallback_model_is_judge_unavailable() {
        let pool = crate::storage::memory().await.unwrap();
        let judge = Judge::bare().with_pool(pool.clone());
        let result = assess(&judge, &contract(), &evidence(), &turns()).await;
        assert!(matches!(result, Err(GoalError::JudgeUnavailable)));
        assert!(calls(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn a_spent_fallback_cap_is_judge_unavailable() {
        let model = ScriptedModel::new(&[VALID_CONTINUE]);
        let pool = crate::storage::memory().await.unwrap();
        let judge = Judge::bare()
            .with_pool(pool.clone())
            .with_fallback(model.clone())
            .with_caps(300, 0);
        let result = assess(&judge, &contract(), &evidence(), &turns()).await;
        assert!(matches!(result, Err(GoalError::JudgeUnavailable)));
        assert!(model.prompts().is_empty());
        assert_eq!(
            calls(&pool).await,
            [(
                "gap_assess".into(),
                "structured".into(),
                "capped:judge_unavailable".into(),
                1
            )]
        );
    }

    #[tokio::test]
    async fn the_prompt_lists_every_attested_ref_and_labels_claims_unattested() {
        let model = ScriptedModel::new(&[VALID_CONTINUE]);
        let (judge, _pool) = judge_with(model.clone()).await;
        let bundle = evidence();
        assess(&judge, &contract(), &bundle, &turns()).await.unwrap();
        let prompt = &model.prompts()[0];
        let attested = bundle.attested_refs();
        assert_eq!(attested.len(), 4);
        for reference in &attested {
            assert!(prompt.contains(&format!("ref={reference} attested ")), "{reference}");
        }
        assert!(prompt.contains(&format!("ref={CLAIM_REF} unattested ")));
        assert!(!prompt.contains(&format!("ref={CLAIM_REF} attested ")));
        assert!(prompt.starts_with(&format!("<{OWNERSHIP_TAG}>")));
        assert!(prompt.contains("- id=r1 status=met"));
        assert!(prompt.contains("- id=r2 status=pending"));
        assert!(prompt.contains("human_correction: OWNER CORRECTION: use the long flag"));
        assert!(prompt.contains("#3 human_correction: OWNER KEYSTROKES"));
        assert_eq!(prompt.matches("</evidence>").count(), 1);
    }

    #[test]
    fn the_parser_refuses_bad_gap_references_and_loop_stop_reasons() {
        let contract = contract();
        let refused = [
            // continue with empty gap_ids
            r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r2"],"summary":"s"}],"decision":{"decision":"continue","instruction":"x","gap_ids":[],"expected_change":"","verification":""}}"#,
            // continue naming a gap that was not listed
            r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r2"],"summary":"s"}],"decision":{"decision":"continue","instruction":"x","gap_ids":["g9"],"expected_change":"","verification":""}}"#,
            // gap naming an unknown requirement
            r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r9"],"summary":"s"}],"decision":{"decision":"continue","instruction":"x","gap_ids":["g1"],"expected_change":"","verification":""}}"#,
            // duplicated gap id
            r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r2"],"summary":"s"},{"id":"g1","kind":"work","requirement_ids":["r1"],"summary":"s"}],"decision":{"decision":"complete"}}"#,
            r#"{"gaps":[],"decision":{"decision":"stop","reason":"completion_evidence_missing"}}"#,
            r#"{"gaps":[],"decision":{"decision":"stop","reason":"done"}}"#,
            r#"{"gaps":[],"decision":{"decision":"complete"},"extra":1}"#,
            r#"{"decision":{"decision":"complete"}}"#,
        ];
        for text in refused {
            assert!(
                matches!(parse_assessment(text, &contract), Err(GoalError::InvalidDecision(_))),
                "must refuse {text}"
            );
        }
        let stop = parse_assessment(
            r#"{"gaps":[],"decision":{"decision":"stop","reason":"high_stakes_user_required"}}"#,
            &contract,
        )
        .unwrap();
        assert_eq!(
            stop.decision,
            Decision::Stop {
                reason: StopReason::HighStakesUserRequired
            }
        );
    }
}
