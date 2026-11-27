//! The keeper's framed UDS server.
//!
//! One thread per connection, and at most [`protocol::MAX_CONNECTIONS`] connected at a time: an
//! acceptor slot is taken before the thread is spawned and released by [`ConnectionGuard`] on every
//! exit path, so a listener cannot be turned into unbounded threads, stacks and per-connection
//! buffers by idle authorized peers. Within a connection frames are handled one at a time, so at
//! most one reply is in flight and the protocol's 32-frame allowance is an upper bound that is never
//! approached rather than a queue that needs draining.
//!
//! ## Replies
//!
//! `ATTACH`, `REPLAY` and `CHECKPOINT_FETCH` answer with a framed run (see the protocol module),
//! every frame of which repeats the request's `request_id` and `frame_sequence`; the run ends at its
//! own end frame or at an `ERROR`. A run is paged by [`RESPONSE_BYTES_PER_REQUEST`] and says so in
//! its end frame, so an 8 MiB replay is fetched in bounded steps instead of one oversize frame. A
//! reply is written whole, under a socket send timeout, so a peer that stops reading cannot pin this
//! thread.
//!
//! An `ATTACH` carries its suffix inline while it fits [`ATTACH_INLINE_MAX_BYTES`]. A larger suffix
//! is sent as a following run only when the request sets `chunked: true`; otherwise the answer is a
//! typed `FRAME_TOO_LARGE` naming the offset to resume from, because a peer that reads exactly one
//! frame per request would desync on frames it never asked for.
//!
//! ## Live output
//!
//! An `ATTACH` that sets `liveOutput: true` also makes the connection the session's output tap:
//! when the socket is idle the loop polls the keeper's output head and writes `OUTPUT` frames for
//! the bytes that appeared since the last one, and the ack echoes `liveOutput` so the tap decision
//! is the answer's. Those frames carry no request id, so a reader must correlate them by kind and
//! offset; a peer that does not opt in gets only the replies it asked for. When the session's
//! backlog cannot be copied under the keeper budget, or the replay log lost the range, the tap is
//! closed with a typed frame and the peer must `REPLAY`.
//!
//! The server is a transport over [`crate::keeper::Keeper`]. It interprets nothing: payloads are
//! JSON metadata plus opaque bytes, and the only field it orders on is `sequenceOffset`.
//!
//! [`Keeper`]'s verbs all take `&self` and synchronise internally, so [`ServerState`] holds an
//! `Arc<Keeper>` and never a mutex: a blocking PTY write in one connection cannot block another
//! connection's verb on a server-wide guard.
//!
//! ## Verbs without their own ack
//!
//! `WRITE`, `HEAD`, `ATTACH`, `REPLAY`, `OWNERSHIP_QUERY` and the checkpoint verbs all have a
//! dedicated response kind. `RESIZE`, `SIGNAL`, `KILL` and `DETACH` do not, and inventing one would
//! renumber a protocol this crate does not own; they are answered by echoing their own kind with
//! the fields that were applied, so the one-response-per-request rule still holds.
//!
//! ## Trust boundary
//!
//! The peer is this uid, but its payload is still input: a malformed field is a typed `BAD_FRAME`
//! naming the field, never a silently defaulted column, a dropped argument or a session launched
//! with an env the daemon did not ask for. `KILL` and every checkpoint mutation additionally require
//! the epoch the caller was granted, because they act on a session whose owner may have changed.

use std::io;
use std::os::fd::RawFd;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::endpoint::Endpoint;
use crate::error::{KeeperError, KeeperErrorCode, KeeperResult};
use crate::budget::Charge;
use crate::keeper::{
    base64_len, Keeper, SessionSignal, SpawnRequest, REPLY_FRAME_OVERHEAD_BYTES, REPLY_PAGE_BYTES,
};
use crate::protocol::{
    self, Frame, Kind, ATTACH_INLINE_MAX_BYTES, CHECKPOINT_FETCH_CHUNK_BYTES,
    CHECKPOINT_FETCH_PAGE_BYTES,
};

/// Largest byte run carried in one `REPLAY_CHUNK` before it is split. Base64 inflates by 4/3, so
/// 64 KiB of bytes stays near 87 KiB of JSON: comfortably under the 1 MiB frame cap.
pub const REPLAY_CHUNK_BYTES: usize = 64 * 1024;

/// Largest byte run carried in one live `OUTPUT` frame, for the same reason as [`REPLAY_CHUNK_BYTES`].
pub const OUTPUT_CHUNK_BYTES: usize = 32 * 1024;

/// How long an idle attachment waits for a frame before checking for new output.
pub const OUTPUT_POLL: Duration = Duration::from_millis(25);

/// How long one reply write may block before the peer is treated as gone.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Most raw bytes one `REPLAY*` run carries before it ends with `more: true`.
///
/// A session's retained log can reach 8 MiB, whose base64 is four times the 1 MiB frame cap, so a
/// whole log cannot be one run: past this page the run stops and the reader asks for the next one at
/// the offset `REPLAY_END` reported.
pub const RESPONSE_BYTES_PER_REQUEST: usize = 512 * 1024;

/// An accepted connection's slot. Dropping it returns the slot on every exit path, including a
/// panic in the connection thread.
pub struct ConnectionGuard {
    live: Arc<AtomicUsize>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Bounded connection admission: `limit` is the plan's [`protocol::MAX_CONNECTIONS`].
pub struct ConnectionLimiter {
    live: Arc<AtomicUsize>,
    limit: usize,
}

impl ConnectionLimiter {
    pub fn new(limit: usize) -> Self {
        Self { live: Arc::new(AtomicUsize::new(0)), limit }
    }

    /// A slot, or `None` when the keeper is already serving `limit` peers. Never blocks: refusing a
    /// ninth peer is the documented behaviour, and the acceptor must stay free for the peer that
    /// disconnects and makes room.
    pub fn try_acquire(&self) -> Option<ConnectionGuard> {
        let mut current = self.live.load(Ordering::SeqCst);
        loop {
            if current >= self.limit {
                return None;
            }
            match self.live.compare_exchange(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Some(ConnectionGuard { live: Arc::clone(&self.live) }),
                Err(actual) => current = actual,
            }
        }
    }

    pub fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
}

/// The live-output tap of one attached connection.
#[derive(Debug, Clone)]
struct Attachment {
    session_id: String,
    epoch: u64,
    /// Absolute offset of the next byte this tap may send.
    sent: u64,
}

/// Everything one connection needs. The keeper is shared, not owned: its verbs take `&self`.
pub struct ServerState {
    pub keeper: Arc<Keeper>,
    pub uid: u32,
}

impl ServerState {
    pub fn new(keeper: Arc<Keeper>, uid: u32) -> Self {
        Self { keeper, uid }
    }

    pub fn keeper(&self) -> &Keeper {
        &self.keeper
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }
}

fn bad(message: impl Into<String>) -> KeeperError {
    KeeperError::new(KeeperErrorCode::BadFrame, message)
}

fn required_str(payload: &serde_json::Value, key: &str) -> KeeperResult<String> {
    match payload.get(key) {
        Some(serde_json::Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(bad(format!("payload field {key} must be a string"))),
        None => Err(bad(format!("payload needs a {key}"))),
    }
}

fn optional_str(payload: &serde_json::Value, key: &str) -> KeeperResult<Option<String>> {
    match payload.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(bad(format!("payload field {key} must be a string when present"))),
    }
}

fn required_u64(payload: &serde_json::Value, key: &str) -> KeeperResult<u64> {
    match payload.get(key) {
        Some(value) => value
            .as_u64()
            .ok_or_else(|| bad(format!("payload field {key} must be a non-negative integer"))),
        None => Err(bad(format!("payload needs a {key}"))),
    }
}

fn optional_u64(payload: &serde_json::Value, key: &str) -> KeeperResult<Option<u64>> {
    match payload.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| bad(format!("payload field {key} must be a non-negative integer"))),
    }
}

fn required_u16(payload: &serde_json::Value, key: &str) -> KeeperResult<u16> {
    let value = required_u64(payload, key)?;
    u16::try_from(value).map_err(|_| bad(format!("payload field {key} is outside the u16 range")))
}

fn required_u32(payload: &serde_json::Value, key: &str) -> KeeperResult<u32> {
    let value = required_u64(payload, key)?;
    u32::try_from(value).map_err(|_| bad(format!("payload field {key} is outside the u32 range")))
}

fn optional_bool(payload: &serde_json::Value, key: &str) -> KeeperResult<Option<bool>> {
    match payload.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(bad(format!("payload field {key} must be a boolean when present"))),
    }
}

/// Reads `env` as either a JSON object of strings or an array of `[key, value]` pairs. The wire
/// documents the object form; the pair form is what a serializer that must preserve order emits.
/// Every entry must be a string: an entry the keeper cannot honour is refused by name rather than
/// dropped, because a silently shortened environment launches a different program.
fn pairs(payload: &serde_json::Value, key: &str) -> KeeperResult<Vec<(String, String)>> {
    let value = match payload.get(key) {
        None | Some(serde_json::Value::Null) => return Ok(Vec::new()),
        Some(value) => value,
    };
    if let Some(object) = value.as_object() {
        let mut out = Vec::with_capacity(object.len());
        for (name, value) in object {
            let text = value
                .as_str()
                .ok_or_else(|| bad(format!("env entry {name} must be a string")))?;
            out.push((name.clone(), text.to_string()));
        }
        return Ok(out);
    }
    let rows = value
        .as_array()
        .ok_or_else(|| bad("env must be an object or an array of [key, value] pairs"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let pair = row
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(|| bad("env row must be a [key, value] pair"))?;
        let name = pair[0]
            .as_str()
            .ok_or_else(|| bad("env row key must be a string"))?;
        let text = pair[1]
            .as_str()
            .ok_or_else(|| bad("env row value must be a string"))?;
        out.push((name.to_string(), text.to_string()));
    }
    Ok(out)
}

fn argv(payload: &serde_json::Value, key: &str) -> KeeperResult<Vec<String>> {
    let items = match payload.get(key) {
        None | Some(serde_json::Value::Null) => return Ok(Vec::new()),
        Some(serde_json::Value::Array(items)) => items,
        Some(_) => return Err(bad("argv must be an array of strings")),
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let text = item
            .as_str()
            .ok_or_else(|| bad("argv entries must be strings"))?;
        out.push(text.to_string());
    }
    Ok(out)
}

/// Opaque bytes. A field that is present but not base64 text is a malformed frame, not an absent
/// field: treating the two alike would silently drop input the daemon believed it had sent.
fn bytes_at(payload: &serde_json::Value, key: &str) -> KeeperResult<Option<Vec<u8>>> {
    match payload.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(text)) => base64_decode(text)
            .map(Some)
            .map_err(|error| bad(format!("payload field {key} is not valid base64: {error}"))),
        Some(_) => Err(bad(format!("payload field {key} must be base64 text"))),
    }
}

/// The epoch a destructive verb must present. `KILL` and every checkpoint mutation change a session
/// whose owner may have changed since the caller last held it, so the fence is checked here before
/// the keeper is asked to act.
fn fenced_epoch(
    state: &ServerState,
    session_id: &str,
    payload: &serde_json::Value,
) -> KeeperResult<u64> {
    let epoch = required_u64(payload, "epoch")?;
    match state.keeper.newest_epoch(session_id) {
        Some(newest) if newest == epoch => Ok(epoch),
        Some(newest) => Err(KeeperError::epoch_mismatch(session_id, epoch, newest)),
        // Authoritative discovery: a verb naming a session the keeper does not own is answered
        // `UNKNOWN_SESSION`, not "your epoch is stale". The two are different facts and the plan
        // makes the keeper the source of truth for which sessions exist, so reporting a fence
        // failure for a session that was never here would send the daemon looking for an epoch it
        // can never find. It still fails closed: nothing is performed either way.
        None if !state.keeper.contains(session_id) => Err(KeeperError::unknown_session(session_id)),
        // A live session always carries a fence - `SPAWN` sets one - so a session that exists with no
        // fence is a state this keeper must not guess about. Refusing is the only safe answer.
        None => Err(KeeperError::epoch_mismatch(session_id, epoch, 0)),
    }
}

fn error_frame(request_id: &str, seq: u64, error: KeeperError) -> Frame {
    let KeeperError { code, message, offset } = error;
    Frame::new(
        Kind::Error,
        request_id.to_owned(),
        seq,
        serde_json::json!({
            "code": code.as_str(),
            "message": message,
            "offset": offset,
        }),
    )
}

/// The reply for a kind that has no dedicated ack: the request kind, echoed with what was applied.
fn acked(frame: &Frame, payload: serde_json::Value) -> Frame {
    Frame::new(frame.kind, frame.request_id.clone(), frame.frame_sequence, payload)
}

/// A `CHECKPOINT_FETCH` page: one slice of the committed checkpoint, bound to the identity the
/// caller asked for.
///
/// The identity is not read first and re-verified later: every read goes through
/// [`Keeper::checkpoint_fetch_page`], which takes the epoch check, the pin, the identity comparison
/// and the bounded copy under one `ownership` hold. The earlier shape read the committed info, then
/// the pin, then each chunk separately, so a `CHECKPOINT_COMMIT` landing in between made the answer
/// describe a checkpoint that was never the one asked for. Here a replacement at any point in the
/// page is `STALE_CHECKPOINT`, never a stitched recovery point.
#[allow(clippy::too_many_arguments)]
fn checkpoint_fetch_run(
    keeper: &Keeper,
    request_id: &str,
    seq: u64,
    session_id: &str,
    checkpoint_id: &str,
    sequence_offset: u64,
    epoch: u64,
    from_byte: u64,
    max_bytes: u64,
) -> KeeperResult<Reply> {
    // A zero-length page, not a second read path: it answers the identity and the total for exactly
    // this checkpoint (refusing a pin that is not the committed one) without copying a byte. The
    // page length has to be known before the scratch buffer and its charge exist. It probes from 0
    // rather than from `from_byte`, so a request past the end is reported as the `BAD_FRAME` it is
    // instead of the store's chunk mismatch.
    let mut probe: [u8; 0] = [];
    let head = keeper.checkpoint_fetch_page(
        session_id,
        checkpoint_id,
        sequence_offset,
        epoch,
        0,
        &mut probe,
    )?;
    let total_bytes = head.total_bytes;
    if from_byte > total_bytes {
        return Err(bad(format!(
            "checkpoint {checkpoint_id} is {total_bytes} bytes; fromByte {from_byte} is past its end"
        )));
    }
    let end = from_byte.saturating_add(max_bytes).min(total_bytes);
    let page_len = end.saturating_sub(from_byte) as usize;
    let chunks = page_len.div_ceil(CHECKPOINT_FETCH_CHUNK_BYTES);
    // Charged before the page exists, at the same bound every other paged reply takes: the scratch
    // copy, the base64 text in the frames and the encode buffer built from that text, plus one
    // envelope per frame. Charging `page_len + base64_len` left the encode buffer uncounted, and the
    // two coexist during the socket write.
    let charge =
        Charge::try_reserve(keeper.budget_handle(), page_charge_bound(page_len, chunks + 1))
            .ok_or_else(|| {
                let available = keeper.budget().available();
                KeeperError::new(
                    KeeperErrorCode::CheckpointBudgetExhausted,
                    format!(
                        "session {session_id}: fetching {page_len} bytes of checkpoint {checkpoint_id} does not fit the keeper budget ({available} bytes available)"
                    ),
                )
            })?;
    let mut scratch = vec![0u8; page_len];
    let mut frames = Vec::with_capacity(chunks + 1);
    let mut at = from_byte;
    for index in 0..chunks {
        let start = index * CHECKPOINT_FETCH_CHUNK_BYTES;
        let want = CHECKPOINT_FETCH_CHUNK_BYTES.min(scratch.len() - start);
        let slice = &mut scratch[start..][..want];
        let chunk = keeper.checkpoint_fetch_page(
            session_id,
            checkpoint_id,
            sequence_offset,
            epoch,
            at,
            slice,
        )?;
        let data = base64_encode(&slice[..chunk.filled]);
        frames.push(Frame::new(
            Kind::CheckpointFetchChunk,
            request_id,
            seq,
            serde_json::json!({
                "sessionId": session_id,
                "checkpointId": chunk.checkpoint_id,
                "sequenceOffset": chunk.sequence_offset,
                "totalBytes": chunk.total_bytes,
                "fromByte": at,
                "chunkIndex": index,
                "data": data,
            }),
        ));
        at = at.saturating_add(chunk.filled as u64);
    }
    let next_byte = end;
    frames.push(Frame::new(
        Kind::CheckpointFetchEnd,
        request_id,
        seq,
        serde_json::json!({
            "sessionId": session_id,
            "checkpointId": checkpoint_id,
            "sequenceOffset": sequence_offset,
            "totalBytes": total_bytes,
            "fromByte": from_byte,
            "nextByte": next_byte,
            "complete": next_byte >= total_bytes,
        }),
    ));
    // The frames now hold their own copies of the page, so the scratch buffer goes. The charge stays
    // at the peak it was taken at: the serialized frame is built from those copies during the write.
    drop(scratch);
    Ok(Reply::charged(frames, charge))
}

/// Answer one frame. Total: every kind either answers, answers with a framed run, or returns a
/// typed error frame. Nothing a peer can send reaches a panic or a silent default.
pub fn dispatch(state: &ServerState, frame: &Frame) -> Reply {
    match dispatch_verb(state, frame) {
        Ok(reply) => reply,
        Err(error) => Reply::free(vec![error_frame(
            &frame.request_id,
            frame.frame_sequence,
            error,
        )]),
    }
}

fn dispatch_verb(state: &ServerState, frame: &Frame) -> KeeperResult<Reply> {
    let request_id = frame.request_id.as_str();
    let seq = frame.frame_sequence;
    let keeper = &state.keeper;

    match frame.kind {
        Kind::Hello => {
            // Additive-only within a major version: a peer speaking another major cannot be served,
            // and saying so here is better than misreading its payloads for the connection's life.
            if let Some(major) = optional_u64(&frame.payload, "protocolMajor")? {
                if major != u64::from(crate::PROTOCOL_MAJOR) {
                    return Err(bad(format!(
                        "protocol major {major} is not supported; this keeper speaks {}",
                        crate::PROTOCOL_MAJOR
                    )));
                }
            }
            // `epoch-fenced-checkpoint-v1` is advertised because the daemon-side audit landed:
            // `.omo/evidence/keeper-concurrency/REPLY-daemon-fenced-reads.md` records that every
            // keeper read in the daemon is fenced (or is `OWNERSHIP_LIST`, which has no epoch to
            // name), and that no `KeeperClient::replay` / `head` call site remains anywhere in
            // `src-tauri`. That audit is the gate on this flag, because once it is out a daemon's
            // `refuse_unfenced` turns any epoch-less `REPLAY`/`HEAD` into a hard client error - a
            // single surviving unfenced call would break that daemon, and the keeper could not
            // detect it from its side. Unfenced forms stay served here regardless (see
            // `Keeper::replay` / `Keeper::head`), so an older daemon that never reads this key is
            // unaffected.
            Ok(Reply::free(vec![Frame::new(
                Kind::HelloAck,
                request_id,
                seq,
                serde_json::json!({
                    "protocolMajor": crate::PROTOCOL_MAJOR,
                    "protocolMinor": crate::PROTOCOL_MINOR,
                    "keeperVersion": crate::KEEPER_VERSION,
                    "pid": std::process::id(),
                    "capabilities": [crate::CAPABILITY_EPOCH_FENCED_CHECKPOINT],
                }),
            )]))
        }

        // -- session lifecycle -----------------------------------------------------------------
        Kind::Spawn => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let request = SpawnRequest {
                session_id,
                request_id: optional_str(&frame.payload, "requestId")?.unwrap_or_default(),
                argv: argv(&frame.payload, "argv")?,
                env: pairs(&frame.payload, "env")?,
                cwd: optional_str(&frame.payload, "cwd")?.map(PathBuf::from),
                cols: required_u16(&frame.payload, "cols")?,
                rows: required_u16(&frame.payload, "rows")?,
                epoch: optional_u64(&frame.payload, "epoch")?.unwrap_or(0),
            };
            let info = keeper.spawn(request)?;
            Ok(Reply::free(vec![Frame::new(
                Kind::SpawnAck,
                request_id,
                seq,
                serde_json::json!({
                    "sessionId": info.session_id,
                    "pid": info.pid,
                    "pgid": info.pgid,
                    "processStartTime": info.process_start_time,
                    "cols": info.cols,
                    "rows": info.rows,
                    "epoch": info.epoch,
                    "sequenceOffset": info.sequence_offset,
                }),
            )]))
        }
        Kind::Write => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let epoch = required_u64(&frame.payload, "epoch")?;
            let data = bytes_at(&frame.payload, "data")?
                .ok_or_else(|| bad("write needs base64 data"))?;
            // `WRITE` has no dedicated ack kind and this crate does not own the numbering, so the
            // answer echoes the request kind with the applied count.
            let written = keeper.write(&session_id, epoch, &data)?;
            Ok(Reply::free(vec![acked(
                frame,
                serde_json::json!({ "sessionId": session_id, "written": written }),
            )]))
        }
        Kind::Resize => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let epoch = required_u64(&frame.payload, "epoch")?;
            let cols = required_u16(&frame.payload, "cols")?;
            let rows = required_u16(&frame.payload, "rows")?;
            keeper.resize(&session_id, epoch, cols, rows)?;
            Ok(Reply::free(vec![acked(
                frame,
                serde_json::json!({
                    "sessionId": session_id,
                    "epoch": epoch,
                    "cols": cols,
                    "rows": rows,
                }),
            )]))
        }
        Kind::Signal => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let epoch = required_u64(&frame.payload, "epoch")?;
            let name = required_str(&frame.payload, "signal")?;
            let signal = SessionSignal::parse(&name).ok_or_else(|| {
                bad("signal name is not one of interrupt/terminate/kill/stop/continue")
            })?;
            keeper.signal(&session_id, epoch, signal)?;
            Ok(Reply::free(vec![acked(
                frame,
                serde_json::json!({
                    "sessionId": session_id,
                    "epoch": epoch,
                    "signal": signal.as_str(),
                }),
            )]))
        }
        Kind::Kill => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let epoch = fenced_epoch(state, &session_id, &frame.payload)?;
            keeper.kill_fenced(&session_id, epoch)?;
            Ok(Reply::free(vec![acked(
                frame,
                serde_json::json!({ "sessionId": session_id, "epoch": epoch }),
            )]))
        }

        // -- attach and replay -----------------------------------------------------------------
        Kind::Attach => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let epoch = required_u64(&frame.payload, "epoch")?;
            let after = optional_u64(&frame.payload, "afterSequence")?.unwrap_or(0);
            // Every flag is parsed before the attach, because attaching fences the epoch: a
            // malformed request must not have a side effect on the session's ownership first.
            let chunked = optional_bool(&frame.payload, "chunked")? == Some(true);
            let live_output = optional_bool(&frame.payload, "liveOutput")? == Some(true);
            // Reserved before the keeper copies, at the page cap the copy is bounded by: a charge
            // taken afterwards would be admitting memory the allocation already took. One page is
            // the most `attach` copies, so this is the peak, not an estimate.
            let charge = charge_page(
                keeper,
                REPLY_PAGE_BYTES,
                replay_frame_bound(),
                KeeperErrorCode::MemoryBudgetExhausted,
                format!("attach of {session_id}"),
            )?;
            let outcome = keeper.attach(&session_id, epoch, after)?;

            // A short suffix rides in the ack so the checkpoint and its bytes are applied in one
            // step. A longer one cannot -- base64 of it exceeds the 1 MiB frame cap -- so it is
            // either announced as a following framed run or refused with the offset to resume from.
            // Only a peer that opted in gets the run: one that reads a single frame per request
            // would desync on frames it never asked for.
            let fits_inline = outcome.data.len() <= ATTACH_INLINE_MAX_BYTES;
            if !fits_inline && !chunked {
                let resume_at = outcome
                    .sequence_offset
                    .saturating_sub(outcome.data.len() as u64);
                return Err(KeeperError::at(
                    KeeperErrorCode::FrameTooLarge,
                    format!(
                        "attach suffix of {} bytes does not fit one frame; request it with REPLAY from {resume_at} or attach with chunked:true",
                        outcome.data.len()
                    ),
                    resume_at,
                ));
            }
            let mut payload = serde_json::json!({
                "sessionId": outcome.session_id.clone(),
                "epoch": outcome.epoch,
                "sequenceOffset": outcome.sequence_offset,
                // The offset the page began at and how many bytes it actually carried. The head alone
                // cannot say where an inline page ended: a page capped at `REPLY_PAGE_BYTES` ends
                // before the head, and a tap that took the head would skip the remainder for good.
                "afterSequence": after,
                "dataBytes": outcome.data.len(),
                "checkpointId": optional_checkpoint_id(&outcome.checkpoint),
                "checkpointSequenceOffset": outcome.checkpoint.sequence_offset,
                "storedBytes": outcome.checkpoint.stored_bytes,
                "dataChunked": !fits_inline,
                // Echoed so the tap decision is the answer's, not a second reading of the request:
                // the connection arms its output pump only when the peer asked for one.
                "liveOutput": live_output,
            });
            if fits_inline {
                payload["data"] = serde_json::Value::String(base64_encode(&outcome.data));
            }
            if let Some(gap) = outcome.replay_gap {
                payload["replayGap"] = gap_payload(gap);
            }
            let mut frames = vec![Frame::new(Kind::AttachAck, request_id, seq, payload)];
            if !fits_inline && outcome.replay_gap.is_none() {
                frames.extend(replay_run(
                    request_id,
                    seq,
                    &outcome.session_id,
                    outcome.sequence_offset,
                    // A chunked ack streams the suffix the attach copied, which began at `after`.
                    after,
                    // Deliberately `false`, unlike `REPLAY`: this run is one page and nothing follows
                    // it on this correlation id. A peer that reads `more` as "more frames are coming"
                    // would wait here for a continuation that is fetched with a separate `REPLAY`.
                    // The bytes past this page are not lost: the run ends at their start, which is
                    // where the reader's next `REPLAY` resumes.
                    false,
                    &outcome.data,
                ));
            }
            Ok(Reply::charged(frames, charge))
        }
        Kind::Replay => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let from = optional_u64(&frame.payload, "fromSequence")?.unwrap_or(0);
            let charge = charge_page(
                keeper,
                REPLY_PAGE_BYTES,
                replay_frame_bound(),
                KeeperErrorCode::MemoryBudgetExhausted,
                format!("replay of {session_id} from {from}"),
            )?;
            // An epoch makes the read fenced: the page is copied while no attach can advance the
            // session's epoch, so the bytes cannot belong to a successor. An older daemon sends no
            // epoch and keeps the unfenced read it was built against.
            let outcome = match optional_u64(&frame.payload, "epoch")? {
                Some(epoch) => keeper.replay_fenced(&session_id, from, epoch)?,
                None => keeper.replay(&session_id, from)?,
            };
            if let Some(gap) = outcome.replay_gap {
                // The lossy-reset contract as the protocol's own event: never a shortened answer
                // the caller could mistake for a complete history.
                return Ok(Reply::free(vec![Frame::new(
                    Kind::ReplayGapEvent,
                    request_id,
                    seq,
                    serde_json::json!({
                        "sessionId": session_id,
                        "code": KeeperErrorCode::ReplayGap.as_str(),
                        "offset": gap.resumed_at_offset,
                        "detail": gap_payload(gap),
                    }),
                )]));
            }
            let frames = replay_run(
                request_id,
                seq,
                &session_id,
                outcome.sequence_offset,
                from,
                outcome.more,
                &outcome.data,
            );
            Ok(Reply::charged(frames, charge))
        }
        Kind::Head => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let head = match optional_u64(&frame.payload, "epoch")? {
                Some(epoch) => keeper.head_fenced(&session_id, epoch)?,
                None => keeper.head(&session_id)?,
            };
            Ok(Reply::free(vec![Frame::new(
                Kind::HeadAck,
                request_id,
                seq,
                serde_json::json!({
                    "sessionId": session_id,
                    "sequenceOffset": head,
                }),
            )]))
        }

        // -- discovery -------------------------------------------------------------------------
        Kind::OwnershipQuery => {
            // Authoritative discovery: the keeper's registry is the answer, and it is read here
            // rather than inferred from connections.
            let sessions: Vec<serde_json::Value> = keeper
                .ownership()
                .iter()
                .map(|state| {
                    serde_json::json!({
                        "id": state.id(),
                        "generation": state.generation(),
                        "pid": state.pid(),
                        "epoch": state.epoch(),
                        "sequenceOffset": state.sequence_offset(),
                        "replayRetainedBytes": state.replay_retained_bytes,
                        "replayResets": state.replay_resets,
                        "cols": state.cols(),
                        "rows": state.rows(),
                        "owner": state.owner().as_str(),
                        "focused": state.is_focused(),
                        "lastActivityMillis": state.last_activity_millis(),
                    })
                })
                .collect();
            Ok(Reply::free(vec![Frame::new(
                Kind::OwnershipList,
                request_id,
                seq,
                serde_json::json!({ "sessions": sessions }),
            )]))
        }

        // -- checkpoint transport (bytes are daemon-produced; the keeper never parses them) -----
        Kind::CheckpointInstall => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let checkpoint_id = required_str(&frame.payload, "checkpointId")?;
            let sequence_offset = required_u64(&frame.payload, "sequenceOffset")?;
            let epoch = fenced_epoch(state, &session_id, &frame.payload)?;
            let total_bytes = optional_u64(&frame.payload, "totalBytes")?;
            let data = bytes_at(&frame.payload, "data")?;
            let ack = match data {
                // Complete in one frame: commit immediately. A declaration that disagrees with the
                // bytes carried is passed through rather than corrected, so the store's typed
                // `CHECKPOINT_CHUNK_MISMATCH` is what the peer sees instead of a silent rewrite of
                // the size it asked for.
                Some(bytes) => {
                    keeper.install_checkpoint(
                        &session_id,
                        &checkpoint_id,
                        sequence_offset,
                        total_bytes.unwrap_or(bytes.len() as u64),
                        bytes,
                        epoch,
                    )?
                }
                // No data: open the chunked upload and answer with its progress so far.
                None => {
                    let progress = keeper.begin_checkpoint(
                        &session_id,
                        &checkpoint_id,
                        sequence_offset,
                        total_bytes.unwrap_or(0),
                        epoch,
                    )?;
                    return Ok(Reply::free(vec![Frame::new(
                        Kind::CheckpointAck,
                        request_id,
                        seq,
                        serde_json::json!({
                            "sessionId": progress.session_id,
                            "checkpointId": progress.checkpoint_id,
                            "sequenceOffset": progress.sequence_offset,
                            "storedBytes": progress.received_bytes,
                            "totalBytes": progress.total_bytes,
                            "complete": progress.complete,
                            // The attempt's own handle. The connection records it and aborts by it on
                            // teardown, so a disconnect releases this upload and never one a retry
                            // opened over the same ids.
                            "uploadToken": progress.upload_token,
                        }),
                    )]));
                }
            };
            Ok(Reply::free(vec![Frame::new(
                Kind::CheckpointAck,
                request_id,
                seq,
                checkpoint_ack(&session_id, &ack),
            )]))
        }
        Kind::CheckpointChunk => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let checkpoint_id = required_str(&frame.payload, "checkpointId")?;
            let chunk_index = required_u32(&frame.payload, "chunkIndex")?;
            let data = bytes_at(&frame.payload, "data")?
                .ok_or_else(|| bad("checkpoint chunk needs base64 data"))?;
            // Shape before ownership: a frame that omits a required field is a `BAD_FRAME` even
            // when the session it names is unknown, so a peer is told to fix its request rather
            // than sent looking for a session that was never here. Taking the fence first reported
            // `UNKNOWN_SESSION` for a frame that could never have been dispatched anyway.
            let epoch = fenced_epoch(state, &session_id, &frame.payload)?;
            let progress =
                keeper.checkpoint_chunk(&session_id, &checkpoint_id, chunk_index, &data, epoch)?;
            Ok(Reply::free(vec![Frame::new(
                Kind::CheckpointAck,
                request_id,
                seq,
                serde_json::json!({
                    "sessionId": progress.session_id,
                    "checkpointId": progress.checkpoint_id,
                    "sequenceOffset": progress.sequence_offset,
                    "storedBytes": progress.received_bytes,
                    "totalBytes": progress.total_bytes,
                    "complete": progress.complete,
                }),
            )]))
        }
        Kind::CheckpointCommit => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let checkpoint_id = required_str(&frame.payload, "checkpointId")?;
            let sequence_offset = required_u64(&frame.payload, "sequenceOffset")?;
            let epoch = fenced_epoch(state, &session_id, &frame.payload)?;
            let info =
                keeper.commit_checkpoint(&session_id, &checkpoint_id, sequence_offset, epoch)?;
            Ok(Reply::free(vec![Frame::new(
                Kind::CheckpointAck,
                request_id,
                seq,
                checkpoint_ack(&session_id, &info),
            )]))
        }
        Kind::CheckpointQuery => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let info = match optional_u64(&frame.payload, "epoch")? {
                Some(epoch) => keeper.checkpoint_info_fenced(&session_id, epoch)?,
                None => keeper.checkpoint_info(&session_id)?,
            };
            Ok(Reply::free(vec![Frame::new(
                Kind::CheckpointAck,
                request_id,
                seq,
                checkpoint_ack(&session_id, &info),
            )]))
        }
        Kind::CheckpointFetch => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let checkpoint_id = required_str(&frame.payload, "checkpointId")?;
            let sequence_offset = required_u64(&frame.payload, "sequenceOffset")?;
            let from_byte = optional_u64(&frame.payload, "fromByte")?.unwrap_or(0);
            let requested = optional_u64(&frame.payload, "maxBytes")?
                .unwrap_or(CHECKPOINT_FETCH_PAGE_BYTES);
            let epoch = fenced_epoch(state, &session_id, &frame.payload)?;
            checkpoint_fetch_run(
                keeper,
                request_id,
                seq,
                &session_id,
                &checkpoint_id,
                sequence_offset,
                epoch,
                from_byte,
                requested.clamp(1, CHECKPOINT_FETCH_PAGE_BYTES),
            )
        }

        // -- administration --------------------------------------------------------------------
        Kind::Detach => {
            let session_id = required_str(&frame.payload, "sessionId")?;
            let epoch = required_u64(&frame.payload, "epoch")?;
            keeper.detach(&session_id, epoch)?;
            Ok(Reply::free(vec![acked(
                frame,
                serde_json::json!({ "sessionId": session_id, "epoch": epoch }),
            )]))
        }
        Kind::Shutdown => {
            // Refuses every later spawn and retires what is owned. The listener is closed by the
            // process as it exits, so the only answer is "understood, and no more work".
            keeper.shutdown();
            Ok(Reply::free(vec![Frame::new(
                Kind::Shutdown,
                request_id,
                seq,
                serde_json::json!({
                    "code": KeeperErrorCode::ShuttingDown.as_str(),
                    "message": "keeper is shutting down",
                }),
            )]))
        }

        // A kind that only the keeper may originate is a peer bug, and it is named rather than
        // silently dropped so a version skew shows up in the daemon's log.
        other => {
            let message = format!("{other:?} is a keeper-originated frame, not a request");
            Err(bad(message))
        }
    }
}

fn checkpoint_ack(session_id: &str, info: &crate::checkpoint::CheckpointInfo) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "sessionId": session_id,
        "sequenceOffset": info.sequence_offset,
        "storedBytes": info.stored_bytes,
    });
    if info.has_checkpoint() {
        payload["checkpointId"] = serde_json::Value::String(info.checkpoint_id.clone());
    }
    payload
}

fn gap_payload(gap: crate::replay::ReplayGap) -> serde_json::Value {
    serde_json::json!({
        "requestedAfterSequence": gap.requested_after_offset,
        "availableFromSequence": gap.resumed_at_offset,
        "retainedFromSequence": gap.retained_from_offset,
    })
}

fn optional_checkpoint_id(info: &crate::checkpoint::CheckpointInfo) -> Option<&str> {
    if info.has_checkpoint() {
        Some(info.checkpoint_id.as_str())
    } else {
        None
    }
}

/// One verb's answer, carrying the budget charge that covers the bytes it holds.
///
/// The charge lives as long as the reply does, so it covers the socket write and is released on the
/// write-failure path too; releasing it inside the handler would free memory the frames still hold.
pub struct Reply {
    frames: Vec<Frame>,
    charge: Option<Charge>,
}

impl Reply {
    pub fn free(frames: Vec<Frame>) -> Self {
        Self { frames, charge: None }
    }

    pub fn charged(frames: Vec<Frame>, charge: Charge) -> Self {
        Self { frames, charge: Some(charge) }
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn charged_bytes(&self) -> u64 {
        self.charge.as_ref().map_or(0, Charge::bytes)
    }

    /// The frames, releasing the charge with them. For a caller that will not write them.
    pub fn into_frames(mut self) -> Vec<Frame> {
        std::mem::take(&mut self.frames)
    }
}

/// Peak bytes one paged reply holds at any instant, which is what stays charged from before the
/// copy until the socket write returns:
///
/// * `page_bytes` - the copy the keeper makes out of the replay log or checkpoint store;
/// * `2 * base64_len(page_bytes)` - the base64 text in the frames, plus the serialized buffer
///   [`protocol::encode_frame`] builds from that text immediately before the write;
/// * one envelope per frame, plus the extra one for the serialized frame's header.
///
/// The fourth term is deliberately not trimmed to the page actually copied: the copy and its text
/// overlap, the encode buffer overlaps the text, and a reservation taken at the page cap is the
/// only bound that cannot be outgrown when the log's head advances mid-request.
fn page_charge_bound(page_bytes: usize, frames: usize) -> u64 {
    page_bytes as u64
        + 2 * base64_len(page_bytes)
        + (frames as u64 + 1) * REPLY_FRAME_OVERHEAD_BYTES
}

/// Reserves the peak of one paged reply before its page is copied. Over-counting is safe: the
/// ledger may count a byte twice, never miss one.
fn charge_page(
    keeper: &Keeper,
    page_bytes: usize,
    frames: usize,
    code: KeeperErrorCode,
    subject: String,
) -> KeeperResult<Charge> {
    let bound = page_charge_bound(page_bytes, frames);
    Charge::try_reserve(keeper.budget_handle(), bound).ok_or_else(|| {
        let available = keeper.budget().available();
        KeeperError::new(
            code,
            format!("{subject} needs {bound} bytes of keeper budget, {available} available"),
        )
    })
}

/// Frames a full [`REPLY_PAGE_BYTES`] page becomes: the ack, one chunk per [`REPLAY_CHUNK_BYTES`],
/// and the end frame.
const fn replay_frame_bound() -> usize {
    2 + REPLY_PAGE_BYTES.div_ceil(REPLAY_CHUNK_BYTES)
}

/// A `REPLAY*` page: at most [`RESPONSE_BYTES_PER_REQUEST`] of the suffix as `REPLAY_CHUNK` frames,
/// then `REPLAY_END`.
///
/// `from` is the cursor the reader asked to resume after, and the page it gets is exactly the bytes
/// `from..from + page.len()`. Each chunk therefore carries `from` plus the bytes before it as its
/// absolute offset: deriving the offset from the head instead would relabel a page that does not
/// reach the head, so a reader paging through more than one page would resume past bytes it never
/// received. The run is written contiguously: the connection loop never interleaves another reply
/// inside it.
///
/// `more` is the keeper's own answer about the **backlog**, not a comparison here: `data` has already
/// been capped at one page by the copy that produced it, so `page.len() < data.len()` could never be
/// true and a reader would stop on the first page of a longer suffix. It comes from the copy
/// (`ReplayOutcome::more`), which is where the untruncated remaining length is known.
fn replay_run(
    request_id: &str,
    seq: u64,
    session_id: &str,
    head: u64,
    from: u64,
    more: bool,
    data: &[u8],
) -> Vec<Frame> {
    let page = &data[..data.len().min(RESPONSE_BYTES_PER_REQUEST)];
    let mut frames = Vec::with_capacity(page.len() / REPLAY_CHUNK_BYTES + 2);
    for (index, chunk) in page.chunks(REPLAY_CHUNK_BYTES).enumerate() {
        let at = from.saturating_add((index * REPLAY_CHUNK_BYTES) as u64);
        frames.push(Frame::new(
            Kind::ReplayChunk,
            request_id,
            seq,
            serde_json::json!({
                "sessionId": session_id,
                "sequenceOffset": at,
                "data": base64_encode(chunk),
            }),
        ));
    }
    let end = from.saturating_add(page.len() as u64);
    frames.push(Frame::new(
        Kind::ReplayEnd,
        request_id,
        seq,
        serde_json::json!({
            "sessionId": session_id,
            "sequenceOffset": end,
            "head": head,
            "more": more,
        }),
    ));
    frames
}

/// Minimal base64: the wire carries opaque bytes as text.
pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(TABLE[(b[0] >> 2) as usize] as char);
        out.push(TABLE[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(b[2] & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// The inverse of [`base64_encode`]. Padding is required and validated, so a truncated payload is
/// a typed refusal instead of bytes that silently lose their tail.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, &'static str> {
    fn value(byte: u8) -> Option<u8> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }

    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("length is not a multiple of 4");
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let quanta = bytes.len() / 4;
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if pad > 2 {
            return Err("more than two padding characters");
        }
        // Padding marks the end of the stream: a padded quantum before the last one is a malformed
        // payload, not two payloads glued together.
        if pad > 0 && index + 1 != quanta {
            return Err("padding before the final quantum");
        }
        let mut packed = [0u8; 4];
        for (position, byte) in chunk.iter().enumerate() {
            packed[position] = if *byte == b'=' {
                if position < 4 - pad {
                    return Err("padding in the middle of a quantum");
                }
                0
            } else {
                value(*byte).ok_or("character outside the base64 alphabet")?
            };
        }
        out.push((packed[0] << 2) | (packed[1] >> 4));
        if pad < 2 {
            out.push((packed[1] << 4) | (packed[2] >> 2));
        }
        if pad < 1 {
            out.push((packed[2] << 6) | packed[3]);
        }
    }
    Ok(out)
}

/// Refuses a peer that is not this keeper's user, before a single frame is read.
fn authorize(endpoint: &Endpoint, state: &ServerState, fd: std::os::fd::RawFd) -> bool {
    match endpoint.verify_peer(fd) {
        Ok(credentials) => credentials.uid == state.uid,
        Err(_) => false,
    }
}

/// Waits until the socket has a frame to read, or the timeout expires.
///
/// `false` means "nothing arrived", which an attached connection uses to take another look at the
/// session's output; `true` means the next [`protocol::recv_frame`] will not wait for its first byte.
fn wait_readable(fd: RawFd, timeout: Option<Duration>) -> io::Result<bool> {
    let mut poll_fd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let millis = match timeout {
        None => -1,
        Some(timeout) => timeout.as_millis().min(i32::MAX as u128) as libc::c_int,
    };
    loop {
        let ready = unsafe { libc::poll(&mut poll_fd, 1, millis) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        return Ok(ready > 0);
    }
}

fn write_run(fd: RawFd, frames: &[Frame]) -> io::Result<()> {
    for frame in frames {
        let bytes =
            protocol::encode_frame(frame).map_err(|error| io::Error::other(error.to_string()))?;
        protocol::write_frame_bytes(fd, &bytes)?;
    }
    Ok(())
}

/// What the connection is attached to after a request was answered: the tap follows a successful
/// `ATTACH`, ends at `DETACH`, and is advanced by a `REPLAY` so the same bytes are never sent twice.
fn next_attachment(
    current: Option<Attachment>,
    request: &Frame,
    replies: &[Frame],
) -> Option<Attachment> {
    let Some(reply) = replies.first() else {
        return current;
    };
    if reply.kind == Kind::Error {
        return current;
    }
    match request.kind {
        Kind::Attach if reply.kind == Kind::AttachAck => {
            // A peer that did not ask for live output gets its one answer and nothing more: the
            // current daemon client reads exactly one frame per request and correlates strictly by
            // request id, so an unsolicited `OUTPUT` would be read as the next verb's reply.
            if reply.payload.get("liveOutput").and_then(|value| value.as_bool()) != Some(true) {
                return None;
            }
            // A run's end frame reports how far its page actually reached; the ack's
            // `sequenceOffset` is the session head, which a paged run has not delivered yet. Taking
            // the head would leave the unsent remainder behind the cursor for good.
            let delivered = replies
                .iter()
                .rev()
                .find(|frame| frame.kind == Kind::ReplayEnd)
                .and_then(|frame| frame.payload.get("sequenceOffset"))
                .and_then(|value| value.as_u64());
            // An inline ack delivered the page it copied in its own frame, so the cursor is where
            // that page ended: `afterSequence + dataBytes`. Reading `sequenceOffset` (the session
            // head) instead claims bytes the page never carried whenever it was capped, and the
            // unsent remainder would sit behind the cursor for the connection's life.
            let sent = reply
                .payload
                .get("replayGap")
                .and_then(|gap| gap.get("availableFromSequence"))
                .and_then(|value| value.as_u64())
                .or(delivered)
                .or_else(|| {
                    let from = reply.payload.get("afterSequence")?.as_u64()?;
                    let bytes = reply.payload.get("dataBytes")?.as_u64()?;
                    Some(from.saturating_add(bytes))
                })?;
            Some(Attachment {
                session_id: reply.payload.get("sessionId")?.as_str()?.to_owned(),
                epoch: reply.payload.get("epoch")?.as_u64()?,
                sent,
            })
        }
        Kind::Detach if reply.kind == Kind::Detach => None,
        Kind::Replay => {
            let mut tap = current?;
            let end = replies.last()?;
            if end.payload.get("sessionId").and_then(|value| value.as_str()) != Some(&tap.session_id)
            {
                return Some(tap);
            }
            let resume = end
                .payload
                .get("sequenceOffset")
                .or_else(|| end.payload.get("offset"))
                .and_then(|value| value.as_u64());
            if let Some(resume) = resume {
                tap.sent = tap.sent.max(resume);
            }
            Some(tap)
        }
        _ => current,
    }
}

/// Wire bytes ready to write, with the budget charge that covers them held until this value drops.
///
/// Dropping releases the charge, so every path that stops writing - a completed write, a failed
/// one, a cancelled connection - returns the bytes to the cap exactly once, after the socket call
/// has returned. Releasing it by hand before the write is what lets a slow peer hold memory the
/// ledger has already spent.
struct ChargedWrite {
    frames: Vec<Vec<u8>>,
    /// Held only for its `Drop`, which releases the charge after the socket write. Never read, hence
    /// the underscore: deleting it would drop the charge before the frames reach the peer.
    _charge: Charge,
}

/// The chunked uploads this connection opened and has not committed yet.
///
/// A `CHECKPOINT_INSTALL` without data charges the upload's declared size before a byte is accepted,
/// and only a commit, an abort or the session's death gives it back. A peer that disconnects
/// mid-upload never sends either of the first two, so the connection owns the abort: dropping this
/// on the way out of the loop releases every charge the upload is still holding. Without it a daemon
/// that crashed once per attempt would strand budget a little at a time until the keeper refused
/// work for a reason no peer could see.
struct OpenUploads<'a> {
    keeper: &'a Keeper,
    ids: Vec<OpenUpload>,
}

/// One chunked upload this connection opened, with the epoch it was opened at and the token the
/// `CHECKPOINT_ACK` returned.
///
/// Both are needed on the abort. The epoch is the fence (a teardown from a superseded daemon must not
/// reach a live session's store at all), and the token is the identity: two connections on one epoch
/// can begin the same `checkpoint_id`, so only the token says which attempt this one is. Without it,
/// a disconnect aborted whichever attempt was live - a successor's, when a retry had replaced this
/// connection's own.
struct OpenUpload {
    session_id: String,
    checkpoint_id: String,
    epoch: u64,
    token: u64,
}

impl<'a> OpenUploads<'a> {
    fn new(keeper: &'a Keeper) -> Self {
        Self { keeper, ids: Vec::new() }
    }

    /// Follows one answered request: an install that only opens an upload is recorded, and an
    /// install that carries its data or a commit settles it. An error settles nothing, because a
    /// refused verb left the keeper's state as it found it.
    fn track(&mut self, request: &Frame, reply: &Reply) {
        if reply.frames().first().map(|frame| frame.kind) == Some(Kind::Error) {
            return;
        }
        let Some(session_id) = request.payload.get("sessionId").and_then(|value| value.as_str())
        else {
            return;
        };
        let Some(checkpoint_id) = request
            .payload
            .get("checkpointId")
            .and_then(|value| value.as_str())
        else {
            return;
        };
        match request.kind {
            // A `data` that is absent *or* null opens a chunked upload, so both have to be recognised
            // here. Testing only for absence made a null-carrying install look like a commit, and the
            // connection then never recorded the upload it had just opened - so its charge survived
            // the disconnect and only the session's death returned it.
            Kind::CheckpointInstall
                if request
                    .payload
                    .get("data")
                    .is_none_or(serde_json::Value::is_null) =>
            {
                self.settle(session_id, checkpoint_id);
                // The epoch the install presented is the upload's, and answering it succeeded, so it
                // is the newest one in force for the session. The token comes from the reply, because
                // only the store issued it.
                let Some(epoch) = request.payload.get("epoch").and_then(|value| value.as_u64()) else {
                    return;
                };
                let Some(token) = reply
                    .frames()
                    .first()
                    .and_then(|frame| frame.payload.get("uploadToken"))
                    .and_then(|value| value.as_u64())
                else {
                    return;
                };
                self.ids.push(OpenUpload {
                    session_id: session_id.to_owned(),
                    checkpoint_id: checkpoint_id.to_owned(),
                    epoch,
                    token,
                });
            }
            Kind::CheckpointInstall | Kind::CheckpointCommit => {
                self.settle(session_id, checkpoint_id);
            }
            _ => {}
        }
    }

    fn settle(&mut self, session_id: &str, checkpoint_id: &str) {
        self.ids
            .retain(|open| open.session_id != session_id || open.checkpoint_id != checkpoint_id);
    }
}

impl Drop for OpenUploads<'_> {
    fn drop(&mut self) {
        for open in self.ids.drain(..) {
            self.keeper.abort_checkpoint(
                &open.session_id,
                &open.checkpoint_id,
                open.epoch,
                open.token,
            );
        }
    }
}

impl ChargedWrite {
    fn write_to(&self, fd: RawFd) -> io::Result<()> {
        for frame in &self.frames {
            protocol::write_frame_bytes(fd, frame)?;
        }
        Ok(())
    }
}

/// The encoded `OUTPUT` frames for everything the session produced since the tap's cursor.
///
/// The cursor only moves over bytes that are actually in the returned frames, so a page that does not
/// fit one frame is delayed, never dropped. A peer that has been fenced off by a newer attachment is
/// told so and the tap ends: an old daemon must not keep reading a session it lost.
fn output_run(keeper: &Keeper, tap: &mut Attachment) -> KeeperResult<Option<ChargedWrite>> {
    fn encode(frame: &Frame) -> KeeperResult<Vec<u8>> {
        protocol::encode_frame(frame).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::FrameTooLarge,
                format!("could not encode an output frame: {error}"),
            )
        })
    }

    // Cheapest possible idle check: measuring the suffix costs one registry lookup, while copying it
    // costs its whole length, and an attached peer is idle for most of a session's life. `None` is a
    // gap, which the bounded replay below reports without copying anything.
    if keeper.page_len(&tap.session_id, tap.sent)? == Some(0) {
        return Ok(None);
    }
    // Charged at the chunk cap before the copy, so the copy itself is covered even though its real
    // size is only known once it exists.
    let charge = charge_page(
        keeper,
        OUTPUT_CHUNK_BYTES,
        1,
        KeeperErrorCode::MemoryBudgetExhausted,
        format!(
            "keeper cannot buffer an output frame for session {}, replay from {}",
            tap.session_id, tap.sent
        ),
    )?;
    // Read through the tap's own epoch, so a peer that has been fenced off is refused instead of
    // having a successor's output copied for it: the copy and the epoch check are one step, which a
    // post-copy `newest_epoch` comparison could not be.
    let outcome =
        keeper.replay_fenced_bounded(&tap.session_id, tap.sent, tap.epoch, OUTPUT_CHUNK_BYTES)?;
    if let Some(gap) = outcome.replay_gap {
        tap.sent = gap.resumed_at_offset;
        let frame = Frame::new(
            Kind::ReplayGapEvent,
            protocol::NO_REQUEST_ID,
            0,
            serde_json::json!({
                "sessionId": tap.session_id,
                "code": KeeperErrorCode::ReplayGap.as_str(),
                "offset": gap.resumed_at_offset,
                "detail": gap_payload(gap),
            }),
        );
        // A gap event is metadata, not a copy of session output, so it is written uncharged.
        drop(charge);
        let bytes = encode(&frame)?;
        return Ok(Some(ChargedWrite {
            frames: vec![bytes],
            _charge: Charge::empty(keeper.budget_handle()),
        }));
    }
    let head = outcome.sequence_offset;
    let first = head.saturating_sub(outcome.data.len() as u64);
    let skip = tap.sent.saturating_sub(first);
    if skip >= outcome.data.len() as u64 {
        return Ok(None);
    }
    let rest = &outcome.data[skip as usize..];
    let take = rest.len().min(OUTPUT_CHUNK_BYTES);
    let at = tap.sent;
    let frame = Frame::new(
        Kind::Output,
        protocol::NO_REQUEST_ID,
        0,
        serde_json::json!({
            "sessionId": tap.session_id,
            "epoch": tap.epoch,
            "sequenceOffset": at,
            "data": base64_encode(&rest[..take]),
        }),
    );
    let bytes = encode(&frame)?;
    tap.sent = at.saturating_add(take as u64);
    Ok(Some(ChargedWrite { frames: vec![bytes], _charge: charge }))
}

fn serve_connection(
    state: Arc<ServerState>,
    stream: UnixStream,
) -> io::Result<()> {
    let fd = stream.as_raw_fd();
    protocol::set_send_timeout(fd, SEND_TIMEOUT)?;
    // A peer that sends a header and then stalls must not hold this connection -- and its slot --
    // until the process exits. The timeout only ever fires mid-frame, because the loop polls first.
    stream.set_read_timeout(Some(SEND_TIMEOUT))?;

    let mut tap: Option<Attachment> = None;
    let mut uploads = OpenUploads::new(&state.keeper);
    loop {
        let ready = wait_readable(fd, tap.as_ref().map(|_| OUTPUT_POLL))?;
        if !ready {
            let Some(active) = tap.as_mut() else {
                continue;
            };
            match output_run(&state.keeper, active) {
                // The charge is owned by the value being written, so it is still held while the
                // socket call blocks and is returned only once that call has finished - on success
                // and on failure alike. `written` is dropped by `continue` in either case.
                Ok(Some(written)) => {
                    if written.write_to(fd).is_err() {
                        return Ok(());
                    }
                }
                Ok(None) => {}
                // The tap is refused on its own channel -- a fenced epoch, a lost range, an
                // unaffordable copy -- and ends, so the peer learns it must re-attach.
                Err(error) => {
                    let refused = error_frame(protocol::NO_REQUEST_ID, 0, error);
                    tap = None;
                    if write_run(fd, &[refused]).is_err() {
                        return Ok(());
                    }
                }
            }
            continue;
        }
        let frame = match protocol::recv_frame(fd) {
            Ok(frame) => frame,
            // A closed peer, a bad header and an oversize payload all end this connection; the
            // keeper itself is unaffected and keeps serving the others.
            Err(_) => return Ok(()),
        };
        let reply = dispatch(&state, &frame);
        tap = next_attachment(tap, &frame, reply.frames());
        uploads.track(&frame, &reply);
        // The reply owns its charge, so it is held for as long as the frames are being written and
        // returned by `reply`'s drop after the last socket call - whether that call succeeded or not.
        let written = write_run(fd, reply.frames());
        drop(reply);
        if written.is_err() {
            return Ok(());
        }
    }
}

/// Answers a peer that arrived while every connection slot was taken, then closes it.
fn refuse_over_limit(stream: &UnixStream) {
    let fd = stream.as_raw_fd();
    let _ = protocol::set_send_timeout(fd, SEND_TIMEOUT);
    let frame = error_frame(
        protocol::NO_REQUEST_ID,
        0,
        KeeperError::new(
            KeeperErrorCode::TooManyConnections,
            format!(
                "this keeper serves at most {} connections at a time",
                protocol::MAX_CONNECTIONS
            ),
        ),
    );
    if let Ok(bytes) = protocol::encode_frame(&frame) {
        let _ = protocol::write_frame_bytes(fd, &bytes);
    }
}

/// Accept connections until the listener fails.
///
/// `endpoint` carries the uid and the peer-check the listener was bound for; it is what
/// [`Endpoint::verify_peer`] is asked, so the check cannot disagree with the socket's owner.
pub fn serve(
    listener: UnixListener,
    state: Arc<ServerState>,
    endpoint: Endpoint,
) -> io::Result<()> {
    let endpoint = Arc::new(endpoint);
    let limiter = ConnectionLimiter::new(protocol::MAX_CONNECTIONS);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if !authorize(&endpoint, &state, stream.as_raw_fd()) {
                    continue;
                }
                let Some(slot) = limiter.try_acquire() else {
                    refuse_over_limit(&stream);
                    continue;
                };
                // The worker gets its own descriptor so a spawn failure can still be answered on
                // this one instead of the peer seeing a bare disconnect.
                let worker = match stream.try_clone() {
                    Ok(worker) => worker,
                    Err(_) => {
                        refuse_over_limit(&stream);
                        continue;
                    }
                };
                let state = Arc::clone(&state);
                let spawned = std::thread::Builder::new()
                    .name("ferryx-ptykeeper-connection".to_owned())
                    .spawn(move || {
                        let _slot = slot;
                        // A connection thread's failure is that connection's, never the keeper's.
                        let _ = serve_connection(state, worker);
                    });
                if spawned.is_err() {
                    refuse_over_limit(&stream);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ServerState {
        ServerState::new(
            Arc::new(Keeper::with_default_budget("test-generation")),
            unsafe { libc::getuid() },
        )
    }

    /// A state whose budget is `total` bytes with the replay sub-cap at `replay`, so a dispatch can
    /// be made to fail on a cap rather than on a real PTY.
    fn state_with_budget(total: usize, replay: usize) -> ServerState {
        let budget = crate::budget::MemoryBudget::with_limits(total, replay);
        ServerState::new(
            Arc::new(Keeper::new(
                "test-generation",
                Arc::new(std::sync::Mutex::new(budget)),
            )),
            unsafe { libc::getuid() },
        )
    }

    fn frame(kind: Kind, seq: u64, payload: serde_json::Value) -> Frame {
        Frame::new(kind, format!("req-{seq:0>32}"), seq, payload)
    }

    fn only(state: &ServerState, frame: &Frame) -> Frame {
        let mut frames = dispatch(state, frame).into_frames();
        assert_eq!(frames.len(), 1, "expected exactly one reply frame");
        frames.remove(0)
    }

    #[test]
    fn a_replay_page_the_budget_cannot_cover_is_a_typed_code_not_a_message_prefix() {
        // Goal 2's wire half: an over-budget page must answer `ERROR` with a code a caller can
        // dispatch on. The budget is one session reservation plus a little, so the spawn fits and the
        // page charge (about 1.75 MiB for 512 KiB of bytes, base64 and frame overhead included) does
        // not. If the code were only a word in the message, this assertion would have to grep prose.
        let reservation = crate::keeper::SESSION_RESERVATION_BYTES as usize;
        let state = state_with_budget(reservation + 64 * 1024, reservation);
        let spawned = only(
            &state,
            &frame(
                Kind::Spawn,
                1,
                serde_json::json!({
                    "sessionId": "s1",
                    "requestId": "request-s1",
                    "argv": ["/bin/cat"],
                    "cols": 80,
                    "rows": 24,
                    "epoch": 1,
                }),
            ),
        );
        assert_eq!(spawned.kind, Kind::SpawnAck, "{:?}", spawned.payload);
        let used_after_spawn = state.keeper.budget().used();
        assert_eq!(used_after_spawn, reservation as u64);

        let refused = only(
            &state,
            &frame(
                Kind::Replay,
                2,
                serde_json::json!({ "sessionId": "s1", "fromSequence": 0 }),
            ),
        );
        assert_eq!(refused.kind, Kind::Error, "{:?}", refused.payload);
        assert_eq!(
            refused.payload["code"], "MEMORY_BUDGET_EXHAUSTED",
            "the refusal travels as a code, not as a sentence to be matched: {:?}",
            refused.payload
        );
        assert_eq!(
            state.keeper.budget().used(),
            used_after_spawn,
            "a refused page charges nothing and releases nothing"
        );
        state.keeper.kill("s1").expect("kill");
        assert_eq!(state.keeper.budget().used(), 0);
    }

    #[test]
    fn base64_matches_the_reference_for_padding_cases() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_decodes_every_padding_case_and_round_trips_opaque_bytes() {
        for (text, bytes) in [
            ("", &b""[..]),
            ("Zg==", &b"f"[..]),
            ("Zm8=", &b"fo"[..]),
            ("Zm9v", &b"foo"[..]),
            ("Zm9vYg==", &b"foob"[..]),
            ("Zm9vYmE=", &b"fooba"[..]),
            ("Zm9vYmFy", &b"foobar"[..]),
        ] {
            assert_eq!(base64_decode(text).expect("decode"), bytes);
        }

        // Every byte value, including the ones that exercise each 6-bit group.
        let all: Vec<u8> = (0u8..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)).expect("round trip"), all);
        assert_eq!(
            base64_decode(&base64_encode(b"\x1b[0m\xe4\xb8\xad")).expect("round trip"),
            &b"\x1b[0m\xe4\xb8\xad"[..]
        );
    }

    #[test]
    fn malformed_base64_is_refused_rather_than_truncated() {
        for text in ["Zg=", "Z===", "Zm9vZ", "Zm$v", "Zg==Zg=="] {
            assert!(base64_decode(text).is_err(), "{text:?} must not decode");
        }
        assert!(base64_decode("Zg=a").is_err(), "padding must be at the end");
    }

    #[test]
    fn hello_answers_with_the_protocol_and_keeper_version() {
        let reply = only(&state(), &frame(Kind::Hello, 1, serde_json::json!({})));
        assert_eq!(reply.kind, Kind::HelloAck);
        assert_eq!(reply.payload["protocolMajor"], crate::PROTOCOL_MAJOR);
        assert_eq!(reply.payload["protocolMinor"], crate::PROTOCOL_MINOR);
        assert_eq!(reply.payload["keeperVersion"], crate::KEEPER_VERSION);
        assert_eq!(reply.payload["pid"], std::process::id());
    }

    #[test]
    fn a_peer_speaking_another_protocol_major_is_refused_by_name() {
        let reply = only(
            &state(),
            &frame(
                Kind::Hello,
                1,
                serde_json::json!({ "protocolMajor": crate::PROTOCOL_MAJOR + 1 }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "BAD_FRAME");
        assert!(reply.payload["message"]
            .as_str()
            .expect("message")
            .contains("protocol major"));
    }

    #[test]
    fn every_request_reply_carries_the_correlation_ids_of_its_request() {
        let state = state();
        let kinds = [
            Kind::Hello,
            Kind::Spawn,
            Kind::Write,
            Kind::Resize,
            Kind::Signal,
            Kind::Kill,
            Kind::Attach,
            Kind::Replay,
            Kind::Head,
            Kind::OwnershipQuery,
            Kind::CheckpointInstall,
            Kind::CheckpointChunk,
            Kind::CheckpointCommit,
            Kind::CheckpointQuery,
            Kind::Detach,
            Kind::Shutdown,
        ];
        for (index, kind) in kinds.iter().enumerate() {
            let seq = 100 + index as u64;
            let request = frame(*kind, seq, serde_json::json!({}));
            let frames = dispatch(&state, &request).into_frames();
            assert!(!frames.is_empty(), "{kind:?} must answer");
            for reply in frames {
                assert_eq!(reply.request_id, request.request_id, "{kind:?}");
                assert_eq!(reply.frame_sequence, seq, "{kind:?}");
            }
        }
    }

    #[test]
    fn an_unknown_session_attach_is_a_typed_error_frame_carrying_no_offset() {
        let reply = only(
            &state(),
            &frame(
                Kind::Attach,
                1,
                serde_json::json!({ "sessionId": "nope", "epoch": 0 }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "UNKNOWN_SESSION");
        assert!(reply.payload["message"].is_string());
        assert!(reply.payload["offset"].is_null());
    }

    #[test]
    fn every_session_verb_on_an_unknown_session_reports_unknown_session() {
        let state = state();
        for (kind, payload) in [
            (Kind::Attach, serde_json::json!({ "sessionId": "nope", "epoch": 1 })),
            (Kind::Head, serde_json::json!({ "sessionId": "nope" })),
            (Kind::Replay, serde_json::json!({ "sessionId": "nope" })),
            (Kind::Kill, serde_json::json!({ "sessionId": "nope", "epoch": 1 })),
            (Kind::Detach, serde_json::json!({ "sessionId": "nope", "epoch": 1 })),
            (
                Kind::Resize,
                serde_json::json!({ "sessionId": "nope", "epoch": 1, "cols": 80, "rows": 24 }),
            ),
            (
                Kind::Signal,
                serde_json::json!({ "sessionId": "nope", "epoch": 1, "signal": "interrupt" }),
            ),
            (
                Kind::Write,
                serde_json::json!({ "sessionId": "nope", "epoch": 1, "data": base64_encode(b"x") }),
            ),
            (Kind::CheckpointQuery, serde_json::json!({ "sessionId": "nope" })),
            (
                Kind::CheckpointFetch,
                serde_json::json!({
                    "sessionId": "nope",
                    "checkpointId": "c",
                    "sequenceOffset": 1,
                    "epoch": 1,
                }),
            ),
        ] {
            let reply = only(&state, &frame(kind, 3, payload));
            assert_eq!(reply.kind, Kind::Error, "{kind:?}");
            assert_eq!(reply.payload["code"], "UNKNOWN_SESSION", "{kind:?}");
        }
    }

    #[test]
    fn ownership_query_lists_the_keepers_registry_not_the_connections() {
        let reply = only(
            &state(),
            &frame(Kind::OwnershipQuery, 1, serde_json::json!({})),
        );
        assert_eq!(reply.kind, Kind::OwnershipList);
        assert_eq!(reply.payload["sessions"], serde_json::json!([]));
    }

    #[test]
    fn missing_correlation_fields_are_typed_bad_frames_not_panics() {
        let state = state();
        for (kind, payload, expected) in [
            (Kind::Attach, serde_json::json!({}), "sessionId"),
            (Kind::Write, serde_json::json!({ "sessionId": "s", "epoch": 1 }), "data"),
            (Kind::Resize, serde_json::json!({ "sessionId": "s", "epoch": 1 }), "cols"),
            (Kind::Signal, serde_json::json!({ "sessionId": "s", "epoch": 1 }), "signal"),
            (Kind::Spawn, serde_json::json!({}), "sessionId"),
            (
                Kind::CheckpointInstall,
                serde_json::json!({ "sessionId": "s", "epoch": 1 }),
                "checkpointId",
            ),
            (
                Kind::CheckpointInstall,
                serde_json::json!({ "sessionId": "s", "checkpointId": "c", "epoch": 1 }),
                "sequenceOffset",
            ),
            (
                Kind::CheckpointChunk,
                serde_json::json!({ "sessionId": "s", "checkpointId": "c", "epoch": 1 }),
                "chunkIndex",
            ),
            (Kind::Kill, serde_json::json!({ "sessionId": "s" }), "epoch"),
        ] {
            let reply = only(&state, &frame(kind, 9, payload));
            assert_eq!(reply.kind, Kind::Error, "{kind:?}");
            assert_eq!(reply.payload["code"], "BAD_FRAME", "{kind:?}");
            assert!(
                reply.payload["message"]
                    .as_str()
                    .expect("message")
                    .contains(expected),
                "{kind:?} must name the missing field"
            );
        }
    }

    #[test]
    fn a_signal_name_the_keeper_does_not_know_is_refused_without_reaching_the_session() {
        let reply = only(
            &state(),
            &frame(
                Kind::Signal,
                1,
                serde_json::json!({ "sessionId": "s1", "epoch": 1, "signal": "suspend" }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "BAD_FRAME");
        assert!(reply.payload["message"]
            .as_str()
            .expect("message")
            .contains("interrupt"));
    }

    #[test]
    fn a_spawn_without_an_argv_is_refused_before_a_pty_is_opened() {
        let reply = only(
            &state(),
            &frame(
                Kind::Spawn,
                1,
                serde_json::json!({ "sessionId": "s1", "cols": 80, "rows": 24 }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "INVALID_SPAWN");
    }

    #[test]
    fn a_keeper_originated_frame_is_named_rather_than_served() {
        let reply = only(
            &state(),
            &frame(Kind::Output, 1, serde_json::json!({})),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "BAD_FRAME");
        assert!(reply.payload["message"]
            .as_str()
            .expect("message")
            .contains("Output"));
    }

    #[test]
    fn gap_detail_names_the_requested_and_available_offsets() {
        let gap = crate::replay::ReplayGap::new(4, 40);
        let detail = gap_payload(gap);
        assert_eq!(detail["requestedAfterSequence"], 4);
        assert_eq!(detail["availableFromSequence"], 40);
    }

    #[test]
    fn env_accepts_both_the_object_and_pair_forms() {
        let object = serde_json::json!({ "env": { "TERM": "xterm-256color" } });
        assert_eq!(
            pairs(&object, "env").expect("object form"),
            vec![("TERM".to_string(), "xterm-256color".to_string())]
        );

        let rows = serde_json::json!({ "env": [["TERM", "xterm-256color"], ["LANG", "C"]] });
        assert_eq!(
            pairs(&rows, "env").expect("pair form"),
            vec![
                ("TERM".to_string(), "xterm-256color".to_string()),
                ("LANG".to_string(), "C".to_string()),
            ]
        );

        assert!(pairs(&serde_json::json!({}), "env").expect("absent").is_empty());
    }

    #[test]
    fn a_malformed_environment_or_argv_is_refused_by_name_instead_of_shortened() {
        for payload in [
            serde_json::json!({ "env": { "TERM": 1 } }),
            serde_json::json!({ "env": { "TERM": null } }),
            serde_json::json!({ "env": [["TERM"]] }),
            serde_json::json!({ "env": [["TERM", "x", "extra"]] }),
            serde_json::json!({ "env": [[1, "x"]] }),
            serde_json::json!({ "env": "TERM=x" }),
        ] {
            let error = pairs(&payload, "env").expect_err("malformed env must be refused");
            assert_eq!(error.code, KeeperErrorCode::BadFrame);
        }
        for payload in [
            serde_json::json!({ "argv": ["/bin/cat", 1] }),
            serde_json::json!({ "argv": "/bin/cat" }),
        ] {
            let error = argv(&payload, "argv").expect_err("malformed argv must be refused");
            assert_eq!(error.code, KeeperErrorCode::BadFrame);
        }
    }

    #[test]
    fn a_data_field_that_is_not_base64_text_is_a_bad_frame_not_an_absent_field() {
        for payload in [
            serde_json::json!({ "data": 1 }),
            serde_json::json!({ "data": ["Zm9v"] }),
            serde_json::json!({ "data": "not base64!" }),
        ] {
            let error = bytes_at(&payload, "data").expect_err("malformed data must be refused");
            assert_eq!(error.code, KeeperErrorCode::BadFrame);
        }
        assert!(bytes_at(&serde_json::json!({}), "data")
            .expect("absent")
            .is_none());
        assert!(bytes_at(&serde_json::json!({ "data": null }), "data")
            .expect("null")
            .is_none());
    }

    #[test]
    fn a_terminal_size_outside_the_u16_range_is_refused_rather_than_truncated() {
        let reply = only(
            &state(),
            &frame(
                Kind::Spawn,
                1,
                serde_json::json!({
                    "sessionId": "s1",
                    "argv": ["/bin/cat"],
                    "cols": 70000,
                    "rows": 24,
                }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "BAD_FRAME");
        assert!(reply.payload["message"]
            .as_str()
            .expect("message")
            .contains("cols"));
    }

    #[test]
    fn a_connection_slot_is_returned_when_the_guard_is_dropped() {
        let limiter = ConnectionLimiter::new(2);
        assert_eq!(limiter.live(), 0);
        let first = limiter.try_acquire().expect("first slot");
        let second = limiter.try_acquire().expect("second slot");
        assert_eq!(limiter.live(), 2);
        assert!(limiter.try_acquire().is_none(), "the limit must hold");
        assert_eq!(limiter.live(), 2, "a refusal must not consume a slot");

        drop(first);
        assert_eq!(limiter.live(), 1);
        let third = limiter.try_acquire().expect("room after a disconnect");
        drop(second);
        drop(third);
        assert_eq!(limiter.live(), 0);
    }

    #[test]
    fn the_limiter_never_exceeds_its_limit_under_concurrent_admission() {
        let limiter = Arc::new(ConnectionLimiter::new(4));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let limiter = Arc::clone(&limiter);
            let peak = Arc::clone(&peak);
            workers.push(std::thread::spawn(move || {
                for _ in 0..64 {
                    let Some(slot) = limiter.try_acquire() else {
                        continue;
                    };
                    peak.fetch_max(limiter.live(), Ordering::SeqCst);
                    drop(slot);
                }
            }));
        }
        for worker in workers {
            worker.join().expect("worker");
        }
        let peak = peak.load(Ordering::SeqCst);
        assert!(peak <= 4, "peak {peak} exceeded the limit");
        assert_eq!(limiter.live(), 0);
    }

    #[test]
    fn replay_chunk_offsets_resume_exactly_where_the_previous_chunk_ended() {
        let data = vec![b'a'; REPLAY_CHUNK_BYTES * 2 + 7];
        // The cursor is what the reader asked for, and the head is where the suffix ends; the page is
        // the bytes between them. In production `from` comes from the request and `data.len()` is the
        // suffix the keeper copied from it, so the page ends at the head.
        let from = 1000u64;
        let head = from + data.len() as u64;
        let frames = replay_run("req", 1, "s1", head, from, false, &data);
        assert_eq!(frames.last().expect("end").kind, Kind::ReplayEnd);
        assert!(frames.len() <= protocol::MAX_INFLIGHT_FRAMES);

        let first = frames[0].payload["sequenceOffset"].as_u64().expect("offset");
        assert_eq!(first, from, "the run starts at the cursor the reader asked for");
        let mut cursor = first;
        for frame in &frames {
            if frame.kind != Kind::ReplayChunk {
                continue;
            }
            assert_eq!(frame.payload["sequenceOffset"].as_u64(), Some(cursor));
            let chunk =
                base64_decode(frame.payload["data"].as_str().expect("data")).expect("base64");
            assert!(chunk.len() <= REPLAY_CHUNK_BYTES);
            cursor += chunk.len() as u64;
        }
        let end = frames.last().expect("end");
        assert_eq!(end.payload["sequenceOffset"].as_u64(), Some(head));
        assert_eq!(end.payload["head"].as_u64(), Some(head));
        assert_eq!(end.payload["more"], false);
    }

    #[test]
    fn a_suffix_larger_than_one_page_is_split_into_resumable_runs() {
        let total = RESPONSE_BYTES_PER_REQUEST * 2 + 100;
        let data: Vec<u8> = (0..total).map(|index| (index % 251) as u8).collect();
        let head = total as u64;
        let mut cursor = 0u64;
        let mut pages = 0;
        let mut reassembled = Vec::new();

        loop {
            // Each request resumes at the cursor the previous page reported, and the keeper answers
            // with the suffix from there; `head` is where that suffix ends, not where the page starts.
            // `more` is the keeper's answer about the backlog, which is what a reader loops on.
            let suffix = &data[cursor as usize..];
            let more = suffix.len() > RESPONSE_BYTES_PER_REQUEST;
            let frames = replay_run("req", 1, "s1", head, cursor, more, suffix);
            assert!(
                frames.len() <= protocol::MAX_INFLIGHT_FRAMES,
                "a page must stay inside the in-flight allowance"
            );
            let mut page_bytes = 0usize;
            for frame in &frames {
                if frame.kind != Kind::ReplayChunk {
                    continue;
                }
                assert_eq!(frame.payload["sequenceOffset"].as_u64(), Some(cursor));
                let chunk =
                    base64_decode(frame.payload["data"].as_str().expect("data")).expect("base64");
                page_bytes += chunk.len();
                assert!(
                    page_bytes <= RESPONSE_BYTES_PER_REQUEST,
                    "a page must not exceed its byte cap"
                );
                reassembled.extend_from_slice(&chunk);
                cursor += chunk.len() as u64;
            }
            pages += 1;
            let end = frames.last().expect("end");
            assert_eq!(end.kind, Kind::ReplayEnd);
            assert_eq!(end.payload["sequenceOffset"].as_u64(), Some(cursor));
            if end.payload["more"] == serde_json::json!(false) {
                assert_eq!(end.payload["head"].as_u64(), Some(head));
                break;
            }
            assert_eq!(end.payload["more"], serde_json::json!(true));
            assert!(pages < 16, "paging must terminate");
        }

        assert_eq!(pages, 3, "two full pages and a remainder");
        assert_eq!(reassembled, data, "paging must not drop or duplicate bytes");
    }

    /// A page that does not reach the head must label its bytes from the cursor, not from the head.
    ///
    /// This is what a partial `REPLAY` of a long backlog answers with: the keeper copies a bounded
    /// suffix, so `data.len()` is shorter than `head - from`. Deriving the first offset as
    /// `head - data.len()` then relabels every byte by the shortfall and reports an end past what was
    /// sent, so a reader paging through the backlog resumes above bytes it never received.
    #[test]
    fn a_page_that_stops_short_of_the_head_labels_its_bytes_from_the_cursor() {
        let from = 500u64;
        // A 3 KiB backlog answered with a 1 KiB page: the classic partial replay of a busy session.
        let backlog = vec![b'x'; 3000];
        let page_data = &backlog[..1000];
        let head = from + backlog.len() as u64;

        let frames = replay_run("req", 1, "s1", head, from, true, page_data);
        let first = frames[0].payload["sequenceOffset"].as_u64().expect("offset");
        assert_eq!(first, from, "the page starts at the cursor, not at `head - len`");

        let end = frames.last().expect("end");
        assert_eq!(end.kind, Kind::ReplayEnd);
        assert_eq!(
            end.payload["sequenceOffset"].as_u64(),
            Some(from + 1000),
            "the run ends where its own bytes end, so the next page resumes there"
        );
        assert_eq!(end.payload["head"].as_u64(), Some(head), "the head is still reported");
        assert_eq!(end.payload["more"], serde_json::json!(true), "the backlog continues");
    }

    #[test]
    fn a_peer_that_stops_reading_is_dropped_without_taking_the_process_down() {
        let (sent, received) = UnixStream::pair().expect("socketpair");
        drop(received);
        let before = crate::protocol::sigpipe_disposition();
        assert_ne!(before, usize::MAX, "the disposition must be readable");
        let error = protocol::write_frame_bytes(
            sent.as_raw_fd(),
            &protocol::encode_frame(&frame(Kind::HeadAck, 1, serde_json::json!({})))
                .expect("encode"),
        )
        .expect_err("a closed peer must fail the write");
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
            ),
            "expected EPIPE, got {error:?}"
        );
        assert_eq!(
            crate::protocol::sigpipe_disposition(),
            before,
            "the socket must not change the process-wide SIGPIPE disposition"
        );
    }

    fn keeper_with_a_checkpoint(bytes: &[u8], offset: u64) -> (Arc<Keeper>, String) {
        let (keeper, session_id) = spawned_keeper();
        keeper
            .install_checkpoint(&session_id, "cp-1", offset, bytes.len() as u64, bytes.to_vec(), 1)
            .expect("install");
        (keeper, session_id)
    }

    /// A keeper owning one live session under `s1`, with no checkpoint.
    fn spawned_keeper() -> (Arc<Keeper>, String) {
        let keeper = Arc::new(Keeper::with_default_budget("test-generation"));
        let session_id = "s1".to_owned();
        keeper
            .spawn(SpawnRequest {
                session_id: session_id.clone(),
                request_id: "request-s1".to_owned(),
                argv: vec!["/bin/cat".to_owned()],
                env: Vec::new(),
                cwd: None,
                cols: 80,
                rows: 24,
                epoch: 1,
            })
            .expect("spawn");
        (keeper, session_id)
    }

    #[test]
    fn a_checkpoint_fetch_pages_the_committed_bytes_with_their_identity() {
        let payload: Vec<u8> = (0..CHECKPOINT_FETCH_CHUNK_BYTES * 2 + 13)
            .map(|index| (index % 253) as u8)
            .collect();
        let (keeper, session_id) = keeper_with_a_checkpoint(&payload, 7);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });

        let mut offset = 0u64;
        let mut reassembled = Vec::new();
        loop {
            let frames = dispatch(
                &state,
                &frame(
                    Kind::CheckpointFetch,
                    4,
                    serde_json::json!({
                        "sessionId": session_id,
                        "checkpointId": "cp-1",
                        "sequenceOffset": 7,
                        "epoch": 1,
                        "fromByte": offset,
                    }),
                ),
            )
            .into_frames();
            let end = frames.last().expect("end frame");
            assert_eq!(end.kind, Kind::CheckpointFetchEnd, "{end:?}");

            for chunk in &frames[..frames.len() - 1] {
                assert_eq!(chunk.kind, Kind::CheckpointFetchChunk);
                assert_eq!(chunk.payload["checkpointId"], "cp-1");
                assert_eq!(chunk.payload["sequenceOffset"], 7);
                assert_eq!(chunk.payload["totalBytes"], payload.len() as u64);
                assert_eq!(
                    chunk.payload["fromByte"].as_u64(),
                    Some(reassembled.len() as u64)
                );
                let bytes = base64_decode(chunk.payload["data"].as_str().expect("data"))
                    .expect("base64");
                assert!(bytes.len() <= CHECKPOINT_FETCH_CHUNK_BYTES);
                reassembled.extend_from_slice(&bytes);
            }
            offset = end.payload["nextByte"].as_u64().expect("nextByte");
            if end.payload["complete"] == serde_json::json!(true) {
                break;
            }
            assert_eq!(end.payload["complete"], serde_json::json!(false));
        }

        assert_eq!(reassembled, payload, "every committed byte must arrive once");
        assert_eq!(offset, payload.len() as u64);
        keeper.kill(&session_id).expect("kill");
    }

    #[test]
    fn a_fetch_for_a_replaced_checkpoint_is_stale_rather_than_mixing_two_commits() {
        let (keeper, session_id) = keeper_with_a_checkpoint(b"first", 5);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        keeper
            .install_checkpoint(&session_id, "cp-2", 9, 6, b"second".to_vec(), 1)
            .expect("second install");

        let reply = only(
            &state,
            &frame(
                Kind::CheckpointFetch,
                4,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-1",
                    "sequenceOffset": 5,
                    "epoch": 1,
                }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "STALE_CHECKPOINT");
        assert_eq!(reply.payload["offset"], 9, "the committed offset is reported");
        keeper.kill(&session_id).expect("kill");
    }

    #[test]
    fn a_fetch_past_the_checkpoint_end_is_a_bad_frame_naming_the_size() {
        let (keeper, session_id) = keeper_with_a_checkpoint(b"tiny", 3);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        let reply = only(
            &state,
            &frame(
                Kind::CheckpointFetch,
                4,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-1",
                    "sequenceOffset": 3,
                    "epoch": 1,
                    "fromByte": 99,
                }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "BAD_FRAME");
        assert!(reply.payload["message"]
            .as_str()
            .expect("message")
            .contains("4 bytes"));
        keeper.kill(&session_id).expect("kill");
    }

    #[test]
    fn a_destructive_verb_presenting_a_stale_epoch_is_fenced_off() {
        let (keeper, session_id) = keeper_with_a_checkpoint(b"x", 1);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        assert_eq!(keeper.newest_epoch(&session_id), Some(1));

        for (kind, extra) in [
            (Kind::Kill, serde_json::json!({})),
            (
                Kind::CheckpointCommit,
                serde_json::json!({ "checkpointId": "cp-1", "sequenceOffset": 2 }),
            ),
            (
                Kind::CheckpointFetch,
                serde_json::json!({
                    "checkpointId": "cp-1",
                    "sequenceOffset": 1,
                    "fromByte": 0,
                }),
            ),
        ] {
            let mut payload = serde_json::json!({ "sessionId": session_id, "epoch": 0 });
            if let (Some(target), Some(source)) = (payload.as_object_mut(), extra.as_object()) {
                target.extend(source.clone());
            }
            let reply = only(&state, &frame(kind, 5, payload));
            assert_eq!(reply.kind, Kind::Error, "{kind:?}");
            assert_eq!(reply.payload["code"], "EPOCH_MISMATCH", "{kind:?}");
        }
        assert!(keeper.contains(&session_id), "a fenced kill must not kill");
        keeper.kill(&session_id).expect("kill");
    }

    #[test]
    fn an_attach_without_live_output_arms_no_tap() {
        let (keeper, session_id) = keeper_with_a_checkpoint(b"x", 1);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        // A fresh epoch above the spawn's: `ATTACH` fences, and the fence refuses anything at or
        // below the newest epoch it has seen (`EpochFence::fence`), so attaching at the spawn's own
        // epoch 1 is `EPOCH_MISMATCH` - which is what the daemon client avoids by minting a new
        // epoch per attach (`keeper_session.rs`'s "newer than the spawn's").
        let request = frame(
            Kind::Attach,
            1,
            serde_json::json!({ "sessionId": session_id, "epoch": 2 }),
        );
        let replies = dispatch(&state, &request).into_frames();
        assert_eq!(replies[0].kind, Kind::AttachAck);
        assert_eq!(replies[0].payload["liveOutput"], serde_json::json!(false));
        assert!(
            next_attachment(None, &request, &replies).is_none(),
            "a peer that did not opt in must not be sent unsolicited frames"
        );
    }

    #[test]
    fn an_attach_that_opts_into_live_output_arms_a_tap_at_the_ack_head() {
        let (keeper, session_id) = keeper_with_a_checkpoint(b"x", 1);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        // Epoch 2 for the same reason as the test above: the attach must be strictly newer than the
        // spawn's epoch, or the fence answers `EPOCH_MISMATCH` and no tap is armed at all.
        let request = frame(
            Kind::Attach,
            1,
            serde_json::json!({ "sessionId": session_id, "epoch": 2, "liveOutput": true }),
        );
        let replies = dispatch(&state, &request).into_frames();
        assert_eq!(replies[0].payload["liveOutput"], serde_json::json!(true));
        let tap = next_attachment(None, &request, &replies).expect("tap armed");
        assert_eq!(tap.session_id, session_id);
        assert_eq!(tap.epoch, 2, "the ack's epoch, which the tap then fences every read with");
        assert_eq!(tap.sent, replies[0].payload["sequenceOffset"].as_u64().expect("head"));
    }

    #[test]
    fn a_chunked_attach_starts_the_tap_where_the_last_page_ended_not_at_the_head() {
        let ack = Frame::new(
            Kind::AttachAck,
            "req",
            1,
            serde_json::json!({
                "sessionId": "s1",
                "epoch": 1,
                "sequenceOffset": 900,
                "dataChunked": true,
                "liveOutput": true,
            }),
        );
        let end = Frame::new(
            Kind::ReplayEnd,
            "req",
            1,
            serde_json::json!({
                "sessionId": "s1",
                "sequenceOffset": 893,
                "head": 900,
                "more": true,
            }),
        );
        let request = frame(
            Kind::Attach,
            1,
            serde_json::json!({ "sessionId": "s1", "epoch": 1, "chunked": true, "liveOutput": true }),
        );
        let tap = next_attachment(None, &request, &[ack, end]).expect("tap armed");
        assert_eq!(
            tap.sent, 893,
            "the undelivered page remainder must not sit behind the cursor"
        );
    }

    #[test]
    fn a_lossy_attach_starts_the_tap_at_the_resume_point_so_the_retained_suffix_is_resent() {
        let ack = Frame::new(
            Kind::AttachAck,
            "req",
            1,
            serde_json::json!({
                "sessionId": "s1",
                "epoch": 1,
                "sequenceOffset": 900,
                "liveOutput": true,
                "replayGap": {
                    "requestedAfterSequence": 10,
                    "availableFromSequence": 800,
                    "retainedFromSequence": 800,
                },
            }),
        );
        let request = frame(
            Kind::Attach,
            1,
            serde_json::json!({ "sessionId": "s1", "epoch": 1, "afterSequence": 10, "liveOutput": true }),
        );
        let tap = next_attachment(None, &request, &[ack]).expect("tap armed");
        assert_eq!(
            tap.sent, 800,
            "the client holds nothing past the resume point, so the tap must resend from there"
        );
    }

    #[test]
    fn detach_ends_the_tap_and_replying_with_an_error_keeps_it() {
        let tap = Attachment { session_id: "s1".to_owned(), epoch: 1, sent: 40 };
        let detach = frame(Kind::Detach, 1, serde_json::json!({ "sessionId": "s1", "epoch": 1 }));
        let ack = Frame::new(Kind::Detach, "req", 1, serde_json::json!({ "sessionId": "s1" }));
        assert!(next_attachment(Some(tap.clone()), &detach, &[ack]).is_none());

        let write = frame(Kind::Write, 2, serde_json::json!({ "sessionId": "s1", "epoch": 1 }));
        let refused = Frame::new(
            Kind::Error,
            "req",
            2,
            serde_json::json!({ "code": "BAD_FRAME", "message": "no" }),
        );
        let kept = next_attachment(Some(tap), &write, &[refused]).expect("a refusal keeps the tap");
        assert_eq!(kept.sent, 40);
    }

    #[test]
    fn a_single_frame_install_that_declares_the_wrong_size_is_refused_not_corrected() {
        let (keeper, session_id) = keeper_with_a_checkpoint(b"x", 1);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        let reply = only(
            &state,
            &frame(
                Kind::CheckpointInstall,
                3,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-2",
                    "sequenceOffset": 2,
                    "totalBytes": 9,
                    "epoch": 1,
                    "data": base64_encode(b"abcd"),
                }),
            ),
        );
        assert_eq!(reply.kind, Kind::Error);
        assert_eq!(reply.payload["code"], "CHECKPOINT_CHUNK_MISMATCH");
        assert_eq!(
            keeper.checkpoint_info(&session_id).expect("info").checkpoint_id,
            "cp-1",
            "a refused install leaves the previous recovery point in place"
        );
    }

    #[test]
    fn hello_advertises_the_fenced_checkpoint_capability_once_the_daemon_reads_it() {
        // `epoch-fenced-checkpoint-v1` is the daemon's licence to stop sending the unfenced
        // `REPLAY`/`HEAD` verbs: seeing it, the client's `refuse_unfenced` turns an epoch-less read
        // into a hard error. `.omo/evidence/keeper-concurrency/REPLY-daemon-fenced-reads.md` records
        // the audit that no such call site remains, so the flag is advertised - and it is the exact
        // list, one element, so a stray second capability is a failure rather than a detail.
        let reply = only(&state(), &frame(Kind::Hello, 1, serde_json::json!({})));
        assert_eq!(reply.kind, Kind::HelloAck);
        assert_eq!(
            reply.payload["capabilities"],
            serde_json::json!([crate::CAPABILITY_EPOCH_FENCED_CHECKPOINT]),
            "the handshake advertises exactly the one capability the daemon audit covers: {:?}",
            reply.payload
        );
    }

    #[test]
    fn an_unfenced_replay_and_head_are_still_served_for_a_legacy_peer() {
        // Advertising the capability must not make the keeper refuse the old form: the flag tells a
        // *new* daemon it may fence, and a peer that never reads the key keeps working unfenced.
        // Without this, advertising the flag would silently break every older daemon.
        let (keeper, session_id) = spawned_keeper();
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        // Nothing has been written to `/bin/cat`, so the head is 0 and a read from 0 is a clean,
        // empty history - not a gap. This test is about the verb being accepted without an epoch,
        // not about what it carries.
        let mut replay = dispatch(
            &state,
            &frame(
                Kind::Replay,
                1,
                serde_json::json!({ "sessionId": session_id, "fromSequence": 0 }),
            ),
        )
        .into_frames();
        assert_eq!(replay.len(), 1, "an empty page answers with its end frame alone: {replay:?}");
        let end = replay.remove(0);
        assert_eq!(end.kind, Kind::ReplayEnd, "{:?}", end.payload);
        let head = only(
            &state,
            &frame(Kind::Head, 1, serde_json::json!({ "sessionId": session_id })),
        );
        assert_eq!(head.kind, Kind::HeadAck, "{:?}", head.payload);
        assert_eq!(head.payload["sequenceOffset"], 0);
        keeper.kill(&session_id).expect("kill");
    }

    #[test]
    fn every_fenced_verb_refuses_a_stale_epoch() {
        // One table, every verb: the contract is that no fenced verb is the exception that lets a
        // superseded epoch act. `ATTACH` at 2 fences the session, so epoch 1 is stale for all of them.
        let (keeper, session_id) = keeper_with_a_checkpoint(b"abcd", 4);
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        let attached = only(
            &state,
            &frame(
                Kind::Attach,
                2,
                serde_json::json!({ "sessionId": session_id, "epoch": 2 }),
            ),
        );
        assert_eq!(attached.kind, Kind::AttachAck);
        let used = keeper.budget().used();

        for (kind, payload) in [
            (Kind::Attach, serde_json::json!({ "sessionId": session_id, "epoch": 1 })),
            (Kind::Head, serde_json::json!({ "sessionId": session_id, "epoch": 1 })),
            (
                Kind::Replay,
                serde_json::json!({ "sessionId": session_id, "epoch": 1, "fromSequence": 0 }),
            ),
            (
                Kind::Write,
                serde_json::json!({
                    "sessionId": session_id,
                    "epoch": 1,
                    "data": base64_encode(b"x"),
                }),
            ),
            (
                Kind::Resize,
                serde_json::json!({ "sessionId": session_id, "epoch": 1, "cols": 90, "rows": 30 }),
            ),
            (
                Kind::Signal,
                serde_json::json!({ "sessionId": session_id, "epoch": 1, "signal": "interrupt" }),
            ),
            (Kind::Detach, serde_json::json!({ "sessionId": session_id, "epoch": 1 })),
            (Kind::Kill, serde_json::json!({ "sessionId": session_id, "epoch": 1 })),
            (
                Kind::CheckpointInstall,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-2",
                    "sequenceOffset": 9,
                    "epoch": 1,
                    "data": base64_encode(b"wxyz"),
                }),
            ),
            (
                Kind::CheckpointInstall,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-3",
                    "sequenceOffset": 9,
                    "totalBytes": 1024,
                    "epoch": 1,
                }),
            ),
            (
                Kind::CheckpointChunk,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-3",
                    "chunkIndex": 0,
                    "epoch": 1,
                    "data": base64_encode(b"wxyz"),
                }),
            ),
            (
                Kind::CheckpointCommit,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-3",
                    "sequenceOffset": 9,
                    "epoch": 1,
                }),
            ),
            (
                Kind::CheckpointFetch,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-1",
                    "sequenceOffset": 4,
                    "epoch": 1,
                }),
            ),
            (
                Kind::CheckpointQuery,
                serde_json::json!({ "sessionId": session_id, "epoch": 1 }),
            ),
        ] {
            let reply = only(&state, &frame(kind, 3, payload));
            assert_eq!(reply.kind, Kind::Error, "{kind:?} must refuse a stale epoch");
            assert_eq!(reply.payload["code"], "EPOCH_MISMATCH", "{kind:?}");
        }

        // And nothing the stale wall refused left a mark.
        assert!(keeper.contains(&session_id), "no stale verb retired the session");
        assert_eq!(
            keeper.checkpoint_info(&session_id).expect("info").checkpoint_id,
            "cp-1",
            "no stale checkpoint verb replaced the committed one"
        );
        assert_eq!(
            keeper.checkpoint_info(&session_id).expect("info").sequence_offset,
            4
        );
        assert_eq!(
            keeper.budget().checkpoint_pending_bytes(),
            0,
            "the refused chunked install opened no upload"
        );
        assert_eq!(keeper.budget().used(), used, "no stale verb charged or released anything");
        keeper.kill(&session_id).expect("kill");
    }

    #[test]
    fn an_uncoverable_read_is_a_gap_event_not_a_short_answer() {
        // The wire half of goal 2: when the requested range cannot be served, the answer is
        // `REPLAY_GAP_EVENT` carrying the resume offset - never a shortened chunk list a caller
        // could mistake for a complete history. The eviction that causes it is proven in
        // `keeper::tests::a_replay_read_past_an_overflow_is_a_gap_that_resumes_at_the_head`, which
        // can drive the ring directly; here the point is only that the gap reaches the wire as an
        // event. A read above the head is the deterministic way to be uncoverable: nothing has been
        // written to this `/bin/cat` session, so the head is 0 and any offset above it is a gap.
        let (keeper, session_id) = spawned_keeper();
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        assert_eq!(keeper.head(&session_id).expect("head"), 0);

        let gap = only(
            &state,
            &frame(
                Kind::Replay,
                1,
                serde_json::json!({ "sessionId": session_id, "fromSequence": 1 }),
            ),
        );
        assert_eq!(gap.kind, Kind::ReplayGapEvent, "{:?}", gap.payload);
        assert_eq!(gap.payload["code"], "REPLAY_GAP");
        assert_eq!(
            gap.payload["offset"].as_u64(),
            Some(0),
            "the event names where the reader must resume, which is the head: {:?}",
            gap.payload
        );
        keeper.kill(&session_id).expect("kill");
    }

    #[test]
    fn a_connection_that_dies_mid_upload_releases_the_charge_it_was_holding() {
        // The disconnect path is the one no peer can send an abort on, so the connection has to own
        // it. This drives exactly what `serve_connection` does: track the answered install, then drop
        // the tracker as the function's scope ends.
        let (keeper, session_id) = spawned_keeper();
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        let used_before_upload = keeper.budget().used();

        {
            let mut uploads = OpenUploads::new(&keeper);
            let install = frame(
                Kind::CheckpointInstall,
                3,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-1",
                    "sequenceOffset": 5,
                    "totalBytes": 4096,
                    "epoch": 1,
                }),
            );
            let reply = dispatch(&state, &install);
            assert_eq!(reply.frames()[0].kind, Kind::CheckpointAck);
            uploads.track(&install, &reply);
            assert!(
                keeper.budget().checkpoint_pending_bytes() >= 4096,
                "the open upload is charged before a byte arrives"
            );
        }

        assert_eq!(
            keeper.budget().checkpoint_pending_bytes(),
            0,
            "dropping the connection released the upload it never committed"
        );
        assert_eq!(
            keeper.budget().used(),
            used_before_upload,
            "the disconnect returned the charge and charged nothing else"
        );
        keeper.kill(&session_id).expect("kill");
        assert_eq!(keeper.budget().used(), 0);
    }

    #[test]
    fn a_commit_on_the_same_connection_settles_the_upload_so_the_drop_does_not_abort_it() {
        // The counterpart to the test above: a peer that *did* commit must not have its checkpoint
        // aborted when the connection closes. This is the pair that makes the drop path safe to run
        // on every disconnect.
        let (keeper, session_id) = spawned_keeper();
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });

        {
            let mut uploads = OpenUploads::new(&keeper);
            let install = frame(
                Kind::CheckpointInstall,
                3,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-1",
                    "sequenceOffset": 5,
                    "totalBytes": 4,
                    "epoch": 1,
                }),
            );
            let reply = dispatch(&state, &install);
            assert_eq!(reply.frames()[0].kind, Kind::CheckpointAck);
            uploads.track(&install, &reply);

            let chunk = frame(
                Kind::CheckpointChunk,
                4,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-1",
                    "chunkIndex": 0,
                    "epoch": 1,
                    "data": base64_encode(b"abcd"),
                }),
            );
            let reply = dispatch(&state, &chunk);
            assert_eq!(reply.frames()[0].kind, Kind::CheckpointAck);

            let commit = frame(
                Kind::CheckpointCommit,
                5,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-1",
                    "sequenceOffset": 5,
                    "epoch": 1,
                }),
            );
            let reply = dispatch(&state, &commit);
            assert_eq!(reply.frames()[0].kind, Kind::CheckpointAck);
            uploads.track(&commit, &reply);
        }

        let committed = keeper.budget().checkpoint_committed_bytes();
        assert_eq!(
            keeper.checkpoint_info(&session_id).expect("info").checkpoint_id,
            "cp-1",
            "a committed checkpoint survives the connection that installed it"
        );
        assert_eq!(
            keeper.budget().checkpoint_pending_bytes(),
            0,
            "the commit moved the charge out of pending"
        );
        assert!(
            committed >= 4,
            "the committed bucket holds the checkpoint's allocation, got {committed}"
        );
        assert_eq!(
            keeper.checkpoint_bytes(&session_id).expect("bytes"),
            b"abcd",
            "and the bytes it kept are the ones the peer sent"
        );
        keeper.kill(&session_id).expect("kill");
        assert_eq!(keeper.budget().used(), 0);
    }

    #[test]
    fn an_install_carrying_data_null_opens_an_upload_the_disconnect_can_release() {
        // `data: null` is accepted wire input and dispatch treats it as absent (`bytes_at` maps
        // `None | Some(Null)` to `None`), so it opens a chunked upload and charges the declared
        // total. A tracker that only tested for the key's absence saw a commit and recorded nothing,
        // leaving the charge to survive the disconnect until the session died.
        let (keeper, session_id) = spawned_keeper();
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        let used_before_upload = keeper.budget().used();

        {
            let mut uploads = OpenUploads::new(&keeper);
            let install = frame(
                Kind::CheckpointInstall,
                3,
                serde_json::json!({
                    "sessionId": session_id,
                    "checkpointId": "cp-null",
                    "sequenceOffset": 5,
                    "totalBytes": 4096,
                    "epoch": 1,
                    "data": serde_json::Value::Null,
                }),
            );
            let reply = dispatch(&state, &install);
            assert_eq!(reply.frames()[0].kind, Kind::CheckpointAck);
            assert!(
                reply.frames()[0].payload["uploadToken"].as_u64().is_some(),
                "the ack carries the attempt's token: {:?}",
                reply.frames()[0].payload
            );
            uploads.track(&install, &reply);
            assert!(keeper.budget().checkpoint_pending_bytes() >= 4096);
        }

        assert_eq!(
            keeper.budget().checkpoint_pending_bytes(),
            0,
            "a null-data begin is tracked, so the disconnect releases it"
        );
        assert_eq!(keeper.budget().used(), used_before_upload);
        keeper.kill(&session_id).expect("kill");
        assert_eq!(keeper.budget().used(), 0);
    }

    #[test]
    fn a_disconnect_aborts_its_own_attempt_not_the_retry_that_replaced_it() {
        // Two connections, one epoch, one `checkpoint_id`. The retry supersedes the first attempt, so
        // only the retry's token names the live buffer. A tracker keyed by `(session, id, epoch)`
        // would abort the retry when the first connection dropped.
        let (keeper, session_id) = spawned_keeper();
        let state = ServerState::new(Arc::clone(&keeper), unsafe { libc::getuid() });
        let install = frame(
            Kind::CheckpointInstall,
            3,
            serde_json::json!({
                "sessionId": session_id,
                "checkpointId": "cp-shared",
                "sequenceOffset": 5,
                "totalBytes": 4096,
                "epoch": 1,
            }),
        );

        let first = {
            let mut uploads = OpenUploads::new(&keeper);
            let reply = dispatch(&state, &install);
            uploads.track(&install, &reply);
            uploads
        };
        let live = {
            let mut uploads = OpenUploads::new(&keeper);
            let reply = dispatch(&state, &install);
            uploads.track(&install, &reply);
            uploads
        };
        let charged = keeper.budget().checkpoint_pending_bytes();
        assert!(charged > 0);

        drop(first);
        assert_eq!(
            keeper.budget().checkpoint_pending_bytes(),
            charged,
            "the dropped connection must leave the retry's upload and its charge alone"
        );

        drop(live);
        assert_eq!(
            keeper.budget().checkpoint_pending_bytes(),
            0,
            "the connection that owns the live attempt releases it"
        );
        keeper.kill(&session_id).expect("kill");
        assert_eq!(keeper.budget().used(), 0);
    }

    #[test]
    fn an_inline_attach_arms_the_tap_where_its_page_ended_not_at_the_head() {
        // An inline ack copies at most one `REPLY_PAGE_BYTES` page, so `sequenceOffset` (the head)
        // can be past the bytes the frame actually carried. Arming at the head skipped the remainder
        // with no gap event; `afterSequence + dataBytes` is where the page really ended.
        let ack = Frame::new(
            Kind::AttachAck,
            "req",
            1,
            serde_json::json!({
                "sessionId": "s1",
                "epoch": 2,
                "sequenceOffset": 900_000,
                "afterSequence": 100_000,
                "dataBytes": 524_288,
                "dataChunked": false,
                "liveOutput": true,
                "data": "AAAA",
            }),
        );
        let request = frame(
            Kind::Attach,
            1,
            serde_json::json!({
                "sessionId": "s1",
                "epoch": 2,
                "afterSequence": 100_000,
                "liveOutput": true,
            }),
        );
        let tap = next_attachment(None, &request, &[ack]).expect("tap armed");
        assert_eq!(
            tap.sent, 624_288,
            "the cursor is the end of the page the ack delivered, not the session head"
        );
        assert!(tap.sent < 900_000, "the head would have skipped the remainder");
    }
}
