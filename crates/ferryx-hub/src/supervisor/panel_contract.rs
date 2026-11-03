//! Frozen shared contract for the autonomous supervisor and its panel.
//!
//! Types and signatures only; every worker builds against this file and the UI mirror in
//! `ui/src/lib/supervisorPanel/contract.ts`. Wire shape is camelCase. The full list of routes,
//! commands and events is in `docs/plans/SUPERVISOR_PANEL_CONTRACT.md`.
//! Plan: `docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md`.
use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{HubError, Result};

/// Handle of one session on one computer: `<hostId>:<backendSessionId>`.
///
/// The host id never contains `:`; the backend session id may.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionRef(String);

impl SessionRef {
    /// Where the host id ends. A local host id has no `:`, so the first one splits. A paired
    /// computer's id is its URL (`https://relay:443/host/<machine>`, the machine part
    /// percent-encoded so it has no `:`), so its ref splits at the first `:` after `/host/`.
    fn split_at(raw: &str) -> Option<usize> {
        let first = raw.find(':')?;
        if raw[first..].starts_with("://") {
            let marker = raw[first + 3..].find("/host/")? + first + 3 + "/host/".len();
            return raw[marker..].find(':').map(|colon| marker + colon);
        }
        Some(first)
    }

    /// Builds a ref from its two parts.
    pub fn new(host_id: &str, backend_session_id: &str) -> Self {
        Self(format!("{host_id}:{backend_session_id}"))
    }

    /// Parses `<hostId>:<backendSessionId>`; both parts must be non-empty.
    pub fn parse(raw: &str) -> Result<Self> {
        match Self::split_at(raw).map(|at| (&raw[..at], &raw[at + 1..])) {
            Some((host, backend)) if !host.is_empty() && !backend.is_empty() => {
                Ok(Self(raw.to_string()))
            }
            _ => Err(HubError::message(format!("invalid session ref: {raw}"))),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn host_id(&self) -> &str {
        Self::split_at(&self.0).map_or("", |at| &self.0[..at])
    }

    pub fn backend_session_id(&self) -> &str {
        Self::split_at(&self.0).map_or("", |at| &self.0[at + 1..])
    }
}

impl fmt::Display for SessionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the health classifier decided about one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HealthKind {
    Healthy,
    #[serde(rename_all = "camelCase")]
    UpstreamError {
        status: Option<u16>,
    },
    #[serde(rename_all = "camelCase")]
    Quota {
        reset_at_ms: Option<i64>,
    },
    #[serde(rename_all = "camelCase")]
    Stalled {
        since_ms: i64,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHealth {
    pub session_ref: SessionRef,
    pub kind: HealthKind,
    pub observed_at_ms: i64,
}

/// A prompt a session is blocked on. `kind` is `question` | `permission` | `approval`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPrompt {
    pub prompt_id: String,
    pub kind: String,
    pub question: String,
    pub command: Option<String>,
    pub options: Vec<String>,
    pub screen_revision: Option<String>,
}

/// One session of the fleet as clients see it.
///
/// `state` is `working` | `blocked` | `idle` | `unknown`. `state_since_ms` and
/// `screen_changed_at_ms` are the daemon's `stateSince` / `screenChangedAt` (ms epoch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetSession {
    pub session_ref: SessionRef,
    pub host_id: String,
    pub host_label: String,
    pub title: String,
    pub cwd: String,
    pub agent: String,
    pub state: String,
    pub state_source: String,
    pub state_since_ms: Option<i64>,
    pub screen_changed_at_ms: Option<i64>,
    pub pending_prompt: Option<PendingPrompt>,
    pub last_assistant_line: Option<String>,
    pub updated_at_ms: i64,
    pub stale: bool,
    /// What daemon telemetry alone says about the session's health. Only `failed` is produced
    /// today: the agent process left the pane in the middle of a turn. Absent from older peers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    Answered,
    Retried,
    Resumed,
    Cancelled,
    Stopped,
    RuleChanged,
    Finished,
    Escalated,
}

/// Proof behind a claim: a delivery receipt, a state-transition sequence, a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub kind: String,
    pub id: String,
}

/// How to reverse an action; `kind` selects the handler, `payload` is its input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoHandle {
    pub kind: String,
    pub payload: serde_json::Value,
}

/// One action-log line. One row per incident, updated in place (`incident_key`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionEntry {
    pub id: i64,
    pub incident_key: String,
    pub session_ref: Option<SessionRef>,
    pub kind: ActionKind,
    pub summary: String,
    pub basis: String,
    pub evidence: Vec<EvidenceRef>,
    pub undo: Option<UndoHandle>,
    /// True while the incident is still being handled (shown under "I'm handling").
    pub live: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// A decision for the owner. The recommendation is mandatory by type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Escalation {
    pub session_ref: SessionRef,
    pub prompt: PendingPrompt,
    pub recommended_index: usize,
    pub reason: String,
    pub created_at_ms: i64,
}

/// Outcome of the answer-on-behalf step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AnswerDecision {
    #[serde(rename_all = "camelCase")]
    Answer { option_index: usize, basis: String },
    #[serde(rename_all = "camelCase")]
    Escalate {
        recommended_index: usize,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    UpstreamRetry,
    StallRestart,
    QuotaResume,
    AskBeforeIrreversible,
}

/// Where a rule applies; every `None` field means "any".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleScope {
    pub repo: Option<String>,
    pub host_id: Option<String>,
    pub agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingRule {
    pub id: i64,
    pub scope: RuleScope,
    pub kind: RuleKind,
    pub params: serde_json::Value,
    pub created_from_message_id: Option<String>,
    pub created_at_ms: i64,
}

/// The D2 defaults, in effect with no instruction. Ids 1-4; storage may renumber on seed.
///
/// Params shapes (seconds):
/// - `upstreamRetry`: `{"backoffSecs":[60,120,300,600],"thenEverySecs":600}`
/// - `stallRestart`: `{"thresholdSecs":300,"maxRestarts":1}`
/// - `quotaResume`: `{"fallbackSecs":600}`
/// - `askBeforeIrreversible`: `{"enabled":true}`
pub fn default_rules() -> Vec<StandingRule> {
    let rule = |id, kind, params| StandingRule {
        id,
        scope: RuleScope::default(),
        kind,
        params,
        created_from_message_id: None,
        created_at_ms: 0,
    };
    vec![
        rule(
            1,
            RuleKind::UpstreamRetry,
            serde_json::json!({ "backoffSecs": [60, 120, 300, 600], "thenEverySecs": 600 }),
        ),
        rule(
            2,
            RuleKind::StallRestart,
            serde_json::json!({ "thresholdSecs": 300, "maxRestarts": 1 }),
        ),
        rule(
            3,
            RuleKind::QuotaResume,
            serde_json::json!({ "fallbackSecs": 600 }),
        ),
        rule(
            4,
            RuleKind::AskBeforeIrreversible,
            serde_json::json!({ "enabled": true }),
        ),
    ]
}

/// Everything the Supervisor panel renders, in one snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelSnapshot {
    pub decisions: Vec<Escalation>,
    /// Live action entries (`live == true`).
    pub handling: Vec<ActionEntry>,
    pub sessions: Vec<FleetSession>,
    pub log: Vec<ActionEntry>,
    pub rules: Vec<StandingRule>,
    pub paused: bool,
    pub setup_required: bool,
}

/// Text sent to resume a session after a retry or cancel. `None`: unknown agent, escalate.
pub fn resume_text(agent: &str) -> Option<&'static str> {
    match agent {
        "claude" | "claude-code" => Some("continue"),
        "codex" => Some("continue"),
        _ => None,
    }
}

/// Persisted action log (task 2).
#[async_trait]
pub trait ActionLog: Send + Sync {
    /// Inserts a new incident row or updates the existing row with the same `incident_key`
    /// in place; returns the stored row.
    async fn upsert_incident(&self, entry: ActionEntry) -> Result<ActionEntry>;
    /// Entries with `updated_at_ms >= since_ms`, newest first.
    async fn list_today(&self, since_ms: i64) -> Result<Vec<ActionEntry>>;
    /// Clears the undo handle of entry `id` after its undo ran.
    async fn mark_undone(&self, id: i64, now_ms: i64) -> Result<()>;
}

/// Persisted standing rules (task 2).
#[async_trait]
pub trait RuleStore: Send + Sync {
    async fn list(&self) -> Result<Vec<StandingRule>>;
    /// Inserts (`id == 0`) or replaces a rule; returns the stored rule.
    async fn upsert(&self, rule: StandingRule) -> Result<StandingRule>;
}

/// Pure session health classification (task 3). `tail` is the terminal tail.
pub trait HealthClassifier: Send + Sync {
    fn classify(
        &self,
        session: &FleetSession,
        tail: &str,
        rules: &[StandingRule],
        now_ms: i64,
    ) -> HealthKind;
}

/// Direct control of sessions over agent control (tasks 4, 6, 9).
#[async_trait]
pub trait SessionControl: Send + Sync {
    /// Interrupts the running turn (Ctrl-C). Never goes through the engine.
    async fn stop(&self, session: &SessionRef) -> Result<()>;
    /// Types `text` into the session and submits it.
    async fn send(&self, session: &SessionRef, text: &str) -> Result<()>;
    /// Answers a pending prompt; fails when `screen_revision` is stale. On success returns the
    /// request id of the delivery receipt the host accepted it under, when the host returned one.
    async fn answer(
        &self,
        session: &SessionRef,
        prompt_id: &str,
        screen_revision: Option<&str>,
        option_index: usize,
    ) -> Result<Option<String>>;
    /// Last `lines` lines of the terminal.
    async fn read_tail(&self, session: &SessionRef, lines: usize) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_ref_round_trips() {
        let made = SessionRef::new("host-1", "abc:123");
        assert_eq!(made.as_str(), "host-1:abc:123");
        let parsed = SessionRef::parse(made.as_str()).unwrap();
        assert_eq!(parsed, made);
        assert_eq!(parsed.host_id(), "host-1");
        assert_eq!(parsed.backend_session_id(), "abc:123");
        assert_eq!(parsed.to_string(), "host-1:abc:123");
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json!("host-1:abc:123"));
        assert!(SessionRef::parse("nohost").is_err());
        assert!(SessionRef::parse(":x").is_err());
        assert!(SessionRef::parse("x:").is_err());
    }

    #[test]
    fn a_paired_computers_url_host_id_is_one_host_id() {
        let host = "http://127.0.0.1:8443/host/a%3Ab";
        let made = SessionRef::new(host, "abc:123");
        let parsed = SessionRef::parse(made.as_str()).unwrap();
        assert_eq!(parsed.host_id(), host);
        assert_eq!(parsed.backend_session_id(), "abc:123");
        assert!(SessionRef::parse("https://relay.example/host/m:").is_err());
        assert!(SessionRef::parse("https://relay.example/no-host-segment:s").is_err());
    }

    fn fleet_session() -> FleetSession {
        FleetSession {
            session_ref: SessionRef::new("mac", "s1"),
            host_id: "mac".into(),
            host_label: "This Mac".into(),
            title: "Fix relay reconnect".into(),
            cwd: "/work/ferryx".into(),
            agent: "claude".into(),
            state: "blocked".into(),
            state_source: "provider".into(),
            state_since_ms: Some(1_000),
            screen_changed_at_ms: None,
            pending_prompt: Some(PendingPrompt {
                prompt_id: "p1".into(),
                kind: "approval".into(),
                question: "Run the migration?".into(),
                command: None,
                options: vec!["Allow".into(), "Deny".into()],
                screen_revision: Some("r7".into()),
            }),
            last_assistant_line: None,
            updated_at_ms: 2_000,
            stale: false,
            health: None,
        }
    }

    #[test]
    fn fleet_session_json_is_camel_case() {
        let value = serde_json::to_value(fleet_session()).unwrap();
        assert_eq!(
            value,
            json!({
                "sessionRef": "mac:s1",
                "hostId": "mac",
                "hostLabel": "This Mac",
                "title": "Fix relay reconnect",
                "cwd": "/work/ferryx",
                "agent": "claude",
                "state": "blocked",
                "stateSource": "provider",
                "stateSinceMs": 1000,
                "screenChangedAtMs": null,
                "pendingPrompt": {
                    "promptId": "p1",
                    "kind": "approval",
                    "question": "Run the migration?",
                    "command": null,
                    "options": ["Allow", "Deny"],
                    "screenRevision": "r7"
                },
                "lastAssistantLine": null,
                "updatedAtMs": 2000,
                "stale": false
            })
        );
        let back: FleetSession = serde_json::from_value(value).unwrap();
        assert_eq!(back, fleet_session());
    }

    #[test]
    fn action_entry_json_is_camel_case() {
        let entry = ActionEntry {
            id: 3,
            incident_key: "proxy-502".into(),
            session_ref: Some(SessionRef::new("win", "s9")),
            kind: ActionKind::RuleChanged,
            summary: "Retried".into(),
            basis: "default rule".into(),
            evidence: vec![EvidenceRef {
                kind: "receipt".into(),
                id: "r1".into(),
            }],
            undo: Some(UndoHandle {
                kind: "restoreRule".into(),
                payload: json!({ "ruleId": 1 }),
            }),
            live: true,
            created_at_ms: 10,
            updated_at_ms: 20,
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(
            value,
            json!({
                "id": 3,
                "incidentKey": "proxy-502",
                "sessionRef": "win:s9",
                "kind": "ruleChanged",
                "summary": "Retried",
                "basis": "default rule",
                "evidence": [{ "kind": "receipt", "id": "r1" }],
                "undo": { "kind": "restoreRule", "payload": { "ruleId": 1 } },
                "live": true,
                "createdAtMs": 10,
                "updatedAtMs": 20
            })
        );
        assert_eq!(serde_json::from_value::<ActionEntry>(value).unwrap(), entry);
    }

    #[test]
    fn tagged_enums_use_camel_case_fields() {
        assert_eq!(
            serde_json::to_value(HealthKind::Quota {
                reset_at_ms: Some(5)
            })
            .unwrap(),
            json!({ "type": "quota", "resetAtMs": 5 })
        );
        assert_eq!(
            serde_json::to_value(HealthKind::Healthy).unwrap(),
            json!({ "type": "healthy" })
        );
        assert_eq!(
            serde_json::to_value(AnswerDecision::Escalate {
                recommended_index: 0,
                reason: "why".into()
            })
            .unwrap(),
            json!({ "type": "escalate", "recommendedIndex": 0, "reason": "why" })
        );
    }

    #[test]
    fn default_rules_match_the_plan() {
        let rules = default_rules();
        let find = |kind: RuleKind| rules.iter().find(|r| r.kind == kind).unwrap();
        assert_eq!(rules.len(), 4);
        assert_eq!(
            find(RuleKind::UpstreamRetry).params,
            json!({ "backoffSecs": [60, 120, 300, 600], "thenEverySecs": 600 })
        );
        assert_eq!(
            find(RuleKind::StallRestart).params,
            json!({ "thresholdSecs": 300, "maxRestarts": 1 })
        );
        assert_eq!(
            find(RuleKind::QuotaResume).params,
            json!({ "fallbackSecs": 600 })
        );
        assert_eq!(
            find(RuleKind::AskBeforeIrreversible).params,
            json!({ "enabled": true })
        );
        assert!(rules.iter().all(|r| r.scope == RuleScope::default()));
    }

    #[test]
    fn resume_text_known_agents_only() {
        assert_eq!(resume_text("claude"), Some("continue"));
        assert_eq!(resume_text("claude-code"), Some("continue"));
        assert_eq!(resume_text("codex"), Some("continue"));
        assert_eq!(resume_text("omo"), None);
    }
}
