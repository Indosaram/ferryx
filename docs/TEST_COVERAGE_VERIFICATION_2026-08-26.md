# Test Coverage Verification — 2026-08-26

## Result

The complete Rust, UI, and site test suites pass. UI coverage is now reproducible with `bun run --cwd ui test:coverage` and is scoped to application sources (`ui/src`) rather than generated or bundled artifacts.

## UI Coverage

| Metric | Before | After |
| :--- | ---: | ---: |
| Lines | 84.00% | 85.39% |
| Functions | 72.92% | 74.45% |
| Branches | 77.43% | 78.07% |

The test additions concentrate on previously untested or weakly covered behavior:

| Module | Line coverage after | Coverage added |
| :--- | ---: | :--- |
| `ui/src/lib/branchFilter.ts` | 100.00% | branch normalization, managed worktree labels, folder fallback |
| `ui/src/lib/browserHistory.ts` | 96.29% | validation, ordering, deduplication, history limit, disabled storage |
| `ui/src/lib/notificationCoordinator.ts` | 99.09% | notification gates, unread state, throttling, completion suppression, reset |
| `ui/src/lib/terminalThroughputMetrics.ts` | 100.00% | metrics gate, histogram percentiles, bounds, debug interval, reset |

`ui/vitest.config.ts` now limits coverage collection to `src/**/*.{ts,tsx}`, excludes test and benchmark artifacts, and produces text plus JSON-summary reports under the ignored `ui/coverage/` directory.

## Full Validation

| Command | Result |
| :--- | :--- |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | Pass — includes the local GPU renderer contract (19 tests) |
| `bun run --cwd ui test` | Pass — 87 files, 753 tests |
| `bun test --cwd site` | Pass — 5 tests |
| `bun run --cwd ui test:coverage` | Pass |
| `bun run --cwd ui build` | Pass — TypeScript check and Vite production build |

## Rust Failures Resolved During Verification

The full suite initially exposed two issues in parallel in-flight code:

1. `src-tauri/src/ipc/browser.rs` passed a consuming `oneshot::Sender` into Tauri's repeatable `Fn` callback. The sender now lives in a mutex-protected `Option`, allowing the first callback result through and ignoring any repeat callback safely.
2. `RemoteGatewayState` used `watch::Sender::send` before any remote client subscribed. Tokio discards that update when no receiver exists, so a later remote terminal client saw no active session and its socket closed after the initial grid frame. `send_replace` persists the active selection for late subscribers. The grid live-output and resize contracts, plus a late-subscriber regression test, now pass.
