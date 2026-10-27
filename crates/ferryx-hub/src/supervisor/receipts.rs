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
//!   [`AgentControl::deliver`]. The target types nothing unless the bound pane was observed
//!   ready (its latest reading idle, with no prompt) right before the send. Its own observer
//!   then attributes the turn start: the reading must be of the bound pane, newer than a cursor
//!   taken before the send, and no other input may have reached the pane since (another
//!   request, a phone chat message, a person typing). Its run id names that reading and the
//!   target daemon's observer epoch ([`crate::agent_control::pty_run_id`]). The settle is the
//!   pane's next idle reading of the same epoch:
//!   - on this machine, recorded from the hub's observer by [`record_pty_settle`]. The observer
//!     notes every idle reading in an [`IdleLedger`] before it asks the store, and the
//!     dispatcher reads the ledger right after it commits the start, so an idle reading that
//!     arrives before the start is committed still settles the run;
//!   - on a paired host, from a bounded, re-armed agent-control `Wait` on that host
//!     ([`watch_paired_settle`]). When that end cannot be observed the run is reported
//!     [`PAIRED_SETTLE_UNOBSERVED`] and its goal is held, never left running unseen.
//! - [`MhcWorker`]: a native mhc session. The engine's reply to this request's own `prompt`
//!   command names its disposition: `started` is the start of this request's run; `queued` and
//!   `handled` were accepted without a start. The session's first `agent_settled` after its
//!   `agent_start` settles the run.
//!
//! A transport without a causal observer reports [`Delivery::Unsupported`] before sending.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};

use super::{record_receipt, Dispatch, Receipt, SupervisorError};
use crate::agent_control::{
    pty_run_start, AgentControl, AgentOp, Delivery, StartReceipt, WorkerSend,
    OPERATION_OUTCOME_UNKNOWN,
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
/// The end of a paired-host run could not be observed on that host; it may still be running.
pub const PAIRED_SETTLE_UNOBSERVED: &str = "PAIRED_SETTLE_UNOBSERVED";

/// What a transport reports about the end of a run it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettleReport {
    /// The run ended.
    Settled(StartReceipt),
    /// The run's end can no longer be observed (`code`, and `reason` in words). It may still
    /// be running, so the goal is held instead of being left `running` unseen.
    Unobserved {
        run: StartReceipt,
        code: String,
        reason: String,
    },
}

/// Where a transport reports the end of a run it started. Dropped unsent when the transport
/// leaves that end to another observer (a local PTY run: [`record_pty_settle`]) or the run's
/// session closed.
pub type SettleNotice = oneshot::Sender<SettleReport>;

/// One kind of worker the dispatcher can deliver an instruction to.
#[async_trait]
pub trait WorkerTransport: Send + Sync {
    /// Transmits `send` once and reports what the target observed. Never retries.
    async fn deliver(&self, send: &WorkerSend, settled: SettleNotice) -> Delivery;
}

/// How a paired-host run's end is watched: each host-side wait lasts at most `wait`; a failed
/// wait is retried after `backoff`, doubled each time; `failures` consecutive failures, or a
/// run longer than `max`, report it [`PAIRED_SETTLE_UNOBSERVED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleWatch {
    pub wait: Duration,
    pub backoff: Duration,
    pub failures: u32,
    pub max: Duration,
}

impl Default for SettleWatch {
    fn default() -> Self {
        Self {
            wait: Duration::from_secs(60),
            backoff: Duration::from_secs(1),
            failures: 5,
            max: Duration::from_secs(12 * 60 * 60),
        }
    }
}

/// Terminal agents on this machine or a paired host.
pub struct PtyWorker {
    control: Arc<dyn AgentControl>,
    watch: SettleWatch,
}

impl PtyWorker {
    pub fn new(control: Arc<dyn AgentControl>) -> Self {
        Self {
            control,
            watch: SettleWatch::default(),
        }
    }

    pub fn with_settle_watch(mut self, watch: SettleWatch) -> Self {
        self.watch = watch;
        self
    }
}

#[async_trait]
impl WorkerTransport for PtyWorker {
    async fn deliver(&self, send: &WorkerSend, settled: SettleNotice) -> Delivery {
        let delivery = self.control.deliver(send).await;
        // On this machine the settle is the hub observer's idle reading (`record_pty_settle`).
        // A paired host's observer is reached only through that host's own agent control.
        if let (Some(host_id), Delivery::Started(receipt)) = (&send.host_id, &delivery) {
            if receipt.names(send) {
                tokio::spawn(watch_paired_settle(
                    Arc::clone(&self.control),
                    host_id.clone(),
                    receipt.clone(),
                    self.watch,
                    settled,
                ));
            }
        }
        delivery
    }
}

/// What a paired host's `Wait` reply says about a run.
#[derive(Debug, PartialEq, Eq)]
enum PairedIdle {
    /// An idle reading of the run's pane, without a prompt, newer than its start reading.
    Settled,
    /// Nothing that ends the run; the host's log is at this sequence.
    Later(u64),
    /// The host's observer restarted since the run started: its sequences cannot be ordered.
    OtherEpoch,
    /// Not a reply this watcher can read.
    Unreadable,
}

fn paired_idle(reply: &Value, session_id: &str, instance: &str, start: u64) -> PairedIdle {
    match reply.get("instance").and_then(Value::as_str) {
        None => return PairedIdle::Unreadable,
        Some(epoch) if epoch != instance => return PairedIdle::OtherEpoch,
        Some(_) => {}
    }
    let ends = |reading: &Value| {
        reading.get("sessionId").and_then(Value::as_str) == Some(session_id)
            && reading.get("state").and_then(Value::as_str) == Some("idle")
            && reading.get("promptId").is_none_or(Value::is_null)
            && reading
                .get("sequence")
                .and_then(Value::as_u64)
                .is_some_and(|sequence| sequence > start)
    };
    let ended = match reply.get("observation") {
        Some(observation) => ends(observation),
        // A gap: the latest reading of every pane stands in for the readings the log lost.
        None => reply
            .get("snapshot")
            .and_then(Value::as_array)
            .is_some_and(|snapshot| snapshot.iter().any(ends)),
    };
    if ended {
        return PairedIdle::Settled;
    }
    match reply.get("afterSequence").and_then(Value::as_u64) {
        Some(sequence) => PairedIdle::Later(sequence),
        None => PairedIdle::Unreadable,
    }
}

/// Watches paired host `host_id` for the end of `run`, a PTY run it started: an idle reading
/// of the run's pane, without a prompt, newer than the run's start reading and of the same
/// observer epoch. Each host-side `Wait` (`until: idle`, after the newest sequence seen) is
/// bounded and re-armed; a wait that times out saw nothing newer. Reports
/// [`SettleReport::Settled`] at that reading, and [`SettleReport::Unobserved`] when the host's
/// observer restarted, its waits keep failing, or the run outlives `watch.max`. Stops without
/// a report once nobody listens (the start was not recorded).
pub async fn watch_paired_settle(
    control: Arc<dyn AgentControl>,
    host_id: String,
    run: StartReceipt,
    watch: SettleWatch,
    settled: SettleNotice,
) {
    let Some((instance, start)) =
        pty_run_start(&run.run_id, &run.session_id).map(|(instance, start)| (instance.to_owned(), start))
    else {
        let _ = settled.send(SettleReport::Unobserved {
            run,
            code: PAIRED_SETTLE_UNOBSERVED.to_owned(),
            reason: "the run id names no observer reading of the host".to_owned(),
        });
        return;
    };
    let until = tokio::time::Instant::now() + watch.max;
    let mut after = start;
    let mut failures = 0u32;
    let mut backoff = watch.backoff;
    let reason = loop {
        if settled.is_closed() {
            return;
        }
        let remaining = until.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break format!("the run did not end within {}s", watch.max.as_secs());
        }
        let slice = watch.wait.min(remaining);
        let op = AgentOp::Wait {
            session_id: run.session_id.clone(),
            until: "idle".to_owned(),
            after_revision: None,
            after,
            timeout_ms: u64::try_from(slice.as_millis()).unwrap_or(u64::MAX).max(1),
        };
        let failure = match control.execute(Some(&host_id), op).await {
            Ok(reply) => match paired_idle(&reply, &run.session_id, &instance, start) {
                PairedIdle::Settled => {
                    let _ = settled.send(SettleReport::Settled(run));
                    return;
                }
                PairedIdle::Later(sequence) if sequence > after => {
                    after = sequence;
                    failures = 0;
                    backoff = watch.backoff;
                    continue;
                }
                PairedIdle::Later(_) => "the host's wait made no progress".to_owned(),
                PairedIdle::OtherEpoch => {
                    break "the host's observer restarted after the run started; its end is reconciled with the host".to_owned()
                }
                PairedIdle::Unreadable => {
                    "the host answered the wait without a reading of the run's pane".to_owned()
                }
            },
            // A bounded wait that saw nothing newer: the run is still going; re-arm.
            Err(error) if error.to_string().split_whitespace().next() == Some("TIMEOUT") => {
                failures = 0;
                backoff = watch.backoff;
                continue;
            }
            Err(error) => error.to_string(),
        };
        failures += 1;
        if failures >= watch.failures {
            break format!("{failures} waits on host {host_id} failed, the last: {failure}");
        }
        tokio::time::sleep(backoff).await;
        backoff = backoff.saturating_mul(2);
    };
    let _ = settled.send(SettleReport::Unobserved {
        run,
        code: PAIRED_SETTLE_UNOBSERVED.to_owned(),
        reason,
    });
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
                let _ = settled.send(SettleReport::Settled(ended));
                return;
            }
            Some("session_closed") => return,
            _ => {}
        }
    }
}

/// The newest idle reading (no prompt) the hub's observer saw per pane on this machine, with
/// its observer epoch. The observer notes a reading here before it asks the store to settle
/// it ([`record_pty_settle`]); the dispatcher reads it right after it commits a start
/// ([`IdleLedger::idle_after`]). Whichever of the two runs second sees the other's write, so
/// an idle reading that arrives before its run's start is committed still settles the run.
#[derive(Debug, Default)]
pub struct IdleLedger {
    seen: Mutex<HashMap<String, (String, u64)>>,
}

impl IdleLedger {
    /// Notes an idle reading without a prompt of `session_id`.
    pub fn note_idle(&self, session_id: &str, instance: &str, sequence: u64) {
        let mut seen = self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let newest = seen
            .entry(session_id.to_owned())
            .or_insert_with(|| (instance.to_owned(), sequence));
        if newest.0 == instance {
            newest.1 = newest.1.max(sequence);
        } else {
            *newest = (instance.to_owned(), sequence);
        }
    }

    /// The newest idle reading noted for the pane `run_id` started on, as (epoch, sequence),
    /// when it is of the run's epoch and newer than the run's start reading.
    pub fn idle_after(&self, session_id: &str, run_id: &str) -> Option<(String, u64)> {
        let (instance, start) = pty_run_start(run_id, session_id)?;
        let seen = self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (epoch, newest) = seen.get(session_id)?;
        (epoch.as_str() == instance && *newest > start).then(|| (epoch.clone(), *newest))
    }
}

/// What one idle reading did to the PTY run bound to its pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtySettle {
    /// The reading ended the live run: its settled dispatch.
    Settled(Box<Dispatch>),
    /// No started run is bound to the pane, or the reading is not newer than its start.
    Ignored,
    /// The live run started under another observer epoch (the daemon restarted since), so the
    /// two sequences cannot be ordered. Nothing is recorded; restart reconciliation decides.
    OtherEpoch { request_id: String, run_id: String },
}

/// Records the settle of the live PTY run bound to `session_id` on `host_id`, from an
/// observer reading of that pane going idle (`instance` is the observer epoch of `sequence`).
/// Only a reading of the run's epoch newer than the one that started it settles it
/// ([`pty_run_start`]).
pub async fn record_pty_settle(
    pool: &SqlitePool,
    host_id: &str,
    session_id: &str,
    instance: &str,
    sequence: u64,
) -> Result<PtySettle, SupervisorError> {
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
        return Ok(PtySettle::Ignored);
    };
    let started = pty_run_start(&run_id, session_id)
        .filter(|(epoch, _)| *epoch == instance)
        .map(|(_, started)| started);
    let Some(started) = started else {
        return Ok(PtySettle::OtherEpoch { request_id, run_id });
    };
    if sequence <= started {
        return Ok(PtySettle::Ignored);
    }
    let settled = Receipt::Settled {
        run_id,
        provider_session_id: session_id.to_owned(),
    };
    record_receipt(pool, &request_id, generation, &settled)
        .await
        .map(|dispatch| PtySettle::Settled(Box::new(dispatch)))
}
