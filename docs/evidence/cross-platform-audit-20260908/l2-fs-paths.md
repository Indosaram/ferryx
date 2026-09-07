### Open file handle held during atomic rename in SSH project store persistence causes sharing violation on Windows
- **ID**: L2-FS-PATHS-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/projects.rs:131` - `        std::fs::rename(&temp, &path)`
- **Why it breaks**: In `save()`, `let mut file = options.open(&temp)?;` holds an open file handle that remains in scope when `std::fs::rename(&temp, &path)` is called. On Windows NT, `MoveFileExW` fails with `ERROR_SHARING_VIOLATION` or `ERROR_ACCESS_DENIED` if any handle to the source file is open without delete-sharing flags. If rename fails, the cleanup `std::fs::remove_file(&temp)` also fails for the same reason, causing remote project saving to fail completely on Windows.
- **Fix**: Explicitly drop the file handle (`drop(file);`) after `file.sync_all()?;` and before calling `std::fs::rename(&temp, &path)`.
- **Status**: OPEN

### Open file handle held during atomic rename in ferryx_scope SSH config persistence fails on Windows
- **ID**: L2-FS-PATHS-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ferryx_scope/ssh/config.rs:50` - `            std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())`
- **Why it breaks**: In `ConfigStore::save()`, `let mut file = options.open(&tmp)...` keeps `file` in scope during `std::fs::rename(&tmp, &self.path)`. On Windows, attempting to rename an open file triggers a sharing violation error (`ERROR_SHARING_VIOLATION`), and the subsequent fallback `std::fs::remove_file(tmp)` also fails because the handle is still open.
- **Fix**: Explicitly drop `file` via `drop(file);` after `file.sync_all()` before calling `std::fs::rename(&tmp, &self.path)`.
- **Status**: OPEN

### Worktree and PTY containment check fails on Windows due to verbatim `\\?\` prefix mismatch
- **ID**: L2-FS-PATHS-3
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ferryx_scope/ssh/helper.rs:70` - `                if !cwd.starts_with(root) { return Err("FORBIDDEN: cwd outside project".into()); }`
- **Why it breaks**: On Windows, `std::fs::canonicalize` prepends the `\\?\` verbatim namespace prefix (e.g. `\\?\C:\repo`). Because `root` is not guaranteed to have the `\\?\` prefix, component-wise `starts_with(root)` returns `false`. This causes `pty.spawn` (and `worktree.create` at line 60) to unconditionally reject valid paths with `FORBIDDEN` on Windows.
- **Fix**: Strip verbatim prefixes before comparing (using `crate::worktree::git::strip_verbatim_prefix` or `crate::daemon::server::normalize_process_cwd`), or canonicalize both paths before calling `starts_with`.
- **Status**: OPEN

### Hardcoded `/tmp/ferryx-switch-debug.jsonl` fails debug log command and silently discards traces on Windows
- **ID**: L2-FS-PATHS-4
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/debug.rs:48` - `            .open("/tmp/ferryx-switch-debug.jsonl")`
- **Why it breaks**: `cmd_switch_debug_log` attempts to open `/tmp/ferryx-switch-debug.jsonl` directly. On Windows, the `/tmp` directory does not exist by default, causing the Tauri command to return an `IoError` to the frontend and fail. The same hardcoded path is also opened in `lib.rs` (lines 373, 426, 841) and `native_terminal.rs` (lines 1315, 1429).
- **Fix**: Replace `/tmp/ferryx-switch-debug.jsonl` with a platform-agnostic path using `std::env::temp_dir().join("ferryx-switch-debug.jsonl")` or `crate::daemon::server::get_runtime_dir().join("switch-debug.jsonl")`.
- **Status**: OPEN

### Worktree and branch ref validators permit NTFS-illegal characters and reserved DOS device names
- **ID**: L2-FS-PATHS-5
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/manager.rs:388` - `                    || matches!(ch, '~' | '^' | ':' | '?' | '*' | '[' | '\\')`
- **Why it breaks**: `validate_ref_component` rejects `~`, `^`, `:`, `?`, `*`, `[`, `\`, but permits `<`, `>`, `"`, and `|`, which are illegal characters on Windows NTFS. Furthermore, neither `validate_ref_component` nor `WorkspaceRegistry::validate_workspace_id` rejects DOS reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`). When `worktree_path_for` joins the workspace ID and slug into a filesystem path, directory creation or `git worktree add` fails on Windows with `ERROR_INVALID_NAME`.
- **Fix**: Extend `validate_ref_component` and `validate_workspace_id` to reject `<`, `>`, `"`, `|`, and check that no path segment case-insensitively matches Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`).
- **Status**: OPEN

### Workspace ID validator allows NTFS alternate data stream colons and illegal path characters
- **ID**: L2-FS-PATHS-6
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/registry.rs:30` - `            || workspace_id.contains('\\')`
- **Why it breaks**: `validate_workspace_id` only checks for `-`, `/`, `\`, control characters, and whitespace. It allows `:`, which on Windows NTFS denotes an Alternate Data Stream (ADS); joining `self.repo_root.join(".orca-worktrees").join(ws_id)` with a colon in `ws_id` targets an ADS instead of a directory, or fails with an invalid path syntax error.
- **Fix**: Add `:` and characters `< > " | ? *` to the invalid character check in `WorkspaceRegistry::validate_workspace_id`.
- **Status**: OPEN

### Remote auth credentials saved with Unix mode bits (0o600/0o700) and no Windows ACL restriction
- **ID**: L2-FS-PATHS-7
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/remote/auth.rs:300` - `        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600));`
- **Why it breaks**: Remote auth pairing secrets and device auth records are written using `std::os::unix::fs::PermissionsExt::from_mode`, which is cfg-gated to Unix. On Windows, file permissions are not set at all, leaving credentials inheriting default parent ACLs and readable by any unprivileged user on the local machine.
- **Fix**: Add a Windows branch using `icacls` or Win32 security descriptor APIs (similar to `crate::ferryx_scope::ssh::private_file`) to restrict ACLs on the auth directory and file to the current user SID (`%USERNAME%:(F)`).
- **Status**: OPEN

### SSH project store creation mode bit (0o600) has no Windows ACL equivalent
- **ID**: L2-FS-PATHS-8
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/projects.rs:126` - `            options.mode(0o600);`
- **Why it breaks**: `save()` configures `options.mode(0o600)` inside `#[cfg(unix)]`. On Windows, the store file containing sensitive SSH host configurations and project paths is created without restricted ACLs, making it readable by all users on a shared Windows machine.
- **Fix**: Apply ACL restrictions on Windows using `crate::ferryx_scope::ssh::private_file(&temp)` before writing sensitive SSH project data.
- **Status**: OPEN

### Process cwd lookup stub returns `None` on Windows, breaking agent session detection
- **ID**: L2-FS-PATHS-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/terminal.rs:980` - `        None`
- **Why it breaks**: `process_cwd` is implemented for Linux (`/proc/{pid}/cwd`) and macOS (`proc_pidinfo`/`lsof`), but returns `None` on all other operating systems. In `src-tauri/src/ipc/agents.rs`, `opencode_session_id` (line 342) and `pi_session_id` (line 448) use `process_cwd(...)?`, causing agent session detection for OpenCode and Pi to immediately return `None` on Windows.
- **Fix**: Implement `process_cwd` for Windows using Win32 `NtQueryInformationProcess` to read `ProcessParameters->CurrentDirectory.DosPath` from the target process PEB, or query `GetProcessInformation`.
- **Status**: OPEN

### Agent session discovery executes `/bin/ps` and `/usr/sbin/lsof` which do not exist on Windows
- **ID**: L2-FS-PATHS-10
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/agents.rs:72` - `    let output = crate::util::no_window_command("/bin/ps")`
- **Why it breaks**: `process_table_entries` executes `/bin/ps` and `lsof_session_id` (`src-tauri/src/ipc/agents.rs:193`) executes `/usr/sbin/lsof`. Neither binary exists on Windows, causing agent session discovery for Claude, Codex, Copilot, Cursor, Kimi, Omo, and Antigravity to silently fail. On Linux, `/usr/sbin/lsof` also fails on distributions where `lsof` is located in `/usr/bin/lsof`.
- **Fix**: On Windows, use Win32 process enumeration APIs (`CreateToolhelp32Snapshot`/`Process32Next`) instead of `/bin/ps`, and query open handles via `NtQuerySystemInformation`. On Unix, invoke `lsof` and `ps` via `PATH` lookup instead of hardcoding absolute paths.
- **Status**: OPEN

### Open process CWD locks prevent `git worktree remove` from deleting worktree directory on Windows
- **ID**: L2-FS-PATHS-11
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/manager.rs:551` - `        git_worktree_remove(&self.repo_root, &canonical, force)?;`
- **Why it breaks**: `remove_worktree_locked` invokes `git worktree remove`, which attempts to delete the worktree directory on disk. On Windows, the operating system locks directories that are the current working directory of any running process (such as an open terminal shell or child agent); `git worktree remove` fails with `Permission denied` even if `force` is true.
- **Fix**: Before calling `git_worktree_remove`, terminate any active terminal sessions whose working directory is inside the worktree, or implement retry logic with exponential backoff on Windows.
- **Status**: OPEN

### External URL launcher `cmd.exe /C start` misinterprets quoted URLs as window titles on Windows
- **ID**: L2-FS-PATHS-12
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/browser.rs:1604` - `            .args(["/C", "start", &valid_url])`
- **Why it breaks**: The Windows built-in `start` command treats its first quoted parameter as the optional console window title. When `valid_url` contains spaces, query parameters, or quotes requiring argument escaping, `cmd.exe /C start "<url>"` opens a blank command prompt titled with the URL instead of launching the default web browser. (Contrast with `cmd_open_file_path` at line 1639, which correctly passes an empty title `""`).
- **Fix**: Pass an empty string title argument before the URL: `.args(["/C", "start", "", &valid_url])`.
- **Status**: OPEN

### Ghostty config and theme discovery checks only Unix `HOME` and `XDG_CONFIG_HOME`, ignoring Windows `%APPDATA%`
- **ID**: L2-FS-PATHS-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/preferences.rs:619` - `    let home = env::var_os("HOME").map(PathBuf::from);`
- **Why it breaks**: `ghostty_config_candidates` and `theme_config_candidates` (line 495) only inspect `HOME`, `XDG_CONFIG_HOME`, and macOS application support directories. On Windows, Ghostty config and themes reside in `%APPDATA%\ghostty` (or `%LOCALAPPDATA%\ghostty`), and `HOME` is unset. Consequently, Ghostty configuration and themes cannot be discovered on Windows.
- **Fix**: Check `std::env::var_os("APPDATA")` on `#[cfg(windows)]` and add `%APPDATA%\ghostty\config` and `%APPDATA%\ghostty\themes` to `ghostty_config_candidates` and `theme_config_candidates`.
- **Status**: OPEN

### Handover manifest serialization sets Unix permissions without Windows ACL protection
- **ID**: L2-FS-PATHS-14
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/manifest.rs:40` - `            let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));`
- **Why it breaks**: In `HandoverManifest::save_to_path`, the temporary manifest file is secured using `fs::Permissions::from_mode(0o600)`, which is cfg-gated to Unix. On Windows, the file is saved with default inherited permissions, allowing other local user accounts on multi-user systems to read internal daemon handover route tokens and socket paths.
- **Fix**: Add a `#[cfg(windows)]` branch setting a restricted security descriptor or calling `crate::ferryx_scope::ssh::private_file(&tmp_path)`.
- **Status**: OPEN

### Unit test `terminal_process_cwd_resolves_accurately` unconditionally asserts `process_cwd` succeeds
- **ID**: L2-FS-PATHS-15
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/tests.rs:607` - `    assert!(resolved.is_some(), "process_cwd should resolve current pid");`
- **Why it breaks**: Test-only defect: `terminal_process_cwd_resolves_accurately` tests `process_cwd(current_pid)` without a `#[cfg(unix)]` gate. Because `process_cwd` returns `None` on Windows (`src-tauri/src/ipc/terminal.rs:980`), this test panics with an assertion failure whenever `cargo test` is executed on Windows.
- **Fix**: Gate the test with `#[cfg(any(target_os = "linux", target_os = "macos"))]`, or update `process_cwd` with a Windows implementation.
- **Status**: OPEN

### Parent directory fsync via `File::open` fails on Windows during session persistence
- **ID**: L2-FS-PATHS-16
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/session/mod.rs:226` - `        if let Ok(dir_file) = File::open(parent) {`
- **Why it breaks**: `save_session_to_path` attempts to fsync the parent directory to flush directory entry metadata by calling `File::open(parent)`. On Windows, standard `File::open` invokes `CreateFileW` without `FILE_FLAG_BACKUP_SEMANTICS`, which always fails with `PermissionDenied` when opening a directory handle.
- **Fix**: Gate directory fsync with `#[cfg(unix)]` or `#[cfg(not(windows))]`, as directory fsync is POSIX-specific and Windows metadata flushing is handled by the filesystem driver upon file handle closure.
- **Status**: OPEN
