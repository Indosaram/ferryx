# Wave2 resumed aggregate verification - PARTIAL / NOT READY

Task st_01a09868. This is a read-only aggregate gap verdict, not downstream
readiness, completed Wave2 verification, platform approval or whole-plan acceptance.
Implementation is incomplete. Parent steering explicitly pauses further aggregate
execution until composition handoff: routed ownership st_01a09869, portable input
st_01a0983c and byte-bounded output subscription st_01a0986b are active.
No source fixes, commits, deployments, desktop launches, canonical daemon
discovery/connections or remote-host writes were performed by this verifier.

## Executed evidence and provenance

Read the complete 833-line approved plan, applicable root/backend/daemon AGENTS,
Wave1 handoff boundaries, RESUME, complete acceptance ledger, Q-batch review,
all four current implementation reports and the input repair report. Inspected
actual session HTTP/journal authority, controller/stream handlers, event module
and fixture, routing backend, session CRUD/stream/relay fixtures and authority/
crash fixtures. This is not a claim that every changed source was fully reviewed;
the final exhaustive composed review remains pending.

`WAVE2-resume-source-before.json` and `-source-after.json` contain SHA-256 for
backend sources/tests/examples, UI source, Cargo.toml and Cargo.lock.
`WAVE2-resume-source-delta.json` compares the actual parent composition in
WAVE2-parent-seed.json, not HEAD; it retains seed and current hashes individually.
No recorded source hash changed during the two completed commands. This is a
bounded observation, not an immutable final composition: active writers are now
explicitly reported by the parent. The manifests must not approve later bytes.

Exact commands, exits and elapsed times are in WAVE2-resume-results.json; full
untruncated stdout/stderr are in WAVE2-resume-integration.log and
WAVE2-resume-remote.log. Commands used four Cargo jobs sequentially, locked
dependencies, private src-tauri/target, DEBUG=0 for dev/test, incremental=0 and
empty RUSTC_WRAPPER. Normal Cargo/Rustup homes were explicit. Before library
initialization HOME, runtime/data/session, XDG and TMPDIR were private beneath
`/tmp/wave2-aggregate-obciu2q0`, outside Git. A Cargo target runner changed test
execution CWD there; a private copy of ui/dist prevented inherited static tests
from writing the checkout's dist. No dependency link or ui/dist was modified.

| Executed command (shared prefix below) | Exit | Result |
| --- | --- | --- |
| `test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence --test machine_worktrees --test machine_sessions --test machine_terminal_stream --test machine_events -- --nocapture --test-threads=1` | 0 | Catalog 4, worktrees 2, sessions 1, stream 1, events 1 passed; none ignored |
| `test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture --test-threads=1` | 101 | Compilation completed; test executable aborted before running tests |
| `git --no-pager diff --check --ignore-submodules=all` | 0 | Whitespace validation only |

Shared Cargo prefix, preserved literally in both logs:

```sh
cargo --config 'target.aarch64-apple-darwin.runner=["sh", "/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8/docs/evidence/paired-daemon/WAVE2-resume-runner.sh"]'
```

Remote failure is **not behavioral RED** and not a passing suite:

```text
dyld[49911]: Library not loaded: @rpath/libghostty-vt.dylib
Reason: no LC_RPATH's found
process ... signal: 6, SIGABRT
EXIT=101
```

Diagnosis: runtime dynamic-library resolution under the isolated runner failed;
the library test executable never entered its assertions. Resolve the private
loader path at the runner boundary without dependency writes before a future
authorized execution. No unchanged retry occurred. Authority, Local/SSH/relay
regressions and headless lib/CLI/relay build were not executed after this failure.
Parent's subsequent pause prevents repair execution/rerun now.

LSP before Cargo: remote directory scanned 37 Rust files, zero errors, one
dead-code warning (`server.rs::no_auth_query`) plus inactive-cfg hints;
session_service.rs had no diagnostics. This is not all-file clean diagnostics.
Compiler warnings remain in logs, unsuppressed.

## Actual runtime surface proved in this continuation

- A09 HTTP epoch **1789263610623**: original shell PID **41338** in private
  `.../.tmpQksUNV/project`; same request returned same target and shell PID/CWD.
  Second-root PID **41545** in `.../.tmpQksUNV/second` survived first close.
  Managed-worktree PID **41771**, relative-CWD PID **41880**, inherited-CWD PID
  **41900** were observed through shell-assembled output sentinels. Explicit
  close checked reap, stale epoch and controller conflict; invalid startup,
  path escape, mirror access and changed-digest requests were refused.
- A10 real concurrent HTTP-created PTYs plus WS: epoch **1789263637878**,
  original PIDs **42105** and **42176**, respective private roots
  `.../.tmphwdw5n/one` and `/two`. Both produced PID/CWD sentinels after three
  mirror focus changes. Controller replacement, stale input/resize, other-device
  close conflict, fresh ticket reuse denial, input/control size refusal and
  revocation assertions passed. Replay suffix/gap uses direct hub publication;
  it is not a measured slow-consumer overflow. This does not prove legacy owners.
- A12 real authorized event socket received initial boundary and the actual
  subsequent HTTP201 projectRegistered identity with a newer sequence. It did
  not exercise mutation during snapshot, lag, worktree/session lifecycle,
  metadata validation or mirror event redaction.
- Catalog private owner PIDs **40733**, **40769**, **40805** were waited/reaped;
  the last is intentional injected-failure SIGKILL, not a production failure.
  Catalog crash-output broken-pipe and injected IPC assertion messages are
  retained in the passing containment test output, not hidden.

The integration log records PTY reap, listeners joined/refused and fixture roots
removed. WAVE2-resume-cleanup.log records supervisor root removal. The remote
loader failure occurred before fixture startup. No pending command or kernel
wait is held by this task. No new lost-reply/crash/capacity-64 or combined relay
runtime was executed; producer reports remain attributed producer evidence only.

## Disjoint actionable gap ledger

These findings describe the inspected snapshot. Active owners must compose
their changes before re-adjudication; do not assign overlapping repair edits.

| ID / owner boundary | Concrete finding and owed acceptance |
| --- | --- |
| W2-I / st_01a0983c plus parent socket integrator | `server.rs:1366` still calls backend.write_input; backend delegates to SessionRouter and the synchronous local writer. The new cancellable PtySession seam is not integrated. Socket generation/revocation cancellation cannot interrupt that synchronous call. Wire the portable seam through the single authority after producer handoff, then prove real saturated original-PTY cancellation, no late input, deadlines and teardown. The existing pending-backend mock and seam-only Unix proof are insufficient. |
| W2-O / st_01a09869 | `session_service.rs:219-237` requires in-memory metadata and local PTY; detail at 316 onward compares against gateway epoch. Machine stream handler also requires machine_pty. Surviving legacy-owner targets cannot traverse the existing router as machine sessions. Compose original-owner metadata/epoch routing, prove actual handover PID/CWD retention, stale/expired distinctions, stream and guarded close. No alternate spawn or Local/SSH fallback is acceptable. |
| W2-B / st_01a0986b then parent stream integrator | Stream consumes count-bounded broadcast receiver; WebSocket write-buffer limit is not queued-output byte accounting. Compose additive byte-bounded subscription, then integrate with replay/gap and prove >1MiB slow-consumer saturation, independent sibling progress and controlled send deadline. No byte-budget PASS follows from current framing tests. |
| W2-E / A12 event implementation lane | machine_events.rs emits internal catalog as projects, not rich Projects/completeness envelope; revision is copied from event sequence. Worktree payloads are invalidations. Local registration, availability transitions, debounced Git watching and validated agent/title/CWD publication are unfinished. Complete projection/order/revisions and feeds without inventing provider identity. Deterministic receiver-before-snapshot race, tiny-buffer lag, mirror isolation and reconnect-no-spawn runtime proofs remain owed. Coordinate metadata feeds with ownership lane; do not edit its files concurrently. |
| W2-J / disjoint session HTTP admission lane | session_api.rs:105,131,166-169 synchronously reads the journal from async tasks. Journal reads share parking_lot state mutex with persist/fsync (journal.rs:106-125). close_machine also reads it while holding close/controller gates; capability readiness similarly reads it. Source concern confirmed, runtime starvation NOT reproduced here. Use a subscribed real write-lock barrier plus external bounded watchdog to adjudicate progress, deadline/revocation and retained permits; offload readers if reproduced. Do not weaken durability or introduce a second journal. |
| W2-R / relay fixture lane after composition | Existing A11 fixture uses synthetic relay identity a11-machine distinct from gateway identity; creates one real session and repeats acknowledged create. It is not two-root combined identity-bound relay acceptance, lost-reply injection or OS direct-path exclusion. Expand actual resulting contract with identity agreement, two roots, dropped reply, original target, suffix/gap, stale epoch, ticket expiry, interrupt/resize/close, events and teardown. |
| W2-V / verifier harness after parent release | Loader failure above blocks full remote runtime. Resolve private loader environment; then run full named suites/build once on stable composition with before/after hashes. No source assertion weakening or skipped failing tests. |

No capability expectation was edited. The inspected advertised set remains
directoryBrowseV1 plus terminalCreateV1. terminalStreamV1/machineEventsV1 must
remain unadvertised until actually operational. A10/A12 own any eventual exact
filesystem_tests.rs capability assertion update, not this verifier.

## Full owed contract and limits

The gate still owes aggregate actual owner/relay HTTP+WS in two roots, original
PID/CWD/owner epoch through retries/lost reply/crash and handover, controller
conflicts/safe explicit close, real input/replay/gap/focus isolation, independent
auth/redaction, deterministic snapshot race/lag, every specified capacity and
deadline and success/failure teardown. A09's source uses the shared handle_spawn
and preallocated-ID seam; inspected machine decoder rejects SSH startup and
client shell/env overrides. This limited review is not a completed all-caller
no-fallback/no-duplicate-spawn audit. Do not conflate incomplete implementation
with only missing tests.

Q1/Q2/Q3 source/proofs and inherited uncommitted baseline were preserved.
Linux/Windows runtime on final composition, native desktop/menu/IME, real provider,
network-excluded forced relay, full logging audit, compatibility and rollback
remain explicit external/later gates. No AC01-AC12 or A01-A24 approval is granted.

Owned changed files are only this report and WAVE2-resume-{runner.sh,validate.py,
source-before.json,source-after.json,source-delta.json,integration.log,remote.log,
results.json,cleanup.log}. All are uncommitted verification artifacts. The runner
is an archived failed-at-loader candidate, not authorization to rerun while
producers are active. Exact source hashes are in the manifests rather than
being attributed to HEAD or to this documentation-only task.
