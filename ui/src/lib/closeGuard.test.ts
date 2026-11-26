import { describe, expect, it } from "vitest";

import { assessClose, reassessConfirmedClose } from "./closeGuard";
import { createLayoutState } from "../state/layout";
import type { WorkspaceState } from "../state/workspaceStore";
import type { TerminalSession, TerminalTab, TabPaneLayout, WorkspaceTab } from "./types";

function session(overrides: Partial<TerminalSession> & { id: string }): TerminalSession {
  return {
    cwd: "/repo",
    workspaceId: "ws-1",
    worktree: { wsId: "ws-1", slug: "main" },
    backendSessionId: `backend-${overrides.id}`,
    lifecycle: "working",
    ...overrides,
  } as TerminalSession;
}

function terminalTab(id: string, sessionId: string, extra: Partial<TerminalTab> = {}): TerminalTab {
  return { kind: "terminal", id, label: id, sessionId, ...extra };
}

function stateWith(
  tabs: WorkspaceTab[],
  sessions: TerminalSession[],
  options: { activeTabId?: string; layoutsByTabId?: Record<string, TabPaneLayout> } = {},
): WorkspaceState {
  const activeTabId = options.activeTabId ?? tabs[0]?.id ?? "";
  return {
    workspaceId: "ws-1",
    worktrees: [],
    activeWorktreePath: "/repo",
    sessions: Object.fromEntries(sessions.map((entry) => [entry.id, entry])),
    layout: {
      ...createLayoutState(tabs, activeTabId),
      layoutsByTabId: options.layoutsByTabId ?? createLayoutState(tabs, activeTabId).layoutsByTabId,
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

function leafLayout(sessionId: string, leafId = "leaf-1"): TabPaneLayout {
  return {
    root: { type: "leaf", leafId },
    activeLeafId: leafId,
    expandedLeafId: null,
    sessionIdsByLeafId: { [leafId]: sessionId },
    contentsByLeafId: {},
  };
}

function splitLayout(leftSessionId: string, rightSessionId: string): TabPaneLayout {
  return {
    root: {
      type: "split",
      direction: "horizontal",
      ratio: 0.5,
      first: { type: "leaf", leafId: "leaf-1" },
      second: { type: "leaf", leafId: "leaf-2" },
    },
    activeLeafId: "leaf-1",
    expandedLeafId: null,
    sessionIdsByLeafId: { "leaf-1": leftSessionId, "leaf-2": rightSessionId },
    contentsByLeafId: {},
  };
}

const noRunning = new Set<string>();
const labelForTab = (tabId: string) => `label:${tabId}`;
const context = (state: WorkspaceState, overrides: Partial<Parameters<typeof assessClose>[0]> = {}) => ({
  state,
  confirmAlways: false,
  runningSessionIds: noRunning,
  labelForTab,
  ...overrides,
});

describe("assessClose", () => {
  it("closes a single-pane tab with a live shell without confirmation by default", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1", lifecycle: "running" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    expect(assessClose(context(state), { kind: "tab", tabId: "tab-1" }).decision).toBe("close");
    expect(assessClose(context(state), { kind: "pane", tabId: "tab-1", leafId: "leaf-1" }).decision).toBe("close");
  });

  it("requires confirmation when closing a whole split tab whose shell sessions are live", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith(
      [tab],
      [session({ id: "term-1", lifecycle: "running" }), session({ id: "term-2", lifecycle: "running" })],
      { layoutsByTabId: { "tab-1": splitLayout("term-1", "term-2") } },
    );

    const result = assessClose(context(state), { kind: "tab", tabId: "tab-1" });

    expect(result.decision).toBe("confirm");
    if (result.decision !== "confirm") throw new Error("expected confirmation");
    expect(result.liveSessions.map((entry) => entry.id)).toEqual(["term-1", "term-2"]);
    expect(result.runningAgentCount).toBe(0);
    expect(result.label).toBe("label:tab-1");
    expect(result.tabCount).toBe(1);
  });

  it("closes without a prompt once the tab's PTY already exited", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1", lifecycle: "exited" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    expect(assessClose(context(state), { kind: "tab", tabId: "tab-1" }).decision).toBe("close");
  });

  it("closes without a prompt when the session only has a standby placeholder backend", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1", backendSessionId: "standby:term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    expect(assessClose(context(state), { kind: "tab", tabId: "tab-1" }).decision).toBe("close");
  });

  it("prompts for every close when the user enabled always-confirm", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1", lifecycle: "exited" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    const result = assessClose(context(state, { confirmAlways: true }), { kind: "tab", tabId: "tab-1" });

    expect(result.decision).toBe("confirm");
    if (result.decision !== "confirm") throw new Error("expected confirmation");
    expect(result.liveSessions).toEqual([]);
  });

  it("closes a single-pane tab with a running agent without confirmation when confirmAlways is disabled", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    expect(
      assessClose(context(state, { runningSessionIds: new Set(["term-1"]) }), { kind: "tab", tabId: "tab-1" }).decision,
    ).toBe("close");
    expect(
      assessClose(
        context(state, { runningSessionIds: new Set(["term-1"]) }),
        { kind: "pane", tabId: "tab-1", leafId: "leaf-1" },
      ).decision,
    ).toBe("close");
  });

  it("still confirms closing a whole split tab for a running agent whose session has no live backend id", () => {
    // A session record can lose its backend id (or never carry one in older state) while the
    // activity map still says the agent is working. Closing a multi-pane tab must not go silent.
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith(
      [tab],
      [
        { cwd: "/repo", workspaceId: "ws-1", worktree: null, backendSessionId: null, lifecycle: "working", id: "term-1" } as TerminalSession,
        session({ id: "term-2", lifecycle: "exited" }),
      ],
      { layoutsByTabId: { "tab-1": splitLayout("term-1", "term-2") } },
    );

    const result = assessClose(
      context(state, { runningSessionIds: new Set(["term-1"]) }),
      { kind: "tab", tabId: "tab-1" },
    );

    expect(result.decision).toBe("confirm");
    if (result.decision !== "confirm") throw new Error("expected confirmation");
    expect(result.runningAgentCount).toBe(1);
    expect(result.liveSessions).toEqual([]);
  });

  it("counts running agents from the activity map when confirmation is required", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    const result = assessClose(
      context(state, { confirmAlways: true, runningSessionIds: new Set(["term-1"]) }),
      { kind: "tab", tabId: "tab-1" },
    );

    expect(result.decision).toBe("confirm");
    if (result.decision !== "confirm") throw new Error("expected confirmation");
    expect(result.runningAgentCount).toBe(1);
  });

  it("does not prompt for a pinned tab, which the store refuses to close", () => {
    const tab = terminalTab("tab-1", "term-1", { pinned: true });
    const state = stateWith([tab], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    expect(assessClose(context(state), { kind: "tab", tabId: "tab-1" }).decision).toBe("refuse");
  });

  it("still dispatches a pane close for a pinned tab, and does not prompt a dead end", () => {
    const tab = terminalTab("tab-1", "term-1", { pinned: true });
    const state = stateWith([tab], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    // closePane on a single-leaf tab delegates to closeTab, which returns early for pinned tabs.
    // The pane is still handed to the store so the previous "always dispatch a pane close"
    // contract holds; the store is what refuses it, silently.
    expect(assessClose(context(state), { kind: "pane", tabId: "tab-1", leafId: "leaf-1" }).decision).toBe("close");
  });

  it("closes one pane of a pinned split tab without a prompt, matching the pre-existing rule", () => {
    const tab = terminalTab("tab-1", "term-1", { pinned: true });
    const state = stateWith(
      [tab],
      [session({ id: "term-1" }), session({ id: "term-2" })],
      { layoutsByTabId: { "tab-1": splitLayout("term-1", "term-2") } },
    );

    // Only a single-leaf tab's pane close was ever gated; a split pane always closed directly.
    expect(assessClose(context(state), { kind: "pane", tabId: "tab-1", leafId: "leaf-2" }).decision).toBe("close");
  });

  it("does not gate a split leaf on its own session", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith(
      [tab],
      [session({ id: "term-1" }), session({ id: "term-2" })],
      { layoutsByTabId: { "tab-1": splitLayout("term-1", "term-2") } },
    );

    const result = assessClose(context(state), { kind: "pane", tabId: "tab-1", leafId: "leaf-2" });

    // A split pane close is unconditional: the sibling keeps running and nothing is prompted.
    expect(result.decision).toBe("close");
  });

  it("assesses a batch as the union of every tab it would close", () => {
    const shared = session({ id: "shared" });
    const first = terminalTab("tab-1", "shared");
    const second = terminalTab("tab-2", "term-2");
    const third = terminalTab("tab-3", "term-3");
    const state = stateWith(
      [first, second, third],
      [shared, session({ id: "term-2" }), session({ id: "term-3" })],
      {
        layoutsByTabId: {
          "tab-1": leafLayout("shared"),
          "tab-2": leafLayout("term-2"),
          "tab-3": leafLayout("term-3"),
        },
      },
    );

    const result = assessClose(context(state), { kind: "batch", mode: "others", tabId: "tab-1" });

    expect(result.decision).toBe("confirm");
    if (result.decision !== "confirm") throw new Error("expected confirmation");
    // tab-2 and tab-3 close; the shared session survives because tab-1 keeps it.
    expect(result.liveSessions.map((entry) => entry.id).sort()).toEqual(["term-2", "term-3"]);
    expect(result.tabCount).toBe(2);
  });

  it("assesses a right-batch from the tab's own group order", () => {
    const tabs = [terminalTab("tab-1", "term-1"), terminalTab("tab-2", "term-2"), terminalTab("tab-3", "term-3")];
    const state = stateWith(tabs, [session({ id: "term-1" }), session({ id: "term-2" }), session({ id: "term-3" })], {
      layoutsByTabId: {
        "tab-1": leafLayout("term-1"),
        "tab-2": leafLayout("term-2"),
        "tab-3": leafLayout("term-3"),
      },
    });

    const result = assessClose(context(state), { kind: "batch", mode: "right", tabId: "tab-2" });

    expect(result.decision).toBe("confirm");
    if (result.decision !== "confirm") throw new Error("expected confirmation");
    expect(result.liveSessions.map((entry) => entry.id)).toEqual(["term-3"]);
    expect(result.tabCount).toBe(1);
  });

  it("skips the prompt for a batch that closes nothing", () => {
    const state = stateWith([terminalTab("tab-1", "term-1")], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    expect(assessClose(context(state), { kind: "batch", mode: "others", tabId: "tab-1" }).decision).toBe("close");
  });

  it("deduplicates sessions that several tabs in the batch share", () => {
    const shared = session({ id: "shared" });
    const first = terminalTab("tab-1", "term-1");
    const second = terminalTab("tab-2", "shared");
    const third = terminalTab("tab-3", "shared");
    const state = stateWith(
      [first, second, third],
      [session({ id: "term-1" }), shared],
      {
        layoutsByTabId: {
          "tab-1": leafLayout("term-1"),
          "tab-2": leafLayout("shared"),
          "tab-3": leafLayout("shared"),
        },
      },
    );

    const result = assessClose(context(state), { kind: "batch", mode: "others", tabId: "tab-1" });

    expect(result.decision).toBe("confirm");
    if (result.decision !== "confirm") throw new Error("expected confirmation");
    // A session shared only by tabs inside the batch dies with the batch, so it counts once.
    expect(result.liveSessions.map((entry) => entry.id)).toEqual(["shared"]);
  });
});

describe("reassessConfirmedClose", () => {
  it("returns null once the tab is gone", () => {
    const state = stateWith([terminalTab("tab-1", "term-1")], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    expect(reassessConfirmedClose(context(state), { kind: "tab", tabId: "tab-gone" })).toBeNull();
  });

  it("returns the sessions still live at confirm time", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    const live = reassessConfirmedClose(context(state), { kind: "tab", tabId: "tab-1" });

    expect(live?.map((entry) => entry.id)).toEqual(["term-1"]);
  });

  it("treats a batch whose tabs have all disappeared as an empty no-op", () => {
    const state = stateWith([terminalTab("tab-1", "term-1")], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": leafLayout("term-1") },
    });

    // Nothing to close is not a stale target: the store's batch close is a no-op too.
    expect(reassessConfirmedClose(context(state), { kind: "batch", mode: "others", tabId: "tab-1" })).toEqual([]);
  });

  it("returns null for a pane whose leaf no longer exists", () => {
    const tab = terminalTab("tab-1", "term-1");
    const state = stateWith([tab], [session({ id: "term-1" })], {
      layoutsByTabId: { "tab-1": splitLayout("term-1", "term-2") },
    });

    expect(reassessConfirmedClose(context(state), { kind: "pane", tabId: "tab-1", leafId: "leaf-gone" })).toBeNull();
  });
});
