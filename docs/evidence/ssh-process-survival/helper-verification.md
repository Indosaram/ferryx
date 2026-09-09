# Verification Evidence: Phase A Remote Helper Process Preservation

**Date:** 2026-09-09  
**Task ID:** `st_01a08427`  
**Parent Session:** `01a083bf-6ac5-7707-9c4d-7b73eea73885`  
**Depth:** 1 (Child lane `hephaestus`)  
**Role:** Independent Verifier (Phase A-4 `verify-helper`)  
**Scope:** `docs/evidence/ssh-process-survival/helper-verification.md`, `helper-verification-windows.log`, `helper-verification-windows-stderr.log`, `helper-verification-native.log`  
**Target:** Standalone Ferryx Remote Helper (`remote-helper`, `src-tauri/src/ferryx_scope/ssh`)  
**Verifier checkpoint before lead repairs:** INCOMPLETE; Windows had 8 failures.

## Lead acceptance after platform repairs

Phase A helper ownership and bridge-lifetime contracts are now accepted. This
does not accept the application's SSH transport or automatic daemon restoration.

- Windows: `helper-platform-fix-windows.log`, 23/23 tests, debug build, PID 8644
  and nonce `b202cd2b-c449-43c5-855d-a48b179db0a1` unchanged, counter 1 to 2.
  Remote shell stopped, tracked PIDs absent, build fixture and copied runner removed.
- Linux: `helper-platform-fix-linux.log`, 23/23 tests, unoptimized debug build,
  PID 2819688 and nonce `c8218ef7-2bf9-4f6a-b1e8-33d954c7b946` unchanged,
  counter 1 to 2. Tracked PIDs absent; runtime and build fixtures removed.
- macOS: the native results below plus `helper-platform-fix-native.log` (23/23)
  and the lead's `helper-harness-audit.log` actual process/cleanup evidence.

The ACL defect was repaired at directory permission creation: current-user
FullControl now includes object/container inheritance. The isolated ACL probe
proved that the old non-inheritable directory grant caused child files to fall
back to a logon-session ACL. Validation was not weakened to allow arbitrary
logon SIDs. Windows test terminals now answer the real ConPTY cursor request
before sending input; assertions still verify actual process memory and PID.

Linux's first debug build hit disk quota after its tests passed. The successful
run used `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, and
`CARGO_INCREMENTAL=0`; optimization remained disabled and no release was built.
Both local snapshot archives were removed. The earlier failed checkpoint and
its raw evidence remain below for provenance; the acceptance above supersedes
its status and remediation proposals.

---

## 1. Executive Summary & Definitive Verdict

This independent verification audits and executes the final Phase A remote helper process preservation implementation across both native (macOS Darwin arm64) and remote Windows (`maho-win` NT 10.0.26200.0) platforms.

### Platform Status Matrix

| Platform | Environment | Test Suite | Debug Build | Bridge Survival Harness | Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Native macOS** | Apple M4 Max (Darwin 25.6.0 arm64) | ✅ **PASS** (23/23 passed in 0.21s) | ✅ **PASS** (Built in 0.76s) | ✅ **PASS** (PID/nonce/state preserved across `SIGKILL`) | **PASS** |
| **Desktop macOS** | `src-tauri` lib tests | ✅ **PASS** (13 process + 5 safety passed) | N/A | N/A | **PASS** |
| **Remote Windows** | `maho-win` (Windows NT 10.0.26200.0, MSVC link `14.44.35207`, Bun 1.4.0) | ❌ **FAIL** (15 passed, 8 failed in `cargo test`) | ⏸️ **HALTED** (Halted by default runner) | ⏸️ **HALTED** (Halted by default runner) | ⚠️ **INCOMPLETE** |

### Retraction & Correction of Earlier Claims

Earlier verification report `st_01a0841b` falsely claimed that Windows private-IPC DACL and owner SID verification code existed and passed. In reality, that code was merely a future proposal documented in prose. 

Following the completion of the `helper-service` node, actual Windows ACL validation code was implemented in `src-tauri/src/ferryx_scope/ssh/process.rs`. However, when executed on the live remote Windows builder (`maho-win`), the actual implementation revealed **8 test failures** out of 23 unit tests. 

In accordance with strict verification policy:
- **No failure is obscured, skipped, or replaced with cross-compilation.**
- The overall Phase A increment is honestly rendered as **INCOMPLETE**.
- Exact failure logs, panics, root causes, and complete multi-host cleanup are documented below.

---

## 2. Producer Deliverable & Source Audit

### 2.1 Service Security Boundary (`src-tauri/src/ferryx_scope/ssh/process.rs`)

The producer added Windows ACL validation via PowerShell `Get-Acl`:

```rust
#[cfg(windows)]
fn validate_windows_acl(path: &Path) -> Result<(), String> {
    use base64::Engine as _;
    let path_str = path.to_str().ok_or_else(|| "FORBIDDEN: helper IPC path is invalid UTF-8".to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(path_str.as_bytes());
    let script = format!(
        r#"$ErrorActionPreference='Stop';$raw=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'));if($raw.StartsWith('\\?\UNC\')){{$raw='\\'+$raw.Substring(8)}}elseif($raw.StartsWith('\\?\')){{$raw=$raw.Substring(4)}};$p=[System.IO.Path]::GetFullPath($raw);$acl=Get-Acl -LiteralPath $p;$u=[System.Security.Principal.WindowsIdentity]::GetCurrent();$allowed=@($u.User.Value,'S-1-5-18','S-1-5-32-544');$owner=try{{$acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$null}};if(-not $owner){{try{{$owner=(New-Object System.Security.Principal.NTAccount($acl.Owner)).Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$owner=$acl.Owner}}}};if($allowed -notcontains $owner){{exit 1}};$rules=$acl.Access;if($null -eq $rules -or $rules.Count -eq 0){{exit 2}};$hasAllowed=$false;foreach($r in $rules){{if($r.AccessControlType.ToString() -eq 'Allow'){{$sid=try{{$r.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$r.IdentityReference.Value}};if($allowed -notcontains $sid){{exit 3}};$hasAllowed=$true}}}};if(-not $hasAllowed){{exit 4}};exit 0;"#
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .output()
        .map_err(|e| format!("FORBIDDEN: cannot execute PowerShell ACL validation: {e}"))?;

    if !output.status.success() {
        return Err("FORBIDDEN: helper IPC must be owned by the current user and private".into());
    }
    Ok(())
}
```

**Root Cause Analysis of Windows ACL Failure:**
The hardcoded allowed principals list:
`$allowed=@($u.User.Value,'S-1-5-18','S-1-5-32-544');`
contains only the user SID, Local SYSTEM (`S-1-5-18`), and BUILTIN\Administrators (`S-1-5-32-544`). When files and directories are created under `AppData\Local\Temp` in Windows, the operating system automatically assigns a Logon Session SID (`S-1-5-5-X-Y`) to the access control list. Because `S-1-5-5-*` is not in `$allowed`, rule evaluation triggers `exit 3` (`$allowed -notcontains $sid`), causing `validate_private` to fail closed and reject valid files created by the current user.

### 2.2 QA Survival Harness (`scripts/qa/ssh-helper-survival.mjs`)

The lead corrected the test harness request counting:
1. **Deduplication Counting:** Probing deduplication resubmits a second `pty.spawn` request with identical parameters. The harness accurately asserts `spawnRequests: 2`, while verifying `pty.list` contains exactly `1` process with the original PID.
2. **Platform Gating:** The POSIX mode bit (`0o755`) and symlink endpoint tests are wrapped with `if (process.platform !== "win32")`. On Windows, ACL security is verified via the native Rust unit test fixture (`ssh_reconnect_safety_rejects_untrusted_windows_acl`).
3. **Lossless Cursors:** Uses decimal string cursors (`"0"`, `"1"`, `"4"`) rather than JavaScript numbers.

### 2.3 Desktop Library Test Discovery

In `src-tauri`, tests were previously filtered out (running 0 tests) because the module path did not match `--lib` filter queries. A module alias `helper_runtime` in `src-tauri/src/ferryx_scope/ssh/mod.rs` allows `cargo test --lib` to discover all 13 process survival tests and 5 reconnect safety tests.

Existing native proofs are in:
- `docs/evidence/ssh-process-survival/helper-service-green-final.log` (23 tests passed)
- `docs/evidence/ssh-process-survival/helper-service-asan.log` (4 tests passed under AddressSanitizer)

---

## 3. Native macOS Execution Evidence

### 3.1 Crate Unit & Integration Tests

**Command:**
```bash
cargo test --manifest-path remote-helper/Cargo.toml
```

**Literal Output:**
```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests ../src-tauri/src/ferryx_scope/ssh/standalone.rs (remote-helper/target/debug/deps/ferryx_remote_helper-24fb5d0aa7f640ae)

running 23 tests
test scoped_contracts::tests::epoch_rejects_noncanonical_or_out_of_range_wire_values ... ok
test scoped_contracts::tests::target_roundtrip_preserves_full_u64_epoch_as_string ... ok
test scoped_contracts::tests::result_rejects_mismatched_discriminants ... ok
test scoped_contracts::tests::producer_boundaries_roundtrip ... ok
test scoped_contracts::tests::shared_envelopes_roundtrip ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_rejects_shutdown_op ... ok
test ssh::process::helper_service_tests::ssh_process_survival_bridge_allows_describe_without_stopping_runtime ... ok
test ssh::process::helper_service_tests::ssh_reconnect_safety_live_runtime_cannot_be_replaced ... ok
test ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_symlink_root_and_endpoint ... ok
test ssh::process::helper_service_tests::ssh_process_survival_stale_endpoint_replaced_only_after_lock_release ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_spawn_dedupe_and_conflict ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_binary_safe_base64_io ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_canonical_decimal_cursor ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_auth_and_target_validation ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_respects_cols_rows ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_target_describe_and_list_metadata ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_frame_bounded_below_1mib ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_env_retains_session_id_without_fabrication ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_project_root_decoupled ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_ring_gap_detection ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_concurrent_spawn_atomic_reservation ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_default_platform_login_shell ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_retains_pid_and_memory_state ... ok

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
```

### 3.2 Crate Debug Build

**Command:**
```bash
cargo build --manifest-path remote-helper/Cargo.toml
```

**Literal Output:**
```
   Compiling ferryx-remote-helper v2026.908.1 (/Users/indo/code/project/orca-lite/remote-helper)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.76s
```

### 3.3 Bridge Survival Harness (`bun scripts/qa/ssh-helper-survival.mjs`)

**Command:**
```bash
bun scripts/qa/ssh-helper-survival.mjs
```

**Literal Output:**
```
{"event":"before-disconnect","target":{"backendSessionId":"145c2123-3847-4dd9-b6a4-9da346f3373d","epoch":"5946771461303060029","hostId":"qa-host","ownerId":"7f64ef70-9022-4a8c-b40d-cd37ad640247"},"pid":5260,"counter":1,"nonce":"4ac8a90c-a566-4bfe-a16a-650fc6e5c2e5"}
cleanup: reaped child 5259
{"event":"after-reconnect","target":{"backendSessionId":"145c2123-3847-4dd9-b6a4-9da346f3373d","epoch":"5946771461303060029","hostId":"qa-host","ownerId":"7f64ef70-9022-4a8c-b40d-cd37ad640247"},"pid":5260,"counter":2,"nonce":"4ac8a90c-a566-4bfe-a16a-650fc6e5c2e5","spawnCount":1}
{"event":"verified-describe","describe":{"cols":100,"cursor":"4","cwd":"/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-helper-qa-ETrnNP/project","exited":false,"pid":5260,"rows":30,"target":{"backendSessionId":"145c2123-3847-4dd9-b6a4-9da346f3373d","epoch":"5946771461303060029","hostId":"qa-host","ownerId":"7f64ef70-9022-4a8c-b40d-cd37ad640247"}}}
{"event":"verified-spawn-dedupe","dedupeSpawn":{"pid":5260,"target":{"backendSessionId":"145c2123-3847-4dd9-b6a4-9da346f3373d","epoch":"5946771461303060029","hostId":"qa-host","ownerId":"7f64ef70-9022-4a8c-b40d-cd37ad640247"}},"spawnRequests":2}
{"event":"verified-spawn-conflict","error":"REQUEST_CONFLICT: clientRequestId already used with different parameters"}
{"event":"verified-expired-target","error":"TARGET_EXPIRED"}
{"event":"verified-unknown-target","error":"NOT_FOUND"}
{"event":"verified-allowlist-rejection","error":"UNSUPPORTED: operation not allowlisted"}
helper[5262]: FORBIDDEN: helper IPC must be owned by the current user and private

{"event":"verified-insecure-root-rejection","exitCode":1,"stderr":"FORBIDDEN: helper IPC must be owned by the current user and private"}
helper[5263]: FORBIDDEN: helper IPC cannot be a symlink

{"event":"verified-symlink-endpoint-rejection","exitCode":1,"stderr":"FORBIDDEN: helper IPC cannot be a symlink"}
{"event":"verified-remote-shell-stopped","shellPid":5260,"shellAlive":false}
PASS: same remote PID, nonce and mutable counter after actual bridge SIGKILL
cleanup: reaped child 5261
cleanup: reaped child 5258
cleanup: removed /var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-helper-qa-ETrnNP; remoteStopped=true
{"event":"cleanup-receipt","fixtureRemoved":"/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-helper-qa-ETrnNP","trackedPids":[5258,5259,5261,5262,5263,5260],"remainingLivePids":[],"remoteStopped":true}
```

### 3.4 Desktop Library Integration Test Suites

**Process Survival (13 tests):**
```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml -- ssh_process_survival
```
*Result:* `13 passed; 0 failed; 0 ignored; 780 filtered out; finished in 0.28s`

**Reconnect Safety (5 tests):**
```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml -- ssh_reconnect_safety
```
*Result:* `5 passed; 0 failed; 0 ignored; 788 filtered out; finished in 0.06s`

---

## 4. Real Windows Execution Evidence (`maho-win`)

### 4.1 Invocation & Fixture Setup

- **Host:** `maho-win` (`DESKTOP-1LAPJMP`), user `sook`, Windows NT 10.0.26200.0.
- **Transport:** OpenSSH with `-o StrictHostKeyChecking=yes -o BatchMode=yes`.
- **Source Archive:** Unique tar `ferryx-verify-f163a57d-7129-43f5-aafa-9703a528d5d3.tar` (153,600 bytes) containing:
  - `remote-helper/Cargo.toml` & `remote-helper/Cargo.lock`
  - `src-tauri/src/ferryx_scope/ssh` (all 7 source and test files)
  - `src-tauri/src/scoped_contracts.rs` & `src-tauri/src/scoped_contracts_tests.rs`
  - `scripts/qa/ssh-helper-survival.mjs`
- **Runner Script:** `scripts/qa/verify-remote-helper.ps1` staged to `C:\Users\sook\AppData\Local\Temp\verify-remote-helper-f163a57d-7129-43f5-aafa-9703a528d5d3.ps1`.
- **PowerShell Invocation:** Base64 UTF-16LE `-EncodedCommand` payload:
```powershell
$ProgressPreference = 'SilentlyContinue'
$runner = 'C:\Users\sook\AppData\Local\Temp\verify-remote-helper-f163a57d-7129-43f5-aafa-9703a528d5d3.ps1'
$archive = 'C:\Users\sook\AppData\Local\Temp\ferryx-verify-f163a57d-7129-43f5-aafa-9703a528d5d3.tar'
$exitCode = 1
try {
    & $runner -Archive $archive
    $exitCode = $LASTEXITCODE
} catch {
    Write-Output "RUNNER_EXCEPTION: $_"
    $exitCode = 1
} finally {
    if (Test-Path -LiteralPath $runner) {
        Remove-Item -LiteralPath $runner -Force
        Write-Output "cleanup: removed copied runner $runner"
    }
}
exit $exitCode
```

### 4.2 Literal Output from Windows Runner

**Standard Output (`helper-verification-windows.log`):**
```
WINDOWS_HELPER_VERIFY C:\Users\sook\AppData\Local\Temp\ferryx-helper-verify-973b90a4-3100-4a47-965e-9fe8945143ff

running 23 tests
test scoped_contracts::tests::target_roundtrip_preserves_full_u64_epoch_as_string ... ok
test scoped_contracts::tests::epoch_rejects_noncanonical_or_out_of_range_wire_values ... ok
test scoped_contracts::tests::result_rejects_mismatched_discriminants ... ok
test scoped_contracts::tests::shared_envelopes_roundtrip ... ok
test scoped_contracts::tests::producer_boundaries_roundtrip ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_canonical_decimal_cursor ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_project_root_decoupled ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_frame_bounded_below_1mib ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_respects_cols_rows ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_spawn_dedupe_and_conflict ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_concurrent_spawn_atomic_reservation ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_auth_and_target_validation ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_ring_gap_detection ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_rejects_shutdown_op ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_target_describe_and_list_metadata ... ok
test ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_untrusted_windows_acl ... FAILED
test ssh::process::helper_service_tests::ssh_process_survival_bridge_allows_describe_without_stopping_runtime ... FAILED
test ssh::process::helper_service_tests::ssh_process_survival_stale_endpoint_replaced_only_after_lock_release ... FAILED
test ssh::process::helper_service_tests::ssh_reconnect_safety_live_runtime_cannot_be_replaced ... FAILED
test ssh::helper::helper_core_tests::ssh_process_survival_default_platform_login_shell ... FAILED
test ssh::helper::helper_core_tests::ssh_process_survival_binary_safe_base64_io ... FAILED
test ssh::helper::helper_core_tests::ssh_process_survival_env_retains_session_id_without_fabrication ... FAILED
test ssh::helper::helper_core_tests::ssh_process_survival_retains_pid_and_memory_state ... FAILED

failures:

failures:
    ssh::helper::helper_core_tests::ssh_process_survival_binary_safe_base64_io
    ssh::helper::helper_core_tests::ssh_process_survival_default_platform_login_shell
    ssh::helper::helper_core_tests::ssh_process_survival_env_retains_session_id_without_fabrication
    ssh::helper::helper_core_tests::ssh_process_survival_retains_pid_and_memory_state
    ssh::process::helper_service_tests::ssh_process_survival_bridge_allows_describe_without_stopping_runtime
    ssh::process::helper_service_tests::ssh_process_survival_stale_endpoint_replaced_only_after_lock_release
    ssh::process::helper_service_tests::ssh_reconnect_safety_live_runtime_cannot_be_replaced
    ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_untrusted_windows_acl

test result: FAILED. 15 passed; 8 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.06s

cleanup: removed Windows helper verification fixture C:\Users\sook\AppData\Local\Temp\ferryx-helper-verify-973b90a4-3100-4a47-965e-9fe8945143ff
RUNNER_EXCEPTION: Helper contracts failed: 101
cleanup: removed copied runner C:\Users\sook\AppData\Local\Temp\verify-remote-helper-f163a57d-7129-43f5-aafa-9703a528d5d3.ps1
```

**Standard Error / Panics (`helper-verification-windows-stderr.log`):**
```
   Compiling ferryx-remote-helper v2026.908.1 (C:\Users\sook\AppData\Local\Temp\ferryx-helper-verify-973b90a4-3100-4a47-965e-9fe8945143ff\remote-helper)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.47s
     Running unittests ..\src-tauri\src\ferryx_scope\ssh\standalone.rs (AppData\Local\Temp\ferryx-helper-verify-973b90a4-3100-4a47-965e-9fe8945143ff\remote-helper\target\debug\deps\ferryx_remote_helper-f81b385313d923c3.exe)
QA_CLEANUP_TRACKING test=concurrent_spawn runtime_dir="C:\\Users\\sook\\AppData\\Local\\Temp\\.tmpRH7SvR" project_dir="C:\\Users\\sook\\AppData\\Local\\Temp\\.tmpkELTSP"
QA_CLEANUP_RECEIPT test=concurrent_spawn removed runtime_dir="C:\\Users\\sook\\AppData\\Local\\Temp\\.tmpRH7SvR" exists=false project_dir="C:\\Users\\sook\\AppData\\Local\\Temp\\.tmpkELTSP" exists=false

thread 'ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_untrusted_windows_acl' (26132) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_service_tests.rs:60:5:
assertion failed: validate_private(&file).is_ok()

thread 'ssh::process::helper_service_tests::ssh_process_survival_bridge_allows_describe_without_stopping_runtime' (6684) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_service_tests.rs:91:66:
called `Result::unwrap()` on an `Err` value: "FORBIDDEN: helper IPC must be owned by the current user and private"

thread 'ssh::process::helper_service_tests::ssh_process_survival_stale_endpoint_replaced_only_after_lock_release' (28560) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_service_tests.rs:27:41:
called `Result::unwrap()` on an `Err` value: "FORBIDDEN: helper IPC must be owned by the current user and private"

thread 'ssh::process::helper_service_tests::ssh_reconnect_safety_live_runtime_cannot_be_replaced' (17412) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_service_tests.rs:15:5:
assertion failed: matches!(bind_runtime(dir.path(), "qa-lock".into()), Err(error) if
    error.starts_with("REMOTE_RUNTIME_CONFLICT:"))

thread 'ssh::helper::helper_core_tests::ssh_process_survival_default_platform_login_shell' (20608) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_core_tests.rs:596:111:
called `Result::unwrap()` on an `Err` value: "Timed out waiting for 'LOGIN_SHELL_CONFIRMED_OK'. Accumulated output: \u{1b}[6n"

thread 'ssh::helper::helper_core_tests::ssh_process_survival_binary_safe_base64_io' (25488) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_core_tests.rs:360:108:
called `Result::unwrap()` on an `Err` value: "Timed out waiting for 'FERRYX_B64_SENTINEL_771'. Accumulated output: \u{1b}[6n"

thread 'ssh::helper::helper_core_tests::ssh_process_survival_env_retains_session_id_without_fabrication' (15252) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_core_tests.rs:715:111:
called `Result::unwrap()` on an `Err` value: "Timed out waiting for 'CLAUDE_ID=NONE'. Accumulated output: \u{1b}[6n"

thread 'ssh::helper::helper_core_tests::ssh_process_survival_retains_pid_and_memory_state' (22256) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_core_tests.rs:277:111:
called `Result::unwrap()` on an `Err` value: "Timed out waiting for 'SURVIVAL_NONCE=NONCE_39e381a9c54c4c7483540e08b4af8773'. Accumulated output: \u{1b}[6n"
error: test failed, to rerun pass `--bin ferryx-remote-helper`
```

### 4.3 Detailed Technical Breakdown of the 8 Failures

The failures divide into two distinct categories:

#### Failure Category 1: Overly Restrictive Windows ACL Validation (4 failures)
- **Failing Tests:**
  1. `ssh_reconnect_safety_rejects_untrusted_windows_acl`
  2. `ssh_process_survival_bridge_allows_describe_without_stopping_runtime`
  3. `ssh_process_survival_stale_endpoint_replaced_only_after_lock_release`
  4. `ssh_reconnect_safety_live_runtime_cannot_be_replaced`
- **Mechanism:** In `validate_windows_acl` (`process.rs`), every ACL rule is tested against `$allowed = @($u.User.Value, 'S-1-5-18', 'S-1-5-32-544')`. When files are created in Windows under `AppData\Local\Temp`, NTFS automatically attaches the active Logon Session SID (`S-1-5-5-*`). Because `$allowed` excludes `S-1-5-5-*`, `validate_private` immediately returns `FORBIDDEN:` on legitimate files created by the test suite itself.

#### Failure Category 2: Windows ConPTY Device Status Report Blocking (4 failures)
- **Failing Tests:**
  5. `ssh_process_survival_default_platform_login_shell`
  6. `ssh_process_survival_binary_safe_base64_io`
  7. `ssh_process_survival_env_retains_session_id_without_fabrication`
  8. `ssh_process_survival_retains_pid_and_memory_state`
- **Mechanism:** In Windows `portable-pty`, spawning `cmd.exe` initializes a pseudo-console (ConPTY). Upon startup, ConPTY emits ANSI DSR query `\x1b[6n` (Device Status Report: Cursor Position Request). The unit test harness pipes text without answering this terminal query or handling ConPTY's synchronous input pump. As a result, the tests time out after accumulating only `\u{1b}[6n`.

#### Runner Halting
Because `cargo test` exited with code 101, `scripts/qa/verify-remote-helper.ps1` threw `Helper contracts failed: 101`, entering its `finally` block and halting before building the debug binary or executing `ssh-helper-survival.mjs`.

---

## 5. Multi-Host Cleanup Verification

Complete cleanup was executed and independently audited on both hosts:

### 5.1 Remote Windows (`maho-win`) Cleanup
1. **Fixture Directory:** `C:\Users\sook\AppData\Local\Temp\ferryx-helper-verify-973b90a4-3100-4a47-965e-9fe8945143ff` was recursively removed by the runner's `finally` block (`cleanup: removed Windows helper verification fixture ...`).
2. **Staged Archive:** `C:\Users\sook\AppData\Local\Temp\ferryx-verify-f163a57d-7129-43f5-aafa-9703a528d5d3.tar` was removed by the runner's `finally` block.
3. **Runner Script:** `C:\Users\sook\AppData\Local\Temp\verify-remote-helper-f163a57d-7129-43f5-aafa-9703a528d5d3.ps1` was removed by the wrapper's `finally` block (`cleanup: removed copied runner ...`).
4. **Post-Run Filesystem Audit:**
   ```powershell
   Get-ChildItem C:\Users\sook\AppData\Local\Temp\ferryx-helper-verify-*, C:\Users\sook\AppData\Local\Temp\verify-remote-helper-*, C:\Users\sook\AppData\Local\Temp\ferryx-verify-*
   ```
   *Result:* 0 items returned. Clean.
5. **Post-Run Process Audit:**
   ```powershell
   Get-Process -Name *ferryx*, *cargo*, *bun* -ErrorAction SilentlyContinue
   ```
   *Result:* No orphan helper or QA processes found. User desktop app `PID 20528` (`ferryx`) was completely untouched.

### 5.2 Native macOS Cleanup
1. **Fixture Directory:** `/private/var/folders/zh/.../ferryx-helper-qa-ETrnNP` was deleted.
2. **Local Archive:** `/tmp/ferryx-verify-f163a57d-7129-43f5-aafa-9703a528d5d3.tar` was deleted.
3. **Process Receipts:** All 6 tracked processes (`[5258, 5259, 5261, 5262, 5263, 5260]`) were cleanly reaped (`remainingLivePids: []`).

---

## 6. Actionable Remediation Roadmap

To achieve an honest GREEN status across all platforms in Phase A, the following two changes are required:

1. **Permit Windows Logon Session SIDs in `process.rs`:**
   In `validate_windows_acl`, dynamically include the current logon session SID (`$u.User.Value` and any SID starting with `S-1-5-5-` or matching `($u.Groups | Where-Object { $_.Value -like 'S-1-5-5-*' })`), or restrict checks to non-logon principals.
2. **ConPTY DSR Query Handling in `helper_core_tests.rs`:**
   In Windows test cases using `portable-pty`, respond to or drain `\x1b[6n` device status queries, or invoke shells in non-interactive batch mode (e.g. `cmd.exe /c` or PowerShell `-NoProfile -Command`) so output stream reading does not block waiting for a terminal emulator cursor report.
