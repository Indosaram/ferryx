import { describe, expect, it } from "vitest";
import {
  assertHonestFeatureBaseline,
  assertHonestMeasurementSurface,
  calculateMibPerSec,
  computeChecksum,
  generateAnsiSgrChunks,
  generateAsciiChunks,
  generateMultiPaneWorkload,
  generateUnicodeChunks,
  getXtermFeatureBaseline,
  median,
  percentile,
  type WorkloadResult,
  type XtermFeatureBaseline,
} from "./workloads";

describe("Terminal Baseline Benchmark Utilities", () => {
  it("computes median accurately for odd and even datasets", () => {
    expect(median([10, 20, 30])).toBe(20);
    expect(median([30, 10, 20])).toBe(20);
    expect(median([1, 2, 3, 4])).toBe(3); // index 2 of [1,2,3,4]
    expect(median([])).toBe(0);
  });

  it("computes percentiles accurately", () => {
    const data = Array.from({ length: 100 }, (_, i) => i + 1); // 1 to 100
    expect(percentile(data, 50)).toBe(51);
    expect(percentile(data, 95)).toBe(96);
    expect(percentile(data, 99)).toBe(100);
    expect(percentile([], 50)).toBe(0);
  });

  it("calculates MiB/s throughput correctly", () => {
    const tenMiB = 10 * 1024 * 1024;
    // 10 MiB in 1000ms = 10 MiB/s
    expect(calculateMibPerSec(tenMiB, 1000)).toBeCloseTo(10, 2);
    // 10 MiB in 100ms = 100 MiB/s
    expect(calculateMibPerSec(tenMiB, 100)).toBeCloseTo(100, 2);
    // 0 ms duration guard
    expect(calculateMibPerSec(tenMiB, 0)).toBe(0);
  });

  it("generates deterministic ASCII chunks matching exact size constraints", () => {
    const totalBytes = 64 * 1024; // 64 KiB
    const chunkSize = 16 * 1024; // 16 KiB
    const chunks = generateAsciiChunks(totalBytes, chunkSize);

    expect(chunks.length).toBe(4);
    const aggregateBytes = chunks.reduce((acc, c) => acc + c.byteLength, 0);
    expect(aggregateBytes).toBe(totalBytes);

    // Verify ASCII characters are printable (between 0x20 and 0x7e)
    for (const chunk of chunks) {
      for (let i = 0; i < chunk.length; i++) {
        expect(chunk[i]).toBeGreaterThanOrEqual(0x20);
        expect(chunk[i]).toBeLessThanOrEqual(0x7e);
      }
    }

    // Verify determinism
    const chunks2 = generateAsciiChunks(totalBytes, chunkSize);
    expect(computeChecksum(chunks[0]!)).toBe(computeChecksum(chunks2[0]!));
  });

  it("generates valid ANSI / SGR escape sequences with correct total bytes", () => {
    const totalBytes = 32 * 1024;
    const chunkSize = 8 * 1024;
    const chunks = generateAnsiSgrChunks(totalBytes, chunkSize);

    expect(chunks.length).toBe(4);
    const aggregateBytes = chunks.reduce((acc, c) => acc + c.byteLength, 0);
    expect(aggregateBytes).toBe(totalBytes);

    const decoder = new TextDecoder();
    const text = decoder.decode(chunks[0]);
    expect(text).toContain("\x1b[");
  });

  it("generates valid Unicode UTF-8 streams with correct total bytes", () => {
    const totalBytes = 32 * 1024;
    const chunkSize = 8 * 1024;
    const chunks = generateUnicodeChunks(totalBytes, chunkSize);

    expect(chunks.length).toBe(4);
    const aggregateBytes = chunks.reduce((acc, c) => acc + c.byteLength, 0);
    expect(aggregateBytes).toBe(totalBytes);
  });

  it("generates valid UTF-8 byte stream across concatenated Unicode chunk boundaries", () => {
    const totalBytes = 100 * 1024; // 100 KiB
    const chunkSize = 1024;
    const chunks = generateUnicodeChunks(totalBytes, chunkSize);

    const total = chunks.reduce((acc, c) => acc + c.byteLength, 0);
    expect(total).toBe(totalBytes);

    const concatenated = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      concatenated.set(chunk, offset);
      offset += chunk.byteLength;
    }

    const decoder = new TextDecoder("utf-8", { fatal: true });
    expect(() => decoder.decode(concatenated)).not.toThrow();
  });

  it("verifies every reported metric measurementSurface cannot falsely claim xterm/parser when only input/checksum work was measured", () => {
    // A checksum or memory-only workload must declare an honest measurementSurface
    const checksumMetric: WorkloadResult = {
      id: "checksum-only",
      name: "Checksum Loop",
      category: "throughput",
      measurementSurface: "xterm-headless-parser", // False claim!
      totalBytes: 1024,
      chunkBytes: 1024,
      chunkCount: 1,
      runs: 1,
      medianMs: 1,
      minMs: 1,
      maxMs: 1,
      mibPerSec: 1,
      checksum: 42,
    };

    expect(() => assertHonestMeasurementSurface(checksumMetric, "checksum")).toThrow(
      /dishonest measurement surface/i,
    );

    // Valid honest surface checks
    const honestChecksumMetric: WorkloadResult = {
      ...checksumMetric,
      measurementSurface: "js-buffer-in-memory",
    };
    expect(() => assertHonestMeasurementSurface(honestChecksumMetric, "checksum")).not.toThrow();

    const honestXtermMetric: WorkloadResult = {
      ...checksumMetric,
      measurementSurface: "xterm-headless-parser",
    };
    expect(() => assertHonestMeasurementSurface(honestXtermMetric, "xterm-parser")).not.toThrow();
  });

  it("generates multi-pane workload with correct pane partitioning", () => {
    const paneCount = 4;
    const bytesPerPane = 16 * 1024;
    const chunkSize = 4 * 1024;
    const workload = generateMultiPaneWorkload(paneCount, bytesPerPane, chunkSize);

    expect(workload.paneCount).toBe(paneCount);
    expect(workload.paneChunks.size).toBe(paneCount);

    for (let pane = 0; pane < paneCount; pane++) {
      const chunks = workload.paneChunks.get(pane)!;
      expect(chunks).toBeDefined();
      expect(chunks.length).toBe(4);
      const total = chunks.reduce((acc, c) => acc + c.byteLength, 0);
      expect(total).toBe(bytesPerPane);
    }
  });

  it("rejects feature baseline entries claiming command verification without evidence", () => {
    const invalidBaseline: XtermFeatureBaseline = {
      xtermPackage: "@xterm/xterm",
      xtermVersion: "6.0.0",
      addons: [],
      features: [
        {
          id: "fake-feature",
          name: "Fake Feature",
          sourceFiles: ["ui/src/fake.ts"],
          contractDescription: "Fake contract",
          verificationStatus: "command_verified",
          hasCommandEvidence: false, // Inconsistent with command_verified!
          commandEvidence: undefined,
          manualQaRequired: false,
        },
      ],
      settingsContracts: {
        fontFamily: { default: "monospace", configurable: true, source: "default" },
        fontSize: { default: 13, min: 10, max: 36, configurable: true },
        scrollback: { default: 10000, min: 1000, max: 100000, configurable: true },
        cursorStyle: { options: ["block"], default: "block" },
        macosOptionAsAlt: { default: false, configurable: true },
        themeColors: { paletteKeys: [], defaultBackground: "#000", defaultForeground: "#fff" },
      },
      unmeasuredRealSurfaceScenarios: [],
    };

    expect(() => assertHonestFeatureBaseline(invalidBaseline)).toThrow(
      /dishonest feature contract/i,
    );

    // Also rejects unmeasured real surface scenarios falsely claiming command evidence
    const invalidUnmeasuredBaseline: XtermFeatureBaseline = {
      ...invalidBaseline,
      features: [],
      unmeasuredRealSurfaceScenarios: [
        {
          id: "fake-unmeasured",
          name: "Fake Unmeasured",
          sourceFiles: ["ui/src/fake.ts"],
          contractDescription: "Fake",
          verificationStatus: "unmeasured_real_surface",
          hasCommandEvidence: true, // False claim!
          commandEvidence: "bun test fake.ts",
          manualQaRequired: false,
        },
      ],
    };

    expect(() => assertHonestFeatureBaseline(invalidUnmeasuredBaseline)).toThrow(
      /unmeasured scenario/i,
    );
  });

  it("records feature contracts as source-audited until their command evidence runs", () => {
    const baseline = getXtermFeatureBaseline();

    // Must satisfy honesty invariants
    expect(() => assertHonestFeatureBaseline(baseline)).not.toThrow();

    expect(baseline.xtermPackage).toBe("@xterm/xterm");
    expect(baseline.xtermVersion).toBe("6.0.0");
    expect(baseline.addons.length).toBeGreaterThanOrEqual(4);

    // Required feature contracts from present UI code
    const requiredAutomatedFeatureIds = [
      "binary-scheduled-output-replay",
      "resize-handling",
      "input-forwarding",
      "title-reporting",
      "bell-notification",
      "search-addon",
      "webgl-renderer",
      "terminal-settings-contracts",
      "desktop-vs-browser-boundary",
    ];
    for (const id of requiredAutomatedFeatureIds) {
      const entry = baseline.features.find((f) => f.id === id);
      expect(entry).toBeDefined();
      expect(entry?.sourceFiles.length).toBeGreaterThan(0);
      expect(entry?.verificationStatus).toBe("source_audited");
      expect(entry?.hasCommandEvidence).toBe(false);
      expect(entry?.commandEvidence).toBeUndefined();
    }

    // Unicode11 addon is source-audited with manual QA required (no dedicated vitest test file in UI)
    const unicode11Entry = baseline.features.find((f) => f.id === "unicode11-addon");
    expect(unicode11Entry).toBeDefined();
    expect(unicode11Entry?.verificationStatus).toBe("source_audited");
    expect(unicode11Entry?.hasCommandEvidence).toBe(false);
    expect(unicode11Entry?.manualQaRequired).toBe(true);

    // Required settings contracts
    expect(baseline.settingsContracts.fontFamily.default).toContain("monospace");
    expect(baseline.settingsContracts.fontSize.min).toBe(10);
    expect(baseline.settingsContracts.fontSize.max).toBe(36);
    expect(baseline.settingsContracts.scrollback.default).toBe(10000);
    expect(baseline.settingsContracts.cursorStyle.options).toEqual(["block", "bar", "underline"]);

    // Unmeasured desktop real-surface scenarios
    const requiredUnmeasuredIds = [
      "idle-cpu-multi-session",
      "tab-switch-real-latency",
      "split-resize-continuous-fps",
      "hmr-restore-process-recovery",
    ];
    expect(baseline.unmeasuredRealSurfaceScenarios.length).toBe(requiredUnmeasuredIds.length);
    for (const id of requiredUnmeasuredIds) {
      const scenario = baseline.unmeasuredRealSurfaceScenarios.find((s) => s.id === id);
      expect(scenario).toBeDefined();
      expect(scenario?.verificationStatus).toBe("unmeasured_real_surface");
      // MUST NOT falsely claim command evidence or automated pass
      expect(scenario?.hasCommandEvidence).toBe(false);
      expect(scenario?.manualQaRequired).toBe(true);
    }
  });
});
