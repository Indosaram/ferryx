import { describe, expect, test } from "bun:test";
import { runSoak, summarize } from "./soak.mjs";

describe("remote input latency measurement", () => {
  test("uses nearest-rank percentiles without changing the samples", () => {
    const samples = [7, 3, 10, 1, 9, 4, 6, 2, 8, 5];
    expect(summarize(samples)).toEqual({
      sampleCount: 10, min: 1, p50: 5, p90: 9, p95: 10, p99: 10, max: 10,
    });
    expect(samples).toEqual([7, 3, 10, 1, 9, 4, 6, 2, 8, 5]);
    expect(() => summarize([])).toThrow();
  });

  test("measures every host through real sockets, excluding warmup", async () => {
    const result = await runSoak({ hostCount: 3, durationMs: 0, warmupSamples: 2, timeoutMs: 5000 });
    expect(result.schemaVersion).toBe(1);
    expect(result.hosts.map((host) => host.hostId)).toEqual(["host-0", "host-1", "host-2"]);
    expect(result.hosts.map((host) => host.latencyMs.sampleCount)).toEqual([1, 1, 1]);
    expect(result.latencyMs.sampleCount).toBe(3);
    expect(result.warmupSamplesPerHost).toBe(2);
    expect(result.durationMs).toBeGreaterThanOrEqual(0);
    expect(result.latencyMs.min).toBeGreaterThanOrEqual(0);
    expect(result.latencyMs.p50).toBeLessThanOrEqual(result.latencyMs.p99);
    expect(result.latencyMs.p99).toBeLessThanOrEqual(result.latencyMs.max);
  });

  test("rejects invalid run configuration before creating hosts", async () => {
    for (const options of [{ hostCount: 0 }, { hostCount: 1.5 }, { durationMs: -1 },
      { durationMs: Infinity }, { warmupSamples: -1 }, { timeoutMs: 0 }]) {
      await expect(runSoak(options)).rejects.toThrow();
    }
  });
});
