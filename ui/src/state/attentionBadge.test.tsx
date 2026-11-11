import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FleetSession } from "../lib/supervisorPanel/contract";
import type { TerminalSession, TerminalTab, Worktree } from "../lib/types";

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  dispatchNotification: vi.fn(() => Promise.resolve({ submitted: true })),
  playNotificationSound: vi.fn(() => Promise.resolve({ played: true })),
}));

const { applyDaemonSessions, backendSessionIdOfRef, resetDaemonAttentionForTest } = await import("../lib/attentionFeed");
const { groupOf } = await import("../lib/attentionGroup");
const { NotificationCoordinator } = await import("../lib/notificationCoordinator");
const { DEFAULT_NOTIFICATION_SETTINGS } = await import("../lib/notificationSettings");
const { dispatchNotification } = await import("../lib/tauri");
const { clearWorkspaceSnapshot } = await import("./workspaceSnapshotCache");
const { clearHmrWorkspaceState } = await import("./hmrWorkspaceState");
const { selectGlobalUnreadBadgeCount, useWorkspaceStore } = await import("./workspaceStore");
type WorkspaceState = import("./workspaceStore").WorkspaceState;
type Group = "needsYou" | "unread" | "working" | "seen";

const worktree: Worktree = {
  path: "/repo/main", head: "abc", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null,
};

function session(id: string, backendSessionId: string): TerminalSession {
  return { id, cwd: worktree.path, workspaceId: "ws", worktree: { wsId: "ws", slug: "main" }, backendSessionId, lifecycle: "working" };
}

/** Four panes, one per backend session b1..b4, none with any local activity. */
function fourPanes(): WorkspaceState {
  const tabs: TerminalTab[] = ["1", "2", "3", "4"].map((n) => ({ id: `tab-${n}`, label: n, sessionId: `s${n}` }));
  return {
    workspaceId: "ws",
    worktrees: [worktree],
    activeWorktreePath: worktree.path,
    sessions: Object.fromEntries(["1", "2", "3", "4"].map((n) => [`s${n}`, session(`s${n}`, `b${n}`)])),
    layout: {
      tabs,
      activeTabId: "tab-1",
      layoutsByTabId: Object.fromEntries(tabs.map((tab) => [tab.id, {
        root: { type: "leaf", leafId: `leaf-${tab.id}` }, activeLeafId: `leaf-${tab.id}`, expandedLeafId: null,
        sessionIdsByLeafId: { [`leaf-${tab.id}`]: tab.sessionId },
      }])),
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  } as unknown as WorkspaceState;
}

function fleet(backend: string, group: Group, updatedAtMs: number, reason: string | null = null): FleetSession {
  return {
    sessionRef: `local:${backend}`, hostId: "local", hostLabel: "This computer", title: backend, cwd: "/repo/main",
    agent: "claude", state: group === "working" ? "working" : group === "needsYou" ? "blocked" : "idle",
    stateSource: "provider", stateSinceMs: 1, screenChangedAtMs: 1, pendingPrompt: null, lastAssistantLine: null,
    updatedAtMs, stale: false, reason,
    attention: {
      axes: { demand: "none", activity: "stopped", completion: "none", read: "unread", ownership: "owner" },
      group,
    },
  };
}

beforeEach(() => {
  resetDaemonAttentionForTest();
  clearWorkspaceSnapshot();
  clearHmrWorkspaceState();
  vi.mocked(dispatchNotification).mockClear();
});
afterEach(() => { resetDaemonAttentionForTest(); clearWorkspaceSnapshot(); });

describe("groupOf", () => {
  it("reads the daemon group and maps only an older peer's state", () => {
    expect(groupOf({ attention: { group: "seen" }, state: "waiting", seen: false })).toBe("seen");
    expect(groupOf({ state: "blocked" })).toBe("needsYou");
    expect(groupOf({ state: "waiting", seen: false })).toBe("needsYou");
    expect(groupOf({ state: "done", seen: false })).toBe("unread");
    expect(groupOf({ state: "done", seen: true })).toBe("seen");
    expect(groupOf({ state: "working" })).toBe("working");
    expect(groupOf({})).toBe("seen");
  });

  it("takes the backend id from a plain and a relay session ref", () => {
    expect(backendSessionIdOfRef("local:b1")).toBe("b1");
    expect(backendSessionIdOfRef("https://relay.example/host/m1:b2")).toBe("b2");
    expect(backendSessionIdOfRef("nohost")).toBeNull();
  });
});

describe("badge count from the attention group", () => {
  it("counts needsYou and unread sessions and never a working or seen one", () => {
    applyDaemonSessions([fleet("b1", "needsYou", 1), fleet("b2", "unread", 1), fleet("b3", "working", 1), fleet("b4", "seen", 1)]);
    expect(selectGlobalUnreadBadgeCount(fourPanes())).toBe(2);
  });

  it("a working session never counts, even when its local state says done and unseen", () => {
    const state = fourPanes();
    state.activityBySessionId = { s3: { state: "done", seen: false, title: "x", isAgent: true } };
    expect(selectGlobalUnreadBadgeCount(state)).toBe(1);
    applyDaemonSessions([fleet("b3", "working", 1)]);
    expect(selectGlobalUnreadBadgeCount(state)).toBe(0);
  });

  it("acknowledging an open question drops the badge while the prompt stays; a new prompt restores it", () => {
    const state = fourPanes();
    // The local pane still shows the open question, unseen.
    state.activityBySessionId = { s1: { state: "waiting", seen: false, title: "x", isAgent: true } };
    applyDaemonSessions([fleet("b1", "needsYou", 1, "Run the migration?")]);
    expect(selectGlobalUnreadBadgeCount(state)).toBe(1);
    applyDaemonSessions([fleet("b1", "seen", 2, "Run the migration?")]);
    expect(selectGlobalUnreadBadgeCount(state)).toBe(0);
    applyDaemonSessions([fleet("b1", "needsYou", 3, "Deploy now?")]);
    expect(selectGlobalUnreadBadgeCount(state)).toBe(1);
  });

  it("an older frame never replaces a newer group", () => {
    applyDaemonSessions([fleet("b1", "seen", 5)]);
    applyDaemonSessions([fleet("b1", "needsYou", 4)]);
    expect(selectGlobalUnreadBadgeCount(fourPanes())).toBe(0);
  });

  it("the store's badge follows a group change with no local activity change", () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws", initialWorktrees: [worktree] }));
    act(() => result.current.restoreWorkspace(fourPanes()));
    expect(result.current.unreadBadgeCount).toBe(0);
    act(() => { applyDaemonSessions([fleet("b2", "unread", 1)]); });
    expect(result.current.unreadBadgeCount).toBe(1);
    act(() => { applyDaemonSessions([fleet("b2", "seen", 2)]); });
    expect(result.current.unreadBadgeCount).toBe(0);
  });
});

describe("desktop notifications from the attention group", () => {
  function harness() {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws", initialWorktrees: [worktree] }));
    act(() => result.current.restoreWorkspace(fourPanes()));
    const coordinator = new NotificationCoordinator({
      isWindowFocused: () => false,
      isNotificationLeader: () => true,
      getSettings: () => ({ ...DEFAULT_NOTIFICATION_SETTINGS, enabled: true, agentTaskComplete: true }),
    });
    const decisions: boolean[] = [];
    result.current.subscribeActivityNotification((event) => {
      decisions.push(coordinator.handleAgentStateChange({ ...event, nextState: event.state }).accepted);
    });
    return { result, decisions };
  }

  it("a_reason_change_inside_needs_you_does_not_renotify", () => {
    const { decisions } = harness();
    act(() => { applyDaemonSessions([fleet("b1", "needsYou", 1, "Run the migration?")]); });
    act(() => { applyDaemonSessions([fleet("b1", "needsYou", 2, "Run the migration on prod?")]); });
    act(() => { applyDaemonSessions([fleet("b1", "needsYou", 3, "Still waiting")]); });
    expect(decisions).toEqual([true]);
    expect(dispatchNotification).toHaveBeenCalledTimes(1);
  });

  it("fires once on entering needsYou or unread, never on working or seen, and again on re-entry", () => {
    const { decisions } = harness();
    act(() => { applyDaemonSessions([fleet("b2", "working", 1)]); });
    act(() => { applyDaemonSessions([fleet("b2", "unread", 2)]); });
    act(() => { applyDaemonSessions([fleet("b2", "unread", 3, "done: tests pass")]); });
    act(() => { applyDaemonSessions([fleet("b2", "seen", 4)]); });
    act(() => { applyDaemonSessions([fleet("b2", "needsYou", 5)]); });
    expect(decisions).toEqual([false, true, false, true]);
    expect(dispatchNotification).toHaveBeenCalledTimes(2);
  });

  it("the snapshot load is state, not news: nothing already waiting notifies", () => {
    const { decisions } = harness();
    act(() => { applyDaemonSessions([fleet("b1", "needsYou", 1), fleet("b2", "unread", 1)], true); });
    expect(decisions).toEqual([]);
    expect(dispatchNotification).not.toHaveBeenCalled();
  });

  it("a local edge on a daemon-judged session never notifies a second time", () => {
    const { result, decisions } = harness();
    act(() => { applyDaemonSessions([fleet("b1", "needsYou", 1)]); });
    act(() => result.current.dispatchWorkspaceAction({
      type: "SESSION_SCREEN_ACTIVITY", sessionId: "s1", tabId: "tab-1", state: "blocked", ruleId: "approval",
    } as never));
    expect(decisions).toEqual([true, false]);
    expect(dispatchNotification).toHaveBeenCalledTimes(1);
  });
});
