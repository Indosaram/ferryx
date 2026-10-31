import { describe, expect, it } from "vitest";

import type { NotificationEntry } from "../../../lib/notificationCenter/types";
import {
  activityLookupFrom,
  buildAttentionRows,
  buildPromptCandidateRows,
  buildRemoteAttentionRows,
  countAttentionEntries,
  countAttentionRows,
  formatAttentionLocation,
  formatAttentionTime,
  groupAttentionRows,
  liveActivityLookup,
  type AttentionActivity,
} from "./attentionModel";

function entry(overrides: Partial<NotificationEntry> & Pick<NotificationEntry, "sessionId">): NotificationEntry {
  return {
    id: `id-${overrides.sessionId}`,
    workspaceId: "ws-1",
    labels: { agentLabel: "Claude Code", workspaceLabel: "ferryx", worktreeLabel: "main" },
    subject: "agent",
    reason: "waiting",
    firstOccurredAt: 1_000,
    lastOccurredAt: 1_000,
    updateOrder: 1,
    revision: 1,
    occurrenceCount: 1,
    read: { unread: true },
    ...overrides,
  };
}

const seen = { seen: true, seenAt: 5_000 } as const;

describe("buildPromptCandidateRows", () => {
  it("offers only unread entries whose pane reads working, as needs-you candidates", () => {
    const activity: Record<string, AttentionActivity> = {
      w: { state: "working" },
      d: { state: "done" },
      r: { state: "working" },
    };
    const entries = [
      entry({ sessionId: "w" }),
      entry({ sessionId: "d" }),
      entry({ sessionId: "r", read: seen }),
    ];
    const candidates = buildPromptCandidateRows(entries, (_, sessionId) => activity[sessionId]);
    expect(candidates.map((row) => [row.sessionId, row.state])).toEqual([["w", "needs-you"]]);
    // A working entry is still no row by itself.
    expect(buildAttentionRows(entries, (_, sessionId) => activity[sessionId]).map((row) => row.sessionId)).toEqual(["d"]);
  });
});

describe("buildAttentionRows", () => {
  it("maps waiting to needs-you and done to done", () => {
    const rows = buildAttentionRows([
      entry({ sessionId: "a", reason: "waiting" }),
      entry({ sessionId: "b", reason: "done" }),
    ]);
    expect(rows.map((row) => [row.sessionId, row.state])).toEqual([
      ["a", "needs-you"],
      ["b", "done"],
    ]);
  });

  it("drops entries the user has already seen instead of keeping them greyed out", () => {
    const rows = buildAttentionRows([
      entry({ sessionId: "a", reason: "waiting", read: seen }),
      entry({ sessionId: "b", reason: "done", read: seen }),
      entry({ sessionId: "c", reason: "done" }),
    ]);
    expect(rows.map((row) => row.sessionId)).toEqual(["c"]);
  });

  it("treats an agent's bell as a request but ignores a plain shell's bell", () => {
    const rows = buildAttentionRows([
      entry({ sessionId: "agent-bell", reason: "bell", subject: "agent" }),
      entry({ sessionId: "shell-bell", reason: "bell", subject: "terminal", labels: { terminalTitle: "zsh" } }),
      entry({ sessionId: "seen-bell", reason: "bell", subject: "agent", read: seen }),
    ]);
    expect(rows.map((row) => [row.sessionId, row.state])).toEqual([["agent-bell", "needs-you"]]);
  });

  it("shows the question only while the session is still waiting on it", () => {
    const detail = "Auth method — Which library should we use?";
    const activities: Record<string, AttentionActivity> = {
      asking: { state: "waiting", detail },
      finished: { state: "done", detail },
    };
    const rows = buildAttentionRows(
      [
        entry({ sessionId: "asking", reason: "waiting", lastOccurredAt: 2 }),
        entry({ sessionId: "finished", reason: "done", lastOccurredAt: 1 }),
      ],
      (workspaceId, sessionId) => (workspaceId === "ws-1" ? activities[sessionId] : undefined),
    );
    expect(rows.map((row) => [row.sessionId, row.text])).toEqual([
      ["asking", detail],
      ["finished", undefined],
    ]);
  });

  it("follows what the session is doing now over what was recorded", () => {
    const live: Record<string, AttentionActivity> = {
      answered: { state: "working" },
      "asked-again": { state: "waiting" },
      "finished-after-ask": { state: "done" },
      "bell-then-working": { state: "working" },
    };
    const rows = buildAttentionRows(
      [
        entry({ sessionId: "answered", reason: "waiting" }),
        entry({ sessionId: "asked-again", reason: "done" }),
        entry({ sessionId: "finished-after-ask", reason: "waiting" }),
        entry({ sessionId: "bell-then-working", reason: "bell" }),
      ],
      (_workspaceId, sessionId) => live[sessionId],
    );
    expect(Object.fromEntries(rows.map((row) => [row.sessionId, row.state]))).toEqual({
      "asked-again": "needs-you",
      "finished-after-ask": "done",
    });
  });

  it("drops a row the user already looked at in its terminal", () => {
    const rows = buildAttentionRows(
      [entry({ sessionId: "looked", reason: "done" }), entry({ sessionId: "fresh", reason: "done" })],
      (_workspaceId, sessionId) => (sessionId === "looked" ? { state: "done", seen: true } : { state: "done", seen: false }),
    );
    expect(rows.map((row) => row.sessionId)).toEqual(["fresh"]);
  });

  it("keeps an agent's new bell even though an earlier completion was already seen", () => {
    const rows = buildAttentionRows(
      [entry({ sessionId: "rang", reason: "bell" })],
      () => ({ state: "done", seen: true }),
    );
    expect(rows.map((row) => [row.sessionId, row.state])).toEqual([["rang", "done"]]);
  });

  it("looks the question up in the entry's own workspace", () => {
    const rows = buildAttentionRows(
      [entry({ sessionId: "s", workspaceId: "ws-2", reason: "waiting" })],
      (workspaceId) => (workspaceId === "ws-2" ? { state: "waiting", detail: "Deploy? — Ship to prod now?" } : undefined),
    );
    expect(rows[0].text).toBe("Deploy? — Ship to prod now?");
  });

  it("falls back to the terminal title when there is no question, without repeating the name", () => {
    const rows = buildAttentionRows([
      entry({ sessionId: "titled", reason: "done", labels: { agentLabel: "Codex", terminalTitle: "✳ Fix login bug" } }),
      entry({ sessionId: "same", reason: "done", labels: { agentLabel: "Codex", terminalTitle: "Codex" } }),
      entry({ sessionId: "untitled", reason: "done", labels: {} }),
    ]);
    const bySession = Object.fromEntries(rows.map((row) => [row.sessionId, row]));
    expect(bySession.titled).toMatchObject({ who: "Codex", text: "Fix login bug" });
    expect(bySession.same).toMatchObject({ who: "Codex", text: undefined });
    expect(bySession.untitled).toMatchObject({ who: "Terminal", text: undefined, location: undefined });
  });

  it("orders requests before completions, newest first within each", () => {
    const rows = buildAttentionRows([
      entry({ sessionId: "done-old", reason: "done", lastOccurredAt: 10 }),
      entry({ sessionId: "wait-old", reason: "waiting", lastOccurredAt: 20 }),
      entry({ sessionId: "done-new", reason: "done", lastOccurredAt: 90 }),
      entry({ sessionId: "wait-new", reason: "waiting", lastOccurredAt: 50 }),
    ]);
    expect(rows.map((row) => row.sessionId)).toEqual(["wait-new", "wait-old", "done-new", "done-old"]);
  });

  it("carries the identity the store needs to acknowledge the row", () => {
    const [row] = buildAttentionRows([entry({ sessionId: "s", id: "entry-7", revision: 4, workspaceId: "ws-9" })]);
    expect(row).toMatchObject({ id: "entry-7", revision: 4, workspaceId: "ws-9", sessionId: "s" });
  });
});

describe("counting", () => {
  const entries = [
    entry({ sessionId: "w", reason: "waiting" }),
    entry({ sessionId: "d", reason: "done" }),
    entry({ sessionId: "seen", reason: "done", read: seen }),
    entry({ sessionId: "agent-bell", reason: "bell" }),
    entry({ sessionId: "shell-bell", reason: "bell", subject: "terminal" }),
  ];

  it("counts exactly the rows the inbox shows, so every badge agrees with the list", () => {
    const rows = buildAttentionRows(entries);
    expect(countAttentionEntries(entries)).toBe(rows.length);
    expect(countAttentionRows(rows)).toEqual({ all: 3, "needs-you": 2, done: 1 });

    const moved = (_workspaceId: string, sessionId: string): AttentionActivity | undefined =>
      sessionId === "w" ? { state: "working" } : undefined;
    expect(countAttentionEntries(entries, moved)).toBe(buildAttentionRows(entries, moved).length);
    expect(countAttentionEntries(entries, moved)).toBe(2);
  });

  it("groups only non-empty states, requests first", () => {
    const groups = groupAttentionRows(buildAttentionRows(entries));
    expect(groups.map((group) => [group.state, group.rows.length])).toEqual([
      ["needs-you", 2],
      ["done", 1],
    ]);
    expect(groupAttentionRows(buildAttentionRows([entry({ sessionId: "d", reason: "done" })])).map((group) => group.state)).toEqual(["done"]);
  });
});

describe("activityLookupFrom", () => {
  it("lets the first source for a workspace win, so the mounted state beats a stale snapshot", () => {
    const lookup = activityLookupFrom([
      { workspaceId: "ws-1", activityBySessionId: { s: { state: "waiting", detail: "live" } } },
      { workspaceId: "ws-1", activityBySessionId: { s: { state: "done" } } },
      { workspaceId: "ws-2", activityBySessionId: { t: { state: "done" } } },
      null,
      { workspaceId: null, activityBySessionId: { s: { state: "working" } } },
    ]);
    expect(lookup("ws-1", "s")).toEqual({ state: "waiting", detail: "live" });
    expect(lookup("ws-2", "t")).toEqual({ state: "done" });
    expect(lookup("ws-2", "s")).toBeUndefined();
    expect(lookup("ws-3", "s")).toBeUndefined();
  });

  it("reads the mounted workspace before its parked snapshot", () => {
    const lookup = liveActivityLookup(
      { workspaceId: "ws-1", activityBySessionId: { s: { state: "waiting" } } },
      [
        ["ws-1", { activityBySessionId: { s: { state: "done", seen: true } } }],
        ["ws-2", { activityBySessionId: { t: { state: "done" } } }],
      ],
    );
    expect(lookup("ws-1", "s")).toEqual({ state: "waiting" });
    expect(lookup("ws-2", "t")).toEqual({ state: "done" });
    expect(liveActivityLookup(null, [["ws-1", { activityBySessionId: { s: { state: "done" } } }]])("ws-1", "s")).toEqual({ state: "done" });
  });
});

describe("buildRemoteAttentionRows", () => {
  it("turns waiting and finished panes into the same two states, requests first", () => {
    const rows = buildRemoteAttentionRows("ferryx", "ferryx", [
      { id: "p-done", label: "Fix login", activityState: "done", agentType: "codex", worktreeLabel: "feat-login" },
      { id: "p-busy", label: "Build", activityState: "working", agentType: "claude" },
      { id: "p-wait", label: "omo", activityState: "waiting", agentType: "omo", worktreeLabel: "main" },
      { id: "p-idle", label: "zsh" },
    ]);
    expect(rows.map((row) => [row.id, row.state, row.who, row.location, row.text])).toEqual([
      ["p-wait", "needs-you", "omo", "ferryx / main", undefined],
      ["p-done", "done", "codex", "ferryx / feat-login", "Fix login"],
    ]);
  });

  it("leaves out the pane the phone is already showing", () => {
    const rows = buildRemoteAttentionRows("ws", null, [
      { id: "shown", label: "a", activityState: "waiting" },
      { id: "other", label: "b", activityState: "done" },
    ], "shown");
    expect(rows.map((row) => row.id)).toEqual(["other"]);
  });
});

describe("formatting", () => {
  it("joins workspace and worktree once", () => {
    expect(formatAttentionLocation("ferryx", "feat-attention")).toBe("ferryx / feat-attention");
    expect(formatAttentionLocation("ferryx", "ferryx")).toBe("ferryx");
    expect(formatAttentionLocation(" ", "main")).toBe("main");
    expect(formatAttentionLocation(undefined, undefined)).toBeUndefined();
  });

  it("describes elapsed time in Korean", () => {
    const now = 10 * 24 * 60 * 60 * 1000;
    expect(formatAttentionTime(now - 30_000, now)).toBe("just now");
    expect(formatAttentionTime(now - 2 * 60_000, now)).toBe("2m ago");
    expect(formatAttentionTime(now - 3 * 60 * 60_000, now)).toBe("3h ago");
    expect(formatAttentionTime(now - 2 * 24 * 60 * 60_000, now)).toBe("2d ago");
    expect(formatAttentionTime(now + 5_000, now)).toBe("just now");
  });
});
