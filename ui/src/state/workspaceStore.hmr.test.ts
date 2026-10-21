import { afterEach, expect, it, vi } from "vitest";
import type { WorkspaceState } from "./workspaceStore";

const probes = vi.hoisted(() => ({ describe: vi.fn(), close: vi.fn(), listeners: new Set<() => void>() }));
vi.mock("../lib/tauri", async (original) => ({
  ...await original<typeof import("../lib/tauri")>(),
  describeTerminal: probes.describe, closeTerminal: probes.close,
}));
vi.mock("./workspaceSnapshotCache", async (original) => {
  const actual = await original<typeof import("./workspaceSnapshotCache")>();
  return { ...actual, subscribeWorkspaceSnapshots: (listener: () => void) => {
    probes.listeners.add(listener);
    const unsubscribe = actual.subscribeWorkspaceSnapshots(listener);
    return () => { probes.listeners.delete(listener); unsubscribe(); };
  } };
});
import * as cache from "./workspaceSnapshotCache";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.(); dispose = undefined;
  cache.clearWorkspaceSnapshot();
  vi.unstubAllGlobals();
});

it("disposes and reinstalls the actual snapshot owner without closing retained PTYs", async () => {
  vi.stubGlobal("__workspaceStoreTestHot", { dispose: (callback: () => void) => { dispose = callback; } });
  const first = await import("./workspaceStore");
  expect(probes.listeners.size).toBe(1);
  const entered = deferred<void>();
  const reply = deferred<Awaited<ReturnType<typeof import("../lib/tauri").describeTerminal>>>();
  probes.describe.mockImplementationOnce(() => { entered.resolve(); return reply.promise; });
  const state: WorkspaceState = {
    workspaceId: "hmr", worktrees: [], activeWorktreePath: null,
    sessions: { pane: { id: "pane", workspaceId: "hmr", worktree: null, worktreePath: "/repo",
      cwd: "/repo", backendSessionId: "backend", incarnation: "process", daemonEpoch: "old",
      lifecycle: "running", remoteConnectionState: "reconnecting" } },
    layout: { tabs: [], activeTabId: null, primaryTabId: null, secondaryTabId: null, split: "none", layoutsByTabId: {} },
    unreadTabIds: {}, unreadWorktreePaths: {},
  };
  cache.setWorkspaceSnapshot("hmr", state);
  const persist = vi.fn(async () => {});
  first.setLocalRecoveryPersistence(persist);
  await entered.promise;
  dispose?.(); dispose = undefined;
  expect(probes.listeners.size).toBe(0);
  const retained = cache.getWorkspaceSnapshot("hmr");
  reply.resolve({ sessionId: "backend", cols: 80, rows: 24, running: true,
    incarnation: "process", daemonEpoch: "new",
    recoveryAuthority: { status: "running", incarnation: "process", daemonEpoch: "new" } });
  await reply.promise;
  expect(cache.getWorkspaceSnapshot("hmr")).toBe(retained);
  expect(persist).not.toHaveBeenCalled();
  vi.resetModules();
  vi.doMock("./workspaceSnapshotCache", () => cache);
  const second = await import("./workspaceStore");
  expect(probes.listeners.size).toBe(1);
  const ready = deferred<void>();
  const unsubscribe = cache.subscribeWorkspaceSnapshots(() => {
    if (cache.getWorkspaceSnapshot("hmr")?.sessions.pane.daemonEpoch === "new") ready.resolve();
  });
  probes.describe.mockResolvedValue({ sessionId: "backend", cols: 80, rows: 24, running: true,
    incarnation: "process", daemonEpoch: "new",
    recoveryAuthority: { status: "running", incarnation: "process", daemonEpoch: "new" } });
  second.setLocalRecoveryPersistence(persist);
  await ready.promise;
  unsubscribe();
  expect(persist).toHaveBeenCalledTimes(1);
  expect(probes.close).not.toHaveBeenCalled();
  expect(probes.listeners.size).toBe(1);
});
