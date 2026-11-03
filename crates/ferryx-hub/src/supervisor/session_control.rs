//! [`SessionControl`] over agent control (plan todo 6, finding M9).
//!
//! Every call goes to the host named by the session ref through [`AgentControl`]; the daemon
//! adapter behind it is installed elsewhere. An answer needs the screen revision the prompt was
//! read at: [`AgentOp::Answer`] carries a revision the daemon compares with the live screen, so
//! a missing one is refused here, before any op is sent, rather than replaced by an empty or
//! made-up revision that the stale guard could not tell from a real one.
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use uuid::Uuid;

use super::panel_contract::{SessionControl, SessionRef};
use crate::agent_control::{AgentControl, AgentOp};
use crate::error::{HubError, Result};

/// Code that starts the refusal message of an answer without a screen revision.
pub const MISSING_SCREEN_REVISION: &str = "SUPERVISOR_SCREEN_REVISION_MISSING";

pub struct AgentSessionControl {
    control: Arc<dyn AgentControl>,
}

impl AgentSessionControl {
    pub fn new(control: Arc<dyn AgentControl>) -> Self {
        Self { control }
    }
}

#[async_trait]
impl SessionControl for AgentSessionControl {
    async fn stop(&self, session: &SessionRef) -> Result<()> {
        self.control
            .execute(
                Some(session.host_id()),
                AgentOp::Stop {
                    session_id: session.backend_session_id().to_owned(),
                    request_id: Uuid::new_v4().to_string(),
                },
            )
            .await
            .map(|_| ())
    }

    async fn send(&self, session: &SessionRef, text: &str) -> Result<()> {
        self.control
            .execute(
                Some(session.host_id()),
                AgentOp::Send {
                    session_id: session.backend_session_id().to_owned(),
                    text: text.to_owned(),
                    request_id: Uuid::new_v4().to_string(),
                },
            )
            .await
            .map(|_| ())
    }

    async fn answer(
        &self,
        session: &SessionRef,
        prompt_id: &str,
        screen_revision: Option<&str>,
        option_index: usize,
    ) -> Result<Option<String>> {
        let Some(revision) = screen_revision.filter(|revision| !revision.trim().is_empty()) else {
            return Err(HubError::message(format!(
                "{MISSING_SCREEN_REVISION}: the answer names no screen revision, so it cannot be checked against the live prompt; read the session again"
            )));
        };
        let accepted = self
            .control
            .execute(
                Some(session.host_id()),
                AgentOp::Answer {
                    session_id: session.backend_session_id().to_owned(),
                    prompt_id: prompt_id.to_owned(),
                    screen_revision: revision.to_owned(),
                    answer: json!({ "optionIndex": option_index }),
                    request_id: Uuid::new_v4().to_string(),
                },
            )
            .await?;
        // Only the delivery receipt the host returned names an accepted request; a result
        // without one yields `None` rather than an id made up here.
        Ok(accepted
            .pointer("/receipt/requestId")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    async fn read_tail(&self, session: &SessionRef, lines: usize) -> Result<String> {
        let read = self
            .control
            .execute(
                Some(session.host_id()),
                AgentOp::Read {
                    session_id: session.backend_session_id().to_owned(),
                },
            )
            .await?;
        let text = read
            .get("session")
            .unwrap_or(&read)
            .pointer("/screen/text")
            .and_then(Value::as_str)
            .ok_or_else(|| HubError::message("the session read carries no screen text"))?;
        let all: Vec<&str> = text.lines().collect();
        Ok(all[all.len().saturating_sub(lines)..].join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;

    fn session() -> SessionRef {
        SessionRef::new("mac", "s1")
    }

    #[tokio::test]
    async fn an_answer_without_a_screen_revision_is_refused_before_any_op() {
        let recording = Arc::new(RecordingControl::default());
        let control = AgentSessionControl::new(recording.clone());
        for revision in [None, Some(""), Some("  ")] {
            let error = control
                .answer(&session(), "p1", revision, 0)
                .await
                .unwrap_err();
            assert!(
                error.to_string().starts_with(MISSING_SCREEN_REVISION),
                "{error}"
            );
        }
        assert!(recording.ops().is_empty(), "no AgentOp::Answer was sent");
    }

    #[tokio::test]
    async fn an_answer_with_its_revision_sends_one_answer_op_to_the_session_host() {
        let recording = Arc::new(RecordingControl::default());
        let control = AgentSessionControl::new(recording.clone());
        // The default recorded result carries no receipt: no accepted request id is reported.
        assert_eq!(
            control.answer(&session(), "p1", Some("r7"), 2).await.unwrap(),
            None
        );
        let ops = recording.ops();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0.as_deref(), Some("mac"));
        match &ops[0].1 {
            AgentOp::Answer {
                session_id,
                prompt_id,
                screen_revision,
                answer,
                ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(prompt_id, "p1");
                assert_eq!(screen_revision, "r7");
                assert_eq!(answer, &json!({"optionIndex": 2}));
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn an_accepted_answer_returns_the_request_id_of_its_receipt() {
        let recording = Arc::new(RecordingControl::default());
        *recording.result.lock().unwrap() =
            json!({"receipt": {"requestId": "sup-req-9", "stage": "accepted"}});
        let control = AgentSessionControl::new(recording.clone());
        assert_eq!(
            control.answer(&session(), "p1", Some("r7"), 0).await.unwrap(),
            Some("sup-req-9".to_owned())
        );
    }

    #[tokio::test]
    async fn stop_send_and_read_tail_use_the_matching_ops() {
        let recording = Arc::new(RecordingControl::default());
        *recording.result.lock().unwrap() =
            json!({"session": {"screen": {"text": "a\nb\nc\nd", "revision": "r1"}}});
        let control = AgentSessionControl::new(recording.clone());
        control.stop(&session()).await.unwrap();
        control.send(&session(), "go").await.unwrap();
        assert_eq!(control.read_tail(&session(), 2).await.unwrap(), "c\nd");
        assert_eq!(
            control.read_tail(&session(), 99).await.unwrap(),
            "a\nb\nc\nd"
        );
        let ops = recording.ops();
        assert!(matches!(&ops[0].1, AgentOp::Stop { session_id, .. } if session_id == "s1"));
        assert!(matches!(&ops[1].1, AgentOp::Send { text, .. } if text == "go"));
        assert!(matches!(&ops[2].1, AgentOp::Read { session_id } if session_id == "s1"));
    }

    /// A test gate that allows exactly the intended answer (option 0 of the fixture prompt).
    struct AllowFirstOption;

    #[async_trait]
    impl crate::meta::mhc_supervisor::AnswerGate for AllowFirstOption {
        async fn allow(
            &self,
            _session: &crate::supervisor::panel_contract::FleetSession,
            _prompt: &crate::supervisor::panel_contract::PendingPrompt,
            option_index: usize,
        ) -> std::result::Result<(), String> {
            (option_index == 0)
                .then_some(())
                .ok_or_else(|| "only option 0 is allowed here".to_owned())
        }
    }

    struct OneFleet;

    #[async_trait]
    impl crate::meta::mhc_supervisor::FleetSource for OneFleet {
        async fn list(&self) -> Result<Vec<crate::supervisor::panel_contract::FleetSession>> {
            Ok(vec![crate::supervisor::panel_contract::FleetSession {
                session_ref: session(),
                host_id: "mac".into(),
                host_label: "This Mac".into(),
                title: "task".into(),
                cwd: "/work".into(),
                agent: "claude".into(),
                state: "blocked".into(),
                state_source: "provider".into(),
                state_since_ms: None,
                screen_changed_at_ms: None,
                pending_prompt: Some(crate::supervisor::panel_contract::PendingPrompt {
                    prompt_id: "p1".into(),
                    kind: "approval".into(),
                    question: "Run the unit tests?".into(),
                    command: None,
                    options: vec!["Allow".into(), "Deny".into()],
                    screen_revision: None,
                }),
                last_assistant_line: None,
                updated_at_ms: 1,
                stale: false,
                health: None,
            }])
        }
    }

    #[tokio::test]
    async fn the_answer_tool_refuses_a_missing_revision_through_the_real_control() {
        let recording = Arc::new(RecordingControl::default());
        let tools = crate::meta::mhc_supervisor::SessionTools::new(
            Arc::new(AgentSessionControl::new(recording.clone())),
            Arc::new(OneFleet),
        )
        .with_gate(Arc::new(AllowFirstOption));
        let call =
            |revision: Option<&str>| crate::meta::mhc_supervisor::WorkToolCall::AnswerPrompt {
                session_ref: session(),
                prompt_id: "p1".into(),
                screen_revision: revision.map(str::to_owned),
                option_index: 0,
            };
        let refused = tools.execute(call(None)).await.unwrap_err();
        assert_eq!(refused.code, "SUPERVISOR_SESSION_FAILED");
        assert!(
            refused.message.contains(MISSING_SCREEN_REVISION),
            "{}",
            refused.message
        );
        assert!(recording.ops().is_empty(), "nothing reached agent control");
        tools.execute(call(Some("r1"))).await.unwrap();
        assert_eq!(recording.ops().len(), 1);
    }
}
