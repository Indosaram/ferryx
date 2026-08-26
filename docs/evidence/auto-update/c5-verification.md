# C5 evidence - regression check against the captured baseline

Captured 2026-08-26T13:37:13.591067

Baselines were taken BEFORE any change, from an rsync snapshot of the then-current working tree
(`baseline-ui-test.txt`, `baseline-cargo-test.txt`), so the parallel edits could not contaminate them.

| Command | Exit | Decisive output | Verdict |
| --- | --- | --- | --- |
| `bun run --cwd ui test` | 0 | `Test Files 84 passed (84)`, `Tests 692 passed (692)` | PASS |
| `bun run --cwd ui build` | 0 | `✓ built in 4.58s` | PASS |
| `cargo build --manifest-path src-tauri/Cargo.toml` | 0 | `Finished \`dev\` profile` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml --test updater_config_contract` | 0 | `test result: ok. 5 passed; 0 failed` | PASS |
| `node --test scripts/sync-version.test.mjs` | 0 | `# pass 6 / # fail 0` | PASS |
| `node --test scripts/build-latest-json.test.mjs scripts/release-workflow.test.mjs` | 0 | `# pass 6 / # fail 0` each | PASS |
| `python3 script/qa/ci-step-order-gate.py` | 0 | `PASS: every building job has zig + submodule init before its build step` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 101 | `test result: FAILED. 251 passed; 2 failed` in `--lib` | FAIL - PRE-EXISTING |

## UI suite: no regression

Baseline was `Test Files 80 passed (80)` / `Tests 657 passed (657)` / `exit=0`. The suite is now
84 files / 692 tests, all passing: +4 files and +35 tests, all of them added by this change
(`ui/src/lib/updater.test.ts`, `ui/src/components/SettingsDialog.update.test.tsx`, plus two files
that were already untracked in the tree). Nothing that passed before fails now.

## Final Rust suite: only baseline daemon failures remain

The final current-worktree rerun is captured in `final-cargo-test-current.txt`. Its library target
passes `254 passed; 0 failed`, including `updater_config_contract` (5/5) and
`updater_endpoint_contract` (3/3). The entire command exits 101 only after reaching the same three
`daemon_persistence_contract` failures already present in `baseline-cargo-test.txt`:

```
test_daemon_gui_process_non_ownership_and_process_tree ... FAILED
test_daemon_terminal_persistence_reconnect_replay_and_isolation ... FAILED
test_daemon_output_sequence_contiguity_and_replay_gap ... FAILED
test result: FAILED. 6 passed; 3 failed
exit=101
```

An intermediate run also observed two uncommitted `remote::tests::test_grid_render_*` failures;
they did not exist in the pre-change snapshot and are unrelated to updater files. They pass in the
final current-worktree rerun, so they are not a remaining release gate.

VERDICT: no updater regression introduced. The only remaining `cargo test` failures are the three
pre-existing daemon persistence contract failures, untouched and out of scope.

## Final UI rerun

The current-worktree UI suite was rerun after the final SemVer, capability, and formatting changes:

```
$ bun run --cwd ui test
Test Files  87 passed (87)
Tests  753 passed (753)
exit=0
```

The final UI production build and the 19 release-script/workflow tests also passed before this run;
their raw command output is captured in the session evidence and summarized in
`c3-ts-ui.md`, `c4-ci-release.md`, and `c2-version-sync.md`.
