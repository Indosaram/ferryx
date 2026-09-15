import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  MAX_AUTO_RESUME_CANDIDATES,
  clearPendingAutoResumes,
  collectAutoResumeCandidates,
  resetAgentAutoResumeGuard,
  scheduleAgentAutoResume,
} from "./agentAutoResume";
import { resetGeneralSettings, saveGeneralSettings } from "./generalSettings";
import type { WorkspaceState } from "../state/workspaceStore";
import type { TerminalSession } from "./types";

function agentSession(id: string): TerminalSession {
  return {
    id,
    backendSessionId: null,
    lifecycle: "exited",
    agentType: "claude",
    providerSession: { key: "session_id", id: `provider-${id}` },
    cwd: "/repo/main",
  } as TerminalSession;
}

function createMockState(overrides: Partial<WorkspaceState> = {}): WorkspaceState {
  return {
    workspaceId: "test-workspace",
    worktrees: [{ path: "/repo/main", branch: "main", head: "123", bare: false, detached: false, locked: null, prunable: null }],
    activeWorktreePath: "/repo/main",
    layout: {
      activeTabId: "tab-1",
      tabs: [
        { id: "tab-1", label: "Agent 1", sessionId: "sess-1" },
        { id: "tab-2", label: "Agent 2", sessionId: "sess-2" },
      ],
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-1" },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "sess-1" },
        },
        "tab-2": {
          root: { type: "leaf", leafId: "leaf-2" },
          activeLeafId: "leaf-2",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-2": "sess-2" },
        },
      },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    sessions: {},
    ...overrides,
  };
}

describe("agentAutoResume", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.clear();
    resetAgentAutoResumeGuard();
    clearPendingAutoResumes();
    saveGeneralSettings({ sessionRestorePolicy: "eager" });
  });

  afterEach(() => {
    clearPendingAutoResumes();
    resetAgentAutoResumeGuard();
    resetGeneralSettings();
    localStorage.clear();
    vi.useRealTimers();
  });

  describe("collectAutoResumeCandidates", () => {
    it("collects exited agent sessions and ignores live and plain-shell sessions", () => {
      const state = createMockState({
        sessions: {
          "sess-exited": agentSession("sess-exited"),
          "sess-live": {
            ...agentSession("sess-live"),
            backendSessionId: "live-backend-id",
            lifecycle: "working",
          } as TerminalSession,
          "sess-shell": {
            id: "sess-shell",
            backendSessionId: null,
            lifecycle: "exited",
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });
      expect(collectAutoResumeCandidates(state)).toEqual(["sess-exited"]);
    });

    it("skips unsupported agents and missing resume references", () => {
      const state = createMockState({
        sessions: {
          unsupported: {
            ...agentSession("unsupported"),
            agentType: "unsupported_nonexistent_agent",
          } as TerminalSession,
          missing: {
            id: "missing",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });
      expect(collectAutoResumeCandidates(state)).toEqual([]);
    });

    it("prioritizes the active tab, then open tabs, then remaining sessions", () => {
      const state = createMockState({
        layout: {
          activeTabId: "tab-2",
          tabs: [
            { id: "tab-1", label: "Tab 1", sessionId: "sess-1" },
            { id: "tab-2", label: "Tab 2", sessionId: "sess-2" },
            { id: "tab-3", label: "Tab 3", sessionId: "sess-3" },
          ],
          layoutsByTabId: {
            "tab-1": { root: { type: "leaf", leafId: "leaf-1" }, activeLeafId: "leaf-1", expandedLeafId: null, sessionIdsByLeafId: { "leaf-1": "sess-1" } },
            "tab-2": { root: { type: "leaf", leafId: "leaf-2" }, activeLeafId: "leaf-2", expandedLeafId: null, sessionIdsByLeafId: { "leaf-2": "sess-2" } },
            "tab-3": { root: { type: "leaf", leafId: "leaf-3" }, activeLeafId: "leaf-3", expandedLeafId: null, sessionIdsByLeafId: { "leaf-3": "sess-3" } },
          },
        },
        sessions: {
          "sess-1": agentSession("sess-1"),
          "sess-2": agentSession("sess-2"),
          "sess-3": agentSession("sess-3"),
          orphan: agentSession("orphan"),
        },
      });
      expect(collectAutoResumeCandidates(state)).toEqual(["sess-2", "sess-1", "sess-3", "orphan"]);
    });

    it("caps candidates when limit is provided, but returns all when limit is omitted", () => {
      const sessions: Record<string, TerminalSession> = {};
      for (let i = 1; i <= 12; i += 1) sessions[`sess-${i}`] = agentSession(`sess-${i}`);
      const state = createMockState({
        layout: {
          activeTabId: "tab-sess-1",
          tabs: Object.keys(sessions).map((id) => ({ id: `tab-${id}`, label: id, sessionId: id })),
          layoutsByTabId: {},
        },
        sessions,
      });
      expect(collectAutoResumeCandidates(state, undefined, MAX_AUTO_RESUME_CANDIDATES)).toHaveLength(MAX_AUTO_RESUME_CANDIDATES);
      expect(collectAutoResumeCandidates(state)).toHaveLength(12);
    });
  });

  describe("scheduleAgentAutoResume restore policies", () => {
    it("does not eagerly resume any agent under the default Lazy policy", async () => {
      saveGeneralSettings({ sessionRestorePolicy: "lazy" });
      resetAgentAutoResumeGuard();
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1"), "sess-2": agentSession("sess-2") } });
      const reconnect = vi.fn().mockResolvedValue(undefined);

      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      await vi.advanceTimersByTimeAsync(1_000);
      expect(reconnect).not.toHaveBeenCalled();
    });

    it("resumes only the previously active tab under Active Only", async () => {
      saveGeneralSettings({ sessionRestorePolicy: "activeOnly" });
      resetAgentAutoResumeGuard();
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1"), "sess-2": agentSession("sess-2") } });
      const reconnect = vi.fn().mockResolvedValue(undefined);

      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      await vi.advanceTimersByTimeAsync(1_000);
      expect(reconnect).toHaveBeenCalledTimes(1);
      expect(reconnect).toHaveBeenCalledWith("sess-1");
    });

    it("Active Only resumes only the focused group's active tab", async () => {
      saveGeneralSettings({ sessionRestorePolicy: "activeOnly" });
      resetAgentAutoResumeGuard();
      const state = createMockState({
        layout: {
          activeTabId: "tab-1",
          focusedGroupId: "group-b",
          tabs: [
            { id: "tab-1", label: "one", sessionId: "sess-1" },
            { id: "tab-2", label: "two", sessionId: "sess-2" },
          ],
          tabGroups: {
            "group-a": { id: "group-a", tabIds: ["tab-1"], activeTabId: "tab-1" },
            "group-b": { id: "group-b", tabIds: ["tab-2"], activeTabId: "tab-2" },
          },
          tabGroupLayout: {
            type: "split",
            direction: "horizontal",
            ratio: 0.5,
            first: { type: "group", groupId: "group-a" },
            second: { type: "group", groupId: "group-b" },
          },
          layoutsByTabId: {
            "tab-1": { root: { type: "leaf", leafId: "leaf-1" }, activeLeafId: "leaf-1", expandedLeafId: null, sessionIdsByLeafId: { "leaf-1": "sess-1" } },
            "tab-2": { root: { type: "leaf", leafId: "leaf-2" }, activeLeafId: "leaf-2", expandedLeafId: null, sessionIdsByLeafId: { "leaf-2": "sess-2" } },
          },
        },
        sessions: { "sess-1": agentSession("sess-1"), "sess-2": agentSession("sess-2") },
      });
      const reconnect = vi.fn().mockResolvedValue(undefined);

      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      await vi.advanceTimersByTimeAsync(1_000);
      expect(reconnect).toHaveBeenCalledTimes(1);
      expect(reconnect).toHaveBeenCalledWith("sess-2");
    });

    it("keeps legacy Eager behavior and staggers multiple resumes", async () => {
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1"), "sess-2": agentSession("sess-2") } });
      const reconnect = vi.fn().mockResolvedValue(undefined);

      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      await vi.advanceTimersByTimeAsync(0);
      expect(reconnect).toHaveBeenCalledTimes(1);
      expect(reconnect).toHaveBeenCalledWith("sess-1");
      await vi.advanceTimersByTimeAsync(399);
      expect(reconnect).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(reconnect).toHaveBeenCalledTimes(2);
      expect(reconnect).toHaveBeenLastCalledWith("sess-2");
    });

    it("fires only once for the same restore token", async () => {
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1") } });
      const reconnect = vi.fn().mockResolvedValue(undefined);
      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      await vi.advanceTimersByTimeAsync(1_000);
      expect(reconnect).toHaveBeenCalledTimes(1);
    });

    it("never auto-resumes HMR-restored sessions", async () => {
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1") } });
      const reconnect = vi.fn().mockResolvedValue(undefined);
      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: true, reconnect });
      await vi.advanceTimersByTimeAsync(1_000);
      expect(reconnect).not.toHaveBeenCalled();
    });

    it("continues the batch when one reconnect fails", async () => {
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1"), "sess-2": agentSession("sess-2") } });
      const reconnect = vi.fn().mockImplementation((sessionId: string) =>
        sessionId === "sess-1" ? Promise.reject(new Error("spawn failed")) : Promise.resolve(undefined),
      );
      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      await vi.advanceTimersByTimeAsync(400);
      expect(reconnect).toHaveBeenCalledTimes(2);
    });

    it("cleanup cancels pending timers", async () => {
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1"), "sess-2": agentSession("sess-2") } });
      const reconnect = vi.fn().mockResolvedValue(undefined);
      const cancel = scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect });
      cancel();
      await vi.advanceTimersByTimeAsync(1_000);
      expect(reconnect).not.toHaveBeenCalled();
    });

    it("schedules auto-resume when ignorePolicy is true even under Lazy policy", async () => {
      saveGeneralSettings({ sessionRestorePolicy: "lazy" });
      resetAgentAutoResumeGuard();
      const state = createMockState({ sessions: { "sess-1": agentSession("sess-1"), "sess-2": agentSession("sess-2") } });
      const reconnect = vi.fn().mockResolvedValue(undefined);

      scheduleAgentAutoResume({ workspaceId: "test-workspace", state, recoveredFromHmr: false, reconnect, ignorePolicy: true });
      await vi.advanceTimersByTimeAsync(1_000);
      expect(reconnect).toHaveBeenCalledTimes(2);
    });
  });
});