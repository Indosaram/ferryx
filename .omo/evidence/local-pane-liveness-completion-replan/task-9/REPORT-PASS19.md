# REPORT — PASS 19 (supersedes PASS 18)

## Scope and discipline

**Authoring only.** Nothing was compiled or run: no `bun`/`vitest`/`node`/`cargo`, no build, no test run, no LSP
server, no commit, no remote or GUI action. Verification of my own edits was **read-only** — `git status
--porcelain`, `git diff --stat`, `git log -S`, and reading the patched text back. Worktree
`C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`, branch
`work/local-pane-liveness-completion-foundation`, HEAD `bc078f70`.

Pass 19 changes three things from pass 18: the **budget shape** (a bursty stall needs a sequence budget, not a
per-attempt one), the **gate wiring** (one canonical config, not a second config nothing invokes), and **the
verifier's precondition discipline applied to my own generated text** (anchor counts asserted, text read back,
interpreter stated).

---

## 1. The budget, re-sized from the measured burst shape

Measured (the verifier's 12 minimal probes + pass-17 r1):

| | value |
|---|---|
| success latency | **0.380 s** (pass-17 r1), **0.502–0.516 s** (probes p9–p12) |
| stall shape | **p1–p8 STALLED, 8 consecutive, ~73 s total**; then p9–p12 SUCCEEDED, 4 consecutive, ~2 s |
| a 6-attempt retry with fresh roots/dirs/task names | landed **entirely inside one burst** — got nowhere |

So a per-attempt retry budget cannot work; the budget has to **outlast a burst**.

| parameter | before | now | line |
|---|---|---|---|
| `interactiveRelaunchTimeoutMs: 180_000` (per attempt) | 180 s on ONE attempt | **removed** | — |
| `interactiveRelaunchEntryMarkerMs` | 8 s | **10 s** (~20× the 0.502–0.516 s success latency; never below 5 s) | `common-harness.mjs:293` |
| `interactiveRelaunchSequenceMs` (the whole retry sequence) | — | **180 s** | `common-harness.mjs:292` |
| `interactiveRelaunchAttempts` | 4 | **18** (10 s × 18 = 180 s) | `common-harness.mjs:294` |

Consequences, and why this is the right shape:

* a stalled attempt now costs **10 s + teardown** instead of **3 minutes**; one attempt can only ever spend the
  stall window plus what is left of the sequence (`runDelegationAttempt` clamps
  `markerWindowMs = min(entryMarkerMs, remaining)` at `windows-interactive.mjs:520-523`), so a single burst cannot
  eat the whole budget;
* the sequence is checked **before each attempt** (`windows-interactive.mjs:710-715`) and stops with
  `stopReason: 'sequence-budget-exhausted'` — bounded end to end, never unbounded;
* 180 s of retries **outlasts the ~73 s burst** that was measured, and it is the cheap experiment that decides
  whether anything heavier is ever justified — the condition **lifts on its own** (p9–p12 succeeded), so no reboot
  or host change is requested;
* the window is a **decision point, not a cutoff**: the marker is re-measured once before any signal
  (`windows-interactive.mjs:619`), so a marker that landed between the deadline and the teardown is still read as
  "this attempt executed" and is never ended-and-retried as a stall.

## 2. The exact change (file:line)

### `scripts/lib/qa-scenarios/windows-interactive.mjs`

| line | change |
|---|---|
| 182 | the delegated bat's **first statement that runs**: `echo DELEGATION_ENTRY %DATE% %TIME% task=<taskName> cwd=%CD% >> "<marker>"` |
| 210 | `delegationAttemptIdentity` (pure): per-attempt task name + attempt dir from `(index, token)` |
| 221 | `delegationMarkerPrecondition` (pure): a marker path that already exists is refused |
| 250 | `buildDelegationProcessProbeScript`: lists `cmd.exe` carrying this attempt's exact bat path, with its parent's identity and the **PowerShell version that produced the payload** |
| 275 | `classifyDelegationAttemptProcesses` (pure): `owned` (may be signalled) vs `refused` (reported only) |
| 311 | `summarizeDelegationLedger` (pure): attempts used, sequence budget, stop reason, interpreter, per-attempt outcome |
| 445 / 456 / 465 / 479 | `awaitEntryMarker` (event-driven bounded wait), `queryDelegationTaskState` (raw), `probeDelegationAttemptProcesses`, `endStalledDelegationAttempt` (`/end` → `/delete` by name → identity-checked exact-PID `taskkill`) |
| 515 | `runDelegationAttempt` (**exported** so the fail-closed branches are testable): one fresh delegation, the late re-measure (619), the stall branch, the typed outcomes |
| 672 | `relaunchIntoInteractiveSession`: the sequence budget (678), the pre-attempt check (710), `DELEGATION_STALLED` on the fail-closed exit (762+) |
| 796 | `admitWindowsInteractiveDesktop`: injectable `deps`, typed `code: relaunched.code ?? 'INTERACTIVE_RELAUNCH_FAILED'`, `delegation` carried into the evidence |

### `scripts/lib/qa-scenarios/common-harness.mjs`

| line | change |
|---|---|
| 292-294 | `interactiveRelaunchSequenceMs: 180_000`, `interactiveRelaunchEntryMarkerMs: 10_000`, `interactiveRelaunchAttempts: 18` |
| 394 | `TYPED_ERRORS` gains `'DELEGATION_STALLED'` |
| 827 | `APP_STDIO_CLOSE_MARKER = '[app-stdio closed:'` |
| 961-971 | `AppStdioSink.close()`: dropped-byte line, then the **completeness marker**, then `endWriteStreamBounded` |

### `scripts/qa/pane-liveness.mjs`

| line | change |
|---|---|
| 142 | `BLOCKED_CODES` gains `'DELEGATION_STALLED'` → `BLOCKED`, exit `EXIT.nativeAutomationUnsupported` (4), nonzero |
| 408 | the `DELEGATED-TO-INTERACTIVE-SESSION` stderr line reports `delegation` (attempts used + what each attempt did) |

### `scripts/qa/pane-liveness-vitest.config.mjs` — the gate (decision **(b)**)

| line | change |
|---|---|
| 15 | `include: ["scripts/qa/pane-liveness.test.mjs", "scripts/qa/pane-liveness-delegation-retry.test.mjs"]` |

**Why (b) and not (a) or (c).** The frozen gate command
`bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` is **one canonical invocation**, and
a second config would have left the retry coverage outside every gate. So the retry/sink suite is listed in that
same config: the frozen argv is untouched and the frozen command now executes **both** files.
**Counts:** `scripts/qa/pane-liveness.test.mjs` is **byte-identical to HEAD** (`git diff` empty) and still declares
**64** tests; the new file adds **15**; the one frozen command therefore runs **79**.
Why not (a) — folding into the frozen file — is answered directly: the frozen file is the verifier's frozen
artifact and its count is the contract I was told to preserve ("suite stays at 64"). Keeping it byte-identical and
adding the new coverage under the same command satisfies both the count rule and the "must actually run" rule.

## 3. How a stale marker is impossible

1. **Fresh path per attempt, by construction.** The marker lives at
   `<evidenceDir>/delegation/attempt-<index>-<token>/delegation-entry.marker`; task name, bat path, exit path and
   record path carry the same `index`+`token`. Nothing is reused across attempts or runs.
2. **A precondition, not a hope.** Before `/create`, an attempt whose marker path already exists is **refused**
   (`STALE_MARKER_REFUSED`, typed `INTERACTIVE_RELAUNCH_FAILED`, `stalled: false` → **not retried** as if the host
   had stalled) — `windows-interactive.mjs:552`.
3. **Ownership for the kill.** The stalled attempt's `cmd.exe` is signalled only when its command line carries
   **this attempt's exact bat path** *and* its parent is the Schedule service host; a candidate matching the path
   with a foreign parent is **refused and reported**, and when nothing matches, **nothing is killed** — a name or
   path pattern is never an ownership proof.
4. **Evidence per attempt.** `<evidenceDir>/windows-interactive-delegation.json` is rewritten after **every**
   attempt (index, task name, marker path, marker appeared?, wait, window, exit budget, create/run codes, the raw
   `schtasks /query` of a stalled task, the teardown, the outcome, the exit code, the stop reason, the interpreter),
   so the run reports **how many attempts it needed** and an interrupted run still carries the attempts that
   really happened.

## 4. The verifier's precondition discipline, applied to my own edits

Its three self-caught injection bugs (a missed anchor; `ArgumentList` under PowerShell 5.1; `context.isolationRoot`
where the enclosing function is `runNativeScenario(ctx)`) each surfaced only because a typed failure named them.
Copied here:

* **Anchor counts asserted, text read back.** The new suite asserts `echo DELEGATION_ENTRY` occurs **exactly once**,
  the marker path occurs **exactly once**, the runner line occurs **exactly once**, and that the marker statement
  is line index 1 (after only `@echo off`) — a silently missed or duplicated injection shows up as 0 or 2. I read
  the patched module back and counted the same anchors by hand: `echo DELEGATION_ENTRY` ×1,
  `interactiveRelaunchSequenceMs` ×5, `interactiveRelaunchTimeoutMs` ×0, `PSVersionTable` ×1.
* **Interpreter stated.** The delegation ledger records `interpreter: { node, platform, arch }`; the process probe
  payload records `powershell = $PSVersionTable.PSVersion`; both are asserted in the suite.
* **Fresh identity asserted, not assumed.** Task names, attempt dirs and marker paths are asserted distinct across
  attempts (pure identity function + the retry replay + the all-stall ledger).
* **The fail-closed path is exercised, not just described.** Three tests drive the **real** attempt runner through
  the injected `deps` seam: the stale-marker refusal (asserting **no** `schtasks`/`taskkill` call happened at all),
  the stalled attempt (asserting the exact teardown order `/query → /end → /delete → taskkill /PID 26416 /F` and
  that the foreign-parent candidate was refused), and the no-match case (asserting **nothing** was killed and the
  refusal reason is recorded).

## 5. The app-stdio sink's teardown — what I read, and the one thing I fixed

Read: `ResourceRegistry.cleanup()` closes the sink in its **logs** pass, after the process reap and **before**
sockets, servers and the removal of the registered roots; the runner calls `cleanup()` in its `finally`, and only
**after** it returns does it compute `result.appStdio = appStdioResult(appStdio)` and call `evidence.finish(result)`
(which writes `result.json`/`latest.json`). `close()` awaits `endWriteStreamBounded`, which resolves on the
stream's own `'finish'` — i.e. after every queued byte reached the OS.

* **Flushed and closed before the run dir is summarised** — ✓ already correct (cleanup → then summary; `close()`
  resolves on `'finish'`).
* **Truncation marker and dropped-byte count written before close** — ✓ already correct (truncation at cap time in
  `writeChunk`; the dropped-byte line at the top of `close()`, before `endWriteStreamBounded`).
* **A run killed mid-flight cannot leave a sink file that looks complete** — ✗ **not guaranteed before this pass**:
  the files carried no statement of completeness, so a partial file was byte-indistinguishable from a finished
  drain. **Fixed**: `close()` writes `[app-stdio closed: <name> drained <n> bytes…; this file is complete]` **after**
  every byte is queued and **before** the stream ends. Its absence is now the proof that the artifact is partial,
  and the same fact travels in the receipt (`closed: false`, `closedOk: null`) that `result.json` carries.

## 6. The tests added (15, in one new file, under the canonical config)

`scripts/qa/pane-liveness-delegation-retry.test.mjs` — bat/marker shape and frozen-argv preservation; fresh
attempt identity; stale-marker refusal (pure); the exact-path + service-parent ownership rule (with the
foreign-parent and other-attempt refusals, case-insensitivity, single-element normalization, PowerShell quoting);
the retry replay reporting **attempts used**; the all-stall `DELEGATION_STALLED` fail-closed path (typed, nonzero,
`BLOCKED`, admission-mapped, never a session-0 fallback); the deterministic-failure non-retry; the budget shape
(10 s / 18 / 180 s, and the old per-attempt name gone); the anchor-count/read-back assertions; the three real
attempt-runner branches; and the three sink tests (flush-then-mark, cannot-look-complete-without-close, registry
closes before it removes roots).

## 7. Flagged, unverified by me (needs one command from the verifier)

The frozen test asserts `expect([logReceipt.label, logReceipt.closed, logReceipt.closedOk]).toEqual(['app-stdio',
true, true])` (`scripts/qa/pane-liveness.test.mjs:208`), while `ResourceRegistry.cleanup()` pushes
`{ kind: 'log', label: entry.label, ...receipt }` (`common-harness.mjs:1081`) and `AppStdioSink.receipt()` returns
`label: this.label` (`common-harness.mjs:932`) with the sink constructed as `label: 'app'` — so the spread's
`label` ('app') appears to win over the registry's ('app-stdio'). I did **not** change either side (the frozen
test is frozen; the spread order is outside this dispatch) and my own new test deliberately asserts the receipt on
`kind`/`closed`/`closedOk`/`streams[].path` instead of the label. **This needs one run of the frozen command to
settle it** — if the assertion does fail, it is a pre-existing failure at `bc078f70`, not one my change caused.

## 8. What is still unknown

* **The cause of the stall is still unknown** (and it is bursty, not per-attempt random). Nothing here guesses it;
  the ledger's `attemptsUsed`/`stopReason` is what will measure the real shape on the host.
* Whether 10 s / 18 attempts / 180 s is the right calibration — the measurement that decides is the next real run.
* Everything here is **uncommitted** in worktree `C` (4 modified files, 1 new file).
