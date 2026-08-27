VERDICT: TITLE_CALLBACK_WORKS
EVIDENCE: `cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib --features native-terminal native_terminal_ffi_probe -- --nocapture`

Captured output tail:

```text
warning: `ferryx` (lib test) generated 5 warnings (run `cargo fix --lib -p ferryx --tests` to apply 1 suggestion)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.48s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/ferryx_lib-97341d444d4f53b7)

running 2 tests
bell_count=1
title_changed=true title="some-agent-title"
test native_terminal::terminal::tests::native_terminal_ffi_probe_bel_reports_counter_observation ... ok
test native_terminal::terminal::tests::native_terminal_ffi_probe_osc_2_reports_title_change_and_value ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 282 filtered out; finished in 0.00s
```

DETAIL: The live FFI probe constructs `NativeTerminal`, feeds OSC 2 and BEL through `TerminalEngine::feed`, and observes the same drains used by the host (`src-tauri/src/native_terminal/terminal.rs:338-368`). Callback registration is in `src-tauri/src/native_terminal/lifecycle.rs:45-77`; the production feed-and-drain path is `src-tauri/src/native_terminal/surface_host.rs:189-214` and `src-tauri/src/native_terminal/surface_host.rs:403-417`. The explicit `native-terminal` feature was passed even though it is currently a default feature.
