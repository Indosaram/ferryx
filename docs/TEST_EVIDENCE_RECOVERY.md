# Test evidence recovery - 2026-09-13

## Verdict and provenance

The original duplicate-daemon mutation command and its filtered output were recovered.
**The decisive assertion diagnostic and mutation cleanup outcome remain unrecovered.**
No historical receipt was recreated, and no tests, builds, or daemons were run in this audit.

Source S (original local JSONL, not a report quotation):

```text
/Users/indo/.omo/sessions/--Users-indo-code-project-orca-lite--/2026-09-12T12-29-35-992Z_01a09598-3778-7aff-83ff-6f8031c71b9d.jsonl
```

Session: `01a09598-3778-7aff-83ff-6f8031c71b9d`. Source identities below use
JSONL line numbers plus event IDs/timestamps so later file growth does not obscure them.

## Original command, RED, restoration, later GREEN

S:2126, event `bf24cce9`, `2026-09-13T02:53:30.350Z`, calls eval
`toolu_01PZWGnRBA498zbFBnBXN2iz`. Both pre-mutation GREEN and RED use:

```sh
# Historical command only; not executed during recovery.
# cwd: /Users/indo/code/project/orca-lite-wt/sa-tests
CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/sa-tests/target cargo test --manifest-path src-tauri/Cargo.toml --test daemon_duplicate_prevention
```

The wrapper inherits `process.env` and overrides `CARGO_TARGET_DIR`. It reads
`src-tauri/src/daemon/server.rs` into `orig`, replaces the exact expression
`libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)` with the `LOCK_SH`
variant, runs RED, and in `finally` writes `orig` back and compares decoded source
text for equality. Its label says byte-identical; the actual check is text equality,
not a saved byte hash. The wrapper does not run GREEN after that restoration.

S:2127, result `3582edb6`, `2026-09-13T02:53:55.465Z`, same tool-call ID,
contains this exact output excerpt (indentation and truncation retained):

```text
=== GREEN (unmutated) ===
  exit=0 | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.08s
  mentions live /tmp/rorca-501: false

mutation (LOCK_EX -> LOCK_SH) applied: true
=== RED (duplicate no longer refused) exit=101 ===
  test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.95s
    test second_daemon_refuses_already_locked_socket_directory ... FAILED
    ---- second_daemon_refuses_already_locked_socket_directory stdout ----
    thread 'second_daemon_refuses_already_locked_socket_directory' (1583783) panicked at tests/daemo
    second_daemon_refuses_already_locked_socket_directory
  MUTATION CAUGHT: true

  restored byte-identical: true
```

Why the cause is missing: the wrapper captures stdout/stderr into local strings,
then retains only lines matching `/assertion|panicked|refus/`, takes four lines,
and prints at most 96 characters per line. The intended diagnostic,
`duplicate daemon reached readiness while owner held both locks`, matches none
of those alternatives. The wrapper prints no RED `isolated`/cleanup entries and
saves no complete stdout/stderr file. Tool metadata `truncated:false` describes
the wrapper output, not the discarded subprocess streams. The RED heading and
`MUTATION CAUGHT` are wrapper-authored labels, not assertion diagnostics.

Later GREEN: S:2149, event `96d719ce`, `2026-09-13T02:58:36.840Z`, eval
`toolu_01AKx3SpUbBKiMTj2Ve74Q4s`, executes the same duplicate command/cwd/target
override during a larger sweep. Its completion notification S:2154 (`bef6e181`,
`2026-09-13T02:59:28.707Z`) points to source G:

```text
/Users/indo/.omo/agent/sessions/--Users-indo-code-project-orca-lite--/2026-09-12T12-29-35-992Z_01a09598-3778-7aff-83ff-6f8031c71b9d-artifacts/local/detached-eval-toolu_01AKx3SpUbBKiMTj2Ve74Q4s.log
```

G:15, exact retained output:

```text
  PASS  C4 daemon_duplicate_prevention     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured
```

The S:2149 wrapper emits PASS only when the subprocess exit is 0. S:2156,
`6e0d7540`, `2026-09-13T02:59:49.936Z`, reprints the stored sweep results.
Thus the supported order is pre-mutation GREEN -> RED exit 101 -> source-text
restoration -> later sweep GREEN exit 0, not an immediate paired restoration run.

## Searches and precise remaining gap

1. Searched `/Users/indo/ferryx-4track-evidence-20260913/` and the requested
   parent notepad for `LOCK_SH`, `LOCK_EX`, the test name, readiness diagnostic,
   and restoration. `EVIDENCE_LEDGER.md:819-820` and notepad `:821-822`
   repeat only the mutation and FAILED summary; they are not independent raw
   captures. FINAL-12 records the later GREEN.
2. After reading the coding-agent-sessions skill and Senpi reference, searched
   original local session stores and the original session's artifact directories.
   The finder first failed under system Python 3.9 (`TypeAlias` import), then
   returned zero matches under Homebrew Python; direct scoped JSONL search
   recovered S:2126-2127. Artifact searches for `LOCK_SH`, `1583783`, and the
   readiness diagnostic found no separate raw mutation output. Source G was
   opened directly. The current `agent/sessions` JSONL copy contains later audit
   quotations, not the original mutation events found in S; they are not substitutes.

These independent evidence-bundle and original-session searches are exhausted
for this recovery. Missing: original unfiltered stdout/stderr showing the actual
assertion message and successful mutation-path child/fixture cleanup. Current
source predicts the intended error but cannot establish that it happened then.
A new run would be prospective evidence only.

## Source-backed description corrections and bounded coverage

- `soak.mjs:6-14` and `soak.test.mjs:5-11`: sorted samples are 1..10.
  Replacing `ceil(p*n)-1` with `floor(p*n)` changes p50 5 -> 6 and p90 9 -> 10;
  p95/p99 stay 10. This is arithmetic inspection, not a new mutation receipt.
- `daemon_duplicate_prevention.rs:60-123,151-209`: explicit fixture environment,
  PID-checked connections, retained owner connection, protocol cleanup before
  outcome assertion. The readiness error is one possible outcome; timeout,
  handshake, PID, and cleanup failures are distinct possible failures.
- `daemon/server.rs:127-168,270-300,569-580,692-716,1452-1464`: explicit runtime
  and data overrides resolve socket and lock paths. Startup acquires locks before
  replacing the socket. Both persistent and runtime locks use the mutated helper;
  this test shares both directories and does not isolate each lock's protection.
- Absence of a path in logs is not proof of absence of access. Environment/path
  configuration is not an OS sandbox. Listing/metadata equality cannot establish
  file-content equality or exclude transient access. Successful cleanup receipts
  do not cover every failed-start cleanup branch.

Only the report and this recovery note were edited. No prose-pinning tests were
added. These corrections do not reverse the parent notepad's leading INCOMPLETE
verdict, the original PID change from 36170 to 1010, or other historical deviations.
