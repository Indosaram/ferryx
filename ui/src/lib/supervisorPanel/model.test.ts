import { describe, expect, it } from "vitest";

import type { ActionEntry, Escalation, FleetSession, PanelEvent, PanelSnapshot, SessionHealth } from "./contract";
import { panelSnapshotFixture } from "./fixtures";
import {
  applyPanelEvent,
  buildPanelView,
  formatStatusLine,
  isReplyUnverified,
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

  it("keeps a session with a decision out of Handling (Needs you wins)", () => {
    const relayRetry = actionEntry({ id: 6, sessionRef: "mac:relay", updatedAtMs: NOW - MIN });
    const view = buildPanelView(
      { ...panelSnapshotFixture, handling: [...panelSnapshotFixture.handling, relayRetry] },
      options,
    );

    expect(view.decisions.map((d) => d.sessionRef)).toContain("mac:relay");
    expect(view.handling.map((h) => h.sessionRef)).toEqual(["win:wintests"]);
    expect(view.sessions.map((s) => s.sessionRef)).not.toContain("mac:relay");
  });

  it("collapses several live incidents on one session to the most recently updated", () => {
    const older = actionEntry({ id: 5, sessionRef: "win:wintests", createdAtMs: NOW - 9 * MIN, updatedAtMs: NOW - 4 * MIN });
    const newer = actionEntry({ id: 7, sessionRef: "win:wintests", createdAtMs: NOW - 2 * MIN, updatedAtMs: NOW - 2 * MIN, summary: "Restarting" });
    const fleetWide = actionEntry({ id: 8, sessionRef: null, summary: "Proxy errors" });
    for (const handling of [[older, newer, fleetWide], [newer, fleetWide, older]]) {
      const view = buildPanelView({ ...panelSnapshotFixture, decisions: [], handling }, options);
      expect(view.handling.map((h) => [h.id, h.sessionRef])).toEqual([
        [7, "win:wintests"],
        [8, null],
      ]);
      const refs = [...view.handling.map((h) => h.sessionRef), ...view.sessions.map((s) => s.sessionRef)].filter(Boolean);
      expect(new Set(refs).size).toBe(refs.length);
    }
  });

  it("keeps raw ids out of a decision reason", () => {
    for (const decision of panelSnapshotFixture.decisions) {
      expect(decision.reason).not.toMatch(/\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{4,40}\b/);
    }
    const reason = "Broke in a4c1 (3f9e2b7c0d) for run 123e4567-e89b-12d3-a456-426614174000. Took 1200 ms, retry at 14:20.";
    const view = buildPanelView(
      { ...panelSnapshotFixture, decisions: [{ ...panelSnapshotFixture.decisions[1], reason }] },
      options,
    );

    expect(view.decisions[0].reason).toBe("Broke in \u2026 (\u2026) for run \u2026. Took 1200 ms, retry at 14:20.");
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
    // Both lanes stay blocked with a prompt: a rejected card may not become an all-clear.
    expect(view.statusLine).toBe(
      "2 lanes are waiting for you. The supervisor is still deciding.",
    );
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
    // Without cards the fixture's two blocked lanes are still work: name them, never all-clear.
    expect(buildPanelView({ ...panelSnapshotFixture, decisions: [] }, options).statusLine).toBe(
      "2 lanes are waiting for you. The supervisor is still deciding.",
    );
  });

  it("never reports an all-clear while a blocked lane has no decision card", () => {
    const view = buildPanelView(
      withSessions([fleetSession({ state: "blocked", lastAssistantLine: "Ship it?" })]),
      options,
    );

    expect(view.decisions).toEqual([]);
    expect(view.waitingCount).toBe(1);
    expect(view.statusLine).toBe("1 lane is waiting for you.");

    // A prompt the hub holds but has not escalated is a judging lane, and the row says so.
    const judging = buildPanelView(
      withSessions([
        fleetSession({
          state: "blocked",
          pendingPrompt: {
            promptId: "p-1",
            kind: "question",
            question: "Ship it?",
            command: null,
            options: ["Yes", "No"],
            screenRevision: "rev-1",
          },
        }),
      ]),
      options,
    );

    expect(judging.sessions[0]).toMatchObject({ judging: true });
    expect(judging.sessions[0]?.age).toBe("supervisor deciding · waiting 1 min");
    expect(judging.statusLine).toBe(
      "1 lane is waiting for you. The supervisor is still deciding.",
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

  it("treats a subject ref as no proof: only a delivery event verifies a claim", () => {
    const claim = (evidence: ActionEntry["evidence"]) =>
      buildPanelView({ ...panelSnapshotFixture, log: [{ ...panelSnapshotFixture.log[0], evidence }] }, options).log.all[0].unverified;
    expect(claim([{ kind: "prompt", id: "p1" }])).toBe(true);
    expect(claim([{ kind: "session", id: "mac:one" }])).toBe(true);
    expect(claim([{ kind: "answerHistory", id: "hist-1" }])).toBe(true);
    expect(claim([{ kind: "receipt", id: "r1" }])).toBe(false);
    expect(claim([{ kind: "stateTransition", id: "seq-1" }])).toBe(false);
    expect(claim([{ kind: "commit", id: "abc" }])).toBe(false);
    expect(isReplyUnverified([])).toBe(true);
    expect(isReplyUnverified([{ kind: "rule", id: "5" }])).toBe(false);
    expect(isReplyUnverified([{ kind: "receipt", id: "r1" }])).toBe(false);
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

  it("shows a fixed retry interval the owner set in plain language", () => {
    const rules = panelSnapshotFixture.rules.map((rule) =>
      rule.kind === "upstreamRetry" ? { ...rule, params: { backoffSecs: [300], thenEverySecs: 300 } } : rule,
    );
    expect(buildPanelView({ ...panelSnapshotFixture, rules }, options).rulesLine).toMatch(/^Retries upstream errors every 5 min, /);
  });
});

function actionEntry(overrides: Partial<ActionEntry>): ActionEntry {
  return {
    id: 9,
    incidentKey: "upstream:x",
    sessionRef: "mac:one",
    kind: "retried",
    summary: "Retried",
    basis: "Default rule",
    evidence: [{ kind: "receipt", id: "r1" }],
    undo: null,
    live: true,
    createdAtMs: NOW - 5 * MIN,
    updatedAtMs: NOW - 5 * MIN,
    ...overrides,
  };
}

function apply(snapshot: PanelSnapshot, ...events: PanelEvent[]): PanelSnapshot {
  return events.reduce(applyPanelEvent, snapshot);
}

const emptyPanel: PanelSnapshot = {
  decisions: [],
  handling: [],
  sessions: [],
  log: [],
  rules: [],
  paused: false,
  setupRequired: false,
};

describe("supervisor panel events", () => {
  it("removes a decision when the frame carries null", () => {
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.decision",
      sessionRef: "mac:relay",
      decision: null,
    });

    expect(next.decisions.map((d) => d.sessionRef)).toEqual(["win:roblox"]);
  });

  it("replaces a decision in place and appends a new one", () => {
    const [relay, roblox] = panelSnapshotFixture.decisions;
    const newer = { ...relay, reason: "New reason", createdAtMs: relay.createdAtMs + 1 };
    const replaced = apply(panelSnapshotFixture, {
      type: "panel.decision",
      sessionRef: relay.sessionRef,
      decision: newer,
    });
    expect(replaced.decisions.map((d) => [d.sessionRef, d.reason])).toEqual([
      ["mac:relay", "New reason"],
      ["win:roblox", roblox.reason],
    ]);

    const appended = apply(emptyPanel, {
      type: "panel.decision",
      sessionRef: relay.sessionRef,
      decision: relay,
    });
    expect(appended.decisions).toEqual([relay]);
  });

  it("ignores a decision older than the one it would replace", () => {
    const [relay] = panelSnapshotFixture.decisions;
    const stale = { ...relay, reason: "Old", createdAtMs: relay.createdAtMs - 1 };
    const next = apply(panelSnapshotFixture, {
      type: "panel.decision",
      sessionRef: relay.sessionRef,
      decision: stale,
    });
    expect(next).toBe(panelSnapshotFixture);
  });

  it("rejects a decision with a blank reason, a bad index, or another session's ref", () => {
    const [relay] = panelSnapshotFixture.decisions;
    for (const decision of [
      { ...relay, reason: "   " },
      { ...relay, recommendedIndex: 7 },
      { ...relay, sessionRef: "win:other" },
    ]) {
      expect(
        apply(emptyPanel, { type: "panel.decision", sessionRef: "mac:relay", decision }).decisions,
      ).toEqual([]);
    }
  });

  it("rejects an empty reason in the built view too", () => {
    const [relay] = panelSnapshotFixture.decisions;
    const view = buildPanelView(
      { ...panelSnapshotFixture, decisions: [{ ...relay, reason: "" }] },
      options,
    );
    expect(view.decisions).toEqual([]);
    expect(view.rejected).toEqual([{ sessionRef: "mac:relay", reason: "missing reason" }]);
  });

  it("moves an action from handling to the log when it stops being live", () => {
    const live = panelSnapshotFixture.handling[0];
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.action",
      action: {
        ...live,
        live: false,
        summary: "Resumed Windows installer tests",
        updatedAtMs: live.updatedAtMs + 1,
      },
    });

    expect(next.handling).toEqual([]);
    expect(next.log.map((e) => e.id)).toContain(live.id);
    expect(buildPanelView(next, options).sessions.map((s) => s.sessionRef)).toContain(
      "win:wintests",
    );
  });

  it("upserts a live action by id: one entry, updated in place", () => {
    const first = actionEntry({ id: 9, summary: "Tried once", updatedAtMs: NOW - 4 * MIN });
    const second = { ...first, summary: "Tried twice", updatedAtMs: NOW - 3 * MIN };
    const next = apply(
      emptyPanel,
      { type: "panel.action", action: first },
      { type: "panel.action", action: second },
    );

    expect(next.handling.map((e) => [e.id, e.summary])).toEqual([[9, "Tried twice"]]);
    expect(next.log).toEqual([]);
  });

  it("keeps handling and the log newest first, whatever the arrival order", () => {
    const a = actionEntry({ id: 1, createdAtMs: NOW - 3 * MIN, updatedAtMs: NOW - 3 * MIN });
    const b = actionEntry({ id: 2, createdAtMs: NOW - 1 * MIN, updatedAtMs: NOW - 1 * MIN });
    const c = actionEntry({ id: 3, createdAtMs: NOW - 2 * MIN, updatedAtMs: NOW - 2 * MIN });
    const live = apply(
      emptyPanel,
      ...[a, b, c].map((action): PanelEvent => ({ type: "panel.action", action })),
    );
    expect(live.handling.map((e) => e.id)).toEqual([2, 3, 1]);

    const done = apply(
      emptyPanel,
      ...[a, b, c].map((action): PanelEvent => ({ type: "panel.action", action: { ...action, live: false } })),
    );
    expect(done.log.map((e) => e.id)).toEqual([2, 3, 1]);
  });

  it("completes a live action into the log with no duplicate in either list", () => {
    const live = actionEntry({ id: 9 });
    const done = { ...live, live: false, summary: "Resumed", updatedAtMs: live.updatedAtMs + MIN };
    const next = apply(
      emptyPanel,
      { type: "panel.action", action: live },
      { type: "panel.action", action: done },
    );

    expect(next.handling).toEqual([]);
    expect(next.log.map((e) => [e.id, e.summary])).toEqual([[9, "Resumed"]]);
  });

  it("does not resurrect a completed action from a late or repeated live frame", () => {
    const live = actionEntry({ id: 9 });
    const done = { ...live, live: false, updatedAtMs: live.updatedAtMs + MIN };
    const completed = apply(emptyPanel, { type: "panel.action", action: done });

    // A late frame (older), and a repeat at the same instant: both are dropped.
    for (const late of [live, { ...live, updatedAtMs: done.updatedAtMs }]) {
      const next = apply(completed, { type: "panel.action", action: late });
      expect(next).toBe(completed);
      expect(next.handling).toEqual([]);
    }
  });

  it("reopens an action when a newer live frame says the incident came back", () => {
    const live = actionEntry({ id: 9 });
    const done = { ...live, live: false, updatedAtMs: live.updatedAtMs + MIN };
    const again = { ...live, summary: "Again", updatedAtMs: done.updatedAtMs + MIN };
    const next = apply(
      emptyPanel,
      { type: "panel.action", action: done },
      { type: "panel.action", action: again },
    );

    expect(next.handling.map((e) => e.summary)).toEqual(["Again"]);
    expect(next.log).toEqual([]);
  });

  it("ignores a stale completed frame for an action already completed later", () => {
    const early = actionEntry({ id: 9, live: false, summary: "Early", updatedAtMs: NOW - 2 * MIN });
    const late = { ...early, summary: "Late", updatedAtMs: NOW - MIN };
    const next = apply(
      emptyPanel,
      { type: "panel.action", action: late },
      { type: "panel.action", action: early },
    );
    expect(next.log.map((e) => e.summary)).toEqual(["Late"]);
  });

  it("upserts a session by ref", () => {
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.session",
      session: fleetSession({ sessionRef: "mac:wtlist", state: "working" }),
    });

    expect(next.sessions.filter((s) => s.sessionRef === "mac:wtlist")).toHaveLength(1);
    expect(next.sessions.find((s) => s.sessionRef === "mac:wtlist")?.state).toBe("working");
  });

  it("adds a row for a session the snapshot did not have", () => {
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.session",
      session: fleetSession({ sessionRef: "mac:late-arrival", state: "working" }),
    });

    expect(next.sessions).toHaveLength(panelSnapshotFixture.sessions.length + 1);
    expect(next.sessions.find((s) => s.sessionRef === "mac:late-arrival")?.state).toBe("working");
  });

  it("keeps a session's place and ignores an older report of it", () => {
    const refs = panelSnapshotFixture.sessions.map((s) => s.sessionRef);
    const target = panelSnapshotFixture.sessions[2];
    const updated = apply(panelSnapshotFixture, {
      type: "panel.session",
      session: { ...target, state: "idle", updatedAtMs: target.updatedAtMs + 1 },
    });
    expect(updated.sessions.map((s) => s.sessionRef)).toEqual(refs);
    expect(updated.sessions[2].state).toBe("idle");

    const older = apply(updated, {
      type: "panel.session",
      session: { ...target, state: "working", updatedAtMs: target.updatedAtMs - 1 },
    });
    expect(older).toBe(updated);
  });

  it("replaces the rules wholesale", () => {
    const next = apply(panelSnapshotFixture, { type: "panel.rules", rules: [] });
    expect(next.rules).toEqual([]);
    expect(buildPanelView(next, options).rulesLine).toBe("No standing rules.");
  });
});

describe("session glyphs", () => {
  const health = (kind: SessionHealth["kind"], observedAtMs: number): ReadonlyMap<string, SessionHealth> =>
    new Map([["mac:one", { sessionRef: "mac:one", kind, observedAtMs }]]);
  const idle = withSessions([fleetSession({ state: "idle", stateSinceMs: NOW - 10 * MIN })]);

  it("shows Failed only for a fresh failed health report", () => {
    const failed = health({ type: "failed", reason: "exit 1" }, NOW - MIN);
    expect(buildPanelView(idle, { ...options, health: failed }).sessions[0].glyph).toBe("failed");
    expect(buildPanelView(idle, options).sessions[0].glyph).toBe("idle");
  });

  it("drops Failed once the session changed state after the report", () => {
    const old = health({ type: "failed", reason: "exit 1" }, NOW - 20 * MIN);
    expect(buildPanelView(idle, { ...options, health: old }).sessions[0].glyph).toBe("idle");
  });

  it("does not turn a non-failed health into Failed", () => {
    const quota = health({ type: "quota", resetAtMs: null }, NOW - MIN);
    expect(buildPanelView(idle, { ...options, health: quota }).sessions[0].glyph).toBe("idle");
  });

  it("ranks Failed right after Needs you", () => {
    const view = buildPanelView(
      withSessions([
        fleetSession({ sessionRef: "mac:one", title: "A", state: "working" }),
        fleetSession({ sessionRef: "mac:two", title: "B", state: "idle", stateSinceMs: NOW - 10 * MIN }),
      ]),
      {
        ...options,
        health: new Map([["mac:two", { sessionRef: "mac:two", kind: { type: "failed", reason: "x" }, observedAtMs: NOW }]]),
      },
    );
    expect(view.sessions.map((s) => s.glyph)).toEqual(["failed", "working"]);
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
