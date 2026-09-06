# Direct SSH project backend

Task st_01a0775f. Implementation and evidence, 2026-09-06/07 (local clock).
No commit. No real desktop/GUI proof is claimed.

## Delivered contract

The registered Tauri command is `cmd_project_register_remote`:

```json
{"request":{"workspaceId":"project","hostId":"saved-host-id","repoPath":"/srv/project"}}
```

Success returns exactly these camelCase fields:

```json
{"workspaceId":"ssh:<64 lowercase SHA-256 hex digits>","repoRoot":"/canonical/remote/directory","gitRoot":null,"hostId":"saved-host-id","hostLabel":"Saved label"}
```

`gitRoot` is a remote absolute path string when Git reports a top-level directory,
otherwise JSON null. `repoRoot` is the canonical selected remote directory, not
necessarily the Git top level. Unknown request fields are rejected. The requested
workspace ID is advisory, as in local canonical registration; clients MUST adopt
and persist the returned ID. A returned remote ID may be submitted again.

Implementation lives in `ipc/project_remote.rs`, registered in `lib.rs`, rather
than further enlarging `ipc/project.rs`. The frontend persists its normal project
row with `target: {kind: "ssh", hostId}`. No frontend changes were made here.

## Public registration core for standalone Cargo harnesses

`ferryx_lib::ipc::project_remote::register_remote_project` is public. It is the
exact function called by `cmd_project_register_remote`, not a parallel QA
implementation:

```rust
use ferryx_lib::ipc::project_remote::{
    register_remote_project, RegisterRemoteProjectRequest,
};

// Inside the harness's Tokio runtime; host_store is its persisted ssh_hosts.json.
let registered = register_remote_project(host_store, RegisterRemoteProjectRequest {
    workspace_id: requested_id, // initial slug OR a previously returned ssh:<hash>
    host_id,
    repo_path: remote_path,
}).await?;
```

The public module/types are already exported by the library. A Cargo harness can
use a path dependency named `ferryx_lib` with `package = "ferryx"` pointing to
`src-tauri`. No Tauri AppHandle is needed. The core retains stored enabled-host
resolution, field validation, bounded real SSH probing, canonical identity and
persistence. Its inputs contain no executable or trust-bypass parameter. The
Tauri wrapper additionally pings the local daemon before invoking the core; that
UI availability check is intentionally not a standalone-core prerequisite.
The lead's independently pinned SSH PATH wrapper can exercise this function
without any production environment override being added.

Chooser/runtime coordination is already aligned and was read from the current
source: `ProjectDialogs.tsx` submits the derived slug and passes the server response
through `toRegisteredProject`, which preserves `workspaceId` verbatim. `App.tsx`
re-registers SSH projects using the stored canonical ID and adopts a changed server
ID before terminal creation. `projectIdentity.ts` preserves the `ssh:` namespace
only with an explicit SSH target, rather than treating it as Local. The backend
skips the local ID validator for that reserved namespace; canonical re-registration
still resolves the enabled host and probes the actual remote directory.

`tests/remote_project_public_contract.rs` is a separate Cargo integration-test
crate, proving the same core is externally callable without Tauri construction.
It submits both a slug and a canonical-format SSH ID and asserts that both reach
the real stored-host validation boundary, not the local namespace validator.
The existing real-SSH E2E separately registers by slug and then re-registers using
the exact returned ID, asserting stable identity and fresh Git-root discovery.
Current verification results:

- `cargo test --manifest-path src-tauri/Cargo.toml --test remote_project_public_contract`:
  **1 passed**, exit **0**. This compiles/runs the caller as an external Rust crate.
- `bun run --cwd ui test src/lib/remoteProject.test.ts src/App.remote.test.tsx`:
  **17 passed** (5 adapter, 12 App lifecycle), exit **0**.
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`: exit **0**.
- The broader `--lib direct_ssh_real_transport_registration_and_pty -- --nocapture`
  run failed in its configurable loopback QA phase with `INVALID_PATH: SSH directory
  probe timed out after its bounded deadline` (8 seconds), exit **101**. This failure
  is retained, not retried or suppressed. No SSH execution/deadline code changed
  for the public export; this run is not claimed as passing real-SSH proof.

Results are captured in `backend-public-core.log`, `backend-public-core-ui.log`,
and `backend-public-core-check.log` with separate exit files. Changed-file LSP
checks reported no diagnostics before Cargo validation. No frontend files were
changed for this public API integration.

## Persistence and routing

- Uses the **existing** `ipc::ssh::get_ssh_store_path` and exact `SshHostStore` /
  `SshHost` DTO from Settings. No second host inventory or helper HostStore.
- Host inventory remains `ssh_hosts.json` under Tauri app data, with the existing
  `dev/` isolation. Remote records live in sibling `remote_projects.json`, a JSON
  object keyed by returned workspace ID. Each record contains `workspaceId`,
  `hostId`, `repoRoot`, `gitRoot`; there are no credentials or executable strings.
- Identity hashes `u64_le(UTF-8-byte-length(hostId)) + UTF-8(hostId) + UTF-8(canonicalRoot)`.
  The length is exactly eight little-endian bytes on every supported target, never
  native-width usize bytes. Length framing prevents concatenation ambiguity. Host
  ID is part of identity, not host label or bare path. Repeated registration is
  idempotent. A conflicting stored hash key is explicitly rejected instead of
  overwriting another location. Remove/re-register any test projects persisted by
  the earlier, unshipped decimal-length draft: their IDs intentionally differ from
  this finalized encoding, and old-ID validation fails closed rather than routing
  them locally.
- `ssh:` is reserved at the local registry boundary, including before local
  registration's canonical-root deduplication. A local directory with the same
  path cannot adopt or overwrite a remote identity.
- Writes serialize under a remote-store mutation mutex, use unique create-new
  temporary files (0600 on Unix), sync, and rename. Failed saves clean their temp
  file and propagate cleanup failures. Corrupt existing remote JSON is not replaced
  with an empty store. Other project/global storage is unchanged.
- There are **no local placeholder directories and no remote WorktreeManager**.
  The remote record is the registration; the daemon resolves it lazily. This is
  an intentional departure from the plan's placeholder sketch, eliminating the
  entire bookkeeping-directory fallback hazard. Local daemon registration is
  unchanged for local projects. Registration checks daemon availability before
  persisting a successful remote probe.
- `cmd_terminal_spawn` recognizes the reserved ID before CWD inheritance or local
  path validation. It re-resolves the saved enabled host, constructs the typed
  daemon startup below, and uses the existing daemon client/stream lifecycle.
  Batch spawn uses the same path. New tabs, splits and cold replacement terminals
  start at the registered remote root; passed local CWD, inherited process CWD,
  and local default-shell settings are not used.
- The daemon independently reloads the record and enabled host on every SSH
  spawn, before idempotency-cache lookup. A fresh GUI/daemon needs no in-memory
  local registration to recover routing. Existing-session attach also checks
  the stored SSH target. Disabling/deleting a host prevents subsequent spawn and
  attach; it does not retroactively terminate an already attached SSH connection.
- Unregister first invokes the existing daemon workspace revocation, which closes
  that workspace's owned PTYs, then removes only its remote record.

## Typed daemon startup and PTY behavior

The additional `TerminalStartup` variant is:

```json
{"kind":"remoteSsh","hostStorePath":"<desktop-derived absolute SSH inventory path>"}
```

The enclosing existing daemon Spawn request carries the workspace ID. This
payload is created by the desktop backend, not accepted as a frontend-selected
program/argument vector. The daemon uses the inventory path to resolve the
persisted workspace and current host. It cannot select an arbitrary executable.
A remote startup submitted for a local ID, or a remote ID without typed SSH
startup, is rejected. Frontend-supplied startup overrides (including agent resume)
and worktree identities are unsupported for remote projects.

The typed service method `TerminalService::spawn_ssh` builds only OpenSSH:

```text
ssh <bounded/noninteractive connection options> -tt [-p port] [-i identityFile]
    [-J jumpHost] [username@]hostname
    "cd '<POSIX-quoted canonical root>' && exec \"${SHELL:-/bin/sh}\" -l"
```

It uses the existing local daemon-owned portable-pty, output hub, sequenced
history, resize/write/close machinery and GUI stream pump. Remote paths are never
passed to local `CommandBuilder::cwd`, local canonicalization, or local Git.
Session metadata returns the remote project root as its CWD; the PTY has no local
worktree path. The existing local/agent shell resolver rejects this startup variant
unless the daemon has resolved it, rather than falling through to a local shell.

The current daemon protocol version remains 3. Existing `None` / AgentResume
startup serialization and behavior remain intact. An older daemon which does not
understand `remoteSsh` fails the request; there is no downgrade to local startup.
The running daemon must contain this implementation for remote tabs to work.

## Validation and failure guards

| Boundary | Guard / result |
| --- | --- |
| Host lookup | Resolve the requested stored ID, never a frontend host DTO. Missing/deleted host or absent inventory: `WORKSPACE_NOT_FOUND`, details `{hostId, reason: "hostMissing"}`. Disabled host: same existing code, reason `hostDisabled`. These adapt the plan's nonexistent NotFound/Forbidden variants. |
| Store reads | Corrupt JSON: `PARSE_ERROR`; non-missing I/O failure: `IO_ERROR`. Registration does not inherit CRUD's historical silent corrupt-store default. |
| Connection fields | Hostname/user/jump tokens are bounded to 1024 bytes and allowlisted ASCII endpoint characters; empty/leading-option/shell-metacharacter/control/whitespace tokens are rejected. Port zero is rejected. Jump chains are comma-separated safe endpoint tokens. |
| Identity file | Absolute local path or `~/...`, at most 4096 bytes, no controls. Spaces and apostrophes are retained as one argv item. Key auth requires an identity file. Relative key paths are rejected because GUI and daemon CWD may differ. |
| Remote path | Explicit absolute POSIX path, at most 4096 bytes, no control characters. No local existence test/canonicalization. Relative paths, `~`, leading-option paths and embedded newlines/NUL are rejected. |
| Shell quoting | POSIX single-quote escaping (`'` becomes `'\\''`); spaces, apostrophes and option-looking path components remain data. No helper is installed remotely. |
| Remote canonicalization | SSH executes `cd` to quoted input, `pwd -P`, and `git rev-parse --show-toplevel`. A strict NUL-framed `FERRYX_REMOTE_V1` response must contain a UTF-8 absolute canonical directory and optional absolute Git root. Malformed/bannering stdout fails closed. |
| SSH policy | `BatchMode=yes`, `StrictHostKeyChecking=yes`, `ConnectTimeout=5`, `ConnectionAttempts=1`, `ServerAliveInterval=15`, `ServerAliveCountMax=2`, `ClearAllForwardings=yes`, `PermitLocalCommand=no`, `RemoteCommand=none`, `ControlMaster=no`, `ControlPath=none`. Saved port/key/jump are honored. |
| Probe lifetime | Async child with null stdin, 8-second total deadline and 16 KiB cap on each output stream. Timeout/read overflow kills and reaps the direct SSH child; cancellation uses Tokio `kill_on_drop`. No polling or unbounded `Command::output()` in registration. |
| Network/auth/trust/path failure | No record is persisted. Nonzero SSH exit / timeout / invalid remote output: `INVALID_PATH`; process/output I/O failures: `IO_ERROR`. Host is checked again after the probe and a changed host snapshot is rejected. |
| Terminal connection failure | SSH reports its failure in the PTY and exits; the daemon never starts a replacement local shell. No helper-retained remote PTY/reconnect guarantee is claimed. |
| Worktree/status/branch operations | Local `WorkspaceRegistry` manager/target resolution rejects `ssh:` with `UNSUPPORTED`, including unloaded/deleted remote records. Covers list/create/delete/destructive delete/status/branch preview/project branch listing. Historical host-DTO SSH worktree commands explicitly return `UNSUPPORTED` too. |
| Reveal | `cmd_path_reveal` accepts optional `workspaceId`; only an explicitly remote workspace ID returns `UNSUPPORTED` before local filesystem access. A local ID or absent context preserves legacy local-only path semantics. The guard has no path/store input, so saved remote roots or corrupt SSH metadata cannot affect local Reveal. |
| Same local/remote path | Path strings never establish provenance. Identical paths, including remote `/` or `/Users/admin`, remain independent by workspace identity. The host-aware Sidebar disables remote Reveal and rejects stale native callbacks using `project.target.kind === "ssh"`; direct backend callers must supply remote workspace context. |
| Browser command scope | The earlier draft's browser-opening wrapper has been removed, and the original foreign `cmd_open_file_path` registration restored. No browser/foreign IPC implementation edits are included in this correction. |

Passwords, first-use host-key prompts, remote non-POSIX login shells, remote Git
worktree mutation, remote agent resume, and remote CWD tracking beyond project
root are not supported in this direct-shell version. Configure host trust and
key/agent authentication using normal OpenSSH before registration. Local SSH
configuration remains trusted user configuration (e.g. bastion authentication).

## Executed evidence

- **RED before production changes:**
  `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::exec::tests::probe_honors_saved_connection_options`
  collected one test and failed its existing argv assertion: missing
  `["-p", "2200"]` from `["ssh", "-o", "BatchMode=yes", "-o",
  "ConnectTimeout=2.5", "user@maho-win"]`. Exit **101**.
  `backend-red.log`, `backend-red.exit`.
- Initial GREEN: `cargo test ... --lib ssh::`: **28 passed**, exit **0**.
  `backend-green-ssh.log`.
- Real transport: `cargo test ... --lib direct_ssh_real_transport_registration_and_pty -- --nocapture`:
  **1 passed**, exit **0** (`backend-real-ssh.log`). Uses ephemeral loopback
  sshd in inetd mode, generated temporary keys, isolated known_hosts, and a child
  test process with an isolated SSH launcher. It exercises real OpenSSH probe,
  canonical directory/quoting, persisted registration, and real daemon PTYs.
  Output subscription precedes input; an octal-escaped marker prevents terminal
  input echo from satisfying the expected exact remote-root assertion. No sleeps.
  No user's known_hosts/config or live daemon is changed.
- `backend-regression.log`: `--lib ssh` **30 passed**; `project_remote` **4**;
  `terminal::shell` **22**; `daemon::protocol` **14**; `test_agent_resume` **13**;
  `test_project_registration_then_daemon_spawn` **1**; `cmd_project_unregister`
  **2**; `cmd_path_reveal` **1**. Every command exit **0**.
- A subsequent wrapper build exposed Tauri's crate-global command macro names:
  duplicate `__cmd__cmd_open_file_path` / `__tauri_command_name_cmd_open_file_path`
  (E0428), exit **101**, recorded in `backend-final.log`. Fixed by giving the
  wrapper a distinct Rust name and using Tauri 2.6.3's command `rename` attribute
  to preserve the original wire name; no errors were suppressed.
- Final post-fix command results (`backend-verified.log`, overall exit **0**):

  | Command after `cargo ... --manifest-path src-tauri/Cargo.toml` | Result | Exit |
  | --- | --- | --- |
  | `test --lib ssh` | 30 passed, including real SSH/PTY and deadline/reaping tests | 0 |
  | `test --lib project_remote` | 4 passed, including all local Git/reveal guards | 0 |
  | `test --test ipc_hardening_contract` | 7 passed | 0 |
  | `test --test rorca_native_contract` | 17 passed | 0 |
  | `test --test worktree_safety` | 8 passed | 0 |
  | `check` | Finished dev profile | 0 |
  | `build --bin ferryx` | Finished dev profile in 1m 10s | 0 |

  Builds ran as monitored background jobs with captured stdout/stderr and exit
  files. Existing/foreign warnings remain visible: vendored wgpu-hal cfg warnings,
  notification/native-terminal unused fields/imports, and unused terminal/worktree
  lifecycle/lease helpers. No warning was suppressed or unrelated source fixed.
- Final boundary audit also moved daemon `RegisterWorkspace` namespace validation
  ahead of local canonicalization/Git construction. `backend-boundary.log` records
  the post-audit real-SSH test (**1 passed**), `--lib register_workspace`
  (**3 passed**), `cargo check` and `cargo build --bin ferryx`; all exits **0**.
  This verifies even direct daemon requests cannot interpret a remote ID locally.
- LSP was requested for every changed Rust file (the SSH directory scanned all
  eight files) before Cargo validation. Initial and post-integration waves reported
  no diagnostics. One wrapper-only refresh was cancelled/timed out; after the
  server resumed, final checks of the wrapper, wrapper tests, lib.rs, daemon server
  and SSH integration test all reported no diagnostics. Cargo remains the compiler
  gate, not an inference from an empty LSP response.
- Scoped rustfmt and `git diff --check` both exited **0**.

## Shared-tree scope and limits

No Cargo manifest, native_terminal implementation, permissions implementation,
frontend, vendor, orphaned ferryx_scope, or global session/storage edits were
made by this task. The foreign browser file-opening implementation is untouched;
its original registered entry is restored after removal of the draft wrapper.
Other sessions changed/formatted many
files while this task ran, including lib.rs; those edits were preserved. Two
new test files disappeared after an initial passing run and were restored from
this task's authored content; final presence is checked rather than assuming the
shared tree stayed unchanged.

This is backend real-SSH/PTY evidence, not desktop Add Project chooser, native
surface, keyboard interaction, or deployment-to-an-independent-machine proof.
Those GUI acceptance gates remain for the integration verifier.

## Lead correction: reveal provenance and fixed-width identity

The path-prefix guard was a regression and is removed, not relaxed. Its replacement
accepts only `Option<&str>` workspace context; no path can be misclassified as
remote. Tests register a real local directory and a remote project with the exact
same canonical path, permit both legacy and explicitly local Reveal, and reject
explicitly remote Reveal. Additional cases save remote `/` and `/Users/admin` and
corrupt the remote store without changing local reveal eligibility. A UTF-8 host
ID vector asserts the exact eight-byte little-endian length prefix for identity.

RED (`backend-reveal-red.log`): three collected tests failed assertions, exit
**101**: same-path local reveal, remote-root interference, and the fixed-width
identity vector. The existing host-aware UI guard was inspected and its unchanged
`Sidebar.remote.test.tsx` suite passed **3 tests**, exit **0**, including disabled
Reveal and rejection of stale native callbacks (`backend-reveal-ui.log`). No UI
files were edited. The browser wrapper and its wrapper-specific assertion were
removed with that out-of-scope draft behavior, not to suppress a failing test.

GREEN (`backend-reveal-green.log`, overall exit **0**):

- `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh`: **33 passed**, including
  the three new regressions and real OpenSSH registration/PTY QA seam.
- `--lib project_remote`: **4 passed**; `--lib cmd_path_reveal`: **1 passed**.
- `--test rorca_native_contract`: **17 passed**.
- `cargo check --manifest-path src-tauri/Cargo.toml`: exit **0**.
- `cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx`: exit **0**.

All six changed Rust files had no LSP diagnostics before Cargo validation; scoped
rustfmt and final diff-check exited **0**. Existing unrelated build warnings remain
visible. The real-SSH E2E/VM QA entry point remains callable and is unchanged by
this correction.

## External VM QA entry point

The existing Rust E2E is now configurable through the **test-only**
`FERRYX_SSH_QA_CONFIG` environment variable. An archived machine-specific manifest is
`docs/evidence/project-location/backend-qa-vm.json`, using the lead's
`admin@192.168.64.197` endpoint and independently pinned trust-file path. The lead
has removed that private trust fixture and stopped the VM; this archived command
is not currently runnable without provisioning a new explicit QA configuration.

```sh
FERRYX_SSH_QA_CONFIG="$PWD/docs/evidence/project-location/backend-qa-vm.json" \
  cargo test --manifest-path src-tauri/Cargo.toml --lib \
  direct_ssh_real_transport_registration_and_pty -- --nocapture
```

The manifest contains the exact `SshHost` DTO plus `knownHostsFile`, `repoPath`,
`expectedRepoRoot`, and nullable `expectedGitRoot`. Its initial path is
`/Users/admin`, treated as a plain-directory project; the lead can substitute an
actual fixture repository and its expected canonical/Git roots. For an explicit
key, set the host's existing `identityFile` and `authMethod: "key"`; otherwise
OpenSSH uses the agent/default identities. No private key is embedded in the
manifest.

`remote_ssh_qa.rs` is compiled only beneath the existing Unix test module. It
launches a child test process with a private PATH-local `ssh` adapter which execs
real `/usr/bin/ssh`, specifying `-F /dev/null`, the explicit `UserKnownHostsFile`,
`GlobalKnownHostsFile=/dev/null`, `StrictHostKeyChecking=yes`, `BatchMode=yes`,
and `UpdateHostKeys=no`. It does not read/write personal SSH configuration or
known_hosts, does not accept unknown host keys, and verifies that the supplied
pin file remains byte-for-byte unchanged. No production trust override, helper,
remote fixture creation, repository mutation, or VM lifecycle operation is added.

This seam invokes the actual registration/probe function used by the Tauri
command, persists and reloads private test host/project stores, and starts two
real daemon-owned SSH PTYs. Output includes machine-readable evidence markers:

- `FERRYX_SSH_QA_REGISTERED <RegisteredRemoteProject JSON>`
- `FERRYX_SSH_QA_PTY_OK qa-new-tab <remote root>`
- `FERRYX_SSH_QA_PTY_OK qa-split-restore <remote root>`

With no QA environment variable, the deterministic loopback E2E still runs and
also exercises this configurable seam against its generated sshd fixture.
`backend-qa-seam.log` records **1 passed, exit 0**, including registration JSON and
both exact remote-CWD PTY markers. Both changed Rust files had clean LSP results
before Cargo; a refresh after the final comment-only edit timed out and is not
claimed clean. Scoped rustfmt/diff checks exited 0. No sleeps or ignored tests
were added.

**Actual VM attempt is blocked:** the supplied directory
`/tmp/ferryx-project-location-qa.VM4GNI` is absent on this workstation. Running the
manifest command above compiled and collected the test, then failed before SSH
with `read pinned QA known_hosts: Os { code: 2, kind: NotFound, message: "No such
file or directory" }`, exit **101** (`backend-qa-vm.log`, `backend-qa-vm.exit`).
No replacement key was fetched, no trust checking was weakened, and no VM
registration or desktop success is claimed. The lead retains VM cleanup ownership.
