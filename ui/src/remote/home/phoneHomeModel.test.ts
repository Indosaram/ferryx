import { describe, expect, it } from "vitest";

import { fixtureAction, fixtureInventory, fixtureLane, fixtureMachine, fixtureOption, fixtureSnapshot } from "./phoneHomeFixtures";
import { actionText, buildInboxRows, headlineSentence, laneFolder, laneStatus, optionForSession, recentActions, supervisorHeadline } from "./phoneHomeModel";

const NOW = 1_000_000;

describe("supervisorHeadline", () => {
  it("is off when no readable machine runs a hub", () => {
    const machines = [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ hub: { available: true, running: false } }) })];
    expect(supervisorHeadline(machines, NOW)).toEqual({ kind: "off" });
  });

  it("is unknown, not off, when no machine can be read", () => {
    const machines = [fixtureMachine({ machineId: "m1", state: "offline", snapshot: null })];
    expect(supervisorHeadline(machines, NOW)).toEqual({ kind: "unknown" });
    expect(headlineSentence({ kind: "unknown" })).not.toMatch(/isn't running/);
  });

  it("is checking, not unknown, while a machine's read is still loading", () => {
    const machines = [
      fixtureMachine({ machineId: "m1", state: "loading", snapshot: null }),
      fixtureMachine({ machineId: "m2", state: "offline", snapshot: null }),
    ];
    expect(supervisorHeadline(machines, NOW)).toEqual({ kind: "checking" });
    expect(headlineSentence({ kind: "checking" })).not.toMatch(/can't reach/i);
  });

  it("is checking while account discovery has not returned any machine yet", () => {
    expect(supervisorHeadline([], NOW, true)).toEqual({ kind: "checking" });
    expect(supervisorHeadline([], NOW, false)).toEqual({ kind: "unknown" });
  });

  it("is paused when every lane on every machine is paused", () => {
    const machines = [
      fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a", paused: true })] }) }),
      fixtureMachine({ machineId: "m2", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "b", paused: true })] }) }),
    ];
    expect(supervisorHeadline(machines, NOW).kind).toBe("paused");
  });

  it("is typing when a lane's session got a fresh OK input", () => {
    const lane = fixtureLane({ key: "a" });
    const snapshot = fixtureSnapshot({ lanes: [lane], actions: [fixtureAction({ atMs: NOW - 1000, sessionId: lane.sessionId })] });
    expect(supervisorHeadline([fixtureMachine({ machineId: "m1", snapshot })], NOW).kind).toBe("typing");
  });

  it("counts lanes and watched sessions across machines", () => {
    const machines = [
      fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a" })], watched: ["w1"] }) }),
      fixtureMachine({ machineId: "m2", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "b", paused: true })] }) }),
      fixtureMachine({ machineId: "m3", state: "offline", snapshot: null }),
    ];
    expect(supervisorHeadline(machines, NOW)).toEqual({ kind: "active", lanes: 2, watched: 1 });
  });
});

describe("laneStatus", () => {
  it("ranks paused over typing over needs-you over the hub status", () => {
    const lane = fixtureLane({ key: "a", status: "waiting_on_review" });
    const acting = new Set([lane.sessionId!]);
    expect(laneStatus({ ...lane, paused: true }, acting, true).kind).toBe("paused");
    expect(laneStatus(lane, acting, true).kind).toBe("typing");
    expect(laneStatus(lane, new Set(), true).kind).toBe("needs-you");
    expect(laneStatus(lane, new Set(), false)).toEqual({ kind: "status", label: "Waiting for review" });
  });

  it("never shows a raw hub status: unknown values read as Working", () => {
    expect(laneStatus(fixtureLane({ key: "a", status: "blocked_on_flux_capacitor" }), new Set(), false).label).toBe("Working");
  });

  it("takes the folder from the lane's cwd leaf", () => {
    expect(laneFolder({ cwd: "/a/b/ferryx/" })).toBe("ferryx");
    expect(laneFolder({ cwd: "" })).toBeNull();
  });
});

describe("recentActions", () => {
  it("merges machines newest first and caps the list", () => {
    const m1 = fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ actions: [fixtureAction({ atMs: 10 }), fixtureAction({ atMs: 30 })] }) });
    const m2 = fixtureMachine({ machineId: "m2", snapshot: fixtureSnapshot({ actions: [fixtureAction({ atMs: 20 })] }) });
    expect(recentActions([m1, m2]).map((a) => [a.machineId, a.atMs])).toEqual([["m1", 30], ["m2", 20], ["m1", 10]]);
    expect(recentActions([m1, m2], 2)).toHaveLength(2);
  });

  it("describes refused actions without codes or session ids", () => {
    const m1 = fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ actions: [fixtureAction({ atMs: 1, code: "RATE_LIMITED", sessionId: "abcdef123456" })] }) });
    const [action] = recentActions([m1]);
    expect(action.where).toBeNull();
    expect(actionText(action)).toBe("Not sent");
  });
});

describe("buildInboxRows", () => {
  it("lists waiting and held sessions from every machine before finished ones, newest first", () => {
    const options = [
      fixtureOption({ machineId: "m1", sessionId: "s-done", inventoryActivityState: "done" }),
      fixtureOption({ machineId: "m1", sessionId: "s-wait-old", inventoryActivityState: "waiting" }),
      fixtureOption({ machineId: "m2", sessionId: "s-wait-new", inventoryActivityState: "waiting", machineDisplayName: "Studio" }),
      fixtureOption({ machineId: "m2", sessionId: "s-held", inventoryActivityState: "waiting" }),
      fixtureOption({ machineId: "m2", sessionId: "s-working", inventoryActivityState: "working" }),
    ];
    const inventories = {
      m1: fixtureInventory("m1", [
        { sessionId: "s-done", activityObservedAt: 500 },
        { sessionId: "s-wait-old", activityObservedAt: 100, title: "Old question" },
      ]),
      m2: fixtureInventory("m2", [
        { sessionId: "s-wait-new", activityObservedAt: 300 },
        { sessionId: "s-held", activityObservedAt: 200 },
      ]),
    };
    const lanes = [{ ...fixtureLane({ key: "l", sessionId: "s-held", paused: true }), machineId: "m2" }];
    const rows = buildInboxRows({ options, inventories, lanes });
    expect(rows.map((row) => [row.machineId, row.sessionId, row.kind])).toEqual([
      ["m2", "s-wait-new", "waiting"],
      ["m2", "s-held", "held"],
      ["m1", "s-wait-old", "waiting"],
      ["m1", "s-done", "done"],
    ]);
    expect(rows[0].where).toBe("Studio · ferryx / main");
    expect(rows[2].title).toBe("Old question");
  });

  it("does not mark a session held when the paused lane belongs to another machine", () => {
    const options = [fixtureOption({ machineId: "m1", sessionId: "s", inventoryActivityState: "waiting" })];
    const lanes = [{ ...fixtureLane({ key: "l", sessionId: "s", paused: true }), machineId: "m2" }];
    expect(buildInboxRows({ options, inventories: {}, lanes })[0].kind).toBe("waiting");
  });
});

describe("optionForSession", () => {
  it("finds only an available option on the same machine", () => {
    const options = [
      fixtureOption({ machineId: "m1", sessionId: "s" }),
      fixtureOption({ machineId: "m2", sessionId: "s", unavailable: true }),
    ];
    expect(optionForSession(options, "m1", "s")?.machineId).toBe("m1");
    expect(optionForSession(options, "m2", "s")).toBeNull();
    expect(optionForSession(options, "m1", null)).toBeNull();
  });
});
