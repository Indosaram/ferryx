# A06 budget resume - mutation RED and restored GREEN

Task `st_01a09806`, parent `01a097f8-4568-7573-897e-d61f0fe6d692`, 2026-09-12.
Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.

## Disposition

**Q1's two budget proof seams are closed at the scoped Darwin backend boundary.**
Two deterministic regression tests and minimal clock seams are implemented.
Three mutation rounds failed at the intended assertions, and the restored
filesystem suite passed once: 13 passed, 0 failed, exit 0. Every mutation was
restored to the same pre-mutation SHA-256. The initial shared compiler blocker
was resolved by its owner; its unsuccessful attempt is retained below as
history, not behavioral RED. No directory implementation redesign was needed.
This is not full A06 acceptance or a new composed acceptance gate.

Basis read: approved plan sections 4.2, 8, A06; the resume acceptance-gaps report,
especially A06 and Q1; current filesystem implementation/tests, module inclusion,
and router attachment of the production handler and limiter.

## Implemented and executed coverage

- `remote::filesystem_tests::a06_budget_inspected_children_exact_cutoff` creates
  native regular files and checks 9,999, 10,000, and 10,001 children. Retained
  entries are always zero; truncation must be false, false, true respectively.
  Every child is equivalent, so native enumeration ordering is irrelevant.
  The actual native scan loop receives elapsed time zero through
  `scan_with_elapsed`; production passes `started.elapsed()`. This isolates the
  child budget from the five-second deadline and from the unrelated retained
  1,000-entry/256 KiB budgets. The fixture closes its private root and asserts
  absence even after an assertion unwind. It creates no listener or worker.
- `remote::filesystem_tests::a06_budget_per_device_burst_and_refill` exercises
  the actual bucket/permit admission path with explicit monotonic instants.
  Both devices independently admit 20 requests at the same instant and reject
  the next with 429, `RATE_LIMITED`, and `Cache-Control: no-store`. Each successful
  permit is released immediately, so four-slot concurrency cannot masquerade
  as rate exhaustion. A request at 99,999,999ns is rejected; at 100ms exactly one
  token is available. Another second admits exactly ten requests; a three-second
  idle interval refills no more than 20. No sleeps, polling, global clock, auth
  mock, network listener, or filesystem fixture is used by this test.
- `BrowseLimits::acquire` delegates to shared `acquire_with_clock`, sampling
  `Instant::now` while holding the budget lock. The test-only `acquire_at`
  supplies its instant through that same path. Limits, authorization, native
  enumeration, cancellation, semaphore ownership, and error mapping are not
  redesigned. Existing tests are unchanged; only the two new tests were added.

Existing proofs preserved rather than duplicated: `a06_directory_http_home`
(1,000 retained entries, native/auth behavior),
`r6_empty_deadline_and_independent_bytes` (independent 256 KiB cap, expired scan,
post-resolution cancellation), `a06_native_bounds_and_query_contract` (four
listing slots), and the existing routed auth, disconnect, and cancelled-worker
fixtures. All passed in the single restored filesystem-suite run below.

## Historical initial attempt (superseded compiler blocker)

Temporary substitutions in `filesystem.rs`:

```diff
- if elapsed() >= SCAN_BUDGET || index >= 10_000 {
+ if elapsed() >= SCAN_BUDGET || index >= 10_001 {
- budget.tokens = (budget.tokens + now.duration_since(budget.updated).as_secs_f64() * 10.0).min(20.0);
+ budget.tokens = (budget.tokens + now.duration_since(budget.updated).as_secs_f64() * 11.0).min(20.0);
```

Intended assertion failures: the 10,001-native-file scan incorrectly reports
complete; refill incorrectly admits before 100ms. These are expectations,
**not observed mutation sensitivity**.

Exact environment and command:

```sh
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=3 RUSTC_WRAPPER=
export CARGO_TARGET_DIR="$PWD/src-tauri/target"
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features \
  --lib remote::filesystem_tests::a06_budget -- --nocapture
```

Artifact: `A06-budget-resume-RED-upper-refill.log` (name denotes attempted RED,
not a successful mutation proof). The shell monitor recorded cargo PID 59131;
it queued on the existing build-directory lock without terminating its holder.
Darwin kqueue `NOTE_EXIT` observed cargo termination, without sleep/polling.
Exit receipt: `A06_RED_UPPER_REFILL_EXIT=101`.

Actual failure:

```text
error[E0425]: cannot find function `machine_service_unavailable` in this scope
    --> src/remote/server.rs:2627:24
2627 | let response = machine_service_unavailable(State(state), headers).await.unwrap();
error[E0282]: type annotations needed
    --> src/remote/server.rs:2627:24
error: could not compile `ferryx` (lib test) due to 2 previous errors; 10 warnings emitted
```

At that attempt the call remained in `a03_absent_machine_service_is_private_and_unavailable`;
search found no matching function definition in that file. On resumption,
source inspection confirmed the owner had replaced it with the actual
`session_api::create` path. The subsequent mutation runs compiled and executed.
Unrelated warnings
are retained in the raw log (macOS unsafe blocks and unused variables). No
errors/warnings were suppressed, tests skipped, or shared dependencies edited.

## Restoration, diagnostics, and cleanup receipts

All source substitutions were reversed with targeted `apply_patch` edits.
The following SHA-256 values match the pre-mutation, new-test baseline exactly:

```text
c0c3ff7b5258817c123b698da3802a4840d3500d852cde5d7d320b00ebaa3432  src-tauri/src/remote/filesystem.rs
4fb794b95130fb557228e758688299439c9e34d15481d41fb0ede0f4fd3d0e70  src-tauri/src/remote/filesystem_tests.rs
```

Restored source inspection shows `index >= 10_000`, initial `tokens: 20.0`,
refill `* 10.0`, and `.min(20.0)`. No mutation remains active.

LSP diagnostics were requested for both files before cargo. Initial results
were empty; the pre-mutation-run refresh timed out for filesystem.rs (3s), while
tests had no diagnostics. Final restored diagnostics contain only inactive-cfg
hints (Windows/Linux/non-test branches), with no errors or warnings in either
changed Rust file. `git diff --check` for these paths returned clean, but both
files are inherited untracked files, so that command does not validate their
full content as a tracked diff. On resumption, diagnostics before RED and
again before restored GREEN returned only inactive-cfg hints for both files.

The initial cargo and shell monitor PIDs 59129/59130/59131 exited without
creating fixtures because compilation failed. Resumed monitor PIDs
92662/94553/95599 also exited, observed using Darwin kqueue `NOTE_EXIT`, and
their absence was checked after GREEN. Each mutation monitor read current
source/status/diff before applying a patch, used a `finally` restoration path
(including handlers for INT/TERM/HUP), and asserted byte equality with its
pre-mutation source. Package/build locks were allowed to queue; no holder was
killed. The new count fixture logged root removal on both mutation failures
and GREEN. The limiter fixture uses no root/listener and releases each owned
permit immediately; any permit unexpectedly returned during RED is dropped
by assertion unwinding along with the private limiter.

GREEN cleanup receipts include native count root removal, routed listener
join/refusal, request-drop worker completion and slot recovery, injected
send-failure root/listener teardown, and exact auth-worker completion-channel
drains (3/3, 3/3, 18/18) before root removal. Expected caught fixture panics
appear under `--nocapture`; all enclosing tests passed.
No canonical daemon, desktop, deployment, provider configuration, manifest,
Cargo.lock, or A09/shared router source was edited. No commits were created.

## Completed mutation-RED receipts

All runs used the environment documented above and the same command prefix:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib
```

| Round / artifact | Exact selector and suffix | Mutation | Observed assertion / exit |
| --- | --- | --- | --- |
| `A06-budget-resume-RED-upper-refill-v2.log` | `remote::filesystem_tests::a06_budget -- --nocapture` | `index >= 10_000` -> `index >= 10_001`; refill `* 10.0` -> `* 11.0` | Count 10,001: actual truncated=false, expected=true. Limiter admitted at 99,999,999ns after both devices exhausted 20; `exhausted bucket must reject admission`. 0 passed / 2 failed; `A06_RED_UPPER_REFILL_V2_EXIT=101`. |
| `A06-budget-resume-RED-lower-burst-saturation.log`, LOWER_BURST | `remote::filesystem_tests::a06_budget -- --nocapture` | `index >= 10_000` -> `index >= 9_999`; initial `tokens: 20.0` -> `tokens: 19.0` | Count 10,000: actual truncated=true, expected=false. `device-a burst admission 20 rejected`. 0 passed / 2 failed; `A06_RED_LOWER_BURST_EXIT=101`. |
| Same artifact, SATURATION (previous mutations restored first) | `remote::filesystem_tests::a06_budget_per_device_burst_and_refill -- --nocapture` | `.min(20.0)` -> `.min(21.0)` | Both bursts, exact 100ms and ten-token second passed; after three-second refill, request 21 unexpectedly returned a permit. 0 passed / 1 failed; `A06_RED_SATURATION_EXIT=101`. |

Every round logs its precise patch and inverse plus
`RESTORED_SHA256=c0c3ff7b5258817c123b698da3802a4840d3500d852cde5d7d320b00ebaa3432`.
No test assertions were weakened or changed between RED and GREEN.

## Restored GREEN

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features \
  --lib remote::filesystem -- --nocapture
```

Artifact: `A06-budget-resume-GREEN.log`. **13 passed, 0 failed, 0 ignored**,
10.16s test execution; `A06_RESTORED_GREEN_EXIT=0`. Both restored file hashes
are printed at the end of the log and match the baseline above. Executable:
`src-tauri/target/debug/deps/ferryx_lib-82e1988715e4a304`.

Observed new-fixture receipts:

```text
A06 inspected: native_children=9999 retained=0 elapsed=0 truncated=false
A06 inspected: native_children=10000 retained=0 elapsed=0 truncated=false
A06 inspected: native_children=10001 retained=0 elapsed=0 truncated=true
A06 inspected cleanup: private root removed; no listener or worker created
A06 rate: device-a admitted=20 same-instant next=429/RATE_LIMITED/no-store
A06 rate: device-b admitted=20 same-instant next=429/RATE_LIMITED/no-store
A06 rate: 99,999,999ns rejected; 100ms admitted exactly one
A06 rate: one-second refill admitted=10 next=429/RATE_LIMITED
A06 rate: three-second refill capped at 20; permits released; private limiter dropped; no roots/listeners
```

Existing independent-byte fixture retained 473/800 entries, JSON 261829 bytes,
next entry 553 bytes. Existing native HTTP home/browsing, 1,000 retained entries,
auth/no-store/service-less/refusal, cancellation and worker-slot tests passed.
Thus the affected real HTTP surface was executed by the existing fixture, not
replaced by a new independent acceptance verifier. Compiler warnings remain
visible in the logs (16 in GREEN, unrelated existing unsafe/unused/dead-code
warnings); no warnings were suppressed.

No separate full build, desktop manual exercise, platform expansion, or full
composed acceptance is claimed. Those gates remain the later batch verifier's
responsibility; Linux/Windows and client/picker acceptance are not closed here.
