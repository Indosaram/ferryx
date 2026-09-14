# Review: Git worktree management and the ferryx scope layer (Rust)

Scope: `src-tauri/src/worktree/` (`manager.rs`, `git.rs`, `model.rs`, `mod.rs`),
`src-tauri/src/ferryx_scope/design/mod.rs`.
Reviewed-at: 2026-09-14
Reviewer: lead session (this lane's dag node did not deliver; findings below are the lead's own,
each verified by direct read)

## Findings

### [P1] `png` 0.18 migration left a production overflow path unhandled — FIXED

- Location: `src-tauri/src/ferryx_scope/design/mod.rs:61`
- Observed: `let mut decoded = vec![0; reader.output_buffer_size()];`. `Cargo.toml:90` pins
  `png = "0.18"`, and in 0.18 `Reader::output_buffer_size()` returns `Option<usize>`, not
  `usize`. Three sibling call sites were migrated (`src/clipboard_image.rs:323` with `.expect`,
  `src/native_terminal/png_decoder.rs:32` with `?`); this one and both sites in
  `tests/scoped_design.rs` (`:30`, `:57`) were not.
- Why it is wrong: the crate returns `None` exactly when the computed buffer size overflows
  `usize` — that is, on a hostile or malformed image. Since `crop_png` is reachable with
  caller-supplied image bytes, the pre-migration code would have aborted rather than
  returning an error. The file also did not compile, so the breakage was latent rather than
  live.
- Why it stayed hidden: neither `tests/scoped_design.rs` nor `tests/ssh_project_identity_live.rs`
  is built by `cargo test --lib`, the command `src-tauri/AGENTS.md` prescribes. Nothing in the
  routine loop compiles `tests/`.
- RED: `/tmp/ulw-massreview/browsercli-check.log:187-251` — three `E0308: mismatched types`
  (expected `usize`, found `Option<usize>`), plus four `E0609: no field 0..3 on type ProjectProbe`
  in `tests/ssh_project_identity_live.rs:46-49` from an unrelated tuple→named-struct refactor of
  `ProjectProbe` (`src/ssh/operations.rs:9-17`).
- Fix applied: production site propagates instead of panicking —
  `.ok_or_else(|| DesignError::Png("decoded png exceeds addressable size".into()))?`. This
  matches the module's existing error style (`DesignError::Png` is used at `:44`, `:47`, and the
  color-type arm). The two test sites use `.expect("decoded png size")`, which is correct in test
  context. `tests/ssh_project_identity_live.rs:46-49` switched to named field access.
- GREEN: `cargo check --manifest-path src-tauri/Cargo.toml --tests` → `TESTS_CHECK_EXIT=0`
  (`/tmp/ulw-massreview/tests-check.log`). Both previously-uncompilable targets now build.
- Note: this function already guards size two other ways — `ATTACHMENT_MAX_FILE_BYTES` at `:44`
  and a 32M-pixel ceiling at `:49`. The `None` case was the remaining hole in a defense that was
  otherwise complete.

### NO-FINDINGS above P3 for the worktree root jail — verified negative

The root jail was examined specifically for the symlink-escape bug and does not have it.

- Location: `src-tauri/src/worktree/manager.rs:261-267` (`canonical_allowed_path`), `:246-259`
  (`ensure_canonical_inside_root`), `:268+` (`validate_new_worktree_path`)
- Observed: `canonical_allowed_path` calls `validate_path_components`, **then**
  `fs::canonicalize`, **then** `ensure_canonical_inside_root`. Canonicalizing *before* the
  containment test is what makes it sound: symlinks and `..` are resolved first, so a symlinked
  worktree cannot pass `starts_with(&self.repo_root)` while actually pointing outside. The
  common exploitable inversion — compare the raw path, canonicalize afterwards — is not present.
- Creation is jailed separately: `validate_new_worktree_path` walks ancestors for a path that
  does not exist yet, where `canonicalize` would fail outright.
- Rejections surface as `WorktreeError::PathOutsideWorkspace`, mapped to
  `IpcErrorCode::PathOutsideWorkspace` with structured details at `src/ipc/error.rs:147-148`, so
  the frontend receives the typed contract rather than a string.
- Destructive-git surface is narrow: `rg 'reset --hard|clean|stash|checkout --'` across
  `src/worktree/` returns only `mod.rs:166` (a `path_for(&manager, "clean")` fixture *name*, not
  `git clean`) and `git.rs:387` (`--force` on `git worktree remove`).
- `git_worktree_remove` (`git.rs:379-392`) routes its path through
  `validate_git_path_argument` (`git.rs:63-81`), which rejects a leading `-` (option injection)
  and control characters, then passes an explicit `--` separator. Both defenses present.

## Summary

- P0: 0
- P1: 1 (fixed, RED→GREEN captured)
- P2: 0
- P3: 0
