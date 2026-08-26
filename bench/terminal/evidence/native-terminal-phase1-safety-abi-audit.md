# Phase 1 Native Terminal Safety and ABI Audit

**Captured:** 2026-08-25  
**Scope:** macOS Phase 1 `libghostty-vt` Rust core spike  
**Verdict:** Phase 1 exit gate satisfied with platform-scoped evidence. This does not accept Phase 3 or authorize Phase 4.

## Required Gate Checklist

| Requirement | Evidence | Result |
| --- | --- | --- |
| Pinned Ghostty revision | `src-tauri/native_terminal/build_ghostty.rs` requires `6a508fd5e34c7e222c052a6d00bb3891ff3feace`; `ghostty_source_lock.json` is checked by `test_ghostty_source_lock_matches_build_contract` | Pass |
| Reproducible Zig/static link input | Build contract requires Zig `0.16.0`; `cargo test --manifest-path src-tauri/Cargo.toml --test ghostty_build_contract` passed 13/13, including Zig version, clean pinned submodule, static link, FFI call, and negative fixtures | Pass |
| Safe owned Rust boundary | `lifecycle.rs` returns non-null opaque Ghostty handles with an owned `TerminalContext`; `terminal.rs` tears callbacks down before freeing; `guards.rs` owns render state, iterators, cells, key, and mouse allocations with `Drop` | Pass by source audit and contracts |
| Create/feed/resize/snapshot/cursor/ANSI/Unicode | `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract` passed 22/22, covering grid snapshots, ANSI/SGR, cursor, alternate screen, scrollback, reflow, CJK, emoji, combining marks, title, bell, key, and mouse paths | Pass |
| Repeated create/destroy safety | The lifecycle contract creates, feeds, snapshots, resizes, and drops a terminal 50 times; it passed under normal execution, macOS GuardMalloc with `MallocScribble=1`, and direct macOS `leaks` execution | Pass, macOS-scoped |

## ABI Boundary Assessment

- FFI data structures use `#[repr(C)]`; the C-facing input structures carry their Rust size where the Ghostty ABI requires it.
- Opaque foreign allocations are represented as non-null pointers and released by their matching Ghostty free functions through typed guard destructors.
- C integer result values are decoded into typed `NativeTerminalError` results instead of being ignored.
- Browser key input is converted through a typed DTO before it reaches Ghostty; unsupported multi-character browser keys return a typed error.
- The build contract includes a linked FFI smoke call and invalid-key negative conversion coverage.

## Dynamic Lifetime Evidence

```text
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract
22 passed; 0 failed

leaks --atExit -- <direct native_terminal_engine_contract binary>
  --exact lifecycle_and_control::test_high_iteration_create_feed_snapshot_drop_lifecycle
1 passed; 0 failed
0 leaks for 0 total leaked bytes

DYLD_INSERT_LIBRARIES=/usr/lib/libgmalloc.dylib MallocScribble=1
  <direct native_terminal_engine_contract binary>
  --exact lifecycle_and_control::test_high_iteration_create_feed_snapshot_drop_lifecycle
1 passed; 0 failed
```

The first `leaks` attempt filtered out every test and is intentionally excluded from evidence. The accepted direct run executed the exact 50-iteration lifecycle test.

## Miri Limitation

`cargo +nightly miri test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract lifecycle_and_control::test_high_iteration_create_feed_snapshot_drop_lifecycle`
was attempted and did not reach the test: Tauri's startup constructor calls macOS `_NSGetExecutablePath`, which Miri does not support when isolation is enabled. This is recorded as a tooling limitation, not as a passing Miri result.

The macOS direct `leaks` report also notes the test process is restricted from full debugging, although it completed the selected test and reported zero leaked bytes. GuardMalloc therefore provides a second, independent macOS heap-misuse execution. Before a cross-platform release gate, run equivalent Windows/Linux lifetime instrumentation and a sanitizer-capable Ghostty/Zig build.

## Conclusion

The documented Phase 1 headless-core exit gate is satisfied: the pinned static Ghostty integration exposes the required terminal behavior and has repeated-lifecycle, leak, and guarded-allocator evidence on macOS. Phase 3 remains incomplete pending the explicit desktop manual scenarios in `native-surface-phase3-input-prototype.md`; Phase 4 must not begin yet.
