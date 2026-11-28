import { act, cleanup, render, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  layoutGet: vi.fn(),
  layoutPut: vi.fn(),
}));

const syncMocks = vi.hoisted(() => ({
  stubLoop: false,
  toSharedLayoutSpy: vi.fn(),
  localChanged: vi.fn(),
  start: vi.fn(() => Promise.resolve()),
  stop: vi.fn(),
  lastApplyRemote: null as null | ((doc: SharedLayout) => void),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  layoutGet: mocks.layoutGet,
  layoutPut: mocks.layoutPut,
}));

vi.mock("./layoutSync", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./layoutSync")>();
  return {
    ...actual,
    toSharedLayout: (...args: Parameters<typeof actual.toSharedLayout>) => {
      syncMocks.toSharedLayoutSpy(...args);
      return actual.toSharedLayout(...args);
    },
    createLayoutSyncLoop: (options: Parameters<typeof actual.createLayoutSyncLoop>[0]) => {
      if (!syncMocks.stubLoop) {
        return actual.createLayoutSyncLoop(options);
      }
      syncMocks.lastApplyRemote = options.applyRemote;
      return {
        start: syncMocks.start,
        stop: syncMocks.stop,
        localChanged: syncMocks.localChanged,
      };
    },
  };
});

import type { PairedDaemonProjectAdapter } from "../lib/pairedDaemonProject";
import { isTerminalTab, type RegisteredProject, type TerminalSession, type WorkspaceTab, type Worktree } from "../lib/types";
import { createLayoutState, layoutReducer } from "./layout";
import type { SharedLayout, SharedTab } from "./layoutSync";
import { useLayoutSync } from "./useLayoutSync";
import type { WorkspaceAction, WorkspaceState } from "./workspaceStore";

const REPO_ROOT = "/Users/alice/repo";

function worktree(path: string, identity?: { wsId: string; slug: string }): Worktree {
  return {
    ...(identity ? { identity } : {}),
    path,
    head: "abc123",
    branch: null,
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
}

const featureWorktree = worktree(`${REPO_ROOT}/.ferryx-worktrees/ws/feat-1`, { wsId: "ws", slug: "feat-1" });
const secondWorktree = worktree(`${REPO_ROOT}/.ferryx-worktrees/ws/feat-2`, { wsId: "ws", slug: "feat-2" });
const mainCheckout = worktree(REPO_ROOT);
const externalWorktree = worktree("/Volumes/scratch/feature-b");

function localProject(): RegisteredProject {
  return { workspaceId: "ws", repoRoot: REPO_ROOT };
}

function pairedProject(suffix = "a"): RegisteredProject {
  return {
    workspaceId: `daemon:${suffix.repeat(64)}`,
    repoRoot: REPO_ROOT,
    target: { kind: "pairedDaemon", hostId: "host-1" },
    remoteWorkspaceId: `ws-${suffix}`,
  };
}

function stateWith(
  worktrees: Worktree[],
  activeWorktreePath: string | null,
): WorkspaceState {
  return {
    worktrees,
    activeWorktreePath,
    sessions: {},
    layout: createLayoutState(),
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

function doc(revision: number, tabs: SharedTab[] = [], worktreeKey?: string): SharedLayout {
  return { workspaceId: "ws", ...(worktreeKey ? { worktreeKey } : {}), revision, tabs };
}

function terminalTab(id: string, backendSessionId: string): SharedTab {
  return {
    id,
    kind: "terminal",
    label: id,
    pinned: false,
    paneTree: { type: "leaf", leafId: `${id}-leaf`, backendSessionId },
  };
}

type Handle = { setState: (updater: (state: WorkspaceState) => WorkspaceState) => void };

function Harness({
  project,
  initialState,
  adapter,
  onDispatch,
  handle,
}: {
  project: RegisteredProject;
  initialState: WorkspaceState;
  adapter?: PairedDaemonProjectAdapter | null;
  onDispatch: (action: WorkspaceAction) => void;
  handle: Handle;
}) {
  const [state, setState] = useState(initialState);
  handle.setState = setState;
  useLayoutSync({
    project,
    state,
    enabled: true,
    adapter: adapter ?? null,
    debounceMs: 0,
    dispatch: (action) => {
      onDispatch(action);
      // App applies a restore exactly like this; enrichment of a minted pane depends on it.
      if (action.type === "RESTORE_WORKSPACE") setState(action.state);
    },
  });
  return null;
}

function renderSync(
  initial: { project: RegisteredProject; state: WorkspaceState; adapter?: PairedDaemonProjectAdapter | null },
  options: { key?: string } = {},
) {
  const calls: WorkspaceAction[] = [];
  const handle: Handle = { setState: () => {} };
  const view = render(
    <Harness
      key={options.key}
      project={initial.project}
      initialState={initial.state}
      adapter={initial.adapter}
      handle={handle}
      onDispatch={(action) => calls.push(action)}
    />,
  );
  const rerenderWith = (next: typeof initial, key?: string) => view.rerender(
    <Harness
      key={key}
      project={next.project}
      initialState={next.state}
      adapter={next.adapter}
      handle={handle}
      onDispatch={(action) => calls.push(action)}
    />,
  );
  return { calls, handle, rerenderWith, unmount: view.unmount };
}

/** RESTORE_WORKSPACE actions that carry the host-side patch `enrichDetached` writes. */
function enrichedRestores(calls: WorkspaceAction[]) {
  return calls.filter((action): action is Extract<WorkspaceAction, { type: "RESTORE_WORKSPACE" }> =>
    action.type === "RESTORE_WORKSPACE"
    && Object.values(action.state.sessions).some((session) => session.cwd === "/host/worktree"));
}

function parkedRead(first: SharedLayout | null): void {
  mocks.layoutGet.mockImplementationOnce(async () => {
    if (first) return first;
    return new Promise<SharedLayout>(() => {});
  });
  mocks.layoutGet.mockImplementation(() => new Promise<SharedLayout>(() => {}));
}

/**
 * A paired adapter whose first read answers `layout` and whose later reads park, so the loop
 * never spins; `sessions` answers the enrichment read.
 */
function pairedAdapter(
  layout: SharedLayout,
  sessions: () => Promise<unknown>,
): PairedDaemonProjectAdapter {
  let reads = 0;
  return {
    layoutGet: vi.fn(async (_workspaceId: string, _afterRevision?: string, worktreeKey?: string) => {
      reads += 1;
      if (reads > 1) return new Promise<never>(() => {});
      return { layout: { ...layout, ...(worktreeKey ? { worktreeKey } : {}) } };
    }),
    layoutPut: vi.fn(async (_workspaceId: string, baseRevision: string, tabs: unknown, worktreeKey?: string) => ({
      layout: doc(Number(baseRevision) + 1, tabs as SharedTab[], worktreeKey),
      conflict: false,
    })),
    sessions: vi.fn(sessions),
  } as unknown as PairedDaemonProjectAdapter;
}

function hostSessionRow(backendSessionId: string) {
  return {
    sessions: [{
      target: { sessionId: backendSessionId, daemonEpoch: "7", machineId: "machine-1" },
      cwd: "/host/worktree",
      worktree: { wsId: "ws", slug: "feat-1" },
      running: true,
    }],
  };
}

describe("useLayoutSync transport scoping", () => {
  it("sends the managed worktree slug, never a path, for both desktop and paired clients", async () => {
    parkedRead(doc(1));
    mocks.layoutPut.mockImplementation(async (_workspaceId: string, baseRevision: number, tabs: SharedTab[], worktreeKey?: string) => ({
      layout: doc(baseRevision + 1, tabs, worktreeKey),
      conflict: false,
    }));

    const { handle, unmount } = renderSync({
      project: localProject(),
      state: stateWith([featureWorktree, secondWorktree], featureWorktree.path),
    });
    await waitFor(() => expect(mocks.layoutGet).toHaveBeenCalled());
    expect(mocks.layoutGet.mock.calls[0][2]).toBe("wt:feat-1");

    // A local layout change publishes into the same bucket.
    act(() => {
      handle.setState((state) => ({ ...state, layout: createLayoutState([{ id: "tab", label: "tab", sessionId: "s1" }], "tab") }));
    });
    await waitFor(() => expect(mocks.layoutPut).toHaveBeenCalled());
    expect(mocks.layoutPut.mock.calls[0][3]).toBe("wt:feat-1");

    // The paired transport derives the same key from the identity, even on a Windows-shaped path.
    const pairedWorktree = worktree("C:\\Users\\alice\\repo\\.ferryx-worktrees\\ws\\feat-1", { wsId: "ws", slug: "feat-1" });
    const adapter = pairedAdapter(doc(2), async () => hostSessionRow("none"));
    const paired = renderSync({
      project: pairedProject(),
      adapter,
      state: stateWith([pairedWorktree], pairedWorktree.path),
    });
    await waitFor(() => expect(adapter.layoutGet).toHaveBeenCalled());
    expect((adapter.layoutGet as unknown as { mock: { calls: unknown[][] } }).mock.calls[0][2]).toBe("wt:feat-1");

    for (const transmitted of [
      JSON.stringify(mocks.layoutGet.mock.calls),
      JSON.stringify(mocks.layoutPut.mock.calls),
      JSON.stringify((adapter.layoutGet as unknown as { mock: { calls: unknown[][] } }).mock.calls),
    ]) {
      expect(transmitted).not.toContain(REPO_ROOT);
      expect(transmitted).not.toContain("ferryx-worktrees");
      expect(transmitted).not.toContain("C:\\Users");
    }
    paired.unmount();
    unmount();
  });

  it("uses `main` for the main checkout and a digest for an unmanaged worktree", async () => {
    parkedRead(null);
    const { rerenderWith, unmount } = renderSync({
      project: localProject(),
      state: stateWith([mainCheckout, externalWorktree], mainCheckout.path),
    });
    await waitFor(() => expect(mocks.layoutGet).toHaveBeenCalled());
    expect(mocks.layoutGet.mock.calls[0][2]).toBe("main");

    rerenderWith(
      { project: localProject(), state: stateWith([mainCheckout, externalWorktree], externalWorktree.path) },
      "external",
    );
    await waitFor(() => expect(mocks.layoutGet.mock.calls.length).toBeGreaterThan(1));
    const externalKey = mocks.layoutGet.mock.calls.at(-1)?.[2];
    expect(externalKey).toMatch(/^ext:[0-9a-f]{16}$/);
    expect(JSON.stringify(mocks.layoutGet.mock.calls)).not.toContain("/Volumes/scratch");
    unmount();
  });

  it("scopes publish and poll per worktree bucket and keeps the workspace-level document otherwise", async () => {
    mocks.layoutGet.mockImplementation(async (_workspaceId: string, afterRevision: number | null, worktreeKey?: string) => {
      // A long poll on the revision the loop already holds parks instead of answering twice.
      if (worktreeKey === "wt:feat-1" && afterRevision === null) return doc(4, [terminalTab("tab-1", "backend-1")], worktreeKey);
      if (worktreeKey === "wt:feat-2" && afterRevision === null) return doc(9, [terminalTab("tab-2", "backend-2")], worktreeKey);
      return new Promise<SharedLayout>(() => {});
    });
    mocks.layoutPut.mockImplementation(async (_workspaceId: string, baseRevision: number, tabs: SharedTab[], worktreeKey?: string) => ({
      layout: doc(baseRevision + 1, tabs, worktreeKey),
      conflict: false,
    }));

    const first = renderSync({
      project: localProject(),
      state: stateWith([featureWorktree, secondWorktree], featureWorktree.path),
    });
    await waitFor(() => expect(mocks.layoutGet).toHaveBeenCalled());
    expect(mocks.layoutGet.mock.calls.every((call) => call[2] === "wt:feat-1")).toBe(true);
    // The feat-1 document landed on feat-1's own tab.
    expect(first.calls.some((action) => action.type === "RESTORE_WORKSPACE"
      && action.state.layout.tabs.some((tab) => tab.id === "tab-1"))).toBe(true);
    first.unmount();

    const second = renderSync({
      project: localProject(),
      state: stateWith([featureWorktree, secondWorktree], secondWorktree.path),
    });
    await waitFor(() => expect(mocks.layoutGet.mock.calls.some((call) => call[2] === "wt:feat-2")).toBe(true));
    // feat-1's tab never leaked into feat-2's apply, and every read named this workspace.
    expect(second.calls.some((action) => action.type === "RESTORE_WORKSPACE"
      && action.state.layout.tabs.some((tab) => tab.id === "tab-2"))).toBe(true);
    expect(second.calls.some((action) => action.type === "RESTORE_WORKSPACE"
      && action.state.layout.tabs.some((tab) => tab.id === "tab-1"))).toBe(false);
    expect(mocks.layoutGet.mock.calls.every((call) => call[0] === "ws")).toBe(true);
    second.unmount();

    // No worktree resolved: the document stays the workspace-level one.
    mocks.layoutGet.mockClear();
    parkedRead(doc(1));
    const third = renderSync({ project: localProject(), state: stateWith([], null) });
    await waitFor(() => expect(mocks.layoutGet).toHaveBeenCalled());
    expect(mocks.layoutGet.mock.calls[0][2]).toBeUndefined();
    third.unmount();
  });
});

describe("useLayoutSync detached-pane enrichment", () => {
  function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((settle) => { resolve = settle; });
    return { promise, resolve };
  }

  it("enriches a minted pane and parks the synced layout under the active worktree", async () => {
    const adapter = pairedAdapter(doc(2, [terminalTab("remote", "backend-9")]), async () => hostSessionRow("backend-9"));
    const { calls, unmount } = renderSync({
      project: pairedProject(),
      adapter,
      state: stateWith([featureWorktree], featureWorktree.path),
    });
    await waitFor(() => expect(enrichedRestores(calls)).toHaveLength(1));

    const restored = enrichedRestores(calls)[0];
    expect(restored.state.sessions["layout-sync:backend-9"]).toMatchObject({
      cwd: "/host/worktree",
      daemonEpoch: "7",
      lifecycle: "working",
      processState: "running",
    });
    // The parked copy of this worktree carries the synced layout and its live session id.
    const parked = restored.state.worktreeLayouts?.[featureWorktree.path];
    expect(parked?.tabs.map((tab) => tab.id)).toEqual(["remote"]);
    const parkedTab = parked?.tabs[0];
    if (!parkedTab || !isTerminalTab(parkedTab)) throw new Error("expected a parked terminal tab");
    expect(parkedTab.sessionId).toBe("layout-sync:backend-9");
    unmount();
  });

  it("does not dispatch after unmount", async () => {
    const gate = deferred<ReturnType<typeof hostSessionRow>>();
    const adapter = pairedAdapter(doc(2, [terminalTab("remote", "backend-9")]), () => gate.promise);
    const { calls, unmount } = renderSync({
      project: pairedProject(),
      adapter,
      state: stateWith([featureWorktree], featureWorktree.path),
    });
    await waitFor(() => expect(adapter.sessions).toHaveBeenCalled());
    unmount();

    await act(async () => {
      gate.resolve(hostSessionRow("backend-9"));
      await gate.promise;
    });
    expect(enrichedRestores(calls)).toHaveLength(0);
  });

  it("does not dispatch once the window switched to another workspace", async () => {
    const gate = deferred<ReturnType<typeof hostSessionRow>>();
    const adapter = pairedAdapter(doc(2, [terminalTab("remote", "backend-9")]), () => gate.promise);
    const project = pairedProject("a");
    const { calls, handle, unmount } = renderSync({
      project,
      adapter,
      state: { ...stateWith([featureWorktree], featureWorktree.path), workspaceId: project.workspaceId },
    });
    await waitFor(() => expect(adapter.sessions).toHaveBeenCalled());

    // The store repoints this hook's state at another workspace without changing the effect's
    // inputs: only the state check inside `enrichDetached` can notice.
    act(() => {
      handle.setState((state) => ({ ...state, workspaceId: "another-workspace" }));
    });

    await act(async () => {
      gate.resolve(hostSessionRow("backend-9"));
      await gate.promise;
    });
    expect(enrichedRestores(calls)).toHaveLength(0);
    unmount();
  });

  it("does not dispatch once the active worktree's key changed", async () => {
    const gate = deferred<ReturnType<typeof hostSessionRow>>();
    // Only the first effect's session read may resolve: the restarted effect parks, so any
    // dispatch after the switch could only come from the torn-down one.
    let reads = 0;
    const adapter = pairedAdapter(doc(2, [terminalTab("remote", "backend-9")]), () => {
      reads += 1;
      return reads === 1 ? gate.promise : new Promise<never>(() => {});
    });
    const { calls, handle, unmount } = renderSync({
      project: pairedProject(),
      adapter,
      state: stateWith([featureWorktree, secondWorktree], featureWorktree.path),
    });
    await waitFor(() => expect(adapter.sessions).toHaveBeenCalled());

    // Selecting another worktree changes `wireKey`, so this effect is torn down mid-flight.
    act(() => {
      handle.setState((state) => ({ ...state, activeWorktreePath: secondWorktree.path }));
    });

    await act(async () => {
      gate.resolve(hostSessionRow("backend-9"));
      await gate.promise;
    });
    expect(enrichedRestores(calls)).toHaveLength(0);
    unmount();
  });
});

function localTerminalTab(id: string, sessionId: string, label = id): WorkspaceTab {
  return { id, label, sessionId };
}

function makeSession(
  id: string,
  backendSessionId: string | null,
  overrides: Partial<TerminalSession> = {},
): TerminalSession {
  return {
    id,
    cwd: "/repo",
    workspaceId: "ws-1",
    worktree: null,
    backendSessionId,
    lifecycle: "working",
    processState: "running",
    ...overrides,
  };
}

function makeState(
  layout: WorkspaceState["layout"],
  sessions: Record<string, TerminalSession>,
): WorkspaceState {
  return {
    workspaceId: "ws-1",
    worktrees: [],
    activeWorktreePath: null,
    sessions,
    layout,
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

const fastPathProject: RegisteredProject = {
  workspaceId: "ws-1",
  repoRoot: "/repo",
};

function FastPathHarness({
  state,
  dispatch,
  enabled = true,
}: {
  state: WorkspaceState;
  dispatch: (action: WorkspaceAction) => void;
  enabled?: boolean;
}) {
  useLayoutSync({ project: fastPathProject, state, dispatch, enabled });
  return null;
}

describe("useLayoutSync fast-path guard", () => {
  beforeEach(() => {
    syncMocks.stubLoop = true;
    syncMocks.toSharedLayoutSpy.mockClear();
    syncMocks.localChanged.mockClear();
    syncMocks.start.mockClear();
    syncMocks.stop.mockClear();
    syncMocks.lastApplyRemote = null;
  });

  afterEach(() => {
    cleanup();
    syncMocks.stubLoop = false;
  });

  it("skips toSharedLayout when state.layout and every session backendSessionId are unchanged", () => {
    const layout = createLayoutState([localTerminalTab("tab-1", "s1")], "tab-1");
    const s1 = makeSession("s1", "backend-1");
    const dispatch = vi.fn();

    const { rerender } = render(
      <FastPathHarness state={makeState(layout, { s1 })} dispatch={dispatch} />,
    );

    // Mount initializes snapshot once inside the loop effect; the change-watcher effect hits fast path.
    expect(syncMocks.toSharedLayoutSpy).toHaveBeenCalledTimes(1);
    syncMocks.toSharedLayoutSpy.mockClear();

    // Unrelated session metadata update (cwd / lifecycle) with identical layout and backendSessionId.
    rerender(
      <FastPathHarness
        state={makeState(layout, {
          s1: makeSession("s1", "backend-1", { cwd: "/repo/subdir", lifecycle: "exited" }),
        })}
        dispatch={dispatch}
      />,
    );

    expect(syncMocks.toSharedLayoutSpy).not.toHaveBeenCalled();
    expect(syncMocks.localChanged).not.toHaveBeenCalled();
  });

  it("runs toSharedLayout and signals localChanged when a session backendSessionId changes or session count changes", () => {
    const layout = createLayoutState([localTerminalTab("tab-1", "s1")], "tab-1");
    const dispatch = vi.fn();

    const { rerender } = render(
      <FastPathHarness state={makeState(layout, { s1: makeSession("s1", "backend-1") })} dispatch={dispatch} />,
    );
    syncMocks.toSharedLayoutSpy.mockClear();
    syncMocks.localChanged.mockClear();

    // Changing backendSessionId must bypass the fast path and notify the sync loop.
    rerender(
      <FastPathHarness state={makeState(layout, { s1: makeSession("s1", "backend-2") })} dispatch={dispatch} />,
    );
    expect(syncMocks.toSharedLayoutSpy).toHaveBeenCalledTimes(1);
    expect(syncMocks.localChanged).toHaveBeenCalledTimes(1);

    syncMocks.toSharedLayoutSpy.mockClear();
    syncMocks.localChanged.mockClear();

    // Adding an extra session changes session count and must bypass the fast path.
    rerender(
      <FastPathHarness
        state={makeState(layout, {
          s1: makeSession("s1", "backend-2"),
          s2: makeSession("s2", null),
        })}
        dispatch={dispatch}
      />,
    );
    expect(syncMocks.toSharedLayoutSpy).toHaveBeenCalledTimes(1);
    expect(syncMocks.localChanged).not.toHaveBeenCalled();

    syncMocks.toSharedLayoutSpy.mockClear();

    // Removing that session changes session count again and must bypass the fast path.
    rerender(
      <FastPathHarness
        state={makeState(layout, {
          s1: makeSession("s1", "backend-2"),
        })}
        dispatch={dispatch}
      />,
    );
    expect(syncMocks.toSharedLayoutSpy).toHaveBeenCalledTimes(1);
  });

  it("bypasses the fast path when state.layout changes", () => {
    const layout1 = createLayoutState([localTerminalTab("tab-1", "s1")], "tab-1");
    const sessions = {
      s1: makeSession("s1", "backend-1"),
      s2: makeSession("s2", "backend-2"),
    };
    const dispatch = vi.fn();

    const { rerender } = render(
      <FastPathHarness state={makeState(layout1, sessions)} dispatch={dispatch} />,
    );
    syncMocks.toSharedLayoutSpy.mockClear();
    syncMocks.localChanged.mockClear();

    const layout2 = layoutReducer(layout1, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      direction: "horizontal",
      sessionId: "s2",
      ratio: 0.5,
    });
    rerender(<FastPathHarness state={makeState(layout2, sessions)} dispatch={dispatch} />);

    expect(syncMocks.toSharedLayoutSpy).toHaveBeenCalledTimes(1);
    expect(syncMocks.localChanged).toHaveBeenCalledTimes(1);
  });

  it("keeps prevLayoutRef and prevBackendIdsRef in sync inside applyRemote so committing remote state hits the fast path", () => {
    const initialLayout = createLayoutState([localTerminalTab("tab-1", "s1")], "tab-1");
    const initialState = makeState(initialLayout, {
      s1: makeSession("s1", "backend-1"),
    });
    let restoredState: WorkspaceState | null = null;
    const dispatch = vi.fn((action: WorkspaceAction) => {
      if (action.type === "RESTORE_WORKSPACE") {
        restoredState = action.state;
      }
    });

    const { rerender } = render(<FastPathHarness state={initialState} dispatch={dispatch} />);
    syncMocks.toSharedLayoutSpy.mockClear();
    syncMocks.localChanged.mockClear();

    const remoteDoc: SharedLayout = {
      workspaceId: "ws-1",
      revision: 2,
      tabs: [
        {
          id: "tab-1",
          kind: "terminal",
          label: "tab-1",
          pinned: false,
          paneTree: {
            type: "split",
            direction: "horizontal",
            ratio: 0.5,
            first: { type: "leaf", leafId: "leaf-a", backendSessionId: "backend-1" },
            second: { type: "leaf", leafId: "leaf-b", backendSessionId: "backend-remote" },
          },
        },
      ],
    };

    act(() => {
      syncMocks.lastApplyRemote?.(remoteDoc);
    });

    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(restoredState).not.toBeNull();
    // applyRemote called toSharedLayout once to compute the applied baseline.
    expect(syncMocks.toSharedLayoutSpy).toHaveBeenCalledTimes(1);
    syncMocks.toSharedLayoutSpy.mockClear();

    // Committing the exact restored layout & sessions must hit the fast path with zero re-serialization.
    rerender(<FastPathHarness state={restoredState!} dispatch={dispatch} />);
    expect(syncMocks.toSharedLayoutSpy).not.toHaveBeenCalled();
    expect(syncMocks.localChanged).not.toHaveBeenCalled();
  });
});
