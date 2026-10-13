import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalTab, Worktree } from "../lib/types";
import { clearHmrWorkspaceState } from "./hmrWorkspaceState";
import { clearWorkspaceSnapshot, getWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

const sshWorktree: Worktree = {
  path: "/home/remote/project",
  head: "abc123",
  branch: "refs/heads/orca/ssh-host/project",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const sshNonGitWorktree: Worktree = {
  path: "/srv/standalone",
  head: "",
  branch: null,
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const localWorktree: Worktree = {
  path: "/repo/local",
  head: "def456",
  branch: "refs/heads/orca/local-ws/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function waitForStarted(started: Promise<void>, timeoutMs = 2_000): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`Timed out waiting for spawn to start (${timeoutMs}ms)`)), timeoutMs);
  });
  try {
    await Promise.race([started, timeoutPromise]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

describe("SSH spawn completion ownership", () => {
  beforeEach(() => {
    clearWorkspaceSnapshot("ssh:my-host:my-project");
    clearWorkspaceSnapshot("ssh:my-host:standalone-proj");
    clearWorkspaceSnapshot("local-ws");
    clearHmrWorkspaceState("ssh:my-host:my-project");
    clearHmrWorkspaceState("ssh:my-host:standalone-proj");
    clearHmrWorkspaceState("local-ws");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("completes split on owning workspace snapshot via spawnTerminalDetailed after switching workspaces", async () => {
    const spawnDeferred = createDeferred<{ sessionId: string; daemonEpoch?: string | null; session?: { cwd?: string | null } | null }>();
    const spawnStarted = createDeferred<void>();

    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "unexpected-fallback"),
      spawnTerminalDetailed: vi.fn(async () => {
        spawnStarted.resolve();
        return await spawnDeferred.promise;
      }),
      getTerminalCwd: vi.fn(async () => "/home/remote/project"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const sshWorkspaceId = "ssh:my-host:my-project";
    const initialTab: TerminalTab = { id: "tab-ssh-1", label: "SSH 1", sessionId: "session-ssh-1" };
    const initialSshState: WorkspaceState = {
      workspaceId: sshWorkspaceId,
      worktrees: [sshWorktree],
      activeWorktreePath: sshWorktree.path,
      sessions: {
        "session-ssh-1": {
          id: "session-ssh-1",
          cwd: sshWorktree.path,
          worktreePath: sshWorktree.path,
          workspaceId: sshWorkspaceId,
          worktree: null,
          backendSessionId: "backend-ssh-1",
          lifecycle: "working",
          remoteConnectionState: "connected",
          remoteGeneration: 1,
        },
      },
      layout: {
        tabs: [initialTab],
        activeTabId: initialTab.id,
        primaryTabId: initialTab.id,
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {
          [initialTab.id]: {
            root: { type: "leaf", leafId: "leaf-ssh-1" },
            activeLeafId: "leaf-ssh-1",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-ssh-1": "session-ssh-1" },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    setWorkspaceSnapshot(sshWorkspaceId, initialSshState);

    const hook = renderHook(
      ({ wsId, worktrees }) =>
        useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      {
        initialProps: { wsId: sshWorkspaceId, worktrees: [sshWorktree] },
      },
    );

    try {
      let splitPromise!: Promise<void>;
      act(() => {
        splitPromise = hook.result.current.splitPane("tab-ssh-1", "leaf-ssh-1", "horizontal");
      });

      await waitForStarted(spawnStarted.promise);

      const sshSessionsDuring = hook.result.current.state.sessions;
      const splitSessionId = Object.keys(sshSessionsDuring).find((id) => id !== "session-ssh-1");
      expect(splitSessionId).toBeDefined();
      expect(sshSessionsDuring[splitSessionId!].backendSessionId).toBeNull();
      expect(sshSessionsDuring[splitSessionId!].lifecycle).toBe("working");

      act(() => {
        hook.rerender({ wsId: "local-ws", worktrees: [localWorktree] });
      });

      expect(hook.result.current.state.workspaceId).toBe("local-ws");

      await act(async () => {
        spawnDeferred.resolve({
          sessionId: "backend-ssh-split-new",
          daemonEpoch: "epoch-ssh-detailed-42",
          session: { cwd: "/home/remote/project/sub" },
        });
        await splitPromise;
      });

      expect(hook.result.current.state.sessions[splitSessionId!]).toBeUndefined();

      const sshSnapshot = getWorkspaceSnapshot(sshWorkspaceId);
      expect(sshSnapshot).toBeDefined();
      expect(sshSnapshot!.sessions[splitSessionId!]).toBeDefined();
      const boundSession = sshSnapshot!.sessions[splitSessionId!];
      expect(boundSession.backendSessionId).toBe("backend-ssh-split-new");
      expect(boundSession.daemonEpoch).toBe("epoch-ssh-detailed-42");
      expect(boundSession.lifecycle).toBe("running");
      expect(boundSession.reconnectLifecycle).toBe("idle");
      expect(boundSession.remoteConnectionState).toBe("connected");
      expect(boundSession.cwd).toBe("/home/remote/project/sub");

      expect(services.closeTerminal).not.toHaveBeenCalledWith("backend-ssh-split-new");

      act(() => {
        hook.rerender({ wsId: sshWorkspaceId, worktrees: [sshWorktree] });
      });
      expect(hook.result.current.state.workspaceId).toBe(sshWorkspaceId);
      expect(hook.result.current.state.sessions[splitSessionId!].backendSessionId).toBe("backend-ssh-split-new");
      expect(hook.result.current.state.sessions[splitSessionId!].daemonEpoch).toBe("epoch-ssh-detailed-42");
      expect(hook.result.current.state.sessions[splitSessionId!].lifecycle).toBe("running");
    } finally {
      hook.unmount();
    }
  });

  it("handles split spawn failure on owning workspace snapshot after switching workspaces", async () => {
    const spawnDeferred = createDeferred<{ sessionId: string; session?: { cwd?: string | null } | null }>();
    const spawnStarted = createDeferred<void>();

    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "unexpected"),
      spawnTerminalDetailed: vi.fn(async () => {
        spawnStarted.resolve();
        return await spawnDeferred.promise;
      }),
      getTerminalCwd: vi.fn(async () => "/home/remote/project"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const sshWorkspaceId = "ssh:my-host:my-project";
    const initialTab: TerminalTab = { id: "tab-ssh-1", label: "SSH 1", sessionId: "session-ssh-1" };
    const initialSshState: WorkspaceState = {
      workspaceId: sshWorkspaceId,
      worktrees: [sshWorktree],
      activeWorktreePath: sshWorktree.path,
      sessions: {
        "session-ssh-1": {
          id: "session-ssh-1",
          cwd: sshWorktree.path,
          worktreePath: sshWorktree.path,
          workspaceId: sshWorkspaceId,
          worktree: null,
          backendSessionId: "backend-ssh-1",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [initialTab],
        activeTabId: initialTab.id,
        primaryTabId: initialTab.id,
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {
          [initialTab.id]: {
            root: { type: "leaf", leafId: "leaf-ssh-1" },
            activeLeafId: "leaf-ssh-1",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-ssh-1": "session-ssh-1" },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    setWorkspaceSnapshot(sshWorkspaceId, initialSshState);

    const hook = renderHook(
      ({ wsId, worktrees }) =>
        useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      {
        initialProps: { wsId: sshWorkspaceId, worktrees: [sshWorktree] },
      },
    );

    try {
      let splitPromise!: Promise<void>;
      act(() => {
        splitPromise = hook.result.current.splitPane("tab-ssh-1", "leaf-ssh-1", "horizontal");
      });

      await waitForStarted(spawnStarted.promise);
      const splitSessionId = Object.keys(hook.result.current.state.sessions).find((id) => id !== "session-ssh-1");

      act(() => {
        hook.rerender({ wsId: "local-ws", worktrees: [localWorktree] });
      });

      await act(async () => {
        spawnDeferred.reject(new Error("SSH connection dropped"));
        await expect(splitPromise).rejects.toThrow("SSH connection dropped");
      });

      const sshSnapshot = getWorkspaceSnapshot(sshWorkspaceId);
      expect(sshSnapshot).toBeDefined();
      expect(sshSnapshot!.sessions[splitSessionId!]).toBeDefined();
      const failedSession = sshSnapshot!.sessions[splitSessionId!];
      expect(failedSession.lifecycle).toBe("exited");
      expect(failedSession.backendSessionId).toBeNull();
      expect(failedSession.reconnectLifecycle).toBe("idle");
      expect(failedSession.remoteConnectionState).toBe("disconnected");
    } finally {
      hook.unmount();
    }
  });

  it("closes returned backend session if split pane was deliberately closed before spawn returned", async () => {
    const spawnDeferred = createDeferred<{ sessionId: string; session?: { cwd?: string | null } | null }>();
    const spawnStarted = createDeferred<void>();

    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "unexpected"),
      spawnTerminalDetailed: vi.fn(async () => {
        spawnStarted.resolve();
        return await spawnDeferred.promise;
      }),
      getTerminalCwd: vi.fn(async () => "/home/remote/project"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const sshWorkspaceId = "ssh:my-host:my-project";
    const initialTab: TerminalTab = { id: "tab-ssh-1", label: "SSH 1", sessionId: "session-ssh-1" };
    const initialSshState: WorkspaceState = {
      workspaceId: sshWorkspaceId,
      worktrees: [sshWorktree],
      activeWorktreePath: sshWorktree.path,
      sessions: {
        "session-ssh-1": {
          id: "session-ssh-1",
          cwd: sshWorktree.path,
          worktreePath: sshWorktree.path,
          workspaceId: sshWorkspaceId,
          worktree: null,
          backendSessionId: "backend-ssh-1",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [initialTab],
        activeTabId: initialTab.id,
        primaryTabId: initialTab.id,
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {
          [initialTab.id]: {
            root: { type: "leaf", leafId: "leaf-ssh-1" },
            activeLeafId: "leaf-ssh-1",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-ssh-1": "session-ssh-1" },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    setWorkspaceSnapshot(sshWorkspaceId, initialSshState);

    const hook = renderHook(
      ({ wsId, worktrees }) =>
        useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      {
        initialProps: { wsId: sshWorkspaceId, worktrees: [sshWorktree] },
      },
    );

    try {
      let splitPromise!: Promise<void>;
      act(() => {
        splitPromise = hook.result.current.splitPane("tab-ssh-1", "leaf-ssh-1", "horizontal");
      });

      await waitForStarted(spawnStarted.promise);

      const tabLayout = hook.result.current.state.layout.layoutsByTabId["tab-ssh-1"];
      const newLeafId = Object.keys(tabLayout.sessionIdsByLeafId).find((lid) => lid !== "leaf-ssh-1")!;

      await act(async () => {
        await hook.result.current.closePane("tab-ssh-1", newLeafId);
      });

      await act(async () => {
        spawnDeferred.resolve({ sessionId: "backend-ssh-orphaned", session: { cwd: "/home/remote/project" } });
        await splitPromise;
      });

      expect(services.closeTerminal).toHaveBeenCalledWith("backend-ssh-orphaned");
    } finally {
      hook.unmount();
    }
  });

  it("handles same-workspace SSH split happy path without workspace switch", async () => {
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "unexpected"),
      spawnTerminalDetailed: vi.fn(async () => ({
        sessionId: "backend-ssh-split-same",
        daemonEpoch: "epoch-ssh-same",
        session: { cwd: "/home/remote/project/same" },
      })),
      getTerminalCwd: vi.fn(async () => "/home/remote/project"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const sshWorkspaceId = "ssh:my-host:my-project";
    const initialTab: TerminalTab = { id: "tab-ssh-1", label: "SSH 1", sessionId: "session-ssh-1" };
    const initialSshState: WorkspaceState = {
      workspaceId: sshWorkspaceId,
      worktrees: [sshWorktree],
      activeWorktreePath: sshWorktree.path,
      sessions: {
        "session-ssh-1": {
          id: "session-ssh-1",
          cwd: sshWorktree.path,
          worktreePath: sshWorktree.path,
          workspaceId: sshWorkspaceId,
          worktree: null,
          backendSessionId: "backend-ssh-1",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [initialTab],
        activeTabId: initialTab.id,
        primaryTabId: initialTab.id,
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {
          [initialTab.id]: {
            root: { type: "leaf", leafId: "leaf-ssh-1" },
            activeLeafId: "leaf-ssh-1",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-ssh-1": "session-ssh-1" },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    setWorkspaceSnapshot(sshWorkspaceId, initialSshState);

    const hook = renderHook(
      ({ wsId, worktrees }) =>
        useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      {
        initialProps: { wsId: sshWorkspaceId, worktrees: [sshWorktree] },
      },
    );

    try {
      await act(async () => {
        await hook.result.current.splitPane("tab-ssh-1", "leaf-ssh-1", "horizontal");
      });

      const splitSessionId = Object.keys(hook.result.current.state.sessions).find((id) => id !== "session-ssh-1");
      expect(splitSessionId).toBeDefined();
      expect(hook.result.current.state.sessions[splitSessionId!].backendSessionId).toBe("backend-ssh-split-same");
      expect(hook.result.current.state.sessions[splitSessionId!].daemonEpoch).toBe("epoch-ssh-same");
      expect(hook.result.current.state.sessions[splitSessionId!].lifecycle).toBe("running");
      expect(hook.result.current.state.sessions[splitSessionId!].cwd).toBe("/home/remote/project/same");
    } finally {
      hook.unmount();
    }
  });

  it("handles non-git initial SSH openTab in same workspace without switch", async () => {
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "backend-ssh-nongit-same"),
      getTerminalCwd: vi.fn(async () => "/srv/standalone"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const sshWorkspaceId = "ssh:my-host:standalone-proj";
    const hook = renderHook(
      ({ wsId, worktrees }) =>
        useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      {
        initialProps: { wsId: sshWorkspaceId, worktrees: [sshNonGitWorktree] },
      },
    );

    try {
      let tabId!: string | null;
      await act(async () => {
        tabId = await hook.result.current.openTab(sshNonGitWorktree);
      });

      expect(tabId).toBeDefined();
      expect(hook.result.current.state.layout.tabs.some((t) => t.id === tabId)).toBe(true);
      const openedTab = hook.result.current.state.layout.tabs.find((t) => t.id === tabId);
      expect(openedTab).toBeDefined();
      if (openedTab && "sessionId" in openedTab) {
        expect(hook.result.current.state.sessions[openedTab.sessionId].backendSessionId).toBe("backend-ssh-nongit-same");
        expect(hook.result.current.state.sessions[openedTab.sessionId].worktreePath).toBe("/srv/standalone");
      }
    } finally {
      hook.unmount();
    }
  });

  it("completes non-git initial SSH openTab on origin snapshot when workspace switches while spawn is in flight", async () => {
    const spawnDeferred = createDeferred<string>();
    const spawnStarted = createDeferred<void>();

    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => {
        spawnStarted.resolve();
        return await spawnDeferred.promise;
      }),
      getTerminalCwd: vi.fn(async () => "/srv/standalone"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const sshWorkspaceId = "ssh:my-host:standalone-proj";
    const initialSnapshot: WorkspaceState = {
      workspaceId: sshWorkspaceId,
      worktrees: [sshNonGitWorktree],
      activeWorktreePath: sshNonGitWorktree.path,
      sessions: {},
      layout: {
        tabs: [],
        activeTabId: null,
        primaryTabId: null,
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {},
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };
    setWorkspaceSnapshot(sshWorkspaceId, initialSnapshot);

    const hook = renderHook(
      ({ wsId, worktrees }) =>
        useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      {
        initialProps: { wsId: sshWorkspaceId, worktrees: [sshNonGitWorktree] },
      },
    );

    try {
      let openPromise!: Promise<string | null>;
      act(() => {
        openPromise = hook.result.current.openTab(sshNonGitWorktree);
      });

      await waitForStarted(spawnStarted.promise);

      act(() => {
        hook.rerender({ wsId: "local-ws", worktrees: [localWorktree] });
      });
      expect(hook.result.current.state.workspaceId).toBe("local-ws");

      let tabIdResult: string | null = null;
      await act(async () => {
        spawnDeferred.resolve("backend-ssh-nongit-switched");
        tabIdResult = await openPromise;
      });

      expect(tabIdResult).toBeDefined();
      expect(services.closeTerminal).not.toHaveBeenCalledWith("backend-ssh-nongit-switched");

      const sshSnapshot = getWorkspaceSnapshot(sshWorkspaceId);
      expect(sshSnapshot).toBeDefined();
      expect(sshSnapshot!.layout.tabs.some((t) => t.id === tabIdResult)).toBe(true);
      const landedTab = sshSnapshot!.layout.tabs.find((t) => t.id === tabIdResult);
      if (landedTab && "sessionId" in landedTab) {
        expect(sshSnapshot!.sessions[landedTab.sessionId].backendSessionId).toBe("backend-ssh-nongit-switched");
      }
    } finally {
      hook.unmount();
    }
  });

  it("does NOT auto-spawn backendless restored SSH existing tab upon ensureTabForWorktree", async () => {
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "backend-auto-spawned"),
      getTerminalCwd: vi.fn(async () => "/home/remote/project"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const sshWorkspaceId = "ssh:my-host:my-project";
    const initialTab: TerminalTab = { id: "tab-ssh-restored", label: "SSH Restored", sessionId: "session-ssh-restored" };
    const restoredSshState: WorkspaceState = {
      workspaceId: sshWorkspaceId,
      worktrees: [sshWorktree],
      activeWorktreePath: sshWorktree.path,
      sessions: {
        "session-ssh-restored": {
          id: "session-ssh-restored",
          cwd: sshWorktree.path,
          worktreePath: sshWorktree.path,
          workspaceId: sshWorkspaceId,
          worktree: null,
          backendSessionId: null,
          lifecycle: "exited",
          reconnectLifecycle: "idle",
          remoteConnectionState: "disconnected",
        },
      },
      layout: {
        tabs: [initialTab],
        activeTabId: initialTab.id,
        primaryTabId: initialTab.id,
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {
          [initialTab.id]: {
            root: { type: "leaf", leafId: "leaf-ssh-restored" },
            activeLeafId: "leaf-ssh-restored",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-ssh-restored": "session-ssh-restored" },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    setWorkspaceSnapshot(sshWorkspaceId, restoredSshState);

    const hook = renderHook(
      ({ wsId, worktrees }) =>
        useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      {
        initialProps: { wsId: sshWorkspaceId, worktrees: [sshWorktree] },
      },
    );

    try {
      await act(async () => {
        await hook.result.current.ensureTabForWorktree(sshWorktree);
      });

      expect(services.spawnTerminal).not.toHaveBeenCalled();
      expect(hook.result.current.state.sessions["session-ssh-restored"].backendSessionId).toBeNull();
      expect(hook.result.current.state.sessions["session-ssh-restored"].lifecycle).toBe("exited");
    } finally {
      hook.unmount();
    }
  });

  it("binds a paired-daemon split to its owning workspace when the project switches mid-spawn", async () => {
    const pairedWorkspaceId = "daemon:host-1:proj";
    clearWorkspaceSnapshot(pairedWorkspaceId);
    clearHmrWorkspaceState(pairedWorkspaceId);
    const spawnDeferred = createDeferred<{ sessionId: string; session?: { cwd?: string | null } | null }>();
    const spawnStarted = createDeferred<void>();
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "unexpected"),
      spawnTerminalDetailed: vi.fn(async () => {
        spawnStarted.resolve();
        return await spawnDeferred.promise;
      }),
      getTerminalCwd: vi.fn(async () => "/srv/paired"),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };
    const tab: TerminalTab = { id: "tab-p", label: "P", sessionId: "session-p" };
    setWorkspaceSnapshot(pairedWorkspaceId, {
      workspaceId: pairedWorkspaceId,
      worktrees: [sshWorktree],
      activeWorktreePath: sshWorktree.path,
      sessions: {
        "session-p": {
          id: "session-p", cwd: sshWorktree.path, worktreePath: sshWorktree.path,
          workspaceId: pairedWorkspaceId, worktree: null, backendSessionId: "backend-p", lifecycle: "working",
        },
      },
      layout: {
        tabs: [tab], activeTabId: tab.id,
        layoutsByTabId: {
          [tab.id]: { root: { type: "leaf", leafId: "leaf-p" }, activeLeafId: "leaf-p", expandedLeafId: null, sessionIdsByLeafId: { "leaf-p": "session-p" } },
        },
      },
      unreadTabIds: {}, unreadWorktreePaths: {}, activityBySessionId: {},
    });
    const hook = renderHook(
      ({ wsId, worktrees }) => useWorkspaceStore({ workspaceId: wsId, initialWorktrees: worktrees, services }),
      { initialProps: { wsId: pairedWorkspaceId, worktrees: [sshWorktree] } },
    );
    try {
      let splitPromise!: Promise<void>;
      act(() => { splitPromise = hook.result.current.splitPane("tab-p", "leaf-p", "horizontal"); });
      await waitForStarted(spawnStarted.promise);
      const splitSessionId = Object.keys(hook.result.current.state.sessions).find((id) => id !== "session-p")!;
      act(() => { hook.rerender({ wsId: "local-ws", worktrees: [localWorktree] }); });
      await act(async () => {
        spawnDeferred.resolve({ sessionId: "backend-p-split", session: { cwd: "/srv/paired/sub" } });
        await splitPromise;
      });
      expect(services.closeTerminal).not.toHaveBeenCalledWith("backend-p-split");
      const bound = getWorkspaceSnapshot(pairedWorkspaceId)!.sessions[splitSessionId];
      expect(bound.backendSessionId).toBe("backend-p-split");
      expect(bound.lifecycle).toBe("running");
    } finally {
      hook.unmount();
      clearWorkspaceSnapshot(pairedWorkspaceId);
      clearHmrWorkspaceState(pairedWorkspaceId);
    }
  });
});
