import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  MAX_AUTO_RESUME_CANDIDATES,
  clearPendingAutoResumes,
  collectAutoResumeCandidates,
  resetAgentAutoResumeGuard,
  scheduleAgentAutoResume,
} from "./agentAutoResume";
import type { WorkspaceState } from "../state/workspaceStore";
import type { TerminalSession } from "./types";

function createMockState(overrides: Partial<WorkspaceState> = {}): WorkspaceState {
  return {
    workspaceId: "test-workspace",
    worktrees: [{ path: "/repo/main", branch: "main", head: "123", bare: false, detached: false, locked: null, prunable: null }],
    activeWorktreePath: "/repo/main",
    layout: {
      activeTabId: "tab-1",
      tabs: [
        { id: "tab-1", label: "Agent 1", sessionId: "sess-agent-1" },
        { id: "tab-2", label: "Agent 2", sessionId: "sess-agent-2" },
      ],
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-1" },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "sess-agent-1" },
        },
        "tab-2": {
          root: { type: "leaf", leafId: "leaf-2" },
          activeLeafId: "leaf-2",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-2": "sess-agent-2" },
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
    resetAgentAutoResumeGuard();
    clearPendingAutoResumes();
  });

  afterEach(() => {
    clearPendingAutoResumes();
    vi.useRealTimers();
  });

  describe("collectAutoResumeCandidates", () => {
    it("collects exited agent sessions and ignores live agent sessions", () => {
      const state = createMockState({
        sessions: {
          "sess-exited": {
            id: "sess-exited",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-exited" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-live": {
            id: "sess-live",
            backendSessionId: "live-backend-id",
            lifecycle: "running",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-live" },
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const candidates = collectAutoResumeCandidates(state);
      expect(candidates).toEqual(["sess-exited"]);
    });

    it("does NOT auto-resume exited plain shell sessions", () => {
      const state = createMockState({
        sessions: {
          "sess-shell": {
            id: "sess-shell",
            backendSessionId: null,
            lifecycle: "exited",
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-empty-agent": {
            id: "sess-empty-agent",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "   ",
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const candidates = collectAutoResumeCandidates(state);
      expect(candidates).toEqual([]);
    });

    it("skips agent sessions with unsupported agent types or missing references", () => {
      const state = createMockState({
        sessions: {
          "sess-unsupported": {
            id: "sess-unsupported",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "unsupported_nonexistent_agent",
            providerSession: { key: "session_id", id: "uuid-1" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-missing-ref": {
            id: "sess-missing-ref",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const candidates = collectAutoResumeCandidates(state);
      expect(candidates).toEqual([]);
    });

    it("prioritizes active tab session first, then other tabs in order, then remaining candidates", () => {
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
          "sess-1": {
            id: "sess-1",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-1" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-2": {
            id: "sess-2",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-2" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-3": {
            id: "sess-3",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-3" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-orphan": {
            id: "sess-orphan",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-orphan" },
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const candidates = collectAutoResumeCandidates(state);
      // Active tab (tab-2 -> sess-2) first, then tab-1 -> sess-1, then tab-3 -> sess-3, then sess-orphan
      expect(candidates).toEqual(["sess-2", "sess-1", "sess-3", "sess-orphan"]);
    });

    it("caps candidates at MAX_AUTO_RESUME_CANDIDATES (8)", () => {
      const sessions: Record<string, TerminalSession> = {};
      for (let i = 1; i <= 12; i++) {
        sessions[`sess-${i}`] = {
          id: `sess-${i}`,
          backendSessionId: null,
          lifecycle: "exited",
          agentType: "claude",
          providerSession: { key: "session_id", id: `uuid-${i}` },
          cwd: "/repo/main",
        } as TerminalSession;
      }

      const state = createMockState({
        layout: {
          activeTabId: "tab-sess-1",
          tabs: Object.keys(sessions).map((id) => ({ id: `tab-${id}`, label: id, sessionId: id })),
          layoutsByTabId: {},
        },
        sessions,
      });

      const candidates = collectAutoResumeCandidates(state);
      expect(candidates).toHaveLength(MAX_AUTO_RESUME_CANDIDATES);
      expect(candidates).toHaveLength(8);
    });
  });

  describe("scheduleAgentAutoResume", () => {
    it("staggers resume invocations by 400ms for 2 candidates", async () => {
      const state = createMockState({
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-1" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-2": {
            id: "sess-2",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-2" },
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const reconnect = vi.fn().mockResolvedValue({ sessionId: "backend-1" });

      scheduleAgentAutoResume({
        workspaceId: "test-workspace",
        state,
        recoveredFromHmr: false,
        reconnect,
      });

      // At t = 0ms: first attempt fired
      expect(reconnect).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(0);
      expect(reconnect).toHaveBeenCalledTimes(1);
      expect(reconnect).toHaveBeenCalledWith("sess-1");

      // At t = 399ms: second attempt not yet fired
      await vi.advanceTimersByTimeAsync(399);
      expect(reconnect).toHaveBeenCalledTimes(1);

      // At t = 400ms: second attempt fired
      await vi.advanceTimersByTimeAsync(1);
      expect(reconnect).toHaveBeenCalledTimes(2);
      expect(reconnect).toHaveBeenLastCalledWith("sess-2");
    });

    it("fires batch only once when restore runs twice for the same snapshot", async () => {
      const state = createMockState({
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-1" },
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const reconnect = vi.fn().mockResolvedValue({ sessionId: "backend-1" });

      // First run
      scheduleAgentAutoResume({
        workspaceId: "test-workspace",
        state,
        recoveredFromHmr: false,
        reconnect,
      });

      // Second run with same snapshot
      scheduleAgentAutoResume({
        workspaceId: "test-workspace",
        state,
        recoveredFromHmr: false,
        reconnect,
      });

      await vi.advanceTimersByTimeAsync(1000);
      expect(reconnect).toHaveBeenCalledTimes(1);
      expect(reconnect).toHaveBeenCalledWith("sess-1");
    });

    it("does not fire when recoveredFromHmr is true", async () => {
      const state = createMockState({
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-1" },
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const reconnect = vi.fn().mockResolvedValue({ sessionId: "backend-1" });

      scheduleAgentAutoResume({
        workspaceId: "test-workspace",
        state,
        recoveredFromHmr: true,
        reconnect,
      });

      await vi.advanceTimersByTimeAsync(1000);
      expect(reconnect).not.toHaveBeenCalled();
    });

    it("degrades silently when reconnect fails without throwing or crashing", async () => {
      const state = createMockState({
        sessions: {
          "sess-fail": {
            id: "sess-fail",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-fail" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-ok": {
            id: "sess-ok",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-ok" },
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const reconnect = vi.fn().mockImplementation((sessionId: string) => {
        if (sessionId === "sess-fail") {
          return Promise.reject(new Error("Daemon spawn failed"));
        }
        return Promise.resolve({ sessionId: "backend-ok" });
      });

      scheduleAgentAutoResume({
        workspaceId: "test-workspace",
        state,
        recoveredFromHmr: false,
        reconnect,
      });

      // Candidate 1 fails silently
      await vi.advanceTimersByTimeAsync(0);
      expect(reconnect).toHaveBeenCalledWith("sess-fail");

      // Candidate 2 succeeds at 400ms
      await vi.advanceTimersByTimeAsync(400);
      expect(reconnect).toHaveBeenCalledWith("sess-ok");
      expect(reconnect).toHaveBeenCalledTimes(2);
    });

    it("cleanup function cancels pending timers", async () => {
      const state = createMockState({
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-1" },
            cwd: "/repo/main",
          } as TerminalSession,
          "sess-2": {
            id: "sess-2",
            backendSessionId: null,
            lifecycle: "exited",
            agentType: "claude",
            providerSession: { key: "session_id", id: "uuid-2" },
            cwd: "/repo/main",
          } as TerminalSession,
        },
      });

      const reconnect = vi.fn().mockResolvedValue({ sessionId: "backend-1" });

      const cancel = scheduleAgentAutoResume({
        workspaceId: "test-workspace",
        state,
        recoveredFromHmr: false,
        reconnect,
      });

      // Cancel before any timer fires
      cancel();

      await vi.advanceTimersByTimeAsync(1000);
      expect(reconnect).not.toHaveBeenCalled();
    });
  });
});
