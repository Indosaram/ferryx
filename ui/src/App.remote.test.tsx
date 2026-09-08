import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RegisteredProject } from "./lib/tauri";
import type { RegisteredRemoteProject } from "./lib/remoteProject";

const native = vi.hoisted(() => ({
  registerProject: vi.fn(), registerRemoteProject: vi.fn(), listWorktrees: vi.fn(),
  spawnTerminalDetailed: vi.fn(), spawnTerminal: vi.fn(), watchDagProject: vi.fn(),
  loadSession: vi.fn(), saveSession: vi.fn(), isTauriRuntime: vi.fn(),
  remoteSelection: null as null | ((payload: import("./lib/tauri").RemoteSelectionRequestedPayload) => void),
  closeGuard: null as null | (() => Promise<void>),
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
  onNewTerminalTabMenu: async () => () => undefined,
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
import { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY, SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY } from "./lib/storageKeys";
import { resetWorkspaceRestore } from "./state/workspaceRestore";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
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
afterEach(cleanup);

describe("App SSH project lifecycle", () => {
  it("shows progress throughout registration, restore, and first SSH tab creation", async () => {
    // Given an inactive remote root and independently controlled connection stages.
    seed([{ workspaceId: "local", repoRoot: remote.repoRoot, gitRoot: null }, remote]);
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
    await mount();
    const registration = deferred<RegisteredRemoteProject>();
    const restore = deferred<null>();
    const spawn = deferred<string>();
    native.registerRemoteProject.mockReturnValueOnce(registration.promise);
    native.loadSession.mockReturnValueOnce(restore.promise);
    native.spawnTerminal.mockReturnValueOnce(spawn.promise);

    // When the user selects the SSH root, each unresolved stage stays busy.
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Expand repo (build)" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "/srv/repo SSH root" })); });
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("empty-workspace-view")).not.toBeInTheDocument();
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("empty-workspace-view")).not.toBeInTheDocument();
    await act(async () => { restore.resolve(null); await restore.promise; });
    expect(native.spawnTerminal).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("empty-workspace-view")).not.toBeInTheDocument();
    await act(async () => { spawn.resolve("connected"); await spawn.promise; });

    // Then the terminal replaces progress, without an extra tab.
    expect(screen.queryByTestId("ssh-workspace-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("active-tab")).not.toBeEmptyDOMElement();
    expect(native.spawnTerminal).toHaveBeenCalledTimes(1);
  });

  it("shows progress for manual first-tab creation and allows retry after failure", async () => {
    seed([remote]);
    await mount();
    expect(screen.getByTestId("empty-workspace-view")).toBeInTheDocument();
    const spawn = deferred<string>();
    native.spawnTerminal.mockReturnValueOnce(spawn.promise);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New Terminal" })); });
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("empty-workspace-view")).not.toBeInTheDocument();
    await act(async () => {
      spawn.reject({ code: "SSH_CONNECT_FAILED", message: "Connection refused" });
      await spawn.promise.catch(() => undefined);
    });
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "false");
    expect(screen.getByTestId("ssh-workspace-status")).toHaveTextContent("SSH_CONNECT_FAILED");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry connection" })); });
    expect(screen.queryByTestId("ssh-workspace-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("active-tab")).not.toBeEmptyDOMElement();
  });

  it("replaces registration failure with progress on explicit retry", async () => {
    seed([remote]);
    native.registerRemoteProject.mockRejectedValueOnce({ code: "SSH_CONNECT_FAILED", message: "Connection refused" });
    await mount();
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "false");
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValueOnce(registration.promise);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry connection" })); });
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(screen.queryByTestId("ssh-workspace-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("empty-workspace-view")).toBeInTheDocument();
  });

  it("does not leak a late SSH failure into the selected local workspace", async () => {
    seed([remote, { workspaceId: "local", repoRoot: "/local", gitRoot: null }]);
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValueOnce(registration.promise);
    await mount();
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "local" })); });
    await act(async () => {
      registration.reject({ code: "SSH_CONNECT_FAILED", message: "Connection refused" });
      await registration.promise.catch(() => undefined);
    });
    expect(screen.queryByTestId("ssh-workspace-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("empty-workspace-view")).toBeInTheDocument();
  });

  it("focuses the existing SSH terminal addressed by its backend identity", async () => {
    native.spawnTerminal.mockResolvedValueOnce("backend-first").mockResolvedValueOnce("backend-second");
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const first = screen.getByTestId("active-tab").textContent;
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New remote tab" })); });
    if (!native.remoteSelection) throw new Error("Missing remote session bridge");
    expect(native.spawnTerminal).toHaveBeenCalledTimes(2);
    expect(screen.getByTestId("active-tab").textContent).not.toBe(first);
    await act(async () => {
      native.remoteSelection?.({ workspaceId: remote.workspaceId, sessionId: "backend-first" });
    });
    expect(screen.getByTestId("active-tab").textContent).toBe(first);
  });

  it("loads a stored remote target without registering its path locally", async () => {
    seed([remote]);
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValue(registration.promise);
    await mount();
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(native.registerRemoteProject).toHaveBeenCalledWith({ workspaceId: remote.workspaceId, hostId: "build", repoPath: remote.repoRoot });
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(native.listWorktrees).not.toHaveBeenCalled();
    expect(native.watchDagProject).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /New Terminal/ })).toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!)[0].target).toEqual(remote.target);
  });

  it("returns from a local project to the requested existing SSH session", async () => {
    native.spawnTerminal.mockResolvedValueOnce("backend-ssh");
    seed([remote, { workspaceId: "local", repoRoot: "/local", gitRoot: null }]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const first = screen.getByTestId("active-tab").textContent;
    await act(async () => { native.remoteSelection?.({ workspaceId: "local" }); });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("local");
    await act(async () => {
      native.remoteSelection?.({ workspaceId: remote.workspaceId, sessionId: "backend-ssh" });
    });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(screen.getByTestId("active-tab").textContent).toBe(first);
    expect(native.spawnTerminal).toHaveBeenCalledTimes(1);
  });

  it("opens the registered SSH project from remote without a preexisting terminal", async () => {
    seed([{ workspaceId: "local", repoRoot: "/local", gitRoot: null }, remote], "local");
    await mount();
    await act(async () => { native.remoteSelection?.({ workspaceId: remote.workspaceId }); });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(native.spawnTerminal).toHaveBeenCalledWith(expect.objectContaining({
      workspaceId: remote.workspaceId, cwd: remote.repoRoot,
    }));
  });

  it("handles the chooser's registered remote project through actual App registration", async () => {
    seed([]); hosts.current = [{ id: "build", label: "Build machine", hostname: "build.example" }];
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValue(registration.promise);
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));
    fireEvent.click(screen.getByTestId("project-type-remote"));
    fireEvent.change(screen.getByTestId("remote-repo-path-input"), { target: { value: "/srv/input" } });
    await act(async () => { fireEvent.click(screen.getByTestId("add-project-confirm-remote")); });
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(native.registerRemoteProject).toHaveBeenLastCalledWith({ workspaceId: remote.workspaceId, hostId: "build", repoPath: remote.repoRoot });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(screen.getByRole("button", { name: /New Terminal/ })).toBeInTheDocument();
  });

  it("routes the chooser Settings CTA to SSH machines, not inbound Remote Access", async () => {
    seed([]);
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));
    fireEvent.click(screen.getByTestId("project-type-remote"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("configure-ssh-settings"));
      await import("./components/SettingsDialog");
    });
    expect(screen.getByTestId("settings-section")).toHaveTextContent("ssh");
    expect(screen.queryByRole("dialog", { name: "Add Project" })).not.toBeInTheDocument();
  });

  it("adopts the server's canonical host-qualified ID and path before new tabs and splits", async () => {
    seed([{ ...remote, workspaceId: "alias", repoRoot: "/srv/input" }]);
    await mount();
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!)).toEqual([remote]);
    const spawn = deferred<string>();
    native.spawnTerminal.mockReturnValueOnce(spawn.promise);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New Terminal" })); });
    expect(native.spawnTerminal).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, cwd: remote.repoRoot, worktree: null }));
    await act(async () => { spawn.resolve("remote-backend"); await spawn.promise; });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New remote tab" })); });
    expect(native.spawnTerminal).toHaveBeenCalledTimes(2);
    expect(native.spawnTerminal.mock.calls.every(([request]) => request.workspaceId === remote.workspaceId && request.cwd === remote.repoRoot)).toBe(true);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Split remote pane" })); });
    expect(native.spawnTerminalDetailed).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, worktree: null, cwd: null, inheritFromSessionId: "remote-backend" }));
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(native.listWorktrees).not.toHaveBeenCalled();
    expect(native.watchDagProject).not.toHaveBeenCalled();
  });

  it("selects inactive remote roots by workspace identity even when a local root has the same path", async () => {
    const local = { workspaceId: "local", repoRoot: remote.repoRoot, gitRoot: null };
    seed([local, remote]);
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
    await mount();
    const registration = deferred<RegisteredRemoteProject>();
    const restore = deferred<null>();
    native.loadSession.mockReturnValueOnce(restore.promise);
    native.registerRemoteProject.mockReturnValueOnce(registration.promise);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /build SSH/ })); });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    await act(async () => { restore.resolve(null); await restore.promise; });
    expect(native.spawnTerminal).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, cwd: remote.repoRoot }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "local" })); });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("local");
    expect(native.registerProject.mock.calls.every(([request]) => request.workspaceId === "local")).toBe(true);
    expect(native.listWorktrees.mock.calls.every(([workspaceId]) => workspaceId === "local")).toBe(true);
  });

  it("leaves a rejected host unavailable instead of spawning locally", async () => {
    seed([remote]);
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValue(registration.promise);
    await mount();
    await act(async () => { registration.reject({ code: "WORKSPACE_NOT_FOUND", message: "Host disabled" }); await registration.promise.catch(() => undefined); });
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.listWorktrees).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "New Terminal" })).not.toBeInTheDocument();
    native.registerRemoteProject.mockResolvedValue(registered);
    await act(async () => { window.dispatchEvent(new Event("focus")); });
    expect(native.registerRemoteProject).toHaveBeenCalledTimes(2);
    expect(native.registerRemoteProject).toHaveBeenLastCalledWith({ workspaceId: remote.workspaceId, hostId: "build", repoPath: remote.repoRoot });
    expect(screen.getByRole("button", { name: "New Terminal" })).toBeInTheDocument();
    expect(native.registerProject).not.toHaveBeenCalled();
  });

  it("ignores remote registration that resolves after switching to a local project", async () => {
    seed([remote, { workspaceId: "local", repoRoot: "/local", gitRoot: null }]);
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValue(registration.promise);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "local" })); });
    await act(async () => { registration.resolve({ ...registered, workspaceId: "ssh:changed", repoRoot: "/remote/changed" }); await registration.promise; });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("local");
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!).some((project: RegisteredProject) => project.workspaceId === "ssh:changed")).toBe(false);
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.registerProject.mock.calls.every(([request]) => request.workspaceId === "local")).toBe(true);
  });

  it.each([{ kind: "ssh" }, { kind: "ssh", hostId: "" }, null, { kind: "unknown" }])("rejects malformed stored targets without converting them to local: %j", async (target) => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ ...remote, target }]));
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      await mount();
      expect(native.registerProject).not.toHaveBeenCalled();
      expect(native.registerRemoteProject).not.toHaveBeenCalled();
      expect(screen.getByTestId("no-projects-view")).toBeInTheDocument();
      expect(error).toHaveBeenCalled();
    } finally { error.mockRestore(); }
  });

  it("restores remote sessions after native startup and preserves their target in the next save", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const saved: import("./lib/types").PersistedWorkspaceSession = {
      version: 2, timestamp: 1, activeWorkspaceId: remote.workspaceId,
      workspaces: { [remote.workspaceId]: {
        ...remote,
        worktrees: [{ path: remote.repoRoot, branch: "", head: "", isMain: true, isLocked: false }],
        activeWorktreePath: remote.repoRoot,
        layout: { tabs: [{ id: "saved-tab", label: "shell", sessionId: "saved-session" }], activeTabId: "saved-tab", splitMode: "none", primaryTabId: "saved-tab", secondaryTabId: null },
        terminalSessions: { "saved-session": { sessionId: "old-backend", localSessionId: "saved-session", backendSessionId: "old-backend", cwd: remote.repoRoot, worktreePath: remote.repoRoot, createdAt: 1 } },
      } },
    };
    native.loadSession.mockResolvedValue(saved);
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValue(registration.promise);
    await mount();
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(native.spawnTerminal).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, cwd: remote.repoRoot }));
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!).find((project: RegisteredProject) => project.workspaceId === remote.workspaceId).target).toEqual(remote.target);
    expect(native.closeGuard).not.toBeNull();
    await act(async () => { await native.closeGuard!(); });
    expect(native.saveSession).toHaveBeenCalled();
    const latest = native.saveSession.mock.calls.at(-1)![0];
    expect(latest.workspaces[remote.workspaceId].target).toEqual(remote.target);
  });
});
