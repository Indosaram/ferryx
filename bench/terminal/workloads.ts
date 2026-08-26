export type VerificationStatus =
  | "source_audited"
  | "command_verified"
  | "unmeasured_real_surface"
  | "manual_qa_required";

export interface FeatureContractEntry {
  id: string;
  name: string;
  sourceFiles: string[];
  contractDescription: string;
  verificationStatus: VerificationStatus;
  hasCommandEvidence: boolean;
  commandEvidence?: string;
  manualQaRequired: boolean;
  notes?: string;
}

export interface XtermFeatureBaseline {
  xtermPackage: string;
  xtermVersion: string;
  addons: Array<{
    name: string;
    version: string;
    purpose: string;
  }>;
  features: FeatureContractEntry[];
  settingsContracts: {
    fontFamily: { default: string; configurable: boolean; source: string };
    fontSize: { default: number; min: number; max: number; configurable: boolean };
    scrollback: { default: number; min: number; max: number; configurable: boolean };
    cursorStyle: { options: string[]; default: string };
    macosOptionAsAlt: { default: boolean; configurable: boolean };
    themeColors: { paletteKeys: string[]; defaultBackground: string; defaultForeground: string };
  };
  unmeasuredRealSurfaceScenarios: FeatureContractEntry[];
}

export function assertHonestFeatureBaseline(baseline: XtermFeatureBaseline): void {
  for (const feature of baseline.features) {
    if (feature.verificationStatus === "command_verified") {
      if (!feature.hasCommandEvidence || !feature.commandEvidence || feature.commandEvidence.trim() === "") {
        throw new Error(
          `Dishonest feature contract: '${feature.id}' claims 'command_verified' without runnable command evidence`,
        );
      }
    } else if (feature.verificationStatus === "source_audited" || feature.verificationStatus === "unmeasured_real_surface") {
      if (feature.hasCommandEvidence) {
        throw new Error(
          `Dishonest feature contract: '${feature.id}' is '${feature.verificationStatus}' but claims command evidence`,
        );
      }
      if (!feature.manualQaRequired) {
        throw new Error(
          `Feature '${feature.id}' is '${feature.verificationStatus}' but does not require manual QA`,
        );
      }
    }
  }

  for (const scenario of baseline.unmeasuredRealSurfaceScenarios) {
    if (scenario.verificationStatus !== "unmeasured_real_surface" && scenario.verificationStatus !== "manual_qa_required") {
      throw new Error(
        `Unmeasured scenario '${scenario.id}' must have status 'unmeasured_real_surface' or 'manual_qa_required'`,
      );
    }
    if (scenario.hasCommandEvidence) {
      throw new Error(
        `Unmeasured scenario '${scenario.id}' cannot falsely claim command evidence`,
      );
    }
    if (!scenario.manualQaRequired) {
      throw new Error(
        `Unmeasured scenario '${scenario.id}' must require manual QA`,
      );
    }
  }
}

export function getXtermFeatureBaseline(): XtermFeatureBaseline {
  return {
    xtermPackage: "@xterm/xterm",
    xtermVersion: "6.0.0",
    addons: [
      { name: "@xterm/addon-fit", version: "0.11.0", purpose: "Auto-sizing terminal to container dimensions" },
      { name: "@xterm/addon-search", version: "0.16.0", purpose: "In-buffer text search with match navigation" },
      { name: "@xterm/addon-unicode11", version: "0.9.0", purpose: "Unicode 11 VTE width and emoji support" },
      { name: "@xterm/addon-webgl", version: "0.19.0", purpose: "Hardware-accelerated glyph rendering pipeline" },
    ],
    features: [
      {
        id: "binary-scheduled-output-replay",
        name: "Binary Scheduled Output & Daemon Replay",
        sourceFiles: ["ui/src/lib/terminalOutputScheduler.ts", "ui/src/lib/terminalOutput.ts"],
        contractDescription: "Coalesced Uint8Array output stream via RAF and 128 KiB threshold flush; daemon sequence/epoch tracking with gap handling.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
        notes: "Source-audited; dedicated command evidence remains required before native parity sign-off.",
      },
      {
        id: "resize-handling",
        name: "Resize Observer & Viewport Preservation",
        sourceFiles: ["ui/src/lib/terminalInstanceFactory.ts", "ui/src/lib/terminalFit.ts"],
        contractDescription: "ResizeObserver on terminal container element, debounced RAF refit, preserves scroll-to-bottom anchor, syncs cols/rows over IPC.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
      {
        id: "input-forwarding",
        name: "Keystroke & Pointer Input Forwarding",
        sourceFiles: ["ui/src/lib/terminalInstanceFactory.ts"],
        contractDescription: "terminal.onData dispatches keystrokes and escape sequences directly to daemon PTY via writeTerminal IPC; pointerdown focuses terminal.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
      {
        id: "title-reporting",
        name: "Window & Tab Title Reporting",
        sourceFiles: ["ui/src/lib/terminalInstanceFactory.ts", "ui/src/lib/agentTitle.ts"],
        contractDescription: "OSC 0 and OSC 2 sequence handling via terminal.onTitleChange updates tab and workspace session title.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
      {
        id: "bell-notification",
        name: "Terminal Bell (BEL) Notification",
        sourceFiles: ["ui/src/lib/terminalInstanceFactory.ts"],
        contractDescription: "BEL (0x07) trigger dispatches terminal.onBell callback for tab badge and audio/visual alerts.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
      {
        id: "search-addon",
        name: "Search Overlay & Addon Navigation",
        sourceFiles: ["ui/src/components/TerminalSearchOverlay.tsx", "ui/src/lib/terminalInstanceFactory.ts"],
        contractDescription: "@xterm/addon-search provides findNext / findPrevious, regex, case sensitivity, and whole word searching over active buffer.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
      {
        id: "unicode11-addon",
        name: "Unicode 11 Extended VTE Addon",
        sourceFiles: ["ui/src/lib/terminalInstanceFactory.ts"],
        contractDescription: "Loads @xterm/addon-unicode11 dynamically in factory and sets activeVersion='11' for wcwidth on emoji and CJK wide glyphs.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
        notes: "Source-audited in ui/src/lib/terminalInstanceFactory.ts; dynamic loading seam present in UI but lacks dedicated vitest UI test file; manual QA required.",
      },
      {
        id: "webgl-renderer",
        name: "WebGL Hardware Accelerated Rendering",
        sourceFiles: ["ui/src/lib/terminalRenderer.ts", "ui/src/lib/terminalInstanceFactory.ts"],
        contractDescription: "Asynchronously loads @xterm/addon-webgl for GPU acceleration; falls back cleanly to canvas/DOM rendering if context fails.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
      {
        id: "terminal-settings-contracts",
        name: "Theme, Font, and Scrollback Configuration",
        sourceFiles: ["ui/src/lib/terminalSettings.ts"],
        contractDescription: "Saves and resolves font stack, font size (10-36), scrollback (1k-100k), cursorStyle, macosOptionAsAlt, and 24-bit theme palette.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
      {
        id: "desktop-vs-browser-boundary",
        name: "Desktop Tauri vs Browser Runtime Boundary",
        sourceFiles: ["ui/src/lib/terminalInstanceFactory.ts", "ui/src/lib/tauri.ts"],
        contractDescription: "isTauriRuntime() guards IPC PTY session creation; browser preview environment renders simulated preview prompt banner.",
        verificationStatus: "source_audited",
        hasCommandEvidence: false,
        manualQaRequired: true,
      },
    ],
    settingsContracts: {
      fontFamily: {
        default: "Berkeley Mono, JetBrains Mono, Fira Code, Menlo, Monaco, Consolas, monospace",
        configurable: true,
        source: "localStorage + native ghostty config sync",
      },
      fontSize: { default: 13, min: 10, max: 36, configurable: true },
      scrollback: { default: 10000, min: 1000, max: 100000, configurable: true },
      cursorStyle: { options: ["block", "bar", "underline"], default: "block" },
      macosOptionAsAlt: { default: false, configurable: true },
      themeColors: {
        paletteKeys: [
          "background", "foreground", "cursor", "cursorAccent", "selectionBackground",
          "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
          "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue",
          "brightMagenta", "brightCyan", "brightWhite",
        ],
        defaultBackground: "#282c34",
        defaultForeground: "#ffffff",
      },
    },
    unmeasuredRealSurfaceScenarios: [
      {
        id: "idle-cpu-multi-session",
        name: "Idle CPU Under Multi-Session Load",
        sourceFiles: ["ui/src/lib/terminalHostManager.ts"],
        contractDescription: "Zero-activity background PTY sessions must exhibit <= 0.1% CPU wake-up frequency on macOS Activity Monitor.",
        verificationStatus: "unmeasured_real_surface",
        hasCommandEvidence: false,
        manualQaRequired: true,
        notes: "Unmeasured real-surface scenario: requires macOS Instruments / Activity Monitor profiling under live 10-pane daemon session.",
      },
      {
        id: "tab-switch-real-latency",
        name: "Real Surface Tab Switch Frame Latency",
        sourceFiles: ["ui/src/lib/terminalHostManager.ts"],
        contractDescription: "Switching between active output tabs must render the new terminal buffer in < 16.6ms (1 frame) without layout reflow glitch.",
        verificationStatus: "unmeasured_real_surface",
        hasCommandEvidence: false,
        manualQaRequired: true,
        notes: "Unmeasured real-surface scenario: host manager unit mocks exist, but real display frame budget requires manual QA / profiler.",
      },
      {
        id: "split-resize-continuous-fps",
        name: "Interactive Split Pane Continuous Drag FPS",
        sourceFiles: ["ui/src/components/TerminalSplitView.tsx"],
        contractDescription: "Continuous mouse dragging on pane divider under active terminal throughput sustains >= 60 FPS without DOM stall.",
        verificationStatus: "unmeasured_real_surface",
        hasCommandEvidence: false,
        manualQaRequired: true,
        notes: "Unmeasured real-surface scenario: requires Chrome DevTools Performance recording during live split drag.",
      },
      {
        id: "hmr-restore-process-recovery",
        name: "Process Crash / Reload Terminal State Recovery",
        sourceFiles: ["ui/src/state/workspaceRestore.test.tsx"],
        contractDescription: "Full desktop application reload restores active terminal sessions, scroll position, and PTY connection without data loss.",
        verificationStatus: "unmeasured_real_surface",
        hasCommandEvidence: false,
        manualQaRequired: true,
        notes: "Unmeasured real-surface scenario: store logic is test-verified, but Tauri process relaunch requires manual QA validation.",
      },
    ],
  };
}

export interface WorkloadResult {
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
}

export function assertHonestMeasurementSurface(
  metric: WorkloadResult,
  actualEngine: "xterm-parser" | "checksum" | "js-buffer-in-memory" | "fixture-input-deterministic",
): void {
  const surface = (metric.measurementSurface || "").toLowerCase();
  if (!surface) {
    throw new Error(`Metric '${metric.id}' is missing required measurementSurface`);
  }
  if (actualEngine === "checksum" || actualEngine === "js-buffer-in-memory" || actualEngine === "fixture-input-deterministic") {
    if (surface.includes("xterm") || surface.includes("parser")) {
      throw new Error(`Dishonest measurement surface: '${metric.measurementSurface}' claimed for ${actualEngine} work in workload '${metric.id}'`);
    }
  } else if (actualEngine === "xterm-parser") {
    if (!surface.includes("xterm")) {
      throw new Error(`Metric '${metric.id}' ran actual xterm parser but reported unexpected surface '${metric.measurementSurface}'`);
    }
  }
}

export interface BaselineReport {
  timestamp: string;
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
  workloads: WorkloadResult[];
  featureBaseline?: XtermFeatureBaseline;
}

export function median(values: number[]): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

export function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(Math.floor((p / 100) * sorted.length), sorted.length - 1);
  return sorted[index] ?? 0;
}

export function calculateMibPerSec(totalBytes: number, ms: number): number {
  if (ms <= 0) return 0;
  return (totalBytes / (1024 * 1024)) / (ms / 1000);
}

export function computeChecksum(chunk: Uint8Array, seed = 0): number {
  let checksum = seed;
  checksum = (checksum + chunk.byteLength + (chunk[0] ?? 0) + (chunk[chunk.byteLength - 1] ?? 0)) >>> 0;
  return checksum;
}

export function generateAsciiChunks(totalBytes: number, chunkBytes: number): Uint8Array[] {
  const chunks: Uint8Array[] = [];
  let remaining = totalBytes;
  let state = 0x9e3779b9 >>> 0;

  while (remaining > 0) {
    const size = Math.min(chunkBytes, remaining);
    const chunk = new Uint8Array(size);
    for (let i = 0; i < size; i++) {
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      state >>>= 0;
      chunk[i] = 0x20 + (state % 95);
    }
    chunks.push(chunk);
    remaining -= size;
  }
  return chunks;
}

export function generateAnsiSgrChunks(totalBytes: number, chunkBytes: number): Uint8Array[] {
  const ansiPatterns = [
    "\x1b[0m",
    "\x1b[31;1m",
    "\x1b[32;4m",
    "\x1b[38;2;120;200;255m",
    "\x1b[48;5;236m",
    "\x1b[2K\x1b[1G",
    "\x1b[A\x1b[2K",
    "plain text token \n",
  ];

  const encoder = new TextEncoder();
  const encodedPatterns = ansiPatterns.map((p) => encoder.encode(p));

  const fullBuffer = new Uint8Array(totalBytes);
  let offset = 0;
  let patternIdx = 0;
  while (offset < totalBytes) {
    const pattern = encodedPatterns[patternIdx % encodedPatterns.length]!;
    patternIdx++;
    if (offset + pattern.length <= totalBytes) {
      fullBuffer.set(pattern, offset);
      offset += pattern.length;
    } else {
      while (offset < totalBytes) {
        fullBuffer[offset++] = 0x20;
      }
    }
  }

  const chunks: Uint8Array[] = [];
  for (let i = 0; i < totalBytes; i += chunkBytes) {
    chunks.push(fullBuffer.subarray(i, Math.min(i + chunkBytes, totalBytes)));
  }
  return chunks;
}

export function generateUnicodeChunks(totalBytes: number, chunkBytes: number): Uint8Array[] {
  const unicodeTokens = [
    "한글 테스트 터미널 텍스트 \n",
    "日本語ターミナル表示 \n",
    "🚀✨🔥⚡️🎉💻 \n",
    "Wide glyph: 寬度測試 🐉 \n",
    "Combined: e\u0301 a\u0300 n\u0303 \n",
    "ASCII fallback segment \n",
  ];

  const encoder = new TextEncoder();
  const encodedTokens = unicodeTokens.map((t) => encoder.encode(t));

  const fullBuffer = new Uint8Array(totalBytes);
  let offset = 0;
  let tokenIdx = 0;
  while (offset < totalBytes) {
    const token = encodedTokens[tokenIdx % encodedTokens.length]!;
    tokenIdx++;
    if (offset + token.length <= totalBytes) {
      fullBuffer.set(token, offset);
      offset += token.length;
    } else {
      // Fill remainder with single-byte ASCII spaces so concatenated stream remains valid UTF-8
      while (offset < totalBytes) {
        fullBuffer[offset++] = 0x20;
      }
    }
  }

  const chunks: Uint8Array[] = [];
  for (let i = 0; i < totalBytes; i += chunkBytes) {
    chunks.push(fullBuffer.subarray(i, Math.min(i + chunkBytes, totalBytes)));
  }
  return chunks;
}

export interface MultiPaneStream {
  paneCount: number;
  bytesPerPane: number;
  paneChunks: Map<number, Uint8Array[]>;
}

export function generateMultiPaneWorkload(paneCount: number, bytesPerPane: number, chunkBytes: number): MultiPaneStream {
  const paneChunks = new Map<number, Uint8Array[]>();
  for (let pane = 0; pane < paneCount; pane++) {
    // Alternate workload types across panes
    if (pane % 3 === 0) {
      paneChunks.set(pane, generateAsciiChunks(bytesPerPane, chunkBytes));
    } else if (pane % 3 === 1) {
      paneChunks.set(pane, generateAnsiSgrChunks(bytesPerPane, chunkBytes));
    } else {
      paneChunks.set(pane, generateUnicodeChunks(bytesPerPane, chunkBytes));
    }
  }
  return { paneCount, bytesPerPane, paneChunks };
}
