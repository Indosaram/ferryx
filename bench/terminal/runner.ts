import { spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { Terminal } from "@xterm/xterm";
import {
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
  type BaselineReport,
  type WorkloadResult,
} from "./workloads";

const EVIDENCE_DIR = path.resolve(__dirname, "evidence");

function getXtermInfo(): { package: string; version: string } {
  try {
    const pkgPath = path.resolve(__dirname, "../../ui/node_modules/@xterm/xterm/package.json");
    if (fs.existsSync(pkgPath)) {
      const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8"));
      return {
        package: pkg.name ?? "@xterm/xterm",
        version: pkg.version ?? "6.0.0",
      };
    }
  } catch {}
  return { package: "@xterm/xterm", version: "6.0.0" };
}

function runWrappedThroughputBenchmark(): {
  directMibPerSec: number;
  legacyMibPerSec: number;
  directMedianMs: number;
  legacyMedianMs: number;
  speedupFactor: number;
  directRunsMs: number[];
  legacyRunsMs: number[];
  stdout: string;
} {
  const benchScript = path.resolve(__dirname, "../../ui/src/lib/terminalThroughput.bench.ts");
  if (!fs.existsSync(benchScript)) {
    throw new Error(`Throughput benchmark script not found: ${benchScript}`);
  }

  const result = spawnSync("bun", ["run", benchScript], {
    encoding: "utf-8",
    stdio: ["ignore", "pipe", "pipe"],
  });

  if (result.status !== 0) {
    throw new Error(`Wrapped benchmark failed with exit code ${result.status}: ${result.stderr}`);
  }

  const stdout = result.stdout;

  // Parse outputs from stdout
  const directMatch = stdout.match(/Uint8Array pass-through\s+([0-9.]+)\s+([0-9.]+)/);
  const legacyMatch = stdout.match(/base64 -> atob -> Uint8Array\s+([0-9.]+)\s+([0-9.]+)/);
  const directRunsMatch = stdout.match(/direct runs ms:\s*([0-9.,\s]+)/);
  const legacyRunsMatch = stdout.match(/legacy runs ms:\s*([0-9.,\s]+)/);

  if (!directMatch || !legacyMatch) {
    throw new Error(`Failed to parse wrapped benchmark output:\n${stdout}`);
  }

  const directMedianMs = parseFloat(directMatch[1]!);
  const directMibPerSec = parseFloat(directMatch[2]!);
  const legacyMedianMs = parseFloat(legacyMatch[1]!);
  const legacyMibPerSec = parseFloat(legacyMatch[2]!);

  const directRunsMs = directRunsMatch
    ? directRunsMatch[1]!.split(",").map((s) => parseFloat(s.trim())).filter((n) => !isNaN(n))
    : [directMedianMs];
  const legacyRunsMs = legacyRunsMatch
    ? legacyRunsMatch[1]!.split(",").map((s) => parseFloat(s.trim())).filter((n) => !isNaN(n))
    : [legacyMedianMs];

  const speedupFactor = legacyMedianMs > 0 ? legacyMedianMs / directMedianMs : 0;

  return {
    directMibPerSec,
    legacyMibPerSec,
    directMedianMs,
    legacyMedianMs,
    speedupFactor,
    directRunsMs,
    legacyRunsMs,
    stdout,
  };
}

async function writeChunksToTerminal(term: Terminal, chunks: Uint8Array[]): Promise<void> {
  for (const chunk of chunks) {
    await new Promise<void>((resolve) => {
      term.write(chunk, resolve);
    });
  }
}

async function runXtermChunkBenchmark(
  id: string,
  name: string,
  category: WorkloadResult["category"],
  chunks: Uint8Array[],
  totalBytes: number,
  runs = 3,
  notes?: string,
): Promise<WorkloadResult> {
  const durations: number[] = [];

  // Warm-up run
  const warmupTerm = new Terminal({ cols: 80, rows: 24, scrollback: 1000 });
  const warmupChunks = chunks.slice(0, Math.min(chunks.length, 10));
  await writeChunksToTerminal(warmupTerm, warmupChunks);
  warmupTerm.dispose();

  let finalChecksum = 0;
  for (const chunk of chunks) {
    finalChecksum = computeChecksum(chunk, finalChecksum);
  }

  for (let r = 0; r < runs; r++) {
    const term = new Terminal({ cols: 80, rows: 24, scrollback: 1000 });
    const start = performance.now();
    await writeChunksToTerminal(term, chunks);
    const duration = performance.now() - start;
    durations.push(duration);
    term.dispose();
  }

  const medianMs = median(durations);
  const minMs = Math.min(...durations);
  const maxMs = Math.max(...durations);
  const mibPerSec = calculateMibPerSec(totalBytes, medianMs);

  const result: WorkloadResult = {
    id,
    name,
    category,
    measurementSurface: "xterm-headless-parser",
    totalBytes,
    chunkBytes: chunks[0]?.byteLength ?? 0,
    chunkCount: chunks.length,
    runs,
    medianMs,
    minMs,
    maxMs,
    mibPerSec,
    checksum: finalChecksum,
    notes,
  };
  assertHonestMeasurementSurface(result, "xterm-parser");
  return result;
}

async function runMultiPaneBenchmark(
  paneCount = 4,
  bytesPerPane = 10 * 1024 * 1024,
  chunkBytes = 32 * 1024,
  runs = 3,
): Promise<WorkloadResult> {
  const totalBytes = paneCount * bytesPerPane;
  const workload = generateMultiPaneWorkload(paneCount, bytesPerPane, chunkBytes);
  const durations: number[] = [];
  const chunkLatencies: number[] = [];
  let finalChecksum = 0;

  for (let r = 0; r < runs; r++) {
    const terms = Array.from({ length: paneCount }, () => new Terminal({ cols: 80, rows: 24, scrollback: 1000 }));
    const maxChunksPerPane = Math.ceil(bytesPerPane / chunkBytes);
    let runChecksum = 0;

    const start = performance.now();
    for (let round = 0; round < maxChunksPerPane; round++) {
      const roundPromises: Promise<void>[] = [];
      for (let pane = 0; pane < paneCount; pane++) {
        const paneArray = workload.paneChunks.get(pane)!;
        const chunk = paneArray[round];
        if (chunk) {
          if (r === 0) {
            runChecksum = computeChecksum(chunk, runChecksum);
          }
          const chunkStart = performance.now();
          const p = new Promise<void>((resolve) => {
            terms[pane]!.write(chunk, () => {
              if (r === 0) {
                chunkLatencies.push(performance.now() - chunkStart);
              }
              resolve();
            });
          });
          roundPromises.push(p);
        }
      }
      await Promise.all(roundPromises);
    }
    const duration = performance.now() - start;
    durations.push(duration);
    finalChecksum = runChecksum;
    terms.forEach((t) => t.dispose());
  }

  const medianMs = median(durations);
  const minMs = Math.min(...durations);
  const maxMs = Math.max(...durations);
  const mibPerSec = calculateMibPerSec(totalBytes, medianMs);

  const result: WorkloadResult = {
    id: `multipane-${paneCount}x${bytesPerPane / (1024 * 1024)}mb`,
    name: `Multi-Pane Concurrent (${paneCount} panes x ${bytesPerPane / (1024 * 1024)} MiB)`,
    category: "multipane",
    measurementSurface: "xterm-headless-parser",
    totalBytes,
    chunkBytes,
    chunkCount: totalBytes / chunkBytes,
    runs,
    medianMs,
    minMs,
    maxMs,
    mibPerSec,
    latency: {
      p50Ms: percentile(chunkLatencies, 50),
      p95Ms: percentile(chunkLatencies, 95),
      p99Ms: percentile(chunkLatencies, 99),
    },
    checksum: finalChecksum,
    notes: "Interleaved multi-pane chunk dispatch simulating simultaneous PTY channels into xterm parsers",
  };
  assertHonestMeasurementSurface(result, "xterm-parser");
  return result;
}

async function runInteractivityLatencyBenchmark(eventsCount = 1000): Promise<WorkloadResult> {
  const latencies: number[] = [];
  let checksum = 0;
  const term = new Terminal({ cols: 80, rows: 24 });
  const keyData = new Uint8Array([0x1b, 0x5b, 0x41]); // ArrowUp sequence

  // Warmup
  for (let i = 0; i < 10; i++) {
    await new Promise<void>((resolve) => term.write(keyData, resolve));
  }

  const startTotal = performance.now();
  for (let i = 0; i < eventsCount; i++) {
    checksum = computeChecksum(keyData, checksum);
    const t0 = performance.now();
    await new Promise<void>((resolve) => term.write(keyData, resolve));
    const t1 = performance.now();
    latencies.push(t1 - t0);
  }
  const totalMs = performance.now() - startTotal;
  term.dispose();

  const result: WorkloadResult = {
    id: "interactivity-event-dispatch",
    name: `Interactive Keystroke / Event Dispatch (${eventsCount.toLocaleString()} cycles)`,
    category: "interactivity",
    measurementSurface: "xterm-headless-parser",
    totalBytes: eventsCount * 3,
    chunkBytes: 3,
    chunkCount: eventsCount,
    runs: 1,
    medianMs: totalMs,
    minMs: Math.min(...latencies),
    maxMs: Math.max(...latencies),
    mibPerSec: calculateMibPerSec(eventsCount * 3, totalMs),
    latency: {
      p50Ms: percentile(latencies, 50),
      p95Ms: percentile(latencies, 95),
      p99Ms: percentile(latencies, 99),
    },
    checksum,
    notes: "Microsecond dispatch jitter profile for terminal input handling through xterm write boundary",
  };
  assertHonestMeasurementSurface(result, "xterm-parser");
  return result;
}

export async function executeBaselineSuite(): Promise<BaselineReport> {
  console.log("================================================================================");
  console.log(" Orca Terminal Native Migration - Phase 0 Baseline Benchmark Runner");
  console.log("================================================================================");

  const xtermInfo = getXtermInfo();
  console.log(`Using xterm seam: ${xtermInfo.package}@${xtermInfo.version} (completion-aware write boundary)`);

  // 1. Run Wrapped Microbenchmark
  console.log("\n[1/5] Executing wrapped microbenchmark (ui/src/lib/terminalThroughput.bench.ts)...");
  const wrapped = runWrappedThroughputBenchmark();
  console.log(`  ✓ Direct Pass-Through: ${wrapped.directMibPerSec.toFixed(1)} MiB/s (${wrapped.directMedianMs.toFixed(3)} ms)`);
  console.log(`  ✓ Legacy Base64:       ${wrapped.legacyMibPerSec.toFixed(1)} MiB/s (${wrapped.legacyMedianMs.toFixed(3)} ms)`);
  console.log(`  ✓ Direct Speedup:      ${wrapped.speedupFactor.toFixed(1)}x faster`);

  const CHUNK_SIZE = 32 * 1024; // 32 KiB standard terminal chunk
  const workloads: WorkloadResult[] = [];

  // Add wrapped benchmark as WorkloadResult entries
  const directResult: WorkloadResult = {
    id: "wrapped-direct-10mb",
    name: "Uint8Array Direct Pass-Through (10 MiB)",
    category: "throughput",
    measurementSurface: "js-buffer-in-memory",
    totalBytes: 10 * 1024 * 1024,
    chunkBytes: CHUNK_SIZE,
    chunkCount: 320,
    runs: wrapped.directRunsMs.length,
    medianMs: wrapped.directMedianMs,
    minMs: Math.min(...wrapped.directRunsMs),
    maxMs: Math.max(...wrapped.directRunsMs),
    mibPerSec: wrapped.directMibPerSec,
    checksum: 0,
    notes: "Direct binary chunk ingestion bypassing base64 decoding (in-memory buffer transform)",
  };
  assertHonestMeasurementSurface(directResult, "js-buffer-in-memory");
  workloads.push(directResult);

  const legacyResult: WorkloadResult = {
    id: "wrapped-legacy-base64-10mb",
    name: "Base64 Decoded Legacy Path (10 MiB)",
    category: "throughput",
    measurementSurface: "js-buffer-in-memory",
    totalBytes: 10 * 1024 * 1024,
    chunkBytes: CHUNK_SIZE,
    chunkCount: 320,
    runs: wrapped.legacyRunsMs.length,
    medianMs: wrapped.legacyMedianMs,
    minMs: Math.min(...wrapped.legacyRunsMs),
    maxMs: Math.max(...wrapped.legacyRunsMs),
    mibPerSec: wrapped.legacyMibPerSec,
    checksum: 0,
    notes: "Historical base64 -> atob -> Uint8Array pipeline (in-memory buffer transform)",
  };
  assertHonestMeasurementSurface(legacyResult, "js-buffer-in-memory");
  workloads.push(legacyResult);

  // 2. ASCII Workloads (10MB, 50MB, 100MB) with actual xterm parser
  console.log("\n[2/5] Executing ASCII workloads with xterm parser (10MB, 50MB, 100MB)...");
  const ascii10 = generateAsciiChunks(10 * 1024 * 1024, CHUNK_SIZE);
  const ascii10Res = await runXtermChunkBenchmark("ascii-10mb", "ASCII Stream (10 MiB)", "throughput", ascii10, 10 * 1024 * 1024, 3, "Sustained ASCII stream parsed via xterm VT parser");
  workloads.push(ascii10Res);
  console.log(`  ✓ ASCII 10 MiB:  ${ascii10Res.mibPerSec.toFixed(1)} MiB/s (${ascii10Res.medianMs.toFixed(3)} ms)`);

  const ascii50 = generateAsciiChunks(50 * 1024 * 1024, CHUNK_SIZE);
  const ascii50Res = await runXtermChunkBenchmark("ascii-50mb", "ASCII Stream (50 MiB)", "throughput", ascii50, 50 * 1024 * 1024, 3, "Sustained ASCII stream parsed via xterm VT parser");
  workloads.push(ascii50Res);
  console.log(`  ✓ ASCII 50 MiB:  ${ascii50Res.mibPerSec.toFixed(1)} MiB/s (${ascii50Res.medianMs.toFixed(3)} ms)`);

  const ascii100 = generateAsciiChunks(100 * 1024 * 1024, CHUNK_SIZE);
  const ascii100Res = await runXtermChunkBenchmark("ascii-100mb", "ASCII Stream (100 MiB)", "throughput", ascii100, 100 * 1024 * 1024, 3, "Sustained ASCII stream parsed via xterm VT parser");
  workloads.push(ascii100Res);
  console.log(`  ✓ ASCII 100 MiB: ${ascii100Res.mibPerSec.toFixed(1)} MiB/s (${ascii100Res.medianMs.toFixed(3)} ms)`);

  // 3. ANSI / SGR Workloads with actual xterm parser
  console.log("\n[3/5] Executing ANSI / SGR 24-bit color sequence workload with xterm parser (10MB)...");
  const ansi10 = generateAnsiSgrChunks(10 * 1024 * 1024, CHUNK_SIZE);
  const ansiRes = await runXtermChunkBenchmark("ansi-sgr-10mb", "ANSI SGR / 24-bit Escape Stream (10 MiB)", "parser", ansi10, 10 * 1024 * 1024, 3, "Color codes, cursor positioning, and attribute reset tokens parsed via xterm VT parser");
  workloads.push(ansiRes);
  console.log(`  ✓ ANSI SGR 10 MiB: ${ansiRes.mibPerSec.toFixed(1)} MiB/s (${ansiRes.medianMs.toFixed(3)} ms)`);

  // 4. Unicode / CJK Workload with actual xterm parser
  console.log("\n[4/5] Executing Unicode / CJK / Wide-glyph workload with xterm parser (10MB)...");
  const unicode10 = generateUnicodeChunks(10 * 1024 * 1024, CHUNK_SIZE);
  const unicodeRes = await runXtermChunkBenchmark("unicode-cjk-10mb", "Unicode CJK / Emoji / Wide-Char Stream (10 MiB)", "parser", unicode10, 10 * 1024 * 1024, 3, "Multi-byte UTF-8 sequences, wide glyphs, and combining diacritics parsed via xterm VT parser");
  workloads.push(unicodeRes);
  console.log(`  ✓ Unicode CJK 10 MiB: ${unicodeRes.mibPerSec.toFixed(1)} MiB/s (${unicodeRes.medianMs.toFixed(3)} ms)`);

  // 5. Multi-Pane & Interactivity with actual xterm parser
  console.log("\n[5/5] Executing Multi-Pane concurrency & Interactivity benchmarks with xterm parser...");
  const multiPaneRes = await runMultiPaneBenchmark(4, 10 * 1024 * 1024, CHUNK_SIZE, 3);
  workloads.push(multiPaneRes);
  console.log(`  ✓ Multi-Pane (4x10 MiB): ${multiPaneRes.mibPerSec.toFixed(1)} MiB/s (${multiPaneRes.medianMs.toFixed(3)} ms) | chunk p95: ${multiPaneRes.latency?.p95Ms.toFixed(4)} ms`);

  const interactivityRes = await runInteractivityLatencyBenchmark(1000);
  workloads.push(interactivityRes);
  console.log(`  ✓ Interactivity (1,000 keystrokes): p50=${interactivityRes.latency?.p50Ms.toFixed(4)}ms, p95=${interactivityRes.latency?.p95Ms.toFixed(4)}ms, p99=${interactivityRes.latency?.p99Ms.toFixed(4)}ms`);

  const featureBaseline = getXtermFeatureBaseline();

  const report: BaselineReport = {
    timestamp: new Date().toISOString(),
    environment: {
      os: `${os.type()} ${os.release()} (${os.platform()})`,
      arch: os.arch(),
      runtime: `bun ${process.versions.bun ?? process.version}`,
      cpuCores: os.cpus().length,
      xtermVersion: xtermInfo.version,
      xtermPackage: `${xtermInfo.package}@${xtermInfo.version}`,
    },
    wrappedMicrobench: {
      directMibPerSec: wrapped.directMibPerSec,
      legacyMibPerSec: wrapped.legacyMibPerSec,
      directMedianMs: wrapped.directMedianMs,
      legacyMedianMs: wrapped.legacyMedianMs,
      speedupFactor: wrapped.speedupFactor,
      directRunsMs: wrapped.directRunsMs,
      legacyRunsMs: wrapped.legacyRunsMs,
      measurementSurface: "js-buffer-in-memory",
    },
    workloads,
    featureBaseline,
  };

  // Ensure evidence dir exists
  fs.mkdirSync(EVIDENCE_DIR, { recursive: true });

  const jsonPath = path.join(EVIDENCE_DIR, "baseline-latest.json");
  fs.writeFileSync(jsonPath, JSON.stringify(report, null, 2), "utf-8");

  const featureBaselinePath = path.join(EVIDENCE_DIR, "xterm-feature-baseline.json");
  fs.writeFileSync(featureBaselinePath, JSON.stringify(featureBaseline, null, 2), "utf-8");

  const mdPath = path.join(EVIDENCE_DIR, "baseline-summary.md");
  fs.writeFileSync(mdPath, generateMarkdownSummary(report), "utf-8");

  console.log("\n================================================================================");
  console.log(` Baseline report artifacts saved:`);
  console.log(`  - JSON:    ${jsonPath}`);
  console.log(`  - Feature: ${featureBaselinePath}`);
  console.log(`  - MD:      ${mdPath}`);
  console.log("================================================================================\n");

  return report;
}

function generateMarkdownSummary(report: BaselineReport): string {
  const lines: string[] = [];
  const interactivityP99Ms = report.workloads.find(
    (workload) => workload.id === "interactivity-event-dispatch",
  )?.latency?.p99Ms;
  const nativeInputP99LimitMs = interactivityP99Ms === undefined
    ? undefined
    : interactivityP99Ms * 1.15;
  lines.push("# Terminal Migration Baseline Evidence (Phase 0)");
  lines.push("");
  lines.push(`- **Date**: ${report.timestamp}`);
  lines.push(`- **Environment**: ${report.environment.os} (${report.environment.arch})`);
  lines.push(`- **Runtime**: ${report.environment.runtime}`);
  lines.push(`- **CPU Cores**: ${report.environment.cpuCores}`);
  if (report.environment.xtermPackage) {
    lines.push(`- **xterm Engine**: ${report.environment.xtermPackage}`);
  }
  lines.push("");
  lines.push("## Microbenchmark Results (Wrapped `terminalThroughput.bench.ts`)");
  lines.push("");
  lines.push("| Path | Median Time (ms) | Throughput (MiB/s) | Surface | Notes |");
  lines.push("| --- | ---: | ---: | --- | --- |");
  lines.push(`| **Direct Pass-Through** | ${report.wrappedMicrobench.directMedianMs.toFixed(3)} | **${report.wrappedMicrobench.directMibPerSec.toFixed(1)}** | \`${report.wrappedMicrobench.measurementSurface}\` | Binary Uint8Array chunk streaming |`);
  lines.push(`| **Legacy Base64** | ${report.wrappedMicrobench.legacyMedianMs.toFixed(3)} | **${report.wrappedMicrobench.legacyMibPerSec.toFixed(1)}** | \`${report.wrappedMicrobench.measurementSurface}\` | Base64 decode overhead |`);
  lines.push(`| *Relative Speedup* | - | **${report.wrappedMicrobench.speedupFactor.toFixed(1)}x** | - | Uint8Array vs Base64 |`);
  lines.push("");
  lines.push("## Workload Benchmark Suite");
  lines.push("");
  lines.push("| Workload | Size | Median Time (ms) | Throughput (MiB/s) | Latency p95 (ms) | Surface | Category |");
  lines.push("| --- | ---: | ---: | ---: | ---: | --- | --- |");

  for (const wl of report.workloads) {
    const sizeMb = (wl.totalBytes / (1024 * 1024)).toFixed(1);
    const p95 = wl.latency ? wl.latency.p95Ms.toFixed(4) : "-";
    lines.push(`| \`${wl.id}\` (${wl.name}) | ${sizeMb} MiB | ${wl.medianMs.toFixed(3)} | ${wl.mibPerSec.toFixed(1)} | ${p95} | \`${wl.measurementSurface}\` | ${wl.category} |`);
  }

  if (report.featureBaseline) {
    lines.push("");
    lines.push("## Auditable xterm Feature & Settings Baseline");
    lines.push("");
    lines.push(`- **xterm Core**: \`${report.featureBaseline.xtermPackage}@${report.featureBaseline.xtermVersion}\``);
    lines.push(`- **Installed Addons**: ${report.featureBaseline.addons.map((a) => `\`${a.name}@${a.version}\``).join(", ")}`);
    lines.push("");
    lines.push("### Verified Feature Contracts");
    lines.push("");
    lines.push("| Feature ID | Name | Source Files | Verification Status | Command Evidence |");
    lines.push("| --- | --- | --- | --- | --- |");
    for (const f of report.featureBaseline.features) {
      const src = f.sourceFiles.map((s) => `\`${s}\``).join("<br>");
      const evidence = f.commandEvidence ? `\`${f.commandEvidence}\`` : "Source-audited (Manual QA)";
      lines.push(`| \`${f.id}\` | ${f.name} | ${src} | \`${f.verificationStatus}\` | ${evidence} |`);
    }

    lines.push("");
    lines.push("### Unmeasured Real-Surface Desktop Scenarios");
    lines.push("");
    lines.push("| Scenario ID | Name | Source Files | Status | Command Evidence | Manual QA Required |");
    lines.push("| --- | --- | --- | --- | --- | --- |");
    for (const s of report.featureBaseline.unmeasuredRealSurfaceScenarios) {
      const src = s.sourceFiles.map((f) => `\`${f}\``).join("<br>");
      lines.push(`| \`${s.id}\` | ${s.name} | ${src} | \`${s.verificationStatus}\` | ${s.hasCommandEvidence ? "Yes" : "None"} | **${s.manualQaRequired ? "Required" : "No"}** |`);
    }
  }

  lines.push("");
  lines.push("## Exit Criteria & Native Comparison Contracts");
  lines.push("");
  lines.push("1. **Throughput Parity**: Native throughput on sustained ASCII, ANSI, and Unicode streams must match or exceed the current xterm parser figures in this report.");
  lines.push("2. **Multi-Pane Fairness**: Multi-pane concurrent output must preserve deterministic interleaving; visual frame-drop assessment remains a real-surface Phase 4+ gate.");
  if (interactivityP99Ms === undefined || nativeInputP99LimitMs === undefined) {
    lines.push("3. **Interactive Dispatch (Provisional)**: The current report has no xterm p99 sample, so native input comparison is blocked pending a valid baseline run.");
  } else {
    lines.push(`3. **Interactive Dispatch (Provisional)**: Native headless parser p99 must not exceed this run's xterm write-boundary p99 (${interactivityP99Ms.toFixed(4)} ms) by more than 15% (${nativeInputP99LimitMs.toFixed(4)} ms). End-to-end desktop input-to-PTY latency remains a Phase 4+ real-surface QA gate.`);
  }
  lines.push("4. **Feature & Settings Contract Parity**: All 10 audited feature contracts and configuration schemas must reach full functional parity prior to WebView terminal retirement.");
  lines.push("5. **Real-Surface Desktop QA**: All unmeasured real-surface scenarios (multi-session idle CPU, live tab switch frame budget, continuous split drag FPS, crash recovery) require real-surface manual QA verification before Phase 4 sign-off.");
  lines.push("");
  return lines.join("\n");
}

if (import.meta.main) {
  executeBaselineSuite()
    .then(() => {
      process.exit(0);
    })
    .catch((err) => {
      console.error("Baseline runner failed with error:", err);
      process.exit(1);
    });
}
