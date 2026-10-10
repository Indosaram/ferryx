# H-27 — final finding: Windows `interrupted_spawn_never_repeats_preallocated_intent` is UNRESOLVED BY REPETITION

Closes F1 audit hole **H-27**:

> `H-27 - Windows interrupted_spawn_never_repeats_preallocated_intent UNRESOLVED BY REPETITION, no explicit final finding.`
> — `.omo/evidence/local-pane-liveness-completion-replan/F1-PLAN-TO-ARTIFACT-AUDIT.md:562` (hole list);
> the audit row itself is `:358`.

**Scope of this artifact.** It records the exact commands, host and raw results; states what the evidence
supports and what it does not; states the three possibilities the record names and that the evidence does not
discriminate them; and gives the explicit final finding **plus the exact experiment that would settle it**. It
changes no product, test, script or plan file. **Nothing was compiled or run for this artifact**; every number
below was read from a stored log or from `git show`/`git diff` in this session, and each is cited.

Test under record: `daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent`
(`src-tauri/src/daemon/session_service_machine_tests.rs`).

---

## 1. Exact commands, host, and raw results

**Host:** maho-win, `DESKTOP-1LAPJMP`, `sook@100.126.171.58` (the host named in
`task-8/pass3/windows/CLASSIFICATION-windows.md` §"Exact scoped-A/B evidence for the `interrupted_spawn`
caveat").

**Scoped command — identical on both sides:**

```
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent \
  -- --nocapture --test-threads=1
```

(verbatim from `CLASSIFICATION-windows.md` §"Exact scoped-A/B evidence"; the driver records are
`task-8/pass3/windows/ab2-windows.log` and `ab3-windows.log`.)

**Raw results per attempt.** Byte counts, `test result:` line counts and timeout-panic counts were recomputed
from the four logs in this session (they match the table in `CLASSIFICATION-windows.md`):

| Attempt | Side | Log (under `task-8/pass3/windows/logs/`) | Bytes | `test result:` lines | `Timed out waiting for PTY reader shutdown` |
| --- | --- | --- | --- | --- | --- |
| ab2 rep1 | **base `d82b35e4`** | `ab2-base-1-daemon__…interrupted_spawn….log` | **27890** | **0** | **0** |
| ab3 | **base `d82b35e4`** | `ab3-base-daemon__…interrupted_spawn….log` | **27890** | **0** | **0** |
| ab2 rep1 | candidate | `ab2-cand-1-daemon__…interrupted_spawn….log` | **30848** | **0** | **1** |
| ab3 | candidate | `ab3-cand-daemon__…interrupted_spawn….log` | **31002** | **0** | **1** |

**The absence of verdict lines is the load-bearing fact: no scoped run on either side ever printed a
`test result:` line.** Consequently `native=-1` in the driver logs is **a kill artifact from the verifier
terminating the run, not a test outcome**:

- `ab2-windows.log`: `AB2 side=base rep=1 native=-1 |  | daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent`
- `ab3-windows.log` (44 bytes total): `AB3 side=base native=-1 timeoutHits=0 |`
- `ab3-windows-PARTIAL.log` is 2 bytes (empty), i.e. no verdict was ever captured for that attempt.

**Base side, verbatim tail (both attempts stop at the same point, 27 890 bytes each):**

```
thread 'tokio-rt-worker' (11612) panicked at src\daemon\session_service_machine_tests.rs:321:17:
A09 controlled interruption at sessionSpawned
A09 interruption=sessionSpawned target=909eaab9-2441-483e-977b-c8d7b777b074 original_pid=Some(10120) replay=outcomeUnknown no-repeat=true
A09 interrupted owner cleanup: every owned PTY reaped and private root removed
```
(second attempt identical in shape, `target=a8d63170-… original_pid=Some(27644)`)

**Candidate side, verbatim tail (the extra panic the base never emits):**

```
thread 'daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent' (13960) panicked at src\daemon\session_service_machine_tests.rs:344:63:
called `Result::unwrap()` on an `Err` value: Other("Timed out waiting for PTY reader shutdown")
```
(second attempt identical in shape, thread id 27228)

The two `tokio-rt-worker` panics at `:321:17` are the test's **own intentional** `A09 controlled interruption`
fixture and are present on **both** sides — they are not the failure.

**Where the failure is thrown** (read from the source; the file is **unchanged** by this candidate —
`git diff --stat d82b35e4..HEAD -- src-tauri/src/daemon/session_service_machine_tests.rs` is empty, so these
line numbers hold across the whole chain): `session_service_machine_tests.rs:344` is
`service.terminal_service.close_session(&id).await.unwrap();` inside the test's post-run cleanup loop
(`:342` `for id in service.terminal_service.list_sessions() {`, `:343` `let pty = …get_session(&id).unwrap();`,
`:345` `assert!(pty.is_reaped());`). The `Err` therefore comes out of `close_session`, and its text is produced
by `src-tauri/src/terminal/pty.rs:644` inside `join_reader_bounded` (`:638`
`tokio::time::timeout(READER_SHUTDOWN_TIMEOUT, &mut reader_task)` → `Err(_)` → abort + `PtyError::Other(...)`),
with `READER_SHUTDOWN_TIMEOUT = 2 s` at `pty.rs:20`.

**Full-suite A/B, which is the only A/B that exists for this row** (command
`cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`):

| Side | Revision | Verdict line for this test | Source |
| --- | --- | --- | --- |
| candidate | `abd9e890` | `test daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent ... FAILED` | `task-8/pass3/windows/logs/full-lib.log:1277` |
| base | `d82b35e4` | `test daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent ... ok` | `task-8/pass3/windows/logs/full-lib-base-PARTIAL.log:465` |

Candidate full-suite total: `test result: FAILED. 2201 passed; 70 failed; 8 ignored; 0 measured; 1 filtered out;
finished in 1095.61s` (`task-8/pass3/windows/CLASSIFICATION-windows.md`), with the `1 filtered out` being
`ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection`, which hung and was killed — reported
**NOT_RUN**, never a pass. The base full run was **bounded at 2174 of 2280 tests** because it was spinning on
`worktree::disk_tests::disk_scan_plain_folder_has_no_worktrees` (partial log preserved as
`windows/logs/full-lib-base-PARTIAL.log`). On linux, the same test **passes** at base and at every candidate
revision measured (`task-8/pass3/linux/logs/full-lib.log:1435`, `full-lib-39e722ce.log:1465`,
`full-lib-70eefafe.log:1473`, `full-lib-base.log:1348`, `full-lib-base-HUNG.log:1892` — all `... ok`), so this
row exists **only on Windows**.

---

## 2. What the evidence supports, and what it does not

**It supports** exactly this: **the single full-suite A/B on maho-win showed base `ok` → candidate `FAILED`**,
with the candidate's failure being `Other("Timed out waiting for PTY reader shutdown")`.

**It does not support calling it a confirmed regression.** The reason is structural, not a judgement call:
scoped repetition is the instrument that would convert a single-run A/B into a confirmed regression, and on
this row scoped repetition **produced no verdict on either side** (§1). Two base attempts stopped at an
identical 27 890 bytes with no `test result:` line and no timeout panic; two candidate attempts stopped after
emitting the timeout panic, also with no `test result:` line. A run with no verdict can neither confirm nor
refute anything. The `native=-1` values are the verifier's own kill, not a result.

**And the candidate may be newly *detecting* a pre-existing condition rather than causing one.** Two
structural facts, both established in this session from `git`, **narrow** the three possibilities below without
discriminating between them — they constrain *how* the failure can arise, they do not tell us *which* of the
three it is:

1. **The error and the timeout budget already exist at base.** `git show d82b35e4:src-tauri/src/terminal/pty.rs`
   contains `READER_SHUTDOWN_TIMEOUT = Duration::from_secs(2)` (`:19`), `async fn join_reader_bounded` (`:520`),
   the identical message `"Timed out waiting for PTY reader shutdown"` (`:531`), and a caller in the close path
   (`:660` `if let Err(reader_error) = Self::join_reader_bounded(&session).await`). So the candidate did not
   invent the error; a pre-existing bounded-wait can surface as this exact `Err`.
2. **The candidate did rewrite that file.** `git diff --stat d82b35e4..HEAD -- src-tauri/src/terminal/pty.rs`
   = 269 insertions / 18 deletions, including removal of the base lifecycle watcher's reader-task take and
   `tokio::select!` on it (`git show d82b35e4:…:444` `let mut reader_task = session.take_reader_task();` versus
   the candidate's plain `tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;`) and a new
   `relinquish_transferred_session` that performs its own bounded reader join.
3. **The test itself is unchanged** (§1), so this is not a test-side change producing the new failure.

Together those facts say: the same underlying condition (a PTY reader that does not finish inside 2 s on this
host) can produce the panic, and the candidate changed the code that waits on it. They **narrow** the space —
fact 1 rules out "the candidate invented a new error path", fact 3 rules out "a test-side change", and fact 2
puts the candidate's own reader-lifecycle rewrite in the causal window — but they do **not discriminate** which
of the three possibilities below is true, and no reading of them can substitute for the settling experiment.

**The three possibilities the record names** (`task-8/pass3/windows/CLASSIFICATION-windows.md` §"Verdict for
this row: UNRESOLVED BY REPETITION — not a confirmed regression"), stated plainly and **not discriminated** by
the evidence:

1. **Real regression** — the candidate introduced a defect in the PTY-reader shutdown path.
2. **Load-sensitive flake** — the failure mode is a *timeout*, and this host was carrying several other
   sessions' `cargo` processes throughout (Task 8 also recorded eight long-lived foreign `cargo.exe` processes
   on that host).
3. **Newly-detected pre-existing condition** — the base scoped run *also* never completed and has no timeout
   detection of its own, so the underlying PTY-reader-shutdown condition may exist at base as well and simply
   be reported instead of waited on.

**The evidence does not discriminate them**, and the record's own routing guidance is explicit: treat this as
**"needs a dedicated A/B on a quiet host"**, not as a confirmed candidate defect, and **do not cite it as the
reason for a repair** without that measurement (`CLASSIFICATION-windows.md`, same section). This artifact
adopts that guidance as its disposition rather than converting it into a verdict.

---

## 3. Explicit final finding

**FINAL FINDING — accepted as an unresolved-by-repetition residual; it does not block acceptance of the changed
behavior on the evidence available, and it does not close as green either.**

- **Why it cannot be closed here:** the only A/B that exists for the row is a single full-suite pair
  (base `ok` → candidate `FAILED`), and the instrument that would either confirm it as a regression or refute
  it — scoped repetition with a printed verdict — **has never produced a verdict on either side** on the host
  where the row lives. A kill artifact (`native=-1`) is not evidence.
- **What it is not:** it is **not** a confirmed regression, **not** a confirmed pre-existing failure, and
  **not** a pass. It must not be folded into the windows pre-existing set, and it must not be repaired on the
  strength of the single-run A/B.
- **Named residual risk:** a real PTY-reader-shutdown defect on Windows would affect session close/adoption
  paths (IS-3 ownership/cancellation and IS-1 retention), and this row is currently the only signal pointing at
  it; conversely, spending a repair on a load-sensitive flake would change production code for no measured
  reason. Both directions of error are live until the experiment below runs.
- **Scope note:** the row is Windows-only — the same test passes at base and at every candidate revision
  measured on linux (§1), so nothing here contradicts the linux `--lib` classification.

### The exact experiment that would settle it

Run this **on maho-win (`sook@100.126.171.58`, DESKTOP-1LAPJMP)** — the only host where the row has ever
appeared — and only after the host is **quiet**: no other session's `cargo`/`rustc` running (Task 8 recorded
eight foreign `cargo.exe` whose parents are `rustup.exe`; they must have exited, and the pre-flight must
enumerate and *report* any that remain rather than killing them), and with `C:` headroom checked first.

1. **Stage both sides as separate trees on that host**, each sha256-verified there before launching:
   candidate at the revision under test, base at `d82b35e4`. Do not reuse a tree whose provenance is not
   verified on that host — a per-host stale tree produces a failure that looks exactly like a product defect
   (the rule the effort already paid for twice).
2. **Command (identical both sides):**
   `cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent -- --nocapture --test-threads=1`
3. **The budget is the wrapper's, not cargo's — and this is the actionable reason the earlier runs produced no
   verdict.** All four prior attempts were **terminated by the verifier before any verdict printed** (that is what
   `native=-1` records), and the wrapper window used for those attempts is **not recorded** in any artifact — so
   the requirement below is derived from what the test must reach, not from a measured comparison. The test's own
   spawn budget is 10 s and the failing wait is 2 s (`READER_SHUTDOWN_TIMEOUT`, `pty.rs:20`), and the candidate's
   run must additionally reach the test's post-run cleanup loop — so **give the wrapper a window of at least
   10 minutes and do not terminate the process until a `test result:` line has been read or the window expires.**
   Record the wrapper's own exit separately from the test binary's. This single parameter is the difference
   between a result and another void repetition, so state it explicitly in the run's evidence.
4. **Interleave and repeat: ≥ 5 repetitions per side, alternating base/candidate** (base, candidate, base,
   candidate, …) so a drift in host load cannot be mistaken for a side effect. Record host load average and
   free disk **at start and end of every repetition**.
5. **Capture per repetition, verbatim:** the exact argv, the log path, the byte count, the count of
   `test result:` lines, the count of `Timed out waiting for PTY reader shutdown` occurrences, the summary
   line if any, the raw exit code, and the wrapper's timeout status. **A void repetition is not a result:**
   a repetition with **no** `test result:` line is **void** and must be reported as void — never as a pass and
   never as a failure. All four prior scoped attempts were void for exactly this reason, and it is the
   over-short wrapper window (item 3) that made them so.
6. **Pre-registered decision rule (decide before running, so the result cannot be read backwards):**
   - candidate fails ≥ 1 of 5 with the timeout panic **and** base passes 5/5 → **candidate-caused**: route a
     repair to the PTY reader-shutdown path (owner: `terminal/**`), with the failing repetition's log as the
     reproduction.
   - both sides fail ≥ 1 of 5 with the same signature → **newly-detected pre-existing condition**: record it
     as pre-existing with this A/B, and it becomes a baseline finding rather than a candidate defect.
   - candidate fails intermittently while base passes, or the failure tracks host load → **load-sensitive
     flake**: raise/qualify the wait or isolate the fixture further; do **not** repair product code on it.
   - either side prints **no** verdict in ≥ 3 of 5 repetitions → the experiment is **void**; re-run it (with a
     longer wrapper window or a quieter host) instead of reporting an outcome.
7. **Also capture, in the same pass, the discriminating runtime detail:** the elapsed time inside
   `close_session` before the error, and whether `is_reaped()` at `session_service_machine_tests.rs:345` was
   ever reached. A close that blocks ~2 s and then errors is the bounded-wait firing; a close that never
   returns is a different defect than the one on record.
8. **Report the revision, host, load, argv and raw verdict line** for every repetition, and state explicitly
   which of the four branches above the result lands in.

---

## 4. What was and was not done here

- **Read-only.** Only this artifact was written. No product, test, script or plan file was touched, and no
  history was rewritten.
- **Nothing was compiled, tested, launched or connected.** No cargo, no bun/vitest, no LSP, no remote host, no
  GUI, no daemon — in particular, no attempt was made to re-run the scoped A/B on maho-win.
- **Every figure above** comes from a stored log, a stored classification artifact, or `git show`/`git diff`
  output read in this session; the byte counts, `test result:` counts and timeout counts in §1 were recomputed
  from the four raw logs rather than copied from the classification's table (they agree).
- **Not established anywhere, and stated as such:** the state of maho-win at the time of the two candidate
  attempts beyond "other sessions' cargo processes were running" (no load figure is stored for those two
  scoped attempts — **not recorded**; it should have come from the verifier's per-attempt load capture, which
  the effort's own discipline requires for timed runs), and whether the two candidate attempts would have
  printed a verdict if left to run to completion.
