import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
}

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, Worktree } from "../lib/types";
import * as browserTauri from "../lib/browserTauri";
import { clearHmrWorkspaceState } from "./hmrWorkspaceState";
import { clearWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-unused"),
  getTerminalCwd: vi.fn(async () => null),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

const worktreeActive: Worktree = {
  path: "/repo/active",
  head: "abc1",
  branch: "refs/heads/orca/ws-active/active",
  workspaceId: "ws-active",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const worktreeWs: Worktree = {
  path: "/repo/ws",
  head: "abc2",
  branch: "refs/heads/orca/ws-target/main",
  workspaceId: "ws-target",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const worktreePathMatch: Worktree = {
  path: "/repo/path-match",
  head: "abc3",
  branch: "refs/heads/orca/ws-other/other",
  workspaceId: "ws-other",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function createBaseBrowser(overrides?: Partial<browserTauri.BrowserSessionCreatedPayload["browser"]>) {
  return {
    browserId: "browser-adopt-1",
    webviewLabel: "webview-adopt-1",
    workspaceId: "ws-active",
    worktreePath: "/repo/active",
    profileId: "default",
    generation: 1,
    url: "http://localhost:3000/adopted",
    title: "Adopted Tab",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomFactor: 1,
    loadError: null,
    visible: true,
    ...overrides,
  };
}

describe("workspaceStore browser adoption", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    clearHmrWorkspaceState();
    clearWorkspaceSnapshot();
  });

  it("adopt inserts tab with adopted browserId and createBrowser called ZERO times", async () => {
    expect(browserTauri.BROWSER_SESSION_CREATED_EVENT).toBe("browser_session_created");
    expect(typeof browserTauri.onBrowserSessionCreated).toBe("function");

    const createBrowserSpy = vi.spyOn(browserTauri, "createBrowser").mockImplementation(async () => {
      throw new Error("createBrowser must not be called during adoption");
    });

    const { result } = renderHook(() =>
      useWorkspaceStore({
        workspaceId: "ws-active",
        initialWorktrees: [worktreeActive, worktreeWs, worktreePathMatch],
        services,
      }),
    );

    expect(typeof result.current.adoptBrowserSession).toBe("function");

    const session = createBaseBrowser();
    let adoptedTabId: string | null = null;
    await act(async () => {
      adoptedTabId = result.current.adoptBrowserSession(session);
    });

    expect(adoptedTabId).toBeTruthy();
    expect(createBrowserSpy).toHaveBeenCalledTimes(0);

    const activeTabs = result.current.state.layout.tabs as BrowserTab[];
    const adoptedTab = activeTabs.find((t) => t.id === adoptedTabId);
    expect(adoptedTab).toBeDefined();
    expect(adoptedTab).toMatchObject({
      kind: "browser",
      id: adoptedTabId,
      label: "Adopted Tab",
      browserId: "browser-adopt-1",
      url: "http://localhost:3000/adopted",
      title: "Adopted Tab",
      loading: false,
      canGoBack: false,
      canGoForward: false,
      zoomFactor: 1,
      loadError: null,
      profileId: "default",
      worktreePath: "/repo/active",
    });
  });

  it("workspaceId-match priority", async () => {
    const createBrowserSpy = vi.spyOn(browserTauri, "createBrowser").mockImplementation(async () => {
      throw new Error("createBrowser must not be called");
    });

    const { result } = renderHook(() =>
      useWorkspaceStore({
        workspaceId: "ws-active",
        initialWorktrees: [worktreeActive, worktreeWs, worktreePathMatch],
        services,
      }),
    );

    // workspaceId match wins even when worktreePath points to a different known worktree
    await act(async () => {
      result.current.adoptBrowserSession(
        createBaseBrowser({
          browserId: "browser-prio-ws",
          workspaceId: "ws-target",
          worktreePath: "/repo/path-match",
        }),
      );
    });

    const wsLayout = result.current.state.worktreeLayouts?.[worktreeWs.path];
    expect(
      wsLayout?.tabs.some((t) => t.kind === "browser" && (t as BrowserTab).browserId === "browser-prio-ws"),
    ).toBe(true);

    // explicit targetWorkspaceId argument also routes by workspaceId
    await act(async () => {
      result.current.adoptBrowserSession(
        createBaseBrowser({
          browserId: "browser-prio-ws-arg",
          workspaceId: "ws-other",
          worktreePath: "/repo/path-match",
        }),
        "ws-target",
      );
    });

    expect(
      result.current.state.worktreeLayouts?.[worktreeWs.path]?.tabs.some(
        (t) => t.kind === "browser" && (t as BrowserTab).browserId === "browser-prio-ws-arg",
      ),
    ).toBe(true);
    expect(createBrowserSpy).toHaveBeenCalledTimes(0);
  });

  it("worktreePath-match priority", async () => {
    const createBrowserSpy = vi.spyOn(browserTauri, "createBrowser").mockImplementation(async () => {
      throw new Error("createBrowser must not be called");
    });

    const { result } = renderHook(() =>
      useWorkspaceStore({
        workspaceId: "ws-active",
        initialWorktrees: [worktreeActive, worktreeWs, worktreePathMatch],
        services,
      }),
    );

    // When workspaceId does not match any worktree, worktreePath match wins over active fallback
    await act(async () => {
      result.current.adoptBrowserSession(
        createBaseBrowser({
          browserId: "browser-prio-path",
          workspaceId: "ws-unknown",
          worktreePath: "/repo/path-match",
        }),
      );
    });

    const pathLayout = result.current.state.worktreeLayouts?.[worktreePathMatch.path];
    expect(
      pathLayout?.tabs.some((t) => t.kind === "browser" && (t as BrowserTab).browserId === "browser-prio-path"),
    ).toBe(true);
    expect(createBrowserSpy).toHaveBeenCalledTimes(0);
  });

  it("active-fallback", async () => {
    const createBrowserSpy = vi.spyOn(browserTauri, "createBrowser").mockImplementation(async () => {
      throw new Error("createBrowser must not be called");
    });

    const { result } = renderHook(() =>
      useWorkspaceStore({
        workspaceId: "ws-active",
        initialWorktrees: [worktreeActive, worktreeWs, worktreePathMatch],
        services,
      }),
    );

    // When neither workspaceId nor worktreePath match, falls back to active worktree
    await act(async () => {
      result.current.adoptBrowserSession(
        createBaseBrowser({
          browserId: "browser-prio-active",
          workspaceId: "ws-unknown",
          worktreePath: "/repo/unknown",
        }),
      );
    });

    const activeTabs = result.current.state.layout.tabs as BrowserTab[];
    expect(activeTabs.some((t) => t.kind === "browser" && t.browserId === "browser-prio-active")).toBe(true);
    expect(createBrowserSpy).toHaveBeenCalledTimes(0);
  });

  it("idempotent re-adopt", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({
        workspaceId: "ws-active",
        initialWorktrees: [worktreeActive, worktreeWs, worktreePathMatch],
        services,
      }),
    );

    const session = createBaseBrowser({ browserId: "browser-idempotent" });

    let firstAdoptId: string | null = null;
    await act(async () => {
      firstAdoptId = result.current.adoptBrowserSession(session);
    });
    expect(firstAdoptId).toBeTruthy();

    const tabsBeforeReAdopt = result.current.state.layout.tabs.length;

    let secondAdoptId: string | null = "not-null";
    await act(async () => {
      secondAdoptId = result.current.adoptBrowserSession(session);
    });

    expect(secondAdoptId).toBeNull();
    expect(result.current.state.layout.tabs.length).toBe(tabsBeforeReAdopt);
    const matchingTabs = (result.current.state.layout.tabs as BrowserTab[]).filter(
      (t) => t.kind === "browser" && t.browserId === "browser-idempotent",
    );
    expect(matchingTabs.length).toBe(1);
  });
});
