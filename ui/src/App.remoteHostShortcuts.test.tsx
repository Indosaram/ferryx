import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { remoteHostStore } from "./state/remoteHostStore";

const native = vi.hoisted(() => ({
  registerProject: vi.fn(),
  listWorktrees: vi.fn(),
  spawnTerminalDetailed: vi.fn(),
  spawnTerminal: vi.fn(),
  watchDagProject: vi.fn(),
  loadSession: vi.fn(),
  saveSession: vi.fn(),
  isTauriRuntime: vi.fn(),
  newTerminalMenuHandler: null as null | (() => void),
  closeMenuHandler: null as null | (() => void),
  selectWorktreeMenuHandler: null as null | ((digit: number) => void),
  selectTabMenuHandler: null as null | ((digit: number) => void),
  splitRightMenuHandler: null as null | (() => void),
  splitDownMenuHandler: null as null | (() => void),
  commandPaletteMenuHandler: null as null | (() => void),
}));

vi.mock("./lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/tauri")>(),
  ...native,
  getInitialProject: async () => ({ workspaceId: "local", repoRoot: "/local", gitRoot: null }),
  detectAgents: async () => [],
  listTerminalSessions: async () => [],
  ensureTerminalEvents: async () => undefined,
  onWorktreeChanged: async () => () => undefined,
  onTerminalLifecycle: async () => () => undefined,
  onTerminalOutput: async () => () => undefined,
  onNewTerminalTabMenu: async (handler: () => void) => {
    native.newTerminalMenuHandler = handler;
    return () => {
      if (native.newTerminalMenuHandler === handler) native.newTerminalMenuHandler = null;
    };
  },
  onCloseTabMenu: async (handler: () => void) => {
    native.closeMenuHandler = handler;
    return () => {
      if (native.closeMenuHandler === handler) native.closeMenuHandler = null;
    };
  },
  onSelectWorktreeMenu: async (handler: (digit: number) => void) => {
    native.selectWorktreeMenuHandler = handler;
    return () => {
      if (native.selectWorktreeMenuHandler === handler) native.selectWorktreeMenuHandler = null;
    };
  },
  onSelectTabMenu: async (handler: (digit: number) => void) => {
    native.selectTabMenuHandler = handler;
    return () => {
      if (native.selectTabMenuHandler === handler) native.selectTabMenuHandler = null;
    };
  },
  onNextTabMenu: async () => () => undefined,
  onPrevTabMenu: async () => () => undefined,
  onSplitRightMenu: async (handler: () => void) => {
    native.splitRightMenuHandler = handler;
    return () => {
      if (native.splitRightMenuHandler === handler) native.splitRightMenuHandler = null;
    };
  },
  onSplitDownMenu: async (handler: () => void) => {
    native.splitDownMenuHandler = handler;
    return () => {
      if (native.splitDownMenuHandler === handler) native.splitDownMenuHandler = null;
    };
  },
  onCommandPaletteMenu: async (handler: () => void) => {
    native.commandPaletteMenuHandler = handler;
    return () => {
      if (native.commandPaletteMenuHandler === handler) native.commandPaletteMenuHandler = null;
    };
  },
  onToggleSidebarMenu: async () => () => undefined,
  onOpenSettingsMenu: async () => () => undefined,
  onRemoteSelectionRequested: async () => () => undefined,
  listenDagRunUpdated: async () => () => undefined,
  publishFocusedTerminal: async () => undefined,
  setBadgeCount: async () => ({ supported: false, count: 0 }),
}));

vi.mock("./remote/RemoteApp", () => ({
  RemoteHostConnection: ({ hostId }: { hostId: string }) => (
    <div data-testid="mock-remote-host-connection" data-host-id={hostId}>
      Remote Connection: {hostId}
    </div>
  ),
}));

vi.mock("./lib/updater", () => ({
  startUpdatePolling: () => undefined,
  registerWindowCloseGuard: () => () => undefined,
}));

vi.mock("./lib/updateToast", () => ({
  initUpdateToasts: () => () => undefined,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async () => () => undefined,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFocused: async () => true,
    onFocusChanged: async () => () => undefined,
  }),
}));

const workspace = {
  activateTab: vi.fn(),
  closeTab: vi.fn(() => Promise.resolve(undefined)),
  splitPane: vi.fn(() => Promise.resolve(undefined)),
  closePane: vi.fn(() => Promise.resolve(undefined)),
  focusPane: vi.fn(),
  setPaneRatio: vi.fn(),
  swapPanes: vi.fn(),
  ensureTabForWorktree: vi.fn(() => Promise.resolve("tab-1")),
  ensureSessionBackends: vi.fn(() => Promise.resolve(undefined)),
  openTab: vi.fn(() => Promise.resolve("tab-2")),
  syncWorktrees: vi.fn(),
  restoreWorkspace: vi.fn(() => Promise.resolve(undefined)),
  createBrowserTab: vi.fn(() => Promise.resolve("tab-browser")),
  dispatchWorkspaceAction: vi.fn(),
  listeners: [] as Array<(state: any) => void>,
  storeState: {
    workspaceId: "local",
    activeWorktreePath: "/local",
    layout: {
      tabs: [{ id: "tab-1", kind: "terminal", label: "Terminal 1", title: "Terminal 1", sessionId: "session-1" }],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-1" },
          activeLeafId: "leaf-1",
        },
      },
    },
    sessions: {
      "session-1": {
        id: "session-1",
        backendSessionId: "backend-1",
        cwd: "/local",
        worktreePath: "/local",
        lifecycle: "alive",
      },
    },
    worktrees: [{ path: "/local", branch: "main" }],
    unreadTabIds: {},
    unreadWorktreePaths: {},
  },
};

const workspaceStoreModule = await import("./state/workspaceStore");

vi.mock("./state/workspaceRuntime", () => ({
  useWorkspaceRuntime: () => ({
    refreshWorktrees: vi.fn().mockResolvedValue(undefined),
    reportRuntimeError: vi.fn(),
    runtimeError: null,
  }),
}));

import { App } from "./App";
import { PROJECTS_STORAGE_KEY, ACTIVE_PROJECT_STORAGE_KEY } from "./lib/storageKeys";

describe("Blocker H3: local workspace shortcut suppression when remote host is active", () => {
  beforeEach(() => {
    localStorage.clear();
    remoteHostStore.setHosts([]);
    remoteHostStore.setActiveHost(null);
    vi.clearAllMocks();

    vi.spyOn(workspaceStoreModule, "useWorkspaceStore").mockImplementation((() => ({
      state: workspace.storeState,
      agents: [],
      tabActivity: {},
      worktreeActivity: {},
      activityNotificationTargets: [],
      store: {
        getState: () => workspace.storeState,
        subscribe: (fn: any) => {
          workspace.listeners.push(fn);
          return () => {
            workspace.listeners = workspace.listeners.filter((l) => l !== fn);
          };
        },
      },
      activateTab: workspace.activateTab,
      closeTab: workspace.closeTab,
      splitPane: workspace.splitPane,
      closePane: workspace.closePane,
      focusPane: workspace.focusPane,
      setPaneRatio: workspace.setPaneRatio,
      swapPanes: workspace.swapPanes,
      ensureTabForWorktree: workspace.ensureTabForWorktree,
      ensureSessionBackends: workspace.ensureSessionBackends,
      openTab: workspace.openTab,
      syncWorktrees: workspace.syncWorktrees,
      restoreWorkspace: workspace.restoreWorkspace,
      createBrowserTab: workspace.createBrowserTab,
      dispatchWorkspaceAction: workspace.dispatchWorkspaceAction,
      subscribeTerminalBell: () => () => undefined,
      subscribeActivityNotification: () => () => undefined,
    })) as any);

    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "local", repoRoot: "/local", gitRoot: "/local" }]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "local");

    native.isTauriRuntime.mockReturnValue(false);
    native.registerProject.mockResolvedValue({ workspaceId: "local", repoRoot: "/local", gitRoot: "/local" });
    native.listWorktrees.mockResolvedValue([{ path: "/local", branch: null }]);
    native.loadSession.mockResolvedValue(null);
    native.saveSession.mockResolvedValue(undefined);
    native.watchDagProject.mockResolvedValue({ projectPath: "/local", runs: [] });
  });

  afterEach(() => {
    cleanup();
  });

  it("suppresses local shortcut and menu mutations when activeRemoteHost is active", async () => {
    render(<App />);

    // Initially local machine is active, local workspace is mounted
    await waitFor(() => {
      expect(screen.queryByTestId("mock-remote-host-connection")).toBeNull();
    });

    // Verify listeners are wired
    expect(native.newTerminalMenuHandler).toBeTypeOf("function");
    expect(native.closeMenuHandler).toBeTypeOf("function");
    expect(native.splitRightMenuHandler).toBeTypeOf("function");
    expect(native.splitDownMenuHandler).toBeTypeOf("function");
    expect(native.commandPaletteMenuHandler).toBeTypeOf("function");

    // Now switch to a remote host
    act(() => {
      remoteHostStore.upsertHost({
        hostId: "remote-worker-1",
        name: "Remote Worker 1",
        address: "https://relay.checka.cc",
        transport: "relay",
        authStatus: "paired",
        online: true,
      });
      remoteHostStore.setActiveHost("remote-worker-1");
    });

    // RemoteHostConnection is now rendered in main
    await waitFor(() => {
      expect(screen.getByTestId("mock-remote-host-connection")).toBeInTheDocument();
    });
    expect(screen.getByTestId("mock-remote-host-connection")).toHaveAttribute("data-host-id", "remote-worker-1");

    expect(workspace.openTab).not.toHaveBeenCalled();
    expect(workspace.closeTab).not.toHaveBeenCalled();
    expect(workspace.splitPane).not.toHaveBeenCalled();

    // 1. Trigger menu actions while remote host is active
    act(() => {
      native.newTerminalMenuHandler?.();
      native.closeMenuHandler?.();
      native.splitRightMenuHandler?.();
      native.splitDownMenuHandler?.();
      native.selectTabMenuHandler?.(1);
      native.selectWorktreeMenuHandler?.(1);
      native.commandPaletteMenuHandler?.();
    });

    // None of these menu actions should mutate the local workspace
    expect(workspace.openTab).not.toHaveBeenCalled();
    expect(workspace.closeTab).not.toHaveBeenCalled();
    expect(workspace.splitPane).not.toHaveBeenCalled();
    expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();

    // 2. Keyboard shortcuts: Cmd+T (new terminal), Cmd+W (close tab), Cmd+D (split right), etc.
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "t", metaKey: true, bubbles: true }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "w", metaKey: true, bubbles: true }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "d", metaKey: true, bubbles: true }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "d", metaKey: true, shiftKey: true, bubbles: true }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "1", metaKey: true, bubbles: true }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "1", ctrlKey: true, bubbles: true }));
    });

    // Still no local workspace mutations
    expect(workspace.openTab).not.toHaveBeenCalled();
    expect(workspace.closeTab).not.toHaveBeenCalled();
    expect(workspace.splitPane).not.toHaveBeenCalled();

    // 3. Switch back to local machine
    act(() => {
      remoteHostStore.setActiveHost(null);
    });

    // RemoteHostConnection is gone, local workspace is back
    await waitFor(() => {
      expect(screen.queryByTestId("mock-remote-host-connection")).toBeNull();
    });

    // Now menu actions and shortcuts work again for the local workspace!
    act(() => {
      native.newTerminalMenuHandler?.();
    });

    await waitFor(() => {
      expect(workspace.openTab).toHaveBeenCalledOnce();
    });

    act(() => {
      native.closeMenuHandler?.();
    });

    await waitFor(() => {
      expect(workspace.closeTab).toHaveBeenCalledWith("tab-1");
    });
  });

  it("suppresses createWorktree modal and creation when activeRemoteHost is active", async () => {
    render(<App />);

    await waitFor(() => {
      expect(screen.queryByTestId("mock-remote-host-connection")).toBeNull();
    });

    // Switch to remote host
    act(() => {
      remoteHostStore.upsertHost({
        hostId: "remote-worker-2",
        name: "Remote Worker 2",
        address: "https://relay.checka.cc",
        transport: "relay",
        authStatus: "paired",
        online: true,
      });
      remoteHostStore.setActiveHost("remote-worker-2");
    });

    await waitFor(() => {
      expect(screen.getByTestId("mock-remote-host-connection")).toBeInTheDocument();
    });

    // Clicking "Add worktree to local" in sidebar should NOT open AddWorktreeDialog
    const addWorktreeBtn = screen.getByRole("button", { name: "Add worktree to local" });
    act(() => {
      addWorktreeBtn.click();
    });

    // AddWorktreeDialog should not be rendered
    expect(screen.queryByRole("dialog", { name: "Create Worktree" })).toBeNull();
    expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
  });

  it("suppresses command palette menu and worktree selection mutations when activeRemoteHost is active", async () => {
    render(<App />);

    await waitFor(() => {
      expect(screen.queryByTestId("mock-remote-host-connection")).toBeNull();
    });

    // Clear initial mount calls
    workspace.activateTab.mockClear();
    workspace.ensureTabForWorktree.mockClear();
    workspace.openTab.mockClear();

    expect(native.commandPaletteMenuHandler).toBeTypeOf("function");

    // Switch to remote host
    act(() => {
      remoteHostStore.upsertHost({
        hostId: "remote-worker-3",
        name: "Remote Worker 3",
        address: "https://relay.checka.cc",
        transport: "relay",
        authStatus: "paired",
        online: true,
      });
      remoteHostStore.setActiveHost("remote-worker-3");
    });

    await waitFor(() => {
      expect(screen.getByTestId("mock-remote-host-connection")).toBeInTheDocument();
    });

    // 1. Invoking onCommandPaletteMenu while remote is active does not open command palette
    act(() => {
      native.commandPaletteMenuHandler?.();
    });

    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
    expect(workspace.openTab).not.toHaveBeenCalled();
    expect(workspace.activateTab).not.toHaveBeenCalled();

    // 2. Invoking handleSelectWorktree (via sidebar worktree click) while remote is active does not mutate local workspace
    const worktreeBtn = document.querySelector<HTMLElement>('[data-shortcut-worktree-path="/local"]');
    expect(worktreeBtn).not.toBeNull();
    act(() => {
      worktreeBtn?.click();
    });

    expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
    expect(workspace.openTab).not.toHaveBeenCalled();
    expect(workspace.activateTab).not.toHaveBeenCalled();
    expect(workspace.dispatchWorkspaceAction).not.toHaveBeenCalled();

    // 3. Switch back to local host
    act(() => {
      remoteHostStore.setActiveHost(null);
    });

    await waitFor(() => {
      expect(screen.queryByTestId("mock-remote-host-connection")).toBeNull();
    });

    // 4. In local mode, onCommandPaletteMenu opens command palette
    act(() => {
      native.commandPaletteMenuHandler?.();
    });

    const commandPalette = screen.getByRole("dialog", { name: "Command palette" });
    expect(commandPalette).toBeInTheDocument();

    // 5. In local mode, selecting a worktree in the command palette mutates workspace (calls ensureTabForWorktree)
    const worktreeOption = within(commandPalette).getByRole("button", { name: /local/i });
    act(() => {
      worktreeOption.click();
    });

    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
      expect.objectContaining({ path: "/local" }),
    );
  });

  it("dismisses open command palette and suppresses worktree selection when switching to remote host", async () => {
    render(<App />);

    await waitFor(() => {
      expect(screen.queryByTestId("mock-remote-host-connection")).toBeNull();
    });

    // Clear initial mount calls
    workspace.activateTab.mockClear();
    workspace.ensureTabForWorktree.mockClear();
    workspace.openTab.mockClear();

    // Open command palette in local mode
    act(() => {
      native.commandPaletteMenuHandler?.();
    });
    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();

    // Switch to remote host while command palette was open
    act(() => {
      remoteHostStore.upsertHost({
        hostId: "remote-worker-4",
        name: "Remote Worker 4",
        address: "https://relay.checka.cc",
        transport: "relay",
        authStatus: "paired",
        online: true,
      });
      remoteHostStore.setActiveHost("remote-worker-4");
    });

    await waitFor(() => {
      expect(screen.getByTestId("mock-remote-host-connection")).toBeInTheDocument();
    });

    // Command palette must now be dismissed / unmounted
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();

    // Worktree selection remains suppressed
    const worktreeBtn = document.querySelector<HTMLElement>('[data-shortcut-worktree-path="/local"]');
    expect(worktreeBtn).not.toBeNull();
    act(() => {
      worktreeBtn?.click();
    });

    expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
    expect(workspace.openTab).not.toHaveBeenCalled();
    expect(workspace.activateTab).not.toHaveBeenCalled();
  });
});
