# Rust SSH remote-host subsystem review — 2026-09-14

Scope reviewed: `src-tauri/src/ssh/` (direct, exec, runtime, operations, browse, worktree,
helper_setup, bridge, state_bridge) plus the two shipped consumers of the SSH transport,
`src-tauri/src/terminal/remote.rs` and `src-tauri/src/terminal/service.rs`, and the Tauri
command surface `src-tauri/src/ipc/ssh.rs`. 12 files opened, no source modified.

## Host key verification: no finding

Every shipped SSH invocation is built by `direct::ssh_plan`, which prepends
`StrictHostKeyChecking=yes` and `UpdateHostKeys=no` ahead of the user's own config
(`src-tauri/src/ssh/direct.rs:70-71`, applied by the `splice(0..0, ...)` at
`direct.rs:84-86`). All 12 `ssh_plan` call sites (helper_setup 147/298, operations
27/130/305/342, worktree 332/360/377, browse 82, state_bridge 79, direct 275) route
through that option block. No `StrictHostKeyChecking=no`, `accept-new`, or
`UserKnownHostsFile=/dev/null` exists anywhere under `src-tauri/src/ssh/`.

## Command injection: no finding

`SshHost` fields never reach a shell. `exec::append_options`/`append_target`
(`src-tauri/src/ssh/exec.rs:23-39`) push each value as its own argv element, and
`validate_host` (`direct.rs:13-40`) rejects leading `-` and any character outside a
strict allowlist. Remote *scripts* are built with `direct::quote_posix`
(`direct.rs:52-54`) or `runtime::powershell_data` (base64 round-trip,
`runtime.rs:37-42`) at every interpolation of a caller-controlled path, branch, or
host id, confirmed at `browse.rs:147`, `worktree.rs:229/262/295`,
`helper_setup.rs:115/284-286`, `operations.rs:118-128`.

---

### [P1] Auto-reconnect attempt counter is never reset after a successful reconnect, so a long-lived remote terminal permanently stops reconnecting

- Location: src-tauri/src/terminal/remote.rs:580
- Observed: `run()` initialises `let mut attempts = 0;` once per task
  (`remote.rs:580`), increments it on every transport failure
  (`attempts += 1;`, `remote.rs:702`), and gives up at
  `if attempts >= MAX_ATTEMPTS {` (`remote.rs:697`, `MAX_ATTEMPTS: u32 = 5` at
  `remote.rs:227`). The success path that marks the session live —
  `s.details.state = RemoteConnectionState::Connected;` (`remote.rs:615`) — sets
  `transport`, `pid`, `state` and `failure`, but never `attempts` and never the local
  counter. The six occurrences of `attempts` in the file are: 139 (field), 330
  (initial 0), 415 (reset inside the manual `retry()`), 580, 697, 702-703, 709 —
  none on the connect-success path.
- Why it is wrong: the budget is cumulative over the lifetime of the session, not per
  outage. A user on flaky Wi-Fi whose remote terminal drops and successfully
  reconnects five separate times over an afternoon hits the sixth blip and the
  session is set to `Disconnected` with the retry loop dead, even though every
  previous reconnect worked within one attempt. The exponential backoff is driven by
  the same counter (`connector.delay(attempts - 1)`, `remote.rs:709`,
  `250ms << attempt`), so the 5th blip of an otherwise healthy session waits 4
  seconds before its first dial instead of 250 ms. The user-visible `attempts` field
  (`remote.rs:139`) likewise keeps climbing and never reads 0 for a healthy
  reconnected session. Recovery requires an explicit user `retry()`, the one path
  that does reset (`remote.rs:415`).
- Minimal fix: reset both counters when the connection is established, inside the
  block that already holds the control gate and the state lock at
  `remote.rs:607-618`: add `attempts = 0;` and `s.details.attempts = 0;` next to
  `s.details.state = RemoteConnectionState::Connected;`.

### [P2] Closing a remote terminal while the host is unreachable leaks the session entry and its output-hub registration forever

- Location: src-tauri/src/terminal/remote.rs:521
- Observed: `RemoteRuntime::close` (`remote.rs:499`) aborts the run task, bumps the
  generation and takes the transport, then tries `client.stop(&d.target).await`. On
  failure it runs `if let Err(error) = result {` (`remote.rs:521`), records the
  failure, sets `Disconnected`, and does `return Err(f);` — returning *before*
  `self.sessions.lock().remove(id);` and `self.hub.remove_session(id);`
  (`remote.rs:529-530`), which are only reached on the success path.
- Why it is wrong: the user closed the tab, so nothing will call `close` again, but
  the entry stays in the `sessions` map with its run task aborted and
  `transport: None` — an inert zombie that never reconnects and never drains. Its
  `TerminalOutputHub` registration also survives, so `hub.has_session(id)` keeps
  returning true and every buffered byte of that session's scrollback stays resident
  for the life of the process. Because `insert` rejects duplicates on both
  `backend_session_id` and target (`remote.rs:313-321`), the leaked entry also
  permanently reserves that identity. Closing a handful of remote terminals during a
  VPN outage leaks all of them.
- Minimal fix: in the error branch at `remote.rs:521-528`, drop the registration
  before returning — call `self.sessions.lock().remove(id);` and
  `self.hub.remove_session(id);` alongside the failure bookkeeping, since the user's
  close intent is unconditional and the remote PTY is already unreachable.

### [P2] `git worktree list --porcelain` is run through the 16 KiB output cap with no remote-side truncation, so worktree listing hard-fails on large repos

- Location: src-tauri/src/ssh/worktree.rs:333
- Observed: `list_remote` runs the porcelain listing through
  `direct::bounded_output(&plan, Duration::from_secs(30))` (`worktree.rs:333`).
  `collect_output` enforces `stream.take(16385).read_to_end(&mut bytes).await?` and
  then `return Err(std::io::Error::other("SSH output exceeds 16 KiB"));`
  (`direct.rs:334-336`). The generated script does no bounding at all: it captures
  the whole command output with `out=$(git -C {quoted_root} worktree list
  --porcelain 2>&1)` and prints it verbatim (`worktree.rs:230`). By contrast the
  directory browser deliberately truncates on the remote side to stay under the same
  cap — see the `bytes -gt 6000` break and the comment "Entry bytes plus two
  4096-byte paths fit the transport's existing 16 KiB cap" at `browse.rs:125-140`.
- Why it is wrong: each porcelain record is roughly `worktree <path>` + `HEAD <40
  hex>` + `branch refs/heads/<name>` ≈ 120-160 bytes, and Ferryx itself creates one
  worktree per agent workspace under `.orca-worktrees/wt-ssh-<slug>`
  (`worktree.rs:95`). Somewhere around 110-130 worktrees the listing exceeds the cap
  and the operator gets a flat failure tagged `worktree-list` whose message is
  "SSH probe output failed: SSH output exceeds 16 KiB" — not a truncation notice, and
  with no partial list. The remote worktree panel stops working entirely on exactly
  the heavy-usage repositories it exists for, and the error text points at the SSH
  transport rather than at the real cause.
- Minimal fix: bound the payload remotely the way `browse.rs` already does — pipe the
  porcelain output through a head-style byte limit in the generated script and emit a
  truncation flag field alongside the marker, or raise the read budget for this one
  call by giving `list_remote` a dedicated collector with a larger cap.

### [P3] Three SSH entry points are dead code, and one of them is the only unhardened `ssh` argv builder in the tree

- Location: src-tauri/src/ssh/exec.rs:3
- Observed: `pub fn probe_argv(host: &SshHost) -> Vec<String>` (`exec.rs:3`) emits
  `ssh -o BatchMode=yes -o ConnectTimeout=2.5 [-p/-i/-J] <target>` — notably without
  the `StrictHostKeyChecking=yes` / `UpdateHostKeys=no` pair that `ssh_plan` adds.
  A repository-wide search for `probe_argv` outside `docs/` returns only its own
  definition and its three tests in the same file; the live probe path
  `cmd_ssh_test_connection` (`src-tauri/src/ipc/ssh.rs:304`) calls
  `crate::ssh::runtime::detect`, which goes through `ssh_plan`. Likewise
  `install_remote_extension_script()` (`direct.rs:147`) has exactly one caller,
  `direct_tests.rs:295`, having been superseded by `POSIX_INTEGRATION_SCRIPT`
  (`operations.rs:172`); and `StateBridge` (`state_bridge.rs:15`) plus the
  `state_endpoint` parameter threaded through `terminal/service.rs:110` into
  `operations.rs:118` is reached only from `src-tauri/tests/ssh_windows_live.rs` —
  every non-test caller of `spawn_ssh` is absent, so the Windows agent-state branch
  at `operations.rs:118-124` never executes in a shipped build.
- Why it is wrong: no user hits any of this today, so it is not a security bug. It is
  a trap: `probe_argv` is a ready-made, plausibly-named argv builder that silently
  omits host-key pinning, and the next person wiring a "quick reachability check"
  will reach for it and reintroduce a real P0. The unused `state_endpoint` plumbing
  similarly reads as a supported Windows feature that is not connected to anything.
- Minimal fix: delete `probe_argv` and `install_remote_extension_script` with their
  tests; if `probe_argv` is kept for a planned caller, build it by delegating to
  `ssh_plan` so the hardening block cannot be bypassed.

Summary: P0: 0, P1: 1, P2: 2, P3: 1
