import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { fixtureAction, fixtureGoalContract, fixtureInventory, fixtureLane, fixtureMachine, fixtureOption, fixtureSnapshot } from "./phoneHomeFixtures";
import { normalizeSetupStatus, type SetupNextAction } from "../../lib/supervisorSetupModel";
import { actionText, buildInboxRows, headlineSentence, laneFolder, laneStatus, optionForSession, recentActions, supervisorHeadline } from "./phoneHomeModel";

const NOW = 1_000_000;

const setup = (nextAction: SetupNextAction) => normalizeSetupStatus({ nextAction })!;

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

  it("never counts a row the lanes already paint as Needs you as running", () => {
    // The phone paints "Needs you" from the account inventory's `waiting` state, which is not the
    // lane's own status field. A headline that only reads lane status called this screen "Running
    // 4 lanes" while one row under it said "Stopped: tests failed twice" (measured on the harness).
    const lane = fixtureLane({ key: "auth", status: "working", sessionId: "session-auth" });
    const machines = [fixtureMachine({ machineId: "studio", snapshot: fixtureSnapshot({ lanes: [lane] }) })];
    expect(supervisorHeadline(machines, NOW, false, new Set(["studio\u0000session-auth"]))).toEqual({
      kind: "needs-you",
      lanes: 1,
    });
    // A key for another machine must not leak across the boundary.
    expect(supervisorHeadline(machines, NOW, false, new Set(["other\u0000session-auth"]))).toEqual({
      kind: "active",
      lanes: 1,
      watched: 0,
    });
  });

  it("ranks needs_setup after off and before paused", () => {
    const pausedNeedsSetup = fixtureSnapshot({ lanes: [fixtureLane({ key: "a", paused: true })], setup: setup("enable_engine") });
    expect(supervisorHeadline([fixtureMachine({ machineId: "m1", snapshot: pausedNeedsSetup })], NOW)).toEqual({ kind: "needs_setup", machine: "m1" });
    const offNeedsSetup = fixtureSnapshot({ hub: { available: true, running: false }, setup: setup("enable_engine") });
    expect(supervisorHeadline([fixtureMachine({ machineId: "m1", snapshot: offNeedsSetup })], NOW)).toEqual({ kind: "off" });
  });

  it.each<SetupNextAction>(["enable_engine", "fix_engine", "create_owner", "map_projects", "cutover", "adopt"])(
    "needs setup when a machine reports %s",
    (kind) => {
      const machines = [
        fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a" })] }) }),
        fixtureMachine({ machineId: "m2", snapshot: fixtureSnapshot({ setup: setup(kind) }) }),
      ];
      expect(supervisorHeadline(machines, NOW)).toEqual({ kind: "needs_setup", machine: "m2" });
    },
  );

  it.each<SetupNextAction>(["none", "wait_engine", "restart_required", "import_memory"])("does not need setup for %s", (kind) => {
    const machines = [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a" })], setup: setup(kind) }) })];
    expect(supervisorHeadline(machines, NOW).kind).toBe("active");
  });

  it("never needs setup for an older machine that does not report it", () => {
    const machines = [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a", paused: true })] }) })];
    expect(supervisorHeadline(machines, NOW).kind).toBe("paused");
  });

  it("names the machine in the needs_setup sentence", () => {
    expect(headlineSentence({ kind: "needs_setup", machine: "Studio Mac" })).toBe("Supervisor setup isn't finished on Studio Mac.");
  });

  it("says how many lanes are waiting on the person, after typing and before active", () => {
    const machines = [
      fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a", status: "blocked" }), fixtureLane({ key: "b", status: "working" })] }) }),
      fixtureMachine({ machineId: "m2", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "c", status: "not_done" })] }) }),
    ];
    expect(supervisorHeadline(machines, NOW)).toEqual({ kind: "needs-you", lanes: 2 });
    expect(headlineSentence({ kind: "needs-you", lanes: 1 })).toBe("1 lane is waiting on you.");
    expect(headlineSentence({ kind: "needs-you", lanes: 3 })).toBe("3 lanes are waiting on you.");
  });

  it("keeps paused over needs-you when the person paused the stopped lane", () => {
    const machines = [
      fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a", status: "blocked", paused: true })] }) }),
    ];
    expect(supervisorHeadline(machines, NOW)).toEqual({ kind: "paused", lanes: 1 });
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

  it("never shows a raw hub status: unknown values read as unknown, never Working", () => {
    expect(laneStatus(fixtureLane({ key: "a", status: "blocked_on_flux_capacitor" }), new Set(), false)).toEqual({
      kind: "status",
      label: "Status unknown",
    });
  });

  it.each([
    ["queued", { kind: "status", label: "Queued" }],
    ["working", { kind: "status", label: "Working" }],
    ["done_claimed", { kind: "status", label: "Verifying" }],
    ["verified_done", { kind: "status", label: "Done" }],
    ["closed", { kind: "status", label: "Closed" }],
    ["failed", { kind: "status", label: "Failed" }],
  ] as const)("reads the hub status %s as %o", (status, expected) => {
    expect(laneStatus(fixtureLane({ key: "a", status }), new Set(), false)).toEqual(expected);
  });

  it.each(["blocked", "not_done", "waiting_on_user"])("reads a %s lane as needing you even with no other signal", (status) => {
    expect(laneStatus(fixtureLane({ key: "a", status }), new Set(), false)).toEqual({ kind: "needs-you", label: "Needs you" });
  });

  it("reads a recorded stop reason as needing you whatever the status says", () => {
    const lane = fixtureLane({ key: "a", status: "working", goalContract: fixtureGoalContract({ stopReason: "high_stakes_user_required" }) });
    expect(laneStatus(lane, new Set(), false)).toEqual({ kind: "needs-you", label: "Needs you" });
  });

  it("reads a proposal awaiting approval as needing you", () => {
    const lane = fixtureLane({
      key: "a",
      status: "working",
      goalContract: fixtureGoalContract({ proposal: { text: "ship it", state: "awaiting_approval", gapIds: [] } }),
    });
    expect(laneStatus(lane, new Set(), false)).toEqual({ kind: "needs-you", label: "Needs you" });
  });

  it("never reads an over lane as needing you, whatever its contract still records", () => {
    for (const status of ["verified_done", "closed", "failed"]) {
      const lane = fixtureLane({ key: "a", status, goalContract: fixtureGoalContract({ stopReason: "no_further_progress" }) });
      expect(laneStatus(lane, new Set(), false).kind).toBe("status");
    }
  });

  it("keeps paused over a stopped lane the person paused themselves", () => {
    const lane = fixtureLane({ key: "a", status: "blocked", paused: true });
    expect(laneStatus(lane, new Set(), false)).toEqual({ kind: "paused", label: "Paused" });
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

  it("lists a stopped lane the inventory reports no activity for, because a stopped pane stays alive", () => {
    const options = [fixtureOption({ machineId: "m1", sessionId: "s", inventoryActivityState: null })];
    const lanes = [{ ...fixtureLane({ key: "l", sessionId: "s", status: "blocked" }), machineId: "m1" }];
    const rows = buildInboxRows({ options, inventories: {}, lanes });
    expect(rows.map((row) => [row.machineId, row.sessionId, row.kind])).toEqual([["m1", "s", "waiting"]]);
  });

  it("never lists a stopped lane the person paused, nor one stopped on another machine", () => {
    const options = [
      fixtureOption({ machineId: "m1", sessionId: "s-paused" }),
      fixtureOption({ machineId: "m1", sessionId: "s-elsewhere" }),
      fixtureOption({ machineId: "m1", sessionId: "s-done", inventoryActivityState: "done" }),
    ];
    const lanes = [
      { ...fixtureLane({ key: "a", sessionId: "s-paused", status: "blocked", paused: true }), machineId: "m1" },
      { ...fixtureLane({ key: "b", sessionId: "s-elsewhere", status: "blocked" }), machineId: "m2" },
      { ...fixtureLane({ key: "c", sessionId: "s-done", status: "blocked" }), machineId: "m1" },
    ];
    expect(buildInboxRows({ options, inventories: {}, lanes }).map((row) => [row.sessionId, row.kind])).toEqual([["s-done", "done"]]);
  });

  it("lists a session once when two account options share its machine and session", () => {
    const options = [
      fixtureOption({ machineId: "m1", sessionId: "s", inventoryActivityState: "waiting" }),
      fixtureOption({ machineId: "m1", sessionId: "s", inventoryActivityState: "waiting", worktreeLabel: "other" }),
    ];
    expect(buildInboxRows({ options, inventories: {}, lanes: [] })).toHaveLength(1);
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

/* The hub owns the status vocabulary (`LaneStatus`, serde `snake_case`). Reading it here means a
   status added on the Rust side fails this suite instead of silently reading as an unknown lane on
   the phone: that drift is what made a waiting lane render as "Working". Vitest runs with `ui/` as
   its root, so the contract sits one level up. */
const rustLaneStatus = readFileSync(resolve(process.cwd(), "../crates/ferryx-hub/src/lanes/state.rs"), "utf8");

function rustLaneStatusWireNames(): string[] {
  const body = /pub enum LaneStatus \{([\s\S]*?)\n\}/.exec(rustLaneStatus)?.[1] ?? "";
  const variants = [...body.matchAll(/^\s{4}([A-Z][A-Za-z0-9]*),$/gm)].map((match) => match[1]);
  // `#[serde(rename_all = "snake_case")]` maps `DoneClaimed` to `done_claimed`.
  return variants.map((variant) => variant.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase());
}

describe("hub lane status vocabulary", () => {
  it("reads the hub's own enum", () => {
    expect(rustLaneStatusWireNames()).toEqual([
      "queued",
      "working",
      "blocked",
      "done_claimed",
      "not_done",
      "verified_done",
      "closed",
      "failed",
    ]);
  });

  it("reads every status the hub can send as a real state, never as unknown", () => {
    const acting = new Set<string>();
    for (const status of rustLaneStatusWireNames()) {
      const reading = laneStatus(fixtureLane({ key: "a", status }), acting, false);
      expect(reading.label, `hub status "${status}" must not read as unknown`).not.toBe("Status unknown");
    }
  });

  it("only calls a lane needing you when the hub is actually holding it for the person", () => {
    const acting = new Set<string>();
    const waitingOnPerson = new Set(["blocked", "not_done"]);
    for (const status of rustLaneStatusWireNames()) {
      const reading = laneStatus(fixtureLane({ key: "a", status }), acting, false);
      const expected = waitingOnPerson.has(status) ? "Needs you" : null;
      if (expected) expect(reading.label).toBe(expected);
      else expect(reading.label).not.toBe("Needs you");
    }
  });
});
