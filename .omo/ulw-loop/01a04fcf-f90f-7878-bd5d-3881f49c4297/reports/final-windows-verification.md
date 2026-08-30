# Ferryx Windows Remediation Final Verification

Final implementation commit: `a317255`

Base commit: `65e3a8f`

Native Windows verification checkout: `C:\Users\sook\ferryx-ulw-01a04fcf`

Protected user checkout: `C:\Users\sook\ferryx-winbuild\orca-lite` (not modified)

## Accepted Fixes

- Windows drive/UNC nested worktree ownership.
- Git CLI normalization for Win32 verbatim paths.
- Native-terminal feature-off compilation.
- Windows CLI launcher status without `HOME`.
- Win32 native terminal clipboard text/image/file classification.
- Native terminal search DTO/navigation and native child-surface yielding.
- Windows/Linux compositor contract expectations.
- Bundled remote SPA resource inclusion and runtime resolution.
- Exclusive Windows daemon locking with `LockFileEx` and RAII release.
- Windows daemon QA harness proving streamed PTY bytes and actual shell CWD.

## Correctly Rejected or Demoted Candidates

- Custom shell argument tokenization: the shipped setting is an executable path, so argv parsing would expand the product contract.
- WSL agent socket propagation: the Windows daemon has no agent-state socket listener; propagating the path would create a dead bridge.
- Cross-process Windows CWD introspection: a current-PID-only patch was rejected and reverted; undocumented PEB reading is racy, cross-bitness-sensitive, and not justified. Real shell CWD is instead proven from PTY output.
- Unix-gated daemon persistence suite: a coverage gap rather than a reproduced user-facing Windows defect. Live TCP/ConPTY behavior is covered by the native E2E probe.

## Windows Native Results

### Frozen Build and Focused Gates

- Native MSVC debug build: PASS on the byte-identical implementation tree represented by `9cb363e` (production code unchanged by later test-only commits).
- UI production build: PASS, 1,840 modules.
- `cargo check`: PASS.
- `cargo check --no-default-features`: PASS.
- Windows worktree Git path test: PASS.
- Windows CLI launcher tests: PASS after portable `PathBuf` comparison in `41a2461`.
- Windows native clipboard tests: PASS.
- Windows native terminal surface contract: PASS.

### Live Daemon and ConPTY

The final native probe ran against only `C:\Users\sook\ferryx-ulw-01a04fcf`.

Two consecutive E2E runs each returned:

```text
verdict=true
handshakeOk=true
errorProbeOk=true
spawnOk=true
writeOk=true
outputMarkerOk=true
cwdOk=true
endSequence=0 -> 9
requestedRepoRoot=C:\Users\sook\ferryx-ulw-01a04fcf
ptyExtractedCwd=C:\Users\sook\ferryx-ulw-01a04fcf
```

The harness attaches before writing, decodes actual streamed PTY bytes, ignores ANSI-colored echoed command text, and accepts only a standalone absolute Windows `FERRYX_CWD=` output line.

### Duplicate Daemon Edge

```text
DUPLICATE_EDGE_PASS exit=1 portStable=61446 daemon1Alive=True
EDGE_CLEANUP daemonProcesses=0 portExists=False
```

The second daemon emitted `Another daemon instance is already holding the lock.`, exited nonzero, did not overwrite `daemon.port`, and did not interrupt daemon 1.

### Windows Baseline Comparison

- Rust library suite before remediation: 280 passed / 28 failed / 1 ignored.
- Rust library suite after remediation: 357 passed / 19 failed / 1 ignored.
- Remaining Rust failures are known Windows-incompatible POSIX shell, font metric, and environment assumption tests; no new failure class appeared.
- UI suite before remediation: 484 passed / 137 failed / 62 errors.
- UI suite after remediation: 494 passed / 136 failed / 61 errors across 630 tests and 125 files.
- Remaining UI failures are the existing Windows Bun/jsdom/updater mock class; no new failure class appeared.

## macOS Adjacent Regression Results

- UI production build: PASS.
- Focused UI Vitest suite: 23 passed.
- Default Rust check: PASS.
- Feature-off Rust check: PASS.
- CLI launcher tests: PASS.
- Native clipboard tests: PASS.
- Worktree tests: PASS.
- Daemon server tests: PASS.
- Remote tests: PASS (37 passed).
- Native terminal surface contract: PASS (17 passed).
- Daemon persistence contract: PASS.
- Strict Miri clipboard safe-seam run: PASS.

Only pre-existing compiler warnings in notification and unrelated dead-code paths remain.

## Manual Physical Desktop Boundary

Headless SSH cannot prove displayed pixels, focus routing in another foreground application, Windows IME composition, mixed-DPI hit testing, or an unobscured WSLg window. Those checks are intentionally not marked PASS. The exact user-owned procedure is persisted at:

`reports/manual-windows-desktop-checklist.md`

It requires debug-only desktop launch using exactly `bun tauri dev`.

## Commits

- `9e50c8e` fix(windows): resolve nested worktree ownership paths
- `9266644` fix(windows): report CLI launcher as unsupported
- `b59a6b1` fix(windows): normalize Git worktree paths
- `7ffa7a4` test(native-terminal): cover Windows child compositor
- `7de71cc` fix(build): restore native-terminal feature-off compilation
- `6ecd743` fix(windows): read native terminal clipboard
- `cb2358d` fix(native-terminal): repair search on child surfaces
- `d70f434` fix(remote): resolve bundled SPA assets
- `74008c7` fix(windows): enforce exclusive daemon locking
- `9cb363e` test(qa): verify Windows PTY output and cwd
- `41a2461` test(windows): compare launcher paths portably
- `91eb17c` test(qa): parse Windows PTY cwd output
- `a317255` test(qa): honor Windows E2E repo root
