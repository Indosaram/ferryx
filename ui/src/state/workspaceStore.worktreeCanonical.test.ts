import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as browserTauri from "../lib/browserTauri";
import type { LayoutState, TerminalSession, WorkspaceTab, Worktree } from "../lib/types";
import { createLayoutState } from "./layout";
import { useWorkspaceStore, workspaceReducer, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

const storedPath = "C:\\Repo\\.ferryx-worktrees\\ws\\wt";
const listedPath = "c:/repo/.ferryx-worktrees/ws/wt/";
const worktree = (path: string): Worktree => ({ path, head: "abc", branch: null, bare: false, detached: false, locked: null, prunable: null });
const session = (overrides: Partial<TerminalSession> & Pick<TerminalSession, "id">): TerminalSession => ({
  cwd: "", workspaceId: "ws", worktree: null, backendSessionId: null, lifecycle: "working", ...overrides,
});
const main = worktree("C:/Repo");
const feature = worktree(listedPath);

function browserLayout(browserId: string): LayoutState {
  return {
    ...createLayoutState(),
    tabs: [{ kind: "browser", id: `tab-${browserId}`, label: browserId, browserId, url: "about:blank" }],
  };
}

function state(overrides: Partial<WorkspaceState> = {}): WorkspaceState {
  return {
    worktrees: [main, feature], activeWorktreePath: main.path, sessions: {},
    layout: createLayoutState(), worktreeLayouts: {}, unreadTabIds: {}, unreadWorktreePaths: {},
    ...overrides,
  };
}

afterEach(() => vi.restoreAllMocks());

describe("canonical worktree layout ownership", () => {
  it("preserves parked tabs and unread marks across Windows slash/case changes", () => {
    const parked = browserLayout("parked");
    parked.tabs.push({ id: "terminal", label: "Shell", sessionId: "session" });
    const next = workspaceReducer(state({
      sessions: { session: session({ id: "session", cwd: storedPath, worktreePath: storedPath, backendSessionId: "backend" }) },
      worktreeLayouts: { [storedPath]: parked },
      unreadWorktreePaths: { [storedPath]: true, "C:/Repo/deleted": true },
      unreadTabIds: { "tab-parked": true },
    }), { type: "SET_WORKTREES", worktrees: [main, feature] });

    expect(next.worktreeLayouts).toEqual({ [storedPath]: parked });
    expect(next.unreadWorktreePaths).toEqual({ [storedPath]: true });
    expect(next.unreadTabIds).toEqual({ "tab-parked": true });
    expect(next.sessions.session.backendSessionId).toBe("backend");
  });

  it("does not replace an active layout when only its listed path spelling changes", () => {
    const layout = browserLayout("active");
    const next = workspaceReducer(state({ activeWorktreePath: storedPath, layout }), {
      type: "SET_WORKTREES", worktrees: [main, feature],
    });
    expect(next.activeWorktreePath).toBe(storedPath);
    expect(next.layout).toBe(layout);
  });

  it("loads the canonical fallback layout when the active worktree is deleted", () => {
    const parked = browserLayout("fallback");
    const next = workspaceReducer(state({
      activeWorktreePath: "C:/Repo/deleted", layout: browserLayout("deleted"),
      worktreeLayouts: { [storedPath]: parked },
    }), { type: "SET_WORKTREES", worktrees: [feature] });

    expect(next.activeWorktreePath).toBe(listedPath);
    expect(next.layout).toBe(parked);
    expect(next.worktreeLayouts).toEqual({});
  });

  it("selects a canonical parked key and overwrites the previous owner's equivalent key", () => {
    const active = browserLayout("active");
    const parked = browserLayout("parked");
    const next = workspaceReducer(state({
      layout: active,
      worktreeLayouts: { "c:\\repo\\": browserLayout("outdated"), [storedPath]: parked },
    }), { type: "SELECT_WORKTREE", path: storedPath });

    expect(next.activeWorktreePath).toBe(listedPath);
    expect(next.layout).toBe(parked);
    expect(next.worktreeLayouts).toEqual({ "c:\\repo\\": active });
    const back = workspaceReducer(next, { type: "SELECT_WORKTREE", path: "c:/repo/" });
    expect(back.layout).toBe(active);
    expect(back.worktreeLayouts).toEqual({ [listedPath]: parked });
  });

  it("does not park the active layout when reselecting its canonical alias", () => {
    const original = state({ activeWorktreePath: storedPath, layout: browserLayout("active") });
    expect(workspaceReducer(original, { type: "SELECT_WORKTREE", path: listedPath })).toBe(original);
  });

  it("clears deleted browser, file and changes tabs when the fallback has no layout", () => {
    const tabs: WorkspaceTab[] = [
      ...browserLayout("deleted").tabs,
      { kind: "file", id: "file", label: "file", path: "file.ts", backendSessionId: "backend",
        line: null, col: null, workspaceId: null, previewId: "preview" },
      { kind: "changes", id: "changes", label: "Changes", workspaceId: "ws", worktreePath: storedPath, worktree: null },
    ];
    const next = workspaceReducer(state({
      activeWorktreePath: storedPath, layout: { ...createLayoutState(), tabs },
      unreadTabIds: { file: true, changes: true },
    }), { type: "SET_WORKTREES", worktrees: [main] });

    expect(next.activeWorktreePath).toBe(main.path);
    expect(next.layout).toEqual(createLayoutState());
    expect(next.unreadTabIds).toEqual({});
  });

  it.each([false, true])("closes removed browser tabs and split webviews once (saved fallback: %s)", async (savedFallback) => {
    const closeBrowser = vi.spyOn(browserTauri, "closeBrowser").mockResolvedValue(undefined);
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined), spawnTerminal: vi.fn(async () => "unused"),
      getTerminalCwd: vi.fn(async () => main.path), closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };
    const removed = browserLayout("active");
    removed.layoutsByTabId["tab-active"] = {
      root: { type: "split", direction: "horizontal", ratio: 0.5,
        first: { type: "leaf", leafId: "nested" }, second: { type: "leaf", leafId: "legacy" } },
      activeLeafId: "nested", expandedLeafId: null, sessionIdsByLeafId: {},
      contentsByLeafId: {
        nested: { kind: "browser", browser: { browserId: "nested", url: "about:blank" } },
        legacy: { kind: "browser", browserId: "legacy", url: "about:blank" },
      },
    };
    const parked = browserLayout("parked");
    parked.tabs.push(...browserLayout("active").tabs, ...browserLayout("retained").tabs);
    const { result, unmount } = renderHook(() => useWorkspaceStore({ initialWorktrees: [main, feature], services }));
    try {
      act(() => result.current.restoreWorkspace(state({
        activeWorktreePath: storedPath, layout: removed,
        worktreeLayouts: { "C:/Repo/deleted": parked, ...(savedFallback ? { "c:\\repo\\": browserLayout("retained") } : {}) },
        sessions: { stale: session({ id: "stale", backendSessionId: "backend-stale", cwd: storedPath }) },
      })));
      await act(async () => { await result.current.syncWorktrees([main]); });

      expect(closeBrowser.mock.calls.map(([id]) => id).sort()).toEqual(
        ["active", "legacy", "nested", "parked", ...(savedFallback ? [] : ["retained"])],
      );
      expect(services.closeTerminal).toHaveBeenCalledExactlyOnceWith("backend-stale");
      expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual(savedFallback ? ["tab-retained"] : []);
      expect(result.current.state.worktreeLayouts).toEqual({});
    } finally {
      unmount();
    }
  });
});
