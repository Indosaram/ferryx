# A10 resume implementation - PARTIAL / NOT READY

Task st_01a0983c. Worktree `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.
The machine socket branch and A09 joint controller-close contract have focused
GREEN runtime evidence. The complete assigned A10 contract is NOT implemented
or downstream-ready. `terminalStreamV1` remains unadvertised. This is not
whole-plan, platform, native, relay, or aggregate approval.

## Blocking prerequisite and remaining gaps

Production `terminal/session.rs:162-173` acquires its writer mutex and performs
synchronous `write_all` and `flush`. The router's local input path ultimately
calls this function. A child that stops reading can hold the Tokio executor in
that call; dropping a socket future cannot interrupt a synchronous syscall.
Moving it to spawn_blocking without cancellable PTY I/O would instead allow
input after disconnect. This is a source-backed blocking prerequisite, NOT a
reproduced saturation latency measurement. The fix requires the terminal I/O
layer, outside this packet's explicitly assigned write ownership. It needs a
bounded, cancellable real PTY write surface, with original-process saturation
and cleanup evidence. No unsafe detached input worker was introduced.

Other incomplete contracts requiring followthrough before enabling capability:

- Machine attachment currently validates local owner metadata. A handed-over
  machine session lacks this in-memory metadata on the new owner; existing
  LegacyPeer describe DTO also has no owner epoch. Routed legacy machine owner
  support is NOT implemented, rather than silently using the gateway epoch.
- Output has bounded framing, a <=1 MiB WebSocket write buffer and a ten-second
  send deadline, but the shared broadcast receiver is count-bounded, not a
  proven <=1 MiB per-consumer queued-byte budget. Slow-consumer saturation and
  controlled-time progress-deadline runtime proofs remain unexecuted.
- Read timeout and pending async-backend cancellation are implemented; saturated
  production synchronous PTY input cancellation is not proven.
- Metadata checks run per input/output; immediate idle ownership invalidation,
  close/exit races, and full epoch-expiry/handover runtime need expanded proof.
- No terminal capability was added, so the A06 exact capability assertion was
  correctly left unchanged. Aggregate validation must exercise any eventual
  capability addition and the completed contract.

## Owned source changes

`A10-resume-source-delta.json` records exact SHA-256 versus the inherited seed.
Seed differences in shared files also include completed A09, not solely A10.
No inherited edits were reverted. Own edits are exactly:

- `src-tauri/src/daemon/session_service.rs`: machine target stored in existing
  authoritative spawn metadata; nonblocking metadata/catalog checks avoid the
  journal/fsync mutex on the socket path. Controller generations, cancellation
  watch and generation-local disconnect reservation share the sole close fence.
  An old lease drop cannot release its replacement. Existing creator-only close
  remains the fallback when there is no current/reserved controller.
- `src-tauri/src/remote/server.rs`: grant-selected machine branch; exact raw
  target, canonical epoch/cursor; no grid/query resize; attached JSON even empty;
  integrated existing codec; replay/gap/reset; bounded controls and IO deadlines;
  generation-checked resize/signal/input; independent socket reader so disconnect
  drops pending async input. Legacy stream branch and View resize guard retained.
- `src-tauri/src/remote/protocol.rs`: strict typed machine resize/signal/ping.
- `src-tauri/src/remote/mod.rs`: expose the inherited verified codec.
- `src-tauri/src/remote/security_socket_tests.rs`: exact reservation boundaries,
  real socket/controller HTTP close transfer, pending async input cancellation.
- `src-tauri/tests/machine_terminal_stream.rs` (new): real HTTP-created PTYs and
  two simultaneous WebSockets; decoded native bytes; focus churn and security.

Codec source and pinned Cargo.lock are unchanged. No state.rs/session_api.rs,
filesystem, Q1/Q2/Q3, relay, UI, dependency link, other worktree, or historical
wave1 source was edited. No commit, deployment, desktop launch, canonical daemon
connection, PTY discovery, app restart, or remote-host write occurred.

## Exact commands and results

All commands ran in the above worktree with private `src-tauri/target`,
`CARGO_BUILD_JOBS=4 RUSTC_WRAPPER= CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`. Each command supervisor had a
fresh `/tmp/a10-*` HOME, FERRYX_RUNTIME_DIR, FERRYX_DATA_DIR,
FERRYX_SESSION_DIR, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME and TMPDIR
before library initialization. CARGO_HOME/RUSTUP_HOME retained `/Users/indo`
toolchain homes. No QA root was beneath any Git checkout.

Final exact commands, output and exits are retained in
`A10-resume-final-validation.log`:

| Command | Exit / result |
| --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_terminal_stream --test machine_sessions -- --nocapture` | 0; 1 + 1 real runtime scenarios |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::tests::security::sockets -- --nocapture` | 0; 15 tests |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::tests::security::resize -- --nocapture` | 0; 2 real raw/grid View/Control PTY tests |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_terminal_wire_codec -- --nocapture` | 0; 5 codec tests |
| `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay` | 0 |
| `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay` | 0 |
| `git diff --ignore-submodules=all --check` | 0 |

Final LSP wave returned no diagnostics for all six owned source/test files.
An earlier server.rs diagnostics request timed out at 3000ms; this did not stop
compiler/runtime verification. Compiler retained 18 library and 16 library-test
warnings; no warning was suppressed.

## RED and every validation failure

- `A10-resume-test-build-failure.log`, exit101: new fixture used reqwest `.json`
  without that provisioned feature; switched to existing body API. NOT RED.
- `A10-resume-fixture-fields-failure.log`, exit101: omitted required explicit
  null request fields, HTTP400 instead of201. Corrected fixture; NOT intended RED.
- **Actual pre-production RED** `A10-resume-RED.log`, exit101: successfully
  created two PTYs, machine WS returned403 MACHINE_ACCESS_REQUIRED rather than
  attaching independently of mirror focus. Both originals reaped and listener,
  root and supervisor removed.
- `A10-resume-GREEN-attempt1.log`, exit101: A09 machine_sessions passed; new
  machine socket failed409 SESSION_OWNERSHIP_CHANGED. Cause: my check used the
  mirror-only registry. Corrected to the authoritative machine catalog, then
  removed durable journal reads from the hot path using stored spawn metadata.
- `A10-resume-GREEN-attempt2.log`, exit0: expanded real-PTY scenario passed.
- `A10-resume-security-attempt1.log`, exit0: 13 socket regressions passed.
- `A10-resume-joint-attempt1.log`, exit0: 2 controller/joint runtime tests passed.
- `A10-resume-GREEN-attempt3.log`, exit101: a one-slot try_send input handoff
  rejected valid adjacent resize+ping. Replaced the try_send with bounded async
  handoff; no unchanged rerun. PTYs/listener/root cleaned on failure.
- `A10-resume-joint-attempt2.log`, exit0: 3 tests including subscribed pending
  machine input disconnect/revocation cancellation passed.
- Final batch above passed on the resulting source.

Intermediate focused invocations used the exact Cargo flags above, selecting
`--test machine_terminal_stream`, or `--lib remote::tests::security::sockets`,
or `--lib remote::tests::security::sockets::a10_`, followed by `-- --nocapture`.
Tool-only failures (missing optional remote AGENTS, no-match rg, ambiguous
apply_patch context) are distinguished in `A10-resume-blocker.md`. Additional
ambiguous patch attempts were rejected atomically and rewritten with unique
context. No unchanged failed command was retried until it happened to pass.

## Actual final runtime proof and cleanup

Epoch **1789262305418**:

- Session `d4cdf6ae-5954-4be2-bb4c-c2c7807c4f75`, PID **12923**, CWD
  `/private/tmp/a10-final.VVJRNL/tmp/.tmpXE9vO6/one`.
- Session `a9581b20-eb4c-4588-9a0f-b3ad2fda84e7`, PID **12982**, CWD
  `/private/tmp/a10-final.VVJRNL/tmp/.tmpXE9vO6/two`.

Shell-assembled A10_PROOF markers came through each real socket after each of
three mirror selection changes. Echoed commands cannot satisfy the sentinel.
PTY PID and CWD matched creation records. Stale replaced-socket input did not
set the shell variable; stale resize failed, fresh resize reached101x31.
Cross-device socket and HTTP close returned409; mirror/View sockets returned403.
Fresh-ticket replay returned401. Wrong/noncanonical/duplicate epochs and cursor,
foreign-scoped ID, grid, oversized binary/control and revoked input were denied.
Suffix/gap/empty replay used the actual shared output hub and socket codec;
the gap fixture injects the existing hub gap API, not a measured memory-overflow
workload. Revocation left the original PTY alive and its variable unset.

Joint runtime epoch **1789262308618**, original PID **13583**: actual socket
disconnect recorded reservation; another device received409; controlled15s
expiry admitted that device; creator HTTP close returned409; current controller
HTTP close returned204 and the original process was reaped. Separate exact
14,999ms/15s predicate assertions use controlled time, no sleeps.

Pending-input fixture uses actual auth, HTTP-created PTY authority and socket,
but replaces the backend IO completion with a subscribed gate. Disconnect and
revocation drop that pending future with completed_inputs=0. This does not prove
cancellation of the synchronous production writer described above.

Final fixture logs confirm original PTY reap, pending work zero, lifecycle waits
for unit fixtures, listener stop/join, and private-root removal. The integration
fixture checks listener refusal after joining. Supervisor
`/tmp/a10-final.VVJRNL` was removed. No original PID was recreated or reassigned.

These changes remain uncommitted. Parent aggregate review must retain NOT READY
until the explicit I/O/handover/output-budget gaps are resolved; no downstream
worker should interpret these focused passes as complete A10 acceptance.
