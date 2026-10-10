# OpenSSH Windows pRtlNtStatusToDosError Infinite Recursion Defect

## 1. Upstream Provenance & Affected Releases
- Upstream Repository: `PowerShell/openssh-portable`
- Target Source: `contrib/win32/win32compat/w32api_proxies.c`
- Upstream Source URL: `https://raw.githubusercontent.com/PowerShell/openssh-portable/latestw_all/contrib/win32/win32compat/w32api_proxies.c`
- Upstream File SHA256: `71a4d38ff22cc1812347a93a47241089b9e9fa545a2e834be8b2c250176f9f8c`
- Confirmed Affected Releases:
  - `v10.0.0.0` (commit `b8c08ef9da9450a94a9c5ef717d96a7bd83f3332`)
  - `v9.8.3.0` (commit `bdacf9868fe9e8024a5312861b7aaa6faba1821f`)
  - `v9.5.0.0` (commit `59aba65cf2e2f423c09d12ad825c3b32a11f408f`)

## 2. Root Cause Analysis
In `w32api_proxies.c`:
```c
ULONG pRtlNtStatusToDosError(NTSTATUS status)
{	
	HMODULE hm = NULL;
	typedef ULONG(NTAPI *RtlNtStatusToDosErrorType)(NTSTATUS);
	static RtlNtStatusToDosErrorType s_pRtlNtStatusToDosError = NULL;

	if (!s_pRtlNtStatusToDosError) {
		if ((hm = load_ntdll()) == NULL)
			return STATUS_ASSERTION_FAILURE;

		if ((s_pRtlNtStatusToDosError = (RtlNtStatusToDosErrorType)get_proc_address(hm, "RtlNtStatusToDosError")) == NULL)
			return STATUS_ASSERTION_FAILURE;
	}	
	return pRtlNtStatusToDosError(status);
}
```
During preauth virtual token generation (`generate_sshd_virtual_token+0x696 -> pRtlNtStatusToDosError+0xa3`), sshd converts NTSTATUS error codes to Win32 DOS errors. Because `pRtlNtStatusToDosError` recursively invokes itself instead of calling the resolved function pointer `s_pRtlNtStatusToDosError`, it enters an infinite recursion / tail-call jump loop pinning 100% of a single CPU core.

## 3. Architecture of Standalone Regression Test
File: `test_pRtlNtStatusToDosError_recursion.c`
- Contains the exact extracted function without any internal modifications or instrumentation.
- Uses narrow Windows loader stubs (`load_ntdll` calling `GetModuleHandleA("ntdll.dll")`, `get_proc_address` calling `GetProcAddress`).
- Process-isolated testing:
  - Parent process invokes itself with `--child` via `CreateProcessA`.
  - Child runs `RunChildWorker()` exercising real `ntdll!RtlNtStatusToDosError`:
    - `STATUS_NO_SUCH_USER` (0xC0000064) -> `ERROR_NO_SUCH_USER` (1317)
    - `STATUS_ACCESS_DENIED` (0xC0000022) -> `ERROR_ACCESS_DENIED` (5)
    - `STATUS_INVALID_HANDLE` (0xC0000008) -> `ERROR_INVALID_HANDLE` (6)
    - Validates loader cache hit (exactly 1 module lookup and 1 procedure lookup across all calls).
  - Parent waits up to 1500ms via `WaitForSingleObject`.
  - In PRE-FIX mode: child gets stuck in infinite recursion; parent times out after 1500ms, calls `TerminateProcess` strictly on its own child test process, and exits with code 0 proving defect confirmation.
  - In POST-FIX mode: child returns 0 immediately; parent inspects exit code 0 and reports test pass.

## 4. Remote Windows Build & Execution (maho-win)

Compiler available on `maho-win`: `C:\Strawberry\c\bin\gcc.exe`.

```powershell
cd scripts\qa\openssh-token-recursion

# Compile with strict flags (-Wall -Wextra -Werror)
cmd /c build_windows.cmd

# 1. Run Pre-fix binary (defect reproduction):
.\bin\test_prefix.exe
# Expected Output:
# [CONFIRMED DEFECT] Pre-fix child timed out after 1500ms as expected due to infinite recursion.
# Expected Process Exit Code: 0

# 2. Run Post-fix binary (repair verification):
.\bin\test_postfix.exe
# Expected Output:
# CHILD_SUCCESS: status_no_such_user=1317 status_access_denied=5 status_invalid_handle=6
# [PASS] Post-fix child succeeded with exit code 0. Error mapping and loader caching verified.
# Expected Process Exit Code: 0
```

## 5. Observed verification (2026-10-02)

On maho-win, inbox OpenSSH `9.5.6.2` left PIDs 8760 and 15092 running
with one CPU-bound thread each, no child processes, and no TCP connections.
Non-invasive CDB stacks on both processes reached
`privsep_preauth -> generate_sshd_virtual_token -> pRtlNtStatusToDosError`.
Disassembly showed the resolved function pointer being stored followed by a
backward jump, rather than a call through that pointer. This is a preauth error
reporting failure, not evidence of a Ferryx transport teardown leak.

Remote GCC build with `-Wall -Wextra -Werror -O2` and both regression executables
completed with exit code 0. The original function timed out; the corrected
function returned 1317, 5, and 6 using real ntdll. A subsequent process inventory
confirmed neither test executable remained running.

The Mac language server cannot validate this Windows-only test because its
SDK has no `windows.h`; the remote Windows compiler is the validation gate.
Full sshd build and deployment are separate from this function regression.
No production SSH service or existing terminal session was changed by these tests.
