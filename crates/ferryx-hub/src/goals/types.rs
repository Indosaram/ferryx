//! Closed vocabularies of the supervisor goal loop. Every string here is stable wire:
//! it is stored in the hub database, shown in the CLI and snapshot, and mirrored by the UI.
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::HubError;

/// Longest instruction one `continue` decision may carry.
pub const INSTRUCTION_MAX_CHARS: usize = 4000;

/// Typed failure of the goal loop and its storage.
#[derive(Debug, thiserror::Error)]
pub enum GoalError {
    #[error("lane goal revision changed (expected {expected}, found {actual})")]
    RevisionChanged { expected: i64, actual: i64 },
    #[error("lane turn limit reached ({limit})")]
    TurnLimit { limit: u32 },
    #[error("requirement {req_id} needs at least one Ferryx-attested evidence ref to be met")]
    EvidenceRequired { req_id: String },
    #[error("a goal contract needs at least one requirement")]
    EmptyContract,
    /// Reserved for the criteria revision guard: the quote is not an owner's exact words.
    #[error("the instruction quote was not found in any owner-authored text")]
    QuoteNotFound,
    /// Reserved for the evaluators: no model, an open breaker, or a spent daily cap.
    #[error("the supervisor checker is unavailable")]
    JudgeUnavailable,
    #[error("a turn is in flight on lane {0}; its contract cannot be replaced now")]
    TurnInFlight(String),
    #[error("lane {0} has no goal contract")]
    NoContract(String),
    #[error("lane {lane_key} has no requirement {req_id}")]
    UnknownRequirement { lane_key: String, req_id: String },
    #[error("invalid contract: {0}")]
    InvalidContract(String),
    #[error("unknown {kind}: {value}")]
    Unknown { kind: &'static str, value: String },
    #[error("invalid model decision: {0}")]
    InvalidDecision(String),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Hub(#[from] HubError),
}

impl From<GoalError> for HubError {
    fn from(error: GoalError) -> Self {
        match error {
            GoalError::Sql(error) => HubError::Sql(error),
            GoalError::Hub(error) => error,
            other => HubError::message(other.to_string()),
        }
    }
}

/// A snake_case enum with `ALL`, `as_str`, `Display` and a `FromStr` that names the
/// vocabulary in its typed error. Serde uses the same strings.
macro_rules! goal_enum {
    ($(#[$meta:meta])* $name:ident, $kind:literal, { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &[$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = GoalError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $($text => Ok($name::$variant),)+
                    other => Err(GoalError::Unknown { kind: $kind, value: other.to_owned() }),
                }
            }
        }
    };
}

goal_enum!(
    /// Why the supervisor stopped or handed a lane to the owner. Closed list.
    StopReason, "stop reason", {
        NoFurtherProgress => "no_further_progress",
        HighStakesUserRequired => "high_stakes_user_required",
        CompletionEvidenceMissing => "completion_evidence_missing",
        TurnLimit => "turn_limit",
        DeliveryUncertain => "delivery_uncertain",
        JudgeUnavailable => "judge_unavailable",
    }
);

/// The only stop reasons a model decision may carry. The rest are assigned by the loop.
pub const MODEL_STOP_REASONS: [StopReason; 2] = [
    StopReason::NoFurtherProgress,
    StopReason::HighStakesUserRequired,
];

goal_enum!(
    /// What closing a gap needs.
    GapKind, "gap kind", {
        Work => "work",
        Verification => "verification",
        GoalRefinement => "goal_refinement",
        ExternalInput => "external_input",
    }
);

goal_enum!(
    /// Who drove a lane turn.
    TurnOrigin, "turn origin", {
        SubmittedRequest => "submitted_request",
        SupervisorProposal => "supervisor_proposal",
        HumanCorrection => "human_correction",
        Other => "other",
    }
);

goal_enum!(
    /// Where an evidence item came from. Only `agent_claim` is not attested by Ferryx.
    EvidenceSource, "evidence source", {
        FerryxObserver => "ferryx_observer",
        FerryxSignal => "ferryx_signal",
        FerryxGit => "ferryx_git",
        FerryxScreen => "ferryx_screen",
        AgentClaim => "agent_claim",
    }
);

goal_enum!(
    /// How the loop treats a lane's proposals.
    GoalMode, "goal mode", {
        Off => "off",
        Supervised => "supervised",
        Full => "full",
    }
);

goal_enum!(
    /// Status of one contract requirement.
    RequirementStatus, "requirement status", {
        Pending => "pending",
        Met => "met",
        Unmet => "unmet",
        Waived => "waived",
    }
);

goal_enum!(
    /// Life of the lane's single pending supervisor proposal.
    ProposalState, "proposal state", {
        Pending => "pending",
        AwaitingApproval => "awaiting_approval",
        Sent => "sent",
        Superseded => "superseded",
        Discarded => "discarded",
        Uncertain => "uncertain",
    }
);

/// One gap between the current state and the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub id: String,
    pub kind: GapKind,
    pub requirement_ids: Vec<String>,
    pub summary: String,
}

/// Exactly one decision per assessed turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Continue {
        instruction: String,
        gap_ids: Vec<String>,
        expected_change: String,
        verification: String,
    },
    Stop {
        reason: StopReason,
    },
    Complete,
}

impl Decision {
    /// Checks the limits a model-produced decision must respect: a model may stop only with
    /// a [`MODEL_STOP_REASONS`] reason, and a continue carries one bounded instruction.
    pub fn validate_from_model(&self) -> Result<(), GoalError> {
        match self {
            Decision::Stop { reason } if !MODEL_STOP_REASONS.contains(reason) => Err(
                GoalError::InvalidDecision(format!("{reason} is not a model stop reason")),
            ),
            Decision::Continue { instruction, .. } if instruction.trim().is_empty() => Err(
                GoalError::InvalidDecision("continue needs an instruction".into()),
            ),
            Decision::Continue { instruction, .. }
                if instruction.chars().count() > INSTRUCTION_MAX_CHARS =>
            {
                Err(GoalError::InvalidDecision(format!(
                    "instruction exceeds {INSTRUCTION_MAX_CHARS} characters"
                )))
            }
            _ => Ok(()),
        }
    }
}

/// One gap assessment: the gaps found and the single decision that follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub gaps: Vec<Gap>,
    pub decision: Decision,
}

/// Parses one model decision as strict JSON and applies [`Decision::validate_from_model`].
pub fn parse_model_decision(text: &str) -> Result<Decision, GoalError> {
    let decision: Decision = serde_json::from_str(text.trim())
        .map_err(|error| GoalError::InvalidDecision(error.to_string()))?;
    decision.validate_from_model()?;
    Ok(decision)
}

/// What a confirmed send observed after the pane accepted the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SendOutcome {
    /// The pane went Working or Blocked after the send.
    Started,
    /// Delivery cannot be confirmed. The instruction is never resent.
    Uncertain { code: String },
    /// Nothing was typed, e.g. CONTROL_CONFLICT while a person typed.
    Refused { code: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_round_trip<T>(all: &[T], as_str: fn(T) -> &'static str)
    where
        T: Copy + std::fmt::Debug + PartialEq + Serialize + for<'de> Deserialize<'de> + FromStr,
        <T as FromStr>::Err: std::fmt::Debug,
    {
        for value in all {
            let text = as_str(*value);
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                serde_json::Value::String(text.to_owned())
            );
            let back: T =
                serde_json::from_value(serde_json::Value::String(text.to_owned())).unwrap();
            assert_eq!(back, *value);
            assert_eq!(text.parse::<T>().unwrap(), *value);
        }
    }

    #[test]
    fn every_variant_round_trips_through_its_snake_case_string() {
        assert_round_trip(StopReason::ALL, StopReason::as_str);
        assert_round_trip(GapKind::ALL, GapKind::as_str);
        assert_round_trip(TurnOrigin::ALL, TurnOrigin::as_str);
        assert_round_trip(EvidenceSource::ALL, EvidenceSource::as_str);
        assert_round_trip(GoalMode::ALL, GoalMode::as_str);
        assert_round_trip(RequirementStatus::ALL, RequirementStatus::as_str);
        assert_round_trip(ProposalState::ALL, ProposalState::as_str);
        let reasons: Vec<&str> = StopReason::ALL
            .iter()
            .map(|reason| reason.as_str())
            .collect();
        assert_eq!(
            reasons,
            [
                "no_further_progress",
                "high_stakes_user_required",
                "completion_evidence_missing",
                "turn_limit",
                "delivery_uncertain",
                "judge_unavailable",
            ]
        );
    }

    #[test]
    fn decisions_gaps_and_send_outcomes_round_trip() {
        let decisions = [
            Decision::Continue {
                instruction: "run the tests".into(),
                gap_ids: vec!["g1".into()],
                expected_change: "tests pass".into(),
                verification: "sig:0".into(),
            },
            Decision::Stop {
                reason: StopReason::HighStakesUserRequired,
            },
            Decision::Complete,
        ];
        for decision in decisions {
            let text = serde_json::to_string(&decision).unwrap();
            assert_eq!(parse_model_decision(&text).unwrap(), decision);
        }
        assert_eq!(
            serde_json::to_value(Decision::Complete).unwrap(),
            serde_json::json!({"decision": "complete"})
        );
        let gap = Gap {
            id: "g1".into(),
            kind: GapKind::ExternalInput,
            requirement_ids: vec!["r1".into()],
            summary: "needs the owner's token".into(),
        };
        let value = serde_json::to_value(&gap).unwrap();
        assert_eq!(value["kind"], "external_input");
        assert_eq!(serde_json::from_value::<Gap>(value).unwrap(), gap);
        for outcome in [
            SendOutcome::Started,
            SendOutcome::Uncertain {
                code: "TURN_NOT_OBSERVED".into(),
            },
            SendOutcome::Refused {
                code: "CONTROL_CONFLICT".into(),
            },
        ] {
            let value = serde_json::to_value(&outcome).unwrap();
            assert_eq!(
                serde_json::from_value::<SendOutcome>(value).unwrap(),
                outcome
            );
        }
        assert_eq!(
            serde_json::to_value(SendOutcome::Started).unwrap(),
            serde_json::json!({"outcome": "started"})
        );
    }

    #[test]
    fn an_unknown_string_is_a_typed_error() {
        assert!(matches!(
            "done".parse::<StopReason>(),
            Err(GoalError::Unknown { kind: "stop reason", ref value }) if value == "done"
        ));
        assert!(matches!(
            "auto".parse::<GoalMode>(),
            Err(GoalError::Unknown {
                kind: "goal mode",
                ..
            })
        ));
        assert!(serde_json::from_str::<StopReason>("\"done\"").is_err());
        assert_eq!(
            "high_stakes_user_required".parse::<StopReason>().unwrap(),
            StopReason::HighStakesUserRequired
        );
    }

    #[test]
    fn the_model_decision_parser_refuses_loop_assigned_stop_reasons() {
        for reason in StopReason::ALL {
            let text = format!(r#"{{"decision":"stop","reason":"{reason}"}}"#);
            let parsed = parse_model_decision(&text);
            if MODEL_STOP_REASONS.contains(reason) {
                assert_eq!(parsed.unwrap(), Decision::Stop { reason: *reason });
            } else {
                assert!(
                    matches!(parsed, Err(GoalError::InvalidDecision(_))),
                    "{reason} must be refused"
                );
            }
        }
        assert!(matches!(
            parse_model_decision(r#"{"decision":"stop","reason":"turn_limit"}"#),
            Err(GoalError::InvalidDecision(_))
        ));
        assert!(matches!(
            parse_model_decision(r#"{"decision":"stop","reason":"done"}"#),
            Err(GoalError::InvalidDecision(_))
        ));
    }

    #[test]
    fn a_continue_must_carry_one_bounded_instruction() {
        let long = "x".repeat(INSTRUCTION_MAX_CHARS + 1);
        let text = serde_json::json!({
            "decision": "continue",
            "instruction": long,
            "gap_ids": ["g1"],
            "expected_change": "",
            "verification": "",
        })
        .to_string();
        assert!(matches!(
            parse_model_decision(&text),
            Err(GoalError::InvalidDecision(_))
        ));
        assert!(matches!(
            parse_model_decision(
                r#"{"decision":"continue","instruction":" ","gap_ids":["g1"],"expected_change":"","verification":""}"#
            ),
            Err(GoalError::InvalidDecision(_))
        ));
        assert!(matches!(
            parse_model_decision("not json"),
            Err(GoalError::InvalidDecision(_))
        ));
    }

    #[test]
    fn a_goal_error_becomes_a_hub_error() {
        let error: HubError = GoalError::TurnLimit { limit: 20 }.into();
        assert!(error.to_string().contains("turn limit"));
    }
}
