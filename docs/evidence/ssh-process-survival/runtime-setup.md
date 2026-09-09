# SSH Process Survival Evidence: Remote Helper Installation & SSH-Independent Startup

**Date:** 2026-09-09  
**Lane:** Phase B-2 `runtime-setup`  
**Worker:** hephaestus (`st_01a08446`)  
**Scope:** `src-tauri/src/ssh/helper_setup.rs`, `src-tauri/src/ssh/helper_setup_tests.rs`, `src-tauri/src/ferryx_scope/ssh/process.rs`, `scripts/qa/ssh-helper-setup.mjs`, `docs/evidence/ssh-process-survival/runtime-setup.md`.

---

## 1. Executive Summary & Contract for Next Node

Phase B-2 implements explicit remote helper installation and SSH-independent startup lifecycle:

### Contract Implementation:
1. **Serializable Location DTO:**
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
   #[serde(rename_all = "camelCase")]
   pub struct HelperLocation {
       pub executable: String,
       pub root: String,
   }
   ```
2. **Deterministic Collision-Resistant Location (`default_location`):**
   - Derives private canonical locations from configured `host.id` via standard SHA-256 (`sha2` crate) plus safe prefix:
     `${safe_prefix}-${sha256(host_id)[..32]}`.
   - Prevents collisions across hostile or aliased names (e.g. `a:b` vs `a/b` vs `a_b`).
   - Normalizes trailing slashes and validates paths through `RemotePlatform::validate_path`.
   - Never depends on local filesystem paths or ephemeral daemon epochs.
   - POSIX: `${home}/.ferryx/bin/ferryx-remote-helper` and `${home}/.ferryx/helper/${slug}`
   - Windows: `${home}\.ferryx\bin\ferryx-remote-helper.exe` and `${home}\.ferryx\helper\${slug}`
3. **Explicit Caller-Selected Binary Installation (`install`):**
   - Reads local binary with blocking offload (`tokio::task::spawn_blocking`).
   - Streams caller-provided binary through `direct::ssh_plan` and `direct::bounded_output_with_stdin`.
   - Never attempts automatic remote source compilation or unverified downloads.
   - Stages to private temporary file before atomic rename (`0700` permissions on POSIX, private ACL on Windows).
   - **Publication Safety on Windows:** Stages binary into private temporary file, strips inheritance and grants Full control only to `$($env:USERNAME)`, creates backup of existing binary before moving, and restores backup if move fails.
4. **SSH-Independent Detached Startup (`ensure_started` & `process::start`):**
   - **Private IPC Invariant:** Validates `validate_private(&root)` before ANY write. Symlinks, wrong ownership, and insecure permissions fail closed immediately without writing or truncating any files.
   - **Diagnostic Logging:** Uses ephemeral private log file `startup-<uuid>.tmp` created with `create_new(true)`, `0o600`, and `O_NOFOLLOW` on Unix; unlinked on readiness or failure.
   - **Session Detachment:**
     - POSIX: `libc::setsid()` in `pre_exec` creates new session leader without controlling terminal. No global SIGHUP suppression in `daemon()`, allowing spawned PTY shells to manage their own signals.
     - Windows: `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`, null stdio. Note: Windows OpenSSH assigns SSH child processes to a job object; break-away handling (`CREATE_BREAKAWAY_FROM_JOB`) is owned by lead in `process.rs`.
   - **Authenticated Readiness (`is_live`):**
     - Reads private endpoint file and connects to loopback address with 500ms timeout.
     - Performs framed `handshake` request with authenticated token and validates `ok: true`, `protocol: 1`, and `hostId == expected_host`.
   - **Bounded Child Reaping:**
     - On startup failure or timeout, kills and reaps ONLY the newly spawned `child` process (`child.kill()` + `child.wait()`); never kills or disturbs an already-running helper.
5. **Actionable Typed Error for Missing Helper:**
   - Detects remote exit code 127 via structured `details.exitCode` (not prose substring matching).
   - Returns typed `IpcErrorCode::CliExecutableNotFound` with structured details:
     `{"stage": "helper_missing", "executable": ..., "root": ...}`
   - Exit code 126 maps to `IpcErrorCode::Unsupported` with `stage: "helper_permissions"`.
   - Never falls back to lossy direct SSH.

---

## 2. Lead Audit Findings & Behavioral Corrections

| Item | Finding | Correction Applied | Proof |
|---|---|---|---|
| 1 | `sanitize_host_id` collides (`a:b` vs `a/b`) and `DefaultHasher` is unstable. | Replaced with SHA-256 hashing (`sha2::Sha256`). Produces distinct deterministic roots for all host IDs. | `ssh_helper_setup_host_id_collision_resistance` test |
| 2 | `ensure_started` parsed readiness and errors with `contains` on prose. | Implemented `parse_ready_output` parsing line-by-line JSON `HelperReadyEvent`, and `map_ensure_started_error` inspecting structured `details.exitCode` (127 -> `CliExecutableNotFound`, 126 -> `Unsupported`). | 5 dedicated parser and error mapping unit tests |
| 3 | `process::start` wrote/truncated `startup.log` before validating root. | Validate `validate_private(&root)` before any writes. Ephemeral random `startup-<uuid>.tmp` created with `create_new(true)`, `0o600`, and `O_NOFOLLOW`. | `ssh_helper_setup_process_start_rejects_symlink_root_without_writing_log` proves sentinel symlink target unchanged. |
| 4 | Startup timeout leaves spawned child alive; `is_live` unauthenticated. | Child process killed and reaped on timeout (`child.kill()` + `child.wait()`). `is_live` performs authenticated framed handshake with token and verifies `hostId`. | Authenticated handshake in `is_live`, clean child reaping in timeout path. |
| 5 | `FERRYX_HELPER_BINARY` production fallback; global SIGHUP suppression. | Removed `FERRYX_HELPER_BINARY` env check; uses `std::env::current_exe()` directly. Removed `signal(SIGHUP, SIG_IGN)` from `daemon()`; documented `libc::setsid()` safety. | Zero env overrides in helper binary; standard shell signal semantics preserved. |
| 6 | Silent returns on missing binary/loopback; release binary search. | Removed silent skips and release binary search. Default library test suite is 100% deterministic and self-contained (14 tests). | 14/14 unit tests pass in 0.02s without external dependencies. |
| 7 | QA script used `execSync`, killed PIDs without awaiting exit. | Refactored `ssh-helper-setup.mjs` to track exact-owned child processes, read daemon PID from `endpoint.json`, and await process exit (`waitForPidExit`) with deadline. Zero `execSync`. | Clean async teardown verified with `cleanup-receipt`. |
| 8 | Windows install lacked private ACL and deleted destination before move. | Added `icacls` private ACL step to `$tmp` before publication; added backup/restore rollback preserving prior binary if move fails. | Hardened PowerShell installation script. |

---

## 3. Verification Evidence

### A. Focused Cargo Suite: `ssh_helper_setup`
Command:
```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib ssh_helper_setup
```
Output:
```
running 14 tests
test ssh::helper_setup::tests::ssh_helper_setup_default_location_rejects_invalid_home ... ok
test ssh::helper_setup::tests::ssh_helper_setup_readiness_parser_rejects_malformed_json ... ok
test ssh::helper_setup::tests::ssh_helper_setup_process_start_rejects_symlink_root_without_writing_log ... ok
test ssh::helper_setup::tests::ssh_helper_setup_ensure_started_exit_code_127_maps_to_missing_helper ... ok
test ssh::helper_setup::tests::ssh_helper_setup_ensure_started_exit_code_126_maps_to_unsupported_permissions ... ok
test ssh::helper_setup::tests::ssh_helper_setup_readiness_parser_rejects_missing_ready_event ... ok
test ssh::helper_setup::tests::ssh_helper_setup_readiness_parser_valid_ready ... ok
test ssh::helper_setup::tests::ssh_helper_setup_ensure_started_rejects_invalid_paths ... ok
test ssh::helper_setup::tests::ssh_helper_setup_location_serde_round_trip ... ok
test ssh::helper_setup::tests::ssh_helper_setup_default_location_posix ... ok
test ssh::helper_setup::tests::ssh_helper_setup_host_id_collision_resistance ... ok
test ssh::helper_setup::tests::ssh_helper_setup_default_location_windows ... ok
test ssh::helper_setup::tests::ssh_helper_setup_install_rejects_missing_local_binary ... ok
test ssh::helper_setup::tests::ssh_helper_setup_install_rejects_empty_local_binary ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 808 filtered out; finished in 0.02s
```

### B. Standalone Remote Helper Suite
Command:
```bash
cargo test --manifest-path remote-helper/Cargo.toml
```
Output:
```
running 23 tests
test scoped_contracts::tests::epoch_rejects_noncanonical_or_out_of_range_wire_values ... ok
test scoped_contracts::tests::producer_boundaries_roundtrip ... ok
test scoped_contracts::tests::shared_envelopes_roundtrip ... ok
test scoped_contracts::tests::target_roundtrip_preserves_full_u64_epoch_as_string ... ok
test scoped_contracts::tests::result_rejects_mismatched_discriminants ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_rejects_shutdown_op ... ok
test ssh::process::helper_service_tests::ssh_reconnect_safety_live_runtime_cannot_be_replaced ... ok
test ssh::process::helper_service_tests::ssh_process_survival_bridge_allows_describe_without_stopping_runtime ... ok
test ssh::process::helper_service_tests::ssh_reconnect_safety_rejects_symlink_root_and_endpoint ... ok
test ssh::process::helper_service_tests::ssh_process_survival_stale_endpoint_replaced_only_after_lock_release ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_frame_bounded_below_1mib ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_spawn_dedupe_and_conflict ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_ring_gap_detection ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_respects_cols_rows ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_target_describe_and_list_metadata ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_project_root_decoupled ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_canonical_decimal_cursor ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_default_platform_login_shell ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_binary_safe_base64_io ... ok
test ssh::helper::helper_core_tests::ssh_reconnect_safety_auth_and_target_validation ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_retains_pid_and_memory_state ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_concurrent_spawn_atomic_reservation ... ok
test ssh::helper::helper_core_tests::ssh_process_survival_env_retains_session_id_without_fabrication ... ok

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
```

### C. End-to-End Loopback SSH Harness (`ssh-helper-setup.mjs`)
Command:
```bash
node scripts/qa/ssh-helper-setup.mjs
```
Output:
```json
{"event":"fixture-created","fixture":"/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/f-setup-qa-MNuF1J","binDir":"/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/f-setup-qa-MNuF1J/bin","stateDir":"/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/f-setup-qa-MNuF1J/state","hostId":"qa-setup-5c072ed8"}
{"event":"helper-installed","path":"/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/f-setup-qa-MNuF1J/bin/ferryx-remote-helper"}
{"event":"verified-missing-helper-rejection","exitCode":127}
{"event":"ssh-startup-complete","sshPid":24914,"ready":{"event":"ready","protocol":1}}
{"event":"verified-detached-daemon-alive","daemonPid":24921,"epoch":"1031312981022991947","hostId":"qa-setup-5c072ed8"}
{"event":"pty-spawned","ptyPid":24923,"target":{"backendSessionId":"f61105a9-a7c2-4a69-86a8-f1eb4354a664","epoch":"1031312981022991947","hostId":"qa-setup-5c072ed8","ownerId":"9cb0faea-f794-4f79-94d9-0fa474438203"}}
{"event":"verified-step-1","ptyPid":24923}
{"event":"verified-step-2-after-reconnect","ptyPid":24923}
PASS: SSH detached helper startup, survival after SSH parent death, and idempotent preserve verified
{"event":"pid-reaped","label":"helper-daemon","pid":24921}
{"event":"cleanup-receipt","fixtureRemoved":"/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/f-setup-qa-MNuF1J","reapedDaemonPid":24921,"reapedPtyPid":24923}
```

---

## 4. Windows Diagnostic & Handoff Summary

- Direct CLI execution on `maho-win` verified that `ferryx-remote-helper.exe start` correctly binds `runtime.lock`, stages `endpoint.json` with PID, and exits 0.
- Lead's concrete Windows diagnosis established that OpenSSH server on Windows retains the connection because Windows assigns SSH child processes to a job object. Local process creation flags (`DETACHED_PROCESS`) do not break away from Windows job membership.
- Lead has taken back `process.rs` Windows spawn flags for a targeted `CREATE_BREAKAWAY_FROM_JOB` correction.
- In accordance with lead instruction:
  - No edits made to `process.rs` Windows spawn flags.
  - All temporary test directories on `maho-win` (`test-build-probe`, `test-root-direct`) and background daemons (`17364`, `19300`, `27768`) were stopped and reaped.
  - Zero lingering processes or fixtures remain on `maho-win` or local workstation.
