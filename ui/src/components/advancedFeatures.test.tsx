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
      <Sidebar
        projects={[{ workspaceId: "ws-main", repoRoot: "/repo", gitRoot: "/repo" }]}
        activeProjectId="ws-main"
        worktrees={[worktree]}
        agents={[]}
        activePath={worktree.path}
        onSelectWorktree={vi.fn()}
        onCreateWorktree={vi.fn()}
        attention={{
          rows: [],
          onOpen: vi.fn(),
          onDismiss: vi.fn(),
          onDismissAll: vi.fn(),
          inboxOpen: false,
          onInboxOpenChange: vi.fn(),
        }}
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
    sidebarViews: sidebarViews(),
    settingsSections: settingsSections(),
    newBrowserTab: screen.queryByRole("button", { name: /new browser tab/i }) !== null,
    dagBadge: screen.queryByTestId("dag-pane-badge") !== null,
  };
}

const CORE_ENTRIES = {
  sidebarViews: ["Worktrees", "Inbox"],
  settingsSections: ["General", "Appearance", "Terminal", "Keyboard Shortcuts", "Agents", "Permissions", "Remote"],
  newBrowserTab: false,
  dagBadge: false,
};

const ALL_ENTRIES = {
  sidebarViews: ["Worktrees", "Inbox", "Supervisor"],
  settingsSections: [
    "General",
    "Appearance",
    "Terminal",
    "Keyboard Shortcuts",
    "Agents",
    "Browser",
    "Notifications",
    "Permissions",
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
