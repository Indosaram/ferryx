import { describe, expect, it } from "vitest";
import { normalizeSnapshot } from "./supervisorModel";

const lane = { key: "k", title: "T", status: "working", sessionId: "s1", cwd: "/r", goal: null, paused: false, lastActivityAt: "" };

describe("normalizeSnapshot lastReason", () => {
  it("keeps a string lastReason from the daemon", () => {
    const snapshot = normalizeSnapshot({ hub: { available: true, running: true }, lanes: [{ ...lane, lastReason: "tests failed" }] });
    expect(snapshot.lanes[0]?.lastReason).toBe("tests failed");
  });
  it("defaults a missing lastReason to null for older daemons", () => {
    const snapshot = normalizeSnapshot({ hub: { available: true, running: true }, lanes: [lane] });
    expect(snapshot.lanes[0]?.lastReason).toBeNull();
  });
});
