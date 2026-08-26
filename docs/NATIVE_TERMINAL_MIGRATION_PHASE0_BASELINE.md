# Native Terminal Migration — Phase 0 Baseline Report & Specifications

**Document Version**: 1.1.0
**Phase**: Phase 0 (Baseline / Freeze)
**Date**: 2026-08-24
**Target Migration**: xterm.js / WebView Terminal -> Native `libghostty-vt` + `wgpu` Engine

---

## 1. Overview & Objectives

As part of the native terminal migration plan (defined in `docs/NATIVE_TERMINAL_MIGRATION_IMPLEMENTATION_PLAN.md`), **Phase 0** establishes the immutable baseline performance profile, contract specifications, feature matrix, and repeatable benchmark harness for the current terminal pipeline before native integration begins.

This report captures:
1. **Measured baseline throughput & latency**: Canonical machine metrics are recorded in `bench/terminal/evidence/baseline-latest.json` and human-readable summary in `bench/terminal/evidence/baseline-summary.md`.
2. **Measurement Surface Separation**: Explicitly isolates in-memory JavaScript buffer transforms (`js-buffer-in-memory`) from actual terminal parser throughput (`xterm-headless-parser`) exercising `@xterm/xterm@6.0.0` through completion-aware write boundaries.
3. **Auditable current xterm feature & settings baseline**: Captures source-audited feature contracts, configuration schemas, and explicit unmeasured desktop real-surface scenarios sourced directly from present UI implementations.
4. **Machine-consumed schema & output contracts**: Defines strict schemas in `bench/terminal/evidence/` for regression tracking and automated native parity verification.
5. **Grounded Comparison Gates**: The runner derives comparison values directly from the current measured artifact rather than hard-coding values in this stable guide.

---

## 2. Benchmark Architecture & Execution Harness

The Phase 0 benchmark runner is implemented in `bench/terminal/runner.ts` and supported by modular workload generators in `bench/terminal/workloads.ts`.

### Runner Commands
- **Execute Full Suite & Record Evidence**:
  ```bash
  bun run bench/terminal/runner.ts
  ```
- **Execute Benchmark Seam Unit Tests**:
  ```bash
  bun test bench/terminal/workloads.test.ts
  ```

### Failure & Isolation Guarantees
- The runner executes wrapped microbenchmarks (`ui/src/lib/terminalThroughput.bench.ts`) via sub-process isolation and terminates immediately with exit code `1` on failure.
- Every reported metric explicitly declares its `measurementSurface`. Checksum loops or memory transforms are strictly forbidden from claiming VT parser or renderer performance.
- Completion-aware write boundaries (`term.write(chunk, resolve)`) are used for all parser workloads to measure full VT state commitment.
- All generated evidence artifacts are strictly sandboxed inside `bench/terminal/evidence/`.

---

## 3. Measured Phase 0 Baseline Evidence Profile

> **Canonical Artifact Notice**: Benchmark numbers vary by runtime invocation. `bench/terminal/evidence/baseline-latest.json` and its generated `baseline-summary.md` are the only authoritative run-specific evidence. This stable guide deliberately contains no benchmark values or ranges; rerun the runner to refresh measurements.

### Hardware & Environment Profile
- **Host Platform**: macOS Darwin 25.6.0 (Apple M4 Max, 16 cores, arm64)
- **Runtime**: Bun 1.4.0 (JavaScriptCore engine)
- **Terminal Engine Seam**: `@xterm/xterm@6.0.0` (headless completion-aware write boundary)
- **PTY Chunk Size**: Standard 32 KiB binary stream chunking

### 3.1 Required Workloads and Measurement Surfaces

| Workload IDs | Measurement Surface | Contract |
| :--- | :--- | :--- |
| `wrapped-direct-10mb`, `wrapped-legacy-base64-10mb` | `js-buffer-in-memory` | Tracks JavaScript buffer transform cost only; it is not a terminal parser or renderer metric. |
| `ascii-10mb`, `ascii-50mb`, `ascii-100mb`, `ansi-sgr-10mb`, `unicode-cjk-10mb` | `xterm-headless-parser` | Feeds the installed `@xterm/xterm` VT parser through completion-aware `Terminal.write` callbacks. |
| `multipane-4x10mb` | `xterm-headless-parser` | Interleaves four parser instances; it is a parser completion benchmark, not a real compositor frame metric. |
| `interactivity-event-dispatch` | `xterm-headless-parser` | Measures completion-aware write-to-parser latency; it is not desktop input-to-PTY latency. |

---

## 4. Auditable xterm Feature & Settings Baseline

Sourced directly from current UI production codebase. Until a feature-specific command is run and retained in its generated artifact, each row is `source_audited`, not command-verified. Machine-consumable contract: `bench/terminal/evidence/xterm-feature-baseline.json`.

### 4.1 Installed Packages & Addons
- **Core Engine**: `@xterm/xterm@6.0.0`
- **Installed Addons**:
  - `@xterm/addon-fit@0.11.0`: Dynamic container size fitting.
  - `@xterm/addon-search@0.16.0`: In-buffer full text search.
  - `@xterm/addon-unicode11@0.9.0`: Extended Unicode 11 character width.
  - `@xterm/addon-webgl@0.19.0`: Hardware-accelerated GPU glyph rendering.

### 4.2 Verified Feature Contracts

| Feature ID | Feature Name | Source Implementation | Verification Status | Command Evidence | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `binary-scheduled-output-replay` | Binary Scheduled Output & Replay | `ui/src/lib/terminalOutputScheduler.ts`<br>`ui/src/lib/terminalOutput.ts` | `source_audited` | None | RAF + 128 KiB threshold flush, sequence/epoch gap handling |
| `resize-handling` | Resize Observer & Viewport Anchor | `ui/src/lib/terminalInstanceFactory.ts`<br>`ui/src/lib/terminalFit.ts` | `source_audited` | None | Debounced RAF refit, preserves scroll bottom, IPC cols/rows sync |
| `input-forwarding` | Keystroke & Input Forwarding | `ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | None | `terminal.onData` forwards to daemon PTY via `writeTerminal` |
| `title-reporting` | Tab & Session Title Updates | `ui/src/lib/terminalInstanceFactory.ts`<br>`ui/src/lib/agentTitle.ts` | `source_audited` | None | OSC 0 and OSC 2 escape sequence handling updates tab title |
| `bell-notification` | Terminal Bell Notification | `ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | None | BEL (`\x07`) triggers `onBell` callback for tab badge alerts |
| `search-addon` | Search Overlay & Addon | `ui/src/components/TerminalSearchOverlay.tsx`<br>`ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | None | `findNext`/`findPrevious`, regex, case sensitivity over buffer |
| `unicode11-addon` | Unicode 11 Extended VTE | `ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | None (Source Audited) | Dynamic addon loading seam verified in factory; requires real-surface manual QA |
| `webgl-renderer` | WebGL GPU Acceleration | `ui/src/lib/terminalRenderer.ts`<br>`ui/src/lib/terminalInstanceFactory.ts` | `source_audited` | None | GPU acceleration with clean fallback to canvas/DOM |
| `terminal-settings-contracts` | Theme, Font & Scrollback | `ui/src/lib/terminalSettings.ts` | `source_audited` | None | Font stack, size (10-36), scrollback (1k-100k), 24-bit theme |
| `desktop-vs-browser-boundary` | Tauri vs Browser Boundary | `ui/src/lib/terminalInstanceFactory.ts`<br>`ui/src/lib/tauri.ts` | `source_audited` | None | `isTauriRuntime()` guards PTY IPC vs browser UI preview |

### 4.3 Existing Settings Contracts
- **Font Family**: `Berkeley Mono, JetBrains Mono, Fira Code, Menlo, Monaco, Consolas, monospace` (configurable via localStorage & native ghostty sync).
- **Font Size**: Default `13`, Range `10` to `36`.
- **Scrollback**: Default `10,000` lines, Range `1,000` to `100,000` lines.
- **Cursor Style**: `"block"` (default), `"bar"`, `"underline"`.
- **macOS Option as Alt**: Default `false` (`macOptionIsMeta`).
- **Theme Palette**: 21 color keys (`background`, `foreground`, `cursor`, `cursorAccent`, `selectionBackground`, 8 ANSI standard colors, 8 ANSI bright colors).

### 4.4 Known Unmeasured Desktop Real-Surface Scenarios (Manual QA Required)

The following desktop real-surface scenarios cannot be validated solely by headless unit benchmarks and are explicitly marked as **unmeasured / manual QA required** without false pass claims:

| Scenario ID | Scenario Name | Source File | Status | Command Evidence | Manual QA Status |
| :--- | :--- | :--- | :--- | :---: | :--- |
| `idle-cpu-multi-session` | Multi-Session Idle CPU | `ui/src/lib/terminalHostManager.ts` | `unmeasured_real_surface` | None | **Manual QA Required**: macOS Instruments / Activity Monitor profiling under live 10-pane session |
| `tab-switch-real-latency` | Tab Switch Frame Latency | `ui/src/lib/terminalHostManager.ts` | `unmeasured_real_surface` | None | **Manual QA Required**: 60/120 FPS display frame budget recording during high output |
| `split-resize-continuous-fps` | Split Resize Drag FPS | `ui/src/components/TerminalSplitView.tsx` | `unmeasured_real_surface` | None | **Manual QA Required**: Chrome DevTools Performance recording during live split drag |
| `hmr-restore-process-recovery` | Process Relaunch State Recovery | `ui/src/state/workspaceRestore.test.tsx` | `unmeasured_real_surface` | None | **Manual QA Required**: Tauri process restart / crash recovery with active PTY buffer |

---

## 5. Machine-Consumed Artifact Contract

All benchmark runs produce two immutable JSON artifacts under `bench/terminal/evidence/`:
1. `bench/terminal/evidence/baseline-latest.json` (Combined performance & feature baseline report)
2. `bench/terminal/evidence/xterm-feature-baseline.json` (Auditable feature & settings contract artifact)

```typescript
interface BaselineReport {
  timestamp: string; // ISO-8601 UTC timestamp
  environment: {
    os: string;
    arch: string;
    runtime: string;
    cpuCores: number;
    xtermVersion?: string;
    xtermPackage?: string;
  };
  wrappedMicrobench: {
    directMibPerSec: number;
    legacyMibPerSec: number;
    directMedianMs: number;
    legacyMedianMs: number;
    speedupFactor: number;
    directRunsMs: number[];
    legacyRunsMs: number[];
    measurementSurface: string;
  };
  workloads: Array<{
    id: string;
    name: string;
    category: "throughput" | "parser" | "multipane" | "interactivity";
    measurementSurface: string;
    totalBytes: number;
    chunkBytes: number;
    chunkCount: number;
    runs: number;
    medianMs: number;
    minMs: number;
    maxMs: number;
    mibPerSec: number;
    latency?: {
      p50Ms: number;
      p95Ms: number;
      p99Ms: number;
    };
    checksum: number;
    notes?: string;
  }>;
  featureBaseline?: {
    xtermPackage: string;
    xtermVersion: string;
    addons: Array<{ name: string; version: string; purpose: string }>;
    features: Array<{
      id: string;
      name: string;
      sourceFiles: string[];
      contractDescription: string;
      verificationStatus: "command_verified" | "source_audited" | "unmeasured_real_surface" | "manual_qa_required";
      hasCommandEvidence: boolean;
      commandEvidence?: string;
      manualQaRequired: boolean;
      notes?: string;
    }>;
    settingsContracts: Record<string, unknown>;
    unmeasuredRealSurfaceScenarios: Array<{
      id: string;
      name: string;
      sourceFiles: string[];
      contractDescription: string;
      verificationStatus: "unmeasured_real_surface";
      hasCommandEvidence: boolean;
      manualQaRequired: boolean;
      notes?: string;
    }>;
  };
}
```

---

## 6. Exit Criteria & Native Comparison Contracts

1. **Throughput Parity**: Native throughput on sustained ASCII, ANSI, and Unicode streams must match or exceed the current xterm parser values in `bench/terminal/evidence/baseline-latest.json`.
2. **Multi-Pane Fairness**: Multi-pane parser completion must remain deterministic. Real compositor frame drops are a Phase 4+ real-surface gate, not a headless parser claim.
3. **Interactive Dispatch (Provisional)**: The generated `baseline-summary.md` computes the allowable native parser p99 from that exact run's xterm p99 plus 15%. Desktop input-to-PTY latency is unmeasured and remains a Phase 4+ real-surface QA gate.
4. **Feature & Settings Contract Parity**: All 10 audited feature contracts and configuration schemas must reach full functional parity prior to WebView terminal retirement.
5. **Real-Surface Desktop QA**: All unmeasured real-surface scenarios (multi-session idle CPU, live tab switch frame budget, continuous split drag FPS, crash recovery) require real-surface manual QA verification before Phase 4 sign-off.
