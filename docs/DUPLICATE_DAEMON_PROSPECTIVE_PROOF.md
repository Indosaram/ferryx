# Duplicate-daemon prospective proof

Task: `st_01a09aeb`; date: 2026-09-13.

## Final lead-run result: prospective proof passed

The parent session resumed execution with its native monitor after the child
reported the interface blocker below. It ran baseline GREEN, the exact
LOCK_EX-to-LOCK_SH mutation RED, restored the one expression with apply_patch,
and ran restored GREEN. No permanent production or test change remains.

All three executions used:

```sh
cd /Users/indo/code/project/orca-lite-wt/sa-tests
export TMPDIR="$PWD/.dup-tmp" TMP="$PWD/.dup-tmp" TEMP="$PWD/.dup-tmp"
export CARGO_TARGET_DIR="$PWD/target"
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_duplicate_prevention -- --nocapture
```

The lead created `.dup-tmp` before baseline and removed it with `rmdir` after
GREEN. Full unfiltered tool log text, including compiler warnings and assertion
diagnostics, is retained in `duplicate-daemon-prospective-logs/baseline.log`,
`red.log`, and `green.log`. PTY progress artifacts and command-width truncation
are preserved as returned by the tool; these are not raw byte stream archives.

Execution identities and exit codes:

- Baseline: `mon_PNZ6K8624PMF4B5J`, `bash_311`, exit 0.
- Mutation: `mon_AT588TAVBSBBP2E3`, `bash_312`, exit 101.
- Restored: `mon_YQP64NT2V25CSYPZ`, `bash_313`, exit 0.

Exact RED evidence:

```text
isolated runtime=/Users/indo/code/project/orca-lite-wt/sa-tests/.dup-tmp/fx-dup-scNIxg/run data=/Users/indo/code/project/orca-lite-wt/sa-tests/.dup-tmp/fx-dup-scNIxg/data
spawned isolated daemon pid=35560
spawned isolated daemon pid=35574
reaped isolated daemon pid=35574 status=exit status: 0
reaped isolated daemon pid=35560 status=exit status: 0
cleanup verified: children reaped and /Users/indo/code/project/orca-lite-wt/sa-tests/.dup-tmp/fx-dup-scNIxg removed
thread 'second_daemon_refuses_already_locked_socket_directory' (4732262) panicked at tests/daemon_duplicate_prevention.rs:209:5:
duplicate daemon reached readiness while owner held both locks
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.94s
DUP_MUTATION_RED_EXIT=101
```

Exact restored GREEN and cleanup:

```text
spawned isolated daemon pid=36305
spawned isolated daemon pid=36322
Ferryx daemon error: Another daemon instance is already holding the lock.
reaped isolated daemon pid=36322 status=exit status: 1
reaped isolated daemon pid=36305 status=exit status: 0
cleanup verified: children reaped and /Users/indo/code/project/orca-lite-wt/sa-tests/.dup-tmp/fx-dup-aBUMwr removed
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.09s
DUP_RESTORED_GREEN_EXIT=0
DUP_TEMP_PARENT_CLEANUP_EXIT=0
```

After restoration, both SHA-256 values matched the preflight hashes below.
`git diff --exit-code` for the production and test files returned 0. A final
process query listed none of the six fixture PIDs (34889, 34901, 35560, 35574,
36305, 36322). The fixture root parent was removed; no fixture-owned socket or
port remains after all child processes were reaped. No tmux session was used.

The application daemon PID 1010 and the separate worktree daemon PID 21591
retained their observed start times before and after this run. Neither was
signalled or connected to. Earlier parent statements that PID 21591 was the
only current live daemon were incomplete: the direct `ps -p 1010,21591`
observations show both. Neither PID is the original baseline PID 36170.

Restoration triggered an LSP diagnostic timeout, not a source error; the restored
Cargo test compiled and passed. Existing 16 compiler warnings remain in all logs.
No commits were created.

This completes fresh duplicate-daemon mutation evidence only. It does not recover
the historical missing output, repair the original unchanged-PID criterion, or
replace implementation-lane pre-production RED evidence.

## Historical child verdict before parent execution

**INCOMPLETE: no prospective RED or GREEN was run.** The child tool interface
provides `functions.bash`, file operations, and LSP operations, but no native
monitor or asynchronous command-monitor tool. The task explicitly requires a
native monitor for long commands. This is an execution-interface blocker, not
a test failure. No background-runner substitute was created.

This report does not recover historical missing output, repair or waive original
C5, or establish failing-first history. The missing original assertion and
cleanup evidence described in `TEST_EVIDENCE_RECOVERY.md` remain missing.

## Read-only preflight

Worktree: `/Users/indo/code/project/orca-lite-wt/sa-tests` (sole worktree used).
Read: `docs/TEST_EVIDENCE_RECOVERY.md`, the complete
`src-tauri/tests/daemon_duplicate_prevention.rs`, `src-tauri/Cargo.toml`, and
the relevant lock acquisition, path-resolution, and listener startup code in
`src-tauri/src/daemon/server.rs`.

Source inspection establishes that the existing fixture clears child environment,
sets runtime/data/session/home/temp roots beneath a fresh tempfile directory,
retains the owner's PID-checked connection, captures the contender's PID-checked
connection when it reaches readiness, and attempts protocol cleanup before its
outcome assertion. These are source observations, not new runtime receipts.
The shared locking helper serves both persistent and runtime locks; therefore
the proposed mutation tests their combined protection, not each independently.

Preflight source SHA-256:

```text
4e15ab78d29bdb333de14d7d9cd4e6e1f60b091b455cec453c3cf844103f6e9a  src-tauri/src/daemon/server.rs
78bc2237187238bb0dfc3656cf9153ffcf4b6b57342fdac53e3e535b770b0c44  src-tauri/tests/daemon_duplicate_prevention.rs
```

Initial read-only process listing identified the live application daemon as
PID 1010, start time `Sun Sep 13 07:55:43 2026`, command
`/Applications/Ferryx.app/Contents/MacOS/ferryx --daemon`.
It also identified an unrelated worktree daemon PID 21591; neither was touched.

## Captured commands and cleanup scope

`duplicate-daemon-prospective-logs/preflight.log` captures shell-traced commands,
unfiltered outputs of status/diff/hash checks, and read-only before/after process
checks with exit codes. Initial status before this report showed an unrelated
modified UI report and untracked scope/recovery/probe files plus `ui/node_modules`;
these were left alone. Production and test diffs were empty.

No production/test edits, mutation, fixture, test child, environment override,
build, commit, signal, daemon connection, or daemon launch was performed.
Consequently there is no restoration receipt: matching hashes only demonstrate
unchanged source at the two observations. No temporary debugging artifacts need
removal. Only this report and its requested evidence log are retained.

## Outstanding evidence

All execution requirements remain outstanding: worktree-local TMPDIR/TMP/TEMP
and CARGO_TARGET_DIR setup; fresh fixture isolation output; exact one-expression
LOCK_EX-to-LOCK_SH mutation; complete `--nocapture` RED with the diagnostic
`duplicate daemon reached readiness while owner held both locks`, both reaping
lines and fixture-removal receipt; exact inverse apply_patch restoration with
pre/post hash equality; restored GREEN exit 0; final fixture-child/directory
absence checks. No cargo exit code is claimed.

The required command, **not executed**, is:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_duplicate_prevention -- --nocapture
```

The lead must run this prospective proof from a session exposing the required
native monitor. Process-list equality is only point-in-time identity evidence,
not proof of absence of transient access; environment isolation is not an OS
sandbox. No claim about live daemon data-content integrity is made.
