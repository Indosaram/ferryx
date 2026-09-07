# Lane 8 — Shell Resolution, PTY, Agent Launching, Env/PATH, Git

### Agent CLI binaries unresolvable on Windows (no PATHEXT/.cmd/.ps1 lookup)
- **ID**: L8-SHELL-AGENT-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/agents.rs:577` — `.find(|candidate| is_executable_file(candidate))`
- **Why it breaks**: `resolve_binary` only tests `dir.join(name)` for exact-name existence. On Windows, npm/npx-installed agent CLIs (claude, codex, opencode, cursor-agent, etc.) install as `claude.cmd`/`claude.ps1` shims, never a bare `claude` or `claude.exe`. `is_executable_file` for `cfg(not(unix))` (line 589) is just `path.is_file()` with no PATHEXT (`.COM;.EXE;.BAT;.CMD;.PS1`) suffix search, so `detect_agents` reports every agent as unavailable and `resolve_startup_command` (`src-tauri/src/terminal/pty.rs`-referenced `shell.rs:421-425`) falls through to the raw name, which `CreateProcessW` (used directly by portable-pty, no shell involved) cannot find either.
- **Fix**: In `resolve_binary`/`is_executable_file` (`src-tauri/src/ipc/agents.rs`), on `cfg(windows)` iterate `%PATHEXT%` (or a hardcoded `[".exe", ".cmd", ".bat", ".ps1", ".com"]` list) appending each extension to `name` before testing `dir.join(candidate)`, and return the matched extended path so downstream `CommandBuilder::new` receives a runnable file.
- **Status**: OPEN

### Agent resume/session commands invoke bare program names via ConPTY CreateProcess, bypassing shell/PATHEXT resolution
- **ID**: L8-SHELL-AGENT-2
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/shell.rs:428` — `let mut cmd = CommandBuilder::new(&plan.program);`
- **Why it breaks**: `resolve_agent_resume_plan` (e.g. `"claude" => program: "claude"`) hands the bare name to `CommandBuilder::new`, then `resolve_startup_command` tries `resolve_binary` (see finding 1) and, if that also fails to extend for `.cmd`/`.ps1`, passes the unresolved bare string straight to portable-pty's Windows backend, which calls `CreateProcessW` directly (confirmed in `portable-pty-0.9.0/src/win/psuedocon.rs`). `CreateProcessW` does not search `PATHEXT` or invoke `cmd.exe`'s command-shim resolution the way a real Windows shell does, so `.cmd`/`.ps1` shims never launch even when nominally "on PATH".
- **Fix**: When the resolved binary (after fix #1) still ends in `.cmd`/`.bat`, build the `CommandBuilder` as `cmd.exe /d /s /c "<resolved path>" <args...>` (or `powershell.exe -File` for `.ps1`) instead of invoking the shim path directly, matching how Node's `child_process` and VS Code's terminal do Windows shim execution.
- **Status**: OPEN

### Linux SHELL-unset fallback hardcodes /bin/bash, which does not exist on many distros
- **ID**: L8-SHELL-AGENT-3
- **Severity**: MEDIUM
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/terminal/shell.rs:356` — `.unwrap_or_else(|| "/bin/bash".to_string());`
- **Why it breaks**: When `SHELL` is unset (common for minimal containers, some display managers, and musl-based distros such as Alpine, which ships `/bin/ash`/`busybox sh` and no `/bin/bash` by default), `resolve_shell_command_pure` hard-codes `/bin/bash`, and `CommandBuilder::new("/bin/bash")` fails to spawn since the path does not exist, leaving the user with no terminal pane and a raw spawn error instead of a working shell.
- **Fix**: In `resolve_shell_command_pure`'s `TargetPlatform::Linux` branch, fall back through a candidate list (`/bin/bash`, `/usr/bin/bash`, then `/bin/sh`) using the existing `is_executable_on_path` probe before defaulting, mirroring the `is_on_path`-driven pwsh/powershell selection already used in the Windows branch (`shell.rs:315-320`).
- **Status**: OPEN

### Agent session discovery (ps/lsof) is unconditionally Unix-only, silently disabled on Windows
- **ID**: L8-SHELL-AGENT-4
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/agents.rs:72` — `let output = crate::util::no_window_command("/bin/ps")`
- **Why it breaks**: `process_table_entries` (feeding `discover_agent_session_id`, called unconditionally from `src-tauri/src/daemon/server.rs:1299` on every platform with no `cfg` gate) shells out to the absolute path `/bin/ps`, which does not exist on Windows. `omo_session_id_from_environment` (`src-tauri/src/ipc/agents.rs:173`) and `lsof_session_id` (`src-tauri/src/ipc/agents.rs:193`, `/usr/sbin/lsof`) have the same problem. On Windows every call fails to spawn, `.ok()` swallows the error, and agent-session auto-discovery (mapping a running PTY to its provider session id for resume) silently returns `None` for every agent type, degrading a core "reconnect to an in-flight agent" feature with no user-visible diagnostic.
- **Fix**: Add a `cfg(windows)` implementation of `process_table_entries`/`lsof_session_id` backed by `CreateToolhelp32Snapshot`/`Process32Next` (via the `windows` or `sysinfo` crate already usable elsewhere) to enumerate the process tree, and reimplement `lsof_session_id`'s "find an open file path matching a marker" step via `NtQuerySystemInformation`/`sysinfo` open-handle inspection or by having each agent report its session file path directly instead of relying on `lsof`.
- **Fix**: (if Windows support of this feature is deliberately deferred) gate the call sites with `#[cfg(unix)]` and return `None` immediately on Windows with a `tracing::debug!` note, so the gap is documented in code rather than silently inherited from a nonexistent binary.
- **Status**: OPEN

### HOME-only home-directory lookup breaks agent-extension install and `~` path expansion on Windows
- **ID**: L8-SHELL-AGENT-5
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/agent_extension.rs:24` — `fn home_dir() -> Option<PathBuf> {`
- **Why it breaks**: `home_dir()` reads only `env::var_os("HOME")`. Windows does not set `HOME` for GUI-launched processes by default (it sets `USERPROFILE`, and only some shells like Git Bash synthesize `HOME`), so `extension_dirs()` returns an empty `Vec` and `install_into` never runs: the Ferryx agent-state lifecycle extension (`ferryx-agent-state.ts`) is never installed into `.omo/.pi/.omp` agent directories on Windows, silently disabling authoritative agent-state reporting in favor of screen-scraping inference. The same `HOME`-only pattern reappears at `src-tauri/src/ipc/browser.rs:1626` for `~/`-prefixed path expansion in `cmd_open_file_path`, so a `~/foo` path typed/pasted by a user on Windows resolves to a bogus relative path instead of the user's profile directory.
- **Fix**: Add a small `home_dir()` helper (or reuse one) that tries `HOME` first and falls back to `USERPROFILE` on `cfg(windows)`, e.g. `env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))`, and use it at both `agent_extension.rs:24` and `browser.rs:1626`.
- **Status**: OPEN

### git worktree remove has no retry for Windows file-locking, breaking worktree/session cleanup
- **ID**: L8-SHELL-AGENT-6
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/git.rs:390` — `run_git(repo_root, &args)?;`
- **Why it breaks**: `git_worktree_remove` runs `git worktree remove [--force] -- <path>` exactly once with no retry/backoff. Windows cannot delete or rename a file while any process holds an open handle to it (unlike POSIX unlink-while-open semantics on macOS/Linux). Because `spawn_in_worktree` (`src-tauri/src/terminal/pty.rs:85`) routinely leaves PTY/agent child processes with a working directory or open log file inside `.orca-worktrees/<ws>/<task>`, a `remove_worktree` call issued right after closing a pane races the OS releasing those handles; git's `unlink`/`rmdir` fails with `ERROR_SHARING_VIOLATION`/`ERROR_ACCESS_DENIED`, and `run_git` returns `WorktreeError::GitError` with no automatic recovery, leaving the worktree stuck and the UI action failed.
- **Fix**: In `git_worktree_remove` (`src-tauri/src/worktree/git.rs:379`), on `cfg(windows)` wrap the `run_git` call in a bounded retry loop (e.g. 3-5 attempts with short backoff, similar to the existing `TERM_GRACE_TIMEOUT`/`KILL_REAP_TIMEOUT` pattern in `src-tauri/src/terminal/pty.rs`) and ensure the owning `PtySession`'s child/master/writer handles are fully closed (`close_io`) and the process reaped before the removal is attempted.
- **Status**: OPEN

### PTY kill on Windows only terminates the direct child, leaving orphaned grandchild processes
- **ID**: L8-SHELL-AGENT-7
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/session.rs:244` — `TerminalSignal::Terminate | TerminalSignal::Kill => self.kill(),`
- **Why it breaks**: The `cfg(unix)` `signal()` path (`session.rs:229`) signals the whole process group via `libc::kill(-(pid as i32), sig)`, but the `cfg(not(unix))` path (`session.rs:240-246`) maps both `Terminate` and `Kill` to `self.kill()`, which delegates to portable-pty's Windows `Child::kill()` — confirmed in `portable-pty-0.9.0/src/win/mod.rs` to be a single `TerminateProcess` call on the direct child handle only, with no Job Object grouping. Windows has no process-group equivalent of POSIX PGIDs reachable this way, so when an agent CLI spawns further children (e.g. a `cmd.exe`-shimmed `claude.cmd` spawning `node.exe`, or any agent that forks helper processes), closing the pane kills only the top-level shim and leaves grandchildren running as orphans consuming resources and holding file locks (compounding finding 6).
- **Fix**: Spawn Windows child processes inside a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (via `windows-sys`'s `CreateJobObjectW`/`AssignProcessToJobObject`/`SetInformationJobObject`) at PTY-spawn time in `spawn_with_id_and_worktree` (`src-tauri/src/terminal/pty.rs:117`), and have the `cfg(not(unix))` `signal()`/`kill()` path in `session.rs` terminate the job object instead of just the child, so all descendants die together.
- **Status**: OPEN

### Agent-launched child processes do not inherit the PATH augmentation on GUI launch outside macOS's Homebrew case
- **ID**: L8-SHELL-AGENT-8
- **Severity**: LOW
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/terminal/pty.rs:157` — `if let Ok(augmented) = std::env::join_paths(crate::ipc::agents::search_paths()) {`
- **Why it breaks**: `search_paths()`/`login_shell_path()` (`src-tauri/src/ipc/agents.rs:553-567`) source PATH augmentation by invoking `$SHELL -lic 'printf %s "$PATH"'`. This generically covers Linux too when `SHELL` is set, but many Linux desktop-launcher paths (systemd user services, `.desktop` Exec= entries, some app-image/snap wrappers) do not set `SHELL` in the environment they hand to a GUI-launched daemon, unlike macOS's `launchd`, which reliably preserves `SHELL` from the user's `dscl` record. In that case `login_shell_path()` returns `None` silently and the daemon is stuck with whatever minimal PATH its launcher gave it (frequently missing `~/.local/bin`, `~/.cargo/bin`, nvm/asdf shims), so agent binaries installed via user-level package managers are invisible to `detect_agents`/`resolve_binary` even though they exist.
- **Fix**: In `login_shell_path()` (`src-tauri/src/ipc/agents.rs:553`), add a Linux-only fallback when `SHELL` is unset: read `/etc/passwd` for the invoking UID's shell field (e.g. via the `nix` crate's `getpwuid`) instead of only trusting the `SHELL` env var, so PATH augmentation still runs under systemd/`.desktop` launch paths that omit it.
- **Status**: OPEN

### OS-opener path-reveal comment claims macOS-only reasoning but branch coverage is actually correct — verify no fourth platform gap
- **ID**: L8-SHELL-AGENT-9
- **Severity**: LOW
- **Platforms affected**: macOS
- **Evidence**: `src-tauri/src/ipc/project.rs:327` — `#[cfg(not(any(target_os = "macos", target_os = "windows")))]`
- **Why it breaks**: Not a defect by itself — `cmd_path_reveal` correctly branches macOS (`open -R`), Windows (`explorer /select,`), and falls back to `xdg-open` for everything else including BSDs. However `xdg-open` assumes a freedesktop-compliant desktop environment; on a machine with no desktop session (e.g. macOS is excluded, but a headless Linux dev container running the Tauri app under Xvfb has no `xdg-open` handler configured) the `Command::new("xdg-open")` spawn succeeds but silently opens nothing, and the caller only surfaces `spawn()` errors, not handler-not-found failures reported asynchronously by `xdg-open` itself.
- **Fix**: This is intrinsic to `xdg-open` and not fixable from Ferryx's side beyond documenting the limitation; no code change is required unless product wants a "no file manager available" toast, in which case check `xdg-open`'s exit status via `.status()` instead of `.spawn()` in `cmd_path_reveal` (`src-tauri/src/ipc/project.rs:335`) and surface non-zero exits to the UI.
- **Status**: FIXED (branch coverage for macOS/Windows/Linux is already complete at `src-tauri/src/ipc/project.rs:302-337`; only the headless-Linux `xdg-open` edge case is unhandled, which is a pre-existing platform-tool limitation, not a missing branch)

### PTY session cwd validation on Windows relies on canonicalize() producing `\\?\` UNC-prefixed paths inconsistently across call sites
- **ID**: L8-SHELL-AGENT-10
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/manager.rs:133` — `fs::canonicalize(&requested).map_err(|_| WorktreeError::InvalidRepoRoot {`
- **Why it breaks**: `std::fs::canonicalize` on Windows returns extended-length `\\?\C:\...` paths. `worktree/git.rs` already strips this prefix defensively before shelling out to `git` (`normalize_path_for_git`/`strip_verbatim_prefix`, confirmed at `src-tauri/src/worktree/git.rs:39-61`), but `WorktreeManager::canonical_allowed_path` (used by `pty.rs:94` for the `spawn_in_worktree` cwd/ownership check) and other `fs::canonicalize` call sites in `manager.rs` (lines 143, 239, 266, 419) do not run the same stripping before comparing paths with `starts_with` or building user-facing strings, so a canonicalized `\\?\C:\...\task` path compared against a non-canonicalized `C:\...\task` path from IPC input can fail a `starts_with` ownership check that would pass identically-shaped inputs on macOS/Linux.
- **Fix**: Route every `fs::canonicalize` result in `src-tauri/src/worktree/manager.rs` through `crate::worktree::git::normalize_path_for_git` (already Windows-tested at `git.rs:420-469`) before using it in path-prefix comparisons or returning it to the frontend, so canonicalization is consistently verbatim-prefix-free across the whole worktree module, not just the `run_git` boundary.
- **Status**: OPEN

### Login-shell `-l` flag is applied uniformly for macOS/Linux without checking the target shell supports it
- **ID**: L8-SHELL-AGENT-11
- **Severity**: LOW
- **Platforms affected**: macOS+Linux
- **Evidence**: `src-tauri/src/terminal/shell.rs:359` — `args: vec!["-l".to_string()],`
- **Why it breaks**: Both the macOS (`shell.rs:343`) and Linux (`shell.rs:359`) "no explicit preference" branches append `-l` to whatever `$SHELL` resolves to. This is correct for bash/zsh/fish, but a user with an unusual `$SHELL` value such as `tcsh` (still shipped on some Linux distros and BSD-derived tooling) or a restricted/non-POSIX shell that does not recognize `-l` will fail to start or start in a degraded mode, since the flag is applied unconditionally based on platform rather than on the resolved shell's basename.
- **Fix**: In `resolve_shell_command_pure`'s macOS/Linux `None` branches, only append `-l` when the resolved shell's basename matches a known-safe set (`bash`, `zsh`, `fish`, `sh`, `dash`), and omit it otherwise, so an uncommon `$SHELL` value degrades to a plain non-login invocation instead of a hard failure.
- **Status**: OPEN
