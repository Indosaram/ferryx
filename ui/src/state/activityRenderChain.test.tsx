import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { NativeTerminalAgentStatePayload, NativeTerminalBellPayload, NativeTerminalTitlePayload, Worktree } from "../lib/types";
import { TabBar } from "../components/TabBar";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const nativeListeners = vi.hoisted(() => ({
  title: new Set<(payload: NativeTerminalTitlePayload) => void>(),
  bell: new Set<(payload: NativeTerminalBellPayload) => void>(),
  agentState: new Set<(payload: NativeTerminalAgentStatePayload) => void>(),
}));

const nativeWindow = vi.hoisted(() => ({
  startDragging: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => nativeWindow,
}));

vi.mock("../lib/tauri", () => ({
  DEFAULT_WORKSPACE_ID: "default",
  spawnTerminal: vi.fn(async () => "backend-default"),
  closeTerminal: vi.fn(async () => undefined),
  getTerminalCwd: vi.fn(async () => "/repo/main"),
  waitForTerminalExit: vi.fn(async () => undefined),
  onNativeTerminalTitle: vi.fn(async (handler: (payload: NativeTerminalTitlePayload) => void) => {
    nativeListeners.title.add(handler);
    return () => nativeListeners.title.delete(handler);
  }),
  onNativeTerminalBell: vi.fn(async (handler: (payload: NativeTerminalBellPayload) => void) => {
    nativeListeners.bell.add(handler);
    return () => nativeListeners.bell.delete(handler);
  }),
  onNativeTerminalAgentState: vi.fn(async (handler: (payload: NativeTerminalAgentStatePayload) => void) => {
    nativeListeners.agentState.add(handler);
    return () => nativeListeners.agentState.delete(handler);
  }),
}));

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function createServices(): WorkspaceServices {
  let backendCounter = 0;
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => `backend-${++backendCounter}`),
    getTerminalCwd: vi.fn(async () => worktree.path),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };
}

function emitNativeTitle(payload: NativeTerminalTitlePayload): void {
  for (const listener of nativeListeners.title) {
    listener(payload);
  }
}

function emitNativeAgentState(payload: NativeTerminalAgentStatePayload): void {
  for (const listener of nativeListeners.agentState) {
    listener(payload);
  }
}

type StoreHandle = {
  current: ReturnType<typeof useWorkspaceStore> | null;
};

function RealWorkspaceTabBarHarness({
  storeHandle,
  initialWorktrees,
  services,
}: {
  storeHandle: StoreHandle;
  initialWorktrees: Worktree[];
  services: WorkspaceServices;
}) {
  const store = useWorkspaceStore({ initialWorktrees, services });
  storeHandle.current = store;

  return (
    <div data-testid="workspace-root">
      <TabBar
        groupId="group-default"
        tabs={store.state.layout.tabs}
        activeTabId={store.state.layout.activeTabId ?? ""}
        onActivate={store.activateTab}
        onClose={store.closeTab}
        onAdd={() => void store.openTab(initialWorktrees[0] ?? worktree)}
        unreadTabIds={store.state.unreadTabIds}
        activityByTabId={store.tabActivity}
      />
    </div>
  );
}

describe("Activity render chain: native title -> workspace store -> TabBar -> StatusDot", () => {
  beforeEach(() => {
    nativeListeners.title.clear();
    nativeListeners.bell.clear();
    nativeWindow.startDragging.mockClear();
    localStorage.clear();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders a spinning working indicator on a non-active tab when an agent title arrives, then transitions to unread attention on completion", async () => {
    const storeHandle: StoreHandle = { current: null };
    const services = createServices();

    render(
      <RealWorkspaceTabBarHarness
        storeHandle={storeHandle}
        initialWorktrees={[worktree]}
        services={services}
      />,
    );

    // Wait for the native title listener subscription in useWorkspaceStore
    await waitFor(() => {
      expect(nativeListeners.title.size).toBeGreaterThan(0);
    });

    const getStore = () => {
      const current = storeHandle.current;
      if (!current) throw new Error("store failed to initialize");
      return current;
    };

    // Open first tab (active tab)
    let tab1Id = "";
    await act(async () => {
      const opened = await getStore().openTab(worktree, "Tab 1");
      if (!opened) throw new Error("failed to open tab 1");
      tab1Id = opened;
    });

    // Open second tab (background / non-active target tab)
    let tab2Id = "";
    await act(async () => {
      const opened = await getStore().openTab(worktree, "Tab 2");
      if (!opened) throw new Error("failed to open tab 2");
      tab2Id = opened;
    });

    // Switch active tab back to tab 1 so tab 2 is non-active
    act(() => {
      getStore().activateTab(tab1Id);
    });

    // Verify Tab 2 is in the DOM and is non-active
    const tab1Element = screen.getByText("Tab 1").closest('[role="tab"]') as HTMLElement;
    const tab2Element = screen.getByText("Tab 2").closest('[role="tab"]') as HTMLElement;
    expect(tab1Element).toHaveAttribute("aria-selected", "true");
    expect(tab2Element).toHaveAttribute("aria-selected", "false");

    // Backend session for tab 2 is backend-2 (from createServices counter)
    const session2 = Object.values(storeHandle.current!.state.sessions).find(
      (s) => s.backendSessionId === "backend-2",
    );
    expect(session2).toBeDefined();

    // 1. Dispatch working title from agent (e.g. OMO) to the native title listener
    act(() => {
      emitNativeTitle({
        sessionId: "backend-2",
        title: "⠋ omo: generating code changes",
      });
    });

    // Assert: Store records working activity for Tab 2
    expect(storeHandle.current!.tabActivity[tab2Id]).toMatchObject({
      hasWorking: true,
      workingCount: 1,
      agentType: "omo",
    });

    // Assert (a): Real DOM element inside Tab 2 with data-status-state="working" exists and has animate-spin
    const workingIndicator = tab2Element.querySelector('[data-status-state="working"]');
    expect(workingIndicator).not.toBeNull();
    expect(workingIndicator).toBeInTheDocument();
    expect(workingIndicator).toHaveClass("animate-spin");
    expect(workingIndicator?.getAttribute("class")).toContain("animate-spin");

    // Tab 2 should also render the agent icon container with working indicator test ID
    const tab2WorkingDot = tab2Element.querySelector('[data-testid="tab-working-indicator"]');
    expect(tab2WorkingDot).not.toBeNull();
    expect(tab2WorkingDot).toContainElement(workingIndicator as HTMLElement);

    // 2. Dispatch completion title from agent to native title listener
    act(() => {
      emitNativeTitle({
        sessionId: "backend-2",
        title: "omo: done",
      });
    });

    // Assert: Store reports tab 2 has unread attention because it was non-active
    expect(storeHandle.current!.state.unreadTabIds[tab2Id]).toBe(true);
    expect(storeHandle.current!.tabActivity[tab2Id]).toMatchObject({
      hasDone: true,
      hasUnread: true,
      hasWorking: false,
    });

    // Assert (b): Real DOM element inside Tab 2 transitions to unread attention dot
    const unreadIndicator = tab2Element.querySelector('[data-status-state="unread"]');
    expect(unreadIndicator).not.toBeNull();
    expect(unreadIndicator).toBeInTheDocument();

    // Working spinner must no longer be present
    expect(tab2Element.querySelector('[data-status-state="working"]')).toBeNull();

    // Tab 2 unread dot container
    const tab2UnreadDot = tab2Element.querySelector('[data-testid="tab-unread-dot"]');
    expect(tab2UnreadDot).not.toBeNull();
    expect(tab2UnreadDot).toContainElement(unreadIndicator as HTMLElement);
  });

  it("handles non-agent working titles (e.g. cargo/test spinners) through the full render chain", async () => {
    const storeHandle: StoreHandle = { current: null };
    const services = createServices();

    render(
      <RealWorkspaceTabBarHarness
        storeHandle={storeHandle}
        initialWorktrees={[worktree]}
        services={services}
      />,
    );

    await waitFor(() => {
      expect(nativeListeners.title.size).toBeGreaterThan(0);
    });

    const getStore = () => {
      const current = storeHandle.current;
      if (!current) throw new Error("store failed to initialize");
      return current;
    };

    let tab1Id = "";
    await act(async () => {
      const opened = await getStore().openTab(worktree, "Active Tab");
      if (!opened) throw new Error("failed to open tab 1");
      tab1Id = opened;
    });

    let tab2Id = "";
    await act(async () => {
      const opened = await getStore().openTab(worktree, "Worker Tab");
      if (!opened) throw new Error("failed to open tab 2");
      tab2Id = opened;
    });

    act(() => {
      getStore().activateTab(tab1Id);
    });

    expect(getStore().state.layout.activeTabId).toBe(tab1Id);
    expect(tab2Id).not.toBe("");

    const tab2Element = screen.getByText("Worker Tab").closest('[role="tab"]') as HTMLElement;

    // Dispatch a non-agent title with spinner glyph
    act(() => {
      emitNativeTitle({
        sessionId: "backend-2",
        title: "⠋ cargo test --workspace",
      });
    });

    // Tab 2 should render the StatusDot with data-status-state="working" and animate-spin
    const workingDot = tab2Element.querySelector('[data-status-state="working"]');
    expect(workingDot).not.toBeNull();
    expect(workingDot).toHaveClass("animate-spin");
    expect(workingDot?.getAttribute("class")).toContain("animate-spin");
  });

  it("renders working then unread on a non-active tab from an EXTENSION-reported state, with no title involved", async () => {
    const storeHandle: StoreHandle = { current: null };
    const services = createServices();

    render(
      <RealWorkspaceTabBarHarness
        storeHandle={storeHandle}
        initialWorktrees={[worktree]}
        services={services}
      />,
    );

    await waitFor(() => {
      expect(nativeListeners.agentState.size).toBeGreaterThan(0);
    });

    const getStore = () => {
      const current = storeHandle.current;
      if (!current) throw new Error("store failed to initialize");
      return current;
    };

    let tab1Id = "";
    await act(async () => {
      const opened = await getStore().openTab(worktree, "Tab 1");
      if (!opened) throw new Error("failed to open tab 1");
      tab1Id = opened;
    });
    let tab2Id = "";
    await act(async () => {
      const opened = await getStore().openTab(worktree, "Tab 2");
      if (!opened) throw new Error("failed to open tab 2");
      tab2Id = opened;
    });
    act(() => {
      getStore().activateTab(tab1Id);
    });

    const tab2Element = screen.getByText("Tab 2").closest('[role="tab"]') as HTMLElement;
    expect(tab2Element).toHaveAttribute("aria-selected", "false");

    // The agent reports its own state. omo emits only a bare name in its title, so this path is the
    // ONLY way its working state can ever be known.
    act(() => {
      emitNativeAgentState({
        sessionId: "backend-2",
        state: "working",
        ruleId: "",
        manifestId: "omo",
      });
    });

    expect(storeHandle.current!.tabActivity[tab2Id]).toMatchObject({ hasWorking: true });
    const workingIndicator = tab2Element.querySelector('[data-status-state="working"]');
    expect(workingIndicator).not.toBeNull();
    expect(workingIndicator).toHaveClass("animate-spin");

    // Work ends. The tab is non-active, so it must be marked as needing attention.
    act(() => {
      emitNativeAgentState({
        sessionId: "backend-2",
        state: "idle",
        ruleId: "",
        manifestId: "omo",
      });
    });

    expect(storeHandle.current!.state.unreadTabIds[tab2Id]).toBe(true);
    expect(tab2Element.querySelector('[data-status-state="working"]')).toBeNull();
    expect(tab2Element.querySelector('[data-testid="tab-unread-dot"]')).not.toBeNull();
  });
});
