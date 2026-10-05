# F2-1 - Windows suspension regression (candidate-introduced, plan-requirement violation)

## 0. Regeneration (2026-10-05) - the pre-apply audit's F1 and F2, and what changed here

This artifact was attacked by `PATCH-PRE-APPLY-AUDIT.md` before it was applied, and that audit found
two `major` behaviour findings in its `daemon/session_service.rs` hunks - both about the F2-2
spawn-owner claim the patch carries. The patch in this directory is **regenerated** to fix both.
Nothing else changed: the five `terminal/**` file sections are byte-identical to the previous artifact,
and so are the `spawn_remote` signature hunk and the `daemon/server.rs` Windows-verifier hunk (verified
with `cmp`/`diff` over the two patch files).

| | |
|---|---|
| Artifact | `E/final/F2-1-suspend-windows.patch` |
| sha256 | `99c3d711ce0555fdfabbe7f3431fac89d1e6bef9c93b314dd0d436616c89351a` |
| Size | 35 103 B, 793 lines, ASCII, LF-only |
| Files | **8** (was 7): `src-tauri/src/daemon/handover.rs` is new to this patch |
| Base | `b1f249f4a710d82dedb314fd42dcc7f90316c8fd`, branch `work/local-pane-liveness-completion-foundation` |
| Method | static reading, `git` inspection, `git apply`/`git apply --check` on scratch copies only |

`git apply --numstat` over the regenerated patch:

```
86      7       src-tauri/src/daemon/handover.rs
34      3       src-tauri/src/daemon/server.rs
30      0       src-tauri/src/daemon/session_service.rs
3       1       src-tauri/src/terminal/mod.rs
2       1       src-tauri/src/terminal/service.rs
14      4       src-tauri/src/terminal/session.rs
38      1       src-tauri/src/terminal/suspension.rs
294     22      src-tauri/src/terminal/suspension/windows.rs
```

### 0.1 Audit-F1 - the claim broke spawn idempotency; fixed by ordering

**The finding.** The claim this patch added to `handle_spawn` was taken at
`daemon/session_service.rs:1742` - *above* the idempotency-cache replay and the machine
`previous`-record replay - so during the `prepare`->`commit` handover window (status `Prepared`,
`is_draining() == false`) a retry of an **already-created** spawn (same `clientRequestId`) failed with
`SpawnError::Other("Daemon handover is in progress and does not accept new sessions")` instead of
returning the session it had already created. That replay is a deliberate idempotency contract: reusing
a `clientRequestId` returns the existing session, and only a *different* fingerprint on the same id is
a conflict. The guard must not break it.

**Confirmed against the source, not only the audit's two line numbers.** Read in full, `handle_spawn`
replays in this order: the machine `previous` record (returns the completed session), then
`prune_dead_spawn_ownership` + the `spawn_idempotency_cache` hit (returns the cached session id), then a
live-session scan by `client_request_id` (returns the live session id). All three are *replays*: a
retried request must reach them before anything can refuse it. The claim sat above all three.

**Fix - the ordering chosen, and why.** The claim now sits **below all three replays and immediately
above the first branch that can create a session** (the `spawn_remote` early return):

```rust
            return Ok(live_session_id);
        }

        // Claim the in-flight spawn slot ... Ordering is load-bearing: the claim sits
        // *below* the idempotency-cache replay and the machine `previous`-record replay
        // above, and below the live-session metadata replay just above. ...
        let spawn_owner = self
            .handover_manager
            .upgrade()
            .map(|manager| manager.retain_spawn_owner())
            .transpose()
            .map_err(|_| SpawnError::Other(
                "Daemon handover is in progress and does not accept new sessions".into(),
            ))?;

        if let Some((project, host)) = remote {
```

Why exactly there, rather than anywhere else in the function:

- **Below every replay** is the requirement. Only a request that is about to create a session is a new
  session, and only a new session may be refused by the handover gate. A retry therefore keeps
  returning its cached session id while a handover is prepared, exactly as it did before this patch.
- **Above the `spawn_remote` early return** is the other half of the F2-2 wiring. Placing the claim
  after the `remote` resolution would leave the whole remote spawn path unprotected, which is the path
  the F2-2 analysis names as the long one.
- **Not lower** (for example immediately before the local `run_blocking` spawn): the `spawn_remote`
  early return is above that point, so a remote spawn would run unguarded.

The F2-2 analysis's original placement - "after the existing draining check" - is superseded, and the
code comment says so: the draining check rejects *new* sessions, and a replay is not a new session, so
the two must not share a position.

### 0.2 Audit-F2 - the guard's activation cost: bounded, typed, and pinned by a test

**The finding.** `retain_spawn_owner()` had zero production callers before this patch, so this patch is
what first activates the three `spawn_owners` gates (`prepare_handover`, `commit_handover_v4`,
`commit_handover_v5`). Consequence: any long spawn - `spawn_remote` waiting on a relay tunnel is the
named case - now blocks handover for its whole duration.

**Step 1 of the brief - is the block already bounded and retryable? Answered from the source: it cannot
hang, but it was not typed.** The `HANDOVER_BUSY` outcome was a bare `String` with no guard identity,
and the three gates returned it into three call sites with no deadline of their own:

| Call site | Code | Bounded? |
|---|---|---|
| `daemon/server.rs:4233` `Ok(DaemonRequest::PrepareHandover)` | `Err(e) => daemon_error(e)` | returns at once; the outcome is an untyped `Error { code: None, message: "HANDOVER_BUSY" }` |
| `daemon/server.rs:4357` `Ok(DaemonRequest::CommitHandover)` | `manager.commit_handover(&service)` via `run_blocking` | returns at once; same untyped `Error` |
| `daemon/server.rs:4774` `handle_upgrade_binary` | `Err(e) => return daemon_error(format!("Failed to prepare handover: {e}"))` | returns at once, but **the caller polls**: `scripts/install-macos-app.mjs` drives `upgradeBinary`, and a bare `Error` is indistinguishable from a structural prepare failure |

So no gate can hang *inside the daemon* - each returns its refusal immediately, and there is no internal
wait or deadline to inspect because none is needed. What was missing is the other half of "bounded":
the outcome was not **typed** and did not name the guard, so the one caller that polls (the installer)
could not tell "a spawn is in flight, retry" from "this handover is structurally broken, stop".

**Step 3 applied - the block is now typed, observable and retryable.** No gate's *behaviour* changed
(they still refuse while a spawn is in flight, and the spawn path still refuses while a handover is
prepared). What changed is that every refusal carries the guard's identity and maps to the typed
`DaemonResponse::HandoverRejected`:

- `daemon/handover.rs` gains `pub(crate) const HANDOVER_BUSY` and `pub(crate) fn is_spawn_gate_busy`,
  so the token has one home and a call site cannot re-implement the check as ad-hoc string matching.
- A private `HandoverManager::spawn_gate_refusal()` builds the refusal from the live guard count
  (`"HANDOVER_BUSY: {n} in-flight spawn(s) hold the handover gate"`). The three gates use it instead of
  the bare token; `retain_spawn_owner`'s own refusal names the handover status
  (`"HANDOVER_BUSY: handover is Prepared and does not accept new spawns"`).
- `daemon/server.rs` gains `handover_failure_response(context, message)`: a spawn-gate refusal becomes
  `DaemonResponse::HandoverRejected { reason }`, and every other handover failure stays the generic
  `Error` it already was, with the same message text as before. All three call sites use it.
- `daemon/handover.rs`'s `#[cfg(test)] mod spawn_owner_tests` gains
  `a_blocked_handover_reports_busy_and_succeeds_on_retry_after_the_spawn_completes` (unix-gated, like
  its neighbour). It pins the retry contract: with one spawn in flight, a prepare reports the busy
  outcome and names the guard (`1 in-flight spawn`), the manager stays `Active`, and the **same**
  prepare succeeds once that spawn's guard drops.

**The retry contract, for the caller.** A blocked handover is **retryable, not terminal**. The gate
reopens the moment the last in-flight spawn's `SpawnOwnerGuard` drops - bounded by that spawn, which is
bounded by its own work (a local PTY spawn, or a remote spawn waiting on a relay tunnel), never by an
unbounded wait inside the handover.

- **`upgradeBinary` (the installer and the desktop staleness probe)**: on
  `{"type":"handoverRejected","reason":"HANDOVER_BUSY: ..."}`, wait and re-issue `upgradeBinary`. It
  must not read the outcome as "this daemon cannot hand over". `scripts/install-macos-app.mjs` is
  unchanged by this patch - the installer's retry policy is the host operator's call, and this patch's
  job is to make the outcome distinguishable.
- **`prepareHandover` / `commitHandover`**: the same variant is the retry signal.
- **Spawn side**: a spawn that arrives while a handover is prepared is refused with the same token and
  still maps to `SpawnError::Other("Daemon handover is in progress and does not accept new sessions")`;
  the frontend retries the spawn after the handover settles. Unchanged by this patch.

**The trade-off, stated plainly.** A long spawn now delays handover for its whole duration, and that is
the intended cost of F2-2's fail-closed invariant: a handover must not commit over a spawn that is still
creating its session. What the regenerated patch removes is the *ambiguity* of that delay - a caller sees
a typed, retryable, guard-identifying refusal instead of an untyped error string.

### 0.3 What deliberately did not change

- The Windows guarantee, the strict Unix contract, the deletion of the stub-asserting test and the six
  Windows tests are exactly as §5 describes.
- The guard covers the **spawn**, not the later attach handshake (audit F3): `SpawnOwnerGuard` is bound
  in `handle_spawn` and drops when it returns, while attach is a separate request
  (`ipc/terminal.rs::cmd_terminal_spawn_operation`). §5.3's "for the whole registered-session
  lifecycle" wording is corrected below rather than silently kept.
- Audit F4 (a runtime worker can block on `status.write()` held across blocking I/O in
  `prepare_handover`), F5 (the Windows test-only verifier `OnceLock`) and F6 (the new guarantee is not
  surfaced in the QA payload the Windows contract tests read) stand as the audit recorded them; none is
  in this brief's scope.

---

Candidate worktree: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`
Branch: `work/local-pane-liveness-completion-foundation`
HEAD verified: `bcb76cfd39eb93e94cfd579d7dd2ab68117ed8ec` (`git rev-parse HEAD`)
Tree verified clean: `git status --porcelain=v1` produced no output
Candidate base: `d82b35e4` (`git merge-base --is-ancestor d82b35e4 HEAD` -> true)
Method: static reading, `git` inspection, `git apply --check` against a pristine `git archive` copy.
No build, no test, no cargo/bun/tsc/vitest/vite, no GUI, no daemon, no remote host was run.

**Branch movement during this audit (not caused by me):** while this analysis ran, another session
advanced the branch from `bcb76cfd` to `0726c70c` with two QA-only commits (`57ef3d2d`, `0726c70c`).
Neither touches any of the seven files this patch modifies (`git diff --stat bcb76cfd 0726c70c --` over
those paths is empty), and the patch was re-checked against a pristine `0726c70c` tree as well:
`git apply --check` exit 0 at both `bcb76cfd` (the commit this audit was pinned to) and `0726c70c`.
The tracked tree was clean at the end of the audit; nothing here was committed.

**Verdict: CONFIRMED, and stricter than a code-quality finding - it is a violation of the plan's own
Windows requirement (line 148) and of the plan's Windows deliverable (line 89). Blocker.**

---

## 1. The verified chain

### 1.1 Base `d82b35e4` - suspend actuated through the legacy signal path, which works on Windows

`src-tauri/src/terminal/service.rs` at base (`git show d82b35e4:src-tauri/src/terminal/service.rs`),
lines 478-487:

```rust
    pub async fn suspend_session(&self, session_id: &str) -> Result<(), PtyError> {
        if self.remote.contains(session_id) || super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Session suspend is supported only for local PTYs".into(),
            ));
        }
        self.pty_manager.signal(session_id, TerminalSignal::Stop)?;
        self.lifecycle.lock().mark_suspended(session_id.to_string());
        Ok(())
    }
```

The stop is actuated at `service.rs:484` (`self.pty_manager.signal(session_id, TerminalSignal::Stop)?`).

That reaches `src-tauri/src/terminal/session.rs`, `#[cfg(not(unix))] pub fn signal`; at base the
Windows arm is at lines 956-967:

```rust
            TerminalSignal::Stop => {
                let pid = self.pid()...;
                windows_suspend::suspend_process(pid).map_err(PtyError::Other)
            }
            TerminalSignal::Continue => {
                let pid = self.pid()...;
                windows_suspend::resume_process(pid).map_err(PtyError::Other)
            }
```

`windows_suspend` is an inline module in the same file, base line 17 (`mod windows_suspend {`), with
`pub fn suspend_process(pid: u32) -> Result<(), String>` at base line 36 and
`pub fn resume_process(pid: u32)` at base line 66. It is real FFI:
`OpenProcess(PROCESS_SUSPEND_RESUME | PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION)`, then
`ntdll!NtSuspendProcess`, then `K32EmptyWorkingSet` (working-set trim, exactly what the user's
"hibernate" intent wants), and `ntdll!NtResumeProcess` for resume.

**Correction to the brief:** the brief placed `windows_suspend::suspend_process(pid)` at
`session.rs:1070` "at base". 1070 is the *candidate* line. At base it is line 960; the shift is the
candidate's own insertions above it. The brief's substantive claim is correct: the module and both
functions are present at base and are **not** in the candidate's diff of that file (the diff's 24
hunk headers run to `@@ -1382,10 +1502,21 @@`; none covers lines ~950-1085).

`suspension.rs` did not exist at base. All three files:

```
$ git show d82b35e4:src-tauri/src/terminal/suspension.rs
fatal: path 'src-tauri/src/terminal/suspension.rs' exists on disk, but not in 'd82b35e4'
$ git show d82b35e4:src-tauri/src/terminal/suspension/windows.rs
fatal: path 'src-tauri/src/terminal/suspension/windows.rs' exists on disk, but not in 'd82b35e4'
$ git show d82b35e4:src-tauri/src/terminal/suspension/unix.rs
fatal: path 'src-tauri/src/terminal/suspension/unix.rs' exists on disk, but not in 'd82b35e4'

$ git log --oneline --diff-filter=A -- src-tauri/src/terminal/suspension.rs \
      src-tauri/src/terminal/suspension/windows.rs src-tauri/src/terminal/suspension/unix.rs
5464da0d feat(terminal): compose pane-liveness completion candidate for consolidated verification
```

`5464da0d` is the candidate's first commit. Confirmed: the whole suspension module is candidate-new.

### 1.2 Candidate `bcb76cfd` - suspend routed into a Windows stub that always errors

`src-tauri/src/terminal/service.rs:533` `pub async fn suspend_session`, body:

```rust
        let session = self.get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.into()))?;      // :540-541
        let target = crate::ipc::run_blocking(move || {
            session.suspension_target().map_err(crate::ipc::IpcError::internal) // :542
        }).await.map_err(|error| PtyError::Other(error.to_string()))?;
        self.suspend_verified_session(session_id, target).await.map(|_| ())      // :544
```

`service.rs:547` `suspend_verified_session` -> `:554`:

```rust
            let receipt = actuate_suspension(&actual, &target, super::stop_for_owned_suspension)
```

`service.rs:621` `fn actuate_suspension` compares `actual != requested` then calls the injected `stop`.
`super::stop_for_owned_suspension` is `src-tauri/src/terminal/suspension.rs:162`, whose Windows arm is
`:166`:

```rust
pub fn stop_for_owned_suspension(target: &SuspensionTarget) -> Result<ActuationReceipt, SuspensionError> {
    #[cfg(unix)]
    { ownership().lock().stop(target, &unix::open(target)?) }
    #[cfg(windows)]
    { windows::stop_for_owned_suspension(target) }
```

`src-tauri/src/terminal/suspension/windows.rs` (candidate, unmodified by me) lines 3-13:

```rust
fn unsupported() -> SuspensionError {
    SuspensionError::UnsupportedPlatform(
        "declared Windows features provide no identity-bound suspension and stop observation backend",
    )
}

pub(super) fn stop_for_owned_suspension(
    _target: &SuspensionTarget,
) -> Result<ActuationReceipt, SuspensionError> {
    Err(unsupported())
}
```

(The brief said "lines 9-14"; the `Err(unsupported())` is at 12 inside the function spanning 9-13, with
14 blank. Substantively exact.) `classify_stop_source` (`:15-19`) and `resume_owned` (`:21-23`) are
identical stubs.

**Windows Suspend therefore always fails where it previously worked.** This is a regression, not a
missing feature: at base the same user-visible action actuated a real stop.

### 1.3 Production chain, end to end (every link verified by reading)

| # | Link | Location | Verified |
|---|------|----------|----------|
| 1 | UI wrapper | `ui/src/lib/tauri.ts:626` `await invokeCommand<void>("cmd_terminal_suspend", { sessionId })` | yes |
| 2 | UI callers of that wrapper | `ui/src/lib/sessionLifecycle.ts:272` inside `suspendRegisteredSession` (`:254`); reached from the idle auto-suspend sweep `:449` (`await Promise.allSettled(confirmed.map(...))`) and from `requestSessionLifecycleAction("suspend")` (`:466`, `:474`) | yes |
| 3 | Tauri command | `src-tauri/src/ipc/terminal.rs:5298` `cmd_terminal_suspend`; registered `src-tauri/src/lib.rs:1468` | yes |
| 4 | Daemon client | `src-tauri/src/daemon/client.rs:3772` `suspend_terminal` -> `DaemonRequest::Suspend` | yes |
| 5 | Daemon handler | `src-tauri/src/daemon/session_service.rs:2440` `handle_suspend`; body `:2444` `self.terminal_service.suspend_session(session_id).await` | yes |
| 6 | Terminal service | `src-tauri/src/terminal/service.rs:533` -> `:544` -> `:547` -> `:554` | yes |
| 7 | Platform backend | `suspension.rs:166` -> `suspension/windows.rs:12` `Err(UnsupportedPlatform)` | yes |

**One correction to the brief:** `ipc/terminal.rs:5301` is the closing brace of the parameter list; the
`daemon_client.suspend_terminal(&session_id).await` call is at `:5302`. Every other line number in the
brief (`service.rs:478/533/547/554`, `session.rs:1070`, `suspension.rs:162`, `suspension/windows.rs:9`,
`tauri.ts:626`, `client.rs:3772`, `session_service.rs:2444`) checked out exactly.

### 1.4 Impact severity, stated honestly

The failure is **fail-safe, not destructive**: suspend returns an error, so the session stays running.
The blast radius today is narrower than "a broken button":

- No UI component in `ui/src` invokes `requestSessionLifecycleAction("suspend")`. The only production
  caller of that function is `ui/src/components/TabBar.tsx:402` with `"restart"`. So there is no
  clickable Suspend control wired right now.
- The reachable production paths are the **idle auto-suspend sweep** (`sessionLifecycle.ts:449`) and
  any programmatic `suspendRegisteredSession` call. On failure the sweep's caller logs
  `console.warn("Failed to suspend session", error)` (`sessionLifecycle.ts:474-476`) - so on Windows
  the feature silently no-ops with a console warning, and the "sleep idle sessions" behavior is simply
  gone.

So: no session is lost, but a shipped capability is silently dead on one platform, and the plan's
Windows requirement is violated. That is a blocker on the plan's terms regardless of the narrow UI
reach.

---

## 2. Is RESUME affected too? - NO, the audit's claim "Resume still works" is CORRECT

Traced end to end; resume never touches the new module.

| # | Link | Location | Verified |
|---|------|----------|----------|
| 1 | UI wrapper | `ui/src/lib/tauri.ts:631` `cmd_terminal_resume` | yes |
| 2 | Tauri command | `src-tauri/src/ipc/terminal.rs:5306` `cmd_terminal_resume`; registered `src-tauri/src/lib.rs:1469` | yes |
| 3 | Daemon client | `src-tauri/src/daemon/client.rs:3791` `resume_terminal` -> `DaemonRequest::Resume` | yes |
| 4 | Daemon handler | `src-tauri/src/daemon/session_service.rs:2447` `handle_resume`; body `:2451` `self.terminal_service.resume_session(session_id).await` | yes |
| 5 | Terminal service | `src-tauri/src/terminal/service.rs:580` `resume_session`; body `:587-591` | yes |
| 6 | Platform | `manager.signal(&id, TerminalSignal::Continue)` -> `session.rs:1077` `windows_suspend::resume_process(pid)` | yes |

`service.rs:580-592` (candidate):

```rust
    pub async fn resume_session(&self, session_id: &str) -> Result<(), PtyError> {
        if self.remote.contains(session_id) || super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Session resume is supported only for local PTYs".into(),
            ));
        }
        let manager = self.pty_manager.clone();
        let id = session_id.to_owned();
        crate::ipc::run_blocking(move || manager.signal(&id, TerminalSignal::Continue)
            .map_err(crate::ipc::IpcError::internal)).await
            .map_err(|error| PtyError::Other(error.to_string()))?;
        if let Some(session) = self.get_session(session_id) { session.set_suspension_receipt(None); }
        self.lifecycle.lock().mark_running(session_id.to_string());
        Ok(())
    }
```

The candidate rewrote the `run_blocking` offload and added the receipt clear, but kept
`TerminalSignal::Continue` as the actuation, i.e. the working legacy path. **Resume still works on
Windows.**

**But there is a second, latent defect:** `resume_owned` in `suspension/windows.rs:21-23` is *also* an
unconditional `UnsupportedPlatform` stub, and unlike the UI resume path it *is* reachable from
production code: `service.rs:563` `resume_owned_session` (the identity-bound, classify-then-resume
variant) is called at `src-tauri/src/terminal/qa_liveness.rs:624`, and `qa_liveness` is compiled only
under `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`
(`src-tauri/src/terminal/mod.rs:38-39`). So today it is QA-gated, not user-facing - but the plan
requires actual Windows suspend/resume contract tests (line 89), and this is the very code those tests
would have to exercise. The stub also makes `suspension-ownership`'s owned path unpassable on Windows.

Net: the *user-facing* regression is Suspend-only. The *plan-requirement* regression covers both
Suspend and the identity-bound Resume contract.

---

## 3. What the plan actually requires - it is not merely "an honest error"

The plan is not silent, and its wording contradicts option (c) as a primary fix.

**line 148** (todo 4 "Preserve retained identity, suspension authority and handover readers", Work
clause), verbatim:

> "run existing handover contract first, fix only observed missing guarantees. Predecessor parked;
> provisional successor must relinquish readers/handles before predecessor resumes. Ambiguous commit
> consults authoritative transaction decision; ambiguous abort never blindly resumes. Reuse handover
> transaction machinery, no replacement protocol. **Windows uses its actual process suspend/resume and
> session-host mechanisms**; Unix signal code stays behind platform modules."

**line 89** (Concrete native scenario contract), verbatim:

> "**Mac runs all nine scenarios; Windows native runs split-happy, split-attach-stall and split-cancel
> for portable changes, plus actual suspend/resume platform contract tests.** Agent executes OS events,
> not browser/UDS substitutes."

**line 150** (todo 4 Acceptance), verbatim:

> "matching incarnation survives epoch change, changed identity not adopted; **suspension attribution
> survives legitimate transfer**; cross-process partial failures never produce dual readers or discard
> live workload."

**line 151** (todo 4 QA failure list), verbatim:

> "QA failure: lost abort reply, partial adoption/commit rejection/successor exit, **manual stop, changed
> process identity**."

**line 71** (Verification strategy), verbatim:

> "**Mac/Windows compile and affected tests mandatory**, Linux affected Unix scopes mandatory."

And `AGENTS.md` (project-wide, and the cross-platform premise the brief cites):

> "**Cross-Platform Premise**: Every implementation must assume cross-platform execution (macOS,
> Windows, Linux). Never land macOS-only input/IME/shell paths without a portable abstraction;
> platform-specific code must be isolated behind explicit platform modules **with working fallbacks on
> all other targets**."

### 3.1 The candidate locked the opposite in with a test

`suspension/windows.rs` (candidate) contains:

```rust
    #[test]
    fn unsupported_backend_never_claims_or_resumes_a_stop() {
        let target = SuspensionTarget { pid: 42, incarnation: "pane-a".into(), started_at_unix_ms: Some(100) };
        assert!(matches!(stop_for_owned_suspension(&target), Err(SuspensionError::UnsupportedPlatform(_))));
        assert!(matches!(classify_stop_source(&target), Err(SuspensionError::UnsupportedPlatform(_))));
        assert!(matches!(resume_owned(&target), Err(SuspensionError::UnsupportedPlatform(_))));
    }
```

This asserts the exact inverse of line 148 ("Windows uses its actual process suspend/resume") and of
the line 89 Windows deliverable. Left in place, the suite would encode the regression as intended
behavior, and any future attempt to implement the plan's requirement would show up as a *test failure*.

### 3.2 Does the plan's Windows requirement include the `suspension-ownership` scenario?

**No - it includes the platform contract tests, not the scenario.** This is the reading the two lines
support when read together:

- line 89 enumerates the Windows native matrix exhaustively: "Windows native runs **split-happy,
  split-attach-stall and split-cancel** for portable changes, **plus actual suspend/resume platform
  contract tests**". `suspension-ownership` is not in that list; "Mac runs all nine scenarios" is the
  clause that owns it.
- line 229 (Success criteria) maps the scenario to a Mac path: "| IS-1 | 2,4,5,6,9,10 |
  retained-handover, handover-abort, **suspension-ownership**, diagnostic-classifier EOF, packaged
  retained smoke | E/task-4,5,9,10 |", and line 89 assigns the nine-scenario matrix to Mac.
- line 86 defines the scenario's oracle in terms a screenshot/OS-action runner can check:
  "| suspension-ownership | Ferryx-owned stop versus externally stopped process, changed foreground
  target and reused identity controls | Only matching actuated targets auto-resume, external stops
  unchanged; explicit user Resume preserved; no PID-only authority |".

Consequence for the Windows matrix, either way:

- As the plan is written: Windows owes **actuation that works** (line 148) plus **contract tests** that
  assert the identity-bound properties (line 89). The Windows matrix does not need a native
  `suspension-ownership` GUI run. My patch satisfies exactly this: the Windows backend actuates real
  stops and its unit tests assert refusal of unverifiable identities, no receipt without actuation,
  resume only of an owned stop, and `Unknown` rather than a false `External`.
- If the reviewer reads line 89's "actual suspend/resume platform contract tests" as including the
  scenario's external-stop clause on Windows, then the honest limit is that **Windows cannot observe a
  stop**, so the *external* half of the scenario is unprovable there (see section 5.2). That would be a
  finding against the plan (an unprovable clause on a required platform), not something a stub fixes.
  My patch at least makes the *owned* half pass and never lets an unobserved stop be reported as
  `External`.

---

## 4. Options assessed

### (a) Windows falls back to the previously-working actuation, receipt honestly marked

Restores line 148 and the line 89 deliverable. Requires the `windows_suspend` FFI to be reachable from
the suspension backend, an identity gate to stand in for the start-time check Windows does not expose,
an ownership ledger, and rewriting the stub-asserting test.

- Pros: preserves the user-facing function; satisfies "Windows uses its actual process suspend/resume";
  keeps the receipt honest; strictly better than the stub on every axis.
- Cons: the receipt carries a *weaker* guarantee than Unix (`stop_observed: false`), so the
  `suspension-ownership` scenario's strongest assertion remains Mac-proven only. Windows identity rests
  on the daemon's own PTY registry (PID + incarnation) rather than a kernel start-time token.

### (b) A real Windows backend that satisfies identity-bound suspension **with stop observation**

- Pros: the literal reading of line 148 plus the strongest reading of line 150.
- Cons and the provable limits: **the observation half is not achievable with supported Windows APIs.**
  `NtSuspendProcess`/`NtResumeProcess` mutate a per-process suspend count that no documented API
  exposes; `NtQueryInformationProcess` has no suspend-count class; `Thread32First`/`Thread32Next`
  enumerate threads but expose no suspend count; `SuspendThread` returns the *previous* count for a
  thread, which is not an observation of the process's post-state and is subject to the same
  race the `Ownership::stop` contract exists to close. A start-time identity token would need
  `GetProcessTimes` creation time (100 ns, ~15.6 ms granularity - weaker than the Linux start-tick token
  and comparable to the macOS start time the code already accepts), and Windows has no `pidfd`
  equivalent, so the PID-reuse window is bounded but not eliminated - exactly the macOS situation
  already documented in `suspension/unix.rs` ("kernel start time is the identity authority.
  Verification immediately before kill bounds, but cannot eliminate, the PID-reuse window").
  A hypothetical `NtQueryInformationProcess` `ProcessBasicInformation` + `PS_PROTECTION`-style trick or a
  debug-object (`DebugActiveProcess`) hold could give stronger pins, but a debugger attach changes the
  target's observable state and would break the PTY's own child management. None of this can be
  compile-verified in this session (build host down) and none of it is in the candidate.

**Judgement: (b) is not feasible as a compile-verified, in-candidate change, and the stop-observation
half is not achievable at all on Windows.** If a genuine identity-bound *observed* guarantee is deemed
mandatory, that is a finding to raise **against the plan**, not something to fake with a stub.

### (c) Keep the strict contract and make the Windows failure honest at the surface

- Would require a typed error the UI presents and gating the UI so an always-failing action is not
  offered. Call sites that would need a gate: the idle auto-suspend sweep
  (`ui/src/lib/sessionLifecycle.ts:449` and `:474`), `suspendRegisteredSession` (`:254`) via
  `requestSessionLifecycleAction` (`:466`), and the wrapper `ui/src/lib/tauri.ts:624`.
- **Contradicts line 148**, which requires Windows to use its actual suspend/resume, and line 89's
  Windows contract tests. It also contradicts `AGENTS.md`'s "working fallbacks on all other targets".

**Verdict on (c): not acceptable as the primary fix.** At most a stopgap while (a) or (b) lands, and
even then it would leave line 148 unmet.

### Recommendation

**Option (a)**, delivered as the patch in this directory. It is the minimum acceptable fix on the
plan's own wording: it restores "Windows uses its actual process suspend/resume" (line 148), it makes
the line 89 Windows contract tests assert real actuation, and it keeps the receipt honest by naming the
one guarantee Windows cannot provide instead of claiming it. It removes the test that asserted the
inverse. It is the honest maximum achievable in this candidate; (b)'s stop-observation half is
unachievable on Windows, and (c) is excluded by the plan's own text.

**Plan deviation to report:** line 148's "identity-bound" is only partially satisfiable on Windows. The
patch delivers PID+incarnation binding via the daemon's own registry, not a kernel start-time token,
and `stop_observed: false` on every Windows receipt. If the plan owner requires the observed guarantee
on Windows, line 148/150 need amending - that is a plan finding, and the patch does not pretend
otherwise.

---

## 5. The patch - what it does and what it deliberately does not

Artifact: `F2-1-suspend-windows.patch` (unified diff at `b1f249f4`, **8** files; regenerated - see §0).

### 5.1 The guarantee is now explicit in the type system

`ActuationReceipt` (`suspension.rs:29`) gains two fields:

```rust
    /// True only when the backend positively observed the stopped state on the
    /// verified identity after actuation.
    pub stop_observed: bool,
    /// What this backend can prove about the stop it just actuated.
    pub guarantee: StopGuarantee,
```

and a new enum:

```rust
pub enum StopGuarantee {
    /// Identity-bound actuation with positive kernel observation of the stop.
    IdentityBoundObservedStop,
    /// Identity-bound actuation with an unverified stop: the platform has no way to
    /// observe the stopped state, so ownership rests on the pre-actuation identity
    /// check plus daemon-local bookkeeping.
    IdentityBoundUnverifiedStop,
}
```

- Unix (`suspension.rs` `Ownership::stop`) always reports `stop_observed: true` /
  `IdentityBoundObservedStop`, because that function returns only after re-observing the verified
  identity in the stopped state. **The Unix contract test keeps asserting the strict guarantee** - the
  patch adds exactly that assertion to `receipt_requires_successful_acknowledged_actuation`.
- Windows always reports `stop_observed: false` / `IdentityBoundUnverifiedStop`.
- `SuspensionReceiptWire` (`session.rs:257`) carries `stop_observed` with `#[serde(default)]`, so a
  handover snapshot written by a predecessor that predates the field still deserializes (it degrades to
  the weaker guarantee rather than failing the handover). Its `receipt()` re-derives `guarantee` from
  `stop_observed`, so the mapping has one home.

### 5.2 The Windows backend now actuates, and refuses what it cannot attribute

`suspension/windows.rs` is rewritten from the three-function stub into a real backend:

- `install_ownership_verifier` (installed once at daemon boot, `daemon/server.rs` right after
  `TerminalService::new`) supplies the daemon's own PTY registry as the ownership authority: a PID is
  the daemon's only if some registered session reports that PID *and* that incarnation.
- `verified()` refuses with `IdentityMismatch` for pid 0 / empty incarnation, and with `NotOwned` when
  no authority is installed or the registry does not vouch for the pair - **before** any actuation.
- `stop_for_owned_suspension` actuates through the pre-existing, base-era
  `crate::terminal::session::windows_suspend::suspend_process(pid)` (module visibility widened to
  `pub(crate)`, FFI body untouched), then mints a receipt with the reduced guarantee.
- `classify_stop_source` returns `FerryxOwned` only for a stop this daemon actuated and still owns for
  the same target, and **`Unknown` otherwise - never `External`**, because Windows cannot prove someone
  else's stop either. `Unknown` is never auto-resumed, so the safe direction is preserved.
- `resume_owned` resumes only an owned stop and clears the ledger.
- The stub-asserting test `unsupported_backend_never_claims_or_resumes_a_stop` is **replaced** by six
  tests asserting the new contract: unverifiable identity refused before any actuation; actuation mints
  a receipt that admits the missing observation; a failed actuation mints no receipt and claims no
  ownership; only an owned stop is resumed; an unprovable stop is never reported as `External`; a
  reused PID with a new incarnation revokes the owned stop.

What the patch **cannot** prove, and does not claim: that the stop took effect (`stop_observed: false`),
and that the target is not a reused PID beyond the registry's PID+incarnation answer. There is no
kernel start-time token on Windows, so `SuspensionTarget.started_at_unix_ms` stays `None` on Windows
(the daemon's `suspension_target()` already computes it only for Linux/macOS).

### 5.3 The same patch also carries the F2-2 spawn-owner fix

Because the two fixes live in different files and one artifact was requested, the same `.patch` also
wires `retain_spawn_owner()` into the production spawn path (see `F2-2-SPAWN-OWNER-GUARD.md`). It is
separable: the `daemon/session_service.rs` hunks (plus this patch's `daemon/handover.rs` and
`daemon/server.rs` additions) belong to F2-2, and the regenerated artifact also fixes the pre-apply
audit's F1 and F2 there - see §0.1 and §0.2. Scope correction for the claim this section used to make:
the guard is held for the **spawn** (`handle_spawn`, including `spawn_remote`), not for the whole
registered-session lifecycle, because the attach handshake is a separate request (audit F3).

---

## 6. What I could not assess

1. **No compile and no test of any kind was run** (maho-win is down; local builds are forbidden by
   standing instruction). The patch's Rust is statically reasoned only. Every construction site of the
   changed types was enumerated by grep and updated (`ActuationReceipt` literals at `suspension.rs:144`,
   `session.rs:276`, `service.rs:663`; `SuspensionReceiptWire` literals at `session.rs:562` (`set_suspension_receipt`, the production site) and
`session.rs:1516` (the round-trip test); new
   `StopGuarantee` added to the `terminal/mod.rs` re-export so `super::super::StopGuarantee` resolves),
   but a compiler has not confirmed it. A Windows `cargo check` and the Windows contract tests remain
   mandatory per plan line 71 and are **not** satisfied by this artifact.
2. **Windows runtime behavior is unverified.** Whether `NtSuspendProcess` on the shell PID visibly
   freezes the pane's workload, and whether `K32EmptyWorkingSet` is the right companion (it was in the
   base-era code, so behavior is preserved rather than newly chosen), were not measured.
3. **Whether the stop reaches descendants.** `suspend_process(pid)` targets one process, not a job
   object. The base-era path had the same scope, so this is not a new defect, but I did not verify
   whether Ferryx spawns a job object whose whole tree should be suspended.
4. **Whether the idle auto-suspend sweep is currently armed.** I found the sweep and its callers but did
   not trace the arming conditions (monitoring start, timeouts, feature gates), so "the user-visible
   impact is limited to the sweep" is a statement about reachability, not about current activation.
5. **No assessment of the Windows scenario matrix beyond the plan text.** I did not run any scenario,
   and I could not confirm from the plan whether its owner intends the `suspension-ownership`
   external-stop clause to be proven on Windows (section 3.2 gives both readings and their
   consequences).
6. **The `resume_owned_session` QA path** (`qa_liveness.rs:563-640`) was read but not exercised; its
   behavior under the new Windows backend is inferred from the code, not observed.
7. **Option (b)'s feasibility is argued from API documentation and the codebase, not from a Windows
   experiment.** I did not test any `NtQueryInformationProcess`/`Thread32*` approach.
