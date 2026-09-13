# Q4 platform repair batch

Status: open; neither platform is accepted.

## Windows compiler repair

The original Windows command exited 101 with E0433 at
`src-tauri/src/clipboard_image.rs:127-128`. The Windows clipboard reader
referenced constants inside the optional native-terminal IPC module.
The definitions inspected in `src-tauri/src/ipc/native_terminal.rs:138-141`
have values 8 and 17.

The parent changed only `src-tauri/src/clipboard_image.rs`: it defines those
two standard format IDs inside the Windows clipboard function and compares
against them directly. The image selection and conversion behavior is unchanged.
This is a compilation repair, not behavioral RED or a clipboard runtime pass.

Before editing, the file had no existing diff. Git status needed
`-c diff.ignoreSubmodules=all` because the inherited Ghostty submodule path
is a symlink; the symlink was not changed.

Parent checks:

- `git -c diff.ignoreSubmodules=all diff --check -- src-tauri/src/clipboard_image.rs`
  exited 0.
- LSP reported only inactive-code hints. Windows code is inactive on the
  Darwin LSP host, so this does not prove Windows compilation.
- The actual diff contains only the two constants, their feature-boundary
  comment and the two reference replacements.

Task `st_01a0984d` was resumed to transfer only this delta into its retained
private Windows snapshot, preserve all original evidence, run the native
compiler and then the previously blocked tests. No success is assumed.

## Linux readiness failures

The Linux report records a successful headless build, 44 worktree tests and
21 additional integration tests passing, but three remote-suite failures and
one catalog-suite failure. The suspected libtest stdout prefix issue remains
unproven.

Task `st_01a0984c` was resumed independently to capture actual child stdout,
establish causality, and produce a minimal candidate repair in its private
snapshot. The parent source and frozen Wave1 remain read-only for that task.
Crash/restart assertions must remain intact; no retry-to-green or timeout
increase is accepted as a repair.

Both tasks must return source hashes, command exits and owned-resource cleanup
receipts. Original failed artifacts remain authoritative. Neither repair
constitutes final A23, platform, native desktop or full-plan acceptance.

The parent clipboard change and this report are uncommitted.
