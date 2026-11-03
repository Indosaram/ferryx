//! Opt-in endpoint trace. Records contain metadata only, never packet payloads.
use serde::Serialize;
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

pub const RECEIVER_TRACE_CAPACITY: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReceiverTraceEvent {
    SocketBuffer,
    Receive,
    ReceiveError,
    Authenticated,
    AuthRejected,
    GapDiscovered,
    GapGrace,
    GapEvicted,
    LateOutsidePending,
    AssemblyComplete,
    AssemblyInvalid,
    AssemblyTimeout,
    AssemblyCapacity,
    AssemblyKeyframe,
    QueueAdmission,
    QueueLate,
    QueueOverflow,
    QueueDiscontinuity,
    QueueDependent,
    QueueRecovered,
    DecodeFailed,
}

/// `sequence` is untrusted for Receive/AuthRejected; frame IDs exist only after auth.
/// Gap records encode a wrapping sequence range of `count` positions. No kind/frame
/// is inferred for a missing position. `result` is a stable code or OS errno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReceiverTraceRecord {
    pub local_ns: u64,
    pub event: ReceiverTraceEvent,
    pub sequence: Option<u32>,
    pub kind: Option<u8>,
    pub bytes: u64,
    pub frame: Option<u32>,
    pub count: u64,
    pub result: i32,
}

impl ReceiverTraceRecord {
    pub(crate) const fn new(event: ReceiverTraceEvent) -> Self {
        Self {
            local_ns: 0,
            event,
            sequence: None,
            kind: None,
            bytes: 0,
            frame: None,
            count: 1,
            result: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReceiverTraceSnapshot {
    pub version: u32,
    pub session: Option<[u8; 16]>,
    pub records: Vec<ReceiverTraceRecord>,
    pub overflow: u64,
}

#[derive(Debug)]
struct TraceState {
    origin: Instant,
    path: PathBuf,
    snapshot: ReceiverTraceSnapshot,
}

/// Clones share one endpoint capacity; disabled handles allocate nothing.
#[derive(Debug, Clone, Default)]
pub struct ReceiverTrace(Option<Arc<Mutex<TraceState>>>);

impl ReceiverTrace {
    pub(crate) fn enabled(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn begin_session(&self, salt: [u8; 16]) {
        if let Some(state) = &self.0 {
            let mut state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.origin = Instant::now();
            state.snapshot.session = Some(salt);
            state.snapshot.records.clear();
            state.snapshot.overflow = 0;
        }
    }
    pub fn at_path(path: PathBuf) -> Self {
        Self(Some(Arc::new(Mutex::new(TraceState {
            origin: Instant::now(),
            path,
            snapshot: ReceiverTraceSnapshot {
                version: 1,
                session: None,
                records: Vec::with_capacity(RECEIVER_TRACE_CAPACITY),
                overflow: 0,
            },
        }))))
    }

    pub(crate) fn record(&self, at: Instant, mut record: ReceiverTraceRecord) {
        if let Some(state) = &self.0 {
            let mut state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.snapshot.records.len() == RECEIVER_TRACE_CAPACITY {
                state.snapshot.overflow = state.snapshot.overflow.saturating_add(1);
                return;
            }
            record.local_ns = u64::try_from(at.saturating_duration_since(state.origin).as_nanos())
                .unwrap_or(u64::MAX);
            state.snapshot.records.push(record);
        }
    }

    pub fn snapshot(&self) -> io::Result<Option<ReceiverTraceSnapshot>> {
        self.0
            .as_ref()
            .map(|state| {
                state
                    .lock()
                    .map(|state| state.snapshot.clone())
                    .map_err(|_| io::Error::other("receiver trace poisoned"))
            })
            .transpose()
    }

    /// Cold explicit/session-end output; no file writes on receive or decode paths.
    pub fn write(&self, receiver: &crate::ReceiverSnapshot) -> io::Result<()> {
        if let Some(state) = &self.0 {
            let state = state
                .lock()
                .map_err(|_| io::Error::other("receiver trace poisoned"))?;
            let mut file = io::BufWriter::new(std::fs::File::create(&state.path)?);
            serde_json::to_writer(&mut file, &(&state.snapshot, receiver))?;
            file.flush()?;
        }
        Ok(())
    }
}
