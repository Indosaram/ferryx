# A11 joint resume - focused relay GREEN, full packet NOT READY

## Current continuation result (supersedes initial preflight below)

The parent explicitly authorized independent relay verification despite the
incomplete A10 handoff. A concrete relay repair and real wire fixture now pass.
This is not complete A10, full A11, Wave2, capability or platform acceptance.

### Exact owned changes versus inherited seed

- `src-tauri/src/remote/relay_server.rs`: add canonical u16 deserialization for
  optional cols/rows, rejecting leading zero/sign forms before WS upgrade instead
  of normalizing them. No route widening, ticket fallback or capability change.
  Current SHA-256: fa3c6e39318f13ec6a2f5fe97dc96bf9cc171e13b33d9d90360c3d350aae4816.
  Seed SHA-256: 89157d33216802f1f63dc85a4ba34807e58731ef697ef852578fa5517d2223f3.
- `src-tauri/tests/relay_pairing_generation_regression.rs`: add
  a11_joint_machine_relay_runtime plus its real HTTP/PTY helper. Preserve every
  inherited regression. Current SHA-256:
  751b73e3b6257be8b9a705e5fdec4e2457f377496ef1d0299cdb138cf0e1df95.
  Seed SHA-256: ea07bf77ac64e915e1001dcc701bb3789609f8257614e7188ce8e99161de12f0.
- This report and A11-joint-resume-prefixed raw logs. relay_client.rs is unchanged
  at 386abd5a2c71c0889912d6790b74bbf77352e2b75fab7d9bf0ac3f48a8786c0e.

### Commands and every failure

All Cargo commands used CARGO_HOME=/Users/indo/.cargo,
RUSTUP_HOME=/Users/indo/.rustup, CARGO_TARGET_DIR=$PWD/src-tauri/target,
CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0,
CARGO_INCREMENTAL=0, RUSTC_WRAPPER=. HOME, FERRYX_RUNTIME_DIR, FERRYX_DATA_DIR,
FERRYX_SESSION_DIR, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME and TMPDIR
were private directories under fresh /tmp/a11-* supervisors before library init.

1. `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test relay_pairing_generation_regression a11_joint_machine_relay_runtime -- --nocapture`:
   A11-joint-resume-RED.log preserves an initial fixture error (detail is a
   tagged session envelope, not a bare target). NOT production RED. The cleanup
   joined the gateway runtime before closing its PTY; the command timed out at
   200 seconds, with no captured Cargo exit. The harness killed the command;
   no matching fixture process remained in the subsequent pgrep. Reordered
   cleanup to close/reap PTYs before joining the gateway and fixed the envelope
   assertion. This first failed run has no original-PTY reap receipt and is
   explicitly NOT successful failure-containment evidence.
2. Same command on corrected fixture: A11-joint-resume-RED2.log, exit101.
   Actual live relay accepted noncanonical cols=01 instead of denying before
   upgrade. Production code had not yet changed. Original PTY reaped and relay
   runtime/connections/root joined/removed on failure. This is behavioral RED.
3. Applied the minimal canonical numeric decoder. Full command
   `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test relay_pairing_generation_regression -- --nocapture`:
   A11-joint-resume-GREEN-attempt1.log, exit0, seven passed. Then expanded
   malformed/cross-machine/path/cursor cases, rather than retrying unchanged.
4. Same full command on expanded fixture: A11-joint-resume-final.log,
   TEST_EXIT=0, seven passed, none skipped.
5. `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`:
   same final log, CHECK_EXIT=0.
6. `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`:
   same final log, BUILD_EXIT=0. Existing 18 compiler warnings retained.
7. LSP diagnostics on both changed Rust files: no diagnostics. No errors or
   warnings suppressed. `git diff --ignore-submodules=all --check`: exit0.

### Actual final relay surface and cleanup

Final original PID **35980**, owner epoch **1789263208193**, raw session
**cc3ed51f-4995-4697-a423-0d469afa1709**, gateway machine identity
**8c41a333-827d-4cb7-9b4a-70c50620e700**. CWD:
`/private/tmp/a11-final.gxj8kv/tmp/.tmpSWWutZ/project %2F space`.

The fixture pairs a machine grant through the production coordinator and
RelayClient. HTTP create/detail/repeated create, capability lookup and encoded
directory lookup use only the host-qualified relay origin. The original shell
assembles A11_PROOF with $$ and $PWD; decoded real WS output matched the actual
PTY PID and registered path. The command's echo cannot satisfy the sentinel.
Cursor33 was taken from actual codec output. A fresh-ticket attachment sent that
cursor and original owner epoch, received the same target with no gap, and
completed ping/pong. Replayed tickets, foreign machine/path tickets and malformed
epoch/cursor/dimension/token queries were denied before upgrade. DELETE carried
its real requestId/daemonEpoch body and returned204 with original PTY reaped.

Existing suite checks retain explicit route/method admission, 64KiB+1 rejection,
literal percent/Unicode directory behavior, controlled HTTP wire/body forwarding,
pairing-generation rejection and injected-failure runtime cleanup. Those
synthetic transport cases are not relabeled real session-service behavior.

Final listeners/reverse tasks/runtime connections joined, original PTY reaped,
private root removed; final supervisor /tmp/a11-final.gxj8kv removed. The RED2
failure also completed cleanup. Initial timed-out supervisor cleanup is recorded
separately below. No canonical daemon discovery, dependency edits, remote writes,
desktop action, commit or deployment occurred.

### Limits and explicit cross-scope handoff

The client fixture has no direct HTTP/WS route; only the production reverse
client receives the gateway address. This proves test-client relay routing,
NOT OS firewall exclusion or future A14 native-client routing. The configured
relay identity is synthetic a11-machine while the actual gateway identity above
is independently generated; this is not native paired identity negotiation.

Create replay is a repeated acknowledged request, NOT an injected lost TCP reply
after durable commit. Fresh cursor reattachment is proved; deterministic output
suffix/gap, ticket expiry, stale gateway epoch denial, saturation/progress budgets
and lost-reply relay reconciliation still need expanded runtime proof. Initial
timeout lacks full original-PID reap evidence. Therefore full assigned packet
remains NOT READY, despite the seven focused passing tests. A10's advertised
capability, cancellable saturated PTY input, legacy-owner handover and byte-budget
gaps remain parent/A10/A12-owned. No capability was enabled and no cross-scope
source was edited. Aggregate must retain these limits and cannot claim Wave2
PASS from this result.

## Initial preflight record (historical, not current source state)

Task `st_01a0985b`, worktree
`/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.

This continuation is blocked on the actual A10 prerequisite, not a cached DAG
status. No new A11 production or test changes were made, and no joint runtime
acceptance is claimed. The inherited A11 allowlists remain intact.

## External blocker reported to parent

The current `A10-resume-implementation.md` explicitly reports PARTIAL / NOT
READY and keeps `terminalStreamV1` unadvertised. Source inspection confirms
`remote/server.rs::get_capabilities` advertises directoryBrowseV1 and, when
ready, terminalCreateV1, but not terminalStreamV1. A capability-negotiated
machine-stream client cannot operationally exercise the requested complete
contract yet. Bypassing negotiation would establish only partial transport
compatibility, not complete A11 against the approved A10 contract.

A10 names these unresolved prerequisites:

- Cancellable production PTY writes under saturation. The actual machine socket
  calls `session_backend.write_input`; `terminal/session.rs::write_input`
  takes the writer mutex and synchronously calls write_all/flush. The A10
  pending-async-backend test does not prove cancellation of this synchronous
  writer. This is source-backed risk, not a measured saturation failure here.
- Original owner epoch and authoritative machine metadata across handover.
- A proven <=1 MiB per-consumer queued-byte budget and real slow-consumer /
  controlled deadline evidence.

These repairs require files outside this task's relay-only ownership. A12 owns
gateway/services concurrently; no gateway, terminal, capability, filesystem
assertion, codec or Q1/Q2/Q3 source was edited. Parent must resolve the actual
A10 prerequisite before treating A11 as downstream-ready. The capability
expectation remains directoryBrowseV1 plus terminalCreateV1; no change to the
A10/A12-owned filesystem assertion is requested at this stage.

## Source provenance

Compared with the inherited resume seed, not git HEAD:

| Owned source | Current SHA-256 | This task's delta |
| --- | --- | --- |
| src-tauri/src/remote/relay_server.rs | 89157d33216802f1f63dc85a4ba34807e58731ef697ef852578fa5517d2223f3 | none; matches seed |
| src-tauri/src/remote/relay_client.rs | 386abd5a2c71c0889912d6790b74bbf77352e2b75fab7d9bf0ac3f48a8786c0e | none |
| src-tauri/tests/relay_pairing_generation_regression.rs | ea07bf77ac64e915e1001dcc701bb3789609f8257614e7188ce8e99161de12f0 | none; matches seed |

The only authored file is this report. The baseline relay server/test dirty
diffs were inspected and preserved, including precise HTTP routes, DELETE
bodies, epoch/cursor forwarding and existing real reverse-channel fixtures.
The reverse client's actual handle_session still opens the data WebSocket and
configured gateway TCP connection; no replacement protocol was introduced.

## Commands, results and evidence limits

Read tools consumed the full approved plan, root and src-tauri AGENTS, Wave2
handoff, resume instructions, acceptance ledger, Q review and current A09/A10
reports. All code findings above derive from reads, not executed behavior.

- `git status --short --ignore-submodules=all` and
  `git diff --stat --ignore-submodules=all`: exit 0; inherited composition dirty.
- `git diff --ignore-submodules=all -- src-tauri/src/remote/relay_server.rs`:
  exit 0; inherited changes inspected, not attributed to this task.
- `shasum -a 256 src-tauri/src/remote/relay_{server,client}.rs
  src-tauri/tests/relay_pairing_generation_regression.rs`: exit 0; hashes above.
- Python hashlib comparison against RESUME-01a097f8-seed.json: exit 0; relay
  server and regression fixture match seed; relay_client is absent from that
  copied-file manifest and was not changed by this task.
- `ls docs/evidence/paired-daemon/A12-resume-implementation.md`: exit 1,
  No such file or directory at inspection. A12 is concurrent, so this is not
  an A12 implementation verdict. No unchanged retry was made.
- `command -v apply_patch`: exit 0; report added with apply_patch only.

No cargo test/check/build or LSP was run: there was no code edit, and the
upstream report identifies an external implementation blocker rather than a
local compilation issue. There is no new behavioral RED or GREEN, no new
original PID/CWD/epoch proof, no actual forced-relay HTTP/WS result, and no
client-routing or OS-firewall-exclusion proof in this continuation. Historical
and upstream passing logs are not relabeled as this task's runtime evidence.

## Cleanup and remaining gates

No fixture process, socket, worker, PTY, child, private HOME or temporary root
was created. Consequently this task has no runtime teardown obligations or
cleanup receipt to fabricate. Canonical daemons/PTYS, other worktrees,
read-only dependency links, private ui/dist and pinned Cargo.lock were untouched.
No commit, deployment, desktop launch, remote-host write or restart occurred.

After the external prerequisite is repaired, the assigned packet still needs
actual relay-only create/detail/close, lost-reply reconciliation with the same
PID/PWD/owner epoch, real stream replay, fresh single-use path/machine tickets,
malformed/stale/cross-machine denial, bounded transfer and injected-failure
cleanup. Client relay routing and externally enforced direct-path exclusion
must remain distinct. Full A11, A01-A24, platform/native and release acceptance
remain open. This uncommitted report is NOT downstream readiness.
