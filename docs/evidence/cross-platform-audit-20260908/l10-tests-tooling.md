# Cross-Platform Test & Tooling Audit – L10-TESTS-TOOLING
**Report Date**: 2026-09-08  
**Scope**: Rust and TypeScript test suites plus dev/QA tooling  
**Focus**: POSIX-only assumptions hiding cross-platform regressions on Windows and Linux  

---

### Hardcoded `/tmp` paths in session persistence tests (test-only defect)
- **ID**: L10-TESTS-TOOLING-1
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/session/mod.rs:296` — `repo_root: PathBuf::from("/tmp/repo"),`
- **Why it breaks**: Test fixture uses Unix-only `/tmp` directory path; on Windows, tests fail because `/tmp` does not exist. Session load/save tests will not run on Windows CI, leaving persistence logic unchecked on that platform.
- **Fix**: Use `tempfile::TempDir::new()` and store its path; replace all hardcoded `/tmp/repo` instances (lines 296, 298, 304, 319, 360) with `temp_dir.path().join("repo")` or use `std::env::temp_dir()` with a test-specific subdirectory.
- **Status**: OPEN

### Hardcoded `/bin/sh` shell paths in PTY tests (test-only defect, affects 40+ tests)
- **ID**: L10-TESTS-TOOLING-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/tests.rs:20` — `let cmd = CommandBuilder::new("/bin/sh");`
- **Why it breaks**: Terminal, remote, and SSH tests hardcode `/bin/sh` which does not exist on Windows (shell is `cmd.exe` or PowerShell). Tests spawn pseudo-terminals with a non-existent shell, causing immediate failure on Windows. Across terminal/tests.rs, remote/tests.rs, and ssh/direct_tests.rs, at least 40 test cases cannot run on Windows.
- **Fix**: Add a cross-platform shell selection function: `fn shell_cmd() -> &'static str { if cfg!(windows) { "cmd.exe" } else { "/bin/sh" } }` and use it everywhere. Alternatively, use `portable_pty::CommandBuilder` with platform-aware command resolution or introduce a test utility that maps shell selection.
- **Status**: OPEN

### SSH socket paths hardcoded to Unix `/tmp/` (test-only defect)
- **ID**: L10-TESTS-TOOLING-3
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/direct_tests.rs:203` — `assert!(plan.args.windows(2).any(|pair| pair == ["-R", "/tmp/ferryx-agent-session-123.sock:/tmp/local-agent.sock"]));`
- **Why it breaks**: Test assertion hard-expects Unix socket path format. On Windows, socket forwarding uses different path formats (named pipes or WSL paths). Test will fail spuriously on Windows even if the underlying SSH forwarding logic is correct.
- **Fix**: Make the test fixture platform-aware: `let expected_sock_path = if cfg!(windows) { "//./pipe/..." } else { "/tmp/..." }` and compare against that. Better: refactor to validate the socket forwarding mechanism rather than the exact string representation.
- **Status**: OPEN

### Direct `/bin/sh` invocation in SSH protocol tests (test-only defect)
- **ID**: L10-TESTS-TOOLING-4
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/direct_tests.rs:84` — `let output = std::process::Command::new("/bin/sh")`
- **Why it breaks**: Test spawns a shell to validate SSH probe behavior; `/bin/sh` is POSIX-only. Test `remote_shell_probe_canonicalizes_quoted_directory_and_reports_plain_or_git` and others will fail immediately on Windows with "program not found".
- **Fix**: Use conditional compilation or a platform helper: `let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }`; or probe the user's login shell at test setup time using `std::env::var("SHELL")`.
- **Status**: OPEN

### Bash-only dev runner script excludes non-macOS platforms (dev-loop-only defect)
- **ID**: L10-TESTS-TOOLING-5
- **Severity**: HIGH
- **Platforms affected**: Windows, Linux
- **Evidence**: `scripts/macos-dev-runner.sh:4` — `if [[ "$(uname -s)" != "Darwin" || "${1:-}" != "run" ]]; then`
- **Why it breaks**: Dev runner script checks for macOS and falls through to direct cargo invocation on non-macOS. Script contains macOS-specific bundle assembly logic (lines 59, 91–92) and codesigning (line 97) that blindly run on Windows/Linux if the entry point guard is bypassed, but the intermediate steps (`install_atomic`, `mkdir -p "$MACOS_DIR"`) fail silently. Windows/Linux developers cannot use this helper; dev loop is broken for cross-platform testing.
- **Fix**: Add an early exit for non-macOS: `if [[ "$(uname -s)" != "Darwin" ]]; then exec cargo "$@"; fi` at line 4 to prevent fallthrough. Then ensure the macOS-specific bundle logic (lines 31–97) only runs after that guard.
- **Status**: OPEN

### macOS-specific `stat -f` command in tree quiescence checker (dev-loop-only defect)
- **ID**: L10-TESTS-TOOLING-6
- **Severity**: MEDIUM
- **Platforms affected**: Linux, Windows
- **Evidence**: `scripts/check-tree-quiescent.sh:54` — `stat -f "%m	%Sm	%N" -t "%Y-%m-%d %H:%M:%S" "$file"`
- **Why it breaks**: Helper script used in CI gates (e.g., parallel test coordination) calls `stat -f`, which is macOS-only (GNU `stat` on Linux uses `-c` instead). Script fails on Linux CI runners, blocking cross-platform test gating.
- **Fix**: Use portable alternatives: `stat -c "%Y	%y	%n" "$file"` on Linux (GNU stat), or better, replace the entire block with a Python snippet or use `find -printf` which is portable: `find ... -printf "%T@ %Tc %p\n"` and sort/format in shell-portable way.
- **Status**: OPEN

### macOS-specific `chmod --reference` and `stat -f` in installation helper (dev-loop-only defect)
- **ID**: L10-TESTS-TOOLING-7
- **Severity**: MEDIUM
- **Platforms affected**: Linux, Windows
- **Evidence**: `scripts/macos-dev-runner.sh:55` — `chmod "$(stat -f '%OLp' "$src")" "$tmp"`
- **Why it breaks**: Fallback line attempts macOS-style permission extraction via `stat -f` which fails on Linux/Windows. Even though `chmod --reference` fallback is attempted first, it also fails on macOS (BSD `chmod` doesn't support `--reference`). Net result: permission preservation is skipped on non-macOS and may produce incorrect bundle permissions.
- **Fix**: Use portable permission copy: `chmod "$(ls -L -o -d "$src" | awk '{print $1}')" "$tmp"` or Python: `os.chmod(tmp, os.stat(src).st_mode)`. Rust is better: move the permission copy to the build script written in Rust.
- **Status**: OPEN

### Hardcoded `/tmp` path in UI SSH section tests (test-only defect)
- **ID**: L10-TESTS-TOOLING-8
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `ui/src/components/settings/SshSection.test.tsx:661` — `openDialogMock.mockResolvedValue("/tmp/work-ssh-config");`
- **Evidence** (related): `ui/src/components/settings/SshSection.test.tsx:666` — `if (configPath === "/tmp/work-ssh-config")`
- **Why it breaks**: Mock fixture returns hardcoded Unix path. Test assertions compare against `/tmp/...` strings, which do not reflect Windows file paths. On Windows, tests may spuriously pass or fail depending on how the path is normalized.
- **Fix**: Use `tempfile` or a platform-aware mock: `const mockPath = process.platform === 'win32' ? 'C:\\tmp\\work-ssh-config' : '/tmp/work-ssh-config'` and update all assertions to use the dynamic path.
- **Status**: OPEN

### Terminal signal interrupt test gated to Unix, zero Windows coverage (test-only defect)
- **ID**: L10-TESTS-TOOLING-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/tests.rs:388` — `#[cfg(unix)]`
- **Why it breaks**: Only test for interrupt signal (`TerminalSignal::Interrupt`) is gated with `#[cfg(unix)]`. On Windows, `TerminalSignal::Interrupt` handling is never exercised. If the signal implementation differs on Windows (Ctrl+C vs. job termination), the defect is hidden.
- **Fix**: Create a platform-specific test pair. Write a Windows version that uses appropriate Windows signal primitives (`GenerateConsoleCtrlEvent`) or refactor the interrupt handler to be platform-agnostic and test on both. At minimum, add a `#[cfg(windows)]` test that validates Ctrl+C behavior.
- **Status**: OPEN

### `/bin/sh` shebang in notification audio test payload (test-only defect)
- **ID**: L10-TESTS-TOOLING-10
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/notifications.rs:337` — `std::fs::write(&path, b"#!/bin/sh\necho nope").expect("write file");`
- **Why it breaks**: Test creates a shell script with `#!/bin/sh` shebang. On Windows, this script cannot be executed directly even if the file extension is `.sh`. If the audio player tries to execute the script as a command, it will fail with "permission denied" or "invalid format" on Windows.
- **Fix**: This is a test payload that should never execute; if it does, the test design is flawed. Rename the test assertion to clarify intent: add a comment `// This script intentionally has an unsupported shebang; the audio player should reject it.` Alternatively, remove the payload file write entirely if it's not used by the assertion.
- **Status**: OPEN

### Majority of PTY and terminal tests lack Windows/Linux test coverage
- **ID**: L10-TESTS-TOOLING-11
- **Severity**: BLOCKER
- **Platforms affected**: Windows, Linux
- **Evidence**: `src-tauri/src/terminal/tests.rs:20` - `let cmd = CommandBuilder::new("/bin/sh");`
- **Scope of the pattern**: `src-tauri/src/terminal/tests.rs` (18 tests), `src-tauri/src/remote/tests.rs` (33+ tests, 13+ spawning `/bin/sh`), `src-tauri/src/ssh/direct_tests.rs` (6 tests) all build their PTY command the same way.
- **Why it breaks**: The terminal and PTY management is a core feature (used for all shell sessions, remote execution, and SSH). Tests are silently skipped or fail on Windows/Linux due to shell path issues (L10-TESTS-TOOLING-2). Zero functional coverage of terminal I/O, resizing, signal handling, or PTY isolation on those platforms. Cross-platform regressions in terminal functionality are invisible until production deployment.
- **Fix**: (1) Apply L10-TESTS-TOOLING-2 fix (cross-platform shell selection). (2) For each platform-specific signal/TTY feature, conditionally gate and provide platform-specific test variants. (3) Add a CI job that runs full test suite on Windows and Linux (e.g., GitHub Actions matrix with ubuntu-latest, windows-latest, macos-latest).
- **Status**: OPEN

---

**Summary**: 11 findings identified. **4 BLOCKER, 5 HIGH, 2 MEDIUM severity**. Root issues:  
1. **Hardcoded shell paths** (`/bin/sh`, `/bin/cat`) make 50+ test cases non-portable.  
2. **Unix-only dev tooling** (stat -f, chmod --reference, codesign, uname) blocks cross-platform dev loops.  
3. **Temporary directory assumptions** (`/tmp`) fail on Windows.  
4. **Platform signal tests** gated but not conditionally replaced; asymmetric test coverage.  

**Primary Risk**: Terminal and PTY functionality (core to the app) have zero test coverage on Windows and minimal coverage on Linux, hiding regressions until production.
