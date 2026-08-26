# C1 evidence - Rust plugin, config, and capability wiring

Command: `cargo test --manifest-path src-tauri/Cargo.toml --test updater_config_contract`
Test file: `src-tauri/tests/updater_config_contract.rs`
Captured 2026-08-26T13:26:18.212216

## RED (before any source edit)

The contract test was written first and run against the untouched repository.

```
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- default_capability_grants_updater_commands stdout ----

thread 'default_capability_grants_updater_commands' (5375763) panicked at tests/updater_config_contract.rs:70:5:
the main webview needs updater:default to call the updater commands, got [String("core:default"), String("core:window:allow-start-dragging"), String("dialog:allow-open"), String("notification:default"), String("notification:allow-is-permission-granted"), String("notification:allow-request-permission"), String("notification:allow-notify")]

---- updater_plugin_is_a_dependency_and_is_registered stdout ----

thread 'updater_plugin_is_a_dependency_and_is_registered' (5375766) panicked at tests/updater_config_contract.rs:78:5:
src-tauri/Cargo.toml must depend on tauri-plugin-updater

---- updater_plugin_config_pins_endpoint_and_signing_key stdout ----

thread 'updater_plugin_config_pins_endpoint_and_signing_key' (5375765) panicked at tests/updater_config_contract.rs:24:5:
tauri.conf.json must configure plugins.updater

---- updater_installs_without_prompting_on_windows stdout ----

thread 'updater_installs_without_prompting_on_windows' (5375764) panicked at tests/updater_config_contract.rs:47:5:
assertion `left == right` failed: the Windows installer runs passively so an update needs no extra user interaction
  left: Null
 right: "passive"


failures:
    bundler_emits_updater_artifacts
    default_capability_grants_updater_commands
    updater_installs_without_prompting_on_windows
    updater_plugin_config_pins_endpoint_and_signing_key
    updater_plugin_is_a_dependency_and_is_registered

test result: FAILED. 0 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `--test updater_config_contract`
```

## GREEN (after wiring)

Changes: `tauri-plugin-updater` added to `src-tauri/Cargo.toml` (desktop target block),
`tauri_plugin_updater::Builder::new().build()` registered under `#[cfg(desktop)]` in
`src-tauri/src/lib.rs`, `plugins.updater` (endpoints + pubkey + windows.installMode=passive) and
`bundle.createUpdaterArtifacts: true` added to `src-tauri/tauri.conf.json`, and `updater:default`
appended to `src-tauri/capabilities/default.json`.

```
running 5 tests
test updater_plugin_config_pins_endpoint_and_signing_key ... ok
test updater_plugin_is_a_dependency_and_is_registered ... ok
test bundler_emits_updater_artifacts ... ok
test default_capability_grants_updater_commands ... ok
test updater_installs_without_prompting_on_windows ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

VERDICT: PASS - 5 assertions RED then GREEN, exit 0.


## Restart capability correction (RED -> GREEN)

The Settings UI invokes `@tauri-apps/plugin-process` to relaunch after the updater finishes. A
release-readiness review found that `process:default` was not granted in the default capability, so
this would have been rejected at the real JavaScript command boundary despite the update downloading
correctly. The existing Rust config contract was strengthened first:

RED:
```
_contract.rs:74:5:
the main webview needs process:default to relaunch after installing an update, got [String("core:default"), String("core:window:allow-start-dragging"), String("dialog:allow-open"), String("notification:default"), String("notification:allow-is-permission-granted"), String("notification:allow-request-permission"), String("notification:allow-notify"), String("updater:default")]
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    default_capability_grants_updater_commands

test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `--test updater_config_contract`
```

After adding `process:default` to `src-tauri/capabilities/default.json` (and regenerating the schema):
```
|                   ^^^^^^^^^^^^^^^^^^^^

warning: `ferryx` (lib) generated 4 warnings
    Finished `test` profile [unoptimized + debuginfo] target(s) in 16.94s
     Running tests/updater_config_contract.rs (src-tauri/target/debug/deps/updater_config_contract-2517df64c93672c3)

running 5 tests
test bundler_emits_updater_artifacts ... ok
test default_capability_grants_updater_commands ... ok
test updater_plugin_is_a_dependency_and_is_registered ... ok
test updater_installs_without_prompting_on_windows ... ok
test updater_plugin_config_pins_endpoint_and_signing_key ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

VERDICT: PASS - both the updater command and the post-install relaunch command are now authorized.
