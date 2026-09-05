# A2 SSH settings / Run on: bounded implementation contract

Status: discovery complete; implementation and runtime QA NOT executed. 2026-09-05.
Scope: SSH host settings, remote project/worktree execution, terminal reconnect only.
Applicable root, Rust, IPC, daemon, terminal, worktree, UI, components, state and lib AGENTS read.
LSP document-symbol request failed: daemon unreachable at ~/.omo/lsp-daemon/v0.1.0/daemon.sock.
Text references substituted; no claim of successful LSP references, diagnostics, tests or build.
Foreign dirty work is active; this assignment writes only this report and makes no commits.

## Verified implementation, not roadmap assumptions
- `src-tauri/src/ssh/mod.rs:9-24`: SshHost persists id, label, hostname, optional user/port/key/jump,
  source, authMethod and disabled; Agent/Key are the only auth variants (35-39).
- `src-tauri/src/ipc/ssh.rs:24-49`: hosts plus tombstones live in app-data ssh_hosts.json,
  under dev/ for dev runtime. Missing, unreadable AND malformed stores silently become empty.
- `src-tauri/src/ipc/ssh.rs:52-71,82-158`: blocking-offloaded CRUD/import really writes JSON via
  fixed .json.tmp then rename; no mutation lock in these functions, so concurrent writes can race.
  Updates upsert by id; deleting imported hosts records endpoint-key tombstones; import skips existing keys.
- `src-tauri/src/ssh/mod.rs:50-56`: endpoint key is user@hostname:port (default 22), not alias/id.
- `src-tauri/src/ssh/config.rs:17-90,103-138`: text-only partial parser, first Host token only,
  skips wildcard/negative aliases, handles hostname/user/port/first identity/proxyjump; import cap 100.
  Include/Match semantics are not implemented; imported id is ssh-<alias>, label preserves alias,
  hostname replaces alias when HostName is present. This is not full OpenSSH config evaluation.
- `src-tauri/src/lib.rs:787-794`: all eight SSH commands are registered Tauri handlers.
  Whole ui/src SSH-symbol search found no consumers; settings SectionId has no SSH entry
  (`ui/src/components/settings/types.ts:1-10`; navigation `SettingsDialog.tsx:145,178` is Remote Access).
- `ui/src/components/ProjectDialogs.tsx:78-114,331-370`: Add Project opens a local folder picker;
  Add Worktree lists local branches then calls createWorktree. No Run on selector exists here.
- `ui/src/lib/tauri.ts:49-53,140-141`: RegisteredProject has workspaceId/repoRoot/gitRoot only;
  registerProject accepts workspaceId/repoPath, with no execution target.
- `src-tauri/src/ssh/exec.rs:3-35`: probe uses BatchMode and ConnectTimeout=2.5 but omits port,
  identity and jump flags and has no terminating remote command. Interactive argv includes those flags.
  Text references to interactive_argv are only its definition and tests (15,85-89), not PTY spawn.
- `src-tauri/src/ipc/ssh.rs:162-183`: probe waits on output(), returns reachable/lastError/checkedAt,
  takes only final stderr line; no typed auth/host-key distinction or total execution deadline.
  Local nonconnecting `ssh -G -F /dev/null -o ConnectTimeout=2.5 ferryx-a2.invalid` accepted it,
  reporting connecttimeout 2: do NOT claim this timeout is rejected on this installed SSH.
- `src-tauri/src/ssh/worktree.rs:61-95`: remote Git argv omits connection flags, repository cwd,
  quoting and root jail; path/base are interpolated into remote shell text. Branch formatting alone
  is delegated to WorktreeManager::format_branch_name (75-76; definition manager.rs:325).
- `src-tauri/src/ipc/ssh.rs:187-250`: list/create/delete run these shell commands; accept full host
  and raw paths, return generic internal errors, emit no worktree-change event here.
- `ui/src/state/workspaceRuntime.ts:15-42,159`: service seam lists by workspaceId, not host;
  `workspaceStore.ts:489-517` spawns with worktree.path as cwd and stores unqualified backendSessionId.
  `workspaceRestore.ts:66-80,103-127` reconciles persisted state against the default terminal inventory.
- `src-tauri/src/ipc/terminal.rs:624-695`: spawn validates cwd on LOCAL filesystem, registers local
  repo root, then calls daemon client. Passing a remote path or ssh string is not remote worktree support.
- `src-tauri/src/daemon/client.rs:22-29`: transport is UnixStream on Unix, TcpStream otherwise,
  not SSH. `daemon/protocol.rs:12,127-176` is version THREE (AGENTS' v2 statement is stale),
  with Spawn client_request_id and Attach after_sequence, but no host/target dimension.
  `ipc/terminal.rs:384-394,787-800` already exposes attachment epoch, replay position and gap.

## Recommended minimum (new contract, not existing capabilities)
Ship one headless Rust remote helper, shared PTY/worktree core where practical, over OpenSSH stdio.
The helper must own persistent PTYs independently of the SSH connection; a short-lived stdio bridge
attaches to that helper through private per-user local IPC (Unix socket / Windows loopback + token).
No GUI/GPU dependency, tmux dependency, public listener, forwarding service or full daemon admin API.
Use system OpenSSH on desktop macOS/Linux/Windows; remote helper supports those OS families too.
Manual install of a versioned helper is the first release boundary; missing binary => actionable error,
not automatic download/install. Package extraction/standalone build is a prerequisite, not verified reuse.
Use one fixed helper launch command and framed requests, never user-controlled remote shell Git text.
Remote Git receives argv and explicit validated repository cwd; canonical jail and dirty-delete checks
execute on the host. Existing SSH worktree argv builders must be replaced, not wired into UI unchanged.

### Proposed minimal API/types (camelCase wire; all additions)
```ts
type RunTarget = { kind: "local" } | { kind: "ssh"; hostId: string };
type RemoteProject = { workspaceId: string; target: RunTarget; remoteRepoId: string };
type RemoteBinding = { hostId: string; epoch: string; backendSessionId: string };
type RemoteState = "connecting" | "connected" | "disconnected" | "exited" | "error";
// Desktop boundary resolves persisted hostId; paths allowed only at explicit registration boundary.
// registerRemoteProject({hostId, workspaceId, repoPath}) -> RemoteProject
// remoteWorktrees({workspaceId, op: list | create | delete, slug?, baseRef?, requestId})
// remoteSpawn({workspaceId, slug?, cols, rows, requestId}) -> RemoteBinding
// remoteAttach({binding, afterSequence?}) -> snapshot + live stream + gap
// remoteWrite / remoteResize / remoteClose take RemoteBinding, never bare remote session id.
```
Reuse backend SshHost/CRUD DTOs after validation; add typed UI invoke wrappers, not a second host store.
Handshake returns version, capabilities, host OS and epoch; initial helper API allowlists only project,
worktree and terminal operations. No proxy of the full DaemonRequest enum (which includes Shutdown).
Persist RunTarget on project and target-qualified bindings/layout keys; absent target migrates to local.
Settings: manual add/edit/delete, paste-config import, Test, disabled toggle, explicit error display.
Add Project: Run on defaults Local; SSH selection replaces native picker with remote repository field.
Add Worktree inherits immutable project target; no implicit clone/upload/local-to-remote migration.

### Reconnect and host-key behavior
- Disconnect detaches transport, not PTY. Freeze input, retain pane and binding, show Disconnected;
  explicit Reconnect re-handshakes and reattaches using epoch + last sequence. Never queue keystrokes.
- Same epoch/session replays once; gap explicitly resets/rebuilds terminal display from snapshot.
  Missing session or changed epoch marks Exited; creating a replacement shell requires explicit action.
- Retry spawn only with the same requestId and helper dedupe in the same epoch; uncertain delivery
  across epoch changes is surfaced. Never blindly replay write/resize/close/worktree mutations.
- Enforce BatchMode, StrictHostKeyChecking=yes, integer connect timeout and total deadline/cancel.
  Unknown/changed keys fail closed; no auto-accept, key deletion or silent known_hosts writes.
  Operator verifies fingerprint out of band and manages trust outside Ferryx before retrying.
- Proposed structured codes: SSH_TRUST_FAILED, SSH_AUTH_FAILED, SSH_UNAVAILABLE,
  SSH_TIMEOUT, REMOTE_RUNTIME_MISSING, REMOTE_PROTOCOL_MISMATCH, REMOTE_SESSION_LOST.
  OpenSSH exit 255 alone cannot classify trust vs auth; backend classifier may retain bounded stderr,
  must fall back to SSH_UNAVAILABLE when uncertain. UI never regex-matches messages.

## Executable ownership and RED seams
1. SSH owner: src-tauri/src/ssh/** and ipc/ssh.rs, isolated temp-store tests. Fix fail-open reads,
   serialize mutations, validate endpoints/key paths, share connection options, bound child lifetime.
2. Runtime owner: NEW standalone helper/build target + SSH bridge; coordinate daemon/protocol and
   terminal IPC boundary edits with their owners. Remote jail, PTY lifetime and framed transport tests.
3. UI owner: NEW SSH settings section/wrappers; coordinate SettingsDialog, settings/types,
   ProjectDialogs, lib/tauri/types/sessionPersistence and state runtime/store/restore with dirty owners.
   Do not overwrite their notification/terminal work. Host deletion disables references, never closes PTYs.
4. First RED: inject a process runner and temp store at SSH boundary; Test and remote worktree requests
   must carry identical nondefault port/key/jump and terminate; corrupt store must error without rewrite.
   Existing shape tests (`exec.rs:63-108`, `worktree.rs:139-168`) pin incomplete argv, not integration health.
5. UI RED: render real settings/dialog with typed IPC adapter and isolated storage; save/reopen/import/delete,
   select SSH, assert target survives restore and local spawn is never called for that project.
6. Transport RED: private helper fixture, subscribe to ready/output/disconnect before trigger;
   sever bridge, reconnect, assert same remote PID/session, ordered replay, gap and epoch-loss behavior.
   Bound awaits; no sleeps/polling, shared daemon, persistence suite or Shutdown.

## Literal real-surface QA (future, isolated; not performed)
Prerequisite: QA owner provisions disposable SSH guest, forwarded 127.0.0.1:22222, account qa,
an existing committed repository /srv/ferryx-a2/repo, standalone helper and dedicated desktop OS account.
Dedicated account owns /tmp/ferryx-a2-key and /tmp/ferryx-a2-known-hosts verified out of band.
Guest/account provisioning and helper launch CLI are NOT available contracts yet; do not invent them.
Commands below use checked SSH usage/options and Git help; key-file-missing warning is expected until provisioned.
```sh
ssh -F /dev/null -p 22222 -i /tmp/ferryx-a2-key -o BatchMode=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/tmp/ferryx-a2-known-hosts -o ConnectTimeout=3 qa@127.0.0.1 'git -C /srv/ferryx-a2/repo worktree list --porcelain'
```
In the isolated packaged desktop: Settings > SSH > Add host (same endpoint/key) > Save > Test.
Close/reopen settings and desktop; host must persist. Import alias, delete it, reimport: stays deleted.
Add Project > Run on > that host > /srv/ferryx-a2/repo; create worktree slug a2-smoke from main.
In its real terminal run the following help-checked command; path must identify only the guest worktree:
```sh
git worktree list --porcelain
```
Disconnect only the disposable guest NIC using its hypervisor UI; pane shows Disconnected, not Exited.
Reconnect NIC then click Reconnect; original binding/PID and terminal output must resume without a new shell.
Rotate only fixture host key via guest provisioning UI, keep old trust file: Test must fail without trust mutation.
With empty fixture trust file: fail unknown-key check. Restore verified trust externally, Test succeeds.
Remove worktree through UI; dirty fixture files must block deletion. Local project terminal stays unaffected.
Future validators (declared ui/package.json:6-10; cargo test --help checked):
```sh
bun run --cwd ui test src/components/settings
bun run --cwd ui test src/components/ProjectDialogs.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::
```
SSH has no --help: it returned usage/exit 255; -G evaluated options without any connection or trust writes.

## Blockers / decisions to settle before implementation
- Approve manual helper prerequisite and minimum supported remote OS/architecture build matrix.
- Supply isolated SSH fixture + packaged QA desktop account; present commands are validation instructions,
  not evidence of working SSH, worktrees, reconnect or tests. Full helper CLI/package remains unimplemented.
- Confirm external host trust management for A2 (recommended); in-app trust enrollment is extra scope.
- Reserve shared-file owners before UI/persistence/PTY integration; current backend stubs alone cannot deliver A2.
