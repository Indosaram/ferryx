import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  getTerminalThroughputMetricsSnapshot,
  metricsNow,
  recordReceiveToDrained,
  resetTerminalThroughputMetricsForTest,
  terminalThroughputMetricsEnabled,
} from "./terminalThroughputMetrics";

const HISTOGRAM_CAPACITY = 4096;
const DUMP_INTERVAL_MS = 5_000;

function enableMetrics(enabled: boolean): void {
  vi.stubEnv("DEV", enabled);
  vi.stubEnv("VITE_FERRYX_TERMINAL_METRICS", enabled ? "1" : "0");
}

function drainAt(drainedAtMs: number, latenciesMs: readonly number[]): void {
  vi.spyOn(performance, "now").mockReturnValue(drainedAtMs);
  recordReceiveToDrained(latenciesMs.map((latency) => drainedAtMs - latency));
}

beforeEach(() => {
  resetTerminalThroughputMetricsForTest();
  enableMetrics(true);
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
  resetTerminalThroughputMetricsForTest();
});

describe("terminalThroughputMetricsEnabled", () => {
  it("is on only when dev mode and the metrics flag agree", () => {
    expect(terminalThroughputMetricsEnabled()).toBe(true);
  });

  it("is off when the metrics flag is not set to 1", () => {
    vi.stubEnv("VITE_FERRYX_TERMINAL_METRICS", "0");
    expect(terminalThroughputMetricsEnabled()).toBe(false);
  });

  it("is off outside dev mode even with the flag set", () => {
    vi.stubEnv("DEV", false);
    expect(terminalThroughputMetricsEnabled()).toBe(false);
  });
});

describe("metricsNow", () => {
  it("reads the high-resolution clock when one is available", () => {
    vi.spyOn(performance, "now").mockReturnValue(123.5);
    expect(metricsNow()).toBe(123.5);
  });
});

describe("snapshot", () => {
  it("reports an empty histogram before any sample arrives", () => {
    expect(getTerminalThroughputMetricsSnapshot()).toEqual({
      count: 0,
      p50Ms: 0,
      p95Ms: 0,
      maxMs: 0,
    });
  });

  it("derives percentiles and max from recorded latencies", () => {
    drainAt(1_000, [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);

    expect(getTerminalThroughputMetricsSnapshot()).toEqual({
      count: 10,
      p50Ms: 60,
      p95Ms: 100,
      maxMs: 100,
    });
  });

  it("clamps a sample whose receive time is in the future to zero", () => {
    drainAt(100, [-50]);

    expect(getTerminalThroughputMetricsSnapshot()).toMatchObject({ count: 1, maxMs: 0 });
  });

  it("skips non-finite receive times", () => {
    vi.spyOn(performance, "now").mockReturnValue(100);
    recordReceiveToDrained([Number.NaN, Number.POSITIVE_INFINITY, 90]);

    expect(getTerminalThroughputMetricsSnapshot()).toMatchObject({ count: 1, maxMs: 10 });
  });

  it("accumulates samples across successive drains", () => {
    drainAt(100, [10]);
    drainAt(200, [20]);

    expect(getTerminalThroughputMetricsSnapshot()).toMatchObject({ count: 2, maxMs: 20 });
  });

  it("retains only the most recent samples once capacity is exceeded", () => {
    const overflowCount = 104;
    const oldestLatency = HISTOGRAM_CAPACITY + overflowCount;
    const oldestFirst = Array.from(
      { length: oldestLatency },
      (_unused, index) => oldestLatency - index,
    );

    drainAt(10_000, oldestFirst);

    expect(getTerminalThroughputMetricsSnapshot()).toMatchObject({
      count: HISTOGRAM_CAPACITY,
      maxMs: oldestLatency - overflowCount,
    });
  });
});

describe("recording gate", () => {
  it("records nothing while metrics are disabled", () => {
    enableMetrics(false);
    drainAt(100, [50]);

    expect(getTerminalThroughputMetricsSnapshot().count).toBe(0);
  });

  it("ignores an empty batch", () => {
    recordReceiveToDrained([]);
    expect(getTerminalThroughputMetricsSnapshot().count).toBe(0);
  });

  it("dumps a snapshot to the debug log once the interval elapses", () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    vi.spyOn(performance, "now").mockReturnValue(0);
    resetTerminalThroughputMetricsForTest();

    drainAt(DUMP_INTERVAL_MS - 1, [10]);
    expect(debug).not.toHaveBeenCalled();

    drainAt(DUMP_INTERVAL_MS, [20]);

    expect(debug).toHaveBeenCalledTimes(1);
    expect(debug.mock.calls[0][0]).toBe("[terminal-output] receive->drained");
    expect(debug.mock.calls[0][1]).toMatchObject({ count: 2 });
  });
});

describe("resetTerminalThroughputMetricsForTest", () => {
  it("discards recorded samples", () => {
    drainAt(100, [50]);

    resetTerminalThroughputMetricsForTest();

    expect(getTerminalThroughputMetricsSnapshot().count).toBe(0);
  });
});
