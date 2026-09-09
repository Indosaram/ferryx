# SSH trust baseline

Captured before Phase B setup traffic. No key contents were read or changed.

- `/Users/indo/.ssh/known_hosts`
  - SHA-256: `cf5b6b029f49021c02ceaddcf07945b5a80f641953ba5c3520fdbceadf53c076`
  - Modification time: `2026-09-08T08:28:54+0900`, before this goal began.
- `/Users/indo/.ssh/config`
  - SHA-256: `e388a32ee80dc7fe85819d9753aaca95cc586ec650808b3a35459af0cd5612b9`
  - Modification time: `2026-09-07T09:47:45+0900`, before this goal began.

`ssh -G omarchy` reports `updatehostkeys true`. Strict host checking alone does
not disable automatic updates to existing trusted-host entries. The direct SSH
plan therefore needs an explicit `UpdateHostKeys=no` option for both interactive
and automated commands. The regression is
`ssh_reconnect_safety_plan_does_not_update_host_keys` in `direct_tests.rs`.

Final real-SSH scenarios must compare trust-file hashes before and after their
own runs. This baseline is not a substitute for those scenario-specific checks.

## Guard verification

- RED: `ssh-trust-red.log`; the new option assertion failed before the change.
- GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::direct::tests -- --nocapture`
  passed all 12 tests in 0.10 seconds (`ssh-trust-green.log`).
- Real CLI: `ssh -G -o UpdateHostKeys=no omarchy` reported `updatehostkeys false`.
- Recomputed known-host and config SHA-256 values matched the baseline exactly.
- The compiler emitted 16 existing warnings in unrelated file-drop, native
  terminal, notification, session and worktree code. They were retained in the
  raw log, not suppressed or represented as a warning-free build.
