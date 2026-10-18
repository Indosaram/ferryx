import { act, cleanup, render } from "@testing-library/react";
import { useLayoutEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RegisteredProject, PersistedWorkspaceSession } from "./lib/types";

const native = vi.hoisted(() => ({
  registerProject: vi.fn(),
  registerRemoteProject: vi.fn(),
  listWorktrees: vi.fn(),
  describeTerminal: vi.fn(),
  spawnTerminalDetailed: vi.fn(),
  spawnTerminal: vi.fn(),
  watchDagProject: vi.fn(),
  loadSession: vi.fn(),
  saveSession: vi.fn(),
  isTauriRuntime: vi.fn(),
  retryPairedSession: vi.fn(),
  retryTerminalRemoteSession: vi.fn().mockResolvedValue({ type: "retryRemoteSessionOk" }),
  terminalStatusSubscriptions: [] as string[],
  terminalLifecycleHandler: null as null | ((event: import("./lib/types").TerminalLifecyclePayload) => void),
  terminalRemoteStatusHandler: null as null | ((status: import("./lib/types").TerminalRemoteStatus) => void),
  onTerminalRemoteStatus: null as null | ((handler: (status: import("./lib/types").TerminalRemoteStatus) => void) => Promise<() => void>),
}));
const storeObservation = vi.hoisted(() => ({ ensure: vi.fn(), state: null as import("./state/workspaceStore").WorkspaceState | null,
  persist: undefined as Parameters<typeof import("./state/workspaceStore").setLocalRecoveryPersistence>[0] }));
const fixtureEvents = vi.hoisted(() => new EventTarget());
vi.mock("./state/workspaceStore", async (original) => {
  const actual = await original<typeof import("./state/workspaceStore")>();
  return { ...actual,
    setLocalRecoveryPersistence: (persist: Parameters<typeof actual.setLocalRecoveryPersistence>[0]) => {
      storeObservation.persist = persist;
      actual.setLocalRecoveryPersistence(persist);
      if (persist) fixtureEvents.dispatchEvent(new Event("persistence-registered"));
    },
    useWorkspaceStore: (options: Parameters<typeof actual.useWorkspaceStore>[0]) => {
      const store = actual.useWorkspaceStore(options);
      storeObservation.state = store.state;
      return { ...store, ensureSessionBackends: (...args: Parameters<typeof store.ensureSessionBackends>) => {
        storeObservation.ensure(...args);
        return store.ensureSessionBackends(...args);
      } };
    },
  };
});

vi.mock("./lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/tauri")>(),
  ...native,
  getInitialProject: async () => ({ workspaceId: "local", repoRoot: "/local", gitRoot: null }),
  detectAgents: async () => [],
  listTerminalSessions: async () => [],
  ensureTerminalEvents: async () => undefined,
  onWorktreeChanged: async () => () => undefined,
  onTerminalRemoteStatus: async (handler: (status: import("./lib/types").TerminalRemoteStatus) => void) => {
    native.terminalStatusSubscriptions.push("subscribed");
    if (native.onTerminalRemoteStatus) {
      return native.onTerminalRemoteStatus(handler);
    }
    native.terminalRemoteStatusHandler = handler;
    return () => { if (native.terminalRemoteStatusHandler === handler) native.terminalRemoteStatusHandler = null; };
  },
  onTerminalLifecycle: async (handler: (event: import("./lib/types").TerminalLifecyclePayload) => void) => {
    native.terminalLifecycleHandler = handler;
    return () => { if (native.terminalLifecycleHandler === handler) native.terminalLifecycleHandler = null; };
  },
  onTerminalOutput: async () => () => undefined,
  onNewTerminalTabMenu: async () => () => undefined,
  onSplitRightMenu: async () => () => undefined,
  onCommandPaletteMenu: async () => () => undefined,
  onCloseTabMenu: async () => () => undefined,
  onSelectWorktreeMenu: async () => () => undefined,
  onRemoteSelectionRequested: async () => () => undefined,
  listenDagRunUpdated: async () => () => undefined,
  publishFocusedTerminal: async () => undefined,
  setBadgeCount: async () => ({ supported: false, count: 0 }),
}));

vi.mock("./lib/remoteDirectories", () => ({
  listRemoteDirectories: vi.fn(async () => ({
    path: "/srv/repo", parentPath: null, homePath: "/home/ubuntu", entries: [], truncated: false,
  })),
}));

vi.mock("./lib/remoteProject", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/remoteProject")>(),
  registerRemoteProject: native.registerRemoteProject,
}));

vi.mock("./state/pairedProjectWorktrees", () => ({
  listPairedProjectWorktrees: async (project: RegisteredProject) => [{
    path: project.repoRoot, branch: "main", head: "HEAD", bare: false,
    detached: false, locked: null, prunable: null,
  }],
}));

vi.mock("./lib/sshHosts", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/sshHosts")>(),
  useSshHosts: () => ({ hosts: [], loading: false, error: null, refresh: async () => [] }),
}));

vi.mock("./lib/pairedHostInventory", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/pairedHostInventory")>(),
  pairedHostInventory: { refresh: vi.fn().mockResolvedValue(undefined) },
}));

vi.mock("./lib/terminalEvents", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/terminalEvents")>(),
  ensureTerminalEvents: async () => undefined,
}));

vi.mock("./lib/updater", () => ({
  subscribeUpdateStatus: () => () => undefined,
  startUpdatePolling: () => undefined,
  registerWindowCloseGuard: () => () => undefined,
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isFocused: async () => true, onFocusChanged: async () => () => undefined }),
}));
vi.mock("./lib/updateToast", () => ({ initUpdateToasts: () => () => undefined }));
vi.mock("./components/SettingsDialog", () => ({ SettingsDialog: () => null }));

let triggerUnavailable: ((sessionId: string, backendSessionId: string, reason: string | null, bindingKey: string | null) => void) | null = null;
let requestRecoveryAdmission: ((frontendSessionId: string, onResult: (result: import("./state/workspaceStore").LocalRecoveryAdmissionResult) => void) => () => void) | null = null;
const admissionCancellations = new Set<() => void>();

function nextFixtureEvent(name: string) {
  let dispose = () => {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  let settled = false;
  let rejectWait!: (reason: Error) => void;
  const promise = new Promise<void>((resolve, reject) => {
    rejectWait = reject;
    const listener = () => { settled = true; dispose(); resolve(); };
    dispose = () => { clearTimeout(timer); fixtureEvents.removeEventListener(name, listener); };
    fixtureEvents.addEventListener(name, listener);
  });
  return {
    get promise() {
      if (!settled && timer === undefined) {
        timer = setTimeout(() => { dispose(); rejectWait(new Error(`Missing fixture event: ${name}`)); }, 1000);
      }
      return promise;
    },
    dispose,
  };
}

async function renderRestoredApp() {
  const ready = nextFixtureEvent("restored-split");
  try {
    let view!: ReturnType<typeof render>;
    await act(async () => { view = render(<App />); });
    await ready.promise;
    return view;
  } finally {
    ready.dispose();
  }
}

async function requestPairedAdmission(onResult: (result: import("./state/workspaceStore").LocalRecoveryAdmissionResult) => void) {
  const entered = nextFixtureEvent("retry-entered");
  const original = native.retryPairedSession.getMockImplementation();
  native.retryPairedSession.mockImplementationOnce((...args: unknown[]) => {
    const result = original?.(...args);
    fixtureEvents.dispatchEvent(new Event("retry-entered"));
    return result;
  });
  try {
    let cancel = () => {};
    await act(async () => {
      cancel = requestRecoveryAdmission!("paired-session", onResult);
      await entered.promise;
    });
    return cancel;
  } finally {
    entered.dispose();
  }
}

vi.mock("./components/TerminalSplitView", () => ({
  TerminalSplitView: ({
    onBackendSessionUnavailable,
    onRefreshSessionIdentity,
  }: {
    onBackendSessionUnavailable?: (sessionId: string, backendSessionId: string, reason: string | null, bindingKey: string | null) => void;
    onRefreshSessionIdentity?: (frontendSessionId: string, onResult: (result: import("./state/workspaceStore").LocalRecoveryAdmissionResult) => void) => () => void;
  }) => {
    useLayoutEffect(() => {
      triggerUnavailable = onBackendSessionUnavailable ?? null;
      requestRecoveryAdmission = onRefreshSessionIdentity ? (sessionId, onResult) => {
        const cancel = onRefreshSessionIdentity(sessionId, onResult);
        admissionCancellations.add(cancel);
        return () => { admissionCancellations.delete(cancel); cancel(); };
      } : null;
      if (requestRecoveryAdmission && Object.keys(storeObservation.state?.sessions ?? {}).length > 0) {
        fixtureEvents.dispatchEvent(new Event("restored-split"));
      }
    });
    return <div data-testid="terminal-split-view" />;
  },
}));

import { App } from "./App";
import { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY } from "./lib/storageKeys";
import { remoteHostStore } from "./state/remoteHostStore";
import { resetWorkspaceRestore } from "./state/workspaceRestore";
import { clearWorkspaceSnapshot } from "./state/workspaceSnapshotCache";
import { clearHmrWorkspaceState } from "./state/hmrWorkspaceState";
import { setLocalRecoveryPersistence } from "./state/workspaceStore";

const pairedProject: RegisteredProject = {
  workspaceId: `daemon:${"a".repeat(64)}`,
  repoRoot: "/srv/repo",
  gitRoot: null,
  target: { kind: "pairedDaemon", hostId: "host-a" },
  remoteWorkspaceId: "remote-project",
};

describe("App paired recovery boundary", () => {
  beforeEach(() => {
    localStorage.clear();
    remoteHostStore.setState(() => ({
      hosts: {
        "host-a": { hostId: "host-a", name: "Host A", address: "127.0.0.1", transport: "relay", authStatus: "paired", online: true, generation: "1", grantScope: "machine" },
      },
      activeHostId: null, discovering: false, nativeStatus: "ready", machineFeaturesEnabled: true,
    }));
    resetWorkspaceRestore();
    clearWorkspaceSnapshot(); clearHmrWorkspaceState();
    vi.clearAllMocks();
    native.retryPairedSession.mockReset();
    native.loadSession.mockReset();
    native.saveSession.mockReset();
    storeObservation.state = null;
    storeObservation.persist = undefined;
    triggerUnavailable = null;
    requestRecoveryAdmission = null;
    native.isTauriRuntime.mockReturnValue(false);
    native.registerProject.mockImplementation(async (r: { workspaceId: string; repoPath: string }) => ({
      workspaceId: r.workspaceId, repoRoot: r.repoPath, gitRoot: null,
    }));
    native.registerRemoteProject.mockResolvedValue({
      workspaceId: pairedProject.workspaceId,
      repoRoot: pairedProject.repoRoot,
      gitRoot: "/srv/repo",
      hostId: "host-a",
      hostLabel: "Host A",
    });
    native.listWorktrees.mockResolvedValue([{ path: "/local", branch: "main", head: "HEAD", bare: false, detached: false, locked: null, prunable: null }]);
    native.describeTerminal.mockReset();
    native.describeTerminal.mockResolvedValue(null);
    native.loadSession.mockResolvedValue(null);
    native.saveSession.mockResolvedValue(undefined);
    native.watchDagProject.mockImplementation(async (projectPath: string) => ({ projectPath, runs: [] }));
    native.spawnTerminal.mockResolvedValue("backend-spawned");
    native.spawnTerminalDetailed.mockResolvedValue({
      sessionId: "backend-detailed",
      daemonEpoch: "epoch-1",
      session: { sessionId: "backend-detailed", incarnation: "inc-1", cwd: "/srv/repo", cols: 80, rows: 24, running: true },
    });
    native.retryPairedSession.mockResolvedValue({
      admitted: true,
      ownerUnavailable: false,
      staleTuple: false,
      blocked: false,
      affirmativeExit: false,
    });
    native.retryTerminalRemoteSession.mockResolvedValue({ type: "retryRemoteSessionOk" });
    native.onTerminalRemoteStatus = null;
    native.terminalStatusSubscriptions = [];
    native.terminalRemoteStatusHandler = null;
    native.terminalLifecycleHandler = null;
  });

  afterEach(() => {
    for (const cancel of admissionCancellations) cancel();
    admissionCancellations.clear();
    cleanup();
    resetWorkspaceRestore();
    remoteHostStore.reset();
    clearWorkspaceSnapshot(); setLocalRecoveryPersistence(undefined); clearHmrWorkspaceState();
  });

  it("invokes zero ensureSessionBackends/spawn calls and retains proxy identity on backend-unavailable", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);

    const persisted: PersistedWorkspaceSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot,
          workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot,
          target: pairedProject.target,
          remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "tab-1": {
              localSessionId: "tab-1",
              backendSessionId: "paired-proxy-123",
              worktreePath: "/srv/repo",
              cwd: "/srv/repo",
              createdAt: Date.now(),
              daemonEpoch: "epoch-paired",
              incarnation: "inc-paired",
            },
          },
          layout: {
            splitMode: "none",
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "tab-1", kind: "terminal" }],
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);

    let view!: ReturnType<typeof render>;
    view = await renderRestoredApp();

    expect(triggerUnavailable).not.toBeNull();
    await act(async () => {
      const session = storeObservation.state!.sessions["tab-1"];
      const binding = [session.backendSessionId ?? "", session.daemonEpoch ?? "", session.remoteGeneration ?? "", session.incarnation ?? ""].join(":");
      triggerUnavailable?.("tab-1", "paired-proxy-123", "owner_unavailable", binding);
    });
    expect(storeObservation.state?.sessions["tab-1"]).toMatchObject({ backendSessionId: "paired-proxy-123", incarnation: "inc-paired", backendUnavailableReason: "owner_unavailable" });
    await act(async () => { view.rerender(<App />); });
    expect(storeObservation.state?.sessions["tab-1"].backendSessionId).toBe("paired-proxy-123");
    expect(storeObservation.ensure).not.toHaveBeenCalled();
    expect(native.spawnTerminal).toHaveBeenCalledTimes(0);
    expect(native.spawnTerminalDetailed).toHaveBeenCalledTimes(0);
  });
  it("waits for a tuple-matched owner wake and cancels it after the tuple changes", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot,
          workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot,
          target: pairedProject.target,
          remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": {
              localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
              daemonEpoch: "epoch-paired", incarnation: "inc-paired",
            },
          },
          layout: {
            splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }],
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.retryPairedSession.mockResolvedValue({ admitted: true, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: false, actorIdentity: { backendSessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired", generation: 1 } });
    await renderRestoredApp();
    expect(requestRecoveryAdmission).not.toBeNull();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let current = storeObservation.state!.sessions["paired-session"];
    const cancel = await requestPairedAdmission(result => outcomes.push(result));
    expect(native.terminalStatusSubscriptions.length).toBeGreaterThan(0);
    expect(outcomes).toEqual([{ kind: "pending" }]);
    expect(native.retryPairedSession).toHaveBeenCalledWith({ backendSessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired" });
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    expect(storeObservation.ensure).not.toHaveBeenCalled();
    await act(async () => {
      native.terminalRemoteStatusHandler?.({
        sessionId: "paired-proxy-123",
        target: { machineId: "host-a", daemonEpoch: "epoch-paired", sessionId: "paired-proxy-123" },
        descriptor: { backendSessionId: "paired-proxy-123", hostId: "host-a", target: { machineId: "host-a", daemonEpoch: "epoch-paired", sessionId: "paired-proxy-123" }, incarnation: "inc-paired", daemonEpoch: "epoch-paired" },
        generation: 2, state: "connected", failure: null, replayGap: null,
      });
    });
    expect(outcomes.at(-1)).toMatchObject({ kind: "ready", session: { backendSessionId: "paired-proxy-123", incarnation: "inc-paired", remoteGeneration: 2, remoteConnectionState: "connected" } });
    expect(native.terminalRemoteStatusHandler).toBeNull();
    outcomes.length = 0;
    native.retryPairedSession.mockClear();
    native.retryPairedSession.mockResolvedValue({ admitted: true, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: false });
    const cancelOld = await requestPairedAdmission(result => outcomes.push(result));
    current = { ...current, incarnation: "inc-rebound" };
    storeObservation.state = { ...storeObservation.state!, sessions: { ...storeObservation.state!.sessions, "paired-session": current } };
    cancelOld();
    native.terminalRemoteStatusHandler?.({
      sessionId: "paired-proxy-123",
      target: { machineId: "host-a", daemonEpoch: "epoch-paired", sessionId: "paired-proxy-123" },
      descriptor: { backendSessionId: "paired-proxy-123", hostId: "host-a", target: { machineId: "host-a", daemonEpoch: "epoch-paired", sessionId: "paired-proxy-123" }, incarnation: "inc-wrong", daemonEpoch: "epoch-paired" },
      generation: 1, state: "connected", failure: null, replayGap: null,
    });
    expect(outcomes).toEqual([{ kind: "pending" }]);
    cancel();
    expect(storeObservation.ensure).not.toHaveBeenCalled();
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
  });
  describe("pre-receipt admission tuple", () => {
    beforeEach(async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
      const persisted: PersistedWorkspaceSession = {
        version: 2, timestamp: Date.now(), activeWorkspaceId: pairedProject.workspaceId,
        workspaces: {
          [pairedProject.workspaceId]: {
            activeWorktreePath: pairedProject.repoRoot, workspaceId: pairedProject.workspaceId,
            repoRoot: pairedProject.repoRoot, target: pairedProject.target, remoteWorkspaceId: "remote-project",
            worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
            terminalSessions: {
              "paired-session": { localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
                worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
                daemonEpoch: "epoch-paired", incarnation: "inc-paired" },
            },
            layout: { splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
              tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }] },
          },
        },
      };
      native.loadSession.mockResolvedValue(persisted);
      await renderRestoredApp();
      expect(requestRecoveryAdmission).not.toBeNull();
      expect(storeObservation.state?.sessions["paired-session"]).toMatchObject({
        backendSessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired",
      });
    });

  it.each([
    "matching", "descriptor-session", "descriptor-epoch", "descriptor-incarnation",
    "descriptor-host", "target-machine", "target-session", "target-epoch",
    "actor-session", "actor-epoch", "actor-incarnation", "actor-generation",
  ] as const)("replays pre-receipt connected status only for a matching admission tuple: %s", async (identityCase) => {
    // Given: the retry RPC is held until a connected event has been delivered.
    let releaseAdmission!: (result: import("./lib/types").PairedSessionRetryResult) => void;
    let retryEntered!: () => void;
    const retryStarted = new Promise<void>((resolve) => { retryEntered = resolve; });
    const admission = new Promise<import("./lib/types").PairedSessionRetryResult>((resolve) => { releaseAdmission = resolve; });
    native.retryPairedSession.mockImplementation(() => { retryEntered(); return admission; });
    const requestAdmission = requestRecoveryAdmission;
    if (!requestAdmission) throw new Error("Missing recovery admission callback");
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let cancel = () => {};
    try {
      act(() => { cancel = requestAdmission("paired-session", result => outcomes.push(result)); });
      await retryStarted;
      const actorIdentity = {
        backendSessionId: identityCase === "actor-session" ? "other-session" : "paired-proxy-123",
        daemonEpoch: identityCase === "actor-epoch" ? "other-epoch" : "epoch-paired",
        incarnation: identityCase === "actor-incarnation" ? "other-incarnation" : "inc-paired",
        generation: identityCase === "actor-generation" ? 3 : 1,
      };
      const descriptor = {
        backendSessionId: identityCase === "descriptor-session" ? "other-session" : actorIdentity.backendSessionId,
        hostId: identityCase === "descriptor-host" ? "other-host" : "host-a",
        daemonEpoch: identityCase === "descriptor-epoch" ? "other-epoch" : actorIdentity.daemonEpoch,
        incarnation: identityCase === "descriptor-incarnation" ? "other-incarnation" : actorIdentity.incarnation,
        target: {
          machineId: identityCase === "target-machine" ? "other-host" : "host-a",
          daemonEpoch: identityCase === "target-epoch" ? "other-epoch" : "epoch-paired",
          sessionId: identityCase === "target-session" ? "other-session" : "paired-proxy-123",
        },
      };
      const emitStatus = native.terminalRemoteStatusHandler;
      if (!emitStatus) throw new Error("Retry started without a status subscription");

      // When: connected arrives before the matching or conflicting RPC receipt.
      act(() => { emitStatus({ sessionId: "paired-proxy-123", descriptor, target: descriptor.target,
        generation: 2, state: "connected", failure: null, replayGap: null }); });
      expect(outcomes).toEqual([{ kind: "pending" }]);
      await act(async () => {
        releaseAdmission({ admitted: true, ownerUnavailable: false, staleTuple: false,
          blocked: false, affirmativeExit: false, actorIdentity });
        await admission;
      });

      // Then: only the full matching tuple can consume the buffered event as ready.
      if (identityCase === "matching") {
        expect(outcomes).toHaveLength(2);
        expect(outcomes.at(-1)).toMatchObject({ kind: "ready", session: {
          backendSessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired",
          remoteGeneration: 2, remoteConnectionState: "connected",
        } });
        expect(native.terminalRemoteStatusHandler).toBeNull();
      } else {
        expect(outcomes).toEqual([{ kind: "pending" }]);
        expect(native.terminalRemoteStatusHandler).not.toBeNull();
      }
      expect(native.spawnTerminal).not.toHaveBeenCalled();
      expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
      expect(storeObservation.ensure).not.toHaveBeenCalled();
    } finally {
      cancel();
    }
  }, 1000);

  it.each(["before-response", "after-response"] as const)("settles async paired blocked status %s without another retry", async (order) => {
    // Given: subscription precedes Retry, whose response is controlled independently.
    let release!: (result: import("./lib/types").PairedSessionRetryResult) => void;
    const response = new Promise<import("./lib/types").PairedSessionRetryResult>(resolve => { release = resolve; });
    native.retryPairedSession.mockReturnValue(response);
    const receipt: import("./lib/types").PairedSessionRetryResult = {
      admitted: true, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: false,
      actorIdentity: { backendSessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired", generation: 1 },
    };
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    const cancel = await requestPairedAdmission(result => outcomes.push(result));
    const emit = native.terminalRemoteStatusHandler;
    if (!emit) throw new Error("Retry started without a status subscription");
    const descriptor = { backendSessionId: "paired-proxy-123", hostId: "host-a", daemonEpoch: "epoch-paired",
      incarnation: "inc-paired", target: { machineId: "host-a", daemonEpoch: "epoch-paired", sessionId: "paired-proxy-123" } };
    try {
      if (order === "after-response") await act(async () => { release(receipt); await response; });

      // When: the current paired actor reports a recognized blocked code asynchronously.
      await act(async () => {
        emit({ sessionId: "paired-proxy-123", descriptor, generation: 1, state: "disconnected",
          failure: { kind: "disconnected", message: "PAIRED_HOST_UNAUTHORIZED" }, replayGap: null });
      });

      // Then: blocked presentation and admission retain the code immediately, not at timeout.
      expect(outcomes).toEqual([{ kind: "pending" }, { kind: "blocked", reason: "PAIRED_HOST_UNAUTHORIZED" }]);
      expect(storeObservation.state?.sessions["paired-session"]).toMatchObject({
        backendSessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired",
        recoveryState: { phase: "blocked", reason: "PAIRED_HOST_UNAUTHORIZED" }, reconnectLifecycle: "failed",
      });
      expect(native.terminalRemoteStatusHandler).toBeNull();
      await act(async () => {
        release(receipt); await response;
        emit({ sessionId: "paired-proxy-123", descriptor, generation: 2, state: "connected", failure: null, replayGap: null });
      });
      expect(outcomes).toHaveLength(2);
      expect(native.retryPairedSession).toHaveBeenCalledTimes(1);
      expect(native.spawnTerminal).not.toHaveBeenCalled();
      expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    } finally { cancel(); }
  });

  it.each(([
    "session", "descriptor-session", "descriptor-epoch", "descriptor-incarnation", "descriptor-host",
    "target-machine", "target-session", "target-epoch", "generation", "retryable", "prose", "failure-kind",
  ] as const).flatMap(eventCase => (["before-response", "after-response"] as const).map(order => [eventCase, order] as const)))("ignores foreign or retryable paired disconnected status: %s %s", async (eventCase, order) => {
    // Given: a subscribed admission with an independently controlled actor receipt.
    let release!: (result: import("./lib/types").PairedSessionRetryResult) => void;
    const response = new Promise<import("./lib/types").PairedSessionRetryResult>(resolve => { release = resolve; });
    const receipt: import("./lib/types").PairedSessionRetryResult = { admitted: true, ownerUnavailable: false, staleTuple: false,
      blocked: false, affirmativeExit: false, actorIdentity: { backendSessionId: "paired-proxy-123",
        daemonEpoch: "epoch-paired", incarnation: "inc-paired", generation: 1 } };
    native.retryPairedSession.mockReturnValue(response);
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    const cancel = await requestPairedAdmission(result => outcomes.push(result));
    const emit = native.terminalRemoteStatusHandler;
    if (!emit) throw new Error("Retry started without a status subscription");
    const before = storeObservation.state?.sessions["paired-session"];
    const descriptor = {
      backendSessionId: eventCase === "descriptor-session" ? "foreign" : "paired-proxy-123",
      hostId: eventCase === "descriptor-host" ? "foreign" : "host-a",
      daemonEpoch: eventCase === "descriptor-epoch" ? "foreign" : "epoch-paired",
      incarnation: eventCase === "descriptor-incarnation" ? "foreign" : "inc-paired",
      target: { machineId: eventCase === "target-machine" ? "foreign" : "host-a",
        daemonEpoch: eventCase === "target-epoch" ? "foreign" : "epoch-paired",
        sessionId: eventCase === "target-session" ? "foreign" : "paired-proxy-123" },
    };
    try {
      if (order === "after-response") await act(async () => { release(receipt); await response; });
      // When: an unfenced event or ordinary transport disconnect arrives.
      await act(async () => {
        emit({ sessionId: eventCase === "session" ? "foreign" : "paired-proxy-123", descriptor,
          generation: eventCase === "generation" ? (order === "before-response" ? -1 : 0) : 1, state: "disconnected",
          failure: { kind: eventCase === "failure-kind" ? "network" : "disconnected",
            message: eventCase === "retryable" ? "PAIRED_HOST_UNAVAILABLE" : eventCase === "prose"
              ? "Transport failed: PAIRED_HOST_UNAUTHORIZED" : "PAIRED_HOST_UNAUTHORIZED" }, replayGap: null });
      });
      // Then: no blocked state or terminal admission result is produced.
      expect(outcomes).toEqual([{ kind: "pending" }]);
      expect(storeObservation.state?.sessions["paired-session"]).toEqual(before);
      expect(native.terminalRemoteStatusHandler).not.toBeNull();
      await act(async () => { release(receipt); await response; });
      expect(outcomes).toEqual([{ kind: "pending" }]);
      if (["retryable", "prose", "failure-kind"].includes(eventCase)) {
        await act(async () => {
          emit({ sessionId: "paired-proxy-123", descriptor, generation: 2, state: "connected", failure: null, replayGap: null });
        });
        expect(outcomes.at(-1)?.kind).toBe("ready");
      }
    } finally { cancel(); }
  });
  });
  it.each([
    [{ admitted: false, ownerUnavailable: false, staleTuple: false, blocked: true, affirmativeExit: false }, "blocked"],
    [{ admitted: false, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: true }, "exited"],
    [{ admitted: false, ownerUnavailable: false, staleTuple: true, blocked: false, affirmativeExit: false }, "blocked"],
    [{ admitted: false, ownerUnavailable: true, staleTuple: false, blocked: false, affirmativeExit: false }, "pending"],
  ] as const)("preserves paired non-admission outcome without spawning", async (retryResult, expected) => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2, timestamp: Date.now(), activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot, workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot, target: pairedProject.target, remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": { localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
              daemonEpoch: "epoch-paired", incarnation: "inc-paired" },
          },
          layout: { splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }] },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.retryPairedSession.mockResolvedValue(retryResult);
    await renderRestoredApp();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    const settled = nextFixtureEvent("admission-settled");
    try {
      await requestPairedAdmission(result => {
        outcomes.push(result);
        if (result.kind !== "pending") fixtureEvents.dispatchEvent(new Event("admission-settled"));
      });
      if (expected !== "pending") await settled.promise;
    } finally {
      settled.dispose();
    }
    expect(outcomes.at(-1)?.kind).toBe(expected);
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    expect(storeObservation.ensure).not.toHaveBeenCalled();
  });
  it("rejects a connected runtime status with a mismatched identity tuple", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2, timestamp: Date.now(), activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot, workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot, target: pairedProject.target, remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": { localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
              daemonEpoch: "epoch-paired", incarnation: "inc-paired" },
          },
          layout: { splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }] },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.retryPairedSession.mockResolvedValue({ admitted: true, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: false });
    // Describe returns a different daemonEpoch: stale expired or connected event must be rejected as identity_conflict
    await renderRestoredApp();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await requestPairedAdmission(result => outcomes.push(result));
    expect(outcomes).toEqual([{ kind: "pending" }]);
    await act(async () => {
      native.terminalRemoteStatusHandler?.({
        sessionId: "paired-proxy-123", daemonEpoch: "epoch-different", incarnation: "inc-different",
        target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-different", backendSessionId: "paired-proxy-123" },
        descriptor: { backendSessionId: "paired-proxy-123", target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-different", backendSessionId: "paired-proxy-123" }, config: {}, clientRequestId: "request-1", remoteCursor: "0", cols: 80, rows: 24 },
        generation: 1, state: "connected", failure: null, replayGap: null,
      });
    });
    expect(outcomes).toEqual([{ kind: "pending" }]);
    expect(native.terminalRemoteStatusHandler).not.toBeNull();
  });
  it("disconnected runtime status does not resolve readiness or trigger spawn", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2, timestamp: Date.now(), activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot, workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot, target: pairedProject.target, remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": { localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
              daemonEpoch: "epoch-paired", incarnation: "inc-paired" },
          },
          layout: { splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }] },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.retryPairedSession.mockResolvedValue({ admitted: true, ownerUnavailable: true, staleTuple: false, blocked: false, affirmativeExit: false });
    await renderRestoredApp();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await requestPairedAdmission(result => outcomes.push(result));
    expect(outcomes).toEqual([{ kind: "pending" }]);
    await act(async () => {
      native.terminalRemoteStatusHandler?.({
        sessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired",
        target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-paired", backendSessionId: "paired-proxy-123" },
        descriptor: { backendSessionId: "paired-proxy-123", target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-paired", backendSessionId: "paired-proxy-123" }, config: {}, clientRequestId: "request-1", remoteCursor: "0", cols: 80, rows: 24 },
        generation: 1, state: "disconnected", failure: null, replayGap: null,
      });
    });
    expect(outcomes).toEqual([{ kind: "pending" }]);
    await act(async () => {
      native.terminalRemoteStatusHandler?.({
        sessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired",
        target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-paired", backendSessionId: "paired-proxy-123" },
        descriptor: { backendSessionId: "paired-proxy-123", target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-paired", backendSessionId: "paired-proxy-123" }, config: {}, clientRequestId: "request-1", remoteCursor: "0", cols: 80, rows: 24 },
        generation: 1, state: "disconnected", failure: null, replayGap: null,
      });
    });
    expect(outcomes).toEqual([{ kind: "pending" }]);
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
  });
  it("missing both epoch or incarnation with affirmative_exit or proxy_attached stays pending", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2, timestamp: Date.now(), activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot, workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot, target: pairedProject.target, remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": { localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
              daemonEpoch: "epoch-paired", incarnation: "inc-paired" },
          },
          layout: { splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }] },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.retryPairedSession.mockResolvedValue({ admitted: true, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: false });
    await renderRestoredApp();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await requestPairedAdmission(result => outcomes.push(result));
    expect(outcomes).toEqual([{ kind: "pending" }]);
    await act(async () => {
      native.terminalRemoteStatusHandler?.({
        sessionId: "paired-proxy-123",
        target: { machineId: "host-a", daemonEpoch: "epoch-mismatch", sessionId: "paired-proxy-123" },
        descriptor: { backendSessionId: "paired-proxy-123", hostId: "host-a", target: { machineId: "host-a", daemonEpoch: "epoch-mismatch", sessionId: "paired-proxy-123" }, incarnation: "inc-paired", daemonEpoch: "epoch-mismatch" },
        generation: 2, state: "disconnected", failure: null, replayGap: null,
      });
    });
    expect(outcomes).toEqual([{ kind: "pending" }]);
    await act(async () => {
      native.terminalRemoteStatusHandler?.({
      sessionId: "paired-proxy-123",
      target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-paired", backendSessionId: "paired-proxy-123" },
      descriptor: { backendSessionId: "paired-proxy-123", hostId: "host-a", target: { machineId: "host-a", daemonEpoch: "epoch-paired", sessionId: "paired-proxy-123" }, incarnation: "inc-paired", daemonEpoch: "epoch-paired" },
        generation: 1, state: "disconnected", failure: null, replayGap: null,
      });
    });
    expect(outcomes).toEqual([{ kind: "pending" }]);
  });
  it("does not use a paired describe query as runtime readiness", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2, timestamp: Date.now(), activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot, workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot, target: pairedProject.target, remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": { localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
              daemonEpoch: "epoch-paired", incarnation: "inc-paired" },
          },
          layout: { splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }] },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.retryPairedSession.mockResolvedValue({ admitted: true, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: false });
    await renderRestoredApp();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await requestPairedAdmission(result => outcomes.push(result));
    expect(outcomes).toEqual([{ kind: "pending" }]);
    await act(async () => {
      native.terminalRemoteStatusHandler?.({
        sessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired",
        target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-paired", backendSessionId: "paired-proxy-123" },
        descriptor: { backendSessionId: "paired-proxy-123", target: { hostId: "host-a", ownerId: pairedProject.workspaceId, epoch: "epoch-paired", backendSessionId: "paired-proxy-123" }, config: {}, clientRequestId: "request-1", remoteCursor: "0", cols: 80, rows: 24 },
        generation: 1, state: "disconnected", failure: null, replayGap: null,
      });
    });
    expect(outcomes).toEqual([{ kind: "pending" }]);
    expect(outcomes).toEqual([{ kind: "pending" }]);
  });
  it("cleans up timer and subscription on timeout or unmount", async () => {
    vi.useFakeTimers();
    try {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
      const persisted: PersistedWorkspaceSession = {
        version: 2, timestamp: Date.now(), activeWorkspaceId: pairedProject.workspaceId,
        workspaces: {
          [pairedProject.workspaceId]: {
            activeWorktreePath: pairedProject.repoRoot, workspaceId: pairedProject.workspaceId,
            repoRoot: pairedProject.repoRoot, target: pairedProject.target, remoteWorkspaceId: "remote-project",
            worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
            terminalSessions: {
              "paired-session": { localSessionId: "paired-session", backendSessionId: "paired-proxy-123",
                worktreePath: pairedProject.repoRoot, cwd: pairedProject.repoRoot, createdAt: Date.now(),
                daemonEpoch: "epoch-paired", incarnation: "inc-paired" },
            },
            layout: { splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
              tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }] },
          },
        },
      };
      native.loadSession.mockResolvedValue(persisted);
      native.retryPairedSession.mockResolvedValue({ admitted: false, ownerUnavailable: true, staleTuple: false, blocked: false, affirmativeExit: false });
      let unmount = () => {};
      unmount = (await renderRestoredApp()).unmount;
      const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
      await requestPairedAdmission(result => outcomes.push(result));
      expect(outcomes).toEqual([{ kind: "pending" }]);
      expect(native.terminalRemoteStatusHandler).not.toBeNull();
      act(() => {
        vi.advanceTimersByTime(15000);
      });
      expect(outcomes.at(-1)).toEqual({ kind: "cancelled" });
      expect(native.terminalRemoteStatusHandler).toBeNull();
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });
  it("rejects invalidated admission after real App loadSession and before saveSession", async () => {
    const localProject: RegisteredProject = {
      workspaceId: "local",
      repoRoot: "/local",
      gitRoot: null,
    };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([localProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "local");

    let release!: (value: null) => void;
    let entered!: () => void;
    const loading = new Promise<void>((resolve) => { entered = resolve; });
    const pending = new Promise<null>((resolve) => { release = resolve; });
    const registered = nextFixtureEvent("persistence-registered");
    try {
      render(<App />);
      await registered.promise;
    } finally {
      registered.dispose();
    }
    native.loadSession.mockImplementationOnce(() => { entered(); return pending; });
    expect(storeObservation.persist).toBeDefined();
    let current = true;
    const persistPromise = storeObservation.persist!("local", storeObservation.state!, () => current);
    const rejectedPromise = expect(persistPromise).rejects.toThrow("Recovery admission cancelled");
    persistPromise.catch(() => undefined);
    await loading;
    const before = native.saveSession.mock.calls.length;
    current = false;
    release(null);
    await rejectedPromise;
    expect(native.saveSession.mock.calls.length).toBe(before);
  }, 1000);

  it("forwards local identity refresh through the real owner until persisted admission is fresh", async () => {
    const localProject: RegisteredProject = { workspaceId: "local", repoRoot: "/local", gitRoot: null };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([localProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "local");
    const retained: PersistedWorkspaceSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "local",
      workspaces: {
        local: {
          activeWorktreePath: "/local",
          workspaceId: "local",
          repoRoot: "/local",
          worktrees: [{ path: "/local", branch: "", head: "", isMain: true, isLocked: false }],
          terminalSessions: {
            "local-session": {
              localSessionId: "local-session",
              backendSessionId: "backend-local",
              worktreePath: "/local",
              cwd: "/local",
              createdAt: Date.now(),
              daemonEpoch: "epoch-before",
              incarnation: "inc-local",
            },
          },
          layout: {
            splitMode: "none",
            primaryTabId: "local-tab",
            secondaryTabId: null,
            activeTabId: "local-tab",
            tabs: [{ id: "local-tab", label: "local terminal", sessionId: "local-session", kind: "terminal" }],
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(retained);
    native.describeTerminal.mockResolvedValue({
      sessionId: "backend-local", daemonEpoch: "epoch-after", incarnation: "inc-local", running: true,
      recoveryAuthority: { status: "running", daemonEpoch: "epoch-after", incarnation: "inc-local" },
    });
    let releaseSave!: () => void;
    let saveEntered!: () => void;
    const saveStarted = new Promise<void>((resolve) => { saveEntered = resolve; });
    const savePending = new Promise<void>((resolve) => { releaseSave = resolve; });
    native.saveSession.mockImplementation(async () => { saveEntered(); await savePending; });

    await renderRestoredApp();
    expect(requestRecoveryAdmission).not.toBeNull();
    expect(storeObservation.state?.sessions["local-session"]).toMatchObject({
      backendSessionId: "backend-local", incarnation: "inc-local", daemonEpoch: "epoch-before",
      remoteConnectionState: "reconnecting",
    });

    const results: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let resolveReady!: (result: import("./state/workspaceStore").LocalRecoveryAdmissionResult) => void;
    const readyResult = new Promise<import("./state/workspaceStore").LocalRecoveryAdmissionResult>((resolve) => { resolveReady = resolve; });
    let unsubscribe = () => {};
    act(() => {
      unsubscribe = requestRecoveryAdmission!("local-session", (result) => {
        results.push(result);
        if (result.kind === "ready") resolveReady(result);
      });
    });
    expect(results).toEqual([{ kind: "pending" }]);
    await saveStarted;
    expect(results).toEqual([{ kind: "pending" }]);
    expect(storeObservation.state?.sessions["local-session"]).toMatchObject({
      backendSessionId: "backend-local", incarnation: "inc-local", daemonEpoch: "epoch-before",
      remoteConnectionState: "reconnecting",
    });
    expect(native.saveSession.mock.calls.at(-1)?.[0].workspaces.local.terminalSessions["local-session"]).toMatchObject({
      localSessionId: "local-session", backendSessionId: "backend-local",
      incarnation: "inc-local", daemonEpoch: "epoch-after",
    });
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();

    let ready: import("./state/workspaceStore").LocalRecoveryAdmissionResult | undefined;
    await act(async () => {
      releaseSave();
      ready = await readyResult;
    });
    expect(ready).toMatchObject({ kind: "ready", session: {
      id: "local-session", backendSessionId: "backend-local", incarnation: "inc-local", daemonEpoch: "epoch-after",
    } });
    expect(storeObservation.state?.sessions["local-session"]).toMatchObject({
      backendSessionId: "backend-local", incarnation: "inc-local", daemonEpoch: "epoch-after",
    });
    expect(native.saveSession).toHaveBeenCalled();
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    unsubscribe();
  }, 1000);

  it("cancels cleanly without fake exit when terminal status registration rejects", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot,
          workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot,
          target: pairedProject.target,
          remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": {
              localSessionId: "paired-session",
              backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot,
              cwd: pairedProject.repoRoot,
              createdAt: Date.now(),
              daemonEpoch: "epoch-paired",
              incarnation: "inc-paired",
            },
          },
          layout: {
            splitMode: "none",
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }],
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.onTerminalRemoteStatus = vi.fn().mockRejectedValue(new Error("Registration failed"));

    await renderRestoredApp();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    let resolveCancelled!: (result: import("./state/workspaceStore").LocalRecoveryAdmissionResult) => void;
    const cancelledPromise = new Promise<import("./state/workspaceStore").LocalRecoveryAdmissionResult>((resolve) => {
      resolveCancelled = resolve;
    });
    const unsubscribe = requestRecoveryAdmission!("paired-session", (result) => {
      outcomes.push(result);
      if (result.kind === "cancelled") {
        resolveCancelled(result);
      }
    });

    await cancelledPromise;

    expect(outcomes).toEqual([{ kind: "pending" }, { kind: "cancelled" }]);
    expect(outcomes.every((o) => o.kind !== "exited")).toBe(true);
    expect(native.spawnTerminal).not.toHaveBeenCalled();
    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
    unsubscribe();
  });

  it("preserves pending on disconnected runtime status", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([pairedProject]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, pairedProject.workspaceId);
    const persisted: PersistedWorkspaceSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: pairedProject.workspaceId,
      workspaces: {
        [pairedProject.workspaceId]: {
          activeWorktreePath: pairedProject.repoRoot,
          workspaceId: pairedProject.workspaceId,
          repoRoot: pairedProject.repoRoot,
          target: pairedProject.target,
          remoteWorkspaceId: "remote-project",
          worktrees: [{ path: pairedProject.repoRoot, branch: "main", head: "HEAD", isMain: true, isLocked: false }],
          terminalSessions: {
            "paired-session": {
              localSessionId: "paired-session",
              backendSessionId: "paired-proxy-123",
              worktreePath: pairedProject.repoRoot,
              cwd: pairedProject.repoRoot,
              createdAt: Date.now(),
              daemonEpoch: "epoch-paired",
              incarnation: "inc-paired",
            },
          },
          layout: {
            splitMode: "none",
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", label: "paired terminal", sessionId: "paired-session", kind: "terminal" }],
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(persisted);
    native.retryPairedSession.mockResolvedValue({ admitted: true, ownerUnavailable: false, staleTuple: false, blocked: false, affirmativeExit: false });

    await renderRestoredApp();
    const outcomes: import("./state/workspaceStore").LocalRecoveryAdmissionResult[] = [];
    await requestPairedAdmission(result => outcomes.push(result));
    expect(outcomes).toEqual([{ kind: "pending" }]);

    await act(async () => {
      native.terminalRemoteStatusHandler?.({
        sessionId: "paired-proxy-123", daemonEpoch: "epoch-paired", incarnation: "inc-paired",
        target: { kind: "pairedDaemon", hostId: "host-a" },
        generation: 1, state: "disconnected", failure: null, replayGap: null,
      });
    });

    expect(outcomes).toEqual([{ kind: "pending" }]);
    expect(outcomes.some((o) => o.kind === "ready")).toBe(false);
  });
});

