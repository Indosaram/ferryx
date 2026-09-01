import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
}

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, TerminalSession, TerminalTab, Worktree } from "../lib/types";
import { useWorkspaceRuntime, type WorkspaceRuntimeServices } from "./workspaceRuntime";
import {
  clearHmrWorkspaceState,
  getHmrWorkspaceState,
  setHmrWorkspaceState,
  useWorkspaceStore,
  type WorkspaceServices,
  type WorkspaceState,
} from "./workspaceStore";

const mainWorktree: Worktree = {
  path: "/repo/main",
  head: "111111",
  branch: "refs/heads/orca/ws-1/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const featureWorktree: Worktree = {
  path: "/repo/feature",
  head: "222222",
  branch: "refs/heads/orca/ws-1/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function createMockServices() {
  const storeServices: WorkspaceServices = {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => "new-backend-pty"),
    getTerminalCwd: vi.fn(async () => "/repo/main"),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };

  const runtimeServices: WorkspaceRuntimeServices = {
    ensureTerminalEvents: vi.fn(async () => undefined),
    listWorktrees: vi.fn(async () => [mainWorktree, featureWorktree]),
    onWorktreeChanged: vi.fn(async () => () => undefined),
    isTauriRuntime: vi.fn(() => true),
  };

  return { storeServices, runtimeServices };
}

function buildTestWorkspaceState(workspaceId = "ws-1"): WorkspaceState {
  const activeTermTab: TerminalTab = {
    id: "tab-term-main",
    label: "main-term",
    sessionId: "session-main-1",
  };

  const activeBrowserTab: BrowserTab = {
    kind: "browser",
    id: "tab-browser-main",
    label: "Frontend Dev",
    browserId: "browser-dev-1",
    url: "http://localhost:5173",
    title: "App Preview",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    worktreePath: "/repo/main",
  };

  const parkedTermTab: TerminalTab = {
    id: "tab-term-feature",
    label: "feature-term",
    sessionId: "session-feature-1",
  };

  const parkedBrowserTab: BrowserTab = {
    kind: "browser",
    id: "tab-browser-feature",
    label: "Docs",
    browserId: "browser-docs-1",
    url: "http://localhost:3000",
    title: "Feature Docs",
    loading: false,
    canGoBack: true,
    canGoForward: false,
    worktreePath: "/repo/feature",
  };

  const sessions: Record<string, TerminalSession> = {
    "session-main-1": {
      id: "session-main-1",
      cwd: "/repo/main",
      worktreePath: "/repo/main",
      workspaceId,
      worktree: { wsId: workspaceId, slug: "main" },
      backendSessionId: "backend-main-1",
      lifecycle: "working",
      daemonEpoch: "1700000000",
      lastOutputSequence: "100",
      agentType: "claude",
      agentSessionId: "claude-session-uuid-hmr-1",
    },
    "session-main-2": {
      id: "session-main-2",
      cwd: "/repo/main/packages/api",
      worktreePath: "/repo/main",
      workspaceId,
      worktree: { wsId: workspaceId, slug: "main" },
      backendSessionId: "backend-main-2",
      lifecycle: "working",
      daemonEpoch: "1700000000",
      lastOutputSequence: "250",
      agentType: "omo",
      agentSessionId: "omo-session-uuid-hmr-2",
    },
    "session-feature-1": {
      id: "session-feature-1",
      cwd: "/repo/feature",
      worktreePath: "/repo/feature",
      workspaceId,
      worktree: { wsId: workspaceId, slug: "feature" },
      backendSessionId: "backend-feature-1",
      lifecycle: "working",
      daemonEpoch: "1700000000",
      lastOutputSequence: "50",
    },
  };

  return {
    worktrees: [mainWorktree, featureWorktree],
    activeWorktreePath: "/repo/main",
    sessions,
    layout: {
      split: "none",
      primaryTabId: "tab-term-main",
      secondaryTabId: null,
      activeTabId: "tab-term-main",
      tabs: [activeTermTab, activeBrowserTab],
      layoutsByTabId: {
        "tab-term-main": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.6,
          },
          activeLeafId: "leaf-2",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-1": "session-main-1",
            "leaf-2": "session-main-2",
          },
        },
      },
    },
    worktreeLayouts: {
      "/repo/feature": {
        split: "none",
        primaryTabId: "tab-term-feature",
        secondaryTabId: null,
        activeTabId: "tab-term-feature",
        tabs: [parkedTermTab, parkedBrowserTab],
        layoutsByTabId: {
          "tab-term-feature": {
            root: { type: "leaf", leafId: "leaf-3" },
            activeLeafId: "leaf-3",
            expandedLeafId: null,
            sessionIdsByLeafId: {
              "leaf-3": "session-feature-1",
            },
          },
        },
      },
    },
    unreadTabIds: { "tab-browser-feature": true },
    unreadWorktreePaths: { "/repo/feature": true },
    activityBySessionId: {},
  };
}

describe("HMR session retention boundary", () => {
  beforeEach(() => {
    clearHmrWorkspaceState?.();
  });

  it("synchronously seeds useWorkspaceStore from HMR handoff and preserves active/parked layouts across runtime refresh", async () => {
    const workspaceId = "ws-1";
    const initialFullState = buildTestWorkspaceState(workspaceId);
    const { storeServices, runtimeServices } = createMockServices();

    // 1. Establish live state in pre-HMR session and verify dispatch sync
    const firstMount = renderHook(() => {
      const store = useWorkspaceStore({ workspaceId, services: storeServices });
      const runtime = useWorkspaceRuntime({
        workspaceId,
        activeWorktreePath: store.state.activeWorktreePath,
        syncWorktrees: store.syncWorktrees,
        ensureTabForWorktree: store.ensureTabForWorktree,
        services: runtimeServices,
      });
      return { store, runtime };
    });

    // Populate initial state into the first mount
    act(() => {
      firstMount.result.current.store.restoreWorkspace(initialFullState);
    });

    // Verify state before HMR
    expect(firstMount.result.current.store.state.layout.tabs).toHaveLength(2);
    expect(firstMount.result.current.store.state.worktreeLayouts?.["/repo/feature"]?.tabs).toHaveLength(2);

    // Verify HMR registry captured the state synchronously at dispatch seam
    const captured = getHmrWorkspaceState(workspaceId);
    expect(captured).not.toBeNull();
    expect(captured?.sessions["session-main-1"]?.backendSessionId).toBe("backend-main-1");

    // 2. Simulate HMR lifecycle: unmount old React tree (simulating Vite Fast Refresh / module reload)
    firstMount.unmount();

    // Reset service mocks to ensure HMR remount does not spawn unnecessary fallback terminals
    storeServices.spawnTerminal = vi.fn(async () => "unexpected-new-spawn");
    storeServices.closeTerminal = vi.fn(async () => undefined);

    // 3. Remount fresh store + runtime (post-HMR module reload)
    const secondMount = renderHook(() => {
      const store = useWorkspaceStore({ workspaceId, services: storeServices });
      const runtime = useWorkspaceRuntime({
        workspaceId,
        activeWorktreePath: store.state.activeWorktreePath,
        syncWorktrees: store.syncWorktrees,
        ensureTabForWorktree: store.ensureTabForWorktree,
        services: runtimeServices,
      });
      return { store, runtime };
    });

    // Initial synchronous state on mount MUST already have the full hydrated state BEFORE refresh
    const initialHydrated = secondMount.result.current.store.state;
    expect(initialHydrated.activeWorktreePath).toBe("/repo/main");
    expect(initialHydrated.layout.tabs).toHaveLength(2);
    expect(initialHydrated.worktreeLayouts?.["/repo/feature"]?.tabs).toHaveLength(2);

    // Allow runtime refreshWorktrees effect to run
    await act(async () => {
      await secondMount.result.current.runtime.refreshWorktrees();
    });

    const finalState = secondMount.result.current.store.state;

    // Assert active worktree & layout
    expect(finalState.activeWorktreePath).toBe("/repo/main");
    expect(finalState.layout.tabs).toHaveLength(2);
    expect(finalState.layout.tabs[0]).toMatchObject({
      id: "tab-term-main",
      label: "main-term",
      sessionId: "session-main-1",
    });
    expect(finalState.layout.tabs[1]).toMatchObject({
      kind: "browser",
      id: "tab-browser-main",
      label: "Frontend Dev",
      browserId: "browser-dev-1",
      url: "http://localhost:5173",
      title: "App Preview",
      worktreePath: "/repo/main",
    });

    // Assert active split pane structure & leaf session mapping
    const activeTabLayout = finalState.layout.layoutsByTabId["tab-term-main"];
    expect(activeTabLayout).toBeDefined();
    expect(activeTabLayout.root).toEqual({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-1" },
      second: { type: "leaf", leafId: "leaf-2" },
      ratio: 0.6,
    });
    expect(activeTabLayout.sessionIdsByLeafId).toEqual({
      "leaf-1": "session-main-1",
      "leaf-2": "session-main-2",
    });

    // Assert parked worktree layout & tabs
    expect(finalState.worktreeLayouts?.["/repo/feature"]).toBeDefined();
    const parkedLayout = finalState.worktreeLayouts!["/repo/feature"];
    expect(parkedLayout.tabs).toHaveLength(2);
    expect(parkedLayout.tabs[0]).toMatchObject({
      id: "tab-term-feature",
      label: "feature-term",
      sessionId: "session-feature-1",
    });
    expect(parkedLayout.tabs[1]).toMatchObject({
      kind: "browser",
      id: "tab-browser-feature",
      label: "Docs",
      browserId: "browser-docs-1",
      url: "http://localhost:3000",
      worktreePath: "/repo/feature",
    });

    // Assert all session metadata preserved (CWD, worktree identity, backendSessionId, daemonEpoch, lastOutputSequence, lifecycle)
    expect(finalState.sessions["session-main-1"]).toEqual({
      id: "session-main-1",
      cwd: "/repo/main",
      worktreePath: "/repo/main",
      workspaceId: "ws-1",
      worktree: { wsId: "ws-1", slug: "main" },
      backendSessionId: "backend-main-1",
      lifecycle: "working",
      daemonEpoch: "1700000000",
      lastOutputSequence: "100",
      agentType: "claude",
      agentSessionId: "claude-session-uuid-hmr-1",
    });

    expect(finalState.sessions["session-main-2"]).toEqual({
      id: "session-main-2",
      cwd: "/repo/main/packages/api",
      worktreePath: "/repo/main",
      workspaceId: "ws-1",
      worktree: { wsId: "ws-1", slug: "main" },
      backendSessionId: "backend-main-2",
      lifecycle: "working",
      daemonEpoch: "1700000000",
      lastOutputSequence: "250",
      agentType: "omo",
      agentSessionId: "omo-session-uuid-hmr-2",
    });

    expect(finalState.sessions["session-feature-1"]).toEqual({
      id: "session-feature-1",
      cwd: "/repo/feature",
      worktreePath: "/repo/feature",
      workspaceId: "ws-1",
      worktree: { wsId: "ws-1", slug: "feature" },
      backendSessionId: "backend-feature-1",
      lifecycle: "working",
      daemonEpoch: "1700000000",
      lastOutputSequence: "50",
    });

    // Ensure no spurious backend terminal spawns occurred during HMR recovery
    expect(storeServices.spawnTerminal).not.toHaveBeenCalled();
    expect(storeServices.closeTerminal).not.toHaveBeenCalled();
  });

  it("isolates HMR handoff strictly to the matching workspace ID", async () => {
    const ws1State = buildTestWorkspaceState("ws-alpha");
    setHmrWorkspaceState("ws-alpha", ws1State);

    const { storeServices } = createMockServices();

    // Render store for an unpopulated workspace ID "ws-beta"
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "ws-beta", services: storeServices }),
    );

    // "ws-beta" must NOT inherit "ws-alpha"'s HMR state
    expect(result.current.state.layout.tabs).toHaveLength(0);
    expect(result.current.state.activeWorktreePath).toBeNull();
  });
});
