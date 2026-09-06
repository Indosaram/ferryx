# Current SSH remote-project registration contract

Date: 2026-09-06. Task: st_01a07749. Discovery only; no production edits or commits.

## Decision

**There is no supported end-to-end SSH remote-project registration path in the
current desktop app. Backend integration changes are necessary for a working
Remote project, not merely a Local/Remote dialog change.**

- The real desktop registration request is `{ workspaceId, repoPath }`, returning
  `{ workspaceId, repoRoot, gitRoot }`. Neither side carries a host or execution
  target (`ui/src/lib/tauri.ts:49-53,140-142`,
  `src-tauri/src/ipc/project.rs:120-135,155-180`).
- The eight registered SSH commands provide host CRUD/import/Test and remote Git
  worktree operations, **not project registration**
  (`src-tauri/src/lib.rs:848-855`, `src-tauri/src/ipc/ssh.rs:81-250`).
- A separate, reusable helper implements `project.register` on its own runtime,
  but this is **scoped implementation code, not a registered desktop command**
  (`src-tauri/src/ferryx_scope/ssh/helper.rs:42-52`). The desktop library's module
  declarations include `ssh` and `scoped_contracts`, not `ferryx_scope`
  (`src-tauri/src/lib.rs:1-16`). Its test compiles that helper via `#[path]`
  (`src-tauri/tests/scoped_ssh.rs:1-3`). The Cargo manifest declares only the
  `ferryx` binary (`src-tauri/Cargo.toml:16-18`); the helper entry explicitly says
  an integrator may register it (`src-tauri/src/ferryx_scope/ssh/standalone.rs:1-8`).

The smallest *currently callable desktop* path is local registration. The
smallest *existing remote registration primitive* is the isolated helper's
`project.register`; there is no existing UI-to-SSH transport joining the two.
Do not represent saving `{ hostId, path }` in a sidebar row, or passing a remote
path to `registerProject`, as working remote registration.

## Real caller/definition chain: local registration

1. `ui/src/state/inactiveProjectWorktrees.ts:3-19` imports the actual
   `registerProject` binding into its default services; lines 144-152 call it
   with the stored project's `workspaceId` and `repoRoot`, then list worktrees.
   This is an important second caller beyond an Add Project dialog: inactive
   project refresh will currently treat every registered project as local.
2. `ui/src/lib/tauri.ts:140-142` invokes `cmd_project_register` with `{ request }`.
   The handler is registered at `src-tauri/src/lib.rs:845`.
3. `src-tauri/src/ipc/project.rs:162-171` offloads
   `register_canonical_project`; its lines 84-114 instantiate
   `WorktreeManager::try_new`, adopt an existing canonical-root owner, otherwise
   call `WorkspaceRegistry::register_unique_root`.
4. `src-tauri/src/worktree/manager.rs:130-148` canonicalizes the **local** path,
   requires an existing non-filesystem-root directory, then runs local Git
   `rev-parse --show-toplevel`. Plain directories are valid terminal-only
   projects. `src-tauri/src/worktree/registry.rs:18-36,70-110` validates IDs,
   deduplicates canonical roots under a write lock, and suffixes occupied IDs.
5. `src-tauri/src/ipc/project.rs:173-178` registers the returned identity/root on
   the default daemon. `src-tauri/src/daemon/client.rs:641-663` sends
   `DaemonRequest::RegisterWorkspace`, and
   `src-tauri/src/daemon/server.rs:1768-1795` again requires an absolute,
   canonical local directory and registers it in the daemon's workspace map.

Consequences: callers must adopt the returned workspace ID; a remote path that
happens to exist locally can select the wrong machine's directory. The Rust
request uses `deny_unknown_fields`, so adding `hostId` or `runTarget` only to a
frontend invoke payload is not a backward-compatible implementation
(`src-tauri/src/ipc/project.rs:120-125`).

LSP successfully followed the import at
`ui/src/state/inactiveProjectWorktrees.ts:6` to
`ui/src/lib/tauri.ts:140`; following the service call at line 144 reaches its
typed service definition at line 12. Rust go-to-definition returned no result,
and a TypeScript references request returned only the declaration despite the
read caller above. Backend links and broader callers were therefore checked by
source reads/text references, not inferred from incomplete LSP results.

## Supported desktop SSH host bindings and persistence

These are **existing backend command names and wire shapes**; no matching SSH
invoke wrappers were found in the requested `ui/src/lib/**` or
`ui/src/state/**` modules. `sshHosts.ts` is not present there. Do not invent a
current frontend host store based on previous-session memory.

| Command | Top-level invoke arguments | Result | Source |
| --- | --- | --- | --- |
| `cmd_ssh_list_hosts` | none | `SshHost[]` | `src-tauri/src/ipc/ssh.rs:81-85` |
| `cmd_ssh_import_config` | `{ configText: string }` | updated `SshHost[]` | `src-tauri/src/ipc/ssh.rs:87-113` |
| `cmd_ssh_update_host` | `{ host: SshHost }` | updated `SshHost[]` | `src-tauri/src/ipc/ssh.rs:115-136` |
| `cmd_ssh_delete_host` | `{ id: string }` | updated `SshHost[]` | `src-tauri/src/ipc/ssh.rs:138-159` |
| `cmd_ssh_test_connection` | `{ host: SshHost }` | `{ host, reachable, lastError?, checkedAt }` | `src-tauri/src/ipc/ssh.rs:12-20,161-184` |

`SshHost` uses camelCase: required `id`, `label`, `hostname`, `source`
(`config | manual`), `authMethod` (`agent | key`); optional `username`, `port`,
`identityFile`, `jumpHost`, `disabled`
(`src-tauri/src/ssh/mod.rs:7-39`). CRUD returns the complete updated inventory,
which can drive both Settings and a Remote selector without a second store.

- Host JSON lives in Tauri app-data `ssh_hosts.json`, or
  `dev/ssh_hosts.json` when `is_dev_runtime()` is true. It contains
  `{ hosts: SshHost[], tombstones: string[] }`
  (`src-tauri/src/ipc/ssh.rs:22-43`). This is not the separate helper HostStore.
- Reads silently replace missing, unreadable, or corrupt files with an empty
  store (`src-tauri/src/ipc/ssh.rs:45-50`). Writes create parent directories,
  write a fixed `ssh_hosts.json.tmp`, then rename, with no mutation lock in the
  command implementation (`src-tauri/src/ipc/ssh.rs:52-71,93-158`). Do not claim
  corrupt-store preservation or concurrent-update serialization here.
- Upsert identity is **`id`**; import deduplication and deletion tombstones use
  **endpoint key** `[user@]hostname:port`, default port 22
  (`src-tauri/src/ipc/ssh.rs:96-107,123-132,147-152`,
  `src-tauri/src/ssh/mod.rs:50-57`). Label is not identity. Imported ID is
  `ssh-<alias>`; label preserves alias while hostname may become HostName
  (`src-tauri/src/ssh/config.rs:112-138`). Manual IDs must be supplied by callers.
- Import accepts pasted configuration text; it does not read `~/.ssh/config`.
  It takes the first Host token, skips wildcard/negative aliases, parses a
  bounded subset of fields, ignores other directives, and caps each imported
  result at 100 (`src-tauri/src/ssh/config.rs:17-90,103-110`).
- Update does not validate hostname, enabled status, or connection options;
  Test and worktree commands accept caller-supplied host DTOs without looking
  their IDs up in the store (`src-tauri/src/ipc/ssh.rs:116-184,187-250`). Deleted
  or disabled-host rejection must not be assumed to exist at this boundary.

Project persistence exposed within the assigned frontend scope consists of the
canonical `ferryx.projects` / `ferryx.active-project` keys and legacy key copying
(`ui/src/lib/storageKeys.ts:1-2,17-19,28-49`). The project type still has no
target (`ui/src/lib/tauri.ts:49-53`). `RunTarget` exists separately as
`{ kind: "local" } | { kind: "ssh", hostId: string }`
(`ui/src/lib/scopedContracts.ts:11`); its existence is **not** a persisted-project
migration. The historical seed explicitly records that limit
(`docs/evidence/ferryx-scope/seed-verified.md:105-112`). Project storage writers
and Settings composition outside this assigned source scope need the UI
discovery lane's current audit, not this report's historical citations.

## Validation and why historical SSH commands are not the registration path

- Test uses `ssh -o BatchMode=yes -o ConnectTimeout=2.5 [user@]hostname`, omitting
  saved port/key/jump and any terminating probe command
  (`src-tauri/src/ssh/exec.rs:3-13`). The handler waits on `output()`, returns
  reachability and the last stderr line, and has no total deadline or typed
  trust/auth failure classification (`src-tauri/src/ipc/ssh.rs:162-183`). The
  nondefault port used for isolated SSH QA therefore cannot be assumed to work
  through this Test path. Do not claim the fractional timeout is rejected: a
  historical nonconnecting probe accepted it
  (`docs/evidence/ferryx-scope/discovery/ssh.md:30-36`).
- `interactive_argv` includes port/key/jump but is not the probe builder and has
  only definition/test text references, not a production spawn caller
  (`src-tauri/src/ssh/exec.rs:15-35,85-108`).
- Existing worktree APIs are `list(host)`,
  `create(host, path, wsId, slug, baseRef?)`, and `delete(host, path)`
  (`src-tauri/src/ipc/ssh.rs:187-250`). Their command builders omit repository
  `git -C`, port/key/jump, and remote-root validation. Create/delete interpolate
  raw path/base into remote-shell command text
  (`src-tauri/src/ssh/worktree.rs:61-95`). Do not use them unchanged as a remote
  repository validation or registration API.

## Separate scoped helper: reusable primitive, not shipped integration

The helper's actual registration sequence is:

1. Provision a private runtime root on the remote machine and run helper
   `daemon --root <root> --host-id <id>`; bridge uses
   `bridge --stdio --root <root>` (or `FERRYX_REMOTE_ROOT`). These CLI parsers
   exist at `src-tauri/src/ferryx_scope/ssh/process.rs:68-77`, but the helper is
   not a declared Cargo binary.
2. Bridge reads the runtime's private `endpoint.json`, connects through a Unix
   socket or non-Unix loopback socket, and injects the endpoint token into each
   allowlisted request (`src-tauri/src/ferryx_scope/ssh/process.rs:18-38,46-68`).
3. Send 4-byte big-endian-length-prefixed JSON (maximum 1 MiB), protocol 1.
   `handshake` returns protocol, `sshHelperV1`, hostId, ownerId, epoch, OS, arch;
   `project.register` params are `{ id: string, path: string }`, returning
   `{ projectId: string }`
   (`src-tauri/src/ferryx_scope/ssh/helper.rs:7-25,37-52`). The bridge wraps the
   response as `{ ok: true, data }` or `{ ok: false, error: string }`, not the
   shared `ScopeResult`/structured IPC error shape
   (`src-tauri/src/ferryx_scope/ssh/process.rs:9-13`).
4. Registration canonicalizes the path **on the remote runtime**, requires a
   directory inside its configured root, and rejects rebinding an existing ID
   to a different path. It neither verifies Git backing nor deduplicates
   different IDs pointing to the same path; it returns no canonical repoRoot.
   Projects are an in-memory HashMap, initially empty on each Runtime creation
   (`src-tauri/src/ferryx_scope/ssh/helper.rs:31-52`). Persistence/re-registration
   across helper restart is not implemented by this primitive.

The scoped `HostConfig` is incompatible with desktop `SshHost`: `name`, required
`user` and `port`, `proxyJump`, required `knownHostsFile`, and no `disabled`,
`source`, or `authMethod`. Its validator rejects empty/leading-option/unsafe
endpoint tokens, port zero, and empty trust path, and produces strict,
noninteractive SSH flags with explicit port/key/jump
(`src-tauri/src/ferryx_scope/ssh/config.rs:4-29`). Its separate `HostStore` writes
a **bare array**, preserves corrupt existing JSON, locks mutations, checks ID
duplicates, and uses a UUID temporary file with restrictive permissions
(`src-tauri/src/ferryx_scope/ssh/config.rs:31-54`). This store is not connected to
desktop host commands; do not silently introduce two Settings inventories.

Remote session identity is `{ hostId, ownerId, epoch, backendSessionId }`
(`ui/src/lib/scopedContracts.ts:3-9`). Runtime host is the daemon's `--host-id`,
not inferred from an SSH address; owner/epoch are freshly generated, and PTY
operations reject mismatched identity
(`src-tauri/src/ferryx_scope/ssh/helper.rs:36,90-97`). A future desktop bridge
must compare the selected persisted host identity to handshake identity.

## "Remote scopes" and browser Remote Access are not SSH registration

`ui/src/lib/remoteClient.ts:25-80` is an HTTP client with bearer auth and
`/api/v1/workspace/state` / worktree operations. Its `spawnTerminal` currently
returns a synthesized ID rather than registering an SSH project (lines 93-101).
The real test named `scoped_remote_inventory_lists_registered_projects_without_desktop`
creates local temp repositories and local PTYs, serves an authenticated loopback
HTTP gateway, then reads `/api/v1/sessions`
(`src-tauri/src/remote/tests.rs:574-674`). Its success proves inventory independent
of desktop selection, not SSH execution or a host registry.

Likewise, prior design proposals and green scoped-helper logs do not establish
a desktop integration. The earlier SSH discovery explicitly marks its remote
API/types as proposed additions
(`docs/evidence/ferryx-scope/discovery/ssh.md:64-84`).

## Smallest working implementation boundary for the requested UX

Scope correction supplied by the parent after discovery: prior user decision
`ferryx-ssh-remote-hosts-decisions.md` D5 requires only an SSH shell for v1;
a remote agent/helper is **not required**. That decision is supplied context,
not a memory file independently inspected in this lane. The scoped helper above
is optional reusable code, not a prerequisite for the requested feature.

Recommendation, **not an already supported API**: prefer a direct SSH-shell
integration for this v1 boundary; the planner may consider the helper separately.

1. Gate the local picker behind explicit Local selection; leave the verified
   local `registerProject` contract unchanged.
2. Use one Settings-owned host inventory through the existing desktop CRUD
   commands, with typed frontend wrappers. Remote chooses a persisted enabled
   host ID plus an explicit remote directory; re-resolve that ID at submission
   so deletion/disable cannot fall back to Local. Empty inventory links to the
   real Settings host-management surface, once wired by the UI lane.
3. Add a typed desktop remote-registration boundary, e.g.
   `registerRemoteProject({ hostId, workspaceId, repoPath })`. Resolve the host
   server-side and validate connection options. A direct SSH integration can
   validate the remote directory and launch an SSH shell there without installing
   a remote agent. It must correctly quote remote-shell arguments, honor saved
   connection options, and avoid the unsafe historical worktree builders.
   This direct transport/registration binding still needs implementation; the
   scoped helper's framed registration is an optional alternative, not mandatory.
4. Persist the immutable project execution target and host-qualified remote
   identity; migrate absent targets to Local. Route subsequent project refresh,
   worktrees, and terminal creation by target, especially the verified inactive
   project caller above. A sidebar-only record must not trigger local
   canonicalization/spawn for its remote path. Surface missing host, trust
   failure, and failed registration rather than adding a fake row. Only a
   helper-based alternative would introduce a missing-helper prerequisite.

The frontend-only portion can deliver the chooser. **The complete requested
Remote registration cannot be delivered without backend changes.** Do not
expand this conclusion into a requirement for a remote helper, retained remote
PTYs, or the entire scoped terminal/control suite. Direct SSH-shell integration
is viable under D5; helper-specific lifecycle guarantees are outside that v1
requirement unless separately requested.

## Exact existing tests and commands

These are discovered entry points, **not test runs performed in this task**.
This report-only task added no prose-pinning tests and started no app, SSH server,
remote helper, build, or test subprocess.

| Existing entry point | Command from repository root | What the source covers |
| --- | --- | --- |
| `ui/src/lib/tauri.test.ts:59-82` | `bun run --cwd ui test src/lib/tauri.test.ts` | Native local registration/branch invoke payloads with mocked Tauri; not actual Git despite the test title. |
| `ui/src/state/inactiveProjectWorktrees.test.tsx:48` | `bun run --cwd ui test src/state/inactiveProjectWorktrees.test.tsx` | Existing inactive-project register/list caller contract; not remote routing. |
| `src-tauri/tests/rorca_native_contract.rs:211-307` | `cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract project_registration_` | Real local Git root/branch registration and idempotency (filter also selects other registration cases). |
| `src-tauri/src/ipc/tests.rs:676-733` | `cargo test --manifest-path src-tauri/Cargo.toml --lib test_project_registration_then_daemon_spawn` | Local registration reaches the real test daemon and can spawn/close a terminal. |
| `src-tauri/src/ssh/{mod,config,exec,worktree}.rs` tests; `src-tauri/src/ipc/ssh.rs:254-288` | `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::` | Host DTO/key/config/argv/parser shapes and JSON round-trip; no desktop Remote-project integration. |
| `src-tauri/tests/scoped_ssh.rs:5-22`; helper test at `src-tauri/src/ferryx_scope/ssh/helper.rs:124-140` | `cargo test --manifest-path src-tauri/Cargo.toml --test scoped_ssh` | Scoped connection-option shape, corrupt-store preservation, in-process retained PTY primitive. |
| `src-tauri/src/remote/tests.rs:574-674` | `cargo test --manifest-path src-tauri/Cargo.toml --lib scoped_remote_inventory_lists_registered_projects_without_desktop` | Real authenticated HTTP inventory of local project PTYs, not SSH. |

Important test limit: the scoped helper's retained-PTY test calls `Runtime`
directly and simulates EOF with an empty Cursor; it does not run/sever/reconnect
an actual SSH bridge. Its `pty.read(waitMs: 2000)` waits for any output and only
asserts nonempty chunks, not the command's exact output; shell startup output
can satisfy that assertion (`src-tauri/src/ferryx_scope/ssh/helper.rs:130-138`).
It is not a reliable signal for the full reconnect behavior its name suggests.
Future acceptance tests need subscribed readiness/output events before trigger,
bounded awaits, and exact target/path checks, not sleeps or output timing luck.

Historical evidence is mixed and must remain labeled: `green-helper.log:63-70`
under `docs/evidence/ferryx-scope/implementation/ssh/` reports three passing
tests; `green-config.log:136-157` in the same directory actually reports two
passed, one failed (`UNSUPPORTED`, exit 101). Neither is a fresh result. Its
`standalone-build.log:1` uses direct rustc with hash-specific cached rlibs, not
a supported Cargo helper target.

## Verification and working-tree safety

Read root/backend/IPC/worktree/daemon/remote/UI-lib/UI-state AGENTS. Traced actual
definitions/callers as above and checked current command/module registrations.
Only this report was created, with apply_patch. All initially listed foreign
dirty files remain read-only, including Cargo.toml. Additional foreign changes
appeared during discovery in permissions/settings and `ui/src/lib/tauri.ts` /
`types.ts`; none were edited by this task. Source lines cite the tree observed
during discovery, so concurrent integration changes can supersede them.
Report ASCII/whitespace checks passed. Markdown LSP diagnostics were requested
but unavailable because no `.md` server is configured; no clean LSP result is
claimed. The inactive-project test uses existing `waitFor` polling at
`ui/src/state/inactiveProjectWorktrees.test.tsx:54`; it is a discovered test
entry point, not an approved pattern for new deterministic acceptance coverage.

Assumption: "Remote project" means a project that is validated and subsequently
operates on the chosen SSH machine, not a decorative local record. Under that
interpretation the backend-required decision above is definitive. Settings
composition and sidebar event wiring outside this assigned scope remain the
parallel UI discovery lane's responsibility.
