# SSH Process Survival Evidence: Helper Service Runtime & Windows Private-IPC Boundary

**Date:** 2026-09-09  
**Lane:** Phase A-2 `helper-service`  
**Worker:** hephaestus (`st_01a08423`)  
**Scope:** `src-tauri/src/ferryx_scope/ssh/process.rs`, `src-tauri/src/ferryx_scope/ssh/helper_service_tests.rs`, and this evidence document (`docs/evidence/ssh-process-survival/helper-service.md`).

---

## 1. Executive Summary & Status

The private IPC security boundary has been implemented and hardened across both Unix and Windows platforms:
- **Exclusive OS-held lock**: `std::fs::File::try_lock` on `runtime.lock` prevents concurrent/conflicting helper daemons.
- **Staged atomic endpoint publication**: Temporary file creation (`endpoint-<uuid>.tmp`), permission restriction via `private_file`, and atomic rename to `endpoint.json`.
- **Stale endpoint cleanup authorization**: Authorized solely after acquiring the OS file lock.
- **PTY lifecycle decoupling**: Bridge EOF closes only the active bridge connection; `Runtime` retains ownership of remote PTY sessions.
- **Operation allowlisting**: Explicit allowlist including `handshake`, `project.register`, `project.list`, `worktree.create`, `pty.spawn`, `pty.list`, `pty.describe`, `pty.read`, `pty.write`, `pty.resize`, `pty.stop`.
- **POSIX security checks**: Owner UID validation against `geteuid()`, mode `0o077 == 0`, symlink rejection via `symlink_metadata()`, and single hard-link enforcement (`nlink == 1`).
- **Windows owner & ACL security checks**: Reparse point rejection (`FILE_ATTRIBUTE_REPARSE_POINT 0x400`), owner SID restriction (current user, SYSTEM `S-1-5-18`, Administrators `S-1-5-32-544`), and DACL Allow-rule restriction to authorized principals only. Broad principals (such as `Everyone` `S-1-1-0`) are rejected fail-closed.

---

## 2. Real Windows RED Capture Evidence

The lead runner captured authentic Windows RED failure on `maho-win` (`DESKTOP-1LAPJMP`) using `scripts/qa/verify-remote-helper.ps1`:
- **Evidence Log:** `docs/evidence/ssh-process-survival/helper-windows-acl-red.log`
- **Host / User:** `maho-win`, user `sook`
- **Fixture Directory:** `C:\Users\sook\AppData\Local\Temp\ferryx-helper-verify-b6982d57-b210-41dc-a4ad-87a53d1f741a`

### RED Failure Summary
```
running 1 test

thread 'ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_untrusted_windows_acl' (4160) panicked at ..\src-tauri\src\ferryx_scope\ssh\helper_service_tests.rs:77:5:
assertion failed: matches!(validate_private(&file), Err(error) if error ==
    "FORBIDDEN: helper IPC must be owned by the current user and private")
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_untrusted_windows_acl ... FAILED

failures:
    ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_untrusted_windows_acl

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 22 filtered out; finished in 0.03s
```

### Analysis of the RED State
1. **True Assertion Failure**: The crate compiled successfully in 7.09s on MSVC. The failure was an authentic security contract assertion failure, not a syntax or compilation failure.
2. **Defect Proved**: When read permission was granted to `Everyone` via `icacls ... /grant "*S-1-1-0:(R)"`, the unpatched `validate_private` returned `Ok`, failing to reject the world-readable IPC endpoint.
3. **Fixture Cleanup**: The verification harness successfully removed the fixture directory upon exit (`cleanup: removed Windows helper verification fixture ...`).

---

## 3. Production Fix Implementation

Applied in `src-tauri/src/ferryx_scope/ssh/process.rs`:

```rust
fn validate_private(path: &Path) -> Result<std::fs::Metadata, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("FORBIDDEN: helper IPC cannot be a symlink".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no arguments, pointers, or caller preconditions.
        let uid = unsafe { libc::geteuid() };
        if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err("FORBIDDEN: helper IPC must be owned by the current user and private".into());
        }
        if metadata.is_file() && metadata.nlink() != 1 {
            return Err("FORBIDDEN: helper IPC cannot have hard links".into());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("FORBIDDEN: helper IPC cannot be a reparse point".into());
        }
        validate_windows_acl(path)?;
    }
    Ok(metadata)
}

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

### Key Properties of the Production Fix
1. **Allowed Principal Scope**: Solely the current user SID (`$u.User.Value`), Local SYSTEM (`S-1-5-18`), and BUILTIN\Administrators (`S-1-5-32-544`) are permitted.
2. **Owner Verification**: If the file owner is not in the allowed set, execution terminates with exit code 1 and returns a `FORBIDDEN:` error.
3. **Strict DACL Filtering**: Every `Allow` rule in the access control list is translated to a SID and checked against `$allowed`. Broad groups such as `Everyone` (`S-1-1-0`) or standard users immediately cause exit code 3, failing closed.
4. **Platform Compatibility & Injection Defense**:
   - Base64 encoding isolates paths from shell injection or quote corruption.
   - Explicit stripping of `\\?\UNC\` and `\\?\` verbatim prefixes enables compatibility with `Get-Acl` across PowerShell 5.1 and 7+.
   - `-LiteralPath` prevents wildcard interpretation.
5. **No Redundant Fallback**: Direct execution of `powershell.exe` without redundant path resolution fallbacks.

---

## 4. Hardened Windows Security Test Specification

Location: `src-tauri/src/ferryx_scope/ssh/helper_service_tests.rs`

```rust
#[cfg(windows)]
#[test]
fn ssh_reconnect_safety_rejects_untrusted_windows_acl() {
    let dir = private_tempdir();
    let file = dir.path().join("endpoint.json");
    std::fs::write(&file, b"{}").unwrap();
    super::super::private_file(&file).unwrap();
    assert!(validate_private(&file).is_ok());

    let grant = std::process::Command::new("icacls")
        .arg(&file)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .expect("icacls grant Everyone SID");
    assert!(grant.status.success());

    assert!(matches!(
        validate_private(&file),
        Err(error) if error.starts_with("FORBIDDEN:")
    ));
    assert!(matches!(
        endpoint(dir.path()),
        Err(error) if error.starts_with("FORBIDDEN:")
    ));
}
```

### Test Quality & Determinism
- **Machine Error Codes**: Asserts `error.starts_with("FORBIDDEN:")` instead of pinning localized or fragile English prose.
- **No Localized Retry**: Uses universal SID notation `*S-1-1-0:(R)` directly without localized `"Everyone:(R)"` retries.
- **Full Endpoint Rejection**: Confirms that both `validate_private(&file)` and `endpoint(dir.path())` fail closed with `FORBIDDEN:`.
- **Clean Fixture Isolation**: Backed by `tempfile::TempDir`, automatically cleaned up on test exit.

---

## 5. Verification Evidence

### 5.1 Windows Target Cross-Compilation Preflights

```bash
cargo check --tests --target x86_64-pc-windows-msvc --manifest-path remote-helper/Cargo.toml
```
**Output:**
```
    Checking ferryx-remote-helper v2026.908.1 (/Users/indo/code/project/orca-lite/remote-helper)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.47s
```
Status: ✅ 0 errors, 0 warnings.

```bash
cargo check --tests --target x86_64-pc-windows-gnu --manifest-path remote-helper/Cargo.toml
```
**Output:**
```
    Checking ferryx-remote-helper v2026.908.1 (/Users/indo/code/project/orca-lite/remote-helper)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.47s
```
Status: ✅ 0 errors, 0 warnings.

### 5.2 Standalone Remote-Helper Test Suite (macOS arm64 Native)

```bash
cargo test --manifest-path remote-helper/Cargo.toml
```
**Output:**
```
     Running unittests ../src-tauri/src/ferryx_scope/ssh/standalone.rs (remote-helper/target/debug/deps/ferryx_remote_helper-24fb5d0aa7f640ae)

running 23 tests
test scoped_contracts::tests::epoch_rejects_noncanonical_or_out_of_range_wire_values ... ok
test scoped_contracts::tests::target_roundtrip_preserves_full_u64_epoch_as_string ... ok
test scoped_contracts::tests::result_rejects_mismatched_discriminants ... ok
test scoped_contracts::tests::shared_envelopes_roundtrip ... ok
test scoped_contracts::tests::producer_boundaries_roundtrip ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_rejects_shutdown_op ... ok
test ssh::process::helper_service_tests::ssh_process_survival_bridge_allows_describe_without_stopping_runtime ... ok
test ssh::process::helper_service_tests::ssh_reconnect_safety_live_runtime_cannot_be_replaced ... ok
test ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_symlink_root_and_endpoint ... ok
test ssh::process::helper_service_tests::ssh_process_survival_stale_endpoint_replaced_only_after_lock_release ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_respects_cols_rows ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_project_root_decoupled ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_retains_pid_and_memory_state ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_frame_bounded_below_1mib ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_spawn_dedupe_and_conflict ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_concurrent_spawn_atomic_reservation ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_target_describe_and_list_metadata ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_auth_and_target_validation ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_ring_gap_detection ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_binary_safe_base64_io ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_env_retains_session_id_without_fabrication ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_canonical_decimal_cursor ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_default_platform_login_shell ... ok

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
```
Status: ✅ 23 passed, 0 failed.

### 5.3 Desktop Library Service Test Suites (macOS arm64 Native)

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml -- ssh_reconnect_safety
```
**Output:**
```
running 5 tests
test ssh::helper_runtime::helper::helper_core_tests::ssh_reconnect_safety_rejects_shutdown_op ... ok
test ssh::helper_runtime::process::helper_service_tests::ssh_reconnect_safety_rejects_symlink_root_and_endpoint ... ok
test ssh::helper_runtime::process::helper_service_tests::ssh_reconnect_safety_live_runtime_cannot_be_replaced ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_reconnect_safety_auth_and_target_validation ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_reconnect_safety_target_describe_and_list_metadata ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 788 filtered out; finished in 0.07s
```
Status: ✅ 5 passed, 0 failed.

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml -- ssh_process_survival
```
**Output:**
```
running 13 tests
test ssh::helper_runtime::process::helper_service_tests::ssh_process_survival_bridge_allows_describe_without_stopping_runtime ... ok
test ssh::helper_runtime::process::helper_service_tests::ssh_process_survival_stale_endpoint_replaced_only_after_lock_release ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_spawn_dedupe_and_conflict ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_binary_safe_base64_io ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_env_retains_session_id_without_fabrication ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_default_platform_login_shell ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_concurrent_spawn_atomic_reservation ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_project_root_decoupled ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_retains_pid_and_memory_state ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_canonical_decimal_cursor ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_ring_gap_detection ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_respects_cols_rows ... ok
test ssh::helper_runtime::helper::helper_core_tests::ssh_process_survival_frame_bounded_below_1mib ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 780 filtered out; finished in 0.28s
```
Status: ✅ 13 passed, 0 failed.

---

## 6. Honest Verification Limits

- **Local & Cross-Compile Complete**:
  - Native tests pass 100% on macOS arm64.
  - Windows cross-compilation preflights (`x86_64-pc-windows-msvc` and `x86_64-pc-windows-gnu`) build cleanly with 0 errors and 0 warnings.
- **Pending Downstream Windows GREEN Verification**:
  - The authentic Windows RED log is documented and verified in `docs/evidence/ssh-process-survival/helper-windows-acl-red.log`.
  - The downstream verifier will execute the live test suite on the remote Windows builder (`maho-win`) using `scripts/qa/verify-remote-helper.ps1`.
  - In accordance with verification discipline, **no Windows GREEN claim is made** until that actual remote Windows run log is returned and recorded.

---

## 7. Scope Audit

- Files touched:
  - `src-tauri/src/ferryx_scope/ssh/process.rs`
  - `src-tauri/src/ferryx_scope/ssh/helper_service_tests.rs`
  - `docs/evidence/ssh-process-survival/helper-service.md`
- No changes to `remote-helper/Cargo.toml` or `src-tauri/Cargo.toml`.
- No user daemon/trust/ACL alterations.
- No release builds or git commits created.
