# Phase 4 Task 8 - Production Fail-Open Fix & Verification Report

## Scope & Target
- Production scope:
  - `src-tauri/src/account/service.rs`: add error-propagating `try_enrolled_machine_by_id`, preserve `enrolled_machine_by_id` behavior for existing non-critical callers.
  - `src-tauri/src/remote/relay_server.rs`: in `bind_machine_key`, when `account_state` is present and billing is enabled, perform machine lookup via `try_enrolled_machine_by_id` and return `ACCOUNT_ENTITLEMENT_UNAVAILABLE: {code}: {message}` on store read error.
- Test scope:
  - `src-tauri/src/account/service.rs`: `try_enrolled_machine_by_id_propagates_read_error_on_corrupt_store` unit test validating `Ok(Some)`, `Ok(None)`, `Err` on corruption, and `enrolled_machine_by_id` swallowing.
  - `src-tauri/src/remote/relay_server.rs`: `billing_evaluation_failure_refuses_control_tunnel` integration test kept intact.

## Root Cause Analysis
In `bind_machine_key` (`src-tauri/src/remote/relay_server.rs:~565`):
The machine lookup called `crate::account::service::enrolled_machine_by_id(&account, &auth.machine_id)`.
`enrolled_machine_by_id` in `service.rs` executed:
```rust
state
    .read(|store| {
        Ok(store
            .machines
            .values()
            .find(|m| m.machine_id == machine_id)
            .cloned())
    })
    .ok()
    .flatten()
```
When a SQLite store read error or disk corruption occurred, `.ok().flatten()` converted the `Err` into `None`.
As a result, `account_enrolled` evaluated to `false`, the entire billing entitlement check block (`if account_enrolled { ... }`) was bypassed, and on relays without static operator machine tokens (`self.inner.machine_tokens.is_empty() == true`), the machine was admitted with `Ok(())`.
A store/billing outage therefore produced a fail-open condition allowing suspended or stopped accounts to open control tunnels without entitlement checks.

## Implementation Details
1. **`src-tauri/src/account/service.rs`**:
   - Added `try_enrolled_machine_by_id(state: &AccountState, machine_id: &str) -> Result<Option<MachineRecord>, ApiError>`.
   - Re-implemented `enrolled_machine_by_id` to delegate to `try_enrolled_machine_by_id(state, machine_id).ok().flatten()`, ensuring 100% backward compatibility for all other callers.
   - Added unit test `try_enrolled_machine_by_id_propagates_read_error_on_corrupt_store` verifying store read failure propagation on corrupt SQLite database header and `enrolled_machine_by_id` backward compatibility.

2. **`src-tauri/src/remote/relay_server.rs`**:
   - In `bind_machine_key`, when `self.account_state()` is `Some` and `deployment_mode.is_billing_enabled()` is true, machine lookup is performed with `try_enrolled_machine_by_id`.
   - On `Err(error)`, it returns early with `Err(format!("ACCOUNT_ENTITLEMENT_UNAVAILABLE: {}: {}", error.code, error.message))`.
   - When billing is disabled (e.g. Selfhost mode), it uses `enrolled_machine_by_id` without error propagation.
   - When no account state is present, `account_record` is `None`.
   - All other admission, key binding, and token validation logic remains untouched.

3. **Constraints Adherence**:
   - Selfhost and no-account relay paths remain unaffected.
   - Other callers of `enrolled_machine_by_id` and `is_account_enrolled_machine` remain unchanged.
   - No local cargo build, cargo test, bun build, or git commit run on this host.
   - Verification delegated to `maho-win`.

## Modified File Hashes (SHA-256)
- `src-tauri/src/account/service.rs`: `eb452b6c4fa9c74afe39757610ecb44de79536f082079e6da40a51de5329811a`
- `src-tauri/src/remote/relay_server.rs`: `5cdd19d1d037835bbd63f6e5c1aaa8707c8f0f5045d3b6d221c7679452a98cac`

## Changed Hunks Summary

### `src-tauri/src/account/service.rs`
```rust
@@ -487,14 +487,23 @@ pub fn enrolled_machine_by_id(state: &AccountState, machine_id: &str) -> Option<
-    state
-        .read(|store| {
-            Ok(store
-                .machines
-                .values()
-                .find(|m| m.machine_id == machine_id)
-                .cloned())
-        })
-        .ok()
-        .flatten()
+    try_enrolled_machine_by_id(state, machine_id).ok().flatten()
+}
+
+/// Looks up an enrolled machine by machine id, propagating any underlying store read error.
+///
+/// Used by relay control admission when billing evaluation is enabled so that a store read
+/// failure fails closed rather than bypassing entitlement checks.
+pub fn try_enrolled_machine_by_id(
+    state: &AccountState,
+    machine_id: &str,
+) -> Result<Option<MachineRecord>, ApiError> {
+    state.read(|store| {
+        Ok(store
+            .machines
+            .values()
+            .find(|m| m.machine_id == machine_id)
+            .cloned())
+    })
 }
```

### `src-tauri/src/remote/relay_server.rs`
```rust
@@ -560,9 +560,19 @@ impl RelayServer {
     fn bind_machine_key(&self, auth: &ControlAuth) -> Result<(), String> {
-        let account_record = self.account_state().and_then(|account| {
-            crate::account::service::enrolled_machine_by_id(&account, &auth.machine_id)
-        });
+        let account_record = if let Some(account) = self.account_state() {
+            if account.deployment_mode.is_billing_enabled() {
+                crate::account::service::try_enrolled_machine_by_id(&account, &auth.machine_id)
+                    .map_err(|error| {
+                        format!(
+                            "ACCOUNT_ENTITLEMENT_UNAVAILABLE: {}: {}",
+                            error.code, error.message
+                        )
+                    })?
+            } else {
+                crate::account::service::enrolled_machine_by_id(&account, &auth.machine_id)
+            }
+        } else {
+            None
+        };
         let account_enrolled = account_record
             .as_ref()
             .is_some_and(|record| record.public_key == auth.public_key);
```

## Status & Behavior
- Status: UNRUN (per instructions, no local build/test executed; verification delegated to maho-win)
