# A23 integration, security, and resource regression

## Decision

**AC08 remains open. AC10 is partially evidenced, not signed off. A23 is not a full release green.** `pairedDaemonProxyV1` remains false; neither contract assertion was changed. No production code was changed.

The required narrow gates passed in this session:

| Command (all Cargo commands use --locked and src-tauri/Cargo.toml) | Result |
| --- | --- |
| cargo test --no-default-features --lib paired_host:: -- --test-threads=1 | 31 passed, 0 failed |
| cargo test --no-default-features --test paired_daemon_native_proxy --test relay_pairing_generation_regression -- --test-threads=1 | 2 resource tests and 8 real relay tests passed |
| cargo check --no-default-features --lib | exit 0 |
| cargo check --no-default-features --bin ferryx-cli --bin ferryx-relay | exit 0 |
| bun run --cwd ui build | exit 0 |
| focused Vitest UI regression, 9 files | 98 passed, 0 failed |

Full commands, output and directly captured exits are in `A23-GREEN.log`. Despite its required filename, that log also preserves failed broader validation attempts. It must not be described as entirely green.

## Added regression coverage and RED

- `src-tauri/tests/paired_daemon_native_proxy.rs`: publish 4096 chunks without consumer progress; observe broadcast lag, exactly 32 retained history bytes, empty sibling-host history for an identical raw session ID, duplicate admission rejection, removal of both hub entries, and closure after draining the finite broadcast queue. Verify the wire codec rejects frames exceeding `MAX_FRAME_BYTES` (1 MiB).
- `src-tauri/src/paired_host/proxy_tests.rs`: a new real HTTP/WS test drops a still-connected proxy without calling detach. The peer explicitly drops its socket and sends a oneshot completion signal. The test awaits that signal, verifies the hub entry disappeared, joins the listener, proves subsequent connections are refused, closes the private credential root and asserts it absent. Existing direct/native actor cases also await socket completion. Oversized input (64 KiB + 1) is rejected before valid input/control messages are observed.
- No sleeps, polling delays or public relay dependencies were added. All Rust tests ran serially.
- Failing-first mutation: changed expected retained history from 32 to 33 before the first execution. `A23-RED.log` records the actual assertion failure (32 versus 33), exit 101, one failed and one passed. Restored the assertion, then both tests passed. This proves the new resource test is non-vacuous; it is not evidence that each A23 fault scenario has independently received a new mutation run.
- Both changed Rust files received language-server diagnostics with no diagnostics reported. The targeted diff whitespace check exited 0.

## Proven boundaries

1. Existing paired-host tests passed together with the added socket-drop case: wrong authenticated machine rejection, absent capability fail-closed behavior, stale-generation cancellation of blocked HTTP, mutation ambiguity without replay, response provenance, host identity separation, native actor controls and output, and fail-closed local daemon contracts.
2. The existing `relay_pairing_generation_regression` target passed all eight tests over real loopback relay sockets, including the composed daemon/reverse-client relay scenario, lost create response, explicit failure containment, machine HTTP forwarding, stale registration after control replacement and independent machine pairing. This was reused rather than duplicating it into a nominal `paired_daemon_relay_e2e.rs` file. It is not renderer integration.
3. In the first broad remote run, the log records passes for single-use gateway and relay tickets/replay rejection, ticket target binding, expired tickets, control generation binding, revocation closing sockets, revocation cancelling pending raw input, machine capability authentication, event snapshot overlap recovery, malformed wire values, bounded filesystem admission, blocked TCP writer overflow cancellation and wire allowance checks. These are individual passing results inside a failing suite, not a full-suite pass.
4. The 98-test UI selection covers mixed-host restoration, identity/grouping/ownership, paired project dialogs, directory picker, saved hosts and remote-project classification. No new prose-pinning tests were added. No claim is made that this is the complete UI suite; the parent-reported three pre-existing push/client failures were not changed.

## Failures and execution limitations, preserved rather than hidden

The first broad `remote::` run returned **275 passed / 8 failed**, exit 101:

- Five `machine_input_cancellation_tests` and `a10_pending_machine_input_is_dropped_on_disconnect_and_revoke` failed at `machine_input_fixture.rs:84`: `path must be shorter than SUN_LEN`. These were caused by this packet's long in-worktree TMPDIR, not an application change. Their cleanup receipts report drained owned PTYs, joined listeners and removed private roots.
- `r12_unreadable_root`: Ready versus PermissionDenied.
- `r1_topology_after_gate_http`: HTTP 201 versus expected 400; the fixture plain folder inherited the enclosing worktree repository. The permission case also registered a nested directory as the enclosing repository, so this run cannot adjudicate either product behavior.

The machine integration batch stopped in its first target (`machine_catalog_persistence`): **1 passed / 3 failed**, exit 101. Its normalized plain-directory case explicitly reports that the fixture path must instead be the enclosing worktree's canonical repository root. The later four targets were not executed by that batch.

An attempted environment correction used relative TMPDIR and a Git discovery ceiling. Cargo runs tests/compiler from `src-tauri`, so the relative temp path did not exist there. The second remote run returned **183 passed / 100 failed**, and the integration command failed compilation with `couldn't create a temp dir ... src-tauri/a23-.../tmp`. This is harness failure, not a regression result. No test was deleted or skipped to hide either attempt. No broad-suite success is claimed.

The first command supervisor hit its tool timeout during `cargo run` compilation, after tests/UI had completed; that command has no captured exit. Subsequent direct executions of the existing debug binaries were safe parser probes: `ferryx-cli --help` printed headless usage and returned 1; `ferryx-relay --help` rejected the unsupported option and returned 2. These prove runnable entry-point execution only, not successful CLI workflows. Real relay serving was exercised by the passing integration target.

## AC08 missing path

The capability remains false because transport/native routing does not demonstrate the complete UDS-to-renderer path, remote terminal creation through an ordinary pane, lifecycle metadata/events, automatic recovery, durable proxy restoration, or remote close. Proxy detach is explicitly not remote session close. Consequently forced-relay browsing/worktree/terminal transport evidence cannot close the requested desktop end-to-end criterion. No fabricated desktop test or empty acceptance file was added. The planned workspaceStore paired-daemon test file and new five-scenario RED campaign remain undelivered; existing targeted UI/relay tests provide only the narrower coverage above.

## AC10 and resource gaps

- Bearer-free URLs are supported by exact query checks and passing permanent-token-query rejection tests. Captured terminal input/control messages are the expected bytes/JSON, without credentials.
- **A blanket claim that bearer tokens never occur in any wire payload is false:** `machine_http_transport` intentionally verifies an Authorization bearer header inside the relay's forwarded HTTP tunnel bytes. Credentials belong in authenticated transport headers, not terminal/application payloads or URLs. This packet does not redesign tunnel authentication.
- Exhaustive production-log non-disclosure, cross-host ticket composition through the native client, and queued native mutation revocation across the full renderer path are not proven here. Gateway target-binding and generation tests are narrower evidence.
- Per-proxy replay and broadcast buffers are bounded in the tested configuration; client WebSocket frame/message limits both use `MAX_FRAME_BYTES`; native actor command channel capacity is 32. These facts are not a global memory-bound proof. `paired_runtime.rs` retains owner map entries for finished actors until explicit removal/replacement; arbitrary-size Write vectors enter command construction before Proxy's size check. Global owner admission and pending caller allocation stability therefore remain unproven.
- The connected-drop test proves its owned socket/hub cleanup, not process-wide FD stability across repeated reconnect/revoke cycles. No FD-baseline soak or global task-count proof was completed. No unqualified no-leaks claim is made.

## Isolation and cleanup

Implementation and evidence writes were confined to `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`. The plan was read at the explicitly supplied canonical plan path because it is not present in the worktree. No commits, reset/restore/clean/stash, release builds, deployment, desktop automation or canonical process control were performed. The broad fixtures were given private HOME/data/runtime paths. No existing PTY was intentionally addressed; passing relay fixtures created and cleaned their own synthetic sessions.

The packet-owned root `a23-uxhjso6h` was explicitly removed and its absence asserted. `A23-owned-open-files.log` is the empty lsof result (exit 1, no open entries) before reuse/removal. New socket tests assert peer socket completion, refused listener connection and private-root absence. Existing relay fixtures join their runtime/connection tasks and remove their stores, including the injected panic path. `ui/dist`, `ui/node_modules`, and `src-tauri/target` were preserved. Git status is obstructed by the existing symbolic-link submodule setup; do not interpret that tooling failure as a clean-worktree claim.

## Required human desktop/Linux QA before signoff

On macOS plus an isolated Linux owner, once the missing capability path is implemented: force relay with direct routing unavailable; browse/register a plain folder and Git root; create/delete worktrees; open ordinary native tabs and splits; verify Linux PID/CWD and agent provenance; exercise input/output/resize/interrupt/remote close and native menus/search/copy/paste; lose relay after spawn before reply; lag event consumption during snapshot; restart renderer/local daemon/remote daemon without replacing existing shells; revoke during queued mutation; use identical raw IDs on two hosts; inspect logs/URLs for credentials; measure FD/task/RSS stability under repeated reconnects and slow consumers. Repeat Local/SSH/legacy mobile coexistence, persistence/handover and compatibility rollback without touching unrelated sessions. None of these desktop observations was fabricated here.
