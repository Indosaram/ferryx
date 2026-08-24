const HISTOGRAM_CAPACITY = 4096;
const DUMP_INTERVAL_MS = 5_000;

export type TerminalThroughputHistogram = {
  count: number;
  p50Ms: number;
  p95Ms: number;
  maxMs: number;
};

const receiveToDrainedSamplesMs: number[] = [];
let lastDumpAtMs = metricsNow();

export function terminalThroughputMetricsEnabled(): boolean {
  return import.meta.env.DEV && import.meta.env.VITE_FERRYX_TERMINAL_METRICS === "1";
}

export function metricsNow(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

export function recordReceiveToDrained(receivedAtMs: readonly number[]): void {
  if (!terminalThroughputMetricsEnabled() || receivedAtMs.length === 0) return;

  const drainedAtMs = metricsNow();
  for (const receivedAt of receivedAtMs) {
    if (!Number.isFinite(receivedAt)) continue;
    receiveToDrainedSamplesMs.push(Math.max(0, drainedAtMs - receivedAt));
  }
  if (receiveToDrainedSamplesMs.length > HISTOGRAM_CAPACITY) {
    receiveToDrainedSamplesMs.splice(0, receiveToDrainedSamplesMs.length - HISTOGRAM_CAPACITY);
  }

  if (drainedAtMs - lastDumpAtMs >= DUMP_INTERVAL_MS) {
    const snapshot = getTerminalThroughputMetricsSnapshot();
    lastDumpAtMs = drainedAtMs;
    console.debug("[terminal-output] receive->drained", snapshot);
  }
}

export function getTerminalThroughputMetricsSnapshot(): TerminalThroughputHistogram {
  if (receiveToDrainedSamplesMs.length === 0) {
    return { count: 0, p50Ms: 0, p95Ms: 0, maxMs: 0 };
  }

  const sorted = [...receiveToDrainedSamplesMs].sort((a, b) => a - b);
  return {
    count: sorted.length,
    p50Ms: percentile(sorted, 0.50),
    p95Ms: percentile(sorted, 0.95),
    maxMs: sorted[sorted.length - 1] ?? 0,
  };
}

export function resetTerminalThroughputMetricsForTest(): void {
  receiveToDrainedSamplesMs.length = 0;
  lastDumpAtMs = metricsNow();
}

function percentile(sorted: readonly number[], fraction: number): number {
  if (sorted.length === 0) return 0;
  const index = Math.ceil((sorted.length - 1) * fraction);
  return sorted[Math.min(index, sorted.length - 1)] ?? 0;
}
