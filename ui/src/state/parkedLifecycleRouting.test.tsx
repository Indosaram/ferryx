import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resolveActivityIndicator } from "../lib/activity";
import * as tauri from "../lib/tauri";
import type { TerminalDescribeResult } from "../lib/tauri";
import type { TerminalLifecyclePayload, Worktree } from "../lib/types";

const lifecycleListeners = vi.hoisted(() => new Set<(payload: TerminalLifecyclePayload) => void>());

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/tauri")>(),
  onTerminalOutput: async () => () => undefined,
  onTerminalLifecycle: async (handler: (payload: TerminalLifecyclePayload) => void) => {
    lifecycleListeners.add(handler);
    return () => lifecycleListeners.delete(handler);
  },
  onNativeTerminalTitle: async () => () => undefined,
  onNativeTerminalBell: async () => () => undefined,
  onNativeTerminalAgentState: async () => () => undefined,
  onNativeTerminalFocus: async () => () => undefined,
  discoverAgentProviderSession: async () => null,
}));

const { terminalEventBus } = await import("../lib/terminalEvents");
const {
  selectWorktreeActivitySummariesAcrossWorkspaces,
  useWorkspaceStore,
  workspaceReducer,
  setLocalRecoveryPersistence,
} = await import("./workspaceStore");
type WorkspaceServices = import("./workspaceStore").WorkspaceServices;
const { clearWorkspaceSnapshot, getWorkspaceSnapshot, setWorkspaceSnapshot, subscribeWorkspaceSnapshots } = await import("./workspaceSnapshotCache");

const alpha: Worktree = {
  path: "/repos/alpha",
  head: "aaa",
  branch: "refs/heads/orca/alpha/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function onSnapshot(workspaceId: string, ready: (state: import("./workspaceStore").WorkspaceState) => boolean) {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  const unsubscribe = subscribeWorkspaceSnapshots(() => {
    const state = getWorkspaceSnapshot(workspaceId);
    if (state && ready(state)) { unsubscribe(); resolve(); }
  });
  return promise;
}

function services(): WorkspaceServices {
  return {
    ensureTerminalEvents: async () => { await terminalEventBus.ensureStarted(); },
    spawnTerminal: async () => "backend-alpha",
    getTerminalCwd: async () => alpha.path,
    closeTerminal: async () => undefined,
    waitForTerminalExit: async () => undefined,
  };
}

beforeEach(() => {
  clearWorkspaceSnapshot();
  setLocalRecoveryPersistence(async () => {});
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
});

afterEach(() => {
  cleanup();
  clearWorkspaceSnapshot();
  setLocalRecoveryPersistence(undefined);
  vi.restoreAllMocks();
});

describe("terminal lifecycle reaches a parked workspace", () => {
  it("automatically recovers ordinary nonsleeping lifecycle loss and persists before ready", async () => {
    let persisted!: () => void;
    const saving = new Promise<void>((resolve) => { persisted = resolve; });
    let release!: () => void;
    const saved = new Promise<void>((resolve) => { release = resolve; });
    let admitted!: () => void;
    const ready = new Promise<void>((resolve) => { admitted = resolve; });
    const describe = vi.spyOn(tauri, "describeTerminal").mockResolvedValue({
      sessionId: "backend-alpha", cols: 80, rows: 24, running: true,
      incarnation: "ordinary", daemonEpoch: "new",
      recoveryAuthority: { status: "running", incarnation: "ordinary", daemonEpoch: "new" },
    });
    const service = services();
    const spawn = vi.spyOn(service, "spawnTerminal");
    const close = vi.spyOn(service, "closeTerminal");
    const hook = renderHook(() => useWorkspaceStore({ workspaceId: "ordinary", initialWorktrees: [alpha], services: service }));
    await act(async () => { await hook.result.current.openTab(alpha); });
    const sessionId = Object.keys(hook.result.current.state.sessions)[0];
    act(() => hook.result.current.dispatchWorkspaceAction({ type: "REBIND_SESSION_BACKEND", sessionId,
      backendSessionId: "backend-alpha", incarnation: "ordinary", daemonEpoch: "old" }));
    expect(hook.result.current.state.sessions[sessionId].remoteConnectionState).not.toBe("reconnecting");
    const unsubscribe = subscribeWorkspaceSnapshots(() => {
      if (getWorkspaceSnapshot("ordinary")?.sessions[sessionId]?.daemonEpoch === "new") admitted();
    });
    setLocalRecoveryPersistence(async () => { persisted(); await saved; });
    await act(async () => {
      for (const listener of lifecycleListeners) listener({ sessionId: "backend-alpha", state: "exited", exitCode: null, reason: "lost" });
      await saving;
    });
    expect(getWorkspaceSnapshot("ordinary")?.sessions[sessionId]).toMatchObject({ backendSessionId: "backend-alpha", daemonEpoch: "old", remoteConnectionState: "reconnecting" });
    await act(async () => { release(); await ready; });
    expect(describe).toHaveBeenCalledTimes(1);
    expect(spawn).toHaveBeenCalledTimes(1);
    expect(close).not.toHaveBeenCalled();
    unsubscribe(); hook.unmount();
  }, 1000);
  it("settles a parked project's working agent when its PTY exits", async () => {
    const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValue({
      sessionId: "backend-alpha",
      cols: 80,
      rows: 24,
      running: false,
      incarnation: "inc-alpha-0",
      daemonEpoch: "epoch-alpha-0",
      recoveryAuthority: {
        status: "affirmative_exit",
        exitCode: 0,
        incarnation: "inc-alpha-0",
        daemonEpoch: "epoch-alpha-0",
      },
    });
    const { result, rerender, unmount } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees: [alpha], services: services() }),
      { initialProps: { workspaceId: "project-a" } },
    );

    await act(async () => { await terminalEventBus.ensureStarted(); });
    expect(lifecycleListeners.size).toBe(1);

    let openedTabId: string | null = null;
    await act(async () => { openedTabId = await result.current.openTab(alpha); });
    const tabId: string = openedTabId ?? ((): never => { throw new Error("expected a tab"); })();
    const openedTab = result.current.state.layout.tabs[0];
    if (openedTab.kind === "browser" || openedTab.kind === "file" || openedTab.kind === "changes") throw new Error("expected a terminal tab");
    const sessionId = openedTab.sessionId;

    act(() => result.current.dispatchWorkspaceAction({
      type: "REBIND_SESSION_BACKEND",
      sessionId,
      backendSessionId: "backend-alpha",
      daemonEpoch: "epoch-alpha-0",
      incarnation: "inc-alpha-0",
      recoveryToken: 1,
      recoveryKey: "local:alpha:backend-alpha:inc-alpha-0",
    }));

    act(() => result.current.dispatchWorkspaceAction({
      type: "SESSION_SCREEN_ACTIVITY",
      tabId,
      sessionId,
      state: "working",
      ruleId: "extension",
      manifestId: "omo",
    }));
    expect(result.current.state.activityBySessionId?.[sessionId]?.state).toBe("working");

    // The user switches to another project; project-a now lives only in the snapshot cache.
    rerender({ workspaceId: "project-b" });
    expect(
      resolveActivityIndicator(selectWorktreeActivitySummariesAcrossWorkspaces("project-b")[alpha.path]),
    ).toBe("working");

    const settled = onSnapshot("project-a", (state) => state.sessions[sessionId]?.lifecycle === "exited");
    await act(async () => {
      for (const listener of lifecycleListeners) {
        listener({ sessionId: "backend-alpha", state: "exited", exitCode: 0, reason: null });
      }
      await settled;
    });

    const parked = getWorkspaceSnapshot("project-a");
    expect(parked?.sessions[sessionId]?.lifecycle).toBe("exited");
    expect(parked?.activityBySessionId?.[sessionId]?.state).toBe("done");
    expect(
      resolveActivityIndicator(selectWorktreeActivitySummariesAcrossWorkspaces("project-b")[alpha.path]),
    ).not.toBe("working");
    expect(describeSpy).toHaveBeenCalledTimes(1);
    describeSpy.mockRestore();
    unmount();
  });

  it("coalesces burst unconfirmed lifecycle events on mounted active hook and reconciles via describe", async () => {
    const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValue({
      sessionId: "backend-alpha",
      cols: 80,
      rows: 24,
      running: false,
      incarnation: "inc-alpha-1",
      daemonEpoch: "epoch-alpha-1",
      recoveryAuthority: {
        status: "affirmative_exit",
        exitCode: 0,
        incarnation: "inc-alpha-1",
        daemonEpoch: "epoch-alpha-1",
      },
    });

    const { result, unmount } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees: [alpha], services: services() }),
      { initialProps: { workspaceId: "project-active" } },
    );

    await act(async () => { await terminalEventBus.ensureStarted(); });

    await act(async () => { void (await result.current.openTab(alpha)); });
    const openedTab = result.current.state.layout.tabs[0];
    if (openedTab.kind === "browser" || openedTab.kind === "file" || openedTab.kind === "changes") throw new Error("expected a terminal tab");
    const sessionId = openedTab.sessionId;

    // Set session matching context on state
    act(() => result.current.dispatchWorkspaceAction({
      type: "REBIND_SESSION_BACKEND",
      sessionId,
      backendSessionId: "backend-alpha",
      daemonEpoch: "epoch-alpha-1",
      incarnation: "inc-alpha-1",
      recoveryToken: 1,
      recoveryKey: "local:alpha:backend-alpha:inc-alpha-1",
    }));

    const settled = onSnapshot("project-active", (state) => state.sessions[sessionId]?.lifecycle === "exited");
    await act(async () => {
      for (let i = 0; i < 5; i++) {
        for (const listener of lifecycleListeners) {
          listener({ sessionId: "backend-alpha", state: "exited", exitCode: 0, reason: null });
        }
      }
      await settled;
    });

    // Coalescing ensures describeTerminal was called only once for the burst
    expect(describeSpy).toHaveBeenCalledTimes(1);
    describeSpy.mockRestore();
    unmount();
  });

  it("coalesces burst unconfirmed lifecycle events for parked workspace and reconciles snapshot", async () => {
    const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValue({
      sessionId: "backend-alpha",
      cols: 80,
      rows: 24,
      running: false,
      incarnation: "inc-parked-1",
      daemonEpoch: "epoch-parked-1",
      recoveryAuthority: {
        status: "affirmative_exit",
        exitCode: 0,
        incarnation: "inc-parked-1",
        daemonEpoch: "epoch-parked-1",
      },
    });

    const { result, rerender, unmount } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees: [alpha], services: services() }),
      { initialProps: { workspaceId: "project-parked-source" } },
    );

    await act(async () => { await terminalEventBus.ensureStarted(); });

    await act(async () => { void (await result.current.openTab(alpha)); });
    const openedTab = result.current.state.layout.tabs[0];
    if (openedTab.kind === "browser" || openedTab.kind === "file" || openedTab.kind === "changes") throw new Error("expected a terminal tab");
    const sessionId = openedTab.sessionId;

    act(() => result.current.dispatchWorkspaceAction({
      type: "REBIND_SESSION_BACKEND",
      sessionId,
      backendSessionId: "backend-alpha",
      daemonEpoch: "epoch-parked-1",
      incarnation: "inc-parked-1",
      recoveryToken: 2,
      recoveryKey: "local:parked:backend-alpha:inc-parked-1",
    }));

    act(() => result.current.dispatchWorkspaceAction({
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: openedTab.id,
      sessionId,
      state: "working",
      ruleId: "extension",
      manifestId: "omo",
    }));

    // Switch to another workspace so project-parked-source is parked in snapshot cache
    rerender({ workspaceId: "project-other" });

    const settled = onSnapshot("project-parked-source", (state) => state.sessions[sessionId]?.lifecycle === "exited");
    await act(async () => {
      for (let i = 0; i < 3; i++) {
        for (const listener of lifecycleListeners) {
          listener({ sessionId: "backend-alpha", state: "exited", exitCode: 0, reason: null });
        }
      }
      await settled;
    });

    // Coalescing ensures describe was called once
    expect(describeSpy).toHaveBeenCalledTimes(1);
    const parked = getWorkspaceSnapshot("project-parked-source");
    expect(parked?.sessions[sessionId]?.lifecycle).toBe("exited");
    expect(parked?.sessions[sessionId]?.backendSessionId).toBeNull();
    expect(parked?.activityBySessionId?.[sessionId]?.state).toBe("done");
    describeSpy.mockRestore();
    unmount();
  });

  it("ignores stale describe completion if parked session is closed via CLOSE_TAB before describe resolves (no resurrection)", async () => {
    let resolveDescribe: ((value: TerminalDescribeResult) => void) | undefined;
    const describePromise = new Promise<TerminalDescribeResult>((resolve) => {
      resolveDescribe = resolve;
    });
    const describeSpy = vi.spyOn(tauri, "describeTerminal").mockReturnValue(describePromise);

    const { result, rerender, unmount } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees: [alpha], services: services() }),
      { initialProps: { workspaceId: "project-parked-close" } },
    );

    await act(async () => { await terminalEventBus.ensureStarted(); });

    let openedTabId: string | null = null;
    await act(async () => { openedTabId = await result.current.openTab(alpha); });
    const tabId: string = openedTabId ?? ((): never => { throw new Error("expected a tab"); })();
    const openedTab = result.current.state.layout.tabs[0];
    if (openedTab.kind === "browser" || openedTab.kind === "file" || openedTab.kind === "changes") throw new Error("expected a terminal tab");
    const sessionId = openedTab.sessionId;

    act(() => result.current.dispatchWorkspaceAction({
      type: "REBIND_SESSION_BACKEND",
      sessionId,
      backendSessionId: "backend-close-1",
      daemonEpoch: "epoch-close-1",
      incarnation: "inc-close-1",
      recoveryToken: 1,
      recoveryKey: "local:close:backend-close-1:inc-close-1",
    }));

    // Park workspace in snapshot cache
    rerender({ workspaceId: "project-different" });

    // Trigger unconfirmed lifecycle exit for backend-close-1, starting in-flight describe
    act(() => {
      for (const listener of lifecycleListeners) {
        listener({ sessionId: "backend-close-1", state: "exited", exitCode: 0, reason: null });
      }
    });

    expect(describeSpy).toHaveBeenCalledTimes(1);

    // Close the parked tab using actual CLOSE_TAB action on parked snapshot (no accessor simulation)
    const parked = getWorkspaceSnapshot("project-parked-close");
    expect(parked?.sessions[sessionId]).toBeDefined();
    if (parked) {
      setWorkspaceSnapshot("project-parked-close", workspaceReducer(parked, { type: "CLOSE_TAB", tabId }));
    }
    const closedParked = getWorkspaceSnapshot("project-parked-close");
    expect(closedParked?.sessions[sessionId]).toBeUndefined();

    // Now deferred describe resolves with affirmative exit for the closed session
    await act(async () => {
      resolveDescribe?.({
        sessionId: "backend-close-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-close-1",
        daemonEpoch: "epoch-close-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-close-1",
          daemonEpoch: "epoch-close-1",
        },
      });
      await describePromise;
    });

    // Verify no resurrection: session remains closed and absent from parked snapshot
    const finalParked = getWorkspaceSnapshot("project-parked-close");
    expect(finalParked?.sessions[sessionId]).toBeUndefined();

    describeSpy.mockRestore();
    unmount();
  });

  it("ignores stale describe completion if parked session is rebound via REBIND_SESSION_BACKEND before describe resolves", async () => {
    let resolveDescribe: ((value: TerminalDescribeResult) => void) | undefined;
    const describePromise = new Promise<TerminalDescribeResult>((resolve) => {
      resolveDescribe = resolve;
    });
    const describeSpy = vi.spyOn(tauri, "describeTerminal").mockReturnValue(describePromise);

    const { result, rerender, unmount } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees: [alpha], services: services() }),
      { initialProps: { workspaceId: "project-parked-rebind" } },
    );

    await act(async () => { await terminalEventBus.ensureStarted(); });

    await act(async () => { void (await result.current.openTab(alpha)); });
    const openedTab = result.current.state.layout.tabs[0];
    if (openedTab.kind === "browser" || openedTab.kind === "file" || openedTab.kind === "changes") throw new Error("expected a terminal tab");
    const sessionId = openedTab.sessionId;

    act(() => result.current.dispatchWorkspaceAction({
      type: "REBIND_SESSION_BACKEND",
      sessionId,
      backendSessionId: "backend-stale-1",
      daemonEpoch: "epoch-stale-1",
      incarnation: "inc-stale-1",
      recoveryToken: 1,
      recoveryKey: "local:stale:backend-stale-1:inc-stale-1",
    }));

    // Park workspace in snapshot cache
    rerender({ workspaceId: "project-different" });

    // Trigger unconfirmed lifecycle exit for backend-stale-1, starting in-flight describe
    act(() => {
      for (const listener of lifecycleListeners) {
        listener({ sessionId: "backend-stale-1", state: "exited", exitCode: 0, reason: null });
      }
    });

    expect(describeSpy).toHaveBeenCalledTimes(1);

    // Rebind the parked session using actual REBIND_SESSION_BACKEND action on parked snapshot
    const parked = getWorkspaceSnapshot("project-parked-rebind");
    expect(parked).toBeDefined();
    if (parked) {
      setWorkspaceSnapshot("project-parked-rebind", workspaceReducer(parked, {
        type: "REBIND_SESSION_BACKEND",
        sessionId,
        backendSessionId: "backend-new-2",
        incarnation: "inc-new-2",
        daemonEpoch: "epoch-new-2",
        recoveryToken: 2,
        recoveryKey: "local:stale:backend-new-2:inc-new-2",
      }));
    }

    // Now describe resolves with stale exit proof for the old backend session
    await act(async () => {
      resolveDescribe?.({
        sessionId: "backend-stale-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-stale-1",
        daemonEpoch: "epoch-stale-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-stale-1",
          daemonEpoch: "epoch-stale-1",
        },
      });
      await describePromise;
    });

    // Stale completion was ignored due to conflict; session retains new backend state
    const updatedParked = getWorkspaceSnapshot("project-parked-rebind");
    expect(updatedParked?.sessions[sessionId]?.backendSessionId).toBe("backend-new-2");
    expect(updatedParked?.sessions[sessionId]?.lifecycle).toBe("running");

    describeSpy.mockRestore();
    unmount();
  });
});
