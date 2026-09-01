# SSH Remote Hosts

Orca-parity SSH remote machine support: add a remote machine as a host, browse its
worktrees, and open terminal panes that run `ssh -tt` into it — reusing the existing
PTY ring buffer, replay, native rendering, and warm-attach machinery unchanged.

## Usage

1. **Add a host**
   - Sidebar → "SSH Hosts" section → `+` (Add SSH host), or
   - Settings → SSH → **Add host**.
   - Import from `~/.ssh/config` (server-side read, ≤100 named aliases, duplicates of
     deleted hosts are tombstoned and never re-imported) or paste config text, or fill
     the manual form (label + hostname required; username, port, identity file, jump
     host optional).
2. **Browse remote worktrees** — expand a host row in the sidebar. The daemon runs
   `ssh <host> git worktree list --porcelain` via the blocking thread pool. Errors
   (host down, auth failure) render inline on the host row.
3. **Open a remote pane** — click a remote worktree row (or use the host itself for a
   plain remote shell). The daemon PTY spawns `ssh -tt [-p port] [-i identity]
   [-J jump] [user@]host` with the remote path as display metadata. Ring buffer
   replay, pane splitting, agent-state detection, and reconnect/resume behave exactly
   like local panes.
4. **Persistence** — hosts live in `ssh_hosts.json` under the app data dir (dev:
   `.../dev/ssh_hosts.json`). Remote pane bindings (`hostId` + remote path) persist in
   `session_state.json` schema **v3** (`sshByLeafId` per terminal tab and `ssh` per
   terminal session); v2 files load unchanged. Restored ssh panes respawn via the same
   startup argv; if a host was deleted, respawn is skipped with a warning.

## Config format

The importer understands standard `~/.ssh/config` subsets: `Host` (named aliases
only — glob patterns are ignored), `HostName`, `User`, `Port`, `IdentityFile`,
`ProxyJump`. The dedupe/tombstone key is `user@hostname:port` (port defaults to 22).

## Auth model

v1 delegates authentication to the system `ssh` process: keys via ssh-agent, encrypted
keys prompt inline inside the PTY. Connection probing uses
`ssh -o BatchMode=yes -o ConnectTimeout=2.5 user@host true`.

**Remote requirements (D5):** `git` must be installed on the host. Unix non-login
shells skip profile PATH setup, so a `git … not found` failure is retried once via
`sh -lc` automatically; if git is simply absent the exact remote error is surfaced
inline on the host row (e.g. an oracle-vps without git shows `git: not found`).

Host fields are validated at the argv boundary: hostname, username, identity file, and
jump host must never start with `-` (ssh option-injection guard, enforced in
`src-tauri/src/ssh/exec.rs::validate_host_argv_fields` and mirrored in the UI form).

## Security notes

- Remote worktree paths are display metadata only; the local root-jail
  (`canonicalize`/jail checks in `handle_spawn`) is skipped **only** for ssh startup
  spawns, never for local ones.
- No credentials are stored by Ferryx; the ssh process owns all auth.

## Agents on remote hosts (v1 limitation, D5)

v1 guarantees the remote **shell** only. Installing/running coding agents on the
remote host is the user's responsibility. Agent resume (`TerminalStartup::AgentResume`)
and ssh startup are mutually exclusive by construction: a pane spawns either a local
agent argv or an ssh argv.

## Implementation map

| Piece | Location |
|---|---|
| Config parse/import | `src-tauri/src/ssh/config.rs` |
| Argv builders + validation | `src-tauri/src/ssh/exec.rs` |
| Remote worktree ops | `src-tauri/src/ssh/worktree.rs` |
| IPC commands + host store | `src-tauri/src/ipc/ssh.rs` |
| Spawn argv branch | `src-tauri/src/terminal/shell.rs` (`TerminalStartup::Ssh`) |
| Host-bound spawn + metadata | `src-tauri/src/daemon/server.rs` (`handle_spawn`) |
| Session schema v3 | `src-tauri/src/session/mod.rs` |
| Host store/subscriptions | `ui/src/lib/sshHosts.ts` |
| Host dialog | `ui/src/components/SshHostDialog.tsx` |
| Sidebar section | `ui/src/components/RemoteHostsSection.tsx` |
| Settings section | `ui/src/components/settings/SshSection.tsx` |
| Spawn wiring / titles | `ui/src/state/workspaceStore.ts` (`openSshHostTerminal`) |
