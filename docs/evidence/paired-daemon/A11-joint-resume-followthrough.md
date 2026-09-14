# A11 W2-R followthrough - PARTIAL / NOT READY

Task st_01a0985b. Only relay-owned followthrough was attempted in
/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8. Parent owns final
stable-source aggregate. This report does not supersede any preserved failure
log or claim a new runtime pass.

## External compilation blocker

The focused command compiled current shared source and failed before running
the fixture, exit101:

```text
error[E0004]: non-exhaustive patterns: &DaemonRequest::MachineSessionDetail { .. } not covered
src/daemon/client.rs:118:11
```

The new variant is at daemon/protocol.rs:131. Both files are outside relay
ownership and actively composed by the owner-routing lane. No edit was made
there. A subsequent scoped read still found no MachineSessionDetail arm in
client.rs. No unchanged Cargo retry was performed. Parent must complete that
composition before this fixture can be executed. This is NOT behavioral RED.

## Candidate edits, not verified runtime behavior

Read current WAVE2-resume-verification.md including W2-R and actual source seams.
The existing relay fixture now:

- Loads the gateway's real private machine identity from the same config/auth
  parent, passes it to production RelayClient, and asserts capabilities and
  session targets agree with that host-qualified identity.
- Wraps the real relay router in fixture-only response middleware. For one
  marked create request it awaits the actual completed201 body, signals a
  subscribed oneshot, and holds all response headers/body. The client request
  task is then aborted and joined before releasing the response. Operation
  lookup must recover that target; repeated create must retain it. This is
  intended lost-response injection, not acknowledged retry mislabeled as loss.
  The candidate has not yet executed successfully.
- Creates a second registered root and PTY, checks shell-assembled PID/PWD
  sentinels and distinct PIDs, and verifies it survives the first explicit close.
- Sends a stale epoch through real WS, then exercises original-target resize,
  subscribed shell trap/interrupt, cursor suffix and gap/reset decoding through
  relay. Suffix/gap uses existing hub publication APIs for deterministic data;
  it is not a saturation workload or change to the hub implementation.
- Retains previous malformed/replayed/cross-machine/path tickets, DELETE bodies,
  real reverse data tunnel and cleanup. No capability was enabled.

An added relay inline test issues a real ticket, sets its expiry to the exact
current boundary and requires WS401 plus consumption. This uses controlled
expiry rather than sleeps. It remains unexecuted behind the same compile blocker.

## Exact commands and receipts

Executed from the worktree above:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test relay_pairing_generation_regression a11_joint_machine_relay_runtime -- --nocapture
```

Raw output: A11-followthrough-attempt1.log, COMMAND_EXIT=101. Private target
src-tauri/target; CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0,
CARGO_PROFILE_TEST_DEBUG=0, CARGO_INCREMENTAL=0, RUSTC_WRAPPER=.
CARGO_HOME=/Users/indo/.cargo and RUSTUP_HOME=/Users/indo/.rustup retained.
HOME/FERRYX_RUNTIME_DIR/FERRYX_DATA_DIR/FERRYX_SESSION_DIR/XDG_CONFIG_HOME/
XDG_DATA_HOME/XDG_CACHE_HOME/TMPDIR were private under mktemp /tmp/a11-follow.*
before library initialization. Supervisor was removed after compile exit;
find /tmp -maxdepth 1 -name a11-follow.* returned no roots. No fixture process,
listener or PTY started in this attempt, so there is no runtime reap to claim.

LSP on both owned Rust files returned `server cancelled the request`; compiler
proof remains blocked externally, not abandoned because of diagnostics.
git diff --ignore-submodules=all --check exited0. Two ambiguous apply_patch
attempts were rejected atomically and replaced with unique context; no partial
source changes from those rejected patches occurred.

Current owned hashes:

| File | SHA-256 |
| --- | --- |
| src-tauri/src/remote/relay_server.rs | cab279075297de1281462a72d746bc1703439a69df7b193c06ea7b51e5ead6b0 |
| src-tauri/tests/relay_pairing_generation_regression.rs | ffe40efd8cd7daed4601136aaa2f6c53b83de58bc15134240b57307500a228df |

Relative to the preceding scoped GREEN, relay_server adds only the inline
expiry fixture; the integration fixture gains the scenarios above. Production
canonical dimension repair is unchanged. relay_client.rs remains unchanged.
Seed hashes and the earlier actual RED/GREEN are retained in
A11-joint-resume-implementation.md. This followthrough does not attribute
inherited allowlists or concurrent producer source to itself.

## Remaining gates

NOT READY: none of the new scenarios has a successful current runtime receipt.
The original noncanonical-dimension RED, initial cleanup timeout and all earlier
logs are preserved. No claim that the initial timed-out original PTY was reaped
has been added. Once owner composition compiles, the candidate requires focused
execution, diagnosis of any fixture failures, and completed socket/PTY/root
cleanup. Parent then runs the stable-source aggregate, not this lane.

No session/server/proxy/PTY/hub, capability assertion, dependency link, lock,
ui/dist or other worktree was edited. No canonical daemon discovery, desktop,
remote write, restart, deployment or commit occurred. Event followthrough remains
A12-owned; OS firewall exclusion, native-client routing, platform/native and
whole-plan acceptance remain external gates. All changes are uncommitted.
