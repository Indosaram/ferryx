# rorca native worker receipt

Scope: `src-tauri/**` native contract implemented in Generation 1 under scope `9f11cd31-f40a-4079-82c8-c3991c2c6707`. This Generation 2 follow-up writes only this evidence receipt; it does not edit source, tests, config, assets, or git state.

## RED evidence captured before native implementation

### 1. rorca product/window identity and icon metadata

Command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract tauri_metadata_uses_rorca_identity_and_generated_icons -- --exact
```

Command ID: `5f53d387-baa2-4251-848b-45c5b9dce8cb`

Exit code: `101`

Verbatim decisive output observed:

```text
running 1 test
test tauri_metadata_uses_rorca_identity_and_generated_icons ... FAILED

failures:

---- tauri_metadata_uses_rorca_identity_and_generated_icons stdout ----

thread 'tauri_metadata_uses_rorca_identity_and_generated_icons' (145595618) panicked at tests/rorca_native_contract.rs:9:5:
assertion `left == right` failed
  left: String("Orca Lite")
 right: "rorca"
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tauri_metadata_uses_rorca_identity_and_generated_icons

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

This proved the pre-implementation Tauri metadata still exposed the legacy `Orca Lite` identity.

### 2. Ghostty parser and terminal-preferences contract

Command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract ghostty_parser_is_last_wins_and_reads_macos_option_as_alt -- --exact
```

Command ID: `4185dc38-9cc4-4fe9-a8af-935f715782f7`

Exit code: `101`

Verbatim decisive compiler output observed:

```text
error[E0432]: unresolved imports `orca_lite_lib::terminal::load_terminal_preferences_from_path`, `orca_lite_lib::terminal::parse_ghostty_config`, `orca_lite_lib::terminal::TerminalPreferencesSource`, `orca_lite_lib::terminal::TerminalPreferencesStatus`, `orca_lite_lib::terminal::DEFAULT_TERMINAL_FONT_FAMILY`
 --> tests/rorca_native_contract.rs:2:5
  |
2 |     load_terminal_preferences_from_path, parse_ghostty_config, TerminalPreferencesSource,
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^^^^^^^^^^ no `TerminalPreferencesSource` in `terminal`
  |     |                                    |
  |     |                                    no `parse_ghostty_config` in `terminal`
  |     no `load_terminal_preferences_from_path` in `terminal`
3 |     TerminalPreferencesStatus, DEFAULT_TERMINAL_FONT_FAMILY,
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ no `DEFAULT_TERMINAL_FONT_FAMILY` in `terminal`
  |     |
  |     no `TerminalPreferencesStatus` in `terminal`

For more information about this error, try `rustc --explain E0432`.
error: could not compile `orca-lite` (test "rorca_native_contract") due to 1 previous error
warning: build failed, waiting for other jobs to finish...
```

This proved the typed Ghostty parser/defaults API did not exist before implementation.

### 3. Git project registration and local-branch IPC contract

Command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract project_registration_returns_canonical_root_and_lists_local_branches -- --exact
```

Command ID: `e157cd92-1240-4ad8-a1af-b38cd124242f`

Exit code: `101`

Verbatim decisive compiler output observed:

```text
error[E0432]: unresolved imports `orca_lite_lib::ipc::cmd_project_branches`, `orca_lite_lib::ipc::cmd_project_register`, `orca_lite_lib::ipc::ProjectBranchesRequest`, `orca_lite_lib::ipc::RegisterProjectRequest`
 --> tests/rorca_native_contract.rs:2:5
  |
2 |     cmd_project_branches, cmd_project_register, IpcErrorCode, ProjectBranchesRequest,
  |     ^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^^^^^                ^^^^^^^^^^^^^^^^^^^^^^ no `ProjectBranchesRequest` in `ipc`
  |     |                     |
  |     |                     no `cmd_project_register` in `ipc`
  |     no `cmd_project_branches` in `ipc`
3 |     RegisterProjectRequest,
  |     ^^^^^^^^^^^^^^^^^^^^^^ no `RegisterProjectRequest` in `ipc`
```

The same compile also still reported the unresolved Ghostty imports because both new contracts were present in the same failing-first integration test file. The project-specific unresolved imports above are the direct RED proof for this requirement.

## Generation 1 worker-owned changed files

The native worker edited or created exactly these implementation/test files:

1. `src-tauri/tauri.conf.json`
2. `src-tauri/src/lib.rs`
3. `src-tauri/src/ipc/mod.rs`
4. `src-tauri/src/ipc/preferences.rs`
5. `src-tauri/src/ipc/project.rs`
6. `src-tauri/src/terminal/mod.rs`
7. `src-tauri/src/terminal/preferences.rs`
8. `src-tauri/tests/rorca_native_contract.rs`

Generated icon binaries under `src-tauri/icons/**` were already modified/untracked at the Generation 1 baseline. The native worker did not regenerate or alter those binaries; it only referenced the existing generated desktop assets from `tauri.conf.json`.

## Final native contracts

### Tauri identity and desktop icons

`src-tauri/tauri.conf.json` now exposes:

```text
productName = "rorca"
app.windows[0].title = "rorca"
```

Bundle icon list, in order:

```text
icons/32x32.png
icons/128x128.png
icons/128x128@2x.png
icons/icon.icns
icons/icon.ico
```

The bundle identifier was intentionally not changed by this task and remains `com.orca.lite`.

### Terminal preferences / Ghostty IPC

Tauri command:

```text
cmd_terminal_preferences
```

No request payload is required. Return type is serialized with camelCase fields:

```ts
type TerminalPreferences = {
  fontFamily: string;
  macosOptionAsAlt: boolean;
  source: "defaults" | "ghostty";
  status: "imported" | "absent" | "malformed";
  sourcePath: string | null;
};
```

Safe defaults are:

```text
fontFamily = "monospace"
macosOptionAsAlt = false
```

Parser contract:

- Blank lines and `#` comment lines are ignored.
- Supported keys are `font-family` and `macos-option-as-alt`.
- Repeated supported keys are deterministic last-wins; therefore the final repeated `font-family` value wins.
- `macos-option-as-alt` accepts case-insensitive `true` or `false` and becomes a typed boolean.
- Empty supported values or malformed supported syntax produce a parse error.
- Unknown keys are ignored; they are never executed or rewritten.
- If the selected config file is absent, preferences fall back to safe defaults with `source="defaults"`, `status="absent"`.
- If file reading/parsing is malformed, partial imported values are discarded and safe defaults are returned with `source="defaults"`, `status="malformed"`.
- A valid parsed file returns `source="ghostty"`, `status="imported"`; any missing supported key independently falls back to its safe default.
- File I/O is exposed through `cmd_terminal_preferences` using the existing `run_blocking` helper so blocking filesystem work is not performed directly on the async caller thread.

Config candidate order implemented by the native loader is:

1. On macOS: `~/Library/Application Support/com.mitchellh.ghostty/config`
2. `$XDG_CONFIG_HOME/ghostty/config` when `XDG_CONFIG_HOME` exists
3. `~/.config/ghostty/config`

The first existing candidate is loaded.

### Git project registration IPC

Tauri command:

```text
cmd_project_register
```

Frontend payload shape:

```ts
{
  request: {
    workspaceId: string;
    repoPath: string;
  }
}
```

Return shape:

```ts
type RegisteredProject = {
  workspaceId: string;
  repoRoot: string;
};
```

Safety contract:

- Registration delegates to the existing `WorkspaceRegistry::register` boundary.
- That boundary constructs `WorktreeManager::try_new`, canonicalizes the supplied path, verifies it is a Git repository, resolves `git rev-parse --show-toplevel`, canonicalizes that Git top-level, and stores the manager under the validated workspace ID.
- A nested directory inside a valid repository therefore returns the canonical repository top-level as `repoRoot`.
- A non-Git path returns the existing structured `INVALID_REPO_ROOT` IPC error.
- Existing workspace-ID validation and duplicate-registration behavior are preserved.

### Local branch-list IPC

Tauri command:

```text
cmd_project_branches
```

Frontend payload shape:

```ts
{
  request: {
    workspaceId: string;
  }
}
```

Return shape:

```ts
type LocalBranch = {
  name: string;
  isCurrent: boolean;
};
```

Safety/behavior contract:

- The branch command accepts only a registered `workspaceId`; it does not accept a raw filesystem path.
- The native implementation resolves the registered `WorktreeManager` and runs fixed Git queries against `manager.repo_root()`.
- Local branches are read from `refs/heads/` using `git for-each-ref --sort=refname --format=%(refname:short) refs/heads/`.
- Current branch is obtained with `git branch --show-current`.
- Returned branch records are sorted lexically by `name`; exactly the matching current local branch is marked `isCurrent=true` when HEAD is attached.
- Querying an unregistered workspace returns the existing structured `WORKSPACE_NOT_FOUND` IPC error.

### Tauri handler registration

`src-tauri/src/lib.rs` registers the three new commands alongside the pre-existing terminal/worktree commands:

```text
cmd_terminal_preferences
cmd_project_register
cmd_project_branches
```

No existing terminal spawn/write/resize/signal/close/list or worktree create/delete/status command was removed.

## GREEN evidence and final gates

### Focused native contract GREEN

Command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract
```

Command ID: `2cb30c70-df3c-4ecb-8a64-f35e84b1871d`

Exit code: `0`

Observed result:

```text
running 5 tests
test tauri_metadata_uses_rorca_identity_and_generated_icons ... ok
test ghostty_parser_is_last_wins_and_reads_macos_option_as_alt ... ok
test terminal_preferences_use_safe_absent_and_malformed_defaults ... ok
test project_registration_rejects_non_git_roots_and_unregistered_branch_queries ... ok
test project_registration_returns_canonical_root_and_lists_local_branches ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.49s
```

### Final full Cargo test gate

Final revision-fresh command:

```text
cargo test --manifest-path src-tauri/Cargo.toml
```

Command ID: `9a4affaf-70be-4980-b9eb-b47694a5c470`

Workspace revision: `12`

Exit code: `0`

Observed suite result groups were all green:

```text
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

That is 53 executed Rust tests with zero failures, plus empty unit/doc-test groups. The passing groups include the pre-existing terminal lifecycle, writer-lease, dirty-worktree, unmerged-branch, worktree isolation, IPC hardening, and agent-workflow safety coverage.

### Final Clippy gate

Final revision-fresh command:

```text
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Command ID: `ce36ee4a-2847-4d53-93b7-9bd31b0a985b`

Workspace revision: `12`

Exit code: `0`

Observed output:

```text
    Checking orca-lite v0.1.0 (/Users/indo/code/project/orca-lite/src-tauri)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.67s
```

### Rust-analyzer diagnostics

Final diagnostics returned `diagnostics: []` for the changed/new Rust surfaces that were checked, including `src-tauri/src/lib.rs`, `src-tauri/src/ipc/mod.rs`, `src-tauri/src/ipc/preferences.rs`, `src-tauri/src/ipc/project.rs`, `src-tauri/src/terminal/mod.rs`, `src-tauri/src/terminal/preferences.rs`, and `src-tauri/tests/rorca_native_contract.rs` after module-tree refresh.

## Residual native risk / intentionally unresolved boundaries

1. **Bundle identifier remains legacy.** Product/window identity is `rorca`, but `identifier` remains `com.orca.lite`. Changing the application identifier was not part of the native contract implemented here and may matter for final packaging/product identity policy.
2. **Generated icons were not visually validated by this worker.** The icon files existed as pre-existing workspace changes before Generation 1. Native tests prove the configured files exist and are wired, not that final OS rendering, masks, signing, or platform-specific appearance is correct.
3. **Ghostty import intentionally supports a narrow subset.** Only `font-family` and `macos-option-as-alt` are interpreted. It does not implement Ghostty include/import semantics, broader Ghostty option types, or merging across multiple candidate config files.
4. **No persistent native rorca-local override store was added.** The native command reports imported/default effective values. If a frontend/local rorca preference is intended to override Ghostty, that precedence must be applied by the owning settings layer or a later persistent native-settings contract.
5. **Malformed-file handling is fail-safe rather than partial.** A malformed supported config causes the whole imported subset to fall back to defaults. This avoids unsafe/ambiguous partial imports but may discard an otherwise valid supported value in the same file.
6. **Workspace registration is process-memory state.** `WorkspaceRegistry` registration is not persisted by this native change; a project must be registered again after application restart unless another layer persists and restores it.
7. **Branch listing is deliberately local-only.** It returns `refs/heads/` only, not remote-tracking refs. On detached HEAD, no returned branch is marked current.
8. **Repository-wide rustfmt remained a pre-existing quality issue.** `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` returned exit `1` because many pre-existing native files had formatting drift. The worker did not run global formatting because that would modify unrelated native code; files newly added by the worker were adjusted to the formatter output where reported. The required final `cargo test` and `cargo clippy ... -D warnings` gates were rerun successfully after the final native mutation.

## Completion record

Generation 1 completion audit returned `ready=true` at workspace revision `12` with no blockers. This receipt is documentary only and does not alter the native implementation, tests, config, assets, or git state.
