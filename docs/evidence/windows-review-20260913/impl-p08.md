# P08 implementation staging and execution handoff

Task st_01a09a01, root 01a0983f-c995-753d-afa9-593f6d118788. 2026-09-13.

Status: PARTIAL GREEN. Recursive upgrade admission inheritance in client.rs and Windows/UNC remote CWD alias preservation in server.rs repaired and verified.
RED: totalExit 101, both p08_upgrade_rpc_inherits_admission and p08_remote_cwd_aliases_preserve_relative_path failed as intended.
GREEN: totalExit 0, both tests passed.
Cargo slot released. Authentication/ACL and Windows native runtime checks remain open.

## Delivered staging

- Read repository/backend/daemon/terminal instructions, goal skill, repair packet, addendum, remaining register and source evidence.
- Checked current git status and scoped diff: daemon client/server were clean; foreign work remains untouched.
- Traced desktop handshake -> stale upgrade admission -> fresh internal client -> recursive handshake. Extracted actual internal-client construction to `upgrade_rpc_client`, preserving original fresh admission behavior for RED.
- Traced SSH root validation -> remote spawn -> relative config field. Existing validator accepts Windows case/slash aliases but server case-sensitive strip_prefix silently substitutes empty. Extracted existing conversion to `remote_spawn_relative_path`, unchanged for RED.
- Added two deterministic tests at those actual production seams. Neither opens sockets, spawns children, changes environment nor reads user profiles. Current assertions are intentionally expected to fail; that expectation is NOT executed RED evidence.

## Official registration

Called official executeAgentToolkit from `/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js`, bound resolveCwd to repo and resolveSessionId to root session. Successful `operation: steer`, `kind: revise_criterion` preserved entire existing C002 scenario and appended exact staging/commands/oracles. Tool receipt JSON has `ok: true`, `operation: steer`; full receipt: `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/pi-bash-5ab581e8dfbb21ed.log`.

## Exact execution request to lead

Grant one exclusive shared Darwin Cargo execution slot and relay it to P08. Execute only these registered commands in the lead-approved owned runner/profile (no broad daemon filter):

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::client::tests::p08_upgrade_rpc_inherits_admission -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::server::tests::p08_remote_cwd_aliases_preserve_relative_path -- --exact --nocapture
```

Each must discover exactly one test. Client oracle: original admitted client and actual internally constructed RPC client share admission Arc, and child cannot compare_exchange false->true. Server oracle: C:\Repo/c:\repo\src, slash aliases, UNC host/repo case aliases yield `src`, POSIX child yields `src` and same-root yields empty. Incidental compile failures and zero tests do not count. Capture actual RED before changing either helper, then identical GREEN. Both tests currently reside in existing Unix-gated modules, so these commands do not provide Windows coverage.

Smallest pending corrections after RED: share originating upgrade admission Arc with the internal RPC client; derive relative components with platform-correct equality and explicit containment failure rather than lossy prefix fallback. Register any signature/oracle extension before editing.

## Other P08 implementation prerequisites

Lead relay to P09/P12/P19 (and P22 QA owner): protocol/auth transport interface must be agreed before publishing changes to server/client/proxy/handovers or external callers. Current protocol handshake carries version only and TCP dispatch accepts commands without connection authentication. Windows token publication must validate SID/DACL, not rely on hidden ports. P23 owns remote/auth.rs; no edits to that writer were made. No wire change was made here.

Remaining approval-independent P08 work not yet delivered: authenticated actual control/attach/proxy listeners and native SID/DACL cases; hosted local authenticated agent-state TCP endpoint advertised only after listener readiness; PTY/terminal portable event-driven fixtures; full upgrade owned-listener integration and remote helper describe-cwd assertions. Native cases go exclusively to runtime owner st_01a099f8; this worker did not mutate maho-win. These are explicitly unfinished, not waived by the bounded seam tests.

## Verification and cleanup receipts

- `git diff --check -- src-tauri/src/daemon/client.rs src-tauri/src/daemon/server.rs`: no whitespace diagnostics.
- Scoped diff: client 19 changed lines, server 27 changed lines; rg confirmed both new production call sites and exact test names.
- LSP client: server cancelled request. LSP server: fresh diagnostics timed out at 3000ms. No clean diagnostics claim.
- Cargo tests/build/real surfaces: NOT RUN, no exclusive execution slot received. No RED/GREEN claim.
- No daemon connection/restart, process kill, shell/PTY launch, global env mutation, native remote mutation, dependency installation, branch/worktree operation, commit or push.
- No owned subprocess/socket/profile requires cleanup; no temporary runner was created. Source changes remain uncommitted in shared working tree.
