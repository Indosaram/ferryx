use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{HubError, Result};
use crate::goals::types::SendOutcome;

/// One agent-control operation. The daemon adapter turns this into `AgentControlOp`.
///
/// Mutations carry the caller's request id through unchanged. The hub never mints a
/// replacement id after a dropped answer, so an outcome-unknown reply is not retried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum AgentOp {
    List,
    Read {
        session_id: String,
    },
    Send {
        session_id: String,
        text: String,
        request_id: String,
    },
    Answer {
        session_id: String,
        prompt_id: String,
        screen_revision: String,
        answer: Value,
        request_id: String,
    },
    Stop {
        session_id: String,
        request_id: String,
    },
    Wait {
        session_id: String,
        until: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_revision: Option<String>,
        #[serde(default, skip_serializing_if = "is_zero")]
        after: u64,
        timeout_ms: u64,
    },
    Events {
        #[serde(default, skip_serializing_if = "is_zero")]
        after: u64,
    },
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl AgentOp {
    pub fn is_mutation(&self) -> bool {
        matches!(
            self,
            Self::Send { .. } | Self::Answer { .. } | Self::Stop { .. }
        )
    }
}

/// Backend the hub uses for every host, including this machine (`host_id` none or `"local"`).
#[async_trait]
pub trait AgentControl: Send + Sync {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value>;

    /// Sends `text` once and reports whether the turn started. Never retries.
    ///
    /// This default cannot observe the pane, so an accepted send counts as `Started`. It
    /// serves fakes; the daemon bridge overrides it with a turn-start observation.
    async fn send_confirmed(
        &self,
        host_id: Option<&str>,
        session_id: &str,
        text: &str,
        request_id: &str,
    ) -> SendOutcome {
        let op = AgentOp::Send {
            session_id: session_id.to_owned(),
            text: text.to_owned(),
            request_id: request_id.to_owned(),
        };
        match self.execute(host_id, op).await {
            Ok(_) => SendOutcome::Started,
            Err(error) => send_error_outcome(&error.to_string()),
        }
    }

    /// Sends one supervisor dispatch once and reports what the target observed, stage by
    /// stage. Only a target-side receipt naming this request and session counts as a start;
    /// acceptance alone never does. Never retries.
    ///
    /// This default has no causal observer, so it sends nothing and reports
    /// [`OBSERVER_CAPABILITY_MISSING`]: a backend that cannot attribute a start to a request
    /// is a capability hold, never assumed success. The daemon bridge overrides it.
    async fn deliver(&self, _send: &WorkerSend) -> Delivery {
        Delivery::Unsupported {
            code: OBSERVER_CAPABILITY_MISSING.to_owned(),
        }
    }
}

/// Wire code of an unknown send outcome: the bytes may or may not have reached the pane.
pub const OPERATION_OUTCOME_UNKNOWN: &str = "OPERATION_OUTCOME_UNKNOWN";

/// Host id of this machine, in execution bindings, dispatcher wiring and PTY settles alike.
pub const LOCAL_HOST_ID: &str = "local";

/// The backend has no observer that can attribute a turn start to one request; nothing was
/// sent.
pub const OBSERVER_CAPABILITY_MISSING: &str = "OBSERVER_CAPABILITY_MISSING";

/// A paired host that does not advertise causal delivery receipts; nothing was sent to it.
pub const PAIRED_HOST_CAPABILITY_UNAVAILABLE: &str = "PAIRED_HOST_CAPABILITY_UNAVAILABLE";

/// A transport failure without a wire code: whether the bytes left is unknown.
pub const SEND_FAILED: &str = "SEND_FAILED";

/// A start receipt that names another request or session than the dispatch it answers.
pub const RECEIPT_MISMATCH: &str = "RECEIPT_MISMATCH";

/// The bound pane was not observed ready (its latest reading idle, with no prompt) before the
/// send, so a start could not be attributed to it; nothing was typed.
pub const PANE_NOT_READY: &str = "PANE_NOT_READY";

/// The executing daemon refused a supervisor request whose lease fence is below the highest
/// it has seen for the account, or the sending machine does not hold the supervisor lease
/// now. Decided before anything runs: nothing was typed.
pub const STALE_SUPERVISOR_FENCE: &str = "STALE_SUPERVISOR_FENCE";

/// A higher supervisor fence could not be stored by the executing daemon, so the request was
/// not executed: nothing was typed.
pub const SUPERVISOR_FENCE_UNAVAILABLE: &str = "SUPERVISOR_FENCE_UNAVAILABLE";

/// Refusal codes that prove nothing was typed: each is decided before the first byte is
/// written. Only these release a dispatch; any other failure code may follow a write.
pub const PRE_WRITE_REFUSALS: &[&str] = &[
    "CONTROL_CONFLICT",
    "INVALID_REQUEST",
    "NOT_FOUND",
    // Decided before typing: a multi-line message whose paste mode cannot be read, or a
    // paired host without the capability (forwarded as UNSUPPORTED).
    "UNSUPPORTED",
    PANE_NOT_READY,
    STALE_SUPERVISOR_FENCE,
    SUPERVISOR_FENCE_UNAVAILABLE,
];

/// One supervisor dispatch to a bound worker: the stable request id, transmitted once, the
/// bound provider session and the host mapping it is bound on (`None` is this machine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSend {
    pub host_id: Option<String>,
    pub session_id: String,
    pub text: String,
    pub request_id: String,
}

/// A target-side start receipt: the run the target attributes to one request on one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartReceipt {
    pub request_id: String,
    pub session_id: String,
    pub run_id: String,
}

impl StartReceipt {
    /// Whether this receipt names exactly this dispatch: its request id and bound session.
    pub fn names(&self, send: &WorkerSend) -> bool {
        self.request_id == send.request_id && self.session_id == send.session_id
    }
}

/// What one [`AgentControl::deliver`] learned. Accepted and started are separate facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Nothing was sent: the target cannot report a causal start (a capability hold).
    Unsupported { code: String },
    /// Nothing was typed; the target refused the send.
    Refused { code: String },
    /// The target accepted the bytes and attributes this run to the request. The caller still
    /// checks [`StartReceipt::names`] before recording a start.
    Started(StartReceipt),
    /// No start can be attributed to the request. `accepted` when the target confirmed it
    /// took the bytes. Never resend.
    Uncertain { accepted: bool, code: String },
}

impl Delivery {
    /// The delivery a failed transport call reports, from its wire code. An unknown outcome
    /// and a failure without a code are uncertain; a paired host without the receipt
    /// capability, or a target without an observer, is a capability hold; a code that proves
    /// nothing was typed ([`PRE_WRITE_REFUSALS`]) is a refusal. Every other code (an internal
    /// error after the host may have executed, a timeout, an unknown code) is uncertain: a
    /// dispatch is released only when nothing can have been typed.
    pub fn from_error_code(code: Option<&str>) -> Self {
        match code.map(str::trim).filter(|code| !code.is_empty()) {
            None => Delivery::Uncertain {
                accepted: false,
                code: SEND_FAILED.to_owned(),
            },
            Some(code) if code.contains(OPERATION_OUTCOME_UNKNOWN) => Delivery::Uncertain {
                accepted: false,
                code: OPERATION_OUTCOME_UNKNOWN.to_owned(),
            },
            Some(code @ (PAIRED_HOST_CAPABILITY_UNAVAILABLE | crate::goals::loop_driver::OBSERVER_UNAVAILABLE)) => {
                Delivery::Unsupported {
                    code: code.to_owned(),
                }
            }
            Some(code) if PRE_WRITE_REFUSALS.contains(&code) => Delivery::Refused {
                code: code.to_owned(),
            },
            Some(code) => Delivery::Uncertain {
                accepted: false,
                code: code.to_owned(),
            },
        }
    }

    /// The legacy confirmed-send outcome of this delivery. A start counts only when its
    /// receipt names `send`; a capability hold typed nothing.
    pub fn send_outcome(&self, send: &WorkerSend) -> SendOutcome {
        match self {
            Delivery::Started(receipt) if receipt.names(send) => SendOutcome::Started,
            Delivery::Started(_) => SendOutcome::Uncertain {
                code: RECEIPT_MISMATCH.to_owned(),
            },
            Delivery::Uncertain { code, .. } => SendOutcome::Uncertain { code: code.clone() },
            Delivery::Refused { code } | Delivery::Unsupported { code } => {
                SendOutcome::Refused { code: code.clone() }
            }
        }
    }
}

/// Run id of a PTY turn: the pane's session, the observer epoch of the target daemon (its
/// sequences restart with the daemon) and the sequence of the reading that started the turn.
/// The target mints it; the hub reads the epoch and sequence back to order a settle.
pub fn pty_run_id(session_id: &str, instance: &str, sequence: u64) -> String {
    format!("pty:{session_id}:{instance}:{sequence}")
}

/// The observer epoch and start sequence of a [`pty_run_id`] minted for `session_id`. A
/// sequence is only comparable with readings of the same epoch.
pub fn pty_run_start<'a>(run_id: &'a str, session_id: &str) -> Option<(&'a str, u64)> {
    let rest = run_id.strip_prefix("pty:")?;
    let (rest, sequence) = rest.rsplit_once(':')?;
    let (session, instance) = rest.rsplit_once(':')?;
    if session != session_id || instance.is_empty() {
        return None;
    }
    Some((instance, sequence.parse().ok()?))
}

/// Maps a failed send's error text (`"{CODE} {message}"` from the daemon bridge) to an
/// outcome. An unknown outcome is uncertain; every other failure typed nothing.
pub fn send_error_outcome(message: &str) -> SendOutcome {
    if message.contains(OPERATION_OUTCOME_UNKNOWN) {
        return SendOutcome::Uncertain {
            code: OPERATION_OUTCOME_UNKNOWN.to_owned(),
        };
    }
    let first = message.split_whitespace().next().unwrap_or("");
    let is_code = !first.is_empty()
        && first
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
    SendOutcome::Refused {
        code: if is_code {
            first.to_owned()
        } else {
            "SEND_FAILED".to_owned()
        },
    }
}

/// In-memory backend for hub tests and for a host the fleet should treat as failed or hung.
#[derive(Debug)]
pub struct RecordingControl {
    pub ops: Mutex<Vec<(Option<String>, AgentOp)>>,
    pub result: Mutex<Value>,
    pub hang: AtomicBool,
    pub fail_hosts: Mutex<HashSet<String>>,
    /// What [`AgentControl::deliver`] reports. `None` (the default) is the trait default: no
    /// causal observer, nothing recorded or sent.
    pub receipts: Mutex<Option<ReceiptScript>>,
}

/// How a [`RecordingControl`] target answers a delivery after recording its send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptScript {
    /// The target attributes a new run to exactly the request and session it was sent.
    Echo,
    /// The target answers with this delivery, whatever was sent.
    Fixed(Delivery),
}

impl Default for RecordingControl {
    fn default() -> Self {
        Self {
            ops: Mutex::new(Vec::new()),
            result: Mutex::new(json!({"ok": true})),
            hang: AtomicBool::new(false),
            fail_hosts: Mutex::new(HashSet::new()),
            receipts: Mutex::new(None),
        }
    }
}

impl RecordingControl {
    /// The observer epoch of every run id this target mints.
    pub const EPOCH: &'static str = "rc0";

    pub fn ops(&self) -> Vec<(Option<String>, AgentOp)> {
        self.ops.lock().expect("ops").clone()
    }

    /// A target that reports a causal start receipt for every delivery it records.
    pub fn observing() -> Self {
        Self::scripted(ReceiptScript::Echo)
    }

    pub fn scripted(script: ReceiptScript) -> Self {
        let control = Self::default();
        *control.receipts.lock().expect("receipts") = Some(script);
        control
    }
}

#[async_trait]
impl AgentControl for RecordingControl {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value> {
        if self.hang.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if let Some(host) = host_id {
            if self.fail_hosts.lock().expect("fail").contains(host) {
                return Err(HubError::message(format!("host {host} failed")));
            }
        }
        self.ops
            .lock()
            .expect("ops")
            .push((host_id.map(str::to_owned), op));
        Ok(self.result.lock().expect("result").clone())
    }

    async fn deliver(&self, send: &WorkerSend) -> Delivery {
        let script = self.receipts.lock().expect("receipts").clone();
        let Some(script) = script else {
            return Delivery::Unsupported {
                code: OBSERVER_CAPABILITY_MISSING.to_owned(),
            };
        };
        // A target that cannot observe, or that refuses, types nothing: nothing is recorded.
        if let ReceiptScript::Fixed(
            delivery @ (Delivery::Unsupported { .. } | Delivery::Refused { .. }),
        ) = script
        {
            return delivery;
        }
        let op = AgentOp::Send {
            session_id: send.session_id.clone(),
            text: send.text.clone(),
            request_id: send.request_id.clone(),
        };
        if let Err(error) = self.execute(send.host_id.as_deref(), op).await {
            return Delivery::from_error_code(error.to_string().split_whitespace().next());
        }
        match script {
            ReceiptScript::Echo => {
                let sequence = self.ops.lock().expect("ops").len() as u64;
                Delivery::Started(StartReceipt {
                    request_id: send.request_id.clone(),
                    session_id: send.session_id.clone(),
                    run_id: pty_run_id(&send.session_id, Self::EPOCH, sequence),
                })
            }
            ReceiptScript::Fixed(delivery) => delivery,
        }
    }
}

/// A pane opened on a paired host: its host-side session id and working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePane {
    pub session_id: String,
    pub cwd: String,
}

/// Launches a pane and returns its daemon session id. The lane runner uses this
/// instead of the gateway's SSH/tmux runner.
#[async_trait]
pub trait TaskLauncher: Send + Sync {
    async fn launch_pane(&self, cwd: &str, command: &[String]) -> Result<String>;

    /// Paired hosts that can take a lane now (machine grant, paired, online).
    async fn remote_hosts(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Opens a lane pane on `host_id` in a new worktree of the project whose git remote
    /// matches the repository at the local `cwd`.
    async fn launch_remote_pane(
        &self,
        host_id: &str,
        _cwd: &str,
        _lane_key: &str,
    ) -> Result<RemotePane> {
        Err(HubError::message(format!(
            "this launcher cannot open a pane on {host_id}"
        )))
    }
}

#[derive(Debug, Default)]
pub struct FixedLauncher {
    pub session_id: Mutex<String>,
    pub launched: Mutex<Vec<(String, Vec<String>)>>,
}

impl FixedLauncher {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Mutex::new(session_id.into()),
            launched: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl TaskLauncher for FixedLauncher {
    async fn launch_pane(&self, cwd: &str, command: &[String]) -> Result<String> {
        self.launched
            .lock()
            .expect("launched")
            .push((cwd.to_owned(), command.to_vec()));
        Ok(self.session_id.lock().expect("session").clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend whose every operation fails with one error text.
    struct FailingControl(&'static str);

    #[async_trait]
    impl AgentControl for FailingControl {
        async fn execute(&self, _host_id: Option<&str>, _op: AgentOp) -> Result<Value> {
            Err(HubError::message(self.0))
        }
    }

    #[tokio::test]
    async fn the_default_confirmed_send_maps_an_accepted_send_to_started() {
        let control = RecordingControl::default();
        let outcome = control
            .send_confirmed(None, "pane-1", "next step", "hub-goal-lane-a-1")
            .await;
        assert_eq!(outcome, SendOutcome::Started);
        assert_eq!(
            control.ops(),
            vec![(
                None,
                AgentOp::Send {
                    session_id: "pane-1".into(),
                    text: "next step".into(),
                    request_id: "hub-goal-lane-a-1".into(),
                }
            )]
        );
    }

    #[tokio::test]
    async fn the_default_confirmed_send_maps_failures_without_retrying() {
        let unknown = FailingControl("OPERATION_OUTCOME_UNKNOWN the writer did not answer");
        assert_eq!(
            unknown
                .send_confirmed(None, "pane-1", "x", "hub-goal-a-1")
                .await,
            SendOutcome::Uncertain {
                code: OPERATION_OUTCOME_UNKNOWN.into()
            }
        );
        let typing = FailingControl("CONTROL_CONFLICT a person typed in this pane");
        assert_eq!(
            typing
                .send_confirmed(None, "pane-1", "x", "hub-goal-a-1")
                .await,
            SendOutcome::Refused {
                code: "CONTROL_CONFLICT".into()
            }
        );
        let plain = FailingControl("host remote failed");
        assert_eq!(
            plain
                .send_confirmed(None, "pane-1", "x", "hub-goal-a-1")
                .await,
            SendOutcome::Refused {
                code: "SEND_FAILED".into()
            }
        );
    }

    fn send() -> WorkerSend {
        WorkerSend {
            host_id: None,
            session_id: "pane-1".into(),
            text: "next step".into(),
            request_id: "sv-goal-r1-d1".into(),
        }
    }

    #[tokio::test]
    async fn the_default_delivery_sends_nothing_and_legacy_sends_are_unchanged() {
        let control = RecordingControl::default();
        assert_eq!(
            control.deliver(&send()).await,
            Delivery::Unsupported {
                code: OBSERVER_CAPABILITY_MISSING.into()
            }
        );
        assert!(control.ops().is_empty(), "no observer, no transmission");
        // The legacy confirmed send keeps its accepted-is-started default for its callers.
        assert_eq!(
            control
                .send_confirmed(None, "pane-1", "next step", "hub-goal-lane-a-1")
                .await,
            SendOutcome::Started
        );
        assert_eq!(control.ops().len(), 1);
    }

    #[test]
    fn only_a_receipt_naming_the_request_and_session_is_a_start() {
        let send = send();
        let receipt = |request_id: &str, session_id: &str| {
            Delivery::Started(StartReceipt {
                request_id: request_id.into(),
                session_id: session_id.into(),
                run_id: pty_run_id(session_id, "e1", 4),
            })
        };
        assert_eq!(
            receipt("sv-goal-r1-d1", "pane-1").send_outcome(&send),
            SendOutcome::Started
        );
        for wrong in [receipt("sv-goal-r1-d2", "pane-1"), receipt("sv-goal-r1-d1", "pane-2")] {
            assert_eq!(
                wrong.send_outcome(&send),
                SendOutcome::Uncertain {
                    code: RECEIPT_MISMATCH.into()
                }
            );
        }
        let accepted = Delivery::Uncertain {
            accepted: true,
            code: "TURN_NOT_OBSERVED".into(),
        };
        assert_eq!(
            accepted.send_outcome(&send),
            SendOutcome::Uncertain {
                code: "TURN_NOT_OBSERVED".into()
            }
        );
    }

    #[test]
    fn transport_error_codes_map_to_delivery_stages() {
        assert_eq!(
            Delivery::from_error_code(Some(OPERATION_OUTCOME_UNKNOWN)),
            Delivery::Uncertain {
                accepted: false,
                code: OPERATION_OUTCOME_UNKNOWN.into()
            }
        );
        assert_eq!(
            Delivery::from_error_code(None),
            Delivery::Uncertain {
                accepted: false,
                code: SEND_FAILED.into()
            }
        );
        assert_eq!(
            Delivery::from_error_code(Some(PAIRED_HOST_CAPABILITY_UNAVAILABLE)),
            Delivery::Unsupported {
                code: PAIRED_HOST_CAPABILITY_UNAVAILABLE.into()
            }
        );
        assert_eq!(
            Delivery::from_error_code(Some("CONTROL_CONFLICT")),
            Delivery::Refused {
                code: "CONTROL_CONFLICT".into()
            }
        );
        assert_eq!(
            Delivery::from_error_code(Some(crate::goals::loop_driver::OBSERVER_UNAVAILABLE)),
            Delivery::Unsupported {
                code: crate::goals::loop_driver::OBSERVER_UNAVAILABLE.into()
            }
        );
        for code in ["INVALID_REQUEST", "NOT_FOUND", "UNSUPPORTED", PANE_NOT_READY] {
            assert_eq!(
                Delivery::from_error_code(Some(code)),
                Delivery::Refused { code: code.into() },
                "{code} is decided before the first byte"
            );
        }
        // A code that can follow a write is never a refusal: the row is not released.
        for code in ["INTERNAL", "TIMEOUT", "REQUEST_CONFLICT", "HOST_UNAVAILABLE"] {
            assert_eq!(
                Delivery::from_error_code(Some(code)),
                Delivery::Uncertain {
                    accepted: false,
                    code: code.into()
                },
                "{code} may follow a write"
            );
        }
    }

    #[test]
    fn a_pty_run_id_names_its_session_epoch_and_start_reading() {
        let run = pty_run_id("sess:a", "e7", 42);
        assert_eq!(run, "pty:sess:a:e7:42");
        assert_eq!(pty_run_start(&run, "sess:a"), Some(("e7", 42)));
        assert_eq!(pty_run_start(&run, "sess"), None);
        assert_eq!(pty_run_start("sv-goal-r1-d1", "sess:a"), None);
        // A run id without an epoch cannot be ordered against any reading.
        assert_eq!(pty_run_start("pty:sess:42", "sess"), None);
    }
}
