import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import dagRunSampleJson from "../state/__fixtures__/dagRunSample.json";
import { loadAdvancedFeatures, setAdvancedFeaturesFallbackForTests } from "../lib/advancedFeatures";
import { parseDagRunSnapshot } from "../lib/dagTypes";
import { ADVANCED_FEATURES_STORAGE_KEY, PROJECTS_STORAGE_KEY } from "../lib/storageKeys";
import type { TerminalSession, Worktree } from "../lib/types";
import { dagStore } from "../state/dagStore";
import { EmptyWorkspaceView } from "./EmptyWorkspaceView";
import { SettingsDialog } from "./SettingsDialog";
import { Sidebar } from "./Sidebar";
import { TerminalPane } from "./TerminalPane";
import { DesktopActivityBar } from "./activity/DesktopActivityBar";

vi.mock("../lib/updater", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/updater")>(),
  cancelUpdateDownload: vi.fn(),
  checkForUpdate: vi.fn(),
  downloadAndInstallUpdate: vi.fn(),
  getCurrentVersion: vi.fn(async () => "2026.1024.1"),
  getUpdateStatus: vi.fn(() => ({ state: "idle" })),
  relaunchApp: vi.fn(),
  subscribeUpdateStatus: vi.fn(() => () => undefined),
  updatesManagedExternally: vi.fn(async () => false),
}));

const native = vi.hoisted(() => ({
  getTerminalPreferences: vi.fn(),
  getCliLauncherStatus: vi.fn(),
  getDaemonSupervisor: vi.fn(),
}));

vi.mock(import("../lib/tauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    getTerminalPreferences: native.getTerminalPreferences,
    getCliLauncherStatus: native.getCliLauncherStatus,
    getDaemonSupervisor: native.getDaemonSupervisor,
  };
});

vi.mock("./NativeTerminalPane", () => ({
  NativeTerminalPane: () => <div data-testid="native-terminal-pane" />,
}));

vi.mock("./TerminalSearchOverlay", () => ({ TerminalSearchOverlay: () => null }));

const parsedDag = parseDagRunSnapshot(dagRunSampleJson);
if (!parsedDag) throw new TypeError("DAG test fixture is invalid");

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const session: TerminalSession = {
  id: "session-agent",
  cwd: "/repo/main",
  worktreePath: "/repo/main",
  workspaceId: "ws-main",
  worktree: { wsId: "ws-main", slug: "main" },
  backendSessionId: "backend-agent",
  lifecycle: "working",
  providerSession: { key: "session_id", id: "provider-dag-owner" },
};

/** Every entry point this todo gates, rendered through its real component. */
function renderShell() {
  return render(
    <>
      <DesktopActivityBar
        mode="worktrees"
        onModeChange={vi.fn()}
        sidebarOpen
        onToggleSidebar={vi.fn()}
        onOpenResources={vi.fn()}
        onOpenSettings={vi.fn()}
        isMac={false}
      />
      <Sidebar
        projects={[{ workspaceId: "ws-main", repoRoot: "/repo", gitRoot: "/repo" }]}
        activeProjectId="ws-main"
        worktrees={[worktree]}
        agents={[]}
        activePath={worktree.path}
        onSelectWorktree={vi.fn()}
        onCreateWorktree={vi.fn()}
        onFocusSupervisorSession={vi.fn()}
      />
      <EmptyWorkspaceView onNewTerminal={vi.fn()} onNewBrowserTab={vi.fn()} />
      <TerminalPane
        session={session}
        active
        activity={{ state: "working", title: "omo", isAgent: true, agentType: "omo" }}
      />
      <SettingsDialog open onClose={vi.fn()} />
    </>,
  );
}

function sidebarViews(): string[] {
  const tablist = screen.getByRole("tablist", { name: "Sidebar view" });
  return within(tablist).getAllByRole("tab").map((tab) => tab.getAttribute("aria-label") ?? tab.textContent ?? "");
}

function settingsSections(): string[] {
  const nav = screen.getByRole("navigation", { name: "Settings sections" });
  return within(nav).getAllByRole("button").map((button) => button.textContent ?? "");
}

function entryPoints() {
  return {
    activityModes: activityModes(),
    sidebarViews: sidebarViews(),
    settingsSections: settingsSections(),
    newBrowserTab: screen.queryByRole("button", { name: /new browser tab/i }) !== null,
    dagBadge: screen.queryByTestId("dag-pane-badge") !== null,
  };
}

/** The rail's modes, read from the real Activity Bar rather than from its exported constant. */
function activityModes(): string[] {
  const tablist = screen.getByRole("tablist", { name: "Activity" });
  return within(tablist).getAllByRole("tab").map((tab) => tab.getAttribute("aria-label") ?? tab.textContent ?? "");
}

// The Activity Bar is the shell's top-level navigation, so all four modes are core entry points:
// `advancedFeatures` gates the DAG badge, new browser tabs and advanced settings, never the rail.
const ACTIVITY_MODES = ["Supervisor", "Worktrees", "Browser", "Remote Desktop"];

const CORE_ENTRIES = {
  activityModes: ACTIVITY_MODES,
  // The Supervisor panel is the sidebar's default view; it is not behind advanced features.
  sidebarViews: ["Supervisor", "Worktrees"],
  // Voice is a core entry point: SettingsDialog renders it outside the advanced gate.
  // Chat is a core entry point: SettingsDialog renders it outside the advanced gate, right after
  // Keyboard Shortcuts. It holds the pane chat preferences (tool-call visibility, announcement
  // level) that every chat pane reads, so it must be reachable on a fresh install.
  settingsSections: ["General", "Appearance", "Terminal", "Keyboard Shortcuts", "Chat", "Agents", "Permissions", "Voice", "Remote"],
  newBrowserTab: false,
  dagBadge: false,
};

const ALL_ENTRIES = {
  activityModes: ACTIVITY_MODES,
  sidebarViews: ["Supervisor", "Worktrees"],
  settingsSections: [
    "General",
    "Appearance",
    "Terminal",
    "Keyboard Shortcuts",
    "Chat",
    "Agents",
    "Browser",
    "Notifications",
    "Permissions",
    "Voice",
    "Remote",
  ],
  newBrowserTab: true,
  dagBadge: true,
};

beforeEach(() => {
  localStorage.clear();
  setAdvancedFeaturesFallbackForTests(null);
  native.getTerminalPreferences.mockResolvedValue({
    fontFamily: "Noto Sans KR",
    macosOptionAsAlt: true,
    source: "ghostty",
    status: "imported",
    sourcePath: "/Users/test/.config/ghostty/config",
  });
  native.getCliLauncherStatus.mockResolvedValue({ isSupported: false, isInstalled: false });
  native.getDaemonSupervisor.mockResolvedValue({ supported: false, enabled: false });
  dagStore.reset();
  dagStore.applySnapshot("/repo/main", { ...parsedDag, rootSessionId: "provider-dag-owner", status: "running" });
});

afterEach(() => {
  cleanup();
  dagStore.reset();
  localStorage.clear();
});

describe("advanced features progressive disclosure", () => {
  it("is off on a fresh install and shows only the core entry points", () => {
    expect(loadAdvancedFeatures()).toBe(false);

    renderShell();

    expect(entryPoints()).toEqual(CORE_ENTRIES);
    expect(screen.getByRole("switch", { name: "Advanced features" })).not.toBeChecked();
    // Core surfaces stay: worktrees, terminal pane, remote and agents.
    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /new terminal/i })).toBeInTheDocument();
    // Browser mode is a core entry point even with advanced features off: only the browser *tab*
    // affordance is gated, never the rail mode that opens the Browser activity.
    expect(screen.getByTestId("activity-mode-browser")).toBeInTheDocument();
  });

  it("is on for an existing install with stored workspace state and shows every entry point", () => {
    localStorage.setItem(
      PROJECTS_STORAGE_KEY,
      JSON.stringify([{ workspaceId: "ws-main", repoRoot: "/repo", gitRoot: "/repo" }]),
    );
    expect(loadAdvancedFeatures()).toBe(true);

    renderShell();

    expect(entryPoints()).toEqual(ALL_ENTRIES);
    expect(screen.getByRole("switch", { name: "Advanced features" })).toBeChecked();
  });

  it("keeps the resolved default once persisted, so a later project does not flip it", () => {
    expect(loadAdvancedFeatures()).toBe(false);
    expect(localStorage.getItem(ADVANCED_FEATURES_STORAGE_KEY)).toBe("false");

    localStorage.setItem(PROJECTS_STORAGE_KEY, "[]");

    expect(loadAdvancedFeatures()).toBe(false);
  });

  it("toggling the General switch shows and hides the entries without a reload", () => {
    renderShell();
    const toggle = screen.getByRole("switch", { name: "Advanced features" });

    act(() => {
      fireEvent.click(toggle);
    });
    expect(entryPoints()).toEqual(ALL_ENTRIES);
    expect(localStorage.getItem(ADVANCED_FEATURES_STORAGE_KEY)).toBe("true");

    act(() => {
      fireEvent.click(screen.getByRole("switch", { name: "Advanced features" }));
    });
    expect(entryPoints()).toEqual(CORE_ENTRIES);
  });
});
