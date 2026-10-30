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
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listFilePreviewIds, releaseFilePreview, retainFilePreview } from "../lib/filePreviewTabRegistry";
import type { Worktree } from "../lib/types";
import { clearWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

vi.mock("../lib/filePreviewTabRegistry", () => ({
  retainFilePreview: vi.fn(),
  releaseFilePreview: vi.fn(async () => undefined),
  getFilePreview: vi.fn(() => null),
  listFilePreviewIds: vi.fn(() => []),
}));

const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-unused"),
  getTerminalCwd: vi.fn(async () => null),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const source = {
  leafId: "leaf-1",
  sessionId: "front-1",
  backendSessionId: "back-1",
  workspaceId: "ws-1",
};

describe("file preview tabs", () => {
  const liveIds = new Set<string>();
  beforeEach(() => {
    liveIds.clear();
    clearWorkspaceSnapshot();
    vi.mocked(releaseFilePreview).mockClear();
    vi.mocked(retainFilePreview).mockImplementation((id) => {
      liveIds.add(id);
      return {} as never;
    });
    vi.mocked(listFilePreviewIds).mockImplementation(() => [...liveIds]);
    vi.mocked(releaseFilePreview).mockImplementation(async (id) => {
      liveIds.delete(id);
    });
  });

  it("opens one file tab per path and releases the controller only on close", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));
    const request = { path: "/repo/readme.md", backendSessionId: "back-1", line: 3, col: 1 };

    let firstId = "";
    act(() => {
      firstId = result.current.openFilePreviewTab(source, request);
    });
    let secondId = "";
    act(() => {
      secondId = result.current.openFilePreviewTab(source, { ...request, line: 9 });
    });
    expect(secondId).toBe(firstId);
    expect(result.current.state.layout.tabs.filter((tab) => tab.kind === "file")).toHaveLength(1);
    const tab = result.current.state.layout.tabs.find((item) => item.id === firstId);
    expect(tab?.kind).toBe("file");
    if (tab?.kind === "file") expect(tab.line).toBe(9);
    expect(retainFilePreview).toHaveBeenCalledTimes(2);
    expect(releaseFilePreview).not.toHaveBeenCalled();

    act(() => {
      result.current.activateTab(firstId);
    });
    expect(releaseFilePreview).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.closeTab(firstId);
    });
    expect(releaseFilePreview).toHaveBeenCalledWith(firstId);
    expect(result.current.state.layout.tabs.some((item) => item.id === firstId)).toBe(false);
  });

  it("reuses the tab for an absolute path whichever session or the CLI asked", () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));
    const request = { path: "/repo/readme.md", backendSessionId: "back-1", line: null, col: null };
    let firstId = "";
    act(() => {
      firstId = result.current.openFilePreviewTab(source, request);
    });
    let cliId = "";
    act(() => {
      cliId = result.current.openFilePreviewTab(source, { ...request, backendSessionId: "" });
    });
    expect(cliId).toBe(firstId);
    expect(result.current.state.layout.tabs.filter((tab) => tab.kind === "file")).toHaveLength(1);
  });


  it("keeps previews parked in another workspace when a tab closes here", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));
    let mountedId = "";
    act(() => {
      mountedId = result.current.openFilePreviewTab(source, {
        path: "/repo/a.md",
        backendSessionId: "back-1",
        line: 1,
        col: 1,
      });
    });
    const mounted = result.current.state;
    setWorkspaceSnapshot("ws-other", {
      ...mounted,
      workspaceId: "ws-other",
      layout: {
        ...mounted.layout,
        tabs: mounted.layout.tabs.map((tab) => (tab.kind === "file" ? { ...tab, previewId: "other-preview" } : tab)),
        // Pane contents would otherwise still name the mounted tab's preview.
        layoutsByTabId: {},
      },
    });
    liveIds.add("other-preview");

    await act(async () => {
      await result.current.closeTab(mountedId);
    });
    expect(releaseFilePreview).toHaveBeenCalledWith(mountedId);
    expect(releaseFilePreview).not.toHaveBeenCalledWith("other-preview");
  });

  it("releases file previews of a removed worktree", async () => {
    const feature: Worktree = { ...worktree, path: "/repo/feature", branch: "refs/heads/feature" };
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree, feature], services }));
    let tabId = "";
    act(() => {
      tabId = result.current.openFilePreviewTab(source, {
        path: "/repo/shared.ts",
        backendSessionId: "back-1",
        line: 1,
        col: 1,
      });
    });
    act(() => {
      result.current.dispatchWorkspaceAction({ type: "SELECT_WORKTREE", path: feature.path });
    });
    expect(result.current.state.worktreeLayouts?.[worktree.path]?.tabs.some((t) => t.id === tabId)).toBe(true);

    await act(async () => {
      await result.current.syncWorktrees([feature], { confirmedRemoval: true });
    });
    expect(releaseFilePreview).toHaveBeenCalledWith(tabId);
  });

  it("disambiguates duplicate basenames across file tabs", () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    let firstId = "";
    act(() => {
      firstId = result.current.openFilePreviewTab(source, {
        path: "/a/src/index.ts",
        backendSessionId: "back-1",
        line: 1,
        col: 1,
      });
    });

    let tab1 = result.current.state.layout.tabs.find((item) => item.id === firstId);
    expect(tab1?.label).toBe("index.ts");

    let secondId = "";
    act(() => {
      secondId = result.current.openFilePreviewTab(source, {
        path: "/b/ui/index.ts",
        backendSessionId: "back-1",
        line: 1,
        col: 1,
      });
    });

    tab1 = result.current.state.layout.tabs.find((item) => item.id === firstId);
    const tab2 = result.current.state.layout.tabs.find((item) => item.id === secondId);
    expect(tab1?.label).toBe("src/index.ts");
    expect(tab2?.label).toBe("ui/index.ts");
  });

  it("reuses an existing file tab when it lives in a parked worktree layout", () => {
    const otherWorktree: Worktree = {
      path: "/repo/feature",
      head: "def",
      branch: "refs/heads/feature",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, otherWorktree], services }),
    );

    let tabId = "";
    act(() => {
      tabId = result.current.openFilePreviewTab(source, {
        path: "/repo/shared.ts",
        backendSessionId: "back-1",
        line: 1,
        col: 1,
      });
    });

    act(() => {
      result.current.dispatchWorkspaceAction({ type: "SELECT_WORKTREE", path: otherWorktree.path });
    });

    expect(result.current.state.activeWorktreePath).toBe(otherWorktree.path);
    expect(result.current.state.layout.tabs.some((t) => t.id === tabId)).toBe(false);
    expect(result.current.state.worktreeLayouts?.[worktree.path]?.tabs.some((t) => t.id === tabId)).toBe(true);

    let reopenedId = "";
    act(() => {
      reopenedId = result.current.openFilePreviewTab(source, {
        path: "/repo/shared.ts",
        backendSessionId: "back-1",
        line: 25,
        col: 4,
      });
    });

    expect(reopenedId).toBe(tabId);
    expect(result.current.state.activeWorktreePath).toBe(worktree.path);
    const reopenedTab = result.current.state.layout.tabs.find((t) => t.id === tabId);
    expect(reopenedTab?.kind).toBe("file");
    if (reopenedTab?.kind === "file") {
      expect(reopenedTab.line).toBe(25);
      expect(reopenedTab.col).toBe(4);
    }
  });

  it("keeps same-path previews separate by backend in the active layout", () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));
    const request = { path: "README.md", backendSessionId: "back-1", line: 1, col: 1 };

    let firstId = "";
    act(() => {
      firstId = result.current.openFilePreviewTab(source, request);
    });

    let secondId = "";
    act(() => {
      secondId = result.current.openFilePreviewTab(
        { ...source, sessionId: "front-2", backendSessionId: "back-2" },
        { ...request, backendSessionId: "back-2" },
      );
    });

    expect(secondId).not.toBe(firstId);
    expect(result.current.state.layout.tabs.filter((tab) => tab.kind === "file")).toHaveLength(2);
    const firstTab = result.current.state.layout.tabs.find((tab) => tab.id === firstId);
    const secondTab = result.current.state.layout.tabs.find((tab) => tab.id === secondId);
    expect(firstTab?.kind).toBe("file");
    expect(secondTab?.kind).toBe("file");
    if (firstTab?.kind === "file" && secondTab?.kind === "file") {
      expect(firstTab.backendSessionId).toBe("back-1");
      expect(secondTab.backendSessionId).toBe("back-2");
    }

    let reopenedId = "";
    act(() => {
      reopenedId = result.current.openFilePreviewTab(source, { ...request, line: 9 });
    });
    expect(reopenedId).toBe(firstId);
    expect(result.current.state.layout.tabs.filter((tab) => tab.kind === "file")).toHaveLength(2);
  });

  it("isolates relative file preview tabs when switching between worktrees", () => {
    const mainWorktree: Worktree = {
      path: "/repo/main",
      head: "abc",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const featureWorktree: Worktree = {
      path: "/repo/feature",
      head: "def",
      branch: "refs/heads/feature",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    const sourceMain = {
      leafId: "leaf-1",
      sessionId: "front-1",
      backendSessionId: "back-1",
      workspaceId: "ws-1",
    };
    const sourceFeature = {
      leafId: "leaf-2",
      sessionId: "front-2",
      backendSessionId: "back-2",
      workspaceId: "ws-1",
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [mainWorktree, featureWorktree], services }),
    );

    let mainTabId = "";
    act(() => {
      mainTabId = result.current.openFilePreviewTab(sourceMain, {
        path: "README.md",
        backendSessionId: "back-1",
        line: 1,
        col: 1,
      });
    });

    act(() => {
      result.current.dispatchWorkspaceAction({ type: "SELECT_WORKTREE", path: featureWorktree.path });
    });

    let featureTabId = "";
    act(() => {
      featureTabId = result.current.openFilePreviewTab(sourceFeature, {
        path: "README.md",
        backendSessionId: "back-2",
        line: 1,
        col: 1,
      });
    });

    expect(featureTabId).not.toBe(mainTabId);
    expect(result.current.state.activeWorktreePath).toBe(featureWorktree.path);

    const parkedMainLayout = result.current.state.worktreeLayouts?.[mainWorktree.path];
    expect(parkedMainLayout?.tabs.some((t) => t.id === mainTabId)).toBe(true);
    const parkedMainTab = parkedMainLayout?.tabs.find((t) => t.id === mainTabId);
    expect(parkedMainTab?.kind).toBe("file");
    if (parkedMainTab?.kind === "file") {
      expect(parkedMainTab.backendSessionId).toBe("back-1");
    }

    expect(result.current.state.layout.tabs.some((t) => t.id === featureTabId)).toBe(true);
    const activeFeatureTab = result.current.state.layout.tabs.find((t) => t.id === featureTabId);
    expect(activeFeatureTab?.kind).toBe("file");
    if (activeFeatureTab?.kind === "file") {
      expect(activeFeatureTab.backendSessionId).toBe("back-2");
    }
  });

  it("parks a delayed file preview under its source session worktree", () => {
    const featureWorktree: Worktree = {
      path: "/repo/feature",
      head: "def",
      branch: "refs/heads/feature",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    act(() => {
      result.current.dispatchWorkspaceAction({
        type: "ADD_TAB_WITH_SESSION",
        tab: { id: "terminal-main", label: "main", sessionId: "front-1" },
        session: {
          id: "front-1",
          cwd: worktree.path,
          worktreePath: worktree.path,
          workspaceId: "ws-1",
          worktree: null,
          backendSessionId: "back-1",
          lifecycle: "working",
        },
      });
      result.current.dispatchWorkspaceAction({ type: "SELECT_WORKTREE", path: featureWorktree.path });
    });
    expect(result.current.state.activeWorktreePath).toBe(featureWorktree.path);

    let delayedTabId = "";
    act(() => {
      delayedTabId = result.current.openFilePreviewTab(source, {
        path: "delayed-only.ts",
        backendSessionId: "back-1",
        line: 1,
        col: 1,
      });
    });

    expect(result.current.state.worktreeLayouts?.[worktree.path]?.tabs.some((tab) => tab.id === delayedTabId)).toBe(true);
    expect(result.current.state.activeWorktreePath).toBe(featureWorktree.path);
    expect(result.current.state.layout.tabs.some((tab) => tab.id === delayedTabId)).toBe(false);
  });
});

