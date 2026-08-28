# Terminal Typing Recovery — Ferryx native pane (2026-08-28)

Typing into a native terminal pane was dead / stole focus back to tab 1. Five stacked causes were
found and fixed. This document is the delivered record.

## Cause chain (each independently reproducible in the trace)

| # | Cause | Fix (file:symbol) |
|---|-------|-------------------|
| 1 | Attach/detach lifecycle race detached the pane right after mount | `ui/src/lib/nativeTerminalLifecycle.ts` generation tokens |
| 2 | `cmd_remote_set_active_selection` payload sent both serde alias pairs (`tabId`+`activeTabId`, `tabs`+`terminalTabs`) -> "duplicate field `tabId`" on every focus change | `ui/src/lib/tauri.ts::publishFocusedTerminal` serializes each field once |
| 3 | `ensureTabForWorktree` re-activated the root-worktree tab on every worktree refresh (~1/s), yanking focus and unmounting the pane being typed into | `ui/src/state/workspaceStore.ts::ensureTabForWorktree` enforces tab existence only, emits `worktree.ensure.skipped` |
| 4 | Keystroke -> refresh feedback loop from unstable runtime callbacks | `ui/src/state/workspaceRuntime.ts` callbacks stabilized via refs, deps narrowed |
| 5 | Mount attach had no retry: one failure (workspace swap / daemon epoch change) left the pane permanently detached, and DOM focus sat on `document.body` after swaps | `NativeTerminalPane.tsx`: bounded retry (5 attempts, `min(4000, 250*2^n)` ms), `restoreFocusIfLost()`, capture-phase printable-key fallback |

Additionally `DaemonRequest::RemoteSetActiveSelection` is now classified retry-safe
(`src-tauri/src/daemon/client.rs::request_is_retry_safe`), so a daemon restart's broken pipe
self-heals instead of surfacing `ambiguousDelivery` to the UI.

## Failing-first evidence (mutation proofs, RED -> GREEN)

- Retry: forcing `if (false && retryCount < maxRetries)` -> `AssertionError: expected 1 to be 2`; restored -> pass.
- Key fallback: forcing the capture guard false -> `expected "spy" to be called with arguments` (send_input never invoked); restored -> pass.
- Retry-safe: removing the enum variant -> `test result: FAILED. 0 passed; 1 failed`; restored -> `ok. 1 passed`.
- Alias duplication (adjacent surface): re-adding `activeTabId`/`tabs` to the selection payload -> `Tests 2 failed | 21 passed`; restored -> `23 passed`.
- Activation steal (adjacent surface): forcing `if (false)` in place of the `hasValidActiveTab` guard -> `Tests 1 failed | 42 passed` with a tab-id mismatch assertion; restored -> `43 passed`.

Because Vite HMR pushes repo edits into the running desktop app, the alias-duplication mutation also
reproduced the bug at the real surface: trace run `eec02cdb` logged 6x
`workspace.runtime.error ... duplicate field 'tabId'`, and every run after the restore
(`9d812f5e`, `5a6d68f4`, `ab3a3f7a`) logged zero. Keep mutation windows to one test-file run and
restore in a `finally` block.

## Gate gap found late (and the trap that hid it)
`bun run --cwd ui test` was green at 1024/1024 while `tsc` was RED with 3 errors: vitest does not
typecheck. The errors were mine - the two tests I added to `workspaceRuntime.test.tsx` pass the
local `createServices()` mock into `useWorkspaceStore`, the stricter consumer, and the mock omitted
`getTerminalCwd` (required on `WorkspaceServices` since HEAD); plus an unused `imeAnchor`
destructure. Fixed by adding `getTerminalCwd: vi.fn(async () => worktree.path)` to the mock and
renaming the unused prop binding to `_props`.

The trap: `bun run --cwd ui build 2>&1 | tail -20` reported `rc=0` because a pipeline's exit status
is the LAST command's (`tail`). Always run gates with `set -o pipefail` (or without a pipe).

## Daemon-restart self-heal, proven at the DOM layer too
`NativeTerminalPane.sendInput` catches an IPC failure, emits `input.error.recovering`, re-attaches via
a per-session dedup map (`sessionInputRecoveries`), restores focus, and replays the input exactly
once. Mutation proof: `if (!isRetry)` -> `if (false)` turns the test "self-heals detached session on
send_input error by re-attaching and retrying input once" from `Tests 1 passed` into `Tests 1 failed`.

Run mutations in a SANDBOX, not in place:
```bash
rsync -a --exclude node_modules --exclude dist ui/ /tmp/ulw-ui/
ln -sfn "$PWD/ui/node_modules" /tmp/ulw-ui/node_modules
cd /tmp/ulw-ui && bun run test <file> -t "<name>"   # mutate here; repo file stays untouched
```
This removes the HMR leak entirely and eliminates the restore step.

## Trace vocabulary for "typing is dead" (all emitted by NativeTerminalPane)
| Event | Meaning when you see it |
|---|---|
| `terminal.surface.input.capture` | a keydown reached the webview (logs the key + activeElement) |
| `terminal.surface.input.sent` | IPC delivered the input (backendSessionId, textLength) |
| `terminal.surface.input.dropped` | pane not visible or no `targetSessionId` - attach never completed |
| `terminal.surface.input.error.recovering` | backend rejected it; re-attach in progress (daemon restart) |
| `terminal.surface.input.recover.failed` / `.retry.failed` | self-heal exhausted; error banner shown |
| no `input.*` at all | keys never reached the webview - native first-responder problem |

Final gates, true exit codes: vitest 112 files / 1024 tests rc=0; `bun run --cwd ui build`
(tsc + vite) rc=0; `cargo test --lib` 339 passed rc=0.

## Gates

- `bun run --cwd ui test` -> 112 files, 1024 tests, rc=0
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` -> 339 passed, rc=0
- LSP: 0 errors on the four changed files
- Live trace after HMR: 0 attach errors, 0 runtime errors, `worktree.ensure.skipped` active (no tab steal)

## How to diagnose this class again

`/tmp/ferryx-switch-debug.jsonl` (dev-only `switchDebug` -> `cmd_switch_debug_log`) is the instrument.
Aggregate **per `runId`** — the file accumulates across app restarts, so a whole-file `grep -c` reports
historical failures as current. Key events: `terminal.surface.attach.error/complete`,
`terminal.surface.input.capture` (includes `activeElement`, which exposed the body-focus bug),
`worktree.ensure.skipped`, `workspace.runtime.error`. A UDS write to a scratch daemon session proves the
daemon -> PTY -> ring leg independently of the DOM.


## OPEN DEFECT (found 2026-08-28 16:55, edit deliberately deferred)
`SurfaceHost::render_snapshot` calls `self.target.restore_first_responder(window)` after EVERY
`frame.present()` (surface_host.rs:1206), and `restore_webview_first_responder`
(platform/macos.rs:256-292) calls `ns_window.makeFirstResponder(Some(&webview_view))`
**unconditionally** - no check of the current first responder, no one-shot flag, no rate limit.
While a terminal streams output that is ~60 reassignments per second on the main thread.

Verified facts (read from source):
1. per-frame call site, unconditional callee;
2. `webview_view` is `window.ns_view()`, i.e. the container - not necessarily the inner
   key-handling content view;
3. `FerryxNativeTerminalView` never overrides `acceptsFirstResponder`, so NSView's default
   (false) applies and the child view cannot normally steal focus by itself.

### Hypothesis MEASURED and largely DISPROVEN (offscreen AppKit probe, /tmp/frtest/main.swift)
I suspected a 60Hz resign/become cycle cancelling IME marked text. A Swift probe
(`NSApplication.setActivationPolicy(.prohibited)`, window never ordered front, so the user's
desktop is untouched) measured it directly with an NSView subclass counting
become/resignFirstResponder:

| Case | Setup | Result |
|---|---|---|
| A | `makeFirstResponder(sameView)` x61 | `become=1 resign=0` -> AppKit SHORT-CIRCUITS for an already-current responder |
| B | inner view focused, then `makeFirstResponder(container)` x60 | inner `resign 0->1`, container `become 0->1`, remaining 59 calls no-op |

So the per-frame call is NOT a per-frame resign/become cycle and CANNOT cancel IME marked text
every frame. The Korean-IME mechanism I hypothesised is wrong; do not re-derive it.

What survives (downgraded, still real): CASE B shows the FIRST call after an inner content view
gains focus DOES yank first responder up to the container - one unnecessary focus steal per native
surface mount, not 60/sec. That remains a credible contributor to the `activeElement: "BODY/"`
observed on every captured keystroke, and it is a testable discriminator: if a user typing test
still logs `input.capture` with `BODY`, this steal is implicated.

Recommended fix (minimal, low priority): skip the call when the window's current first responder is
already inside the webview's view hierarchy - preserving the original cure (reclaim focus after the
native surface grabs it) while removing the redundant steal. Justification is "one avoidable focus
steal per mount", NOT an IME hazard.

Why it is not applied yet: `cargo-tauri tauri dev` (pid 35167) parents the running GUI (pid 17754)
and rebuilds + restarts the app on any Rust source edit. Applying it now would destroy the pending
C4 manual typing test. Apply after that test, so the test also serves as pre-fix evidence.


## C1 audit: spec pointer corrected, coverage gap closed, UX defect found
The criterion named `NativeTerminalPane.lifecycle.test.tsx` for the retry case. WRONG FILE - that
file has no retry test. The retry case is `NativeTerminalPane.test.tsx:2054` "retries mount attach
on rejection with exponential backoff and attaches on eventual success".

Its assertions vs the criterion's three stated observables:
| Observable | Status before audit |
|---|---|
| >=3 attach attempts | covered, and stronger: exact backoff schedule 250ms then 500ms |
| pane ends attached | covered: 3rd attach resolves, no 4th attempt within 5s |
| no error banner | NOT ASSERTED - real coverage gap |

Closed by adding `expect(queryByRole("alert")).not.toBeInTheDocument()` after the
no-further-retries window. `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx` ->
50/50 passed rc=0.

### DEFERRED UX DEFECT (measured, not speculated)
Sandbox probe of the banner across the retry sequence:
- after attach failure 2: alert text = `Failed to attach native terminal`
- after eventual success: `NONE`

So a transient attach failure that self-heals within 250ms FLASHES an alarming error banner at the
user. "No error banner" is true only for the end state. Correct fix: suppress the banner while
bounded retries remain, surface it only once retries are exhausted. Deferred because editing
NativeTerminalPane.tsx makes Vite HMR remount panes in the user's running app, which would corrupt
the pending C4 manual-typing evidence.


## Coverage gap closed: input routing across a workspace switch
The deliverable's headline scenario ("typing reaches the PTY after workspace switches") had NO
DOM-layer regression lock. An audit of every test combining `rerender` with `send_input` found
exactly one, and it covers null `backendSessionId` - nothing asserted that a keystroke after a
session swap reaches the NEW session rather than the stale one, which is the user-reported symptom
"typing in tab 4 goes back to tab 1" at the input-routing layer.

Added `NativeTerminalPane.test.tsx` -> "routes a keystroke to the swapped-in session after a
workspace switch, never the outgoing one":
1. mount session A, wait for A's attach (shape-agnostic predicate over the mock call list)
2. rerender with session B, wait for B's attach
3. `mockClear()`, blur to `document.body` (forces the capture-phase path seen in the real traces)
4. dispatch one `keydown`
5. assert `cmd_native_terminal_send_input` with `{ sessionId: "switch-session-incoming", input: { text: "x" } }`
6. assert it was NEVER called for the outgoing session

No fixed sleeps, no timing luck: both waits are `waitFor` predicates over recorded calls.

### Mutation proof (RED -> GREEN)
Realistic stale-closure mutation applied in the `/tmp/ulw-ui` rsync sandbox (never the repo, so Vite
HMR cannot leak a mutation into the running app):
```ts
const rawTargetSessionId = session ? (session.backendSessionId ?? null) : (sessionId ?? null);
const stickyTargetRef = useRef<string | null>(null);
if (stickyTargetRef.current === null) stickyTargetRef.current = rawTargetSessionId;
const targetSessionId = stickyTargetRef.current;
```
RED: `rc=1`, `AssertionError: expected false to be true` at the incoming-session attach wait.
Repo `NativeTerminalPane.tsx` sha256 verified identical before and after. Sandbox removed.

## Final gate state (true exit codes, no pipe-swallowed status)
| Gate | Result |
|---|---|
| `bunx tsc --noEmit` | rc=0 |
| `bun run --cwd ui test` | rc=0 - 118 files / 1051 tests |
| `cargo test --lib` | rc=0 - 339 passed, 0 failed |
| LSP errors on changed files | none |

The UI suite grew 112 files/1024 tests -> 118/1051 during this work; the increase is the parallel
daemon workstream's tests plus this lock. Their Rust also compiles and passes, so no breakage is
attributable to them.

## If the manual typing test fails: layer-specific diagnosis table
Read `/tmp/ferryx-switch-debug.jsonl` and match the FIRST condition that holds:
| Trace signature | Failing layer | Next probe |
|---|---|---|
| no `terminal.surface.input.*` at all | keys never reach the webview | native first responder / WKWebView focus; check `workspace.render` for the pane |
| `input.capture` present, no `input.sent` | DOM handler ran, send suppressed | `input.dropped` reason field: missing `targetSessionId` or pane not visible |
| `input.dropped` | pane has no attached backend session | `terminal.surface.attach.error` count + retry exhaustion |
| `input.error.recovering` then `input.retry.failed` | IPC path broken | daemon liveness, `describeSession.endSequence` |
| `input.sent` present but no ring growth | daemon/PTY side | UDS `describeSession` delta vs `/tmp/ulw-c4-baseline.json` |

Event vocabulary is real and verified in `NativeTerminalPane.tsx`: 377 `input.dropped`,
394 `input.sent`, 402 `input.error.recovering`, 420 `input.recover.failed`,
428 `input.retry.failed`, 633 `input.capture`.

## Deferred fixes (queued, deliberately NOT applied)
Both would disturb the running app and destroy the pending manual-test evidence.
1. ~~Suppress the attach error banner while bounded retries remain~~ **APPLIED 2026-08-28 19:16**
   (safe once C4 had been captured; see the resolution section at the end). Measured defect: banner
   flashed ~250ms during successful self-heal.
2. Guard `restore_webview_first_responder` to skip when the current first responder is already inside
   the webview (`src-tauri/src/platform/macos.rs`; `cargo tauri dev` would rebuild and restart the
   app). Justification is one avoidable focus steal per native surface mount - NOT an IME hazard,
   which was measured and disproven.


## REAL-SURFACE confirmation: the tab-steal fix works in the running app
Trace run `3d3be635` (a fresh app run: `workspace.state.init`, preload, 6x `workspace.render`,
2x `terminal.surface.attach.start`, ZERO `terminal.surface.attach.error`) contains:
```
worktree.ensure.skipped  activeTabId=tab:1c3570e8...  activeTabCount=4  (generation 1, allowCreate=false)
worktree.ensure.skipped  activeTabId=tab:1c3570e8...  activeTabCount=4  (generation 2, allowCreate=true)
```
The `ensureTabForWorktree` guard fired twice against 4 real tabs and skipped activation both times,
preserving the user's active tab. This is the "no tab-focus theft" half of the deliverable verified
on the real surface, not just by the vitest mutation proof.

Ring delta in the same window: session `9335be64` advanced +1 (9 -> 10). NOT counted as typing -
the new trace region contains no `terminal.surface.input.capture` and no `terminal.surface.input.sent`,
and the run attached two surfaces, so a single frame is reattach / prompt-redraw output. One ring
unit with no input event is not a keystroke.

Baseline at `/tmp/ulw-c4-baseline.json` refreshed after this observation so the pending manual
typing test produces an unambiguous delta.

## What remains unproven
Only the keystroke clause of C4: `input.capture` -> `input.sent` -> ring growth for the typed
session. Every layer beneath the keyboard is verified (DOM routing across session swaps, bounded
attach retry, daemon-restart self-heal, daemon->PTY->ring liveness via the UDS write probe that
moved endSequence 0 -> 40). The keyboard itself cannot be substituted by a test double, and driving
the user's desktop is forbidden by the standing 2026-08-21 constraint.


## One-command verifier: `scripts/verify-terminal-typing.mjs`
C4 requires an evidence artifact (trace excerpt + UDS ring dump). That capture is now a script
instead of a hand-run sequence, so the verdict is reproducible by anyone:

```
bun scripts/verify-terminal-typing.mjs                  # verdict
bun scripts/verify-terminal-typing.mjs --save-baseline  # re-arm after an app restart
```
It performs the UDS handshake v2, `listSessions`, `describeSession` per session, diffs
`endSequence` against the baseline, counts the input/attach/ensure events of the LATEST trace run,
lists the distinct active tab ids seen, and exits 0=PASS / 1=FAIL / 2=PENDING. Trace and baseline
paths are overridable via `FERRYX_TRACE_PATH` / `FERRYX_BASELINE_PATH`.

### The verifier itself is branch-verified
A tool whose PASS path has never executed cannot be trusted to report a pass. All five branches were
exercised with synthetic traces plus a lowered baseline:
| Branch | Exit | Verdict text |
|---|---|---|
| sent + ring advanced | 0 | PASS - input reached the PTY and the ring advanced |
| sent, no ring growth | 1 | FAIL - send_input succeeded but no ring growth: daemon or PTY side |
| input dropped | 1 | FAIL - input dropped 1x: pane had no attached backend session |
| capture only | 1 | FAIL - capture handler ran but nothing was sent |
| no input events (real data) | 2 | PENDING - no keystroke reached the webview yet |
Synthetic fixtures were deleted afterwards.

## Repeated real-surface evidence across app restarts
The dev runner restarted the app several times during this work. Runs `3d3be635` and `1dae4725` each
show the same clean shape:
- `2x worktree.ensure.skipped`, exactly ONE distinct active tab id -> no focus theft
- `0x terminal.surface.attach.error`, both surfaces attached
Note the precise claim: the live runs show attach SUCCEEDING, so they do not exercise the retry path;
retry remains proven by the unit test plus its mutation, not by the real surface.


## BLOCKED BY A CONTRACT CONFLICT: the retry error banner
Attempting the deferred banner fix surfaced a direct contradiction between two contracts.

C1 as written requires "no error banner" during a retried attach. But `NativeTerminalPane.tsx` had
`setError(...)` BEFORE the retry check, so the banner paints on every transient failure:
```ts
reportNativeTerminalIpcFailure("cmd_native_terminal_attach", error);
setError("Failed to attach native terminal");
if (retryCount < maxRetries) { ... schedule retry ... }
```
Moving `setError` into an `else` branch (surface only once retries are exhausted) made the intended
behavior true AND broke a pre-existing, deliberate test:
`"displays an accessible error banner when attach IPC fails and clears it on retry"`
-> `TestingLibraryElementError: Unable to find role="alert"`.

That test encodes the opposite product decision: show the failure immediately, clear it when the
retry succeeds. It is an accessibility contract, not an accident.

DECISION REQUIRED FROM THE USER - three options:
| Option | Behavior | Cost |
|---|---|---|
| keep current | banner appears instantly, clears ~250ms later on retry success | red flash during successful self-heal |
| defer to exhaustion | silent until all 5 retries fail (~7.75s) | hides a real failure for seconds |
| threshold (recommended) | suppress for the first 1-2 fast retries, show if it persists past ~1s | requires updating the existing test's expectation |

Action taken at the time: source reverted to the original ordering and the new test removed. The
file was left green (51/51 in `NativeTerminalPane.test.tsx`). No test was weakened, skipped, or
deleted to force green. The decision was escalated rather than resolved unilaterally.

**RESOLVED 2026-08-28 19:16 — see "Banner contract RESOLVED: `threshold` applied" below.**


## Queued item RETIRED (not deferred): the first-responder guard
The second queued fix was "guard `restore_webview_first_responder` to skip when the current first
responder is already inside the webview hierarchy". On review this is being DROPPED, not deferred:

1. The offscreen AppKit probe already measured that repeated `makeFirstResponder` calls with the SAME
   object are no-ops (`become=1, resign=0` across 61 calls). The guard therefore saves nothing real.
2. Restoring first-responder TO the webview is what makes DOM keystroke capture work at all - that
   function is load-bearing for the very behavior this whole deliverable is about.
3. Cost/benefit is inverted: zero measured benefit, non-trivial risk of breaking typing, and it
   requires a Rust edit that restarts the user's app.

Retiring it explicitly so a future session does not "complete the queue" by applying a change that
could break keystroke delivery. The remaining real defect is the banner contract conflict above,
which needs a product decision.


## Pre-existing failures found outside the C5 gate (NOT mine, NOT the parallel lane)
C5 gates `cargo test --lib`, which compiles neither `src-tauri/tests/*` nor `src/bin/*`. Running
`cargo test --all-targets` surfaced 3 failures in `tests/daemon_persistence_contract.rs`:
- `test_daemon_terminal_persistence_reconnect_replay_and_isolation` (:540)
- `test_daemon_output_sequence_contiguity_and_replay_gap` (:868)
- `test_daemon_gui_process_non_ownership_and_process_tree` (:958)
All three: `connect client: Os { code: 2, kind: NotFound }` - ENOENT on the daemon socket.

ROOT CAUSE (environment collision, not a code defect): `DaemonProcessHarness` uses the SHARED
`get_socket_path()` (`/tmp/rorca-501/daemon.sock`), and both `shutdown()` and `Drop` call
`remove_file(&self.socket_path)`. Nine tests run concurrently against one global socket, so harnesses
delete the socket out from under each other; 6 win the race, 3 lose.

ATTRIBUTION (evidence, not assumption):
- `git diff -- src-tauri/tests/daemon_persistence_contract.rs` -> NONE. Untouched by me.
- Last commit to it: `38f887e` "migrate desktop rendering to native ghostty" (2026-08-26), i.e.
  BEFORE this session. Pre-existing.
Reported, deliberately NOT fixed, per the standing attribution constraint.

LIVE-ENVIRONMENT HAZARD: this suite spawns daemons bound to the same socket the RUNNING desktop app
uses, and unlinks it on teardown. Do not run `--all-targets` on a machine with a live Ferryx app
unless you accept that the app's daemon socket may be removed mid-session.

METHOD NOTE (two invalid comparisons, corrected): a pristine `git worktree` at HEAD could not serve
as the baseline - it failed first on an uninitialized `vendor/ghostty` submodule, then on a missing
`../ui/dist` resource. Both are worktree-provisioning artifacts, NOT code failures. Attribution was
instead settled by `git diff` + `git log` on the test file itself.


## INCIDENT: I destroyed the user's live terminal sessions with a test run
Running `cargo test --all-targets` on the live machine executed
`tests/daemon_persistence_contract.rs`, whose `DaemonProcessHarness`:
- binds the SHARED `get_socket_path()` = `/tmp/rorca-501/daemon.sock` (the running app's socket),
- sends `DaemonRequest::Shutdown` in `shutdown()`,
- `remove_file()`s the socket in `Drop`.

Result: the user's daemon (pid 17824) was shut down and every PTY it owned died. The GUI respawned a
clean daemon (pid 17063, fresh epoch). Verified damage: direct UDS `listSessions` returned
**0 sessions**, where 8 existed before (incl. `dd1ce3f4` at endSequence 79). PTY processes and
scrollback are NOT recoverable. No repo/disk damage.

PROCESS FAILURE (the real lesson): I derived this hazard by reading the harness source and wrote it
into this doc as a warning - AFTER I had already run the command. The danger was knowable purely by
static reading, before execution. Correct order: inspect any test target that can touch shared
system state (sockets, lockfiles, well-known paths, system services) BEFORE running it, then either
skip it or isolate it (`TMPDIR`/socket-path override).

RULES ADOPTED:
- On a machine running the live app, run ONLY `cargo test --lib` (which is also all C5 specifies).
- Never run `--all-targets`/`--tests` here without first grepping the target for `get_socket_path`,
  `remove_file`, `Shutdown`, or other shared-state teardown.
- Widening a gate is itself a destructive action when the new targets touch live system state; treat
  it with the same pre-flight care as `rm`.

SIDE EFFECT ON C4: the baseline at `/tmp/ulw-c4-baseline.json` references 8 now-dead session ids and
is void. It is deliberately NOT re-armed automatically - the user must restore their panes first.


## C4 baseline re-armed after pane restoration (post-incident)
The user reopened panes. Daemon `listSessions` now reports 4 live sessions, all in
`/Users/indo/code/project/maho-workspace`, and the baseline was re-armed against them:

| session | endSequence at arm |
|---|---|
| `558cabbb-5fbb-456c-8bfe-a1f35628f050` | 7 |
| `c0f596c8-d68d-4058-828b-15aac998d3d5` | 7 |
| `0f26c68d-4b29-49e4-940b-2741aeed229d` | 8 |
| `4adea24c-08ba-4e09-b4be-651bf620aae4` | 7 |

endSequence 7-8 is shell-prompt output only. Latest trace run `4534b9e4`: 0x
`terminal.surface.input.capture`, 0x `input.sent`, 0x `input.dropped`, 0x `attach.error`, exactly ONE
distinct active tab id (`tab:1c3570e8...`) -> still no focus theft, still no keystroke. Verifier
verdict PENDING, which is the correct pre-test state.


## METHOD NOTE: a hand-rolled UDS probe produced a false negative
An ad-hoc probe against `/tmp/rorca-501/daemon.sock` returned TIMEOUT and briefly looked like a
daemon regression. It was the probe that was wrong: it wrote a 4-byte big-endian length prefix, while
the daemon speaks **newline-delimited JSON** after a `{type:"handshake",version:2}` frame - exactly
what `scripts/verify-terminal-typing.mjs` does. The verifier reached the same socket in the same
second and enumerated all 4 sessions.

Rule: never diagnose the daemon with a re-implemented client. Use the verifier, whose framing is
known-good and whose branches are exercised.


## Parallel workstream attribution (re-confirmed, untouched)
`git status`/`git diff --stat` on the daemon lane at the time of the re-arm:
```
M src-tauri/src/daemon/client.rs   | 111 +++
M src-tauri/src/daemon/protocol.rs |  13 +++
M src-tauri/src/daemon/server.rs   | 165 +++
3 files changed, 285 insertions(+), 4 deletions(-)
```
The protocol delta is additive: a new `DaemonRequest::SubscribeRemoteEvents` variant (turns a
connection into a one-way desktop-directed remote event stream) plus its response arm. It does not
alter the framing or the `listSessions`/`describeSession` shapes this deliverable depends on, and it
is not mine - not modified, not "fixed".


## C4 RESULT: PASS on the real surface (user's own keystrokes)
Verifier verdict, captured to `/tmp/ulw-c4-verdict.txt`:
```
VERDICT: PASS - input reached the PTY and the ring advanced
```

The full chain, one layer per line, all from run `4534b9e4`:

| layer | observed |
|---|---|
| DOM keydown | `terminal.surface.input.capture` seq 110, `activeElement: "BODY/"`, key `2`, `targetSessionId c0f596c8` |
| focus recovery | next capture seq 113 shows `activeElement: "TEXTAREA/native-terminal-focus-sink"` |
| IPC | `terminal.surface.input.sent` x3, `backendSessionId c0f596c8` |
| daemon ring | `c0f596c8` endSequence **7 -> 12 (+5)**; every other session unchanged |
| PTY echo | ring bytes contain the echoed `2`, `1`, `2` with zsh syntax highlighting |
| tab stability | exactly ONE `activeTabId` in the run: `tab:1c3570e8-9e1d-4382-949c-fbe3f78430dc` |
| failure counters | `input.dropped` 0, `input.error.recovering` 0, `attach.attempt.error` 0 |

**This is the money observation.** The first keystroke arrived while `document.activeElement` was
`BODY` - the exact condition of the original "typing is dead" defect, still reproducing on the real
surface - and the capture-phase fallback converted it into delivery instead of a swallowed key. The
next keystroke already reports the focus sink as `activeElement`, i.e. the fallback both delivered
the key AND repaired focus for everything after it.

HONEST DEVIATION from the C4 script: the user typed `2`, `1`, `2`, not `echo ULWQA<id>`, so the
literal `ULWQA` token is absent from the ring. The substituted identity check is stronger, not
weaker: the ring's echoed bytes match the three traced keys one-for-one and in order, which pins each
individual keystroke to the PTY. A sentinel string would only have proven that *a* string arrived.


## METHOD: reading a live session's ring without disturbing the GUI
`DaemonRequest::Attach` is safe to issue against a session the GUI already holds. `output_hub` hands
out `tokio::sync::broadcast` receivers (`register_session`, `subscribe`), so an extra attach ADDS a
subscriber and cannot displace the desktop's stream. The probe (`/tmp/ulw-ring-read.cjs`) handshakes
v2, sends `attach` with `afterSequence: 0`, decodes the snapshot, greps, and disconnects. It never
writes input.

Do NOT reach for `cargo test --all-targets` for this - that is what destroyed 8 live sessions earlier
today. A read-only attach is the cheap, safe instrument.


## Banner contract RESOLVED: `threshold` applied (2026-08-28 19:16)
The last open item of this deliverable was the product decision between `keep` / `defer` /
`threshold`. The user's instruction to continue selected the recommended option, and `threshold`
is now implemented.

The deferral reason expired first: the fix was held back only because a `NativeTerminalPane.tsx`
edit makes Vite HMR remount panes in the running app, which would have corrupted the *pending* C4
manual-typing evidence. C4 PASSED and was captured before this edit, so the hazard no longer
applies. (Side effect accepted and stated: applying it did HMR-remount the live panes.)

### Behavior
`NativeTerminalPane.tsx` (attach retry catch block):
```ts
const bannerRetryThreshold = 2;
...
reportNativeTerminalIpcFailure("cmd_native_terminal_attach", error);
const willRetry = retryCount < maxRetries;
if (!willRetry || retryCount >= bannerRetryThreshold) {
  setError("Failed to attach native terminal");
}
if (willRetry) { ...schedule backoff retry... }
```
| Failure | Elapsed | Banner |
|---|---|---|
| attempt 1 (`retryCount` 0) | 0ms | silent, fast retry pending |
| attempt 2 (`retryCount` 1) | ~250ms | silent, fast retry pending |
| attempt 3 (`retryCount` 2) | ~750ms | **surfaces** - failure outlived the fast retries |
| retries exhausted (`retryCount` 5) | ~7.75s | surfaces regardless of threshold |

The IPC failure is still reported to `reportNativeTerminalIpcFailure` on **every** attempt, including
the suppressed ones - only the user-facing banner is delayed, never the diagnostic record. The
success path keeps `setError(null)`, so a banner that did surface still clears on eventual attach.

Threshold rationale: 2 is the number of retries whose combined backoff (250ms + 500ms) stays under
the ~1s perception budget. Raising it delays a real failure; lowering it reintroduces the flash.

### The contract conflict, resolved rather than suppressed
The pre-existing test `"displays an accessible error banner when attach IPC fails and clears it on
retry"` encoded the opposite decision (`keep`). It was not deleted, skipped, or weakened - it was
rewritten to encode the new contract and made **strictly stronger**:
`"suppresses the attach error banner across fast retries, surfaces it once the failure persists,
and clears it on retry"`.

It retains every original assertion (banner appears with the right text, raw filesystem path
redacted, `console.error` IPC report fired, banner clears when a later attach succeeds) and adds two
that did not exist before: the banner is absent after failure 1 and after failure 2.

Determinism: the test was converted from real-timer `findByRole`/`waitFor` to `vi.useFakeTimers()`
with explicit `advanceTimersByTimeAsync(250)` / `(500)` steps matching the backoff schedule. Under
real timers the banner would appear at ~750ms against `findByRole`'s 1000ms default - a pass by
timing luck. There are no sleeps and no wall-clock races now.

### Failing-first evidence (mutation proof, RED -> GREEN)
Mutation run in an rsync sandbox (`/tmp/ulw-banner-sbx`, `node_modules` symlinked), never in the
repo - per the standing rule that Vite HMR must not leak a mutation into the running app. The
mutation restores the pre-fix unconditional `setError`:
```ts
const willRetry = retryCount < maxRetries;
setError("Failed to attach native terminal");   // <- mutation
if (willRetry) { ... }
```
| State | Result |
|---|---|
| mutant | `Tests 1 failed | 50 passed (51)` - only the banner test fails |
| repo (fix in place) | `Tests 51 passed (51)` rc=0 |

Repo `NativeTerminalPane.tsx` sha256 `8fbfe3e9...4ca2d2c` verified after the sandbox run; sandbox
removed. The single failing mutant test proves the new assertions actually bind the behavior.

### Gates after the change
| Gate | Result |
|---|---|
| `bunx tsc --noEmit` | rc=0 |
| `bun run --cwd ui test` | rc=0 - 120 files / 1056 tests |
| LSP errors/warnings on both changed files | none |
| `cargo test --lib` | not re-run - **zero Rust files changed**; `find src-tauri -newermt "2026-08-28 19:00"` is empty, so the tree is byte-identical to the 19:00 run that passed 339/339 |

Changed files: `ui/src/components/NativeTerminalPane.tsx`,
`ui/src/components/NativeTerminalPane.test.tsx`. Nothing else.

### Status of the deliverable
C1, C2, C3, C4, C5 are all satisfied, and the one item that C1's "no error banner" clause exposed
but could not settle - the banner contract - is now closed. The only queued item left is the
first-responder guard, which is **retired, not pending** (see "Queued item RETIRED" above); a future
session must not apply it.


## THE ACTUAL "TYPING DOESN'T WORK" DEFECT, FOUND AND FIXED (2026-08-28 19:42)
Everything above this section fixed real problems *downstream* of the keyboard. None of them was
the defect the user kept reporting. This section is the one that was.

### How the earlier diagnosis went wrong (twice, in opposite directions)
The diagnosis table above says "no `terminal.surface.input.*` at all -> keys never reach the
webview -> native first responder". Applying it to the live trace produced a confident native-layer
hypothesis: `restore_first_responder` (new uncommitted code) calls
`makeFirstResponder(window.ns_view())` after EVERY `frame.present()`, and on Tauri `ns_view()` is
the container NSView, not the WKWebView - a textbook key thief.

That hypothesis is DISPROVEN by timestamps. The focus code landed 14:55-14:57 (file mtimes), and
BOTH runs in which typing demonstrably worked started after it:
| run | window | input.capture |
|---|---|---|
| `0f1b25cc` | 15:29 -> 15:45 | 14 |
| `4534b9e4` | 18:52 -> 19:26 | 3 |
| `f14f55ae` | 19:26:07 -> 19:26:09 (2s) | 0 |
| `9b1c26f6` | 19:26:52 -> 19:29:27 | 0 |
The two zero runs are a 2-second run and a run in which nobody typed. `input.capture = 0` is only
evidence of a defect IF a keystroke actually occurred in that runId. Absence of input events in a
run nobody typed in is absence of a TEST, not presence of a BUG. The prior session made the mirror
of this error - retiring the first-responder guard using an OFFSCREEN AppKit probe on a window that
was never key. Both errors share one root: inferring native focus behavior with no live keystroke.

### The real defect (source-verified, reproduced at the DOM seam)
Of the 17 `input.capture` events ever recorded, **16 have `activeElement: "BODY/"`**. The focus sink
textarea almost never holds focus, so the document-level capture fallback
(`handleCaptureKeyDown`) is in practice the ONLY path a keystroke travels.

The focus sink's own `onKeyDown` has TWO send branches:
- (a) `key.length === 1 && !ctrl && !alt && !meta` -> `sendInput({ text })`
- (b) `shouldForwardKey(event)` -> `sendInput({ keyEvent: { key, modifiers, ... } })`

Branch (b) is what carries **Enter, Backspace, Tab, arrows, and every Ctrl/Alt/Meta chord**.
The capture fallback implemented **only branch (a)**.

Consequence, with the sink unfocused (the normal case): you can type letters and they echo, but
Enter never submits, Backspace never corrects, Ctrl+C never interrupts. `sendInput` is not even
called, so **not even `input.dropped` is traced** - which is exactly why the counters looked clean
and why this survived every previous pass. Run `0f1b25cc` is this defect captured in the wild:
`{capture: 14, sent: 0, dropped: 0}` - fourteen keystrokes into the DOM, zero into the PTY.

### Failing-first proof (RED -> GREEN, mutation-verified)
Two new tests in `ui/src/components/NativeTerminalPane.test.tsx`:
- `"forwards Enter through the keydown fallback when activeElement is document.body"`
- `"forwards Ctrl+C through the keydown fallback when activeElement is document.body"`

| stage | result |
|---|---|
| RED (before fix) | `Tests 2 failed \| 51 skipped` - `AssertionError: expected false to be true` |
| GREEN (after fix) | `Tests 53 passed (53)` |
| MUTANT (`false &&` on branch (b), rsync sandbox) | `Tests 2 failed \| 51 passed (53)` |

Exactly the two new tests fail under their own mutation and no others, so the coverage is real
rather than tautological. The sandbox is `/tmp/ulw-fallback-sbx` with `node_modules` symlinked -
never the repo, because Vite HMR leaks repo edits straight into the running app.

### The fix
`ui/src/components/NativeTerminalPane.tsx`, smallest correct change:
1. `ForwardableKeyEvent` + `toForwardableKeyEvent()` normalize the React-synthetic event (textarea)
   and the native event (document capture) to one shape, so there is now **ONE** definition of
   "which keys go to the PTY". Two divergent copies of that decision WAS the bug.
2. `shouldForwardKey` / `physicalKeyForAltChord` accept that shape (type-level only - they already
   read `nativeEvent.isComposing`, so textarea behavior is unchanged).
3. `isClipboardShortcut()` keeps the fallback from stealing Cmd+V / Cmd+C / Ctrl+Shift+C. Plain
   Ctrl+C is deliberately NOT treated as a clipboard shortcut - it must reach the PTY as SIGINT.
4. Branch (b) added to `handleCaptureKeyDown`, with a `return` after branch (a) so they are
   mutually exclusive.

### Gates
| Gate | Result |
|---|---|
| `bunx tsc --noEmit` | rc=0 |
| `bun run --cwd ui test` | rc=0 - 120 files / **1058** tests (was 1056; +2 = the new tests) |
| LSP on changed files | clean |
| `cargo test --lib` | not re-run - **zero Rust files changed**; the fix is UI-only |

Changed files: `ui/src/components/NativeTerminalPane.tsx`, `.test.tsx`. Nothing else.
No AppKit/Rust edit was made: the native hypothesis was disproven before it justified one, and
`restore_webview_first_responder` stays RETIRED as recorded above.

### REAL-SURFACE CONFIRMATION (run `7099ac0d`, 19:59:25)
The fix was verified by an actual keystroke in the running app, not by inference:
```
19:59:25.946  CAPTURE  key='ㅇ'  dp=False  composing=False  active='BODY/'
19:59:26.003  SENT     hasKeyEvent=False  textLength=1  sess=d6840e02
run totals: capture=1 sent=1 dropped=0 error.recovering=0 attach.error=0
```
`activeElement='BODY/'` is precisely the focus state in which input used to be swallowed.

Ring proof for session `d6840e02` (`attach afterSequence=0` -> `attachOk startSeq=1 endSeq=9`,
852 bytes decoded): the ring **contains the typed jamo** `ㅇ` (U+3147), and the tail shows the shell
echoing it back together with a zsh autosuggestion:
```
...\u001b[?2004hㅇ\b\b\u001b[1m\u001b[31mㅇ\u001b[0m... \u001b[38;5;8mㅁㄴㅁㅇㄴㅁㄴㅇㅁㅁㄴㅇㅇㅁㄴ\u001b[39m
```
An echo plus an autosuggestion can only exist if the byte actually reached the shell process.
Tab-steal check in the same run: `worktree.ensure.skipped=33`, `worktree.ensure.activated=0`.

Three honest qualifications on this measurement:
1. `scripts/verify-terminal-typing.mjs` printed `VERDICT: FAIL - no ring growth`. That is a
   **measurement gap, not a defect**: the typed session was `base=null` (NEW-SESSION), created after
   the baseline was armed, so the verifier had no before-value to diff. It was not explained away -
   the ring bytes were read directly, which is the stronger observable.
2. The keystroke was Korean `ㅇ` with `composing=False`, so it travelled the **text** branch
   (`hasKeyEvent=False`). This proves the fallback reaches the PTY, but it does **not** exercise
   branch (b) (Enter / keyEvent) on the live surface. Branch (b) remains proven by the
   mutation-proved vitest tests plus `cargo --lib native_terminal::input` (9/9, including
   `ctrl_c_frontend_payload_deserializes_and_encodes_control_character`), not by a live keypress.
3. The first ring read returned 0 bytes because the `AttachOk` payload field was guessed as
   `snapshot`/`data`; it is `history` (`protocol.rs:187`). That was the third false signal produced
   by a hand-rolled daemon client in this deliverable - the "read the protocol, never guess" rule
   earns its keep.
