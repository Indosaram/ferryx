import { describe, expect, it, vi } from "vitest";

import { createSwitchDebugLogger } from "./switchDebug";

describe("switchDebug", () => {
  it("emits ordered structured entries through the configured sink", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: true,
      runId: "run-1",
      now: () => 1234,
      sink,
    });

    expect(log("project.select", { from: "alpha", to: "beta" })).toEqual({
      runId: "run-1",
      sequence: 1,
      event: "project.select",
      wallTimeMs: 1234,
      details: { from: "alpha", to: "beta" },
    });
    expect(log("workspace.swap")).toMatchObject({ sequence: 2 });
    expect(sink).toHaveBeenCalledTimes(2);
  });

  it("does not invoke the sink when disabled", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: false,
      runId: "run-1",
      now: () => 1234,
      sink,
    });

    expect(log("project.select")).toBeNull();
    expect(sink).not.toHaveBeenCalled();
  });
});
