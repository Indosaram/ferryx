# V06 parent acceptance

Accepted scope: deny View resize in both legacy raw and real Ghostty grid WebSocket receive paths. Control resize, initial viewport policy, SSH generation admission, scrolling, and active selection remain unchanged.

## Parent-executed evidence

All commands ran in the isolated `herdr-wave0-clean` checkout with its existing private `src-tauri/target` and `CARGO_BUILD_JOBS=4`. Ambient A03 fixture and machine-token/relay overrides were unset. Fixtures allocate their own private roots and loopback ports; no user daemon was launched, killed, or restarted.

- RED: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::tests::security::resize::v06_ -- --nocapture --test-threads=1`, exit 101, two failures. Both saved View backend observations were 51x17 instead of 80x24 after successful Control backend and child checks. See `V06-parent-red.log`.
- Initial GREEN: identical command, exit 0, two passes. See `V06-parent-green.log`.
- Related headless regression: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture`, exit 0, 212 passes. See `V06-headless-remote.log`.
- Default-feature regression: `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib remote:: -- --nocapture`, exit 0, 218 passes.
- Headless executable check: `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`, exit 0. Both preceding commands are captured in `V06-default-and-check.log`.
- Final test-only cleanup refinement: identical focused command, exit 0, two passes in 0.56 seconds. See `V06-cleanup-green.log`. No production changes followed the full regressions.

The new module and registration had clean error diagnostics. The production server diagnostic request timed out waiting for freshness; actual compilation and tests passed. Existing compiler warnings remain in the complete logs and were not suppressed.

## Cleanup and source verification

Final fixture PIDs 53949 and 53954 were explicitly closed and reaped, and both output pumps joined. Parent `ps -p 53949,53954 -o pid=,ppid=,command=` returned no rows. Parent existence checks confirmed both recorded temporary roots were removed:

- `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpeoDhYM`
- `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpsjEas5`

The listener owner is stopped and awaited before PTY cleanup. Test setup after successful spawn is inside the unwind boundary. Pairing precedes allocation. Cleanup attempts are collected before assertions so one failure does not skip the remaining removals.

Parent inspected all source changes. `server.rs` has exactly six added permission-guard lines. The new fixture's final SHA-256 is `5d9d773045ef88a1ceeaad6f76730499054e2ef6c68e1e991a47faebabb99986`; server SHA-256 is `406026428fc8f30356a0cb5eccf3b65383b6f5b7c8bbbd2de560a557e53a6e27`. Registration adds only the focused module.

Independent reviewer `st_01a093d4` approved the authorization fix and then the cleanup refinement. Both reports are retained. Its review is source/evidence review, not a separately executed test run.

## Limits

The real PTY tests are under the existing Unix test-module gate and were executed on macOS. They do not prove Windows or Linux runtime behavior, native desktop pixels, forced-relay machine APIs, or A04-A24. The Close/EOF barrier is valid for this controlled fixture, not a general protocol acknowledgment. Process aborts and internal spawn failures are not claimed covered.

Log trailing spaces were normalized for Git whitespace hygiene; output content, assertions, warnings, and exit statuses were retained. V05 UI work is excluded from this increment and remains uncommitted pending its separate acceptance.
