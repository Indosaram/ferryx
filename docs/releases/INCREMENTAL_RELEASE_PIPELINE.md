# Ferryx Incremental Release Pipeline Guide

**Status**: Configured and Active across all three build machines (MacBook, omaki, maho-win).  
**Date**: 2026-09-08  

---

## 1. Overview & Root Cause Analysis

### Why past release builds took so long (~45–60 minutes total)
1. **Fresh clones & wiped directories**:
   - **omaki (Linux)** wiped `$REL` and ran `git clone --depth 1` from scratch on every release (`rel-0906.sh`), starting with an empty `target/` and no `node_modules`.
   - **MacBook (macOS)** created fresh `/tmp/ferryx-rel-XXXX` worktrees with an empty `src-tauri/target`. Building Universal binaries (`universal-apple-darwin`) compiled ~600 Rust crates and vendored Ghostty **twice** (once for `aarch64`, once for `x86_64`) from 0% cache.
   - **maho-win (Windows)** previously wiped `target/` due to historical C: drive space limits (~22GB).
2. **Missing compiler cache**:
   - Cargo's `[profile.release]` has `incremental = false` by default, and `sccache` was neither configured in `~/.cargo/config.toml` nor set in `RUSTC_WRAPPER`.
   - As a result, identical crates across releases (such as `tokio`, `tauri`, `axum`, `wgpu`, `portable-pty`) were recompiled every single time.

---

## 2. Two-Tier Incremental Architecture

### Tier 1: Compiler-Level Object Caching (`sccache`)
- All three machines now have `sccache` installed and configured as the global Cargo compiler wrapper via `~/.cargo/config.toml`:
  ```toml
  [build]
  rustc-wrapper = "<path/to/sccache>"
  ```
- **How it works**: When Cargo builds any crate, `sccache` intercepts the `rustc` call, hashes compiler flags, environment, and inputs, and instantly returns cached object code on cache hits.
- **Cross-worktree & Cross-directory**: Even if a build is triggered in a brand-new directory or temporary worktree, `sccache` serves all pre-compiled dependencies in milliseconds.

### Tier 2: Persistent Workspace & Target Directories
- Rather than destroying release checkouts, each remote builder now has a dedicated, persistent `ferryx-release` directory.
- `git fetch origin --tags && git checkout -f <tag>` updates the source code while preserving:
  - `src-tauri/target/` (Cargo fingerprints, `.rlib` dependencies, Zig Ghostty build artifacts)
  - `ui/node_modules/` (Bun skips reinstall if `bun.lock` is unchanged)
- When `target/` is preserved, Cargo skips checking/compiling unchanged crates in 0.01 seconds without even invoking rustc.

---

## 3. Builder Configuration Matrix

| Builder | OS / Arch | sccache Version | Cache Storage | Cargo Config Location | Persistent Release Path |
|---|---|---|---|---|---|
| **MacBook** | macOS arm64 | `0.16.0` | MinIO S3 (`100.126.171.58:9000/sccache`) via launchd daemon (`port 4226`) | `~/.cargo/config.toml` (`/opt/homebrew/bin/sccache`) | Persistent worktree or `CARGO_TARGET_DIR` |
| **omaki** | Arch Linux x86_64 | `0.16.0-3` | Local NVMe (`/home/indo/.cache/sccache`, 50 GiB max) | `/home/indo/.cargo/config.toml` (`/usr/bin/sccache`) | `/home/indo/ferryx-release` |
| **maho-win** | Windows 11 x86_64 | `0.17.0` | Local disk (`C:\Users\sook\.cache\maho\sccache`, 50 GiB max) | `C:\Users\sook\.cargo\config.toml` (`sccache.exe`) | `C:\Users\sook\ferryx-winbuild\ferryx-release` |

---

## 4. Release Execution Guide (Incremental)

### A. Linux (omaki)
Execute via SSH or terminal:
```bash
ssh omaki "/home/indo/build-release-incremental.sh <TAG>"
# Example:
# ssh omaki "/home/indo/build-release-incremental.sh v2026.09.08.1"
```
- **What it does**:
  1. Fetches the latest tag in `/home/indo/ferryx-release`.
  2. Ensures Ghostty submodule is pinned at `6a508fd5`.
  3. Stamps version via `sync-version.mjs`.
  4. Incrementally builds UI (`bun run --cwd ui build`).
  5. Incrementally builds DEB bundle (`bun tauri build --bundles deb`) with sccache and preserved target.
  6. Bundles AppImage via `linuxdeploy` and signs both DEB and AppImage.
  7. Stages artifacts into `/home/indo/ferryx-release-artifacts/`.

### B. Windows (maho-win)
Execute via SSH:
```powershell
ssh maho-win "powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\sook\ferryx-winbuild\build-release-incremental.ps1 -Tag <TAG>"
# Example:
# ssh maho-win "powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\sook\ferryx-winbuild\build-release-incremental.ps1 -Tag v2026.09.08.1"
```
- **What it does**:
  1. Fetches the latest tag in `C:\Users\sook\ferryx-winbuild\ferryx-release`.
  2. Stamps version via `sync-version.mjs`.
  3. Incrementally builds UI (`bun run --cwd ui build`).
  4. Incrementally builds NSIS setup (`cargo tauri build --bundles nsis`) with sccache.
  5. Builds MSIX package via `scripts\build-msix.ps1 -Version <version> -SkipSigning`.
  6. Stages artifacts into `C:\Users\sook\ferryx-winbuild\ferryx-release-artifacts\`.

### C. macOS (MacBook)
When building the macOS Universal release:
```bash
# Recommended: point to a persistent target directory so universal slices share cache
export CARGO_TARGET_DIR="$HOME/.cargo/targets/ferryx-release"
mkdir -p "$CARGO_TARGET_DIR"

# From your clean release worktree or directory:
bun scripts/sync-version.mjs --tag <TAG>
bun run --cwd ui build
bun tauri build --target universal-apple-darwin
```
- `sccache` will automatically serve compiled objects for both `aarch64` and `x86_64` targets.
- Subsequent runs will find existing compiled `.rlib` dependencies in `$CARGO_TARGET_DIR`.

---

## 5. Maintenance & Diagnostics

### Checking Cache Stats
To inspect cache hits and miss rates on each builder:
- **MacBook**: `sccache --show-stats`
- **omaki**: `ssh omaki "sccache --show-stats"`
- **maho-win**: `ssh maho-win "sccache --show-stats"`

### When to Clear Caches
If a major Rust toolchain upgrade occurs (e.g. rustc 1.92 -> 1.95) or C ABI incompatibilities arise:
- Clear sccache: `sccache --clear` (or delete the cache directory).
- Clean target: `cargo clean` inside the respective release directory.
