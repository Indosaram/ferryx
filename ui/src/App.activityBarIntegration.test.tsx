// Integration coverage for the Activity Bar shell: the real `<App />` across all four modes.
//
// What this suite is for: the Wave 1-3 pieces were each tested alone, and the risk this todo
// exists to retire is the *seam* - the rail switching modes without unmounting the worktree
// surface, Browser mode drawing the real Maho drawer + canvas over the same store, the
// Command Bar / switcher / library actually being reachable from the shell, and the drawer
// collapsing under `Cmd+B` without taking the rail with it.
//
// The native page is replaced by a probe: the native surface's bounds/visibility contract is
// covered by `BrowserPane`'s own suite, and driving a real webview under jsdom is impossible.
// Everything else - `App`, `DesktopActivityBar`, `MahoBrowserSidebar`, `BrowserModeCanvas`,
// `MahoCommandBar`, `MahoLibraryMode`, `browserStore` - is the shipped code.

import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Element = dom.window.Element;
  globalThis.Node = dom.window.Node;
  globalThis.Event = dom.window.Event;
  globalThis.CustomEvent = dom.window.CustomEvent;
  globalThis.MouseEvent = dom.window.MouseEvent;
  globalThis.KeyboardEvent = dom.window.KeyboardEvent;
  globalThis.dispatchEvent = dom.window.dispatchEvent.bind(dom.window);
  globalThis.addEventListener = dom.window.addEventListener.bind(dom.window);
  globalThis.removeEventListener = dom.window.removeEventListener.bind(dom.window);
}

if (!(window as any).__TAURI_INTERNALS__) {
  (window as any).__TAURI_INTERNALS__ = {
    invoke: (cmd: string) => Promise.resolve(cmd === "cmd_browser_download_list" ? [] : undefined),
    transformCallback: () => 1,
  };
}
if (!(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__) {
  (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
}

// The shortcut contract under test is the macOS one (`Cmd+B`); the detector reads
// navigator.platform and navigator.userAgent together, and jsdom interpolates the real
// process.platform into the user agent, so both are pinned.
const originalNavigatorPlatform = Object.getOwnPropertyDescriptor(globalThis.navigator, "platform");
const originalNavigatorUserAgent = Object.getOwnPropertyDescriptor(globalThis.navigator, "userAgent");
Object.defineProperty(globalThis.navigator, "platform", { configurable: true, value: "MacIntel" });
Object.defineProperty(globalThis.navigator, "userAgent", {
  configurable: true,
  value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)",
});

const { act, cleanup, fireEvent, render, screen, waitFor, within } = await import("@testing-library/react");
const { afterAll, afterEach, beforeEach, describe, expect, it, vi } = await import("vitest");
await import("./test/setup");

afterAll(() => {
  for (const [key, descriptor] of [
    ["platform", originalNavigatorPlatform],
    ["userAgent", originalNavigatorUserAgent],
  ] as const) {
    if (descriptor) Object.defineProperty(globalThis.navigator, key, descriptor);
    else Reflect.deleteProperty(globalThis.navigator, key);
  }
});

const native = vi.hoisted(() => ({
  registerProject: vi.fn(),
  listWorktrees: vi.fn(),
  loadSession: vi.fn(),
  saveSession: vi.fn(),
  isTauriRuntime: vi.fn(),
  newTerminalMenuHandler: null as null | (() => void),
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
  onNewWindowMenu: async () => () => undefined,
  onCloseTabMenu: async () => () => undefined,
  onSelectWorktreeMenu: async () => () => undefined,
  onSelectTabMenu: async () => () => undefined,
  onNextTabMenu: async () => () => undefined,
  onPrevTabMenu: async () => () => undefined,
  onSplitRightMenu: async () => () => undefined,
  onSplitDownMenu: async () => () => undefined,
  onCommandPaletteMenu: async () => () => undefined,
  onToggleSidebarMenu: async () => () => undefined,
  onOpenSettingsMenu: async () => () => undefined,
  onRemoteSelectionRequested: async () => () => undefined,
  listenDagRunUpdated: async () => () => undefined,
  publishFocusedTerminal: async () => undefined,
  setBadgeCount: async () => ({ supported: false, count: 0 }),
}));

vi.mock("./lib/updater", () => ({
  startUpdatePolling: () => undefined,
  registerWindowCloseGuard: () => () => undefined,
  applyStagedUpdate: async () => undefined,
  checkForUpdate: async () => undefined,
  downloadAndInstallUpdate: async () => undefined,
  getBackgroundUpdateDownload: () => false,
  getCurrentVersion: async () => null,
  getUpdateChannel: () => "stable",
  getUpdateStatus: () => ({ state: "idle" }),
  setBackgroundUpdateDownload: () => undefined,
  setUpdateChannel: async () => undefined,
  stageAvailableUpdate: async () => undefined,
  subscribeUpdateStatus: () => () => undefined,
  updatesManagedExternally: async () => false,
  cancelUpdateDownload: async () => undefined,
  relaunchApp: async () => undefined,
}));

vi.mock("./lib/updateToast", () => ({ initUpdateToasts: () => () => undefined }));

vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isFocused: async () => true, onFocusChanged: async () => () => undefined }),
}));

// The native page: a probe that records which browserId it was asked to draw, so the canvas
// and the mini pane can be told apart without a real webview.
vi.mock("./components/BrowserPane", () => ({
  BrowserPane: ({ tab, chromeMode, visible }: { tab: { browserId: string }; chromeMode?: string; visible?: boolean }) => (
    <div
      data-testid="browser-pane-probe"
      data-browser-id={tab.browserId}
      data-chrome={chromeMode ?? ""}
      data-visible={String(visible ?? true)}
    />
  ),
}));

// The Browser surfaces' IPC: no host here, and the store is the source of truth under test.
vi.mock("./lib/browserTauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/browserTauri")>(),
  createBrowser: vi.fn(async (request: { browserId?: string; url: string }) => ({
    browserId: request.browserId ?? "created",
    url: request.url,
    title: "",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    visible: false,
  })),
  ensureBrowser: vi.fn(async (request: { browserId: string; url: string }) => ({
    browserId: request.browserId,
    url: request.url,
    title: "",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    visible: false,
  })),
  closeBrowser: vi.fn(async () => undefined),
  navigateBrowser: vi.fn(async () => undefined),
  reloadBrowser: vi.fn(async () => undefined),
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
}));

// Remote Desktop keeps its own canvas; the probe records whether the shell kept it mounted,
// and the store spy records whether anything asked for a disconnect.
vi.mock("./components/remoteDesktop/GlobalRemoteDesktopCanvas", () => ({
  GlobalRemoteDesktopCanvas: ({ active, activeHostId }: { active: boolean; activeHostId: string | null }) => (
    <div data-testid="global-rd-canvas" data-active={String(active)} data-host-id={activeHostId ?? ""} />
  ),
  liveRemoteDesktopSessionCount: (state: { sessions?: Record<string, unknown> }) =>
    Object.keys(state.sessions ?? {}).length,
}));

// The Settings dialog is a lazy heavy tree; this stub exposes exactly the one handoff the
// Activity Bar integration owns: Settings > Remote handing a machine to Remote Desktop mode.
vi.mock("./components/SettingsDialog", () => ({
  SettingsDialog: ({ open, onOpenRemoteDesktop }: { open: boolean; onOpenRemoteDesktop?: (hostId: string, label: string) => void }) =>
    open ? (
      <div data-testid="settings-dialog" role="dialog" aria-label="Settings">
        <button type="button" onClick={() => onOpenRemoteDesktop?.("host-1", "Studio")}>
          Open remote desktop
        </button>
      </div>
    ) : null,
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
  adoptBrowserSession: vi.fn(() => "tab-adopted"),
  dispatchWorkspaceAction: vi.fn(),
  storeState: {
    workspaceId: "local",
    activeWorktreePath: "/local",
    layout: {
      tabs: [{ id: "tab-1", kind: "terminal", label: "Terminal 1", sessionId: "session-1" }],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-1" },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "session-1" },
        },
      },
    },
    sessions: {
      "session-1": {
        id: "session-1",
        backendSessionId: "backend-1",
        cwd: "/local",
        worktreePath: "/local",
        workspaceId: "local",
        worktree: null,
        lifecycle: "running",
      },
    },
    worktrees: [{ path: "/local", branch: "main", head: "", bare: false, detached: false, locked: null, prunable: null }],
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
import { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY, ACTIVITY_MODE_STORAGE_KEY } from "./lib/storageKeys";
import { browserStore } from "./state/browserStore";
import { remoteHostStore } from "./state/remoteHostStore";
import { remoteDesktopStore } from "./state/remoteDesktopStore";

/** The rail's mode button, by its user-facing name. */
function railTab(name: "Supervisor" | "Worktrees" | "Browser" | "Remote Desktop") {
  return screen.getByRole("tab", { name });
}

function seedStoreTab(browserId: string, url = `https://${browserId}.test/`, agentControlled = false) {
  const result = browserStore.upsertTab({ browserId, url, title: browserId, agentControlled });
  expect(result.ok).toBe(true);
}

describe("Activity Bar shell integration", () => {
  beforeEach(() => {
    localStorage.clear();
    browserStore.reset();
    remoteHostStore.setHosts([]);
    remoteHostStore.setActiveHost(null);
    vi.clearAllMocks();

    vi.spyOn(workspaceStoreModule, "useWorkspaceStore").mockImplementation((() => ({
      state: workspace.storeState,
      agents: [],
      tabActivity: {},
      worktreeActivity: {},
      activityNotificationTargets: [],
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
      adoptBrowserSession: workspace.adoptBrowserSession,
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
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("keeps the worktree surface and the Remote Desktop canvas mounted across all four modes", async () => {
    const disconnect = vi.spyOn(remoteDesktopStore, "disconnect");
    render(<App />);

    const surface = await screen.findByTestId("workspace-surface");
    expect(surface).toBeVisible();

    // Supervisor -> Worktrees -> Browser -> Remote Desktop -> Worktrees.
    act(() => { railTab("Supervisor").click(); });
    expect(screen.getByTestId("workspace-surface")).toBeVisible();

    act(() => { railTab("Worktrees").click(); });
    expect(screen.getByTestId("workspace-surface")).toBeVisible();

    act(() => { railTab("Browser").click(); });
    // The worktree surface stays mounted (hidden, not unmounted): unmounting it would tear down
    // PTY attachments and native surfaces, which is the reload this shell exists to prevent.
    const hiddenSurface = screen.getByTestId("workspace-surface");
    expect(hiddenSurface).toBeInTheDocument();
    expect(hiddenSurface).toHaveAttribute("hidden");
    expect(screen.getByTestId("activity-canvas-browser")).toBeInTheDocument();

    act(() => { railTab("Remote Desktop").click(); });
    expect(screen.getByTestId("global-rd-canvas")).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("workspace-surface")).toHaveAttribute("hidden");

    act(() => { railTab("Worktrees").click(); });
    expect(screen.getByTestId("workspace-surface")).toBeVisible();
    // The RD canvas is still mounted, only hidden: a live session must survive a mode switch.
    expect(screen.getByTestId("global-rd-canvas")).toHaveAttribute("data-active", "false");
    expect(disconnect).not.toHaveBeenCalled();
  });

  it("draws the Maho drawer and canvas in Browser mode and collapses the drawer on Cmd+B while the rail stays", async () => {
    render(<App />);
    await screen.findByTestId("workspace-surface");

    act(() => { railTab("Browser").click(); });

    const drawer = screen.getByTestId("activity-drawer-browser");
    expect(within(drawer).getByRole("complementary", { name: /Browser sidebar/ })).toBeInTheDocument();
    expect(screen.getByTestId("browser-mode-canvas")).toBeInTheDocument();
    // Cmd+B collapses the drawer; the Activity Bar is the shell's left edge and never hides.
    act(() => {
      fireEvent.keyDown(window, { key: "b", metaKey: true });
    });
    await waitFor(() => expect(screen.queryByTestId("activity-drawer-browser")).toBeNull());
    expect(railTab("Browser")).toBeInTheDocument();
    expect(screen.getByTestId("browser-mode-canvas")).toBeInTheDocument();
  });

  it("opens the Command Bar from the Browser-mode chord event and toggles the Library over the tab body", async () => {
    seedStoreTab("tab-a", "https://example.com/");
    browserStore.touch("tab-a");
    render(<App />);
    await screen.findByTestId("workspace-surface");
    act(() => { railTab("Browser").click(); });

    // The chord router in `App` publishes the action; the shell owns the surface that shows it.
    act(() => {
      window.dispatchEvent(new CustomEvent("ferryx:browser-mode-shortcut", { detail: { action: "command-bar" } }));
    });
    const commandBar = await screen.findByRole("dialog", { name: "Maho command bar" });
    expect(commandBar).toBeInTheDocument();
    act(() => { fireEvent.keyDown(window, { key: "Escape" }); });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Maho command bar" })).toBeNull());

    // Library replaces the tab body (favorites, pinned, Today) with the library rail. Agent tasks
    // stay: they are their own section below the body, not part of it.
    const drawer = screen.getByTestId("activity-drawer-browser");
    expect(within(drawer).getByTestId("maho-agent-tasks")).toBeInTheDocument();
    expect(within(drawer).queryByRole("tablist", { name: "Browser library" })).toBeNull();
    const libraryToggle = within(drawer).getByRole("button", { name: "Library" });
    act(() => { libraryToggle.click(); });
    expect(await within(drawer).findByRole("tablist", { name: "Browser library" })).toBeInTheDocument();
    act(() => { libraryToggle.click(); });
    await waitFor(() => expect(within(drawer).queryByRole("tablist", { name: "Browser library" })).toBeNull());
  });

  it("tidy groups the Space's Today tabs and shows the browser badge for an agent-controlled tab", async () => {
    seedStoreTab("docs-1", "https://docs.example.com/a");
    seedStoreTab("docs-2", "https://docs.example.com/b");
    seedStoreTab("agent-1", "https://agent.example.com/", true);
    browserStore.touch("agent-1");
    render(<App />);
    await screen.findByTestId("workspace-surface");

    // An agent-controlled tab is the Browser mode's attention count.
    await waitFor(() => expect(screen.getByTestId("activity-mode-browser-badge")).toHaveTextContent("1"));

    act(() => { railTab("Browser").click(); });
    const drawer = screen.getByTestId("activity-drawer-browser");
    const tidy = within(drawer).getByRole("button", { name: "Tidy Today" });
    act(() => { tidy.click(); });

    // Two tabs on one host become one folder named for that host.
    await waitFor(() => expect(Object.values(browserStore.getState().folders).map((folder) => folder.name)).toContain("docs.example.com"));
    expect(browserStore.getState().tabs["docs-1"]?.folderId).toBeDefined();
  });

  it("hands a Settings > Remote machine to Remote Desktop mode and shows the connected count on the rail", async () => {
    const connect = vi.spyOn(remoteDesktopStore, "connect").mockResolvedValue(undefined as never);
    render(<App />);
    await screen.findByTestId("workspace-surface");

    // The rail's Settings button opens the dialog; its machine row performs the handoff.
    act(() => {
      screen.getByTestId("activity-settings").click();
    });
    // The dialog is lazy-loaded: its content arrives one dynamic import after the shell's
    // placeholder, so the handoff button is awaited rather than assumed.
    const handoff = await screen.findByRole("button", { name: "Open remote desktop" });
    act(() => { handoff.click(); });

    await waitFor(() => expect(connect).toHaveBeenCalledWith("host-1", "Studio"));
    expect(screen.queryByRole("button", { name: "Open remote desktop" })).toBeNull();
    // The mode switched to Remote Desktop: its drawer is the mode's own surface.
    expect(await screen.findByTestId("activity-drawer-remote-desktop")).toBeInTheDocument();
    expect(localStorage.getItem(ACTIVITY_MODE_STORAGE_KEY)).toBe("remoteDesktop");
  });
});
