# Herdr machine parity — live verification, 2026-09-15

Supersedes the "NOT met" status recorded in `AC-LEDGER.md` and `FINAL-REPORT.md` for the
ACs listed below. Every claim here was produced by driving the real desktop daemon's UDS
socket (`/tmp/rorca-501-dev/daemon.sock`) against a real Linux host (omaki) through the
public relay `https://relay.checka.cc`. No fixtures, no mocks, no unit-test-only evidence.

## Defects found and fixed

1. **Relay control channel keyed only by bearer token.** `GET /host/{uuid}/api/v1/...`
   returned 404 after any daemon restart, because the channel was registered under the
   machine token while callers address the machine UUID. The client now sends
   `x-ferryx-machine-id` on every control connect and the relay binds that UUID as a
   channel alias.

2. **`managedWorktreesV1` never advertised.** The client requires it for all three
   worktree operations; the routes and handlers were already served, but the capability
   list omitted it, so every remote worktree call failed
   `PAIRED_HOST_CAPABILITY_UNAVAILABLE`. `machineWorkspaceV1` was missing for the same
   reason and blocked project registration.

3. **Relay query allowlist had no `sessions` entry.** `validate_http_query` matched
   literal path strings, so the sessions allowlist was empty and `workspaceId` /
   `daemonEpoch` were rejected with 400, surfacing as `PAIRED_HOST_INVALID_RESPONSE`.
   Now a structural match: `["sessions"]` and `["sessions", _]` each accept their own
   parameters.

4. **`Inventory::list` defaulted `online` to false** and `set_online` had no production
   caller, so a paired machine always rendered offline.

5. **The gateway ignored `workspaceId` when listing sessions.** `list_sessions` took no
   `Uri`, so the query string was unreachable and a workspace-scoped list returned every
   session on the host, including other workspaces'. The client correctly rejected the
   mismatch as `PAIRED_HOST_INVALID_RESPONSE`. The list is now filtered by workspace.

## AC status changes

- **AC05 (remote worktree create/delete) — MET.** Create returned `managed: true` on
  branch `orca/<ws>/<slug>`; `git worktree list` on the host showed it appear, and after
  delete both the worktree and the branch were gone and `.orca-worktrees/<ws>/` was empty.
- **AC06 (remote session attribution) — MET.** A session created with a worktree
  identity reported `cwd` inside that worktree and the correct `worktree.wsId`.
- **AC08 (forced-relay functional pass) — MET.** A live PTY was created over the relay
  (`running: true`) and the fixture daemon owned a real `/usr/bin/bash -l` child process.
  `pairedDaemonProxyV1` is now advertised true, which this evidence earns.
- **AC07 (session survival across relay loss) — MET for the relay-loss case.** With a
  live session open, `systemctl restart ferryx-relay` left the PTY PID byte-identical
  (164639 before and after); afterwards the session still resolved through the detail
  endpoint as `running` and appeared in the workspace-scoped list. Per the session
  continuity rule, PID survival alone was not accepted: functional reattach through the
  canonical endpoints was confirmed.

## Still user-only

AC04 (native tabs, splits, focus, shortcuts, menus), the remaining AC07 desktop cases
(host switch, settings, renderer close), and AC12 (Linux/Windows frozen-backend
rehearsals) need `bun tauri dev` on the user's desktop. OS automation of the user's
desktop is prohibited, so no agent can close these.

## Traps worth remembering

- `INVALID_PATH` from `createSession` is the catch-all `_ =>` arm in
  `session_service.rs`; it masks the real spawn error. Two apparent product bugs were in
  fact test-fixture errors: registering a non-git directory, and passing the internal
  `daemon:<hash>` id where the remote API expects `remoteWorkspaceId` (`project-<hash>`).
- Running the lib test binary directly fails with
  `Library not loaded: @rpath/libghostty-vt.dylib`; use `cargo test --lib`, and never
  `--all-targets` while a live app shares the daemon socket.

## Verification commands

    cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1   # 1185 passed, exit 0
    bunx vitest run --maxWorkers=1 <paired-host suites>                          # 162 passed, exit 0
    bun run --cwd ui build                                                       # exit 0
