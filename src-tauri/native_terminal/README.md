# Native Terminal Build Integration (Phase 1)

This directory defines the Phase 1 build contract and verification harness for embedding Ghostty's virtual terminal engine (`libghostty-vt`) into the Orca/Ferryx desktop runtime.

## Source & Toolchain Contract

- **Vendored Submodule**: `src-tauri/vendor/ghostty`
- **Pinned Commit SHA**: `6a508fd5e34c7e222c052a6d00bb3891ff3feace`
- **Required Zig Compiler**: `0.16.0` (`zig version`)
- **Built Static Artifact**: `libghostty-vt.a` (emitted into Cargo `$OUT_DIR/ghostty_vt/lib`)
- **Include Directory**: `$OUT_DIR/ghostty_vt/include` (and vendored headers at `vendor/ghostty/include`)

## Build Guarantees & Architecture

1. **Structured, Fail-Fast Verification**:
   - `build.rs` delegates compilation to `build_ghostty::build_ghostty_vt()`.
   - All helper paths strictly propagate structured `Result<GhosttyBuildConfig, String>` with zero production `unwrap()` or `expect()`.
   - `verify_zig` validates that `zig` is present in `PATH` (or specified via the `ZIG` environment variable) and that its version starts with `0.16.0`.
   - `verify_submodule_sha` validates that the vendored submodule exists at `src-tauri/vendor/ghostty`, contains `build.zig`, and resolves to commit `6a508fd5e34c7e222c052a6d00bb3891ff3feace`.
   - If missing, mismatching, or uninitialized, actionable remediation commands are printed and the build fails immediately.

2. **No Silent Upstream Downloads**:
   - The build script strictly avoids performing git pulls, network fetches, or downloading moving upstream revisions during compilation.

3. **Narrowly Scoped Compilation & Hermetic Caching**:
   - Only `libghostty-vt` is built (macOS app, XCFramework, documentation, executables, and tests are disabled with `-Demit-*` flags).
   - Compilation artifacts and Zig caches (`--cache-dir`, `--global-cache-dir`) are isolated strictly inside Cargo's `$OUT_DIR`, retaining no untracked cache files in the workspace.

4. **FFI Safety & ABI Matching**:
   - `GhosttyResult`, `GhosttyOptimizeMode`, and `GhosttyBuildInfo` use `#[repr(i32)]` to align with C `enum : int` / Zig `enum(c_int)` ABI.
   - `GhosttyString` matches `{ ptr: *const u8, len: usize }` layout.
   - Unsafe operations are restricted strictly to FFI call sites with detailed `// SAFETY:` proofs documenting pointer validity, alignments, non-null guarantees, borrowed lifetimes, and absence of unwinding across the ABI boundary.

5. **Miri Boundary Limitations**:
   - Miri interprets Rust MIR in a virtual machine and cannot execute compiled foreign machine code inside external static libraries (`libghostty-vt.a`).
   - Contract verification is therefore achieved through deterministic host integration tests (`cargo test --test ghostty_build_contract`).
