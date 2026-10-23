import { describe, expect, it } from "vitest";
import {
  actingSessionIds,
  EMPTY_SNAPSHOT,
  nextSupervisionExpiry,
  normalizeSnapshot,
  paneSupervision,
  SUPERVISOR_ACTING_MS,
  type SupervisorSnapshot,
} from "./supervisor";

const lane = { key: "ledger", title: "Ledger", status: "working", sessionId: "s1", cwd: "/r", goal: "Get tests green", paused: false, lastActivityAt: "" };
const now = 1_000_000;
const snap = (over: Partial<SupervisorSnapshot> = {}): SupervisorSnapshot => ({
  hub: { available: true, running: true }, lanes: [lane], watched: [], actions: [], ...over,
});
const act = (code: string, atMs: number, sessionId = "s1") => ({ deviceId: "owner", op: "send", sessionId, requestId: null, code, atMs, text: "run tests" });

describe("paneSupervision", () => {
  it("shows nothing when the hub is off or the pane is not supervised", () => {
    expect(paneSupervision(EMPTY_SNAPSHOT, "s1", now)).toBeNull();
    expect(paneSupervision(snap(), "other", now)).toBeNull();
    const stopped = snap({ hub: { available: true, running: false } });
    expect(paneSupervision(stopped, "s1", now)).toBeNull();
    expect(paneSupervision({ ...stopped, lanes: [{ ...lane, paused: true }] }, "s1", now)?.kind).toBe("paused");
  });
  it("orders paused > left to you > held > acting > watching", () => {
    expect(paneSupervision(snap(), "s1", now)?.kind).toBe("watching");
    expect(paneSupervision(snap({ actions: [act("OK", now - 1000)] }), "s1", now)).toMatchObject({ kind: "acting", text: "run tests" });
    expect(paneSupervision(snap({ actions: [act("OK", now - SUPERVISOR_ACTING_MS - 1)] }), "s1", now)?.kind).toBe("watching");
    expect(paneSupervision(snap({ actions: [act("CONTROL_CONFLICT", now - 500)] }), "s1", now)?.kind).toBe("held");
    expect(paneSupervision(snap({ actions: [act("OK", now)] }), "s1", now, true)?.kind).toBe("left-to-you");
    expect(paneSupervision(snap({ lanes: [{ ...lane, paused: true }], actions: [act("OK", now)] }), "s1", now, true)?.kind).toBe("paused");
  });
  it("covers watched panes and fresh input into unlisted panes", () => {
    expect(paneSupervision(snap({ lanes: [], watched: ["w"] }), "w", now)?.kind).toBe("watching");
    expect(paneSupervision(snap({ lanes: [], actions: [act("OK", now - 10, "x")] }), "x", now)?.kind).toBe("acting");
    expect(paneSupervision(snap({ lanes: [], actions: [act("OK", now - SUPERVISOR_ACTING_MS - 10, "x")] }), "x", now)).toBeNull();
  });
});

describe("timers and parsing", () => {
  it("reports acting sessions and the next expiry", () => {
    const s = snap({ actions: [act("OK", now - 1000), act("OK", now - SUPERVISOR_ACTING_MS - 5, "old")] });
    expect([...actingSessionIds(s, now)]).toEqual(["s1"]);
    expect(nextSupervisionExpiry(s, now)).toBe(now - 1000 + SUPERVISOR_ACTING_MS);
    expect(nextSupervisionExpiry(snap(), now)).toBeNull();
  });
  it("normalizes a partial or missing snapshot to the off state", () => {
    expect(normalizeSnapshot(null)).toEqual(EMPTY_SNAPSHOT);
    expect(normalizeSnapshot({ hub: { available: true } }).lanes).toEqual([]);
  });
});
