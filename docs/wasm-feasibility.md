# WASM feasibility (2026-10-19)

Tested on Linux x86_64, Rust 1.99, Zig 0.16.0, Node 22.

| Piece | wasm32 result |
|---|---|
| crates/fxsh | builds (unknown-unknown, wasip1); 14 codec tests pass under WASI |
| crates/session-core | builds; 26 tests pass under WASI (proptest needs `default-features=false, features=["std"]`, because its `fork` feature does not compile on wasm) |
| libghostty-vt (ferryx-vt backend) | `zig build -Dtarget=wasm32-freestanding` works, 771 KB, no imports. Smoke test (scripts/wasm/vt-smoke.mjs) feeds VT bytes and reads cols/rows/cursor correctly |
| crates/ferryx-vt (Rust adapter) | not built for wasm: build_ghostty.rs has no wasm target mapping |
| portable-pty | does not compile (serial2/sys) |
| tokio net/process (ferryx-host, src-tauri) | mio rejects wasm; src-tauri lib fails |
| src-tauri (daemon, remote server, ssh, Tauri) | native only |

Run tests: `CARGO_TARGET_WASM32_WASIP1_RUNNER="node scripts/wasm/wasirun.mjs" cargo test --target wasm32-wasip1 --release` inside a crate.

Verdict: the protocol, session state machines and terminal emulator can run in the browser. PTYs, local sockets, SSH and the daemon cannot, so a browser build needs a native host reached over WebSocket.
