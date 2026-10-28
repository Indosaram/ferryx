import { describe, expect, it, vi } from "vitest";

import {
  appendHistory,
  fetchPairedResources,
  formatBytes,
  formatDuration,
  formatPercent,
  groupSessionsByWorktree,
  resourceHosts,
  RESOURCE_HISTORY_LIMIT,
  sessionsByCost,
  unavailableLabel,
  usageRatio,
  worktreeLabel,
  type HostResourceSnapshot,
  type SessionResourceUsage,
} from "./systemResources";
import type { HostEndpoint } from "../state/remoteHostStore";

function session(overrides: Partial<SessionResourceUsage>): SessionResourceUsage {
  return {
    sessionId: "session",
    pid: 1,
    worktreePath: null,
    cpuPercent: 0,
    residentBytes: 0,
    processCount: 1,
    ...overrides,
  };
}

describe("systemResources", () => {
  it("renders a missing metric as a dash rather than zero", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(undefined)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
    expect(formatPercent(null)).toBe("—");
    expect(formatDuration(null)).toBe("—");
  });

  it("formats bytes with unit scaling", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.00 KB");
    expect(formatBytes(1536)).toBe("1.50 KB");
    expect(formatBytes(1024 * 1024 * 1024 * 8)).toBe("8.00 GB");
  });

  it("formats percentages and durations", () => {
    expect(formatPercent(0.426)).toBe("43%");
    expect(formatDuration(90)).toBe("1m");
    expect(formatDuration(3700)).toBe("1h 1m");
    expect(formatDuration(90000)).toBe("1d 1h");
  });

  it("derives a usage ratio only from a positive total", () => {
    expect(usageRatio(5, 10)).toBe(0.5);
    expect(usageRatio(20, 10)).toBe(1);
    expect(usageRatio(null, 10)).toBeNull();
    expect(usageRatio(5, 0)).toBeNull();
    expect(usageRatio(5, null)).toBeNull();
  });

  it("labels a session by its worktree folder and falls back to the session id", () => {
    expect(worktreeLabel(session({ worktreePath: "/Volumes/T9/project/ferryx" }))).toBe("ferryx");
    expect(worktreeLabel(session({ worktreePath: "C:\\Users\\me\\proj" }))).toBe("proj");
    expect(worktreeLabel(session({ worktreePath: null, sessionId: "abc-123" }))).toBe("abc-123");
  });

  it("orders sessions by memory and breaks ties by cpu", () => {
    const ordered = sessionsByCost([
      session({ sessionId: "small", residentBytes: 100, cpuPercent: 9 }),
      session({ sessionId: "big", residentBytes: 900, cpuPercent: 1 }),
      session({ sessionId: "mid", residentBytes: 500, cpuPercent: 2 }),
      session({ sessionId: "unknown", residentBytes: null, cpuPercent: null }),
    ]);
    expect(ordered.map((entry) => entry.sessionId)).toEqual(["big", "mid", "small", "unknown"]);
  });

  it("names only the metrics the host could not report", () => {
    expect(unavailableLabel([])).toBeNull();
    expect(unavailableLabel(["cpuUtilization"])).toBe("This host could not report: cpuUtilization");
    expect(unavailableLabel(["disk", "swapTotalBytes"])).toBe(
      "This host could not report: disk, swapTotalBytes",
    );
  });

  const host = { hostId: "h1", generation: "4" };
  const sample: HostResourceSnapshot = {
    sampledAtMs: 1, platform: "linux", cpuCount: 8, cpuUtilization: 0.5, loadAverage1m: null,
    memoryTotalBytes: 100, memoryUsedBytes: 25, swapTotalBytes: null, swapUsedBytes: null,
    uptimeSeconds: null, diskTotalBytes: null, diskFreeBytes: null, processCount: null,
    sessions: [], unavailable: [],
  };

  it("reads a paired host through the typed resources operation", async () => {
    const call = vi.fn(async () => ({ hostId: "h1", generation: "4", result: { kind: "resources", data: sample } }));
    await expect(fetchPairedResources(host, call)).resolves.toEqual({ status: "sampled", snapshot: sample });
    expect(call).toHaveBeenCalledWith("paired_host_operation", {
      request: { hostId: "h1", generation: "4", operation: { kind: "resources" } },
    });
  });

  it("treats a missing capability as update-required and anything else as unreachable", async () => {
    const refuse = (error: unknown) => vi.fn(async () => { throw error; });
    await expect(fetchPairedResources(host, refuse({ code: "PAIRED_HOST_CAPABILITY_UNAVAILABLE" })))
      .resolves.toEqual({ status: "update-required" });
    await expect(fetchPairedResources(host, refuse({ code: "TRANSPORT" }))).resolves.toEqual({ status: "unreachable" });
    await expect(fetchPairedResources(host, refuse("boom"))).resolves.toEqual({ status: "unreachable" });
  });

  it("rejects a cross-host or malformed answer instead of showing it", async () => {
    const answer = (value: unknown) => vi.fn(async () => value);
    await expect(fetchPairedResources(host, answer({ hostId: "other", result: { kind: "resources", data: sample } })))
      .resolves.toEqual({ status: "unreachable" });
    await expect(fetchPairedResources(host, answer({ hostId: "h1", result: { kind: "resources", data: { platform: 1 } } })))
      .resolves.toEqual({ status: "unreachable" });
    await expect(fetchPairedResources(host, answer(null))).resolves.toEqual({ status: "unreachable" });
  });

  it("lists only paired machine-grant hosts, by name", () => {
    const make = (hostId: string, name: string, extra: Partial<HostEndpoint> = {}): HostEndpoint => ({
      hostId, name, address: "", transport: "relay", authStatus: "paired", online: true, generation: "1", grantScope: "machine", ...extra,
    });
    const hosts = resourceHosts({
      b: make("b", "zed"), a: make("a", "alpha"), m: make("m", "mirror", { grantScope: "mirror" }),
      r: make("r", "revoked", { authStatus: "revoked" }), g: make("g", "nogen", { generation: undefined }),
    });
    expect(hosts.map((entry) => entry.hostId)).toEqual(["a", "b"]);
  });

  it("groups sessions by worktree and sums only known costs", () => {
    const groups = groupSessionsByWorktree([
      session({ sessionId: "a1", worktreePath: "/r/a", cpuPercent: 2, residentBytes: 100 }),
      session({ sessionId: "a2", worktreePath: "/r/a", cpuPercent: null, residentBytes: 300 }),
      session({ sessionId: "x", worktreePath: null, cpuPercent: null, residentBytes: null }),
    ], "memory");
    expect(groups.map((group) => [group.label, group.cpuPercent, group.residentBytes, group.sessions.map((s) => s.sessionId)])).toEqual([
      ["a", 2, 400, ["a2", "a1"]],
      ["Other sessions", null, null, ["x"]],
    ]);
  });

  it("keeps a bounded history and records a missing reading as a gap", () => {
    let history = appendHistory(undefined, sample);
    history = appendHistory(history, { ...sample, cpuUtilization: null, memoryTotalBytes: null });
    expect(history).toEqual({ cpu: [0.5, null], memory: [0.25, null] });
    for (let i = 0; i < RESOURCE_HISTORY_LIMIT + 5; i += 1) history = appendHistory(history, sample);
    expect(history.cpu).toHaveLength(RESOURCE_HISTORY_LIMIT);
  });
});
