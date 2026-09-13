# A09 resume implementation and producer evidence

Task: `st_01a097fc`. Worktree:
`/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.

A09 machine session CRUD and durable idempotent spawn are implemented and pass
the producer checks below. This is not A10 socket acceptance, aggregate Wave2
verification, native desktop acceptance, or full-plan/platform approval.
The aggregate report remains parent-owned `WAVE2-resume-verification.md`.

## Provenance and source changes

The baseline is the inherited composition in `RESUME-01a097f8-seed.json`, not
git HEAD. `A09-resume-source-delta.json` records the seed/current SHA-256 for
all 12 A09-owned source/test files and marks the three added files. It also
verifies that the provisioned `src-tauri/Cargo.lock`, terminal codec, and codec
test still match that seed. Inherited preallocated-ID and partial journal code
are not claimed as new work.

- `daemon/session_service.rs`: machine adapter around the existing shared spawn
  path; bounded queued admission/rechecks; exact parent project/worktree/epoch;
  canonical relative CWD; remote typed provider validation/claim; durable spawn
  publication; retained detail/list projection; guarded explicit close and
  lifecycle-writer completion.
- `daemon/server.rs`: initialize close-authority and lifecycle coordination.
- `remote/session_api.rs` (new): authenticated bounded machine HTTP adapters,
  strict request-field admission, device-scoped request digest/replay, typed
  errors, list/detail/create/close.
- `remote/machine_operation_journal.rs`: atomically publish session ownership
  and its create receipt; retain session metadata/revision; recognize machine
  targets from pending intents as well as committed records.
- `remote/server.rs` / `remote/mod.rs`: mount each route once, preserve legacy
  GET and its missing/invalid-auth responses, exclude machine sessions from
  mirror listing and socket admission, expose only operational
  `terminalCreateV1`, retain actual absent-service regression coverage.
- `remote/state.rs`: exclude machine-only selections/tabs/attention from the
  legacy mirror selection snapshot and broadcaster.
- `terminal/service.rs` / `terminal/pty.rs`: use the inherited shared raw-ID
  spawn seam; add an explicit machine close using a five-second graceful phase
  with authorization rechecked before escalation. Local/SSH close behavior
  retains its existing grace policy.
- `tests/machine_sessions.rs`: real owner HTTP/PTY behavior.
- `daemon/session_service_machine_tests.rs` and
  `daemon/session_service_crash_tests.rs` (new): subscribed authority, lost-reply,
  resource-limit, remote-provider, fault-interruption and real process-crash
  fixtures.

No filesystem, worktree-preview, codec, UI, dependency-link, or other worktree
source was edited by A09. Parent restored 13 UI files and replaced ui/dist's
old link with a private build during this task; later Rust builds consumed that
current build input. Its UI verification belongs to the V05 evidence, not A09.
No commits, deployment, desktop launch, OS input manipulation, or canonical
daemon/PTY manipulation occurred. (PTYs below are fixture-owned.)

## Implemented semantics

- Authorization and machine-Control scope precede body/resource/journal work.
  Shared admission retains the 64 KiB body, 64-live-machine-session and eight
  simultaneous machine-mutation limits. Queued spawn rechecks authorization
  after the shared spawn/workspace gates and again before intent/spawn.
- The daemon resolves the registered root or exact managed worktree. Relative
  CWD cannot escape through parent traversal or symlinks. Inheritance requires
  a running parent on the same machine, epoch, project and worktree. HTTP accepts
  neither executable/env/absolute-CWD overrides nor SSH/startup command injection.
- Shell creation uses the existing native resolver. Machine `agentResume`
  currently supports OMO's existing ID-based transcript/CWD resolver plus
  executable availability and provider claims. Providers lacking authoritative
  remote ownership/CWD validation return `AGENT_RESUME_UNSUPPORTED`; argv support
  alone is not treated as resumability. Client transcript paths are rejected.
  No provider ID is generated and no failed resume becomes a fresh agent.
- The raw target ID is allocated before PTY creation. The sole device/request
  journal intent is persisted inside shared spawn admission before the shared
  PTY path executes. Ownership and successful create receipt commit atomically
  before acknowledgement. Same device/input returns the original target/result;
  changed digest or operation kind returns `REQUEST_CONFLICT`.
- An interrupted attempt retains its intended target and becomes unknown rather
  than repeating spawn. Restart preserves completed receipts and expired/exited
  metadata. Cold restart never invents a replacement process.
- DELETE checks the exact epoch and controller fence. In the absence of an A10
  controller, only the durable creator can close; absence is not permission for
  arbitrary takeover. Another controller blocks close before intent. Close waits
  for process termination/reap and lifecycle persistence before success. Unknown
  close outcomes remain journaled; retries do not blindly issue termination again.
- Detach/socket loss has no call to machine DELETE. Machine-only resources are
  excluded from legacy listing, selection snapshots/events, and mirror sockets.
  `terminalStreamV1` and `machineEventsV1` are not advertised.

## Execution environment and exact commands

Commands ran on macOS arm64 in the worktree above, with:

```sh
CARGO_TARGET_DIR=$PWD/src-tauri/target
CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0
CARGO_INCREMENTAL=0
CARGO_BUILD_JOBS=4
RUSTC_WRAPPER=
```

The final batch and the legacy RED/GREEN used fresh `/tmp/a09-*` supervisors:
`HOME=$qa/home`, `FERRYX_RUNTIME_DIR=$qa/runtime`,
`FERRYX_DATA_DIR=$qa/data`, `FERRYX_SESSION_DIR=$qa/sessions`.
Existing `CARGO_HOME` and `RUSTUP_HOME` were retained explicitly while HOME was
isolated. TMPDIR was not placed inside a repository. Every supervisor was removed
after its commands; receipts are in the logs. Earlier session fixtures already
supplied private owner config/auth/root/listener paths explicitly.

All final commands and exits below are in `A09-resume-final-validation.log`
unless a different log is named:

| Exact command | Exit / result |
| --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_sessions -- --nocapture` | 0; 1 real HTTP/runtime scenario |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::session_service::machine_tests -- --nocapture` | 0; 9 tests, including two subprocess fixture entry tests |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::server::tests::r3_http_boundary_contract -- --nocapture` | 0; 1 boundary/legacy test |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::server::tests::a03_absent_machine_service_is_private_and_unavailable -- --nocapture` | 0; 1 actual session API absence test |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::a04_shared_services_tests -- --nocapture` | 0; 4 shared-authority/cleanup tests |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib terminal::pty::tests -- --nocapture` | 0; 5 tests; `A09-resume-PTY.log` |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::remote_ssh_tests -- --nocapture` | 0 after helper provisioning; real loopback SSH; `A09-resume-SSH-provisioned.log` |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test worktree_safety --test remote_project_public_contract -- --nocapture` | 0; 9 + 1 tests; `A09-resume-worktree-local-SSH.log` |
| `cargo build --locked --manifest-path remote-helper/Cargo.toml` | 0; `A09-resume-helper-build.log` |
| `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay` | 0; `A09-resume-headless-build.log` |
| `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay` | 0; `A09-resume-headless-check.log` |
| `git --no-pager -c diff.ignoreSubmodules=all diff --check` | 0 |

The helper compiled in the same private src-tauri target. The resulting binary
was copied to the existing fixture's expected
`remote-helper/target/debug/ferryx-remote-helper`; no test source was weakened.
Both build artifact locations are private to this worktree.

## RED, failures, and recovery (not relabeled as GREEN)

1. Required initial RED: the exact machine_sessions command built successfully,
   then observed 503 `MACHINE_SERVICE_UNAVAILABLE` versus expected 201, exit 101.
   `A09-resume-RED.log` includes teardown. This preceded A09 production edits.
2. `A09-resume-GREEN-attempt1.log`: exit 101, test compile error from a local
   variable shadowing the response helper. Renamed the variables; not behavioral RED.
3. `A09-resume-GREEN-attempt2.log`: exit 101, fixture expected 422 but sent the
   invalid provider-key wire spelling `sessionId`, yielding 400. Corrected it
   to the existing typed `session_id`; private shells/listener were cleaned up.
4. `A09-resume-GREEN-attempt3.log`: exit 0; expanded HTTP scenario passed.
5. `A09-resume-authority-attempt1.log`: exit 101, concurrent preview lane had
   declared a test module before its file existed. Did not edit that lane.
   Resumed after the file appeared; attempt2, expanded and final authority logs
   all have passing runs on their respective source versions.
6. `A09-resume-SSH.log`: exit 101, explicit missing remote-helper build
   prerequisite. Built/staged the helper as above; provisioned SSH run passed.
7. Legacy regression: `A09-resume-legacy-RED.log` independently reproduced the
   changed missing-auth response, exit 101, under full supervisor isolation.
   Restored production GET authentication behavior without altering the strict
   `Missing auth token` assertion. The same exact selector with
   `-- --exact --nocapture` passed, exit 0, in `A09-resume-legacy-GREEN.log`.
   The final batch passed it again without `--exact`.

Controlled panic messages inside the passing authority tests are intentional
interruptions at persisted-intent/spawn phases. Assertions verify their unknown
outcomes and cleanup; they are not swallowed production errors or skipped tests.

## Actual runtime proof on final source

Final HTTP owner epoch: `1789259266004`.

- Root shell `61be9b98-47ea-4421-ab28-4cc0afee5268`, PID **39103**, native
  `/bin/zsh`, fixture `.../.tmpD5uVtF/project`. Replay retained that raw ID/PID/CWD.
- Second root shell `cc5ff45a-8f2a-4de9-9981-e1f5d7e32085`, PID **39276**,
  `/bin/zsh`, fixture `.../.tmpD5uVtF/second` (Git repository). It survived explicit
  close of the first shell with the same PID/CWD.
- Managed-worktree PID **39463** ran in the actual returned managed path.
  Relative and inherited nested-CWD shells were PIDs **39574** and **39593**.
  Missing worktree, mismatched parent worktree/project, traversal/symlink escape,
  scope, body-size, stale epoch, injected shell/env/SSH/startup, mirror list and
  socket admission were checked through HTTP/WS, not source grep.
- A real TCP request was disconnected while the server was held after commit
  and before replying. Replay returned target
  `fa92ff1a-e1fb-4c21-a0b2-5c669894da10` with original PID **41214**.
- Actual private owner subprocesses exited **73** at intent-before-spawn,
  after-spawn-before-commit, and after-commit-before-reply. Kernel NOTE_EXIT was
  subscribed before permitting owner exit for PTY PIDs **41321** and **42046**.
  Restored journals retained the original target; pending outcomes remained
  unknown, completed targets became expired, and no replacement PTY appeared.
- The limit test held **64 real machine PTYs**; the 65th was refused before
  journal intent. Every retained PTY object was checked reaped, all lifecycle
  writers completed, then the private root was removed.
- The provider fixture used a private executable/transcript in an isolated
  subprocess. It consumed `--session existing-provider-id`, verified transcript
  CWD, rejected a wrong typed key, and enforced the cross-device provider claim.
  This proves the adapter/validation/claim contract, not a real provider service.

`A09-resume-cleanup.log` collects receipts from the executed logs. Fixtures join
private HTTP listeners, close/reap their PTYs, await lifecycle writers and child
processes, and remove roots. Crash fixtures observe process exit rather than
sleeping/polling for luck. No canonical daemon PID was targeted. The final disk
check reported 78,176,180 KiB available, above the required 10 GiB floor.

## Diagnostics and downstream boundaries

LSP was attempted before compiler validation. The initial eight production-file
requests timed out at 3000 ms; a later production-file wave was cancelled by the
language server. The two new unit-test files and integration test returned no
diagnostics; state.rs returned only inactive-cfg hints. Production LSP cleanliness
is therefore not claimed. Cargo check/build passed; compiler output retained
18 library / 16 library-test warnings without suppression.

A10 must consume the authoritative journal targets and the
`machine_controllers` async fence when implementing actual controlling sockets,
generation replacement, reservation/release and input/resize fencing. A09 tests
the close-authority seam, not those future sockets. A12 must preserve the legacy
selection redaction and use a separate machine-event projection. Neither socket
drop nor event reconnect may invoke Create or DELETE implicitly.

Linux/Windows runtime, native desktop, real external provider, forced-relay
streaming, controller reconnection and the final all-writer aggregate remain
their named later gates. A09's source remains uncommitted on the inherited
composition; no claim of complete A01-A24 or AC01-AC12 approval is made.
