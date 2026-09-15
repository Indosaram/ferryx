import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RegisteredProject } from "./lib/tauri";
import type { RegisteredRemoteProject } from "./lib/remoteProject";

const deletion = vi.hoisted(() => ({
  createPairedWorktreeActions: vi.fn(), previewDelete: vi.fn(), deleteSafe: vi.fn(),
  previewWorktreeDelete: vi.fn(), deleteWorktree: vi.fn(), deleteWorktreeDestructive: vi.fn(),
}));
vi.mock("./lib/pairedWorktreeActions", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/pairedWorktreeActions")>(),
  createPairedWorktreeActions: deletion.createPairedWorktreeActions,
}));
vi.mock("./components/Sidebar", () => ({ Sidebar: ({ onDeleteWorktree }: {
  onDeleteWorktree: (row: import("./lib/types").Worktree) => void;
}) => <button onClick={() => onDeleteWorktree(pairedWorktree)}>Delete paired fixture</button> }));

const native = vi.hoisted(() => ({
  registerProject: vi.fn(), registerRemoteProject: vi.fn(), listWorktrees: vi.fn(),
  spawnTerminalDetailed: vi.fn(), spawnTerminal: vi.fn(), watchDagProject: vi.fn(),
  loadSession: vi.fn(), saveSession: vi.fn(), isTauriRuntime: vi.fn(),
  remoteSelection: null as null | ((payload: import("./lib/tauri").RemoteSelectionRequestedPayload) => void),
  closeGuard: null as null | (() => Promise<void>),
  newTab: null as null | (() => void),
  split: null as null | (() => void),
  palette: null as null | (() => void),
}));
vi.mock("./lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/tauri")>(),
  ...native,
  previewWorktreeDelete: deletion.previewWorktreeDelete,
  deleteWorktree: deletion.deleteWorktree,
  deleteWorktreeDestructive: deletion.deleteWorktreeDestructive,
  getInitialProject: async () => ({ workspaceId: "local", repoRoot: "/local", gitRoot: null }),
  detectAgents: async () => [],
  listTerminalSessions: async () => [],
  ensureTerminalEvents: async () => undefined,
  onWorktreeChanged: async () => () => undefined,
  onTerminalLifecycle: async () => () => undefined,
  onTerminalOutput: async () => () => undefined,
  onNewTerminalTabMenu: async (handler: () => void) => { native.newTab = handler; return () => { native.newTab = null; }; },
  onSplitRightMenu: async (handler: () => void) => { native.split = handler; return () => { native.split = null; }; },
  onCommandPaletteMenu: async (handler: () => void) => { native.palette = handler; return () => { native.palette = null; }; },
  onCloseTabMenu: async () => () => undefined,
  onSelectWorktreeMenu: async () => () => undefined,
  onRemoteSelectionRequested: async (handler: (payload: import("./lib/tauri").RemoteSelectionRequestedPayload) => void) => {
    native.remoteSelection = handler;
    return () => { native.remoteSelection = null; };
  },
  listenDagRunUpdated: async () => () => undefined,
  publishFocusedTerminal: async () => undefined,
  setBadgeCount: async () => ({ supported: false, count: 0 }),
}));
vi.mock("./lib/remoteDirectories", () => ({
  listRemoteDirectories: vi.fn(async (_host: string, path: string | null) => ({
    path: path ?? "/srv/repo",
    parentPath: null,
    homePath: "/home/ubuntu",
    entries: [],
    truncated: false,
  })),
}));
vi.mock("./lib/remoteProject", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/remoteProject")>(),
  registerRemoteProject: native.registerRemoteProject,
}));
vi.mock("./lib/sshHosts", async (importOriginal) => ({ ...await importOriginal<typeof import("./lib/sshHosts")>(), useSshHosts: () => ({
  hosts: hosts.current, loading: false, error: null, refresh: async () => hosts.current,
}) }));
const hosts = vi.hoisted(() => ({ current: [] as Array<{ id: string; label: string; hostname: string }> }));
vi.mock("./lib/terminalEvents", async (importOriginal) => ({ ...await importOriginal<typeof import("./lib/terminalEvents")>(), ensureTerminalEvents: async () => undefined }));
vi.mock("./lib/updater", () => ({ startUpdatePolling: () => undefined, registerWindowCloseGuard: (guard: () => Promise<void>) => { native.closeGuard = guard; return () => { native.closeGuard = null; }; } }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ isFocused: async () => true, onFocusChanged: async () => () => undefined }) }));
vi.mock("./lib/updateToast", () => ({ initUpdateToasts: () => () => undefined }));
vi.mock("./components/SettingsDialog", () => ({ SettingsDialog: ({ initialSection }: { initialSection?: string }) => <div data-testid="settings-section">{initialSection}</div> }));
vi.mock("./components/TerminalSplitView", () => ({ TerminalSplitView: ({ onAddTab, onSplitPane, layout }: {
  onAddTab: () => void;
  onSplitPane: (tabId: string, leafId: string, direction: "horizontal") => void;
  layout: import("./lib/types").LayoutState;
}) => <div>
  <output data-testid="active-tab">{layout.activeTabId}</output>
  <button onClick={() => onAddTab()}>New remote tab</button>
  <button onClick={() => {
    const tab = layout.tabs[0];
    const leaf = layout.layoutsByTabId![tab.id].activeLeafId!;
    onSplitPane(tab.id, leaf, "horizontal");
  }}>Split remote pane</button>
</div> }));

import { App } from "./App";
import { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY } from "./lib/storageKeys";
import { resetWorkspaceRestore } from "./state/workspaceRestore";
import { isMacShortcutPlatform } from "./lib/shortcuts";

const remote: RegisteredProject = { workspaceId: "ssh:remote", repoRoot: "/srv/repo", gitRoot: "/srv/repo", target: { kind: "ssh", hostId: "build" } };
const registered: RegisteredRemoteProject = { workspaceId: remote.workspaceId, repoRoot: remote.repoRoot, gitRoot: remote.gitRoot!, hostId: "build", hostLabel: "Build machine" };
function seed(projects: RegisteredProject[], active = projects[0]?.workspaceId) {
  localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
  if (active) localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, active);
}
async function mount() { await act(async () => { render(<App />); }); }
beforeEach(() => {
  localStorage.clear(); resetWorkspaceRestore(); vi.clearAllMocks(); hosts.current = [];
  native.isTauriRuntime.mockReturnValue(false);
  native.registerProject.mockImplementation(async (request: { workspaceId: string; repoPath: string }) => ({ workspaceId: request.workspaceId, repoRoot: request.repoPath, gitRoot: null }));
  native.registerRemoteProject.mockResolvedValue(registered);
  native.listWorktrees.mockResolvedValue([]);
  native.loadSession.mockResolvedValue(null); native.saveSession.mockResolvedValue(undefined);
  native.watchDagProject.mockImplementation(async (projectPath: string) => ({ projectPath, runs: [] }));
  native.spawnTerminal.mockResolvedValue("backend-new");
  native.spawnTerminalDetailed.mockImplementation(async (request: { cwd?: string }) => ({ sessionId: `backend-${native.spawnTerminalDetailed.mock.calls.length}`, daemonEpoch: "epoch", session: { cwd: request.cwd ?? remote.repoRoot } }));
});
afterEach(() => { cleanup(); remoteHostStore.reset(); });


import { remoteHostStore } from "./state/remoteHostStore";
const paired: RegisteredProject = { workspaceId: `daemon:${"a".repeat(64)}`, repoRoot: "/srv/repo", gitRoot: null,
  target: { kind: "pairedDaemon", hostId: "host-a" }, remoteWorkspaceId: "remote-project" };

const pairedWorktree: import("./lib/types").Worktree = {
  workspaceId: paired.workspaceId, path: "/srv/repo/feature", head: "abc123",
  branch: "refs/heads/orca/remote-project/feature", bare: false, detached: false, locked: null, prunable: null,
};

describe("App paired desktop shell", () => {
  it("routes deletion of an inactive paired project's worktree through its owning project", async () => {
    seed([{ workspaceId: "local", repoRoot: "/local", gitRoot: null }, paired], "local");
    deletion.previewDelete.mockResolvedValue({ branch: "orca/remote-project/feature", head: "abc123", upstream: null, merged: true, ahead: null, behind: null });
    deletion.deleteSafe.mockResolvedValue(undefined);
    deletion.createPairedWorktreeActions.mockReturnValue({ previewDelete: deletion.previewDelete, deleteSafe: deletion.deleteSafe, deleteDestructive: vi.fn() });
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete paired fixture" })); });
    // On the broken App call site this renders PAIRED_OWNER_REQUIRED instead.
    expect(deletion.createPairedWorktreeActions).toHaveBeenCalledWith(paired);
    expect(deletion.previewDelete).toHaveBeenCalledWith(pairedWorktree);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete remote worktree" })); });
    expect(deletion.deleteSafe).toHaveBeenCalledWith(pairedWorktree);
    expect(screen.queryByRole("dialog", { name: "Delete worktree" })).not.toBeInTheDocument();
    expect(deletion.previewWorktreeDelete).not.toHaveBeenCalled();
    expect(deletion.deleteWorktree).not.toHaveBeenCalled();
    expect(deletion.deleteWorktreeDestructive).not.toHaveBeenCalled();
  });

  it("preserves paired ownership and reports disabled native support without local registration", async () => {
    seed([paired]);
    await mount();
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(native.registerRemoteProject).not.toHaveBeenCalled();
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!)[0]).toMatchObject(paired);
    await act(async () => {
      fireEvent.keyDown(window, { key: "t", code: "KeyT", metaKey: isMacShortcutPlatform(), ctrlKey: !isMacShortcutPlatform() });
      native.newTab!();
      native.split!();
    });
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
  });

  it("keeps local tabs and the palette mounted when inventory selection changes", async () => {
    seed([{ workspaceId: "local", repoRoot: "/local", gitRoot: null }]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New Terminal" })); });
    const tab = screen.getByTestId("active-tab").textContent;
    expect(native.palette).toBeTypeOf("function");
    await act(async () => { native.palette!(); });
    const palette = screen.getByRole("dialog");
    const main = screen.getByRole("main");
    await act(async () => { remoteHostStore.setActiveHost("host-a"); });
    expect(screen.getByRole("main")).toBe(main);
    expect(screen.getByRole("dialog")).toBe(palette);
    expect(screen.getByTestId("active-tab").textContent).toBe(tab);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New remote tab" })); });
    expect(native.spawnTerminal).toHaveBeenCalledTimes(2);
    expect(native.newTab).toBeTypeOf("function");
    await act(async () => { native.newTab!(); });
    expect(native.spawnTerminal).toHaveBeenCalledTimes(3);
    expect(native.split).toBeTypeOf("function");
    await act(async () => { native.split!(); });
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
    expect(native.spawnTerminalDetailed).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: "local" }));
    await act(async () => { fireEvent.keyDown(window, { key: "Escape", code: "Escape" }); });
    await act(async () => {
      fireEvent.keyDown(window, { key: "t", code: "KeyT", metaKey: isMacShortcutPlatform(), ctrlKey: !isMacShortcutPlatform() });
    });
    expect(native.spawnTerminal).toHaveBeenCalledTimes(4);
  });
});
