# Fix Report: P-auth-persist / F-remote-01

## Problem
In `src-tauri/src/remote/auth.rs`, `validate_token` was updating `device.last_seen_at` and unconditionally invoking `self.persist_best_effort()` on every authenticated request. `persist_best_effort()` cloned the entire device and token maps, serialized them to pretty-printed JSON, and synchronously wrote them to disk with temporary file creation, permissions setting, and atomic rename. Under concurrent mobile requests, this created unnecessary disk I/O and latency spikes.

## Solution
1. Defined `LAST_SEEN_PERSIST_INTERVAL = Duration::from_secs(60)`.
2. Added `last_persisted_at: Arc<RwLock<Instant>>` to `AuthManager` to track the timestamp of the most recent disk persistence.
3. Updated `validate_token` to only invoke `self.persist_best_effort()` if `self.last_persisted_at.read().elapsed() >= LAST_SEEN_PERSIST_INTERVAL`, keeping rapid validation in-memory.
4. Preserved immediate persistence for security-critical actions: `exchange_pairing_code` (device pairing) and `revoke_device` (device revocation).
5. Added unit test `validate_token_throttles_disk_persistence` verifying that pairing and revocation persist immediately, while rapid token validations execute purely in-memory without synchronous disk writes unless >=60s has elapsed.

## Changes (file:line)
- `src-tauri/src/remote/auth.rs:10`: Added `const LAST_SEEN_PERSIST_INTERVAL: Duration = Duration::from_secs(60);`.
- `src-tauri/src/remote/auth.rs:48`: Added `last_persisted_at: Arc<RwLock<Instant>>` to `AuthManager`.
- `src-tauri/src/remote/auth.rs:69`: Initialized `last_persisted_at` in `AuthManager::with_persistence`.
- `src-tauri/src/remote/auth.rs:145-148`: Added elapsed time check against `LAST_SEEN_PERSIST_INTERVAL` in `validate_token`.
- `src-tauri/src/remote/auth.rs:175`: Updated `last_persisted_at` on persistence in `persist_best_effort`.
- `src-tauri/src/remote/auth.rs:167-170`: Added `#[cfg(test)] pub(crate) fn set_last_persisted_at` for deterministic time testing.
- `src-tauri/src/remote/auth.rs:242-277`: Added `validate_token_throttles_disk_persistence` test.

## RED Phase Output
Command:
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib remote::auth
```

Tail:
```text
running 2 tests
test remote::auth::persistence_tests::paired_devices_and_revocations_survive_reopen ... ok
test remote::auth::persistence_tests::validate_token_throttles_disk_persistence ... FAILED

failures:

---- remote::auth::persistence_tests::validate_token_throttles_disk_persistence stdout ----

thread 'remote::auth::persistence_tests::validate_token_throttles_disk_persistence' (174187577) panicked at src/remote/auth.rs:261:9:
validate_token must not persist to disk on every request
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    remote::auth::persistence_tests::validate_token_throttles_disk_persistence

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 115 filtered out; finished in 0.09s
```

## GREEN Phase Output
Command:
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib remote::auth
```

Tail:
```text
running 2 tests
test remote::auth::persistence_tests::paired_devices_and_revocations_survive_reopen ... ok
test remote::auth::persistence_tests::validate_token_throttles_disk_persistence ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 126 filtered out; finished in 0.02s
```

Command:
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib remote::
```

Tail:
```text
running 8 tests
test remote::tests::test_auth_manager_pairing_and_revocation ... ok
test remote::tests::test_tailscale_status_parsing ... ok
test remote::tests::test_remote_server_health_and_lifecycle ... ok
test remote::state::tests::gateway_config_survives_reopen ... ok
test remote::tests::test_remote_server_serves_spa_index_html ... ok
test remote::auth::persistence_tests::validate_token_throttles_disk_persistence ... ok
test remote::auth::persistence_tests::paired_devices_and_revocations_survive_reopen ... ok
test remote::tests::test_derive_session_metadata_matches_workspace ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 117 filtered out; finished in 0.13s
```
