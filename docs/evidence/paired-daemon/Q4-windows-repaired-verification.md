# Q4 Windows: reviewed compiler repair verification

## Disposition

The reviewed clipboard-only delta resolves the original native Windows E0433
compiler blocker. The headless library, ferryx-cli and ferryx-relay build exited
0; six existing pure clipboard payload/conversion tests passed. **Full Q4
Windows verification remains NOT ACCEPTED**: subsequent backend suites expose
Windows fixture/compiler and runtime failures. No production repair beyond the
parent-reviewed file was imported or authored in this lane.

## Exact source and environment

The original frozen report and all failures remain in
`Q4-windows-frozen-verification.md` and its referenced artifacts. The retained
private root is `C:\Users\sook\ferryx-herdr-q4-windows-01a097f8`, native maho-win,
Rust/Cargo 1.97.0, x86_64-pc-windows-msvc. This is not WSL or desktop evidence.

Read the actual parent diff and full frozen clipboard_image.rs. The sole delta
adds function-local CF_DIB_ID:u32=8 and CF_DIBV5_ID:u32=17, replacing references
to the feature-gated IPC constants. Exact before/after SHA-256 values are in
`Q4-windows-repaired-delta.json`; the complete diff is
`Q4-windows-repaired-source.patch`. The transferred file is retained verbatim.

Before replacement all 969 original manifest entries matched on Windows. After
replacement and again after all runs, all 969 repaired entries matched. The
original manifest was preserved; the repaired manifest differs in only this
one file. Frozen Wave1's 34 historical inputs matched again locally at finish.
No A10, Q1 or Q3 source was imported. Cargo.lock, private UI dist, pinned Ghostty
dependency and all other frozen source remain unchanged.

Local LSP on the reviewed file produced only inactive Windows/non-macOS cfg
hints; native compiler execution, not those hints, proves this repair compiles.
The script sets private HOME, USERPROFILE, APPDATA/LOCALAPPDATA, FERRYX paths,
XDG paths and TMP/TEMP/TMPDIR before library initialization, retains explicit
Cargo/Rustup homes and private target, jobs=3, empty wrapper via private Cargo
override and child environment, debug=0 and incremental=0. Explicit child cwd
is the private source root. Global/system Git configuration is isolated.

## Executed command ledger

Every command begins `cargo --config C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\cargo-qa.toml`.
The override only disables host-configured sccache. Arguments below otherwise
retain the requested locked headless baseline selectors. Full stdout/stderr,
commands, process IDs and exit receipts are in `Q4-windows-native-repaired-*`.
Every run executed once and was waited by a bounded process monitor; no unchanged
retry or sleep-based test stabilization occurred.

| Arguments | Result |
| --- | --- |
| `build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay` | Exit 0; 50.82s; warnings retained |
| `test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib clipboard_image::tests:: -- --nocapture` | Exit 0; 6 passed, 724 filtered out |
| `test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture` | Exit 101; 156 passed, 16 failed, 558 filtered out |
| `test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence --test machine_worktrees --test worktree_safety --test machine_worktree_legacy_bounds --test machine_worktree_transports --test relay_pairing_generation_regression -- --nocapture` | Exit 101; integration compilation blocked, no integration execution claimed |
| `test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib worktree:: -- --nocapture` | Exit 101; 38 passed, 2 failed, 690 filtered out |

Clipboard tests were read in full before execution: bottom-up 24bpp conversion,
top-down row order, zero alpha opacity, real alpha preservation, malformed/
truncated bitmap refusal and empty payload rejection operate exclusively on
in-memory bytes. No test reads or writes the actual Windows clipboard.

## New blockers retained for coordination

Integration compiler: `machine_worktrees.rs:131` has untyped
`let mut owned_session = None`; its Some assignment at line 245 is inside a
Unix-only section, while cleanup at line 427 remains compiled on Windows.
Native rustc reports three E0277 unsized-str errors at lines 131/427. This is
a fixture compiler defect, not behavioral RED. No test was removed or skipped
to make the aggregate pass.

Remote failures, with exact assertions retained in the full log:

- `a06_directory_http_home` and `a06_http_send_failure_still_tears_down`:
  fixture directory creation at filesystem_tests.rs:154 fails Windows OS 123
  InvalidFilename. The old shared fixture contains Windows-invalid names.
- `coordinator_pairs_through_relay_to_real_gateway` and
  `test_get_active_running_sessions_independent_of_desktop`: explicit `/bin/sh`
  cannot spawn on native Windows (OS 3). No successful PTY claim follows.
- `test_relay_browser_ws_terminal_bridge_success`: data-channel OS 10053.
  Cause is unresolved; a socket error is not successful relay WS evidence.
- `a07_project_http_registration`: actual HTTP 400 INVALID_PATH versus 201.
- `followthrough_delete_publication_blocks_spawn`: 404 versus 200.
- `followthrough_prune_partial_replays_after_restart`: 409 versus 201.
- `local_worktree_errors_survive_owner_wire_and_native_adapter`:
  INTERNAL_ERROR/WORKTREE_NOT_FOUND versus DIRTY_WORKTREE, with actual private
  Windows verbatim worktree path in the log.
- `followthrough_write_failures_non_head_and_prunable_preview`: missing map key
  at authority test line 115, then JoinError.
- `interrupted_git_transaction_restart_never_repeats`: child assertion/barrier
  fails; child exit 101 is reaped, not a successful crash recovery proof.
- `r12_real_kill_windows`: initial register returns 400 versus 201; child exits
  101 before barrier. No completed four-window crash proof.
- `owner_http_gate_and_eight_admissions`: target count 0 versus 1.
- `r12_revocation_fences`, `cancelled_publication_preserves_unknown_after_git`,
  `r1_topology_after_gate_http`: bounded waits return Elapsed. Not classified as
  timing luck or repaired by increasing deadlines.

Worktree failures: `a08_git_real_legacy_worktree_path` creates a name containing
a quote and trailing space; native Git rejects its metadata path with exit 128
Invalid argument. `ssh::worktree::tests::red_posix_script_builders_executable_behavior`
fails its create_script assertion. The broad worktree:: selector includes SSH
tests; their counts are not all paired-daemon coverage.

These are observed failures, not a claim that every failure is an independent
production bug. Apart from explicit fixture/cfg causes above, root causes need
coordinated investigation. No source was patched or tests retried here.

## Actual behavior versus missing platform evidence

The remote log contains individual passing test names and real routed receipts:
R3 HTTP refusal and body-boundary checks with listener join/refusal; R6
service-less 503, HTTP disconnect/worker retention and auth revocation/deadline
drain (18/18 workers); R12 auth admission and incomplete-body teardown; AC09
private child preferences HTTP redaction with child wait/root removal. Expected
injected panics in passing teardown tests are distinct from the 16 failed tests.
`Q4-windows-repaired-passing-ledger.log` indexes these receipts without turning
the failed aggregate into acceptance. Nested helper/child selector passes are
not extra independent scenarios.

Native worktree Git execution passed create/list, safe clean deletion, branch
deletion, dirty refusal/isolation and existing local-budget create/delete/output
bound tests; exact test names are in the worktree log. This exercises the native
Windows runner for ordinary operations, not a dedicated suspended-process Job
assignment/descendant cleanup proof. The multigeneration Unix tests are cfg
excluded. No new Job/ACL fixture was added.

Drive-root navigation, actual UNC refusal, native hidden attributes, ACL denied
listing, successful PTY relay and explicit suspended-create/Job drain remain
unproven. A nonexistent UNC share was not invented, no share/ACL was changed,
and failed fixture setup is not directory coverage. The full
machine_worktree_transports fixture is Unix-only; remote/tests.rs and its
security socket fixture subtree are also Unix-gated. Zero cfg matches are not
passes. Further live platform expansion stopped at the concrete compiler and
runtime failures above instead of silently fixing or replacing fixtures.

## Fixture isolation and cleanup

Pre-execution source inspection checked tempfile roots, private daemon path
overrides, explicit socket constructors, platform cfg and clipboard purity.
Runtime roots are outside Git checkouts. The retained source is an extracted
archive, not an existing checkout. Source-relative UI fixtures refer only to
private UI assets; Unix-gated fixtures did not run on Windows. The integration
tempdir_in(".") fixtures never executed because compilation failed. No real
daemon endpoint, credential, installed app or desktop was used.

Final `Q4-windows-native-repaired-cleanup-final.log` records 969 unchanged
repaired inputs, zero owned runtime processes, all five immediate command
children waited, and removal/absence of all nine runtime/home/data/session/
config/cache/temp/appdata/localappdata directories. Cleanup exit is 0. Initial
cleanup conservatively stopped on the current SSH pwsh wrapper; that failure
is retained, and the final receipt excludes only that exact cleanup wrapper.
The suite logs retain joined listener/worker and reaped child receipts where
the fixtures supply them; no universal per-listener inventory beyond those
receipts is claimed after process exit.

Private source, target, archives/bundle and verification tooling are retained
for the next composed run; runtime fixture roots are removed. Original failures
and manifests are untouched. Evidence is uncommitted; no production edits,
release builds, deployments, privileged changes or commits occurred.
