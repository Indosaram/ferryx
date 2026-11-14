import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RegisteredProject } from "./lib/tauri";
import type { RegisteredRemoteProject } from "./lib/remoteProject";

const native = vi.hoisted(() => ({
  registerProject: vi.fn(), registerRemoteProject: vi.fn(), listWorktrees: vi.fn(),
  spawnTerminalDetailed: vi.fn(), spawnTerminal: vi.fn(), watchDagProject: vi.fn(),
  loadSession: vi.fn(), saveSession: vi.fn(), isTauriRuntime: vi.fn(),
  getTerminalRemoteStatus: vi.fn<typeof import("./lib/tauri").getTerminalRemoteStatus>(),
  retryTerminalRemoteSession: vi.fn(),
  onTerminalRemoteStatus: vi.fn(),
  remoteStatusHandler: null as null | ((status: import("./lib/types").TerminalRemoteStatus) => void),
  reconnect: null as null | ((sessionId: string) => Promise<void>),
  refresh: null as null | ((sessionId: string, onResult: (result: import("./state/workspaceStore").LocalRecoveryAdmissionResult) => void) => () => void),
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
  onTerminalRemoteStatus: (...args: any[]) => native.onTerminalRemoteStatus(...args),
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
vi.mock("./lib/updater", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/updater")>(),
  getCurrentVersion: async () => "2026.1004.1",
  subscribeUpdateStatus: () => () => undefined,
  startUpdatePolling: () => undefined,
  stopUpdatePolling: () => undefined,
  registerWindowCloseGuard: (guard: () => Promise<void>) => { native.closeGuard = guard; return () => { native.closeGuard = null; }; },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ isFocused: async () => true, onFocusChanged: async () => () => undefined }) }));
vi.mock("./lib/updateToast", () => ({ initUpdateToasts: () => () => undefined }));
let appRebindHandler: ((sessionId: string, backendSessionId: string, cwd?: string, daemonEpoch?: string | null, incarnation?: string | null) => Promise<void>) | null = null;
vi.mock("./lib/sessionLifecycle", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./lib/sessionLifecycle")>();
  return {
    ...actual,
    setSessionRebindHandler: (handler: any) => {
      appRebindHandler = handler;
      actual.setSessionRebindHandler(handler);
    },
  };
});
const toastMocks = vi.hoisted(() => ({ warning: vi.fn() }));
vi.mock("./components/ui/sonner", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./components/ui/sonner")>();
  return { ...actual, toast: Object.assign(actual.toast, { warning: toastMocks.warning }) };
});
vi.mock("./components/SettingsDialog", () => ({ SettingsDialog: ({ initialSection, onOpenMachineProject }: {
  initialSection?: string;
  onOpenMachineProject?: (target: import("./lib/machineNavigation").MachineProjectTarget, context: import("./lib/machineNavigation").RemoteContext) => void;
}) => <div>
  <div data-testid="settings-section">{initialSection}</div>
  <button onClick={() => onOpenMachineProject?.({ kind: "pairedDaemon", hostId: "h", generation: "1" }, { page: "machines", filter: "all" })}>Open paired project</button>
</div> }));
vi.mock("./components/TerminalSplitView", () => ({ TerminalSplitView: ({ onAddTab, onSplitPane, layout, onReconnectSshSession, onRefreshSessionIdentity }: {
  onAddTab: () => void;
  onSplitPane: (tabId: string, leafId: string, direction: "horizontal") => void;
  layout: import("./lib/types").LayoutState;
  onReconnectSshSession: (sessionId: string) => Promise<void>;
  onRefreshSessionIdentity: (sessionId: string, onResult: (result: import("./state/workspaceStore").LocalRecoveryAdmissionResult) => void) => () => void;
}) => { native.reconnect = onReconnectSshSession; native.refresh = onRefreshSessionIdentity; return <div>
  <output data-testid="active-tab">{layout.activeTabId}</output>
  <button onClick={() => onAddTab()}>New remote tab</button>
  <button onClick={() => {
    const tab = layout.tabs[0];
    const leaf = layout.layoutsByTabId![tab.id].activeLeafId!;
    onSplitPane(tab.id, leaf, "horizontal");
  }}>Split remote pane</button>
</div>; } }));

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
// 7e8fdfb7 uses detailed spawn for new tabs so their durable identity is available immediately.
beforeEach(() => {
  localStorage.clear(); resetWorkspaceRestore(); vi.clearAllMocks(); hosts.current = [];
  // clearAllMocks keeps implementations; a connected status from one test must not settle the next.
  native.getTerminalRemoteStatus.mockReset();
  native.isTauriRuntime.mockReturnValue(false);
  const remoteStatusListeners = new Set<(status: import("./lib/types").TerminalRemoteStatus) => void>();
  native.onTerminalRemoteStatus.mockImplementation(async (handler: (status: import("./lib/types").TerminalRemoteStatus) => void) => {
    remoteStatusListeners.add(handler);
    native.remoteStatusHandler = (status: import("./lib/types").TerminalRemoteStatus) => {
      for (const listener of Array.from(remoteStatusListeners)) listener(status);
    };
    return () => {
      remoteStatusListeners.delete(handler);
      if (remoteStatusListeners.size === 0) native.remoteStatusHandler = null;
    };
  });
  native.registerProject.mockImplementation(async (request: { workspaceId: string; repoPath: string }) => ({ workspaceId: request.workspaceId, repoRoot: request.repoPath, gitRoot: null }));
  native.registerRemoteProject.mockResolvedValue(registered);
  native.listWorktrees.mockResolvedValue([]);
  native.loadSession.mockResolvedValue(null); native.saveSession.mockResolvedValue(undefined);
  native.watchDagProject.mockImplementation(async (projectPath: string) => ({ projectPath, runs: [] }));
  native.spawnTerminal.mockResolvedValue("backend-new");
  native.spawnTerminalDetailed.mockImplementation(async (request: { cwd?: string }) => {
    const id = `backend-${native.spawnTerminalDetailed.mock.calls.length}`;
    return {
      sessionId: id,
      daemonEpoch: "epoch",
      session: { sessionId: id, incarnation: `life:${id}`, cwd: request.cwd ?? remote.repoRoot, cols: 80, rows: 24, running: true },
    };
  });
});
afterEach(cleanup);

describe("App SSH project lifecycle", () => {
  it.each(["rejected", "unexpected"])("retains the SSH backend when retry is %s without spawning a shell", async mode => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const snapshot = (await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!;
    const retained = Object.values(snapshot.sessions)[0];
    const spawned = native.spawnTerminalDetailed.mock.calls.length;
    const retry = deferred<{ type: string }>();
    native.retryTerminalRemoteSession.mockReturnValue(retry.promise);
    let first!: Promise<void>;
    let second!: Promise<void>;
    await act(async () => {
      first = native.reconnect!(retained.id); second = native.reconnect!(retained.id);
      const failure = first.catch(error => error);
      const secondFailure = second.catch(error => error);
      if (mode === "rejected") retry.reject({ code: "SSH_CONNECT_FAILED", message: "offline" });
      else retry.resolve({ type: "unexpected" });
      expect(await failure).toMatchObject({ code: mode === "rejected" ? "SSH_CONNECT_FAILED" : "INVALID_RESPONSE" });
      expect(await secondFailure).toMatchObject({ code: mode === "rejected" ? "SSH_CONNECT_FAILED" : "INVALID_RESPONSE" });
    });
    expect(second).toBe(first);
    expect(native.retryTerminalRemoteSession).toHaveBeenCalledWith(retained.backendSessionId);
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(spawned);
    expect((await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!.sessions[retained.id].backendSessionId).toBe(retained.backendSessionId);
  });
  it("routes SSH identity Retry through the coalesced remote owner and fences the result", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const snapshot = (await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!;
    const retained = Object.values(snapshot.sessions)[0];
    const retry = deferred<{ type: string }>();
    native.retryTerminalRemoteSession.mockReturnValue(retry.promise);
    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await act(async () => {
      native.refresh!(retained.id, result => results.push(result));
    });
    expect(results).toEqual([{ kind: "pending" }]);
    expect(native.retryTerminalRemoteSession).toHaveBeenCalledWith(retained.backendSessionId);
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
    await act(async () => { retry.resolve({ type: "retryRemoteSessionOk" }); await retry.promise; });
    expect(results).toEqual([{ kind: "pending" }]);
    await act(async () => { native.remoteStatusHandler!({ sessionId: retained.backendSessionId!, state: "connected", generation: 4, failure: null, replayGap: null }); });
    expect(results.at(-1)).toMatchObject({ kind: "ready", session: {
      id: retained.id, backendSessionId: retained.backendSessionId,
      daemonEpoch: retained.daemonEpoch, incarnation: retained.incarnation,
      remoteConnectionState: "connected", remoteGeneration: 4,
    } });
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
  });
  const connectedDetails = (backendSessionId: string, generation: number): import("./lib/types").RemoteSessionStatusResponse => ({
    type: "remoteSessionDetailsOk", legacyDirectSsh: false,
    details: {
      state: "connected", generation, failure: null, replayGap: null, attempts: 0, pid: 1234,
      descriptor: {
        backendSessionId,
        target: { hostId: "build", ownerId: "owner", backendSessionId: "remote-process", epoch: "1" },
        config: {}, clientRequestId: "request", remoteCursor: "0", cols: 80, rows: 24,
      },
    },
  });
  it("settles SSH identity Retry from the current status when the daemon already holds the session connected", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const retained = Object.values((await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!.sessions)[0];
    // Retry on a connected session is a daemon no-op: no terminal_remote_status event ever follows.
    native.retryTerminalRemoteSession.mockResolvedValue({ type: "retryRemoteSessionOk" });
    native.getTerminalRemoteStatus.mockResolvedValue(connectedDetails(retained.backendSessionId!, 7));
    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await act(async () => { native.refresh!(retained.id, result => results.push(result)); });
    await act(async () => { await Promise.resolve(); });
    expect(native.getTerminalRemoteStatus).toHaveBeenCalledWith(retained.backendSessionId);
    expect(results).toEqual([{ kind: "pending" }, { kind: "ready", session: expect.objectContaining({
      id: retained.id, backendSessionId: retained.backendSessionId,
      remoteConnectionState: "connected", remoteGeneration: 7,
    }) }]);
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
  });
  it("settles SSH identity Retry exactly once when the status event races the current-status read", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const retained = Object.values((await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!.sessions)[0];
    native.retryTerminalRemoteSession.mockResolvedValue({ type: "retryRemoteSessionOk" });
    const status = deferred<import("./lib/types").RemoteSessionStatusResponse>();
    native.getTerminalRemoteStatus.mockReturnValue(status.promise);
    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await act(async () => { native.refresh!(retained.id, result => results.push(result)); });
    expect(native.getTerminalRemoteStatus).toHaveBeenCalledTimes(1);
    await act(async () => { native.remoteStatusHandler!({ sessionId: retained.backendSessionId!, state: "connected", generation: 5, failure: null, replayGap: null }); });
    await act(async () => { status.resolve(connectedDetails(retained.backendSessionId!, 9)); await status.promise; });
    expect(results.filter(result => result.kind === "ready")).toHaveLength(1);
    expect(results.at(-1)).toMatchObject({ kind: "ready", session: { remoteGeneration: 5 } });
  });
  it("keeps waiting for the status event when the current status after Retry is not connected", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const retained = Object.values((await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!.sessions)[0];
    native.retryTerminalRemoteSession.mockResolvedValue({ type: "retryRemoteSessionOk" });
    const reconnecting = connectedDetails(retained.backendSessionId!, 7);
    native.getTerminalRemoteStatus.mockResolvedValue({ ...reconnecting, details: { ...reconnecting.details!, state: "reconnecting" } });
    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await act(async () => { native.refresh!(retained.id, result => results.push(result)); });
    await act(async () => { await Promise.resolve(); });
    expect(native.getTerminalRemoteStatus).toHaveBeenCalledTimes(1);
    expect(results).toEqual([{ kind: "pending" }]);
    await act(async () => { native.remoteStatusHandler!({ sessionId: retained.backendSessionId!, state: "connected", generation: 8, failure: null, replayGap: null }); });
    expect(results).toEqual([{ kind: "pending" }, { kind: "ready", session: expect.objectContaining({ remoteConnectionState: "connected", remoteGeneration: 8 }) }]);
  });
  it("keeps waiting for the status event when the current-status read after Retry fails", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const retained = Object.values((await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!.sessions)[0];
    native.retryTerminalRemoteSession.mockResolvedValue({ type: "retryRemoteSessionOk" });
    native.getTerminalRemoteStatus.mockRejectedValue({ code: "DAEMON_UNAVAILABLE", message: "status read failed" });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await act(async () => { native.refresh!(retained.id, result => results.push(result)); });
    await act(async () => { await Promise.resolve(); });
    expect(warn).toHaveBeenCalledWith("SSH status read after retry failed; waiting for status event", { code: "DAEMON_UNAVAILABLE", message: "status read failed" });
    warn.mockRestore();
    expect(results).toEqual([{ kind: "pending" }]);
    await act(async () => { native.remoteStatusHandler!({ sessionId: retained.backendSessionId!, state: "connected", generation: 6, failure: null, replayGap: null }); });
    expect(results).toEqual([{ kind: "pending" }, { kind: "ready", session: expect.objectContaining({ remoteConnectionState: "connected", remoteGeneration: 6 }) }]);
  });
  it("never settles a Retry cancelled while its current-status read is in flight", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const retained = Object.values((await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!.sessions)[0];
    native.retryTerminalRemoteSession.mockResolvedValue({ type: "retryRemoteSessionOk" });
    const status = deferred<import("./lib/types").RemoteSessionStatusResponse>();
    native.getTerminalRemoteStatus.mockReturnValue(status.promise);
    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let cancel!: () => void;
    await act(async () => { cancel = native.refresh!(retained.id, result => results.push(result)); });
    expect(native.getTerminalRemoteStatus).toHaveBeenCalledTimes(1);
    cancel();
    await act(async () => { status.resolve(connectedDetails(retained.backendSessionId!, 9)); await status.promise; });
    expect(results).toEqual([{ kind: "pending" }]);
  });
  it("suppresses a cancelled SSH Retry callback and cancels when its owner tuple changes", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const snapshot = (await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!;
    const retained = Object.values(snapshot.sessions)[0];
    const retry = deferred<{ type: string }>();
    native.retryTerminalRemoteSession.mockReturnValue(retry.promise);
    const cancelledResults: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let cancel!: () => void;
    await act(async () => { cancel = native.refresh!(retained.id, result => cancelledResults.push(result)); });
    cancel();
    await act(async () => { retry.resolve({ type: "retryRemoteSessionOk" }); await retry.promise; });
    await act(async () => { if (native.remoteStatusHandler) native.remoteStatusHandler({ sessionId: retained.backendSessionId!, state: "connected", generation: 5, failure: null, replayGap: null }); });
    expect(cancelledResults).toEqual([{ kind: "pending" }]);

    const reboundRetry = deferred<{ type: string }>();
    native.retryTerminalRemoteSession.mockReturnValue(reboundRetry.promise);
    const reboundResults: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    native.refresh!(retained.id, result => reboundResults.push(result));
    await act(async () => {
      await appRebindHandler!(retained.id, retained.backendSessionId!, retained.cwd, "new-epoch");
    });
    await act(async () => { reboundRetry.resolve({ type: "retryRemoteSessionOk" }); await reboundRetry.promise; });
    await act(async () => { native.remoteStatusHandler!({ sessionId: retained.backendSessionId!, state: "connected", generation: 6, failure: null, replayGap: null }); });
    expect(reboundResults).toEqual([{ kind: "pending" }, { kind: "cancelled" }]);
    expect(native.remoteStatusHandler).toBeNull();
  });
  it("creates a new request when owner tuple changes and rejects old operation with RECOVERY_OWNER_CHANGED", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const snapshot = (await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!;
    const retained = Object.values(snapshot.sessions)[0];
    const retry = deferred<{ type: string }>();
    native.retryTerminalRemoteSession.mockReturnValue(retry.promise);

    let first!: Promise<void>;
    act(() => { first = native.reconnect!(retained.id); });
    const firstError = first.catch(err => err);

    await act(async () => {
      await appRebindHandler!(retained.id, retained.backendSessionId!, retained.cwd, "epoch-mutated");
    });

    let second!: Promise<void>;
    act(() => { second = native.reconnect!(retained.id); });
    expect(second).not.toBe(first);
    const secondOutcome = second.then(() => "ok", err => err);

    await act(async () => { retry.resolve({ type: "retryRemoteSessionOk" }); await retry.promise; });
    await act(async () => {
      if (native.remoteStatusHandler) {
        native.remoteStatusHandler({ sessionId: retained.backendSessionId!, state: "connected", generation: 10, failure: null, replayGap: null });
      }
    });

    expect(await firstError).toMatchObject({ code: "RECOVERY_OWNER_CHANGED" });
    expect(await secondOutcome).toBe("ok");
  });
  it("preserves rebound SSH owner operation when earlier captured cancel is invoked and resolves ready", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const snapshot = (await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!;
    const retained = Object.values(snapshot.sessions)[0];
    const oldRetry = deferred<{ type: string }>();
    const newRetry = deferred<{ type: string }>();
    native.retryTerminalRemoteSession
      .mockReturnValueOnce(oldRetry.promise)
      .mockReturnValueOnce(newRetry.promise);

    let oldReconnect!: Promise<void>;
    act(() => { oldReconnect = native.reconnect!(retained.id); });
    const oldOutcome = oldReconnect.catch(err => err);

    const oldResults: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let oldCancel!: () => void;
    await act(async () => {
      oldCancel = native.refresh!(retained.id, res => oldResults.push(res));
    });
    expect(oldResults).toEqual([{ kind: "pending" }]);

    await act(async () => {
      await appRebindHandler!(retained.id, retained.backendSessionId!, retained.cwd, "epoch-rebound");
    });

    let newReconnect!: Promise<void>;
    act(() => { newReconnect = native.reconnect!(retained.id); });
    const newOutcome = newReconnect.then(() => "ok", err => err);

    const newResults: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await act(async () => {
      native.refresh!(retained.id, res => newResults.push(res));
    });
    expect(newResults).toEqual([{ kind: "pending" }]);

    oldCancel();

    await act(async () => {
      oldRetry.resolve({ type: "retryRemoteSessionOk" });
      await oldRetry.promise;
    });
    await act(async () => {
      newRetry.resolve({ type: "retryRemoteSessionOk" });
      await newRetry.promise;
    });

    expect(native.remoteStatusHandler).not.toBeNull();
    await act(async () => {
      native.remoteStatusHandler!({
        sessionId: retained.backendSessionId!,
        state: "connected",
        generation: 7,
        failure: null,
        replayGap: null,
      });
    });

    // A pane disposing its refresh no longer cancels the button's wait; the old tuple ends
    // because its owner was rebound.
    expect(await oldOutcome).toMatchObject({ code: "RECOVERY_OWNER_CHANGED" });
    expect(oldResults).toEqual([{ kind: "pending" }]);
    expect(await newOutcome).toBe("ok");
    expect(newResults.at(-1)).toMatchObject({
      kind: "ready",
      session: {
        id: retained.id,
        backendSessionId: retained.backendSessionId,
        daemonEpoch: "epoch-rebound",
        remoteConnectionState: "connected",
        remoteGeneration: 7,
      },
    });
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
  });
  it("keeps the Reconnect SSH wait alive when a pane identity refresh on the same owner is disposed", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const retained = Object.values((await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!.sessions)[0];
    const retry = deferred<{ type: string }>();
    native.retryTerminalRemoteSession.mockReturnValue(retry.promise);
    let reconnect!: Promise<void>;
    let disposePane!: () => void;
    await act(async () => {
      disposePane = native.refresh!(retained.id, () => undefined);
      reconnect = native.reconnect!(retained.id);
    });
    const outcome = reconnect.then(() => "ok", error => error);
    // The pane effect re-runs (state flip) and disposes its refresh while the button still waits.
    disposePane();
    await act(async () => { retry.resolve({ type: "retryRemoteSessionOk" }); await retry.promise; });
    await act(async () => { native.remoteStatusHandler!({ sessionId: retained.backendSessionId!, state: "connected", generation: 3, failure: null, replayGap: null }); });
    expect(await outcome).toBe("ok");
    expect(native.retryTerminalRemoteSession).toHaveBeenCalledTimes(1);
  });
  it("aborts before retryTerminalRemoteSession when cancelled during listener registration", async () => {
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const snapshot = (await import("./state/workspaceSnapshotCache")).getWorkspaceSnapshot(remote.workspaceId)!;
    const retained = Object.values(snapshot.sessions)[0];

    const registration = deferred<() => void>();
    native.onTerminalRemoteStatus.mockReturnValueOnce(registration.promise);
    native.retryTerminalRemoteSession.mockClear();

    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let cancel!: () => void;
    await act(async () => {
      cancel = native.refresh!(retained.id, res => results.push(res));
    });
    expect(results).toEqual([{ kind: "pending" }]);
    expect(native.retryTerminalRemoteSession).not.toHaveBeenCalled();

    cancel();

    const stopFn = vi.fn();
    await act(async () => {
      registration.resolve(stopFn);
      await registration.promise;
    });

    expect(native.retryTerminalRemoteSession).not.toHaveBeenCalled();
    expect(stopFn).toHaveBeenCalled();
  });
  it("shows progress throughout registration, restore, and first SSH tab creation", async () => {
    // Given an inactive remote root and independently controlled connection stages.
    seed([{ workspaceId: "local", repoRoot: remote.repoRoot, gitRoot: null }, remote]);
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
    await mount();
    const registration = deferred<RegisteredRemoteProject>();
    const restore = deferred<null>();
    const spawn = deferred<Awaited<ReturnType<typeof import("./lib/tauri").spawnTerminalDetailed>>>();
    native.registerRemoteProject.mockReturnValueOnce(registration.promise);
    native.loadSession.mockReturnValueOnce(restore.promise);
    native.spawnTerminalDetailed.mockReturnValueOnce(spawn.promise);

    // When the user selects the SSH root, each unresolved stage stays busy.
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Expand repo (build)" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /repo build|\/srv\/repo SSH root/ })); });
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("empty-workspace-view")).not.toBeInTheDocument();
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("empty-workspace-view")).not.toBeInTheDocument();
    await act(async () => { restore.resolve(null); await restore.promise; });
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("empty-workspace-view")).not.toBeInTheDocument();
    await act(async () => { spawn.resolve({ sessionId: "connected", daemonEpoch: "epoch", session: { sessionId: "connected", incarnation: "life:connected", cwd: remote.repoRoot, cols: 80, rows: 24, running: true } }); await spawn.promise; });

    // Then the terminal replaces progress, without an extra tab.
    expect(screen.queryByTestId("ssh-workspace-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("active-tab")).not.toBeEmptyDOMElement();
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
  });

  it("shows progress for manual first-tab creation and allows retry after failure", async () => {
    seed([remote]);
    await mount();
    expect(screen.getByTestId("empty-workspace-view")).toBeInTheDocument();
    const spawn = deferred<Awaited<ReturnType<typeof import("./lib/tauri").spawnTerminalDetailed>>>();
    native.spawnTerminalDetailed.mockReturnValueOnce(spawn.promise);
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

  it("says so when a New Terminal is discarded because the workspace changed mid-spawn", async () => {
    // Given: two local projects, and a spawn for the first that is still in flight.
    seed([{ workspaceId: "local", repoRoot: "/local", gitRoot: null }, { workspaceId: "other", repoRoot: "/other", gitRoot: null }]);
    await mount();
    const spawn = deferred<Awaited<ReturnType<typeof import("./lib/tauri").spawnTerminalDetailed>>>();
    native.spawnTerminalDetailed.mockReturnValueOnce(spawn.promise);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New Terminal" })); });

    // When: the user switches project before the spawn returns, so the new tab is discarded.
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "other" })); });
    await act(async () => {
      spawn.resolve({ sessionId: "late", daemonEpoch: "epoch", session: { sessionId: "late", incarnation: "life:late", cwd: "/local", cols: 80, rows: 24, running: true } });
      await spawn.promise;
    });

    // Then: the discard is reported instead of silently producing no terminal.
    expect(toastMocks.warning).toHaveBeenCalledWith(expect.stringContaining("workspace changed"));
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
    native.spawnTerminalDetailed.mockResolvedValueOnce({ sessionId: "backend-first", daemonEpoch: "epoch", session: { sessionId: "backend-first", incarnation: "life:backend-first", cwd: remote.repoRoot, cols: 80, rows: 24, running: true } }).mockResolvedValueOnce({ sessionId: "backend-second", daemonEpoch: "epoch", session: { sessionId: "backend-second", incarnation: "life:backend-second", cwd: remote.repoRoot, cols: 80, rows: 24, running: true } });
    seed([remote]);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /New Terminal/ })); });
    const first = screen.getByTestId("active-tab").textContent;
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New remote tab" })); });
    if (!native.remoteSelection) throw new Error("Missing remote session bridge");
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(2);
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
    await act(async () => {});
    expect(native.listWorktrees).toHaveBeenCalledWith(remote.workspaceId);
    expect(native.watchDagProject).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /New Terminal/ })).toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!)[0].target).toEqual(remote.target);
  });

  it("returns from a local project to the requested existing SSH session", async () => {
    native.spawnTerminalDetailed.mockResolvedValueOnce({ sessionId: "backend-ssh", daemonEpoch: "epoch", session: { sessionId: "backend-ssh", incarnation: "life:backend-ssh", cwd: remote.repoRoot, cols: 80, rows: 24, running: true } });
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
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(1);
  });

  it("opens the registered SSH project from remote without a preexisting terminal", async () => {
    seed([{ workspaceId: "local", repoRoot: "/local", gitRoot: null }, remote], "local");
    await mount();
    await act(async () => { native.remoteSelection?.({ workspaceId: remote.workspaceId }); });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(native.spawnTerminalDetailed).toHaveBeenCalledWith(expect.objectContaining({
      workspaceId: remote.workspaceId, cwd: remote.repoRoot,
    }));
  });

  it("opens an SSH folder inside a parent Git repository without selecting the local project", async () => {
    const folder: RegisteredProject = {
      workspaceId: "ssh:roblox", repoRoot: "C:\\Users\\sook\\code\\roblox-game-forge",
      gitRoot: "C:/Users/sook", gitBranch: "master", target: { kind: "ssh", hostId: "build" },
    };
    seed([{ workspaceId: "project", repoRoot: "/Users/example/code/project", gitRoot: null }, folder], "project");
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
    native.registerRemoteProject.mockResolvedValue({
      workspaceId: folder.workspaceId, repoRoot: folder.repoRoot, gitRoot: folder.gitRoot,
      gitBranch: "master", hostId: "build", hostLabel: "Build machine",
    });
    native.listWorktrees.mockImplementation(async (workspaceId: string) => workspaceId === folder.workspaceId
      ? [{ path: "C:/Users/sook", head: "", branch: "master", bare: false, detached: false, locked: null, prunable: null }]
      : []);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Expand roblox-game-forge (build)" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "roblox-game-forge build" })); });

    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(folder.workspaceId);
    expect(native.spawnTerminalDetailed).toHaveBeenCalledWith(expect.objectContaining({
      workspaceId: folder.workspaceId, cwd: folder.repoRoot,
    }));
    expect(native.spawnTerminalDetailed.mock.calls.some(([request]) => request.workspaceId === "project" && request.cwd === "C:/Users/sook")).toBe(false);
  });

  it("handles the chooser's registered remote project through actual App registration", async () => {
    seed([]); hosts.current = [{ id: "build", label: "Build machine", hostname: "build.example" }];
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValue(registration.promise);
    await mount();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add Project" })); });
    await act(async () => { fireEvent.click(screen.getByTestId("project-type-remote")); });
    await act(async () => { fireEvent.change(screen.getByTestId("remote-repo-path-input"), { target: { value: "/srv/input/" } }); });
    await act(async () => {});
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

  it("opens the paired project form for the machine Settings hands to Add Project", async () => {
    seed([]);
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));
    fireEvent.click(screen.getByTestId("project-type-remote"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("configure-ssh-settings"));
      await import("./components/SettingsDialog");
    });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Open paired project" })); });
    expect(screen.queryByTestId("settings-section")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Add Paired Daemon Project" })).toBeInTheDocument();
    // The machine selector is locked only when AddProjectDialog received a pairedDaemon initialTarget.
    expect(screen.getByRole("combobox", { name: "Paired machine" })).toBeDisabled();
  });

  it("opens a plain Add Project after a paired one from Settings is closed", async () => {
    seed([]);
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));
    fireEvent.click(screen.getByTestId("project-type-remote"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("configure-ssh-settings"));
      await import("./components/SettingsDialog");
    });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Open paired project" })); });
    expect(screen.getByRole("combobox", { name: "Paired machine" })).toBeDisabled();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel" })); });
    expect(screen.queryByRole("dialog", { name: "Add Paired Daemon Project" })).not.toBeInTheDocument();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add Project" })); });
    // A stale pairedDaemon target would reopen the locked paired form instead of the location chooser.
    expect(screen.getByRole("dialog", { name: "Add Project" })).toBeInTheDocument();
    expect(screen.getByTestId("project-type-local")).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Add Paired Daemon Project" })).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Paired machine" })).not.toBeInTheDocument();
  });

  it("adopts the server's canonical host-qualified ID and path before new tabs and splits", async () => {
    seed([{ ...remote, workspaceId: "alias", repoRoot: "/srv/input" }]);
    await mount();
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!)).toEqual([expect.objectContaining(remote)]);
    const spawn = deferred<Awaited<ReturnType<typeof import("./lib/tauri").spawnTerminalDetailed>>>();
    native.spawnTerminalDetailed.mockReturnValueOnce(spawn.promise);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New Terminal" })); });
    expect(native.spawnTerminalDetailed).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, cwd: remote.repoRoot, worktree: null }));
    await act(async () => { spawn.resolve({ sessionId: "remote-backend", daemonEpoch: "epoch", session: { sessionId: "remote-backend", incarnation: "life:remote-backend", cwd: remote.repoRoot, cols: 80, rows: 24, running: true } }); await spawn.promise; });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New remote tab" })); });
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(2);
    expect(native.spawnTerminalDetailed.mock.calls.every(([request]) => request.workspaceId === remote.workspaceId && request.cwd === remote.repoRoot)).toBe(true);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Split remote pane" })); });
    expect(native.spawnTerminalDetailed).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, worktree: null, cwd: null, inheritFromSessionId: "remote-backend" }));
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(native.listWorktrees).toHaveBeenCalledWith(remote.workspaceId);
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
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Expand repo (build)" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "repo build" })); });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(remote.workspaceId);
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    await act(async () => { restore.resolve(null); await restore.promise; });
    expect(native.spawnTerminalDetailed).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, cwd: remote.repoRoot }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "local" })); });
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(local.workspaceId);
    expect(native.registerProject.mock.calls.every(([request]) => request.workspaceId === "local")).toBe(true);
    expect(native.listWorktrees).toHaveBeenCalledWith("local");
    expect(native.listWorktrees).toHaveBeenCalledWith(remote.workspaceId);
  });

  it("leaves a rejected host unavailable instead of spawning locally", async () => {
    seed([remote]);
    const registration = deferred<RegisteredRemoteProject>();
    native.registerRemoteProject.mockReturnValue(registration.promise);
    await mount();
    await act(async () => { registration.reject({ code: "WORKSPACE_NOT_FOUND", message: "Host disabled" }); await registration.promise.catch(() => undefined); });
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
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
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
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

  it("reattaches remote sessions after native startup without replacement and preserves their target and identity in the next save", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const status = deferred<import("./lib/types").RemoteSessionStatusResponse>();
    native.getTerminalRemoteStatus.mockReturnValue(status.promise);
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
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    await act(async () => { registration.resolve(registered); await registration.promise; });
    expect(native.getTerminalRemoteStatus).toHaveBeenCalledWith("old-backend");
    await act(async () => {
      status.resolve({
        type: "remoteSessionDetailsOk", legacyDirectSsh: false,
        details: {
          state: "connected", generation: 2, failure: null, replayGap: null,
          attempts: 0, pid: 1234,
          descriptor: {
            backendSessionId: "old-backend",
            target: { hostId: "build", ownerId: "owner", backendSessionId: "remote-process", epoch: "1" },
            config: {}, clientRequestId: "original-request", remoteCursor: "0", cols: 80, rows: 24,
          },
        },
      });
      await status.promise;
    });
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    expect(screen.getByTestId("active-tab")).toHaveTextContent("saved-tab");
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!).find((project: RegisteredProject) => project.workspaceId === remote.workspaceId).target).toEqual(remote.target);
    expect(native.closeGuard).not.toBeNull();
    await act(async () => { await native.closeGuard!(); });
    expect(native.saveSession).toHaveBeenCalled();
    const latest = native.saveSession.mock.calls.at(-1)![0];
    expect(latest.workspaces[remote.workspaceId].target).toEqual(remote.target);
    expect(latest.workspaces[remote.workspaceId].terminalSessions["saved-session"]).toMatchObject({
      localSessionId: "saved-session", backendSessionId: "old-backend", cwd: remote.repoRoot,
    });
  });
});
