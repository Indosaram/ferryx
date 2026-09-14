# Rust PTY lifecycle + ring buffer review — 2026-09-14

Scope: `src-tauri/src/terminal/{output_hub.rs,pty.rs,session.rs,service.rs}` plus the two shipped
consumers of the hub metadata (`src-tauri/src/daemon/server.rs`, `src-tauri/src/ipc/terminal.rs`)
and the frontend gap handler (`ui/src/lib/terminalEvents.ts`). Only findings whose decisive lines I
opened and read are listed.

### [P2] `SessionHub::replay_gap` is set once and never cleared, so every later full attach reports a stale gap
- Location: src-tauri/src/terminal/output_hub.rs:467 (set), src-tauri/src/terminal/output_hub.rs:571-576 (read)
- Observed: `publish_gap` stores the discontinuity on the hub — `hub.replay_gap = Some(gap.clone());`
  — and no other site ever assigns it back to `None` (the only two assignments in the file are the
  `replay_gap: None` initializer at line 439 and this line). `subscribe_with_sequence` then folds it
  into every snapshot:
  ```rust
  let gap = gap.or_else(|| {
      hub.replay_gap.clone().filter(|gap| {
          after_sequence.is_none_or(|after| after < gap.available_from_sequence - 1)
      })
  });
  ```
  `is_none_or` makes the filter unconditionally true for `after_sequence == None`, i.e. for every
  fresh attach.
- Why it is wrong: after a single SSH reconnect gap (`terminal/remote.rs:655` calls
  `hub.publish_gap(...)` on `read.gap`), the session is permanently flagged. Every subsequent full
  attach of that session — new pane, window reopen, desktop reattaching after a daemon stream
  re-open — receives `gap: Some(..)` in `AttachOk` (`daemon/server.rs:1926`, surfaced to the UI at
  `ipc/terminal.rs:847`), and the frontend treats a replay gap as a hard discontinuity: it resets the
  decoder registry, backlog, and title carry (`ui/src/lib/terminalEvents.ts:241-247`). Worse, the
  retained gap keeps its original `available_from_sequence` (`sequence + 1` at
  output_hub.rs:457) while the ring buffer has since evicted far past it, so the response pairs a
  `history_start_sequence` of, say, `N+500` with a gap claiming data is available from `N+1` —
  mutually inconsistent metadata for the client (and `daemon/server.rs:3224-3230` prefers the gap
  value over `history_start_sequence` when it builds the `Lagged` frame).
- Minimal fix: clear the marker once it has been served past — in `subscribe_with_sequence`, only
  apply `hub.replay_gap` when the caller's cursor actually predates it (drop the `is_none_or`
  blanket-true for `None`, or require `self.buffer.start_sequence()` to still be `> gap
  .available_from_sequence`), and reset `hub.replay_gap = None` in `publish_with_read_timestamp`
  once the buffer's `start_sequence()` has advanced beyond `gap.available_from_sequence`.

### [P2] A killed-but-unreapable PTY child is dropped without `wait`, leaving a permanent zombie
- Location: src-tauri/src/terminal/pty.rs:409-419, src-tauri/src/terminal/session.rs:285-299, src-tauri/src/terminal/session.rs:336-344
- Observed: `close_session` escalates to SIGKILL and then polls for at most `KILL_REAP_TIMEOUT`
  (1 s, pty.rs:17). On timeout it records `"Timed out reaping killed PTY session '{session_id}'"`,
  then falls through to `session.mark_failed(...); session.close_output(); self
  .remove_from_registry(session_id);`. The child handle is a `std::process::Child`
  (portable-pty's `impl Child for std::process::Child`), and `impl Drop for PtySession` only takes
  the writer/master/output sender and aborts the reader task — it never calls `wait`:
  ```rust
  fn drop(&mut self) {
      self.writer.lock().take();
      self.master.lock().take();
      self.output_tx.lock().take();
      if let Some(handle) = self.reader_task.lock().take() { handle.abort(); }
  }
  ```
  The blocking reaper that exists for exactly this case, `wait_and_reap` (session.rs:285), has no
  callers anywhere in the tree (`rg wait_and_reap` matches only its definition).
- Why it is wrong: `std::process::Child`'s drop deliberately does not wait, so a child that has not
  been reaped within 1 s of SIGKILL (uninterruptible I/O, stopped-by-SIGSTOP job, heavily loaded
  machine) becomes a zombie for the lifetime of the daemon. Nothing retries: the session was already
  removed from the registry, so no watcher ever polls it again. Operators see PIDs accumulating in
  `<defunct>` state across a long-running daemon.
- Minimal fix: on the reap-timeout branch, spawn the already-written blocking reaper instead of
  dropping the handle — `let s = Arc::clone(&session); tokio::task::spawn_blocking(move || { let _ =
  s.wait_and_reap(); });` — before `remove_from_registry`. SIGKILL has already been delivered, so the
  blocking `wait` cannot hang indefinitely on a live process.

### [P2] Lifecycle watcher abandons a *running* child when `try_wait` errors — the process is never signalled
- Location: src-tauri/src/terminal/pty.rs:267-272
- Observed: in `start_lifecycle_watcher`, the poll arm is
  ```rust
  Err(error) => {
      session.mark_failed(error.to_string());
      session.close_io();
      session.close_output();
      manager.remove_from_registry(&session_id);
      break;
  }
  ```
  No `signal`/`kill` is issued, and `Drop for PtySession` (session.rs:336) does not kill either.
- Why it is wrong: unlike the exit paths, this branch is reached while the child is still believed to
  be alive (`poll_exit_code` failed, it did not report an exit). Closing the master only sends
  SIGHUP-on-hangup semantics to the foreground group at the tty layer, which a `nohup`-style or
  signal-trapping child ignores; the process survives with no registry entry, so it can never be
  closed through `close_session` again. For Ferryx that means an orphaned agent/shell still holding
  its worktree, invisible to session listing.
- Minimal fix: in that arm, best-effort terminate before dropping the registry entry — call
  `let _ = session.signal(TerminalSignal::Kill).or_else(|_| session.kill());` immediately after
  `mark_failed`, keeping the rest of the branch unchanged.

### [P3] Bracketed-paste (DECSET 2004) detection is per-chunk, so a sequence split across two PTY reads is missed
- Location: src-tauri/src/terminal/output_hub.rs:141-143, src-tauri/src/terminal/output_hub.rs:71-96
- Observed: the ring buffer classifies each chunk in isolation — `if let Some(enabled) =
  scan_dec_mode_2004(&chunk.bytes) { self.bracketed_paste_enabled = enabled; }` — and
  `scan_dec_mode_2004` scans a single `&[u8]` with `while i + 3 < bytes.len()`, carrying no state
  between calls. Chunks are whatever the blocking reader happened to read (`let mut buf = [0u8;
  4096];`, session.rs:75), so `\x1b[?2004h` can straddle a boundary.
- Why it is wrong: `snapshot()` (output_hub.rs:188-196) injects a synthetic `\x1b[?2004h` only when
  `bracketed_paste_enabled` is true and the enabling sequence is no longer in the buffer. If the
  original sequence was split across reads *and* has since been evicted, the flag is false and no
  prefix is injected, so a pane reattached after a long-running session pastes unbracketed — a
  multi-line paste is executed line by line by the shell instead of being delivered as one block.
  Narrow (needs both a split read and eviction of that region), hence P3 rather than P1.
- Minimal fix: keep a small carry-over tail on `BoundedBuffer` (the last ~8 bytes of the previous
  chunk) and scan `carry ++ chunk.bytes` in `push_with_read_timestamp`, updating the carry each push;
  `buffer_contains_active_bracketed_paste` should scan the concatenated buffer for the same reason.

## Not defects (checked, correct)

- Eviction-gap boundary `req_seq + 1 < first_seq` (output_hub.rs:248) is off-by-one correct: a client
  whose next expected sequence equals `first_seq` gets no gap; the in-tree test at
  output_hub.rs:786-793 pins exactly that boundary.
- Chunk and resize sequences come from one counter (`allocate_sequence`, output_hub.rs:111 and 530),
  so `segment_history`'s `ledger[li].sequence <= chunk.sequence` comparisons (output_hub.rs:324, 338)
  can never tie between a resize point and a chunk.
- `available_from_sequence - 1` (output_hub.rs:574) cannot underflow: `publish_gap` always stores
  `sequence + 1 >= 2`.
- Ring eviction accounting (output_hub.rs:147-150) is balanced; with a 512 KiB default capacity
  (output_hub.rs:7) and 4 KiB reads, a single oversized chunk cannot self-evict in the shipped path.

Summary: P0=0, P1=0, P2=3, P3=1
