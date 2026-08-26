# Terminal Migration Baseline Evidence (Phase 0)

- **Date**: 2026-08-24T10:31:01.344Z
- **Environment**: Darwin 25.6.0 (darwin) (arm64)
- **Runtime**: bun 1.4.0
- **CPU Cores**: 16
- **xterm Engine**: @xterm/xterm@6.0.0

## Microbenchmark Results (Wrapped `terminalThroughput.bench.ts`)

| Path | Median Time (ms) | Throughput (MiB/s) | Surface | Notes |
| --- | ---: | ---: | --- | --- |
| **Direct Pass-Through** | 0.024 | **408847.5** | `js-buffer-in-memory` | Binary Uint8Array chunk streaming |
| **Legacy Base64** | 87.781 | **113.9** | `js-buffer-in-memory` | Base64 decode overhead |
| *Relative Speedup* | - | **3657.5x** | - | Uint8Array vs Base64 |

## Workload Benchmark Suite

| Workload | Size | Median Time (ms) | Throughput (MiB/s) | Latency p95 (ms) | Surface | Category |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| `wrapped-direct-10mb` (Uint8Array Direct Pass-Through (10 MiB)) | 10.0 MiB | 0.024 | 408847.5 | - | `js-buffer-in-memory` | throughput |
| `wrapped-legacy-base64-10mb` (Base64 Decoded Legacy Path (10 MiB)) | 10.0 MiB | 87.781 | 113.9 | - | `js-buffer-in-memory` | throughput |
| `ascii-10mb` (ASCII Stream (10 MiB)) | 10.0 MiB | 486.936 | 20.5 | - | `xterm-headless-parser` | throughput |
| `ascii-50mb` (ASCII Stream (50 MiB)) | 50.0 MiB | 2424.343 | 20.6 | - | `xterm-headless-parser` | throughput |
| `ascii-100mb` (ASCII Stream (100 MiB)) | 100.0 MiB | 4848.202 | 20.6 | - | `xterm-headless-parser` | throughput |
| `ansi-sgr-10mb` (ANSI SGR / 24-bit Escape Stream (10 MiB)) | 10.0 MiB | 570.594 | 17.5 | - | `xterm-headless-parser` | parser |
| `unicode-cjk-10mb` (Unicode CJK / Emoji / Wide-Char Stream (10 MiB)) | 10.0 MiB | 566.642 | 17.6 | - | `xterm-headless-parser` | parser |
| `multipane-4x10mb` (Multi-Pane Concurrent (4 panes x 10 MiB)) | 40.0 MiB | 821.445 | 48.7 | 2.5946 | `xterm-headless-parser` | multipane |
| `interactivity-event-dispatch` (Interactive Keystroke / Event Dispatch (1,000 cycles)) | 0.0 MiB | 1258.504 | 0.0 | 1.2743 | `xterm-headless-parser` | interactivity |

## Auditable xterm Feature & Settings Baseline

- **xterm Core**: `@xterm/xterm@6.0.0`
- **Installed Addons**: `@xterm/addon-fit@0.11.0`, `@xterm/addon-search@0.16.0`, `@xterm/addon-unicode11@0.9.0`, `@xterm/addon-webgl@0.19.0`

### Verified Feature Contracts

| Feature ID | Name | Source Files | Verification Status | Command Evidence |
| --- | --- | --- | --- | --- |
| `binary-scheduled-output-replay` | Binary Scheduled Output & Daemon Replay | `ui/src/lib/terminalOutputScheduler.ts`<br>`ui/src/lib/terminalOutput.ts` | `source_audited` | Source-audited (Manual QA) |
| `resize-handling` | Resize Observer & Viewport Preservation | `ui/src/lib/terminalInstanceFactory.ts`<br>`ui/src/lib/terminalFit.ts` | `source_audited` | Source-audited (Manual QA) |
| `input-forwarding` | Keystroke & Pointer Input Forwarding | `ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | Source-audited (Manual QA) |
| `title-reporting` | Window & Tab Title Reporting | `ui/src/lib/terminalInstanceFactory.ts`<br>`ui/src/lib/agentTitle.ts` | `source_audited` | Source-audited (Manual QA) |
| `bell-notification` | Terminal Bell (BEL) Notification | `ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | Source-audited (Manual QA) |
| `search-addon` | Search Overlay & Addon Navigation | `ui/src/components/TerminalSearchOverlay.tsx`<br>`ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | Source-audited (Manual QA) |
| `unicode11-addon` | Unicode 11 Extended VTE Addon | `ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | Source-audited (Manual QA) |
| `webgl-renderer` | WebGL Hardware Accelerated Rendering | `ui/src/lib/terminalRenderer.ts`<br>`ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | Source-audited (Manual QA) |
| `terminal-settings-contracts` | Theme, Font, and Scrollback Configuration | `ui/src/lib/terminalSettings.ts` | `source_audited` | Source-audited (Manual QA) |
| `desktop-vs-browser-boundary` | Desktop Tauri vs Browser Runtime Boundary | `ui/src/lib/terminalInstanceFactory.ts`<br>`ui/src/lib/tauri.ts` | `source_audited` | Source-audited (Manual QA) |

### Unmeasured Real-Surface Desktop Scenarios

| Scenario ID | Name | Source Files | Status | Command Evidence | Manual QA Required |
| --- | --- | --- | --- | --- | --- |
| `idle-cpu-multi-session` | Idle CPU Under Multi-Session Load | `ui/src/lib/terminalHostManager.ts` | `unmeasured_real_surface` | None | **Required** |
| `tab-switch-real-latency` | Real Surface Tab Switch Frame Latency | `ui/src/lib/terminalHostManager.ts` | `unmeasured_real_surface` | None | **Required** |
| `split-resize-continuous-fps` | Interactive Split Pane Continuous Drag FPS | `ui/src/components/TerminalSplitView.tsx` | `unmeasured_real_surface` | None | **Required** |
| `hmr-restore-process-recovery` | Process Crash / Reload Terminal State Recovery | `ui/src/state/workspaceRestore.test.tsx` | `unmeasured_real_surface` | None | **Required** |

## Exit Criteria & Native Comparison Contracts

1. **Throughput Parity**: Native throughput on sustained ASCII, ANSI, and Unicode streams must match or exceed the current xterm parser figures in this report.
2. **Multi-Pane Fairness**: Multi-pane concurrent output must preserve deterministic interleaving; visual frame-drop assessment remains a real-surface Phase 4+ gate.
3. **Interactive Dispatch (Provisional)**: Native headless parser p99 must not exceed this run's xterm write-boundary p99 (1.2895 ms) by more than 15% (1.4830 ms). End-to-end desktop input-to-PTY latency remains a Phase 4+ real-surface QA gate.
4. **Feature & Settings Contract Parity**: All 10 audited feature contracts and configuration schemas must reach full functional parity prior to WebView terminal retirement.
5. **Real-Surface Desktop QA**: All unmeasured real-surface scenarios (multi-session idle CPU, live tab switch frame budget, continuous split drag FPS, crash recovery) require real-surface manual QA verification before Phase 4 sign-off.
