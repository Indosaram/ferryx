# Task 8 frozen candidate verification

Task 8 verification COMPLETE; candidate NOT ACCEPTED. Product lanes reopened, no repairs performed.

Candidate 5464da0dd65a7a6312d096229d2067ec02735c78, tree 3c91df668bd2fac62a4fc37a0aa2df4cdbd3cc9d, base d82b35e4. Provenance: manifest.json and host-provenance.txt. Exact timestamps/PIDs/argv/native exits: commands.jsonl; original unnormalized records: {mac,windows,linux}-commands.jsonl. Raw logs: logs/{mac,windows,linux}/<gate>.log.

## Platform summary

| Platform | Gates | Full UI raw exit | Tests | Verdict |
| --- | ---: | ---: | --- | --- |
| maho-mac | 15 | 1 | 243 failed / 6277 passed (6520); 2 errors | FAILED |
| maho-win | 15 | 1 | 244 failed / 6271 passed / 5 skipped (6520); 3 errors | FAILED |
| Linux indo | 19 | 1 | 1271 failed / 5249 passed (6520); 1 error | FAILED |

All builds native 2; localSplitLifecycle 19/19 passed each. Scoped lifecycle Mac/Windows 9 failed/102 passed, Linux 12 failed/99 passed (111 each). Runner zero selected, native 1, failed coverage. All 34 Rust attempts native 101: missing ui/dist from failed UI prerequisite; test bodies NOT RUN, selection UNKNOWN, not zero-naming claims. Windows skipped tests remain skipped, never passes.

## Grouped owning-lane findings

Frontend REOPEN: three identical TypeScript build errors (verbatim below), scoped persistence/lifecycle failures, full-UI failures. Full raw assertions are in corresponding logs; failure-index.jsonl is convenience only.

```text
src/lib/sessionPersistence.ts(2,33): error TS6133: 'LocalSplitIntent' is declared but its value is never read.
src/state/workspaceStore.ts(773,22): error TS6133: 'layout' is declared but its value is never read.
src/state/workspaceStore.ts(2695,38): error TS2304: Cannot find name 'isLegacyUnknown'.
```

Scripts adapters REOPEN: exact gate discovers zero tests:
```text
No test files found, exiting with code 1
include: scripts/qa/pane-liveness.test.mjs
```

Backend daemon/terminal/ipc-terminal and native native_terminal/ipc-native_terminal UNPROVEN, rerun required after frontend prerequisite repair. No fabricated backend/native compiler findings. Unix list/run output:
```text
resource path `../ui/dist` doesn't exist
```
Windows:
```text
thread 'main' (22896) panicked at build.rs:122:39:
tauri build failed: resource path `..\ui\dist` doesn't exist
```

Baseline-confirmed ONLY Mac TerminalSearchOverlay.test.tsx:332 (expected 2 calls, got 3) and updater.test.ts:141 (expected 1 call, got 0), exact base A/B native1, 2 failed/32 passed/34. Evidence logs/base-ab.log. Other failures unclassified relative to base; no pre-existing claims. Detailed routing: baseline-findings.md.

## Exact per-platform ledger

### mac
| Command | Host | Raw exit | Asserted line | Selected | Verdict |
| --- | --- | ---: | --- | --- | --- |
| `bun run --cwd ui build` | mac | 2 | src/lib/sessionPersistence.ts(2,33): error TS6133: 'LocalSplitIntent' is declared but its value is never read.; src/state/workspaceStore.ts(773,22): error TS6133: 'layout' is declared but its value is never read.; src/state/workspaceStore.ts(2695,38): error TS2304: Cannot find name 'isLegacyUnknown'. | UNKNOWN / N/A | RAN_FAILED |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | mac | 0 |  Test Files  1 passed (1);       Tests  19 passed (19) | 19 | RAN_PASSED |
| `bun run --cwd ui test src/lib/sessionPersistence.test.ts src/lib/sessionLifecycle.test.ts src/lib/nativeTerminalLifecycle.test.ts src/components/NativeTerminalPane.lifecycle.test.tsx` | mac | 1 |  Test Files  2 failed / 2 passed (4);       Tests  9 failed / 102 passed (111) | 111 | RAN_FAILED |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | mac | 1 | No test files found, exiting with code 1 | 0 | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --list` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | mac | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `bun run --cwd ui test` | mac | 1 |  Test Files  19 failed / 353 passed (372);       Tests  243 failed / 6277 passed (6520) | 6520 | RAN_FAILED |

### windows
| Command | Host | Raw exit | Asserted line | Selected | Verdict |
| --- | --- | ---: | --- | --- | --- |
| `bun run --cwd ui build` | windows | 2 | src/lib/sessionPersistence.ts(2,33): error TS6133: 'LocalSplitIntent' is declared but its value is never read.; src/state/workspaceStore.ts(773,22): error TS6133: 'layout' is declared but its value is never read.; src/state/workspaceStore.ts(2695,38): error TS2304: Cannot find name 'isLegacyUnknown'.; NATIVE_EXIT=2 | UNKNOWN / N/A | RAN_FAILED |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | windows | 0 |  Test Files  1 passed (1);       Tests  19 passed (19); NATIVE_EXIT=0 | 19 | RAN_PASSED |
| `bun run --cwd ui test src/lib/sessionPersistence.test.ts src/lib/sessionLifecycle.test.ts src/lib/nativeTerminalLifecycle.test.ts src/components/NativeTerminalPane.lifecycle.test.tsx` | windows | 1 | 2076:  Test Files  2 failed / 2 passed (4) | 111 | RAN_FAILED |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | windows | 1 | No test files found, exiting with code 1; NATIVE_EXIT=1 | 0 | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --list` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | windows | 101 | tauri build failed: resource path `..\ui\dist` doesn't exist; NATIVE_EXIT=101 | UNKNOWN / N/A | RAN_FAILED |
| `bun run --cwd ui test` | windows | 1 |  Test Files  21 failed / 351 passed (372);       Tests  244 failed / 6271 passed / 5 skipped (6520); NATIVE_EXIT=1 | 6520 | RAN_FAILED |

### linux
| Command | Host | Raw exit | Asserted line | Selected | Verdict |
| --- | --- | ---: | --- | --- | --- |
| `bun run --cwd ui build` | linux | 2 | src/lib/sessionPersistence.ts(2,33): error TS6133: 'LocalSplitIntent' is declared but its value is never read.; src/state/workspaceStore.ts(773,22): error TS6133: 'layout' is declared but its value is never read.; src/state/workspaceStore.ts(2695,38): error TS2304: Cannot find name 'isLegacyUnknown'. | UNKNOWN / N/A | RAN_FAILED |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | linux | 0 |  Test Files  1 passed (1);       Tests  19 passed (19) | 19 | RAN_PASSED |
| `bun run --cwd ui test src/lib/sessionPersistence.test.ts src/lib/sessionLifecycle.test.ts src/lib/nativeTerminalLifecycle.test.ts src/components/NativeTerminalPane.lifecycle.test.tsx` | linux | 1 |  Test Files  2 failed / 2 passed (4);       Tests  12 failed / 99 passed (111) | 111 | RAN_FAILED |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | linux | 1 | No test files found, exiting with code 1 | 0 | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --list` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `bun run --cwd ui test` | linux | 1 |  Test Files  107 failed / 265 passed (372);       Tests  1271 failed / 5249 passed (6520) | 6520 | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib suspension -- --list` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib suspension -- --nocapture --test-threads=1` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib split_journal -- --list` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib split_journal -- --nocapture --test-threads=1` | linux | 101 |   resource path `../ui/dist` doesn't exist | UNKNOWN / N/A | RAN_FAILED |

## Boundaries and teardown

Owned source staging removed on all three hosts after raw archive; Ghostty links/junction detached first, shared checkout untouched; Linux private Ghostty removed, shared bundle preserved. Base A/B source removed. Receipts cleanup.json and monitor-ledger.json. No product edits, git commits, GUI, production/daemon mutation. Task 9/10 NOT RUN, explicitly outside dispatch. Evidence retained uncommitted under task-8.

