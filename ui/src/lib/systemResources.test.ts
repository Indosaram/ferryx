import { describe, expect, it } from "vitest";

import {
  formatBytes,
  formatDuration,
  formatPercent,
  sessionsByCost,
  unavailableLabel,
  usageRatio,
  worktreeLabel,
  type SessionResourceUsage,
} from "./systemResources";

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
});
