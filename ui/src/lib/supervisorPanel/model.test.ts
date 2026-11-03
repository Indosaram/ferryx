import { describe, expect, it } from "vitest";

import type { Escalation, FleetSession, PanelSnapshot } from "./contract";
import { panelSnapshotFixture } from "./fixtures";
import {
  applyPanelEvent,
  buildPanelView,
  formatStatusLine,
  parseMentions,
} from "./model";

const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);
const START_OF_DAY = Date.UTC(2026, 10, 2);
const MIN = 60_000;
const options = { nowMs: NOW, startOfDayMs: START_OF_DAY };

function fleetSession(overrides: Partial<FleetSession>): FleetSession {
  return {
    sessionRef: "mac:one",
    hostId: "mac",
    hostLabel: "This Mac",
    title: "One",
    cwd: "/work",
    agent: "claude",
    state: "idle",
    stateSource: "provider",
    stateSinceMs: NOW - MIN,
    screenChangedAtMs: NOW - MIN,
    pendingPrompt: null,
    lastAssistantLine: null,
    updatedAtMs: NOW,
    stale: false,
    ...overrides,
  };
}

function withSessions(sessions: FleetSession[]): PanelSnapshot {
  return {
    ...panelSnapshotFixture,
    decisions: [],
    handling: [],
    sessions,
  };
}

describe("supervisor panel view", () => {
  it("shows each session in exactly one section", () => {
    const view = buildPanelView(panelSnapshotFixture, options);
    const refs = [
      ...view.decisions.map((d) => d.sessionRef),
      ...view.handling.map((h) => h.sessionRef),
      ...view.sessions.map((s) => s.sessionRef),
    ];

    expect(refs.sort()).toEqual(
      panelSnapshotFixture.sessions.map((s) => s.sessionRef).sort(),
    );
    expect(new Set(refs).size).toBe(refs.length);
    expect(view.sessions.map((s) => s.sessionRef).sort()).toEqual([
      "mac:wtlist",
      "omarchy:notes",
    ]);
  });

  it("keeps a session single when the feed repeats it", () => {
    const dup = panelSnapshotFixture.sessions[3];
    const view = buildPanelView(
      {
        ...panelSnapshotFixture,
        sessions: [...panelSnapshotFixture.sessions, { ...dup, updatedAtMs: dup.updatedAtMs + 1 }],
      },
      options,
    );

    expect(view.sessions.filter((s) => s.sessionRef === dup.sessionRef)).toHaveLength(1);
  });

  it("lists the recommended option first and flagged", () => {
    const view = buildPanelView(panelSnapshotFixture, options);
    const relay = view.decisions.find((d) => d.sessionRef === "mac:relay");

    expect(relay?.options.map((o) => o.label)).toEqual(["Allow", "Deny"]);
    expect(relay?.options[0]).toMatchObject({ index: 0, recommended: true });

    const flipped: Escalation = {
      ...panelSnapshotFixture.decisions[1],
      recommendedIndex: 2,
    };
    const view2 = buildPanelView({ ...panelSnapshotFixture, decisions: [flipped] }, options);

    expect(view2.decisions[0].options.map((o) => [o.index, o.recommended])).toEqual([
      [2, true],
      [0, false],
      [1, false],
    ]);
  });

  it("filters an escalation without a recommendedIndex and flags it", () => {
    const broken = { ...panelSnapshotFixture.decisions[0], recommendedIndex: undefined };
    const outOfRange: Escalation = { ...panelSnapshotFixture.decisions[1], recommendedIndex: 9 };
    const view = buildPanelView(
      {
        ...panelSnapshotFixture,
        decisions: [broken as unknown as Escalation, outOfRange],
      },
      options,
    );

    expect(view.decisions).toEqual([]);
    expect(view.rejected.map((r) => r.sessionRef)).toEqual(["mac:relay", "win:roblox"]);
    expect(view.statusLine).toBe("Nothing needs you.");
  });

  it("maps unknown state to Idle with 'status unknown', never Working", () => {
    const view = buildPanelView(
      withSessions([
        fleetSession({ state: "unknown", lastAssistantLine: "Compiling..." }),
      ]),
      options,
    );

    expect(view.sessions[0]).toMatchObject({ glyph: "idle", why: "status unknown" });
    expect(view.runningCount).toBe(0);
  });

  it("builds the status line for 0, 1 and 2 decisions", () => {
    expect(formatStatusLine(0)).toBe("Nothing needs you.");
    expect(formatStatusLine(1)).toBe("1 decision for you. The rest is running.");
    expect(buildPanelView(panelSnapshotFixture, options).statusLine).toBe(
      "2 decisions for you. The rest is running.",
    );
    expect(buildPanelView({ ...panelSnapshotFixture, decisions: [] }, options).statusLine).toBe(
      "Nothing needs you.",
    );
  });

  it("derives the header counts", () => {
    const view = buildPanelView(panelSnapshotFixture, options);

    expect(view.headerLine).toBe("2 running · handled 4 things today");
  });

  it("ages stalled and blocked rows from telemetry", () => {
    const view = buildPanelView(
      withSessions([
        fleetSession({
          sessionRef: "win:stuck",
          title: "Stuck",
          state: "working",
          screenChangedAtMs: NOW - 7 * MIN,
        }),
        fleetSession({
          sessionRef: "win:fresh",
          title: "Fresh",
          state: "working",
          screenChangedAtMs: NOW - MIN,
        }),
        fleetSession({
          sessionRef: "win:screen",
          title: "Screen",
          state: "working",
          stateSource: "screen",
          screenChangedAtMs: NOW - 20 * MIN,
        }),
        fleetSession({
          sessionRef: "win:waiting",
          title: "Waiting",
          state: "blocked",
          stateSinceMs: NOW - 23 * MIN,
        }),
      ]),
      options,
    );
    const row = (ref: string) => view.sessions.find((s) => s.sessionRef === ref);

    expect(row("win:stuck")).toMatchObject({
      stalled: true,
      age: "screen unchanged for 7 min",
    });
    expect(row("win:fresh")).toMatchObject({ stalled: false, age: null });
    expect(row("win:screen")?.stalled).toBe(false);
    expect(row("win:waiting")).toMatchObject({
      glyph: "needsYou",
      age: "waiting 23 min",
      injectable: true,
    });
    expect(view.sessions[0].sessionRef).toBe("win:waiting");
  });

  it("collapses the log to the newest two with the total", () => {
    const view = buildPanelView(panelSnapshotFixture, options);

    expect(view.log.total).toBe(4);
    expect(view.log.visible.map((e) => e.id)).toEqual([4, 3]);
    expect(view.log.all.map((e) => e.id)).toEqual([4, 3, 2, 1]);
    expect(view.log.visible.every((e) => !e.live)).toBe(true);
  });

  it("marks a claim without evidence as unverified", () => {
    const entry = { ...panelSnapshotFixture.log[0], evidence: [] };
    const view = buildPanelView(
      { ...panelSnapshotFixture, log: [entry, panelSnapshotFixture.log[1]] },
      options,
    );

    expect(view.log.all.map((e) => [e.id, e.unverified])).toEqual([
      [4, true],
      [3, false],
    ]);
  });

  it("describes the rules in one sentence", () => {
    const view = buildPanelView(panelSnapshotFixture, options);

    expect(view.rulesLine).toBe(
      "Retries upstream errors on a backoff, restarts sessions silent for 5 min, resumes after usage limits reset, and asks before anything irreversible.",
    );
    expect(buildPanelView({ ...panelSnapshotFixture, rules: [] }, options).rulesLine).toBe(
      "No standing rules.",
    );
  });
});

describe("supervisor panel events", () => {
  it("removes a decision when the frame carries null", () => {
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.decision",
      sessionRef: "mac:relay",
      decision: null,
    });

    expect(next.decisions.map((d) => d.sessionRef)).toEqual(["win:roblox"]);
  });

  it("moves an action from handling to the log when it stops being live", () => {
    const live = panelSnapshotFixture.handling[0];
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.action",
      action: { ...live, live: false, summary: "Resumed Windows installer tests" },
    });

    expect(next.handling).toEqual([]);
    expect(next.log.map((e) => e.id)).toContain(live.id);
    expect(buildPanelView(next, options).sessions.map((s) => s.sessionRef)).toContain(
      "win:wintests",
    );
  });

  it("upserts a session by ref", () => {
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.session",
      session: fleetSession({ sessionRef: "mac:wtlist", state: "working" }),
    });

    expect(next.sessions.filter((s) => s.sessionRef === "mac:wtlist")).toHaveLength(1);
    expect(next.sessions.find((s) => s.sessionRef === "mac:wtlist")?.state).toBe("working");
  });
});

describe("mention parsing", () => {
  const sessions = panelSnapshotFixture.sessions;

  it("resolves a unique mention by name or prefix", () => {
    expect(parseMentions("@roblox revert it", sessions)).toEqual({
      targets: ["win:roblox"],
      ambiguous: [],
    });
    expect(parseMentions("ask @Roblox build · maho-win to stop", sessions).targets).toEqual([
      "win:roblox",
    ]);
  });

  it("reports an ambiguous mention instead of guessing", () => {
    const twins = [
      fleetSession({ sessionRef: "mac:a", title: "Build", hostLabel: "This Mac" }),
      fleetSession({ sessionRef: "win:b", title: "Build", hostLabel: "maho-win" }),
    ];

    expect(parseMentions("@build go on", twins)).toEqual({
      targets: [],
      ambiguous: [{ mention: "@build", candidates: ["mac:a", "win:b"] }],
    });
    expect(parseMentions("@Build · maho-win go on", twins).targets).toEqual(["win:b"]);
  });

  it("ignores text that names no session", () => {
    expect(parseMentions("mail me@example.com", sessions)).toEqual({
      targets: [],
      ambiguous: [],
    });
  });
});
