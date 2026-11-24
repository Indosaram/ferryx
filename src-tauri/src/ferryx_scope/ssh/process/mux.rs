//! `bridge --mux`: many helper connections over one SSH stdio channel.
//!
//! One desktop daemon keeps a single SSH connection per host and carries every pane, DAG watcher
//! and control RPC inside it. Each mux channel is an ordinary helper connection to the daemon's
//! IPC endpoint, opened and authenticated exactly like `bridge --stdio` does, so the daemon needs
//! no change and a long `pty.read` on one channel never blocks another channel.
//!
//! Wire format, both directions:
//!
//! ```text
//! u32 BE length of the rest | u32 BE channel | u8 kind | payload
//! ```
//!
//! `DATA` carries a slice of one helper frame (the helper's own `u32 BE length | JSON` frame,
//! unchanged). Slices are at most `MUX_SEGMENT` bytes and the writer serves channels in turn, so
//! one channel's 1 MiB response cannot hold back another channel's small reply for more than one
//! segment. Channel 0 is reserved for the hello and ping exchange.
//!
//! Invariants the code alone does not convey:
//! - The mux owns no PTY. Every PTY belongs to the helper daemon, so the mux ending (client gone,
//!   transport loss, idle bound) never ends a terminal; the client reconnects and resumes from
//!   its cursor.
//! - A channel's daemon connection is closed when its `CLOSE` arrives or the mux ends, which is
//!   what lets the daemon release that connection's DAG subscriptions.
//! - The idle bound is on received frames of any kind. The client pings well inside it, so a quiet
//!   but live host is never reaped; a client that vanished without closing the SSH channel is.

use super::{connect_helper_ipc, endpoint, HelperIpc};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const MUX_HELLO_MAGIC: &str = "ferryx-mux";
pub const MUX_PROTOCOL: u32 = 1;
/// Largest `DATA` payload. Small enough that interleaving keeps input latency low behind a
/// large output burst, large enough that a 1 MiB helper frame costs only 16 segments.
pub const MUX_SEGMENT: usize = 64 * 1024;
/// Largest whole mux frame accepted: header plus one segment, with room for a hello.
const MAX_MUX_FRAME: usize = MUX_SEGMENT + 64;
/// The mux exits after this long without any frame from the client.
pub const MUX_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// Most channels one mux will hold open at a time.
pub const MUX_MAX_CHANNELS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Hello = 1,
    Open = 2,
    Data = 3,
    Close = 4,
    Ping = 5,
    Pong = 6,
}

impl Kind {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Hello,
            2 => Self::Open,
            3 => Self::Data,
            4 => Self::Close,
            5 => Self::Ping,
            6 => Self::Pong,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxFrame {
    pub channel: u32,
    pub kind: Kind,
    pub payload: Vec<u8>,
}

pub fn write_mux_frame(writer: &mut impl Write, frame: &MuxFrame) -> std::io::Result<()> {
    let len = 5 + frame.payload.len();
    let mut header = [0u8; 9];
    header[..4].copy_from_slice(&(len as u32).to_be_bytes());
    header[4..8].copy_from_slice(&frame.channel.to_be_bytes());
    header[8] = frame.kind as u8;
    writer.write_all(&header)?;
    writer.write_all(&frame.payload)
}

/// `Ok(None)` is a clean EOF before any byte of a frame.
pub fn read_mux_frame(reader: &mut impl Read) -> Result<Option<MuxFrame>, String> {
    let mut len = [0u8; 4];
    match reader.read(&mut len[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(error) => return Err(error.to_string()),
    }
    reader.read_exact(&mut len[1..]).map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len) as usize;
    if !(5..=MAX_MUX_FRAME).contains(&len) {
        return Err(format!("PROTOCOL_ERROR: mux frame length {len} out of range"));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    let channel = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let kind = Kind::from_u8(body[4])
        .ok_or_else(|| format!("PROTOCOL_ERROR: unknown mux frame kind {}", body[4]))?;
    body.drain(..5);
    Ok(Some(MuxFrame { channel, kind, payload: body }))
}

struct Outbox {
    queues: HashMap<u32, VecDeque<MuxFrame>>,
    /// Channels with queued frames, in round-robin service order.
    ready: VecDeque<u32>,
    closed: bool,
}

struct Shared {
    outbox: Mutex<Outbox>,
    outbox_changed: Condvar,
    state: Mutex<MuxState>,
    state_changed: Condvar,
}

struct MuxState {
    last_frame: Instant,
    end: Option<Result<(), String>>,
    /// Each open channel's daemon connection, tagged with the open that created it so a pump
    /// finishing late cannot remove a channel id the client has since reopened.
    channels: HashMap<u32, (u64, HelperIpc)>,
    next_open: u64,
}

impl Shared {
    fn outbox(&self) -> MutexGuard<'_, Outbox> {
        self.outbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn state(&self) -> MutexGuard<'_, MuxState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn send(&self, frame: MuxFrame) {
        let mut outbox = self.outbox();
        if outbox.closed {
            return;
        }
        let channel = frame.channel;
        let queue = outbox.queues.entry(channel).or_default();
        let was_empty = queue.is_empty();
        queue.push_back(frame);
        if was_empty {
            outbox.ready.push_back(channel);
        }
        self.outbox_changed.notify_all();
    }

    fn send_data(&self, channel: u32, bytes: &[u8]) {
        for segment in bytes.chunks(MUX_SEGMENT) {
            self.send(MuxFrame { channel, kind: Kind::Data, payload: segment.to_vec() });
        }
    }

    fn finish(&self, result: Result<(), String>) {
        let mut state = self.state();
        if state.end.is_none() {
            state.end = Some(result);
        }
        self.state_changed.notify_all();
        drop(state);
        let mut outbox = self.outbox();
        outbox.closed = true;
        self.outbox_changed.notify_all();
    }

    fn close_channel(&self, channel: u32) {
        if let Some((_, stream)) = self.state().channels.remove(&channel) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn write_loop(output: &mut impl Write, shared: &Shared) -> Result<(), String> {
    loop {
        let frame = {
            let mut outbox = shared.outbox();
            loop {
                if let Some(channel) = outbox.ready.pop_front() {
                    let queue = outbox.queues.get_mut(&channel).expect("ready channel has a queue");
                    let frame = queue.pop_front().expect("ready channel has a frame");
                    if queue.is_empty() {
                        outbox.queues.remove(&channel);
                    } else {
                        outbox.ready.push_back(channel);
                    }
                    break frame;
                }
                if outbox.closed {
                    return Ok(());
                }
                outbox = shared
                    .outbox_changed
                    .wait(outbox)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        write_mux_frame(output, &frame).map_err(|e| format!("MUX_OUTPUT: {e}"))?;
        // Flush per frame: a reply must not wait for the next frame to leave the buffer.
        output.flush().map_err(|e| format!("MUX_OUTPUT: {e}"))?;
    }
}

fn pump_daemon(channel: u32, open: u64, mut from_daemon: HelperIpc, shared: Arc<Shared>) {
    loop {
        let mut header = [0u8; 4];
        if from_daemon.read_exact(&mut header).is_err() {
            break;
        }
        let len = u32::from_be_bytes(header) as usize;
        if len > super::super::helper::MAX_FRAME {
            break;
        }
        let mut frame = Vec::with_capacity(4 + len);
        frame.extend_from_slice(&header);
        frame.resize(4 + len, 0);
        if from_daemon.read_exact(&mut frame[4..]).is_err() {
            break;
        }
        shared.send_data(channel, &frame);
    }
    // The daemon ended this connection (or the mux closed it): tell the client, unless the
    // client already closed the channel itself.
    let mut state = shared.state();
    if state.channels.get(&channel).is_some_and(|(current, _)| *current == open) {
        state.channels.remove(&channel);
        drop(state);
        shared.send(MuxFrame { channel, kind: Kind::Close, payload: Vec::new() });
    }
}

fn open_channel(root: &Path, channel: u32, shared: &Arc<Shared>) -> Result<(), String> {
    if channel == 0 {
        return Err("PROTOCOL_ERROR: channel 0 is reserved".into());
    }
    {
        let state = shared.state();
        if state.channels.contains_key(&channel) {
            return Err(format!("PROTOCOL_ERROR: channel {channel} already open"));
        }
        if state.channels.len() >= MUX_MAX_CHANNELS {
            return Err("RESOURCE_EXHAUSTED: too many mux channels".into());
        }
    }
    let endpoint = endpoint(root)?;
    let stream = connect_helper_ipc(&endpoint.address)?;
    let from_daemon = stream.try_clone().map_err(|e| e.to_string())?;
    let open = {
        let mut state = shared.state();
        state.next_open += 1;
        let open = state.next_open;
        state.channels.insert(channel, (open, stream));
        open
    };
    let pump_shared = Arc::clone(shared);
    std::thread::spawn(move || pump_daemon(channel, open, from_daemon, pump_shared));
    Ok(())
}

/// Per-channel reassembly of helper frames from `DATA` segments, so the token can be injected
/// into each complete request before it reaches the daemon.
#[derive(Default)]
struct Inbound {
    pending: Vec<u8>,
}

impl Inbound {
    fn push(&mut self, segment: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        self.pending.extend_from_slice(segment);
        let mut frames = Vec::new();
        loop {
            if self.pending.len() < 4 {
                break;
            }
            let len = u32::from_be_bytes([
                self.pending[0],
                self.pending[1],
                self.pending[2],
                self.pending[3],
            ]) as usize;
            if len > super::super::helper::MAX_FRAME {
                return Err("INVALID_REQUEST: frame exceeds 1 MiB".into());
            }
            if self.pending.len() < 4 + len {
                break;
            }
            let body: Vec<u8> = self.pending.drain(..4 + len).skip(4).collect();
            frames.push(body);
        }
        Ok(frames)
    }
}

/// Same token injection and op allowlist as `bridge --stdio`.
fn forward_request(
    channel: u32,
    body: &[u8],
    token: &str,
    shared: &Arc<Shared>,
) -> Result<(), String> {
    let mut request: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("INVALID_REQUEST: {e}"))?;
    let op = request.get("op").and_then(serde_json::Value::as_str).unwrap_or("");
    if !super::BRIDGE_OPS.contains(&op) {
        let mut reply = Vec::new();
        super::super::helper::write_frame(
            &mut reply,
            &json!({"ok":false,"error":"UNSUPPORTED: operation not allowlisted"}),
        )?;
        shared.send_data(channel, &reply);
        return Ok(());
    }
    request["token"] = json!(token);
    // Write through a clone so a slow daemon socket never holds the lock other channels need.
    let stream = shared.state().channels.get(&channel).map(|(_, stream)| stream.try_clone());
    let Some(stream) = stream else {
        // Closed by the daemon side while the request was in flight; the client already got
        // (or will get) the CLOSE.
        return Ok(());
    };
    let written = stream
        .map_err(|e| e.to_string())
        .and_then(|mut stream| super::super::helper::write_frame(&mut stream, &request));
    if written.is_err() {
        shared.close_channel(channel);
        shared.send(MuxFrame { channel, kind: Kind::Close, payload: Vec::new() });
    }
    Ok(())
}

fn read_loop(
    root: &Path,
    input: &mut impl Read,
    token: &str,
    shared: &Arc<Shared>,
) -> Result<(), String> {
    let mut inbound: HashMap<u32, Inbound> = HashMap::new();
    while let Some(frame) = read_mux_frame(input)? {
        shared.state().last_frame = Instant::now();
        match frame.kind {
            Kind::Ping => {
                shared.send(MuxFrame { channel: 0, kind: Kind::Pong, payload: frame.payload });
            }
            Kind::Pong | Kind::Hello => {}
            Kind::Open => {
                if let Err(error) = open_channel(root, frame.channel, shared) {
                    shared.send(MuxFrame {
                        channel: frame.channel,
                        kind: Kind::Close,
                        payload: error.into_bytes(),
                    });
                } else {
                    inbound.insert(frame.channel, Inbound::default());
                }
            }
            Kind::Data => {
                let Some(buffer) = inbound.get_mut(&frame.channel) else {
                    continue;
                };
                match buffer.push(&frame.payload) {
                    Ok(requests) => {
                        for body in requests {
                            forward_request(frame.channel, &body, token, shared)?;
                        }
                    }
                    Err(error) => {
                        inbound.remove(&frame.channel);
                        shared.close_channel(frame.channel);
                        shared.send(MuxFrame {
                            channel: frame.channel,
                            kind: Kind::Close,
                            payload: error.into_bytes(),
                        });
                    }
                }
            }
            Kind::Close => {
                inbound.remove(&frame.channel);
                shared.close_channel(frame.channel);
            }
        }
    }
    Ok(())
}

pub fn bridge_mux<I, O>(root: &Path, input: I, output: O) -> Result<(), String>
where
    I: Read + Send + 'static,
    O: Write + Send + 'static,
{
    bridge_mux_with_idle_timeout(root, input, output, MUX_IDLE_TIMEOUT)
}

pub(crate) fn bridge_mux_with_idle_timeout<I, O>(
    root: &Path,
    mut input: I,
    mut output: O,
    idle_timeout: Duration,
) -> Result<(), String>
where
    I: Read + Send + 'static,
    O: Write + Send + 'static,
{
    // Fail before saying hello when there is no daemon, so the client sees a plain error
    // instead of a mux it cannot use.
    let token = endpoint(root)?.token;
    let shared = Arc::new(Shared {
        outbox: Mutex::new(Outbox { queues: HashMap::new(), ready: VecDeque::new(), closed: false }),
        outbox_changed: Condvar::new(),
        state: Mutex::new(MuxState {
            last_frame: Instant::now(),
            end: None,
            channels: HashMap::new(),
            next_open: 0,
        }),
        state_changed: Condvar::new(),
    });

    let hello = json!({
        "magic": MUX_HELLO_MAGIC,
        "protocol": MUX_PROTOCOL,
        "segment": MUX_SEGMENT,
        "idleTimeoutMs": idle_timeout.as_millis() as u64,
        "helperVersion": super::HELPER_VERSION,
    });
    shared.send(MuxFrame {
        channel: 0,
        kind: Kind::Hello,
        payload: serde_json::to_vec(&hello).map_err(|e| e.to_string())?,
    });

    let writer_shared = Arc::clone(&shared);
    let (writer_done, writer_finished) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let result = write_loop(&mut output, &writer_shared);
        writer_shared.finish(result);
        let _ = writer_done.send(());
    });

    let reader_shared = Arc::clone(&shared);
    let reader_root = root.to_path_buf();
    // A blocking stdin read cannot be interrupted portably, so this thread is never joined; the
    // binary exits when this function returns.
    std::thread::spawn(move || {
        let result = read_loop(&reader_root, &mut input, &token, &reader_shared);
        reader_shared.finish(result);
    });

    let mut state = shared.state();
    let result = loop {
        if let Some(result) = state.end.take() {
            break result;
        }
        let idle_for = state.last_frame.elapsed();
        if idle_for >= idle_timeout {
            break Err(format!(
                "REMOTE_IDLE_TIMEOUT: no mux frame for {}s",
                idle_timeout.as_secs()
            ));
        }
        state = shared
            .state_changed
            .wait_timeout(state, idle_timeout - idle_for)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
    };
    let channels: Vec<HelperIpc> = state.channels.drain().map(|(_, (_, stream))| stream).collect();
    drop(state);
    for stream in channels {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    shared.finish(Ok(()));
    // Bounded: a client that stopped reading leaves the writer blocked on stdout, and the mux
    // must still exit so the process ends.
    let _ = writer_finished.recv_timeout(Duration::from_secs(2));
    result
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod mux_tests;
