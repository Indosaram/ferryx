# P25 implementation evidence - st_01a09a0b

Status: **partial implementation, not packet or Windows acceptance**. No whole-objective completion is claimed. Changes are uncommitted in the shared working tree.

## Registration and ownership

Read root AGENTS.md, programming skill, packet/addendum/register, gap-tooling.md,
remaining-powershell-wrappers.md, original goals and current source. Initial scoped
files were clean; foreign changes including P22 `script/qa/win-daemon-e2e.mjs`
were preserved. Used official `executeAgentToolkit` with repo resolveCwd and
parent resolveSessionId `01a0983f-c995-753d-afa9-593f6d118788`, operation steer,
kind revise_criterion, appending full existing C002 scenario. The registration
receipt is `p25-logs/registration.json`; goals.json contains the initial
`P25 st_01a09a0b pre-repair registration` and VM invocation correction.

## Delivered source changes

- All five SSH/resume drivers are import-safe, export main, and reject the
  previously unsafe `--self-test` spelling before live execution. The safe
  contract invocation is below; rejecting `--self-test` is not a test pass.
- Bridge marker reads accumulate bytes across cursor-based reads, preserve
  fragmented UTF-8, accept CRLF, require newline-complete nonce markers, reject
  replay gap/EOF/PID mismatch, and bound accumulated bytes to 1 MiB.
- Detached setup/bridge no longer signal numeric endpoint PIDs. They fail with
  `QA_OWNERSHIP_PREREQUISITE` before any fixture or SSH operation. Their retained
  cleanup code propagates failures and keeps evidence instead of falsely
  reporting removal. **This is a safety interlock, not a functioning detached
  startup/survival repair.** No bypass flag is supplied. Runtime-owned process
  handle/start identity support is still required to reopen these entry points.
- Direct helper survival verifies endpoint PID against the owned daemon handle
  before first RPC, verifies helper v1 host/owner/epoch and repeats identity on
  reconnect/cleanup. Failed child/remote stop retains the fixture and throws an
  AggregateError; removal follows absence checks rather than preceding them.
- Process survival performs daemon v3 handshake on every socket, verifies owned
  live child PID and canonical executable and pins epoch until deliberate
  restart. Mismatch destroys socket before any subsequent request.
- Resume-CWD verifies v3 PID/executable/epoch before workspace registration or
  session cleanup, explicitly isolates FERRYX_DATA_DIR, awaits receiver closure,
  and retains root if session cleanup is unproved.
- Stager no longer discovers GNU/debug fallback candidates or preserves stale
  resources as evidence. Explicit complete target/profile/hash/source-lock
  receipts are required. It checks PE/ELF machine, stages into a private sibling
  directory, rechecks copied bytes, and publishes by rename only after success.
  Existing destination is rejected, not overwritten. Helper protocol remains 1;
  version is read from Cargo.toml. Debug staging requires explicit profile=debug.
  Receipt inputs are assertions from the build owner, **not independent compiler
  provenance**. Full source closure and PE dependency/import validation remain
  P10/P18 native artifact prerequisites; PE machine validation alone proves no
  loadability or GNU-vs-MSVC toolchain origin.

## Actual RED/GREEN evidence

Invocation (both files, no skipped tests):

```
node --experimental-vm-modules --test scripts/qa/ssh-harness-safety.test.mjs scripts/build-remote-helpers.test.mjs
```

Tests evaluate real driver source in a VM with host effects denied, or invoke
actual extracted driver functions with controlled boundary responses. This is
source-boundary execution, **not real framed/socket integration or native
survival acceptance**. No sleeps/polling success in added tests.

All logs are under `docs/evidence/windows-review-20260913/p25-logs/`:

| Log | Observed result |
| --- | --- |
| harness-red.log | 8 failures: five import effects, two unsafe PID paths, fragmented INIT assertion |
| stager-red.log | 10 failures against actual old stager redirected to disposable roots; empty/fallback/stale publication and ignored receipts |
| initial-green.log | same initial 18 cases pass |
| detached-red.log | 2 intended missing preflight ownership rejections, 8 pass |
| identity-red.log | 2 intended missing handshake ownership rejections, 10 pass |
| identity-green.log | 12 pass |
| helper-red.log | 2 intended endpoint/cleanup failures, 12 pass |
| combined-green.log | 24 pass |
| detached-cleanup-red.log | 2 intended swallowed-cleanup failures, 14 pass |
| crlf-red.log | CRLF marker read beyond final available fragment, 16 pass |
| final-green.log | final combined invocation receipt; see exact final counts in log |

Real runnable entry checks: `bun scripts/qa/ssh-bridge-survival.mjs`,
`bun scripts/qa/ssh-helper-setup.mjs`, and `bun scripts/build-remote-helpers.mjs`
each exited 1 at the explicit missing-ownership/receipt preflight (entry logs).
These are expected safety negatives, not behavioral RED or live success.
No helper binary, Cargo build/test, SSH, desktop, release/install, remote write,
branch/worktree, commit or daemon action was run.

## Narrowly outstanding work and runtime-owner relay

**Parent must relay `scripts/qa/p25-native-handoff.md` to st_01a099f8.** No child
send/monitor tool or PowerShell interpreter is available here. Staged command:

```
powershell.exe -NoProfile -File scripts/qa/helper-wrapper-contracts.ps1
```

It executes actual wrappers with tar/cargo/bun boundary functions and owned
random fixtures; it builds/launches nothing. Require intended archive-preservation
RED before wrapper production changes, then identical GREEN. Both existing
PowerShell wrappers remain unchanged and unsafe for real input archives.
After native RED the minimal wrapper repair is:

1. Remove caller Archive deletion from both finally blocks.
2. Pass explicit owned `--target-dir (Join-Path $fixture 'target')` to every Cargo
   command; preserve inherited CARGO_TARGET_DIR unchanged.
3. Capture cargo test output and require a parsed `test result: ok. N passed`
   with N > 0 in addition to exit 0; zero-match means incomplete.
4. Build with --message-format=json, select exactly one non-test helper bin
   compiler-artifact executable inside the owned target tree, verify it exists,
   assign FERRYX_QA_HELPER_BINARY only for the child, restore in finally.
5. Mark survival launch pending before invoking bun, clear only on exit 0;
   retain extracted/build fixture on failure. Do not infer child exit from
   printed cleanup text or remove its evidence.

Still missing locally (not disguised as native prerequisites): real framed/socket
peer tests for transport EOF/late reply/timeout poisoning; complete success and
failure matrix for ownership and removal; independent full source/build receipt
closure and injected copy failure; functioning detached process ownership and
same-host SSH proof; PowerShell apostrophe quoting repair. The safety interlock
prevents these incomplete detached paths from running but does not implement
those features. Direct helper/Unix process/resume live paths remain unexecuted.

## Diagnostics and cleanup

All eight changed JS source/test files were submitted to LSP. Final changed-file
results and node syntax checks are in diagnostics.log. PowerShell LSP is not
configured, and neither powershell.exe nor pwsh is installed: PS syntax/runtime
is unverified, not green. No application build applies to these standalone
scripts; no Cargo slot was requested or used. Owned stager roots are removed by
test teardown; final temp-root inventory finds no p25-stage-* directories.
Foreign work is preserved; no shared resources manifest was staged. Final
scoped git diff --check and JS node --check are recorded in validation.log.
