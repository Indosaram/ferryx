# Phase B Verdict: SSH Bridge & Runtime Setup Transport Layer

**Date:** 2026-09-09  
**Verdict:** **PASS for Narrowly Proven Client/Helper Layer (Phase B)**  
**Auditor:** hephaestus (`st_01a08485`)  
**Scope Boundary:** Strictly evaluates Phase B (`ssh-bridge`, `runtime-setup`, and `verify-bridge`) following lead repairs. Does NOT certify unbuilt Phase C (daemon restart/reattach), Phase D (UI recovery/restoration), or Phase E (end-to-end application lifecycle).

---

## 1. Producer Scopes Reconstructed (Phase B)

From `docs/SSH_PROCESS_SURVIVAL_PLAN.md`:

1. **`ssh-bridge` (Phase B-1):**
   - Source: `src-tauri/src/ssh/bridge.rs`, `src-tauri/src/ssh/bridge_tests.rs`, `src-tauri/src/ssh/direct.rs`, `src-tauri/src/ssh/direct_tests.rs`.
   - Async framed OpenSSH client over real non-TTY SSH tunnels (`direct::bridge_plan`, `direct::bridge_command`).
   - Wire framing bounded at `<= 1 MiB` (`MAX_FRAME = 1024 * 1024`), 4-byte big-endian header, rejecting oversized frames without allocation.
   - Dual independent connections: `control` (handshake, spawn, describe, write, resize, stop) and `reader` (dedicated long-poll `pty.read` to prevent head-of-line blocking).
   - Strict canonical decimal `u64` remote chunk cursor (`RemoteCursor`), tracking remote ring buffer chunk sequence (distinct from local byte sequence).
   - Single-attempt mutation semantics: `pty.write` and `pty.stop` fail immediately upon connection severing and are never queued or replayed across reconnect.
   - Transparent non-killable `RemotePid(u32)` newtype preventing local POSIX signal misdirection.
   - OpenSSH plan hardening: `-T`, `BatchMode=yes`, `StrictHostKeyChecking=yes`, `UpdateHostKeys=no`, `ConnectTimeout=5`.
   - Lifecycle-poisoning on timeout/error/cancellation before I/O, ensuring stale responses cannot corrupt subsequent requests.

2. **`runtime-setup` (Phase B-2):**
   - Source: `src-tauri/src/ssh/helper_setup.rs`, `src-tauri/src/ssh/helper_setup_tests.rs`, `src-tauri/src/ferryx_scope/ssh/process.rs`, `src-tauri/src/ferryx_scope/ssh/process/process_windows.rs`.
   - Deterministic collision-resistant helper location derived from SHA-256 host hash (`host_slug`).
   - Explicit caller-selected binary installation (`install`); strictly no automatic remote compilation or unverified downloads.
   - Staged atomic installation with private permissions (`0700` POSIX, private `icacls` ACL with backup/rollback on Windows).
   - Detached startup (`ensure_started`): POSIX uses `libc::setsid()` in pre-exec; Windows uses WMI/CIM process creation (`Win32_Process.Create`) outside the SSH job object.
   - Authenticated readiness check (`is_live`): loopback connection with authenticated token handshake and `hostId` validation.
   - Actionable typed errors: remote exit code 127 maps to `IpcErrorCode::CliExecutableNotFound`, 126 to `IpcErrorCode::Unsupported` (permissions). Never falls back to lossy direct SSH.

3. **`verify-bridge` (Phase B-3 / Verifier):**
   - Real OpenSSH loopback and remote host execution proving helper daemon survival, PTY survival, and PID/nonce/counter continuity across severed SSH connections on macOS, Linux (`omarchy`), and Windows (`maho-win`).

---

## 2. Windows Detached Startup Resolution & Evidence Audit

### A. The RED State
During initial Windows verification, the OpenSSH parent process (`ssh.exe`) remained hung for over 15 minutes after `ferryx-remote-helper start` completed. Windows OpenSSH assigns incoming SSH sessions and all child processes to a Windows Job Object. When standard process creation or console detachment (`DETACHED_PROCESS`) was used, the helper daemon process remained pinned inside the SSH job object, preventing OpenSSH from closing the connection upon session exit.

### B. Rejected Flag-Only Breakaway
An isolated test using `CREATE_BREAKAWAY_FROM_JOB` allowed the SSH start command to exit 0. However, subsequent SSH bridge connection attempts failed immediately with Windows error 10061 (connection refused), and process queries confirmed no running helper. **Flag-only breakaway failed and is rejected; it must not be recommended.**

### C. Proven Lead Repair: Tested CIM Creation Outside SSH Job
The production start path in `src-tauri/src/ferryx_scope/ssh/process/process_windows.rs` was refactored to invoke WMI/CIM:
- Creates `Win32_ProcessStartup` instance with `CreateFlags = 8` (`DETACHED_PROCESS`).
- Calls `Invoke-CimMethod -ClassName Win32_Process -MethodName Create`.
- Because WMI process creation is serviced by the WMI provider service (`wmiprvse.exe`), the helper daemon is created in an independent process hierarchy outside the OpenSSH session's job object.
- Stdio byte probe verified in `windows-native-byte-probe.md`: Windows OpenSSH binary pipeline preserves null bytes, non-UTF-8 bytes, CR, and LF (`00ff800a0d41`) without PowerShell text stream corruption.

### D. Lead Windows Artifact Proof
Raw artifacts confirm successful execution, survival, and teardown:
1. **`windows-cim-build.log`:** Debug remote helper compiled cleanly in 5.04s.
2. **`windows-cim-start.log`:** `ferryx-remote-helper start` returned `{"event":"ready","protocol":1}`, and its parent SSH process exited 0 immediately.
3. **`windows-cim-session.log`:**
   - A subsequent, independent OpenSSH connection authenticated to the running helper (handshake returned protocol 1, epoch `3930053837692487182`, host `qa-job-b20977b8-da14-4961-b1d5-fad16d809f8f`).
   - Spawned test process (remote PID 27964, nonce `e5f64559-1c4e-4fae-a6e1-ca2fd5cb951c`, counter = 1).
   - Local QA SSH transport (PID 47164) was severed (exit 137).
   - A fresh OpenSSH connection reconnected to the exact same target ref (`backendSessionId: dc0ad535-d7d4-4f0f-96ad-6b02d0ba2842`), verified identical PID 27964 and nonce `e5f64559-1c4e-4fae-a6e1-ca2fd5cb951c`, incremented mutable in-memory counter 1 -> 2, with exactly 1 spawn request recorded (`spawnRequests: 1`).
   - The remote PTY was explicitly stopped (`{"event":"remote-pty-stopped","pid":27964}`).
4. **`windows-cim-cleanup.log`:** Exact QA helper PID 20324 reaped, unique fixture directory and temporary archives deleted (`WINDOWS_CIM_FIXTURE_CLEANED`).
5. **`windows-old-qa-cleanup.log`:** Abandoned earlier fixture cleaned (`OLD_QA_FIXTURE_CLEANED`); zero leaked helper processes remaining.

---

## 3. Cross-Platform Verification & Audit Log Evidence

| Platform / Target | Log Artifact | Results | Status |
|---|---|---|---|
| **Native Unit / Lifecycle** | `bridge-lifecycle-audit.log` | 13 passed in 1.15s (`ssh::bridge::tests::*`, `ssh::direct::tests::*`). Zero helper processes leaked before/after. | **PASS** |
| **Native Helper Regression** | `startup-recovery-native.log` | 23 passed in 0.28s (`ferryx-remote-helper` standalone crate). | **PASS** |
| **Integrated Cargo Check** | `startup-recovery-check.log` | Crate `ferryx` passed `cargo check` in 6.34s (pre-existing warnings retained). | **PASS** |
| **Linux Remote (`omarchy`)** | `bridge-verification-linux.log` | 23 remote tests passed; real SSH severed (dead PID 23387); reconnected to identical remote PID 2996484; counter 1 -> 2 -> 3; TargetRef and epoch preserved. | **PASS** |
| **Windows Remote (`maho-win`)** | `windows-cim-start.log`, `windows-cim-session.log` | Start SSH exited 0; separate OpenSSH reconnected to remote PID 27964, nonce `e5f64559-1c4e-4fae-a6e1-ca2fd5cb951c`, counter 1 -> 2 with 1 spawn. | **PASS** |
| **SSH Trust Integrity** | `ssh-trust-baseline.md` vs logs | Hashes for `known_hosts` (`cf5b6b02...`) and `config` (`e388a32e...`) match baseline before and after runs. `UpdateHostKeys=no` strictly enforced. | **PASS** |

---

## 4. Remaining Verification Weaknesses & Missing Obligations

To avoid inflating Phase B completion into claims of full end-to-end survival, the following specific weaknesses and incomplete boundaries are documented:

1. **Polling Cleanup in Earlier Native Setup Script (`scripts/qa/ssh-helper-setup.mjs`):**
   - The test script's `reapPid` function uses a polling loop (`while (Date.now() < deadline) { await new Promise((r) => setTimeout(r, 50)); ... }`) polling `process.kill(pid, 0)` rather than event-driven process handle completion.
   - The `rmRetry` function relies on a fixed polling delay (`setTimeout(r, 100)`) against file lock release races.
   - This violates the project test discipline ("fixed sleeps, polling delays, and wait-for-time patterns are forbidden; tests must not pass by timing luck"). Future test harnesses (`scripts/qa/ssh-process-survival.mjs` for Phase D/E) must replace polling loops with event-driven notifications or bounded OS-level wait handles.

2. **Unbuilt Downstream Phases (Explicit Boundaries):**
   - **Phase C (Daemon Reattach):** Desktop daemon restart, persistent TargetRef/cursor mapping in desktop store, and local daemon reconnect handling are NOT implemented in Phase B.
   - **Phase D (UI Reattach):** Frontend terminal reattachment (`TerminalPane.tsx`, `workspaceStore.ts`), input freeze during outage, and replacement of `shellReplacement` bypass are NOT implemented in Phase B.
   - **Phase E (End-to-End Scenarios):** Debug application and final scenario verification remain pending Phases C and D. Release builds are not authorized.

The two exploratory setup/bridge JavaScript harnesses are not accepted final
regression runners: their polling cleanup must be replaced by event-driven
teardown before use in the final scenarios. Their files remain outside the
verified production increment. The lead's native startup check instead
subscribed to socket closure before signaling exact helper PID 50639, observed
closure, verified PID absence and removed `/tmp/ferryx-start-audit.H6nd7O`.

---

## 5. Final Verdict

**PASS for Narrowly Proven Client/Helper Layer (Phase B)**

The Phase B obligations are met:
- Async framed OpenSSH client with dual connections, 1 MiB framing, and canonical decimal cursors is implemented and tested.
- Detached remote helper installation and startup operate cleanly on POSIX (via `setsid`) and Windows (via CIM `Win32_Process.Create` outside OpenSSH jobs).
- Real loopback and remote transport severing verify identical remote PID, nonce, and mutable memory state retention across reconnect without extra spawns.
- Zero QA processes or fixtures were leaked, and SSH host trust baselines were preserved.
- Phase C may proceed based on this verified transport contract.
