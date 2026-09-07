# Web Remote Security Gate Review Report

**Goal**: Repair four confirmed web remote security vulnerabilities in Ferryx:
1. Static path traversal in `serve_static_or_index` allowing arbitrary file read outside UI dist.
2. Device revocation not terminating already-open WebSocket connections (events, raw terminal, native grid).
3. View-permission devices being able to revoke other devices.
4. Absence of rate limiting / attempt budgets on the 6-digit pairing PIN exchange.

**Recommendation**: **APPROVE**
**Confidence**: **HIGH**
**Reviewer**: `omo-senpi-gate-reviewer` (`st_01a07921`)
**Timestamp**: 2026-09-06 23:55 UTC
**Worktree**: `/Users/indo/code/project/ferryx-web-remote-security-20260907`

---

## 1. Executive Summary & Original Intent

### Original Intent
The user requested "모두 수정하고 재검토" (Repair all and re-review) following the failed security review on 2026-09-06, where an unauthenticated GET request with path traversal leaked `/etc/hosts`, device revocation allowed existing WebSockets to linger indefinitely and process I/O, View-only devices could delete other paired devices, and 6-digit pairing PINs had no attempt budget.

### Desired Outcome
- Complete containment of static file requests within the canonical `dist` directory across macOS, Windows, and Linux.
- Immediate termination of all active WebSocket streams (events, raw terminal, native grid) upon device revocation, including cancellation of in-flight input and prevention of upgrade callback races.
- Strict enforcement of device permissions: View devices can self-revoke (HTTP 204), but cannot revoke any other device (HTTP 403). Control devices retain full device revocation capabilities.
- Atomic, global rate limiting on pairing attempts (5 failure budget per 60-second window, returning HTTP 429 `{"code":"pairing_rate_limited"}` upon exhaustion), with late-generated PINs retaining their full 60-second validity.
- Complete isolation within the dedicated worktree; no git commits; no modifications to main checkout; preservation of focus-independent pane inventory; zero test flakiness or sleeps.

### User Outcome Review
All four security vulnerabilities have been completely and robustly repaired. Legitimate user operations—pairing new devices, streaming events, controlling PTYs via raw or native grid WebSockets, accessing assets, and SPA routing—continue to function seamlessly. Adversarial probes are decisively blocked:
1. Directory traversal payloads (`/../../`, `%2e%2e`, double encoded, Windows ADS/drives, backslashes, NUL bytes, symlinks) return HTTP 400 Bad Request or HTTP 404 Not Found without leaking bytes.
2. Revoking a device instantly drops its event and terminal WebSockets, cancels pending PTY input futures, and denies any pending WebSocket upgrade callbacks.
3. View devices attempting to revoke other devices are blocked with HTTP 403 Forbidden.
4. Brute-force guessing of pairing PINs is locked out on the 6th attempt with HTTP 429 Too Many Requests, while valid late-generated PINs continue to pair successfully within their 60s lifetime.

---

## 2. Goal Breakdown & Completeness

| Requirement | Status | Evidence & Implementation Details |
|---|---|---|
| **1. Static Path Traversal Blocked** | **ACHIEVED** | `src-tauri/src/remote/server.rs:1424-1482`. `static_relative_path` percent-decodes once, strictly validates against portable invalid characters (`\`, `:`, `%`, `\0`), leading slashes, and `.`/`..` path components, returning HTTP 400 on violations. Canonical asset root containment (`tokio::fs::canonicalize` and `.starts_with(&root)`) prevents symlink escapes (returning HTTP 404). Valid assets and SPA fallback to `index.html` are preserved. Tested in `security_tests.rs:59-150` and live reproduced via QA fixture. |
| **2. Device Revocation Socket Invalidation** | **ACHIEVED** | `src-tauri/src/remote/auth.rs:74, 199-232`; `src-tauri/src/remote/server.rs:826-859, 897-940, 979-1078, 1148-1366`. `AuthManager` maintains a latched `tokio::sync::watch` channel per device. Handlers wrap sockets and attachment in `while_device_authorized` with `tokio::select! { biased; ... }`. Sockets convert background pumps from detached `tokio::spawn` to locally pinned futures (`std::pin::pin!`), ensuring prompt cancellation of in-flight input and socket drops. Tested in `security_socket_tests.rs:125-273` across events, raw terminal, and native grid. |
| **3. View Device Permission Boundaries** | **ACHIEVED** | `src-tauri/src/remote/server.rs:796-815`. In `revoke_device`, the caller's validated `DeviceInfo` is inspected: if `device.permission != DevicePermission::Control && device.id != device_id`, it immediately returns `(StatusCode::FORBIDDEN, "View-only device cannot revoke another device")`. View devices self-revoking succeed with HTTP 204. Tested in `security_tests.rs:152-218` and live reproduced via QA fixture. |
| **4. Pairing Attempt Rate Limiting** | **ACHIEVED** | `src-tauri/src/remote/auth.rs:10, 41-60, 128-145`; `src-tauri/src/remote/server.rs:212-218`. `PairingWindow` tracks `failures: u8` and `codes: HashMap` behind a single `parking_lot::RwLock`. 5 failure budget per 60s window. Both invalid and expired attempts increment the budget. After 5 failures, attempts return `AuthError::PairingRateLimited` mapped to HTTP 429 JSON `{"code":"pairing_rate_limited"}`. Window refresh retains non-expired codes (`now.duration_since(created_at) < 60s`). Tested in `auth_security_tests.rs:3-103`, `security_tests.rs:220-256`, and live reproduced. |

---

## 3. Constraint Compliance

| Constraint | Status | Evidence |
|---|---|---|
| **Cross-platform premise (macOS, Windows, Linux)** | **ACHIEVED** | Path normalization explicitly checks and rejects Windows backslashes (`\`), drive colons (`C:`), ADS streams (`:`), and UNC paths on all hosts. Asset discovery uses `crate::ipc::run_blocking` to prevent reactor thread blocking. Tested across Unix and Windows bundle layout tests (`tests.rs`). |
| **Confined to isolated review worktree** | **ACHIEVED** | All edits are inside `/Users/indo/code/project/ferryx-web-remote-security-20260907`. `git status` in main checkout `/Users/indo/code/project/orca-lite` shows no remote security changes. |
| **No git commits, no edits to main checkout** | **ACHIEVED** | `git log -n 1` in worktree is pinned to baseline commit `37272f5`. Working tree changes remain uncommitted. Main checkout untouched. |
| **Retain focus-independent terminal tab/pane inventory** | **ACHIEVED** | `server.rs` inventory queries and `WorkspaceRegistry` unchanged. `scoped_remote_inventory_lists_registered_projects_without_desktop` passes cleanly. |
| **Do not weaken or delete existing tests; all remote tests pass** | **ACHIEVED** | All 86 cargo unit tests in `remote::` pass (0 failed, 0 ignored). UI vitest suite has 117 passing tests across 8 test files. Scoped control failure (`control::lease::rejects_competing_controller_until_expiry_or_revoke`) was verified to be pre-existing in baseline commit `37272f5` with identical SHA-256. |
| **No fixed sleeps or timing-luck tests; deterministic synchronization** | **ACHIEVED** | Zero `sleep` calls introduced in tests. Synchronization uses `std::sync::Barrier`, `tokio::sync::Notify`, atomic counters, and bounded `tokio::time::timeout` deadlines. |

---

## 4. Manual QA Matrix Audit & Live Verification

| # | Scenario | Expected | Observed | Verdict | Evidence Artifact & Live Reproduction |
|---|---|---|---|---|---|
| 1 | Static path traversal blocked | HTTP 400 Bad Request, 0 bytes leaked, normal assets/index served 200 OK | HTTP 400 Bad Request (0 bytes), index.html served 200 OK, CSS asset served 200 OK | **PASS** | `docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-1`. Live reproduction via QA fixture: `curl --path-as-is` returned HTTP 400 Bad Request. |
| 2 | Device revocation invalidates open sockets | Device A events + terminal WebSockets close immediately on revocation; Device B unaffected | Device A events and terminal WebSockets closed immediately (code 1006); Device B remained open and received broadcast | **PASS** | `docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-2`. Validated via automated test suite `security_socket_tests.rs` (10 passed tests). |
| 3 | View device permission boundaries | Viewer revoking victim returns 403; Viewer self-revoking returns 204; token rejected 401 after | HTTP 403 on revoking victim, victim remained paired; HTTP 204 on self-revoke, subsequent calls returned 401 | **PASS** | `docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-3`. Live reproduction via QA fixture: Viewer revoking victim gave HTTP 403, Viewer self-revoke gave HTTP 204, subsequent call gave HTTP 401. |
| 4 | Pairing attempt rate limiting | 5 invalid PINs return 400; 6th attempt returns HTTP 429 `{"code":"pairing_rate_limited"}` | First 5 attempts returned 400; 6th attempt returned HTTP 429 `{"code":"pairing_rate_limited"}` locking out even valid PIN | **PASS** | `docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-4`. Live reproduction via QA fixture: attempts 1-5 returned HTTP 400, attempt 6 returned HTTP 429 with expected JSON body. |

---

## 5. Skills Compliance Pass (`remove-ai-slops` & `programming`)

- **Overfit / Slop Pass**:
  - No excessive or useless tests: every new test targets an exact concurrency, boundary, or security failure mode.
  - No deletion-only tests or tests that merely verify code removal.
  - No tautological tests or mock-heavy isolation: all 23 security tests exercise real loopback HTTP servers, real Axum routing, real `AuthManager`, and real WebSocket framing.
  - No implementation-mirroring tests: assertions verify status codes, payload schemas, and connection state transitions.
  - No unnecessary extraction or normalization: `static_relative_path` performs minimal, direct validation; `PairingWindow` and `while_device_authorized` are compact and single-purpose.
- **Programming & Architecture Pass**:
  - Idiomatic concurrency: `parking_lot::RwLock` for fast synchronous state; `tokio::sync::watch` for broadcast cancellation without channel drops; `std::pin::pin!` avoids runtime task leakage.
  - Lock ordering and safety explicitly documented to prevent deadlocks: devices lock acquired prior to revocations registry lock.
  - No scope creep: purely targeted to the 4 stated vulnerabilities.

---

## 6. Blocking Issues

*None.* (Recommendation is **APPROVE**).

---

## 7. Notes & Observations

- **Pre-existing test failure**: `control::lease::rejects_competing_controller_until_expiry_or_revoke` in `tests/scoped_control.rs` fails, but was verified to be pre-existing in baseline commit `37272f5` (identical file hash `5c34443223fc35f8dd04b12030a66d81a519369b634e546728d9b6eaa502831d`). It does not use the remote server/auth code.
- **Cargo warnings**: 7 pre-existing warnings in unrelated modules (such as unused `Manager` import in notifications, unused `super_key` in native terminal tests, unused `wait_and_reap` in PTY session) remain unmodified, avoiding unrelated diff churn.
- **Decoupled PIN lifetime**: The parent's review finding regarding PIN lifetime truncation at rate-window rollover was properly addressed: `PairingWindow::refresh` retains codes whose age is under 60 seconds while resetting the failure counter.
