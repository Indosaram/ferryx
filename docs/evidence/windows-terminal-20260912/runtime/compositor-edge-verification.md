# Compositor edge verification

Commands pinned before execution, from the repository root:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_child_surface_contract -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib attach_daemon_attachment_with_bounds_tolerates_invalid_initial_bounds -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib reattach_existing_session_with_bounds_tolerates_invalid_bounds -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib ghostty_grid_resize_notifies_pty_with_matching_dimensions -- --test-threads=1
```

Each library filter must run exactly one test; a zero-test run is not success.
No Windows launch or user-daemon operation is involved.

## Results

Monitor `mon_SMBNNTPWKXH4X8TR` / `bash_17` completed with exit 0.

- Child surface contract: 5 passed, 0 failed. Covers scaled geometry,
  non-positive extent rejection, negative origin clamping, hidden-until-present,
  and no resurrection after detach.
- `attach_daemon_attachment_with_bounds_tolerates_invalid_initial_bounds`:
  1 passed, 1034 filtered.
- `reattach_existing_session_with_bounds_tolerates_invalid_bounds`:
  1 passed, 1034 filtered.
- `ghostty_grid_resize_notifies_pty_with_matching_dimensions`:
  1 passed, 1034 filtered.

Existing compiler warnings were not suppressed: unused imports/fields in
notifications, unnecessary unsafe blocks in macos_file_drop, unused variables
in input/auth, unused mut in font_manager, and dead-code warnings in
remote/session/worktree modules. None of those files was changed by this
verification. Composition LSP reported no errors.

The extraction runner's synchronous git subprocess was changed to an awaited
Bun subprocess after its original successful RED/GREEN runs. `node --check`
passed after that tooling-only edit; the retained Rust specimens and result
receipts were not regenerated or changed. The final runner LSP refresh had
previously timed out, so no clean refreshed LSP result is claimed.
