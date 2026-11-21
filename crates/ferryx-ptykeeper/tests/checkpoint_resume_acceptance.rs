//! Deterministic acceptance for the checkpoint/replay resume contract
//! (`.omo/plans/local-pty-crash-survival-20261116.md`, Phase 3, rev 5).
//!
//! ## Why this file exists, and what it is NOT
//!
//! The combined gate's Phase 3 script (`scripts/qa/phase3-pty-crash-survival.acceptance.mjs`) is
//! frozen to its author, and it asserts whole-process survival outcomes. It therefore cannot state
//! the *byte-level* resume contract cheaply or deterministically: every one of its cases needs a
//! daemon binary, a 60-session fixture, or an 8 MiB PTY run reported only as a summary.
//!
//! This file covers the same contract at the one surface both a daemon and a keeper agree on - the
//! real framing protocol against a **real** `ferryx-ptykeeper` process. Nothing here is a source-text
//! check and nothing here is a mocked transport:
//!
//! * the keeper is the actual binary `cargo` builds for this package (`CARGO_BIN_EXE_...`), started
//!   as its own process in an isolated `FERRYX_RUNTIME_DIR`;
//! * the transport is a real `AF_UNIX` stream carrying real 60-byte-header frames, encoded by the
//!   shipped `protocol::encode_frame` and decoded by the shipped `protocol::recv_frame`;
//! * the PTY is a real `portable_pty` master driven by `/bin/sh` + `cat`, so offsets below are real
//!   output offsets, not injected numbers.
//!
//! ## Coverage of F1-F5
//!
//! | Id | Contract | Covered here | Where the rest lives |
//! |---|---|---|---|
//! | F1 | reset the parser at the gap *before* later bytes; a split escape / split UTF-8 tail is never partially retained | yes, byte-exact (two cases) | daemon-side `gap_sink` ordering is `KeeperRuntime` (seam, see below) |
//! | F2 | the checkpoint's serialized state covers exactly its offset, and replay continues strictly above it - no byte double-covered, none missing | yes, byte-exact | the producer half is `CheckpointSource` (seam) |
//! | F3 | an idle session checkpoints on its own within 60 s | **no** - seam | `keeper_session.rs::CheckpointClock::interval_elapsed` + the replay pump's idle-poll arm |
//! | F4 | an aborted upload returns its charge | **no** - seam | `checkpoint.rs` in-crate test + `server.rs::OpenUploads::drop` |
//! | F5 | a failed checkpoint attempt is retried on a bounded cadence, not a hot loop | **no** - seam | `keeper_session.rs::CheckpointClock::skipped` |
//!
//! The F3/F4/F5 seams are documented in `SEAMS.md` next to this file. They are *not* faked here: a
//! case that cannot reach the state it asserts is a gap, never a pass. That is the same rule the
//! frozen gate script applies with its `GAP` table.
//!
//! ## Determinism rules this file follows
//!
//! * No fixed sleeps. Progress is awaited on the exact signal that proves it - the keeper's own
//!   `sequenceOffset`, or the marker the echo child renames into place the moment its tty is raw -
//!   under a bounded deadline.
//! * Volume, not time: the overflow is produced by writing a computed number of bytes, never by
//!   waiting for a clock.
//! * Every offset asserted is computed from the bytes this file wrote, so a failure names an exact
//!   byte count rather than "eventually".
//!
//! ## Run
//!
//! ```text
//! cargo test --manifest-path crates/ferryx-ptykeeper/Cargo.toml \
//!   --test checkpoint_resume_acceptance -- --nocapture --test-threads=1
//! ```
//!
//! `--test-threads=1` is not required for correctness (each case owns its own runtime directory and
//! keeper process) but keeps the evidence log readable. `cargo` builds the keeper binary for this
//! test automatically; there is no second build step and no environment variable to set.

#![cfg(unix)]

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ferryx_ptykeeper::protocol::{self, Frame, Kind};
use ferryx_ptykeeper::server::base64_encode;
use ferryx_ptykeeper::{REPLAY_SESSION_BYTES};
use serde_json::{json, Value};

/// The binary `cargo` built for this package. Cargo sets this for integration tests of the same
/// package, so the real keeper is always the one under test - never a PATH lookup that could name
/// an installed keeper from some other build.
const KEEPER_BIN: &str = env!("CARGO_BIN_EXE_ferryx-ptykeeper");

/// How long any single awaited signal may take. Generous because a debug-build keeper pumping 8 MiB
/// through a PTY on a loaded CI host is slow; it is a failure bound, never a race window.
const CASE_WAIT: Duration = Duration::from_secs(240);

/// How long the keeper may take to bind its socket after `exec`.
const BIND_WAIT: Duration = Duration::from_secs(60);

/// Bytes per `WRITE`. 64 KiB is the figure the frozen gate harness uses, so a difference here would
/// be a different transport behaviour, not a different assertion.
const WRITE_CHUNK: usize = 64 * 1024;

/// Where the echo child records the tty mode it applied, relative to its cwd (the runtime dir).
fn termios_marker(session_id: &str) -> String {
    format!(".pty-{session_id}.mode")
}

/// A child that echoes its input unchanged: `raw` mode turns off echo, output post-processing and
/// canonical buffering, so the bytes the keeper's pump appends to the replay log are exactly the
/// bytes this test wrote, in order. That equality is what makes every offset below exact.
///
/// The child records `stty -a` after setting the mode and before `exec cat`, and renames it into
/// place so a reader never sees a half-written file. [`Harness::spawn_echo`] waits for that marker
/// before the case writes a single byte. The wait is not decoration: `SPAWN` is acknowledged as soon
/// as the fork happens, so `stty` runs later, and a write that lands while the tty is still
/// canonical is either echoed with ONLCR (`"\n"` comes back as `"\r\n"`, so the head is one byte
/// past what this file wrote) or left in the canonical line buffer until the keeper's `write_all`
/// blocks on a full input queue - which answers no request at all. Both are the child's initial
/// state, never a keeper behaviour, so no case may observe either.
fn echo_child(session_id: &str) -> Vec<String> {
    let marker = termios_marker(session_id);
    vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        // `raw` plus the three flags spelled out: what `raw` implies is not identical across
        // platforms (whether it drops `opost` is the difference that matters here), so the flags the
        // assertion below checks are the flags this command asks for by name. Repeating them after
        // `raw` is idempotent.
        format!(
            "stty raw -echo -icanon -opost 2>/dev/null; stty -a > '{marker}.partial' 2>/dev/null && \
             mv '{marker}.partial' '{marker}' 2>/dev/null; exec cat"
        ),
    ]
}

/// One real keeper process plus one real connection to it.
///
/// The connection is used for correlated request/reply traffic only, which is what
/// `KeeperClient` documents: keeper-initiated output arrives on the attach path, so a test that
/// attached must read frames itself. These cases never attach for live output, so a single
/// synchronous connection is the correct shape.
struct Harness {
    runtime_dir: tempfile::TempDir,
    child: Child,
    stream: UnixStream,
    next_seq: u64,
}

impl Harness {
    fn start() -> Self {
        let runtime_dir = tempfile::tempdir().expect("isolated runtime dir");
        // The keeper derives `<runtime>/keeper.sock` from its own environment, exactly as it does in
        // production. Pointing it at a temp dir is the isolation rule the plan requires: this test
        // can never reach the user's daemon, keeper or `/tmp/ferryx-<uid>`.
        let runtime = runtime_dir.path().canonicalize().expect("canonical runtime dir");
        let child = Command::new(KEEPER_BIN)
            .env("FERRYX_RUNTIME_DIR", &runtime)
            // Keep the child's diagnostics out of the test's stdout so a failure message is not
            // buried; the socket is the readiness signal, not a log line.
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the real ferryx-ptykeeper");

        let socket = runtime.join("keeper.sock");
        let deadline = Instant::now() + BIND_WAIT;
        let stream = loop {
            match UnixStream::connect(&socket) {
                Ok(stream) => break stream,
                Err(error) => {
                    if Instant::now() >= deadline {
                        panic!(
                            "the keeper never bound {}: {error}. The binary under test is {KEEPER_BIN}",
                            socket.display()
                        );
                    }
                    std::thread::yield_now();
                }
            }
        };
        stream.set_read_timeout(Some(CASE_WAIT)).expect("read timeout");
        stream.set_write_timeout(Some(CASE_WAIT)).expect("write timeout");

        let mut harness = Self { runtime_dir, child, stream, next_seq: 1 };
        let ack = harness.call(Kind::Hello, json!({
            "protocolMajor": ferryx_ptykeeper::PROTOCOL_MAJOR,
            "protocolMinor": ferryx_ptykeeper::PROTOCOL_MINOR,
        }));
        assert_eq!(ack.kind, Kind::HelloAck, "the keeper refused HELLO: {ack:?}");
        assert_eq!(ack.payload["protocolMajor"], ferryx_ptykeeper::PROTOCOL_MAJOR);
        harness
    }

    fn runtime_dir(&self) -> &Path {
        self.runtime_dir.path()
    }

    /// Writes one request and returns its correlation id and frame sequence.
    ///
    /// `frame_sequence` is monotonic per connection and the request id is unique per request, so a
    /// reply this test did not ask for is a protocol defect it will see as a correlation failure
    /// rather than silently accept.
    fn send_request(&mut self, kind: Kind, payload: Value) -> (String, u64) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let request_id = format!("{seq:0>36}", seq = seq);
        protocol::write_frame_bytes(
            self.stream.as_raw_fd(),
            &protocol::encode_frame(&Frame::new(kind, request_id.clone(), seq, payload))
                .expect("encode request"),
        )
        .expect("write request");
        (request_id, seq)
    }

    /// Reads the next frame that belongs to this request.
    ///
    /// A keeper-initiated `REPLAY_GAP_EVENT` from the live tap carries no correlation id and answers
    /// no request, so it is skipped. One that echoes *this* request id is this verb's whole answer -
    /// `REPLAY` below the retained window answers a single gap event instead of a chunk run - and is
    /// returned. Skipping that one too would swallow the answer and then wait for a frame that is
    /// never coming, which reports as a read timeout instead of the mismatch it really is.
    fn recv_answer(&mut self, kind: Kind, request_id: &str, seq: u64) -> Frame {
        loop {
            let frame = match protocol::recv_frame(self.stream.as_raw_fd()) {
                Ok(frame) => frame,
                Err(error) => panic!(
                    "no answer to {kind:?} (frame sequence {seq}) within {CASE_WAIT:?}: {error:?}. \
                     A read timeout means the keeper never replied to this verb, so the stall is in \
                     the handler that never returned, not in the transport"
                ),
            };
            if frame.kind == Kind::ReplayGapEvent && frame.request_id != request_id {
                continue;
            }
            return frame;
        }
    }

    /// One request, one reply, both over the real wire.
    fn call(&mut self, kind: Kind, payload: Value) -> Frame {
        let (request_id, seq) = self.send_request(kind, payload);
        let reply = self.recv_answer(kind, &request_id, seq);
        if reply.kind == Kind::Error {
            panic!("{kind:?} was refused: {reply:?}");
        }
        assert_eq!(reply.request_id, request_id, "{kind:?} answered another request: {reply:?}");
        assert_eq!(reply.frame_sequence, seq, "{kind:?} answered another sequence: {reply:?}");
        reply
    }

    /// `REPLAY` from `from`, consuming the whole chunk run the verb answers with.
    ///
    /// `REPLAY` replies with `REPLAY_CHUNK`…`REPLAY_END` (or one `REPLAY_GAP_EVENT`), so a caller that
    /// read only the first frame would leave the run's tail in the socket and desync the next
    /// request on this connection. The start field is `fromSequence`; `sequenceOffset` is what the
    /// *reply* frames carry, and sending that name instead is silently ignored by the keeper, which
    /// then replays from 0 - outside the retained window - and answers a gap event.
    fn replay_from(&mut self, session_id: &str, epoch: u64, from: u64) -> ReplayAnswer {
        let (request_id, seq) = self.send_request(Kind::Replay, json!({
            "sessionId": session_id,
            "epoch": epoch,
            "fromSequence": from,
        }));
        let mut data = Vec::new();
        loop {
            let frame = self.recv_answer(Kind::Replay, &request_id, seq);
            if frame.kind == Kind::Error {
                panic!("REPLAY from {from} was refused: {frame:?}");
            }
            assert_eq!(frame.request_id, request_id, "REPLAY answered another request: {frame:?}");
            assert_eq!(frame.frame_sequence, seq, "REPLAY answered another sequence: {frame:?}");
            match frame.kind {
                Kind::ReplayChunk => {
                    let at = frame.payload["sequenceOffset"].as_u64().expect("no sequenceOffset");
                    assert_eq!(
                        at,
                        from + data.len() as u64,
                        "a chunk must start exactly where the previous one ended, so the run is \
                         contiguous and nothing is dropped or repeated at a page seam"
                    );
                    data.extend(base64_decode(frame.payload["data"].as_str().unwrap_or_default()));
                }
                Kind::ReplayEnd => {
                    let offset =
                        frame.payload["sequenceOffset"].as_u64().expect("no sequenceOffset");
                    let more = frame.payload["more"].as_bool().unwrap_or(false);
                    return ReplayAnswer { offset, data, gap: None, more };
                }
                Kind::ReplayGapEvent => {
                    return ReplayAnswer {
                        offset: frame.payload["offset"].as_u64().expect("no gap offset"),
                        data: Vec::new(),
                        gap: Some(frame.payload["detail"].clone()),
                        more: false,
                    };
                }
                other => panic!("REPLAY answered {other:?} instead of a chunk run: {frame:?}"),
            }
        }
    }

    /// `CHECKPOINT_FETCH` of a whole checkpoint, draining its chunk run.
    ///
    /// Like `REPLAY`, the verb answers with `CHECKPOINT_FETCH_CHUNK`…`CHECKPOINT_FETCH_END`, so the
    /// run is consumed here rather than left in the socket to desync the next request.
    fn fetch_checkpoint(
        &mut self,
        session_id: &str,
        epoch: u64,
        checkpoint_id: &str,
        sequence_offset: u64,
    ) -> (Vec<u8>, u64) {
        let (request_id, seq) = self.send_request(Kind::CheckpointFetch, json!({
            "sessionId": session_id,
            "epoch": epoch,
            "checkpointId": checkpoint_id,
            "sequenceOffset": sequence_offset,
            "fromByte": 0,
        }));
        let mut data = Vec::new();
        // Every fetch frame repeats the committed total, so the first one fixes the value every
        // later frame must agree with. An Option rather than a zero-initialised `u64`: a real total
        // of 0 must not be confused with "no frame has been read yet".
        let mut agreed_total: Option<u64> = None;
        loop {
            let frame = self.recv_answer(Kind::CheckpointFetch, &request_id, seq);
            if frame.kind == Kind::Error {
                panic!("CHECKPOINT_FETCH of {checkpoint_id} was refused: {frame:?}");
            }
            assert_eq!(
                frame.request_id, request_id,
                "CHECKPOINT_FETCH answered another request: {frame:?}"
            );
            assert_eq!(
                frame.frame_sequence, seq,
                "CHECKPOINT_FETCH answered another sequence: {frame:?}"
            );
            let total_bytes = frame.payload["totalBytes"]
                .as_u64()
                .expect("every fetch frame names the committed total");
            assert_eq!(
                *agreed_total.get_or_insert(total_bytes),
                total_bytes,
                "every frame of one fetch must name the same committed total, or a daemon paging \
                 the checkpoint would be told two different sizes mid-download"
            );
            match frame.kind {
                Kind::CheckpointFetchChunk => {
                    data.extend(base64_decode(frame.payload["data"].as_str().unwrap_or_default()));
                }
                Kind::CheckpointFetchEnd => return (data, total_bytes),
                other => panic!("CHECKPOINT_FETCH answered {other:?}: {frame:?}"),
            }
        }
    }

    /// Spawns a real PTY holding `echo_child()` and returns the child's pid.
    ///
    /// Returns only once the child has proven it reached raw mode, so every byte a case writes
    /// afterwards is byte-exact.
    fn spawn_echo(&mut self, session_id: &str, epoch: u64) -> u32 {
        let ack = self.call(Kind::Spawn, json!({
            "sessionId": session_id,
            "requestId": format!("{session_id}-spawn-{epoch}"),
            "argv": echo_child(session_id),
            "env": {},
            "cwd": self.runtime_dir().to_string_lossy(),
            "cols": 120,
            "rows": 40,
            "epoch": epoch,
        }));
        assert_eq!(ack.kind, Kind::SpawnAck, "SPAWN was not acknowledged: {ack:?}");
        assert_eq!(ack.payload["sessionId"], session_id);
        let pid = ack.payload["pid"].as_u64().expect("the keeper reported no pid") as u32;
        self.await_raw_child(session_id);
        pid
    }

    /// Waits until the echo child has applied `stty raw -echo`, proven by the mode it recorded.
    ///
    /// The signal is the marker rename, never a timer: the file exists whole or not at all, and the
    /// flags it carries are the mode the child's shell left in force before `exec cat`.
    fn await_raw_child(&self, session_id: &str) {
        let marker = self.runtime_dir().join(termios_marker(session_id));
        let deadline = Instant::now() + BIND_WAIT;
        loop {
            match std::fs::read_to_string(&marker) {
                Ok(mode) => {
                    // `stty -a` lists every flag as its own whitespace-separated token on both
                    // platforms (Linux prints one line per class, macOS prefixes `iflags:`/`lflags:`/
                    // `oflags:`), so the tokens are compared whole: `contains("-echo")` alone would
                    // also match macOS's `-echoprt`/`-echonl` and certify a tty that still echoes.
                    // Those three flags are exactly what makes the child byte-exact: `-opost` stops
                    // ONLCR rewriting `"\n"` to `"\r\n"`, `-icanon` stops the line buffer holding a
                    // newline-less run, and `-echo` stops the kernel copying the payload back.
                    // Trailing `;` is trimmed because macOS ends each class line with one, so the
                    // last flag on such a line would otherwise never compare equal.
                    let flags: Vec<&str> =
                        mode.split_whitespace().map(|flag| flag.trim_end_matches(';')).collect();
                    for required in ["-opost", "-icanon", "-echo"] {
                        assert!(
                            flags.contains(&required),
                            "the echo child recorded a tty that is not raw ({required} is \
                             missing), so its output would not be byte-exact. It recorded:\n{mode}"
                        );
                    }
                    return;
                }
                Err(error) => assert!(
                    Instant::now() < deadline,
                    "the echo child never recorded its tty mode at {} within {BIND_WAIT:?} \
                     ({error}); the marker is renamed into place right after `stty raw -echo`, so \
                     its absence means the child never reached raw mode and any head below would \
                     count echo bytes instead of payload",
                    marker.display()
                ),
            }
            std::thread::yield_now();
        }
    }

    /// Writes `bytes` and returns how many the keeper accepted.
    fn write(&mut self, session_id: &str, epoch: u64, bytes: &[u8]) -> usize {
        let ack = self.call(Kind::Write, json!({
            "sessionId": session_id,
            "epoch": epoch,
            "data": base64_encode(bytes),
        }));
        assert_eq!(ack.kind, Kind::Write, "WRITE was not acknowledged: {ack:?}");
        ack.payload["written"].as_u64().expect("no written count") as usize
    }

    /// The keeper's own output head, read from the real protocol.
    fn head(&mut self, session_id: &str) -> u64 {
        let ack = self.call(Kind::Head, json!({ "sessionId": session_id }));
        assert_eq!(ack.kind, Kind::HeadAck, "HEAD was not acknowledged: {ack:?}");
        ack.payload["sequenceOffset"].as_u64().expect("no sequenceOffset")
    }

    /// Writes `bytes` and waits until the keeper's head has advanced by exactly that many bytes.
    ///
    /// The head is the authoritative signal: the pump appends asynchronously, so a byte count read
    /// before it lands would be a race. Nothing here sleeps on a timer.
    fn write_and_await_head(&mut self, session_id: &str, epoch: u64, bytes: &[u8], expected: u64) {
        let written = self.write(session_id, epoch, bytes);
        assert_eq!(written, bytes.len(), "the keeper accepted {written} of {} bytes", bytes.len());
        let deadline = Instant::now() + CASE_WAIT;
        loop {
            let head = self.head(session_id);
            if head == expected {
                return;
            }
            assert!(
                head < expected,
                "the keeper's head is {head}, past the {expected} bytes this test wrote: the echo \
                 child is not byte-exact and every offset assertion below would be meaningless"
            );
            assert!(
                Instant::now() < deadline,
                "the keeper's head stayed at {head} of {expected} bytes for the whole deadline"
            );
            std::thread::yield_now();
        }
    }

    /// Fills the replay log to exactly its cap, then returns the head (== the cap).
    ///
    /// Exactly full is the interesting state: the *next* push is the one that must reset, which is
    /// what lets a case place the reset boundary on a chosen byte instead of wherever a bulk loop
    /// happened to cross the cap.
    fn fill_replay_to_cap(&mut self, session_id: &str, epoch: u64) -> u64 {
        let cap = REPLAY_SESSION_BYTES as u64;
        let filler = vec![b'x'; WRITE_CHUNK];
        let mut expected = 0u64;
        while expected < cap {
            let remaining = (cap - expected) as usize;
            let take = remaining.min(filler.len());
            expected += take as u64;
            self.write_and_await_head(session_id, epoch, &filler[..take], expected);
        }
        assert_eq!(expected, cap);
        cap
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // The keeper is this test's own child, started from `KEEPER_BIN`; killing it can never reach
        // a production keeper. `kill` is refused by the OS for a reaped child, which is why the pid
        // is not stored and signalled by number anywhere in this file.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

use std::os::unix::io::AsRawFd;

/// One `REPLAY` verb's whole answer: the resumed offset, the bytes, and the gap when it could not
/// serve them.
struct ReplayAnswer {
    offset: u64,
    data: Vec<u8>,
    gap: Option<Value>,
    more: bool,
}

/// The `replayGap` triple `ATTACH_ACK` carries, decoded from the real reply.
///
/// `requested` is the caller's own `afterSequence` - the offset it asked from and did not get -
/// never a value the keeper substitutes for it
/// (`.omo/evidence/keeper-concurrency/KEEPER-FINAL-CONTRACT.md` §4.1). `available` is the head to
/// resume at, and `retained` is the log's retained lower bound at the moment of the gap. A case that
/// wants to say "the first offset it could not retain" must therefore ask from that offset; asking
/// from 0 makes the same field read 0, which says only where the consumer stood.
struct ReplayGap {
    requested: u64,
    available: u64,
    retained: u64,
}

/// The `ATTACH_ACK` fields these cases assert, decoded from the real reply.
struct AttachAck {
    sequence_offset: u64,
    checkpoint_id: Option<String>,
    checkpoint_sequence_offset: u64,
    gap: Option<ReplayGap>,
    data: Vec<u8>,
}

fn attach(harness: &mut Harness, session_id: &str, epoch: u64, after: u64) -> AttachAck {
    let ack = harness.call(Kind::Attach, json!({
        "sessionId": session_id,
        "epoch": epoch,
        "afterSequence": after,
        // Small suffixes ride inline; a chunked answer would need the follow-up run, which these
        // cases do not assert.
        "chunked": false,
        "liveOutput": false,
    }));
    assert_eq!(ack.kind, Kind::AttachAck, "ATTACH was not acknowledged: {ack:?}");
    let gap = ack.payload.get("replayGap").and_then(|gap| {
        Some(ReplayGap {
            requested: gap.get("requestedAfterSequence")?.as_u64()?,
            available: gap.get("availableFromSequence")?.as_u64()?,
            retained: gap.get("retainedFromSequence")?.as_u64()?,
        })
    });
    AttachAck {
        sequence_offset: ack.payload["sequenceOffset"].as_u64().expect("no sequenceOffset"),
        checkpoint_id: ack.payload["checkpointId"].as_str().map(str::to_owned),
        checkpoint_sequence_offset: ack.payload["checkpointSequenceOffset"]
            .as_u64()
            .unwrap_or_default(),
        gap,
        data: base64_decode(ack.payload["data"].as_str().unwrap_or_default()),
    }
}

/// Decodes the keeper's own base64 without adding a dependency. The encoder under test is the
/// shipped `server::base64_encode`, and the round trip is asserted below, so a broken decoder here
/// cannot silently pass a case.
fn base64_decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut accumulator = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let Some(value) = ALPHABET.iter().position(|entry| *entry == byte) else {
            continue; // padding or whitespace
        };
        accumulator = (accumulator << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }
    out
}

#[test]
fn the_transport_round_trips_opaque_bytes_before_it_is_relied_on() {
    // A decoder bug would make every `data` assertion below vacuously wrong, so the round trip is
    // asserted on the shipped encoder first.
    for bytes in [
        &b""[..],
        &b"x"[..],
        &b"\x1b[31m\xe4\xb8\xad"[..],
        &(0u8..=255).collect::<Vec<u8>>()[..],
    ] {
        assert_eq!(base64_decode(&base64_encode(bytes)), bytes);
    }
}

/// **F1 (a).** The reset boundary lands inside a split escape sequence, and the run that caused it
/// is discarded whole.
///
/// Rev 5 requires the keeper to be byte-oriented: it must not claim to resume at a terminal-language
/// boundary, and it must never feed a partially retained suffix to a parser. So the assertion is
/// *not* "boundary-aligned state survived" - it is:
///
/// 1. the gap is reported with the resume offset at the keeper's own head;
/// 2. the retained suffix is empty at the moment of the reset, i.e. nothing of the old window was
///    kept;
/// 3. the discarded run is exactly the escape-fragment this case wrote, so no part of that fragment
///    is ever replayed;
/// 4. the retained bytes after the resume offset are exactly the later bytes, so the consumer that
///    resumes there sees a consistent stream.
#[test]
fn f1_a_reset_lands_inside_a_split_escape_and_discards_that_run_whole() {
    let mut harness = Harness::start();
    let session_id = "f1a-split-escape";
    let epoch = 1;
    harness.spawn_echo(session_id, epoch);
    let cap = harness.fill_replay_to_cap(session_id, epoch);
    assert_eq!(harness.head(session_id), cap, "the replay log should be exactly full");

    // The trigger run: an incomplete CSI. It cannot be retained - the log is full and never trims
    // from the front - so its three bytes become the gap's lost range and the resume offset moves
    // to the head, which is *inside* the sequence.
    let trigger = b"\x1b[3";
    harness.write_and_await_head(session_id, epoch, trigger, cap + trigger.len() as u64);

    // Asked from `cap`: the consumer holds every byte up to there, so the gap's `requested` offset is
    // the first byte it could not retain - which is what this case is about. Asking from 0 would
    // make the same field read 0 (the caller's own offset, by contract) and the assertion below
    // would be measuring the test's argument instead of the keeper's loss.
    let after_reset = attach(&mut harness, session_id, epoch + 1, cap);
    let gap = after_reset.gap.expect("an overflow must report a replay gap");
    assert_eq!(
        gap.requested, cap,
        "the gap must name the offset the consumer asked from and did not get"
    );
    assert_eq!(
        gap.available,
        cap + trigger.len() as u64,
        "the gap must resume at the keeper's head"
    );
    assert!(
        gap.retained > gap.requested,
        "the retained bound {} must be above the offset the gap declared lost ({}), or the log \
         would be claiming bytes it just said were gone",
        gap.retained,
        gap.requested
    );
    let resumed = gap.available;
    assert_eq!(
        after_reset.sequence_offset, resumed,
        "the ack's offset must be the resume offset, or a consumer would resume past bytes it has"
    );
    assert!(
        after_reset.data.is_empty(),
        "the retained suffix must be discarded whole, but {} bytes came back",
        after_reset.data.len()
    );

    // Later bytes, including a UTF-8 character the reset did not see the start of. They are retained
    // normally: the reset happened before them, and a consumer resuming at `resumed` sees them all.
    let later = b"1m\xe4\xb8\xadOK";
    harness.write_and_await_head(session_id, epoch + 1, later, resumed + later.len() as u64);

    let replay = harness.replay_from(session_id, epoch + 1, resumed);
    assert!(
        replay.gap.is_none(),
        "a replay from the resume offset is inside the retained window and must not report a gap: \
         {:?}",
        replay.gap
    );
    assert_eq!(
        replay.data,
        later.to_vec(),
        "the bytes above the resume offset must be exactly the bytes written after the reset"
    );
    assert_eq!(
        replay.offset,
        resumed + later.len() as u64,
        "the run must end at the head, so it covers every byte above the resume offset exactly once"
    );
    assert!(!replay.more, "the whole retained suffix must arrive in one run at this size");
}

/// **F1 (b).** A UTF-8 character split across the reset boundary is never partially retained.
///
/// The conversion of "reset before later bytes" into an observable: a consumer resuming at the
/// reported offset can rebuild the stream, while the byte sequence the reset cut is not spliced into
/// it. Feeding the resume tail to a fresh parser must therefore be able to complete the character -
/// the leading bytes are in the tail, not stranded in a dropped suffix the consumer was never told
/// about.
#[test]
fn f1_b_bytes_after_a_gap_resume_exactly_once_from_the_reported_offset() {
    let mut harness = Harness::start();
    let session_id = "f1b-split-utf8";
    let epoch = 1;
    harness.spawn_echo(session_id, epoch);
    let cap = harness.fill_replay_to_cap(session_id, epoch);

    // The reset trigger is a *single* byte, so the cut lands one byte into the multi-byte sequence
    // this case writes across two frames. `\xe4` alone is an incomplete UTF-8 lead byte.
    harness.write_and_await_head(session_id, epoch, b"\xe4", cap + 1);
    let head = cap + 1;

    // Asked from `cap`, the offset a consumer that has everything up to the full window holds: the
    // gap then names the lead byte itself as the first byte it could not retain, instead of naming
    // whatever offset this test happened to pass in.
    let after_reset = attach(&mut harness, session_id, epoch + 1, cap);
    let gap = after_reset.gap.expect("an overflow must report a replay gap");
    assert_eq!(gap.requested, cap, "the gap must name the offset the consumer asked from");
    assert_eq!(gap.available, head, "the resume offset must be the head at the reset");
    assert_eq!(
        gap.available - gap.requested,
        1,
        "exactly the one lead byte at the reset boundary is lost, and not the later bytes"
    );
    assert_eq!(
        gap.retained, head,
        "the discarded suffix is gone whole, so nothing is claimed as retained at the reset"
    );
    let resumed = gap.available;
    assert!(
        after_reset.data.is_empty(),
        "the partial UTF-8 lead byte must not be retained: {} bytes came back",
        after_reset.data.len()
    );

    // The remaining bytes of the same character, then a marker. Delivery is exactly once: one write
    // is one appearance, so a duplicated byte at the seam is visible as a count of two.
    let rest = b"\xb8\xadMARK";
    harness.write_and_await_head(session_id, epoch + 1, rest, resumed + rest.len() as u64);

    let replay = harness.replay_from(session_id, epoch + 1, resumed);
    assert!(
        replay.gap.is_none(),
        "a replay from the reported resume offset must be inside the retained window: {:?}",
        replay.gap
    );
    let data = replay.data;
    assert_eq!(
        data,
        rest.to_vec(),
        "the stream above the resume offset must be exactly the post-reset writes"
    );
    assert_eq!(
        data.windows(4).filter(|window| *window == b"MARK").count(),
        1,
        "the marker must appear exactly once, so the seam neither duplicated nor dropped a byte"
    );
}

/// **F2.** A committed checkpoint and the replay above it are contiguous and non-overlapping.
///
/// This is the offset half of the plan's "serialized state + its sequence offset" pair. The keeper
/// never inspects the bytes, so what it owes the daemon is a *framing* guarantee: the checkpoint it
/// hands back names offset `X`, and the replay it hands back for `X` begins at `X` and covers every
/// byte after it exactly once. If either side were off by a byte, a daemon restoring state and then
/// applying the suffix would either drop a byte or replay one it already applied.
#[test]
fn f2_a_committed_checkpoint_and_the_replay_above_it_are_contiguous() {
    let mut harness = Harness::start();
    let session_id = "f2-checkpoint-seam";
    let epoch = 1;
    harness.spawn_echo(session_id, epoch);

    let before = b"BEFORE-CHECKPOINT\n";
    harness.write_and_await_head(session_id, epoch, before, before.len() as u64);
    let checkpoint_offset = harness.head(session_id);
    assert_eq!(checkpoint_offset, before.len() as u64);

    // A checkpoint is daemon-produced terminal state; the keeper treats it as opaque bytes. Its
    // content here is deliberately not valid VT: if the keeper ever parsed it, this case would
    // behave differently, which is the point.
    let checkpoint_bytes = b"\x00\x01opaque-state-not-vt\xff".to_vec();
    let ack = harness.call(Kind::CheckpointInstall, json!({
        "sessionId": session_id,
        "epoch": epoch,
        "checkpointId": "cp-f2",
        "sequenceOffset": checkpoint_offset,
        "totalBytes": checkpoint_bytes.len(),
        "data": base64_encode(&checkpoint_bytes),
    }));
    assert_eq!(ack.kind, Kind::CheckpointAck, "the install was not acknowledged: {ack:?}");
    assert_eq!(ack.payload["checkpointId"], "cp-f2");
    assert_eq!(
        ack.payload["sequenceOffset"].as_u64(),
        Some(checkpoint_offset),
        "the keeper must store the checkpoint at the offset it was given"
    );
    assert_eq!(ack.payload["storedBytes"].as_u64(), Some(checkpoint_bytes.len() as u64));

    // Bytes produced after the checkpoint are the replay the daemon applies on top of it.
    let after = b"AFTER-CHECKPOINT\n";
    harness.write_and_await_head(
        session_id,
        epoch,
        after,
        checkpoint_offset + after.len() as u64,
    );

    let recovered = attach(&mut harness, session_id, epoch + 1, checkpoint_offset);
    assert!(
        recovered.gap.is_none(),
        "the checkpoint is inside the retained window, so no gap may be reported"
    );
    assert_eq!(
        recovered.checkpoint_id.as_deref(),
        Some("cp-f2"),
        "the attach must name the checkpoint a daemon should restore"
    );
    assert_eq!(
        recovered.checkpoint_sequence_offset, checkpoint_offset,
        "the checkpoint must be reported at the offset it covers"
    );
    assert_eq!(
        recovered.data,
        after.to_vec(),
        "the suffix must be exactly the bytes above the checkpoint offset: any overlap would \
         double-apply a byte, any shortfall would drop one"
    );
    assert_eq!(
        recovered.sequence_offset,
        checkpoint_offset + after.len() as u64,
        "the ack's offset must be the head the suffix ends at"
    );

    // The bytes themselves, through the fetch path, so the transport - not only the ack - is proven.
    let (fetched, total_bytes) =
        harness.fetch_checkpoint(session_id, epoch + 1, "cp-f2", checkpoint_offset);
    assert_eq!(
        fetched,
        checkpoint_bytes,
        "the fetched checkpoint must be the bytes that were installed, unmodified"
    );
    assert_eq!(
        total_bytes,
        checkpoint_bytes.len() as u64,
        "the fetch must report the committed total, so a daemon knows how much it has left"
    );
}

/// **F2 (b).** A checkpoint at an offset at or below the committed one is refused, so the framing
/// guarantee cannot be retroactively broken by a rewinding upload.
///
/// Without this, a daemon that retried a stale upload after a successor committed could move the
/// recovery point backwards while the replay log had already been trimmed at the newer offset,
/// leaving a hole no consumer is told about.
#[test]
fn f2_b_a_rewinding_checkpoint_is_refused_and_the_commit_survives() {
    let mut harness = Harness::start();
    let session_id = "f2b-stale";
    let epoch = 1;
    harness.spawn_echo(session_id, epoch);

    let bytes = b"state".to_vec();
    harness.write_and_await_head(session_id, epoch, b"12345678", 8);
    let ack = harness.call(Kind::CheckpointInstall, json!({
        "sessionId": session_id,
        "epoch": epoch,
        "checkpointId": "cp-new",
        "sequenceOffset": 8,
        "totalBytes": bytes.len(),
        "data": base64_encode(&bytes),
    }));
    assert_eq!(ack.payload["sequenceOffset"].as_u64(), Some(8));

    // The real wire refusal: the keeper answers with an ERROR frame carrying its typed code, which
    // `call` would panic on, so this request is written by hand.
    let seq = harness.next_seq;
    harness.next_seq += 1;
    let request_id = format!("{seq:0>36}");
    protocol::write_frame_bytes(
        harness.stream.as_raw_fd(),
        &protocol::encode_frame(&Frame::new(
            Kind::CheckpointInstall,
            request_id.clone(),
            seq,
            json!({
                "sessionId": session_id,
                "epoch": epoch,
                "checkpointId": "cp-old",
                "sequenceOffset": 4,
                "totalBytes": bytes.len(),
                "data": base64_encode(&bytes),
            }),
        ))
        .expect("encode"),
    )
    .expect("write");
    let reply = protocol::recv_frame(harness.stream.as_raw_fd()).expect("read");
    assert_eq!(reply.kind, Kind::Error, "a rewinding checkpoint must be refused: {reply:?}");
    assert_eq!(
        reply.payload["code"], "STALE_CHECKPOINT",
        "the refusal must be the typed code a daemon branches on, not a generic error"
    );

    // The newer commit is untouched: a refusal preserves the recovery point.
    let query = harness.call(Kind::CheckpointQuery, json!({ "sessionId": session_id }));
    assert_eq!(query.payload["checkpointId"], "cp-new");
    assert_eq!(query.payload["sequenceOffset"].as_u64(), Some(8));
}
