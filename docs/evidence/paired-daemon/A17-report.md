# A17 - partial delivery; acceptance NOT met

All edits and commands ran in `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`. The binding plan was read from its explicitly supplied canonical path. No canonical daemon, running PTY, desktop automation, deployment, release build, or commit was used.

## Changes

- `ui/src/lib/sessionPersistence.ts`: writes schema 3; persists `remoteWorkspaceId` on first and subsequent saves; projects paired targets to kind/hostId rather than copying extra credential fields; validates paired workspace ownership before restoration; retains paired proxy IDs despite local inventory absence/epoch changes; resets their local replay boundary; restores remote worktree ownership; ignores explicitly incomplete inventory as evidence of death.
- `ui/src/state/workspaceRestore.ts`: paired offline restore defers local reconciliation. Mixed preloading containing paired projects preserves snapshots on local inventory failure. Existing local-only failure behavior remains unchanged.
- `src-tauri/src/session/mod.rs`: before replacing legacy state with v3, publishes and verifies a one-time `*.json.pre-v3` backup using the old inode, syncs it and its directory, and aborts on backup failure. Rejects writes that downgrade an existing schema. v1/v2 reading remains supported; migration is performed by UI deserialize/serialize, not by mutating a legacy file during load.
- Tests extended in `sessionPersistence.test.ts`, `workspaceRestore.test.tsx`, and `daemon_persistence_contract.rs`.

## Evidence

- `A17-RED.log`: initial UI run has four expected failures for schema, ownership, incomplete inventory, and invalid paired entry; coordinator test fails on offline preload; credential projection test fails on injected sentinel; Rust backup test fails because the baseline writes without a backup. An initial Rust exact-name filter selected zero tests; the corrected full name subsequently executed and failed before implementation. The intermediate coordinator run also exposed SSH fixture incompatibility from overly broad validation; implementation was narrowed to paired validation and the complete final suites passed.
- `A17-GREEN.log`: required command `PATH=/Users/indo/.local/bin:/Users/indo/.bun/bin:/opt/homebrew/bin:/usr/bin:/bin bun run --cwd ui test src/lib/sessionPersistence.test.ts src/state/workspaceRestore.test.tsx` exits 0: 54 tests pass in one run. Node is v22.22.3.
- `A17-rust-test.log`: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test daemon_persistence_contract a17_pre_v3_backup_is_required_and_never_replaced -- --exact` exits 0. Tests a directory blocking backup, unchanged source after failure, exact backup bytes, repeat v3 save, and downgrade rejection. Uses a tempfile cleaned by RAII; no sockets or PTYs created.
- `A17-cargo-check.log`: required `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib` exits 0, with 19 existing warnings. No compile retry was necessary.
- Diagnostics initially returned no findings on all six changed source/test files. Refresh after the final target projection edit timed out on the two persistence TS files; do not interpret that timeout as fresh verification.
- `A17-ui-build.log`: UI build exits 2 on concurrent/out-of-packet errors: missing `pairedConnectionStatus` in Sidebar.tsx lines 692/871/876 and optional string passed as required string in pairedProjectWorktrees.ts line 17. No A17 errors were emitted. No unrelated fixes applied.

## Not proved / blockers

A17 is NOT complete. A15/A16 paired proxy descriptor/runtime and native integration were absent when inspected. I did not invent their descriptor format or implement terminal proxy operations. No paired descriptor persistence test, exact remote reattach, remote restart, graceful legacy-owner handover, explicit remote expiry UI, or real desktop/local-daemon/remote-daemon restart exercise is proved. The UI tests demonstrate binding retention and no spawn in the restore coordinator, not process survival or end-to-end no-create behavior.

Malformed paired workspace identity is rejected independently in UI deserialization; full Rust disk row quarantine (including arbitrary syntactically valid corrupt rows) is not implemented. The credential test proves projection of a paired target's extra fields, not a recursive credential audit of arbitrary old layout extras. Two-machine identical raw-session-ID recovery through native proxies remains unproved. These limitations prevent claiming the packet or release acceptance met.

Assumptions: native proxy IDs will be host/epoch-qualified by A15, local inventory cannot establish paired process death, and the desktop persistence path is a single writer as in the existing atomic replacement implementation. Hard-link backup fails closed on unsupported filesystems. Existing legacy backup differing from the current legacy file also fails closed rather than silently replacing rollback evidence.
