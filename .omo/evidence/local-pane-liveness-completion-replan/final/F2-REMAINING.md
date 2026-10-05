# F2-REMAINING — apply-ready stacked patch for the remaining F2 findings

Authored 2026-10-05 by the F2-remaining lane. **Nothing was committed, nothing was applied to any
worktree, and nothing was compiled.** All applicability proof below was obtained on scratch copies
extracted with `git archive`.

## 1. Artifacts and apply order

| # | Artifact | Size | sha256 | Role |
|---|---|---|---|---|
| 1 | `final/F2-1-suspend-windows.patch` | 25 221 B | `7dc9366eaeb9de779508b4c078469c8adf8ca13fef39390006a536f5d8b725cb` | pre-existing, **awaiting its own compile host** |
| 2 | `final/F2-REMAINING.patch` | 15 036 B | `e8070bdbfadb60389d91446b7fcb27b363b34db658c47896a08d27b4082a1e7f` | this lane's stacked patch |

**Apply order: `F2-1-suspend-windows.patch` first, then `F2-REMAINING.patch`**, both with `git apply -p1`
at repository root.

`F2-REMAINING.patch` touches 6 files:

```
src-tauri/src/daemon/server.rs
src-tauri/src/ipc/terminal.rs
src-tauri/src/terminal/pty.rs
src-tauri/src/terminal/qa_liveness.rs
ui/src/lib/paneDebugInfo.ts
ui/src/lib/paneLiveness.ts
```

`daemon/server.rs` is shared with F2-1, which is why this is authored as a stack: F2-1's `server.rs`
hunk lands at `@@ -2073,6 +2073,15 @@` and this patch's `server.rs` hunks land at `@@ -2921,8 +2921,12 @@`
and `@@ -4376,7 +4380,13 @@`. They do not overlap, so the two patches are also independently
applicable — but the declared sequence is the one proven below.

### Base revision

Both patches are authored against `work/local-pane-liveness-completion-foundation` at **`0726c70c`**,
which was HEAD with a clean tree when this lane started.

While this lane was working, a concurrent session (the `scripts/**` lane) committed on that same
branch: the tip is now **`b1f249f4`** = `0726c70c` + `fix(qa): exclude fixture sessions from the split
delta and drop an unused capture import`, whose diff is **4 files, all under `scripts/`** —
`git diff --name-only 0726c70c..b1f249f4` returns none of the six files this patch touches. This lane
did not create, move, or touch that commit; it only observed the tip move and re-verified against it.

### Applicability proof (scratch copies only)

Fresh `git archive` extractions, `git apply --check`/`git apply`, no worktree involved:

```
[A] from 0726c70c (the documented base)
    pristine 0726c70c : git apply -p1 --check F2-1-suspend-windows.patch      -> exit 0
    pristine 0726c70c : git apply -p1 --check F2-REMAINING.patch              -> exit 0
    apply F2-1                                                                -> exit 0
    F2-1 applied      : git apply -p1 --check F2-REMAINING.patch              -> exit 0
    apply F2-REMAINING                                                        -> exit 0
    resulting 6 files sha256-identical to the authored scratch:
      MATCH ab0837a42a4d9f79  src-tauri/src/daemon/server.rs
      MATCH e0b2ae2ee89f1125  src-tauri/src/ipc/terminal.rs
      MATCH 53cf4dcaad668e34  src-tauri/src/terminal/pty.rs
      MATCH 5e35c5f50331bf63  src-tauri/src/terminal/qa_liveness.rs
      MATCH 66ef429564c3a564  ui/src/lib/paneDebugInfo.ts
      MATCH 09af4f04562c14fc  ui/src/lib/paneLiveness.ts

[B] from the current tip b1f249f4 (0726c70c + scripts-only commit)
    F2-1        --check -> exit 0 ; apply -> exit 0
    F2-REMAINING --check -> exit 0 ; apply -> exit 0 ; 6/6 file hashes MATCH
```

So the sequence applies cleanly to `0726c70c` and to the current branch tip; the scripts-only commit
does not perturb it. Worktree state was `HEAD=b1f249f4`, `git status --porcelain` empty, before and
after all of this — this lane wrote only the two artifact files under
`.omo/evidence/local-pane-liveness-completion-replan/final/`.

---

## 2. Per-finding record

### F2-3 — synchronous disk I/O on the async runtime (fixed)

Claim verified as stated. `daemon/handover.rs:545-570` (`record_decision`) does
`create_new` → `write_all` → `sync_all` → `rename` → parent-directory `sync_all` inline;
`abort_handover` (`handover.rs:813`) calls it and additionally does a blocking `fs::remove_file`.
`AbortHandover` was handled directly on the async request loop (`server.rs:4363-4364`, now `:4382`),
while `CommitHandover` is wrapped in `crate::ipc::run_blocking` (`server.rs:4353-4361`).

**Change** (`daemon/server.rs`, arm `Ok(DaemonRequest::AbortHandover)`): the abort call is now
offloaded exactly like the commit call above it —

```rust
let manager = Arc::clone(&self.handover_manager);
match crate::ipc::run_blocking(move || {
    manager.abort_handover().map_err(crate::ipc::IpcError::internal)
}).await {
    Ok(()) => { /* resume_paused_readers + tracing unchanged */ DaemonResponse::AbortHandoverOk }
    Err(e) => daemon_error(e.to_string()),
}
```

The success branch (reader resume, the `tracing::warn!` with `resumed_count`) and the response type
are byte-for-byte the previous behaviour; only the error type changed from `String` to
`IpcError` (Display delegates to the message, so the wire `Error { message, .. }` is unchanged).
`Arc::clone` is deliberate: `self.handover_manager` is used again later in the loop, so the closure
must own its own handle.

### F2-4 — QA lane: blocking sidecar read on the 25 ms poll (fixed)

Claim verified. `terminal/qa_liveness.rs:344-369` (`read_frame_submission_in`) does
`std::fs::read_to_string`; it was called at `:449` inside the `loop { ticker.tick().await; … }` of
`async fn await_frame_evidence` (`:429`), ticked by `CONTROL_POLL_MS = 25` (`:109`).

**Change** (`terminal/qa_liveness.rs`, inside `await_frame_evidence`): the poll now runs the read on
the blocking pool and awaits it —

```rust
let polled_channel = Arc::clone(channel);
let polled_session = observation.session_id.clone();
let submitted = match crate::ipc::run_blocking(move || {
    Ok(read_frame_submission(&polled_channel, &polled_session))
}).await {
    Ok(submitted) => submitted,
    Err(_) => None,   // a blocking read that cannot complete is "no evidence yet", never a pass
};
if let Some(covered) = submitted { tracker.note_frame(&observation.session_id, covered); }
```

`Arc::clone` is required: the closure is `'static + Send`, so it cannot borrow the `&Arc<…>`
parameter. `read_frame_submission` itself is untouched and still used synchronously by the in-module
unit tests (`:1086-1099`). `CONTROL_POLL_MS` and `FRAME_SUBMISSION_WINDOW_MS` are untouched.

### F2-5 — QA lane: blocking control read on the 25 ms tick (fixed)

Claim verified. `ipc/terminal.rs:1879-1893` (`read_control`) does `std::fs::read_to_string`; it was
called from `:2576`, `:3040`, `:3112` and `:3231-3232` on every `WATCH_TICK_MS = 25` (`:1831`).

**Change** (`ipc/terminal.rs`, `mod qa_split_producers`):

- The synchronous body is renamed `read_control_in(dir: &Path, name: &str, channel: &QaBarrierChannel)`
  with its nonce check unchanged, and its doc comment now says the async watchers run it through
  `run_blocking`.
- A new async wrapper keeps the call-site shape:

```rust
async fn read_control(dir: &Path, name: &str, channel: &Arc<QaBarrierChannel>) -> Option<Value> {
    let dir = dir.to_owned(); let name = name.to_owned(); let channel = Arc::clone(channel);
    match crate::ipc::run_blocking(move || Ok(read_control_in(&dir, &name, &channel))).await {
        Ok(control) => control,
        Err(_) => None,   // "no control observed yet", never an armed control
    }
}
```

- The four watcher call sites now `.await` the wrapper. `run_handover_watcher` was the one site whose
  call was nested inside a `&&` short-circuit expression, which cannot be awaited; it is rewritten to
  two awaited bindings followed by the same `is_none() && is_none()` test, so the trigger condition
  and the tick cadence are unchanged:

```rust
let handover_armed = read_control(&dir, TRIGGER_HANDOVER, &channel).await;
let handover_abort_armed = read_control(&dir, TRIGGER_HANDOVER_ABORT, &channel).await;
if handover_armed.is_none() && handover_abort_armed.is_none() { continue; }
```

- **Construction-site sweep:** the five in-module unit-test call sites (`:3474`, `:3486`, `:3498`,
  `:3880`, `:3895` — all inside `#[test] fn control_files_are_read_only_for_this_run` and
  `#[test] fn retry_and_batch_controls_are_read_only_for_this_run`, both synchronous) were switched
  to `read_control_in`, which preserves exactly what they assert. Without this the patch would not
  compile. `WATCH_TICK_MS` is untouched.

### F2-7 — `transfer_owners` entry survives a failed adopt (fixed)

Claim verified. `daemon/server.rs:2896-2899` inserts the predecessor identity via
`PtyManager::expect_transferred_owner` (`terminal/pty.rs:1008-1013`); the entry is removed **only**
inside a successful `adopt_transferred_session_with_hub` (`terminal/pty.rs:953`). The adopt-failure
branch (`server.rs:2908-2911`) called only `release_adopted_ownership` and returned, so the moved
`PtySessionSnapshot` stayed in the map for the daemon's lifetime.

**Change:**

- `terminal/pty.rs`: new minimal accessor next to `expect_transferred_owner`, same `#[cfg(unix)]`
  gate as the map itself:

```rust
/// Drops the predecessor identity record installed for a session this daemon then failed to adopt.
#[cfg(unix)]
pub fn forget_transferred_owner(&self, session_id: &str) {
    self.transfer_owners.lock().remove(session_id);
}
```

- `daemon/server.rs` adopt-failure branch: one added call —
  `self.terminal_service.pty_manager().forget_transferred_owner(&session_id);` — beside the existing
  `release_adopted_ownership`, plus a comment saying why. No restructuring of the transfer path; the
  error string, the return, and the surrounding loop are unchanged.

### F2-8 — the 100 ms race is invisible to an artifact (fixed)

Claim verified. `ui/src/lib/paneLiveness.ts:224-229`: `Promise.race([invoke(...), new Promise<null>(
resolve => setTimeout(() => resolve(null), 100))])` — when the timer wins, `nativeSnapshot` is
`null` and nothing records that the deadline fired, so "no native telemetry" and "the IPC outran
100 ms" are indistinguishable in the copied debug artifact.

**Change** — visibility only; the 100 ms value and `classifyPaneLiveness` are untouched:

- `PaneLivenessSnapshot` gains an optional `readonly nativeSnapshotDeadlineFired?: boolean`
  (absent = the deadline never fired; the synchronous `observePaneLiveness` never sets it).
- The race now resolves a discriminated value on both arms —
  `invoke(...).then(snapshot => ({ snapshot }))` versus `setTimeout(() => resolve({ snapshot: null }), 100)`
  — so `nativeSnapshotDeadlineFired = observed.snapshot === null` records *which* arm won without
  changing the 100 ms bound or the null-handling.
- `observePaneLivenessAsync` now returns a new `PaneLivenessObservation { verdict, nativeSnapshotDeadlineFired }`
  instead of a bare verdict; the early "no session identity" return becomes
  `{ verdict: "UNKNOWN", nativeSnapshotDeadlineFired: false }` (no IPC was attempted, so no deadline
  fired), and the snapshot it classifies carries the flag.
- **Construction-site sweep** (grep over `ui/src`): `observePaneLivenessAsync` has exactly one
  consumer, `ui/src/lib/paneDebugInfo.ts:10`, updated to take the observation and emit
  `nativeSnapshotDeadlineFired` into its JSON — that JSON is the artifact the finding is about. The
  two test files that build `PaneLivenessSnapshot` literals (`paneLiveness.test.ts`) and call
  `formatPaneDebugInfo` (`paneDebugInfo.test.ts`, `TerminalSplitView.test.tsx`) keep compiling: the
  new snapshot field is optional and the sync formatter's signature is unchanged.

The verdict logic still degrades to `UNKNOWN` on a loaded machine — the degradation is now *visible*
in the artifact, which is what this finding asked for. I did **not** replace the fixed bound with a
bound derived from the event under measurement: that would change verdict semantics beyond this
finding's scope, and the brief explicitly offered the README route instead. The honest framing is
recorded here: this patch makes the loss observable, it does not eliminate it.

### F2-6 — documented only, deliberately not fixed

See section 3.

---

## 3. F2-6 — deliberate narrowing of the suspend target (needs measurement, no code change)

**This is a finding for the host window, not a code change. Nothing in `F2-REMAINING.patch` touches
the suspend or resume path.**

### Before / after semantics

**Legacy path (still live for the generic signal route):** `terminal/session.rs:1010-1055`,
`PtySession::signal`. It converts `TerminalSignal::Stop` to `SIGSTOP` and then addresses the *group*:

- `let shell_group = self.pgid().unwrap_or(pid);` (`:1032`) — portable-pty makes the child the
  session/process-group leader, so this is the shell's group;
- `libc::kill(-(shell_group as i32), sig)` with a direct-pid fallback (`:1044-1046`);
- plus a **foreground-group extension** for `Terminate`/`Kill`: `self.foreground_process_group()`
  (`:647`) read *before* signalling, filtered to `> 1 && != shell_group`, then
  `libc::kill(-(group as i32), sig)` (`:1037-1041`, `:1047-1049`).

**New identity-bound path (what auto-suspend actually uses now):**
`TerminalService::suspend_session` (`terminal/service.rs:533-545`) builds the target through
`PtySession::suspension_target` (`session.rs:572-620` — pid + incarnation + kernel start time) and
calls `suspend_verified_session` (`service.rs:547-562`) → `stop_for_owned_suspension`
(`terminal/suspension.rs:162-169`) → `unix::open(target)`:

- Linux: `pidfd_send_signal` on a pidfd (`terminal/suspension/unix.rs:71-79`) — the pidfd *is* the
  identity, and a pidfd can only address one process;
- macOS: `libc::kill(self.target.pid as i32, signal)` after `verified_observation()`
  (`terminal/suspension/unix.rs:313-321`) — start time verified immediately before the signal.

Resume is symmetric: `resume_owned_session` → `resume_owned` → the same single-pid signalling, while
the legacy route resumes with `pty_manager.signal(session_id, TerminalSignal::Continue)`
(`service.rs:568-586`), i.e. the group.

So: **`kill(-pgid, SIGSTOP)` + foreground-group extension → `SIGSTOP` to exactly one pid.** A
foreground job that job control placed in its own process group (any interactive-shell foreground
job) is no longer stopped by the new path; the shell alone stops, and the job keeps running and
consuming CPU. This is deliberate — a process group carries no verifiable identity, so the
identity-bound contract ("stop only what this daemon provably spawned, verified against kernel start
time/pidfd") cannot be extended to a group without giving up the property the path exists to provide.

### What it means for the auto-suspend feature's purpose

Auto-suspend exists to stop an unattended pane from consuming resources while nobody is looking, and
the *pane* is the unit the user experiences, not the shell process. Under the legacy semantics,
suspend ≈ "this pane is frozen". Under the new semantics, suspend ≈ "this pane's shell is frozen",
which is equivalent only when the shell has no foreground job in its own group. The worst case is the
common one for an idle-detector: a long-running foreground job that produces no output (a build, a
CPU-bound script, a hung agent) is exactly what looks "idle" to `last_output_age_ms`, and exactly the
case where the job may now survive the suspend. The failure is silent: the lifecycle registry marks
the session suspended, the UI reports `BLOCKED_IN_ATTRIBUTED_SUSPENSION` on a verified receipt, and
the pane can still be burning CPU.

Two things bound the risk, and both are unmeasured: job control only puts a foreground job in its own
group for interactive shells with job control enabled, and the identity-bound path only stops the pid
it verified (so a job in the *same* group as the shell is still stopped).

### The measurement that would settle it

- **Where:** one host that can run a local (non-remote, non-paired) pane through the real app —
  `suspend_session` refuses remote and paired-runtime sessions (`service.rs:533-536`), so it must be
  a local PTY. Either macOS or Linux (the Linux path is pidfd, the macOS path is start-time-verified
  `kill`; run it on the platform whose auto-suspend behaviour is in question, ideally both, since
  `suspension/windows.rs` is a different backend and not covered by this finding).
- **Setup:** open a local terminal pane, let auto-suspend's idle window elapse with a *foreground*
  CPU-bound job that prints nothing, e.g. `python3 -c 'while True: pass'` (or `yes > /dev/null`).
  Record, before the suspend fires, the pid/pgid of the shell and of the job
  (`ps -o pid,ppid,pgid,stat,time,comm -g <shell_pgid>` or `ps -eo pid,ppid,pgid,stat,time,comm`).
- **Trigger:** let the idle detector suspend the session (or call `suspend_session` on the same
  session) and confirm the receipt — the daemon logs/QA producers record
  `suspend_pid`/`incarnation`/`verified_actuation_receipt`, so the target pid is in the artifact.
- **Compare:** over a fixed wall interval after the suspend (e.g. 10 s), the CPU-time delta of the
  foreground job (`time` column of `ps`, or `pidstat -p <pid> 1 10` on Linux) and the `stat` column
  of both the shell (expect `T`) and the job (the question: `R`/`S` and a rising CPU time = the
  narrowing matters; `T` = it does not, because the job is in the shell's group).
- **Settles as "matters"** if the job's CPU time advances while the shell is `T`; **"does not
  matter"** if the job stops with the shell or job control in practice never moves it out of the
  shell's group for the workloads the feature is meant to catch. Only that measurement can decide,
  and it is out of scope for this lane (no builds, no GUI, no daemon runs).

---

## 4. What I could not verify

- **No compilation of any kind.** No `cargo`, `bun`, `tsc`, `vitest`, `vite`, no GUI, no daemon, no
  remote host was run — the build host is down. This patch is **statically authored and
  `git apply`-proven, not compile-proven**. The specific places a compiler would have to confirm:
  - `daemon/server.rs`: `Arc<HandoverManager>` moved into a `spawn_blocking` closure (`Send + 'static`),
    and `crate::ipc::IpcError::internal` accepting `String` (it takes `impl Display`).
  - `terminal/pty.rs`: `forget_transferred_owner` under `#[cfg(unix)]` matching the map's own gate
    (the call site is inside the unix-only handover-transfer block; a non-unix build never sees either).
  - `terminal/qa_liveness.rs` / `ipc/terminal.rs`: closure `Send` bounds (`Arc<QaBarrierChannel>` is
    already moved into `spawn_blocking` elsewhere in the same module, which is the precedent relied on),
    deref coercions `&Arc<T> → &T` and `&PathBuf → &Path`, and the `Option<Value>`/`Option<u64>` type
    inference for `run_blocking`.
  - `ui/src/lib/paneLiveness.ts`: `Promise.race` over `{ snapshot: … }` on both arms narrowing to
    `{ snapshot: Partial<PaneLivenessSnapshot> | null }`.
- **No test was executed.** The two `#[test]` functions whose call sites I rewrote, and the three UI
  tests that touch the changed types, were reasoned about by reading only.
- **The behavioural claims are static readings, not measurements:** that the fsync/rename sequence in
  `record_decision` blocks the runtime worker, that the 25 ms QA reads block a worker, that the
  100 ms timer can lose on a loaded machine, and that the QA lane's *runtime* behaviour is otherwise
  unchanged. For the last one I can only assert what is checkable statically: no timeout, budget,
  poll interval or constant was modified (`CONTROL_POLL_MS` 25, `WATCH_TICK_MS` 25, `FRAME_SUBMISSION_WINDOW_MS`
  2000, `setTimeout` 100 all untouched), and no verdict logic changed.
- **F2-6 is unmeasured by design** (section 3) — the whole point of documenting it instead of fixing it.
- **The patch does not include a regression test** for any finding. For F2-3/4/5/7 that would mean a
  test asserting "this work runs off the runtime thread", which is not observable from a unit test
  without instrumentation I was not asked to add; for F2-8 a test would pin the new field, which is
  cheap but was left out to keep the patch minimal and reviewable. Flag if the host wants one.
- **I did not and could not verify the F2-1 patch itself** (it is another lane's artifact, awaiting its
  own compile host). I only used it as the stack's first layer.
- **Base-revision drift:** the branch tip moved from `0726c70c` to `b1f249f4` while this lane worked
  (another session's scripts-only commit). Verified harmless for this patch (section 1), but the host
  should re-confirm the tip before applying, and this lane did not author that commit.
