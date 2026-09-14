# Review: lead-owned findings (browser CLI, worktree IPC, UI test harness)

Scope: src-tauri/src/ipc/browser_cli.rs, src-tauri/src/ipc/worktree.rs, ui/vitest.config.ts,
ui/src/remote/input-latency-soak/, plus triage of the 8 failing Rust library tests and the
1 failing UI test file captured at HEAD 48ee5a9c.
Reviewed-at: 2026-09-14
Reviewer: lead session (not a dag lane)

## Findings

### [P0] Browser CLI control socket accepted every local peer without authentication

- Location: `src-tauri/src/ipc/browser_cli.rs:195-237` (`start_browser_cli_server_at_path`,
  `cfg(not(unix))`), `src-tauri/src/ipc/browser_cli.rs:299-341` (`handle_connection`),
  `src-tauri/src/ipc/browser_cli.rs:343-370` (`execute_request`)
- Observed: the non-unix server bound `TcpListener::bind("127.0.0.1:0")` and wrote the chosen
  port to a plaintext port file. `handle_connection` read one JSON line and called
  `execute_request` directly. There was no token, no peer-credential check, and no capability
  of any kind anywhere in that path. On unix the only access control was filesystem mode:
  runtime dir `0o700` and socket `0o600` (`browser_cli.rs:143-162`).
- Why it is wrong: on Windows any local process can read the port file and then `list` every
  browser session with its URL, title and workspace, `snapshot` page content, and `act` to
  click, fill and send keystrokes inside the user's *authenticated* browser profile. That is
  full cross-origin credentialed control of the user's logged-in sessions by any unprivileged
  local program. On unix the socket mode narrows this to same-uid processes, which still means
  every program the user runs, not just Ferryx.
- Falsification: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli`.
  Two tests committed in `a0d76b20` already encoded this contract and FAILED at HEAD:
  `p12_tcp_rejects_unauthenticated_commands` (`browser_cli.rs:546`) panicked
  `unauthenticated {"command":"list"} reached dispatch: List { sessions: [... p12-private ...] }`,
  and `p12_tcp_rejects_forged_credential` (`browser_cli.rs:562`) panicked
  `forged credential reached dispatch: ...`. The failure text itself discloses the private URL
  the socket should never have surrendered.
- RED capture: `/tmp/ulw-massreview/baseline-cargo-lib.log` (HEAD 48ee5a9c, before any edit).
- Minimal fix (applied): per-server-start capability token.
  - `BrowserCliEnvelope { token, #[serde(flatten)] request }` is now the wire shape.
  - `generate_browser_cli_token()` draws 32 bytes from `rand::rngs::OsRng`, hex-encoded.
  - `write_token_file()` persists it beside the socket/port file as `<name>.token`, mode `0600`
    on unix, replacing any pre-existing entry rather than following it.
  - `tokens_match()` compares length first, then accumulates an XOR difference, so a matching
    prefix does not leak through timing.
  - `handle_connection` authorizes BEFORE interpreting the command, so an unauthorized peer
    learns nothing about which commands exist or whether its arguments named a real session.
  - Both server paths (unix + non-unix) mint and persist the token; both client paths
    (`send_browser_cli_request_at_path`) read and send it.
- Test seam: `ipc::browser_cli::tests`. The two pre-existing rejection tests are the RED.
  Added `p12_tcp_accepts_the_capability_token` so rejection cannot pass trivially by breaking
  the envelope for everyone, and `browser_cli_tokens_match_only_on_exact_equality` covering
  length, exact match, truncation, empty and non-collision. The two
  `browser_cli_server_starts_without_tokio_reactor` variants now read back the token the real
  server minted, and the unix variant asserts the token file is mode `0600`.

### [P1] Worktree deletion panicked when the disk-scan cache was unmanaged

- Location: `src-tauri/src/ipc/worktree.rs:174-180`
- Observed: the delete path called
  `app.state::<crate::ipc::worktree_disk::WorktreeDiskScans>()` unconditionally. Tauri's
  `state()` panics when the type was never `manage()`d
  (`tauri-2.11.5/src/lib.rs:734`).
- Why it is wrong: worktree deletion aborts the entire command with an unrecoverable panic
  instead of a typed `IpcError` on any host that did not manage the scan cache. Production
  survives only because `src-tauri/src/lib.rs:1048` happens to manage it; the command carries
  no such guarantee itself. Every other optional-state call site in this crate already uses
  the fallible form — `ipc/agents.rs:45`, `ipc/terminal.rs:745`, `:751`, `:770` — so this line
  was the lone outlier.
- Falsification: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::tests`.
  `tauri_mock_worktree_commands_use_identity_contract` panicked
  `state() called before manage() for ferryx_lib::ipc::worktree_disk::WorktreeDiskScans`.
- RED capture: `/tmp/ulw-massreview/baseline-cargo-lib.log`.
- Minimal fix (applied): `try_state` plus a nested `if let`, emitting the progress event only
  when the cache exists.
- Test seam: `ipc::tests::tauri_mock_worktree_commands_use_identity_contract` — an existing
  test that builds a mock app without the scan cache, which is exactly the unmanaged case.

### [P1] The UI vitest suite could never be green, so its exit code carried no signal

- Location: `ui/vitest.config.ts` (`test` block; `exclude` was unset),
  `ui/src/remote/input-latency-soak/soak.test.mjs:1`
- Observed: with no `test.exclude`, vitest's default include swept up
  `src/remote/input-latency-soak/soak.test.mjs`, whose first line is
  `import { describe, expect, test } from "bun:test"`. Vite externalizes `bun:test` for the
  jsdom environment, so the file failed on every run with
  `Module "bun:test" has been externalized for browser compatibility.`
- Why it is wrong: the suite exited non-zero regardless of product health, so a genuine
  regression was indistinguishable from this permanent failure and nobody could gate on
  `bun run --cwd ui test`. `ui/src/remote/input-latency-soak/README.md` documents the harness
  as an opt-in Bun measurement run with
  `bun test ./ui/src/remote/input-latency-soak/soak.test.mjs` — it was never meant to be part
  of the vitest suite.
- Falsification: `bun run --cwd ui test`.
- RED capture: `/tmp/ulw-massreview/baseline-ui-vitest.log:341` —
  `FAIL src/remote/input-latency-soak/soak.test.mjs`, totals
  `Test Files 1 failed | 225 passed (226)`, `Tests 2577 passed (2577)`, exit 1.
- Minimal fix (applied): `ui/vitest.config.ts` `test.exclude` now lists
  `**/node_modules/**`, `**/dist/**` and `src/remote/input-latency-soak/**`, keeping vitest's
  defaults explicit while releasing the bun-only harness. The harness still runs under its
  documented bun command.
- Test seam: the suite's own exit code. No new test — adding one that asserts the config
  excludes a path would restate the config rather than prove behavior.

### [P2] Two integration test targets do not compile at HEAD

- Location: `src-tauri/tests/ssh_project_identity_live.rs:46-49`,
  `src-tauri/tests/scoped_design.rs:30`, `:57`, `src-tauri/src/ferryx_scope/design/mod.rs:61`
- Observed: `cargo check --lib --tests` fails with `E0609: no field 0 on type ProjectProbe`
  (four sites) and three `E0308: mismatched types` around
  `reader.output_buffer_size()`.
- Why it is wrong: these targets cannot build, so whatever they assert is unenforced. They are
  invisible to `cargo test --lib`, which is the command this project's AGENTS.md prescribes,
  so the breakage persists unnoticed.
- Falsification: `cargo check --manifest-path src-tauri/Cargo.toml --lib --tests`.
  Capture: `/tmp/ulw-massreview/browsercli-check.log:152-251`.
- Confirmed pre-existing, not caused by this session: `git diff --stat HEAD -- src-tauri/tests/`
  is empty.
- Minimal fix: out of scope for this pass; recorded as deferred so it is not mistaken for
  collateral from the fixes above.
- Test seam: the targets themselves once they compile.

### [P3] Remaining HEAD library-test failures are environment-bound, not defects fixed here

- `daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty` — fails at
  `src-tauri/src/daemon/remote_ssh_gateway_qa.rs:39` asserting the terminal WebSocket upgrade
  is `403` without an active desktop selection, but observing `101`. This is either a lost
  session-lock gate or a fixture race and needs the remote lane's verdict; the test also spawns
  a real `sshd` and emits BSM audit noise, so it is environment-heavy.
- `dag::watcher::tests::test_dag_scan_runs_off_async_worker` (`src/dag/watcher.rs:382`,
  `async sentinel: SendError`) and
  `dag::watcher::tests::test_dag_watcher_recovers_after_silent_watch_loss`
  (`src/dag/watcher.rs:332`, `Elapsed`) — both are timing/channel-bound.
- `remote::relay_server::tests::test_relay_key_store_transaction_is_cross_process` with its
  child `test_relay_transaction_lock_probe_child` (`src/remote/relay_server.rs:1894`, `:1914`)
  — the probe re-execs the test binary; the child assertion is the one that fails.
- `tests::app_remote_state_persists_pairing_but_starts_off` (`src/lib.rs:1793`) — fails with
  `Timed out waiting for relay registration ACK`, requiring a reachable relay.

## Summary

- P0: 1 (fixed)
- P1: 2 (both fixed)
- P2: 1 (deferred, pre-existing, recorded with reason)
- P3: 4 pre-existing test failures triaged, not fixed in this pass

## Prior art: the P0 was known and left open across two passes

`docs/BUILTIN_BROWSER_CODE_REVIEW_2026-09-07.md` reported this exact hole as finding **F9**,
one week before this session:

> on Windows the fallback is a `127.0.0.1` TCP listener whose port is written to a plain file
> — **any local process of any user on the machine can connect and drive automation**
> (snapshot page content, click, fill, keypress) with no token. The port file itself is not
> permission-restricted.

What happened after that report:

1. It was ranked **priority 5 of 6**, framed as "before Windows ships this CLI surface, the
   loopback TCP listener needs a token". That review explicitly modified no source file.
2. The companion request in the same finding — a cap on the unbounded `read_line` — **was**
   implemented (`MAX_REQUEST_BYTES`, `src-tauri/src/ipc/browser_cli.rs:242-246`). The token
   was not.
3. Commit `a0d76b20` (2026-09-13) then added `p12_tcp_rejects_unauthenticated_commands` and
   `p12_tcp_rejects_forged_credential`, encoding the token contract as tests — still without
   the production change. That left two permanently-failing security tests in the tree, where
   they became indistinguishable from ambient baseline noise.

**Severity correction.** F9's priority-5 ranking rested on the premise that Windows had not
shipped yet. That gate has passed: `scripts/release-local.mjs:219` builds `windows-x86_64`
and `scripts/build-msix.ps1` packages it. The condition the deferral was waiting on is gone,
so P0 is the correct severity today.

The fix landed in this session is the first production closure of F9.
