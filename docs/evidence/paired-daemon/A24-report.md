# A24 report

## Outcome

Partial implementation and automated rehearsal delivered. **AC12 is not met.** The archived relay/local/remote/desktop/mobile version matrix, live-session drain and unrelated-process survival, v2-backup downgrade rehearsal, native surface and Linux evidence remain unproven. They require the human-only steps in `A24-rollout-rollback-runbook.md`; none were executed.

## Changes

- `ui/src/lib/pairedDaemonProject.ts`: clear cached negotiated admission before attempting capability negotiation. Previously a failed renegotiation retained a prior capability set, allowing subsequent operations to dispatch after an incompatible rollback/upgrade, changed machine or mirror-scope reply. This is a two-line behavioral fix, not a new routing path.
- `ui/src/lib/pairedDaemonRollout.test.ts`: five deterministic tests covering an older epoch/fewer capabilities, unsupported old API version, unknown future capability, scope downgrade and wrong machine. Tests check concrete error codes and invocation counts after failed negotiation, retain the captured host/generation and machine grant, and never dispatch a Local/SSH fallback. Native command responses are injected; these are adapter contract tests, not real remote binary tests.
- `src-tauri/src/rollout_tests.rs`, registered under `cfg(test)` in `src-tauri/src/lib.rs`: actual production HandoverManager request-drain rehearsal. Two independent in-process managers, no spawned daemon or PTY. Dropping the connection guard alone leaves the old owner Draining; releasing the retained operation triggers the injected retirement action. A pre-subscribed watch with a five-second timeout observes completion. The other owner stays Active, receives no client abort or retirement action, and fixture route/credential bytes remain unchanged. Temp root is explicitly closed. **No live sessions exist in this test: it does not prove the nonempty session-drain branch or running process survival.**
- Runbook and RED/GREEN/check/build/cleanup evidence under this directory.

No files in the prohibited A15/A16/A17/A18/A19/A22 lanes were edited. No commit, daemon restart/replacement, PTY access, native desktop automation, deployment, signing, packaging or installation occurred. The plan was read at its explicitly supplied canonical documentation path; no canonical source/runtime was changed.

## Failing-first evidence

`A24-RED.log`: Vitest exited 1 before implementation, one passed and four failed. Each failure showed `directories()` resolving successfully instead of rejecting after a failed capability negotiation. This is a real behavior failure, not a synthetic missing-file or prose assertion.

After the fix, the same five tests passed together with 19 existing adapter tests in one Vitest execution. No existing tests were skipped or modified. No sleeps/polling were added.

## Verification

- `A24-GREEN.log`: 24 UI tests passed, one Rust request-drain test passed, required `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib` exited 0.
- Rust test command: `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --no-default-features --lib rollout_tests:: -- --test-threads=1`. This is a library seam test, not an integration test. Full output is `A24-rust-current.log`.
- UI command: `PATH=/Users/indo/.local/bin:/Users/indo/.bun/bin:/opt/homebrew/bin:/usr/bin:/bin bun run --cwd ui test src/lib/pairedDaemonRollout.test.ts src/lib/pairedDaemonProject.test.ts`.
- UI build command with the same PATH: `bun run --cwd ui build`, exit 0 (`A24-ui-build.log`). This is the frontend validation build, not a native release build/package. Vite reports an oversized chunk warning; it is not suppressed.
- Diagnostics: no diagnostics for changed adapter, new UI test and new Rust test. `lib.rs` fresh diagnostics timed out twice; cargo check provides the actual compiler result. Existing compiler warnings are retained in logs, not suppressed.
- Scoped `git diff --check` exited 0.
- Rust execution used a worktree-private HOME/TMPDIR/runtime/data/session/config/cache and a copied private Cargo registry/git cache. Rust toolchain files were read from the installed toolchain; no real HOME runtime or daemon was used. `A24-cleanup.log` records deletion of all A24 private roots. No socket/task/process cleanup against a real daemon was necessary.

## Concurrent build history

The first Rust compile exceeded the tool timeout before tests ran. The resumed compile encountered the concurrent A16 mid-edit state:

```
error[E0599]: no method named `paired` found for struct `terminal::service::TerminalService`
  src/paired_host/proxy_tests.rs:84:14 and :89:14
error[E0282]: type annotations needed
  src/paired_host/proxy_tests.rs:89:5
```

These assertions could not execute at that time. The entire historical output remains in `A24-concurrent-build.log`; it was not erased or represented as GREEN. After parent steering and A16 progress, the rerun compiled and the request-drain assertion passed. Final check also exited 0. `A24-GREEN.log` contains only recorded passing runs, with full original clean Rust logs retained separately. No edits were made to repair another packet's intermediate state.

## Remaining limits and assumptions

Compatibility means wire-contract simulation here, not installation of archived binaries. An old epoch is an opaque target identity, not a lower protocol version; tests separately check API version 0 and valid API version 1 with epoch 1. Unknown capabilities are deliberately rejected by this desktop adapter. No security downgrade or fallback occurs at this adapter seam, but end-to-end Local/SSH/native/relay behavior still needs real-surface evidence.

Request drain is proven with the injected action; live-session drain is not. The user prohibits touching any running PTY, and the existing live owner handover test starts PTYs, so it was not run. No release or manual sign-off is implied by a passing headless check or frontend build. AC12 stays open until the runbook matrix has actual evidence.
