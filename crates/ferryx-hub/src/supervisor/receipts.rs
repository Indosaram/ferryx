//! Causal worker receipts for supervisor dispatches.
//!
//! A dispatch is transmitted once by one [`WorkerTransport`] and recorded stage by stage, each
//! as its own fact ([`record_receipt`]):
//! - accepted: the target took the bytes;
//! - started: the target attributes a run to exactly this request id on the bound session.
//!   The dispatcher checks [`StartReceipt::names`] before recording it, so a receipt for
//!   another request or session never starts this run;
//! - settled: the run the target named ended;
//! - uncertain: none of that can be shown. It is never resent.
//!
//! Transports:
//! - [`PtyWorker`]: a terminal agent on this machine or a paired host, through
//!   [`AgentControl::deliver`]. The target's own observer attributes the turn start: the
//!   reading must be of the bound pane, newer than a cursor taken before the send, and no other
//!   input may have reached the pane since (another request, a phone chat message, a person
//!   typing). Its run id names that reading ([`crate::agent_control::pty_run_id`]). The settle
//!   is the pane's next idle reading, recorded from the hub's observer by
//!   [`record_pty_settle`].
//! - [`MhcWorker`]: a native mhc session. The engine's reply to this request's own `prompt`
//!   command names its disposition: `started` is the start of this request's run; `queued` and
//!   `handled` were accepted without a start. The session's first `agent_settled` after its
//!   `agent_start` settles the run.
//!
//! A transport without a causal observer reports [`Delivery::Unsupported`] before sending.
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};

use super::{record_receipt, Dispatch, Receipt, SupervisorError};
use crate::agent_control::{
    pty_run_sequence, AgentControl, Delivery, StartReceipt, WorkerSend, OPERATION_OUTCOME_UNKNOWN,
};
use crate::meta::mhc_stdio::{RequestError, StdioClient};

/// No mhc worker engine is reachable from this dispatcher; nothing was sent.
pub const MHC_WORKER_UNAVAILABLE: &str = "MHC_WORKER_UNAVAILABLE";
/// The engine queued the prompt behind a running turn: accepted, its start not observed.
pub const MHC_PROMPT_QUEUED: &str = "MHC_PROMPT_QUEUED";
/// The engine handled the prompt without starting a turn.
pub const MHC_PROMPT_HANDLED: &str = "MHC_PROMPT_HANDLED";
/// The engine refused the prompt without an error code.
pub const MHC_PROMPT_REFUSED: &str = "MHC_PROMPT_REFUSED";
/// The engine transport ended while the prompt was outstanding.
pub const MHC_TRANSPORT_LOST: &str = "MHC_TRANSPORT_LOST";

/// Where a transport reports the end of a run it started. Dropped unsent when the transport
/// cannot observe that end.
pub type SettleNotice = oneshot::Sender<StartReceipt>;

/// One kind of worker the dispatcher can deliver an instruction to.
#[async_trait]
pub trait WorkerTransport: Send + Sync {
    /// Transmits `send` once and reports what the target observed. Never retries.
    async fn deliver(&self, send: &WorkerSend, settled: SettleNotice) -> Delivery;
}

/// Terminal agents on this machine or a paired host.
pub struct PtyWorker(pub Arc<dyn AgentControl>);

#[async_trait]
impl WorkerTransport for PtyWorker {
    async fn deliver(&self, send: &WorkerSend, _settled: SettleNotice) -> Delivery {
        // A PTY settle is an observer reading on the hub: see `record_pty_settle`.
        self.0.deliver(send).await
    }
}

/// The engine that holds mhc worker sessions: its current stdio transport.
#[async_trait]
pub trait MhcEngine: Send + Sync {
    async fn client(&self) -> Result<Arc<StdioClient>, String>;
}

/// An engine reached through one fixed transport.
pub struct StdioEngine(pub Arc<StdioClient>);

#[async_trait]
impl MhcEngine for StdioEngine {
    async fn client(&self) -> Result<Arc<StdioClient>, String> {
        if self.0.is_gone() {
            return Err(self.0.gone_reason().unwrap_or_else(|| "engine transport ended".into()));
        }
        Ok(Arc::clone(&self.0))
    }
}

/// Run id of an mhc turn the engine started for one request on one session.
pub fn mhc_run_id(session_id: &str, request_id: &str) -> String {
    format!("mhc:{session_id}:{request_id}")
}

/// Native mhc worker sessions. The bound provider session id is the engine routing handle the
/// worker session was opened under.
pub struct MhcWorker {
    engine: Arc<dyn MhcEngine>,
    /// How long the engine has to answer the prompt command. No answer is an unknown outcome.
    request_deadline: Duration,
}

impl MhcWorker {
    pub fn new(engine: Arc<dyn MhcEngine>, request_deadline: Duration) -> Self {
        Self {
            engine,
            request_deadline,
        }
    }
}

#[async_trait]
impl WorkerTransport for MhcWorker {
    async fn deliver(&self, send: &WorkerSend, settled: SettleNotice) -> Delivery {
        let unavailable = || Delivery::Unsupported {
            code: MHC_WORKER_UNAVAILABLE.to_owned(),
        };
        // Worker sessions live on this hub's own engine; another host has no route to them.
        if send.host_id.is_some() {
            return unavailable();
        }
        let Ok(client) = self.engine.client().await else {
            return unavailable();
        };
        // Subscribe before the prompt: the run's events can never arrive before the receiver.
        let Ok(events) = client.subscribe(&send.session_id) else {
            return unavailable();
        };
        let command = json!({
            "type": "prompt",
            "sessionId": send.session_id,
            "message": send.text,
        });
        let data = match client.request(command, self.request_deadline).await {
            Ok(data) => data,
            Err(RequestError::Refused { code, .. }) => {
                return Delivery::Refused {
                    code: code
                        .map(|code| format!("MHC_{}", code.to_ascii_uppercase()))
                        .unwrap_or_else(|| MHC_PROMPT_REFUSED.to_owned()),
                }
            }
            Err(RequestError::Timeout) => {
                return Delivery::Uncertain {
                    accepted: false,
                    code: OPERATION_OUTCOME_UNKNOWN.to_owned(),
                }
            }
            Err(RequestError::Gone(_)) => {
                return Delivery::Uncertain {
                    accepted: false,
                    code: MHC_TRANSPORT_LOST.to_owned(),
                }
            }
        };
        // The engine answered this request's own command, so its disposition is causal. The
        // receiver is dropped on every other path; its route is replaced by the next
        // subscriber of the session and is never removed here, since that would also remove
        // a newer subscriber's route.
        match data.get("disposition").and_then(Value::as_str) {
            Some("started") => {
                let receipt = StartReceipt {
                    request_id: send.request_id.clone(),
                    session_id: send.session_id.clone(),
                    run_id: mhc_run_id(&send.session_id, &send.request_id),
                };
                let ended = receipt.clone();
                tokio::spawn(settle_after_start(events, ended, settled));
                Delivery::Started(receipt)
            }
            Some("queued") => Delivery::Uncertain {
                accepted: true,
                code: MHC_PROMPT_QUEUED.to_owned(),
            },
            _ => Delivery::Uncertain {
                accepted: true,
                code: MHC_PROMPT_HANDLED.to_owned(),
            },
        }
    }
}

/// Reports `ended` on `settled` at the session's first `agent_settled` after an
/// `agent_start`. A settle before the start belongs to an earlier turn and never ends this
/// run; a closed session or transport drops the notice unsent.
pub(crate) async fn settle_after_start(
    mut events: mpsc::UnboundedReceiver<Value>,
    ended: StartReceipt,
    settled: SettleNotice,
) {
    let mut began = false;
    while let Some(event) = events.recv().await {
        match event.get("type").and_then(Value::as_str) {
            Some("agent_start") => began = true,
            Some("agent_settled") if began => {
                let _ = settled.send(ended);
                return;
            }
            Some("session_closed") => return,
            _ => {}
        }
    }
}

/// Records the settle of the live PTY run bound to `session_id` on `host_id`, from an
/// observer reading of that pane going idle. Only a reading newer than the one that started
/// the run settles it ([`pty_run_sequence`]); anything else, or a pane no started run is bound
/// to, records nothing and returns `None`.
pub async fn record_pty_settle(
    pool: &SqlitePool,
    host_id: &str,
    session_id: &str,
    sequence: u64,
) -> Result<Option<Dispatch>, SupervisorError> {
    let live: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT d.request_id, d.generation, d.run_id FROM supervisor_dispatches d
           JOIN supervisor_execution_bindings b
             ON b.goal_id = d.goal_id AND b.generation = d.generation
          WHERE b.kind = 'pty' AND b.host_id = ? AND b.provider_session_id = ?
            AND d.state = 'started' AND d.run_id = b.run_id
          ORDER BY d.id DESC LIMIT 1",
    )
    .bind(host_id)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    let Some((request_id, generation, run_id)) = live else {
        return Ok(None);
    };
    match pty_run_sequence(&run_id, session_id) {
        Some(started) if sequence > started => {}
        _ => return Ok(None),
    }
    let settled = Receipt::Settled {
        run_id,
        provider_session_id: session_id.to_owned(),
    };
    record_receipt(pool, &request_id, generation, &settled)
        .await
        .map(Some)
}
