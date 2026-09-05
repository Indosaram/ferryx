# Remote inventory: desktop-independent registered sessions

## RED captured before production edit

Command: `cargo test --manifest-path src-tauri/Cargo.toml --lib scoped_remote_inventory_lists_registered_projects_without_desktop -- --nocapture`

Monitor mon_3W8TND7PA23H53YE / bash_52 compiled successfully, then exited 101:
```text
assertion `left == right` failed: Authenticated remote inventory must list both registered projects without desktop selection
  left: []
 right: ["9b002c83-cc71-482b-967a-d26e4d97cb12", "c28e879e-aaf6-415f-b552-33464fb0ff5f"]
test remote::tests::scoped_remote_inventory_lists_registered_projects_without_desktop ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 557 filtered out; finished in 2.37s
```

This is a real loopback HTTP call against create_remote_router with paired View auth and two real registered-project PTYs. It does not call a mocked listing function. No desktop selection exists. Listener and PTYs were stopped before assertions; temporary repos are RAII-owned. Initial attempt had API-name compiler errors and is not counted as RED.

## Production change and pending proof

`src-tauri/src/remote/server.rs` get_active_running_sessions enumerates backend sessions, filters running and registered workspace membership, derives host metadata without exposing paths and retains selected-session metadata for legacy entries. Auth endpoint checks remain unchanged. Foreign attention helper edit was preserved.

GREEN command: `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::tests:: -- --nocapture`, monitor mon_GXK9J296VXFX9GX4 / bash_55. Result pending. This increment is only list visibility; independent attach/control, global waiting and mobile UI remain unfinished.

## GREEN and adjacent regression

Initial GREEN made the new scenario pass but exposed a legacy active-peer listing regression (38/39). Its existing test uses an active legacy session whose workspace is not in this runtime registry. Retained that explicit active-selection compatibility path; background sessions still require registered workspace membership. No legacy test was weakened.

Re-run same command, monitor mon_MJ3SZ63H24D1Q25V / bash_57: exit0, 39 passed, 0 failed, runtime2.49s. Includes actual loopback API/PTYS regression, pairing/revocation, grid rendering, selection change and legacy attach/write/output/exit. All test processes exited; new test cleans PTYs/listener before assertions. LSP unavailable; Rust compiler and tests ran against real library.
