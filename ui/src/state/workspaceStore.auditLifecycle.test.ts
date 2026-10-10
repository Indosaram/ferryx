import { describe, expect, it } from "vitest";

import { workspaceReducer, type WorkspaceState } from "./workspaceStore";
import { createLayoutState } from "./layout";
import { serializeWorkspaceState, deserializeWorkspaceState } from "../lib/sessionPersistence";
import type { TerminalSession, TerminalTab } from "../lib/types";

const createLeafNode = (leafId: string) => ({ type: "leaf" as const, leafId });

function createInitialState(session: TerminalSession): WorkspaceState {
  const tab: TerminalTab = {
    id: "tab-1",
    label: "Test Tab",
    sessionId: session.id,
  };
  return {
    workspaceId: session.workspaceId,
    worktrees: [],
    activeWorktreePath: "/repo",
    layout: createLayoutState([tab], "tab-1"),
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    sessions: {
      [session.id]: session,
    },
    activityBySessionId: {},
  };
}

describe("workspaceStore audit lifecycle regression tests", () => {
  describe("ui-local-lifecycle-2: REBIND_SESSION_BACKEND local remoteConnectionState reset", () => {
    it("resets remoteConnectionState, remoteFailure, and remoteReplayGap on local sessions", () => {
      const session: TerminalSession = {
        id: "term-local",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: null,
        backendSessionId: null,
        remoteConnectionState: "disconnected",
        remoteFailure: { kind: "network", message: "stale failure" },
        remoteReplayGap: { requestedAfterCursor: "1", availableFromCursor: "10" },
        lifecycle: "exited",
      };

      const state = createInitialState(session);
      const next = workspaceReducer(state, {
        type: "REBIND_SESSION_BACKEND",
        sessionId: "term-local",
        backendSessionId: "new-backend-pty",
        cwd: "/repo",
      });

      expect(next.sessions["term-local"].remoteConnectionState).toBeUndefined();
      expect(next.sessions["term-local"].remoteFailure).toBeNull();
      expect(next.sessions["term-local"].remoteReplayGap).toBeNull();
      expect(next.sessions["term-local"].backendSessionId).toBe("new-backend-pty");
      expect(next.sessions["term-local"].lifecycle).toBe("running");
    });
  });

  describe("ui-local-lifecycle-4: SESSION_LIFECYCLE failed clears backendSessionId", () => {
    it("clears backendSessionId and sets reconnectLifecycle to idle on lifecycle: failed", () => {
      const session: TerminalSession = {
        id: "term-fail",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: null,
        backendSessionId: "backend-failed-1",
        lifecycle: "working",
        reconnectLifecycle: "binding",
      };

      const state: WorkspaceState = {
        ...createInitialState(session),
        activityBySessionId: {
          "term-fail": {
            state: "working",
            title: "Crashing process",
            isAgent: false,
          },
        },
      };

      const next = workspaceReducer(state, {
        type: "SESSION_LIFECYCLE",
        backendSessionId: "backend-failed-1",
        lifecycle: "failed",
      });

      expect(next.sessions["term-fail"].lifecycle).toBe("failed");
      expect(next.sessions["term-fail"].backendSessionId).toBeNull();
      expect(next.sessions["term-fail"].reconnectLifecycle).toBe("idle");
      expect(next.activityBySessionId?.["term-fail"]?.state).toBe("done");
    });
  });

  describe("ui-local-lifecycle-5: LOCAL_SPLIT_REMOVE prunes split leaf", () => {
    it("removes split leaf from layout and collapses layout to single leaf", () => {
      const session1: TerminalSession = {
        id: "term-primary",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: null,
        backendSessionId: "backend-1",
        lifecycle: "running",
      };
      const session2: TerminalSession = {
        id: "term-split-to-cancel",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: null,
        backendSessionId: null,
        lifecycle: "working",
        spawnIntent: {
          requestId: "req-1",
          prepared: null,
          cancelRequested: false,
          generation: 1,
          createSent: true,
        },
      };

      const baseState = createInitialState(session1);
      const splitState: WorkspaceState = {
        ...baseState,
        sessions: {
          "term-primary": session1,
          "term-split-to-cancel": session2,
        },
        layout: {
          ...baseState.layout,
          layoutsByTabId: {
            "tab-1": {
              root: {
                type: "split",
                direction: "horizontal",
                ratio: 0.5,
                first: createLeafNode("leaf-1"),
                second: createLeafNode("leaf-2"),
              },
              activeLeafId: "leaf-2",
              expandedLeafId: null,
              sessionIdsByLeafId: {
                "leaf-1": "term-primary",
                "leaf-2": "term-split-to-cancel",
              },
              contentsByLeafId: {},
            },
          },
        },
      };

      const next = workspaceReducer(splitState, {
        type: "LOCAL_SPLIT_REMOVE",
        sessionId: "term-split-to-cancel",
      });

      expect(next.sessions["term-split-to-cancel"]).toBeUndefined();
      expect(next.sessions["term-primary"]).toBeDefined();
      const tabLayout = next.layout.layoutsByTabId["tab-1"];
      expect(tabLayout.sessionIdsByLeafId["leaf-2"]).toBeUndefined();
      expect(tabLayout.root.type).toBe("leaf");
      if (tabLayout.root.type === "leaf") {
        expect(tabLayout.root.leafId).toBe("leaf-1");
      }
      expect(tabLayout.activeLeafId).toBe("leaf-1");
    });
  });

  describe("ui-local-lifecycle-7: Persistence retains primary tab session with non-terminal panes", () => {
    it("retains tab.sessionId in referenced sessions during serialization and deserialization", () => {
      const session: TerminalSession = {
        id: "term-primary-tab",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: null,
        backendSessionId: "backend-live-pty",
        lifecycle: "running",
      };

      const state: WorkspaceState = {
        ...createInitialState(session),
        layout: {
          tabs: [
            {
              id: "tab-1",
              label: "File Split Tab",
              sessionId: "term-primary-tab",
            },
          ],
          activeTabId: "tab-1",
          tabGroups: {
            "group-1": { id: "group-1", tabIds: ["tab-1"], activeTabId: "tab-1" },
          },
          tabGroupLayout: { type: "group", groupId: "group-1" },
          focusedGroupId: "group-1",
          layoutsByTabId: {
            "tab-1": {
              root: createLeafNode("leaf-f1"),
              activeLeafId: "leaf-f1",
              expandedLeafId: null,
              sessionIdsByLeafId: {},
              contentsByLeafId: {
                "leaf-f1": {
                  kind: "file",
                  path: "/repo/file.txt",
                  previewId: "prev-1",
                  backendSessionId: "",
                  line: null,
                  col: null,
                  workspaceId: "ws-local",
                },
              },
            },
          },
        },
      };

      const serialized = serializeWorkspaceState("ws-local", "/repo", state);
      const ws = serialized.workspaces["ws-local"];
      expect(ws.terminalSessions["term-primary-tab"]).toBeDefined();
      expect(ws.terminalSessions["term-primary-tab"].localSessionId).toBe("term-primary-tab");

      const deserialized = deserializeWorkspaceState("ws-local", serialized, null);
      expect(deserialized).not.toBeNull();
      expect(deserialized?.sessions["term-primary-tab"]).toBeDefined();
      expect(deserialized?.sessions["term-primary-tab"].id).toBe("term-primary-tab");
    });
  });

  describe("ui-ssh-5: REBIND_SESSION_BACKEND preserves agent when clearAgent is false", () => {
    it("preserves agentType, agentSessionId, providerSession, and activity on SSH session rebind", () => {
      const session: TerminalSession = {
        id: "term-ssh-agent",
        cwd: "/remote/repo",
        workspaceId: "ssh:remote-server-1",
        worktree: null,
        backendSessionId: "backend-ssh-old-1",
        agentType: "codex",
        agentSessionId: "agent-run-123",
        providerSession: {
          key: "conversation_id",
          id: "ext-codex-1",
        },
        lifecycle: "working",
      };

      const state: WorkspaceState = {
        ...createInitialState(session),
        activityBySessionId: {
          "term-ssh-agent": {
            state: "working",
            title: "Codex generating code",
            isAgent: true,
            agentType: "codex",
          },
        },
      };

      const next = workspaceReducer(state, {
        type: "REBIND_SESSION_BACKEND",
        sessionId: "term-ssh-agent",
        backendSessionId: "backend-ssh-new-2",
        clearAgent: false,
      });

      expect(next.sessions["term-ssh-agent"].backendSessionId).toBe("backend-ssh-new-2");
      expect(next.sessions["term-ssh-agent"].agentType).toBe("codex");
      expect(next.sessions["term-ssh-agent"].agentSessionId).toBe("agent-run-123");
      expect(next.sessions["term-ssh-agent"].providerSession).toEqual({
        key: "conversation_id",
        id: "ext-codex-1",
      });
      expect(next.activityBySessionId?.["term-ssh-agent"]?.title).toBe("Codex generating code");
      expect(next.activityBySessionId?.["term-ssh-agent"]?.state).toBe("working");
    });
  });
});
