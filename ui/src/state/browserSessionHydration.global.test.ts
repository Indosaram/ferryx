import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";

import { saveBrowserSettings } from "../lib/browserSettings";
import type { BrowserAgentOverview } from "../lib/browserAgent";
import type { BrowserState } from "../lib/types";
import {
  applyAgentOverviewToStore,
  applyBrowserStateChangedToStore,
  collectGlobalBrowserRestoreTargets,
  hydrateGlobalBrowserStore,
  resetGlobalBrowserHydration,
  useGlobalBrowserStoreSync,
} from "./browserSessionHydration";
import { createBrowserStore, type BrowserStore } from "./browserStore";

const tauriMocks = vi.hoisted(() => ({
  ensureBrowser: vi.fn(async (request: { browserId: string; url: string }): Promise<BrowserState> => ({
    browserId: request.browserId,
    webviewLabel: `browser-${request.browserId}`,
    workspaceId: null,
    worktreePath: null,
    profileId: "default",
    generation: 1,
    url: request.url,
    title: "",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomFactor: 1,
    loadError: null,
    visible: false,
    engine: "system",
  })),
  stateChangedListener: null as null | ((payload: { browserId: string; url: string; title?: string | null }) => void),
  onBrowserStateChanged: vi.fn(async (listener: (payload: unknown) => void) => {
    tauriMocks.stateChangedListener = listener as typeof tauriMocks.stateChangedListener;
    return () => {
      tauriMocks.stateChangedListener = null;
    };
  }),
}));

const agentMocks = vi.hoisted(() => ({
  overview: vi.fn(async (): Promise<BrowserAgentOverview> => ({ settings: { requirePairing: true }, pending: [], controllers: [] })),
  approvalsListener: null as null | ((pending: unknown[]) => void),
  onBrowserAgentApprovals: vi.fn(async (listener: (pending: unknown[]) => void) => {
    agentMocks.approvalsListener = listener;
    return () => {
      agentMocks.approvalsListener = null;
    };
  }),
}));

vi.mock("../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserTauri")>();
  return {
    ...actual,
    ensureBrowser: tauriMocks.ensureBrowser,
    onBrowserStateChanged: tauriMocks.onBrowserStateChanged,
  };
});

vi.mock("../lib/browserAgent", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserAgent")>();
  return {
    ...actual,
    getBrowserAgentOverview: agentMocks.overview,
    onBrowserAgentApprovals: agentMocks.onBrowserAgentApprovals,
  };
});

function browserState(browserId: string, url: string, title = ""): BrowserState {
  return {
    browserId,
    webviewLabel: `browser-${browserId}`,
    workspaceId: null,
    worktreePath: null,
    profileId: "default",
    generation: 1,
    url,
    title,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomFactor: 1,
    loadError: null,
    visible: false,
    engine: "system",
  };
}

function testStore(): BrowserStore {
  return createBrowserStore({ storage: null });
}

/** Three Spaces, three tabs each; the first tab of every Space is that Space's active tab. */
function nineTabsAcrossThreeSpaces(store: BrowserStore) {
  const spaces = ["space-default"];
  for (const name of ["Work", "Research"]) {
    const created = store.createSpace({ name });
    if (!created.ok) throw new Error("space create failed");
    spaces.push(created.value.id);
  }
  const active: string[] = [];
  const background: string[] = [];
  spaces.forEach((spaceId, spaceIndex) => {
    for (let index = 0; index < 3; index += 1) {
      const browserId = `b-${spaceIndex}-${index}`;
      store.upsertTab({ browserId, spaceId, url: `https://tab-${spaceIndex}-${index}.test/` });
      if (index === 0) {
        store.touch(browserId);
        active.push(browserId);
      } else {
        background.push(browserId);
      }
    }
  });
  return { spaces, active, background };
}

describe("global browser session hydration", () => {
  // Vitest globals are disabled, so RTL cannot register global auto-cleanup.
  afterEach(cleanup);

  beforeEach(() => {
    localStorage.clear();
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    tauriMocks.ensureBrowser.mockReset();
    tauriMocks.ensureBrowser.mockImplementation(async (request: { browserId: string; url: string }) =>
      browserState(request.browserId, request.url));
    tauriMocks.onBrowserStateChanged.mockClear();
    tauriMocks.stateChangedListener = null;
    agentMocks.overview.mockClear();
    agentMocks.onBrowserAgentApprovals.mockClear();
    agentMocks.approvalsListener = null;
    resetGlobalBrowserHydration();
  });

  it("collects one eager target per Space and hydrates only 3 of 9 tabs", async () => {
    const store = testStore();
    const { active, background } = nineTabsAcrossThreeSpaces(store);

    const targets = collectGlobalBrowserRestoreTargets(store.getState());
    expect(targets.map((target) => target.browserId).sort()).toEqual([...active].sort());

    const result = await hydrateGlobalBrowserStore(store);

    expect(tauriMocks.ensureBrowser).toHaveBeenCalledTimes(3);
    expect(result.hydrated.sort()).toEqual([...active].sort());
    for (const browserId of background) {
      expect(tauriMocks.ensureBrowser).not.toHaveBeenCalledWith(expect.objectContaining({ browserId }));
    }
  });

  it("creates one session for a browserId referenced by both the store and a worktree split", async () => {
    const store = testStore();
    store.upsertTab({ browserId: "shared", url: "https://shared.test/" });
    store.upsertTab({ browserId: "split-only", url: "https://split.test/" });
    store.touch("shared");
    store.attachSplit("shared", { workspaceId: "ws-1", tabId: "tab-1", leafId: "leaf-1" });
    store.attachSplit("split-only", { workspaceId: "ws-1", tabId: "tab-1", leafId: "leaf-2" });

    const targets = collectGlobalBrowserRestoreTargets(store.getState());
    expect(targets.map((target) => target.browserId).sort()).toEqual(["shared", "split-only"]);

    await hydrateGlobalBrowserStore(store);

    expect(tauriMocks.ensureBrowser).toHaveBeenCalledTimes(2);
    expect(tauriMocks.ensureBrowser.mock.calls.filter(([request]) => request.browserId === "shared")).toHaveLength(1);
  });

  it("never hydrates private tabs", async () => {
    const store = testStore();
    store.upsertTab({ browserId: "public", url: "https://public.test/" });
    store.upsertTab({ browserId: "secret", url: "https://secret.test/", private: true });
    store.touch("secret");

    const targets = collectGlobalBrowserRestoreTargets(store.getState());
    expect(targets.map((target) => target.browserId)).toEqual([]);

    await hydrateGlobalBrowserStore(store);
    expect(tauriMocks.ensureBrowser).not.toHaveBeenCalled();
  });

  it("does not spawn a session twice for re-entrant hydration of the same browserId", async () => {
    const store = testStore();
    store.upsertTab({ browserId: "b1", url: "https://one.test/" });
    store.touch("b1");

    const first = hydrateGlobalBrowserStore(store);
    const second = hydrateGlobalBrowserStore(store);
    await Promise.all([first, second]);

    expect(tauriMocks.ensureBrowser).toHaveBeenCalledTimes(1);
  });

  it("keeps other tabs hydrated when one backend session is missing", async () => {
    const store = testStore();
    store.upsertTab({ browserId: "alive", url: "https://alive.test/" });
    store.upsertTab({ browserId: "gone", url: "https://gone.test/" });
    store.touch("alive");
    store.attachSplit("gone", { workspaceId: "ws-1", tabId: "tab-1", leafId: "leaf-1" });

    tauriMocks.ensureBrowser.mockImplementation(async (request: { browserId: string; url: string }) => {
      if (request.browserId === "gone") throw { code: "BROWSER_NOT_FOUND", message: "no such browser" };
      return browserState(request.browserId, request.url);
    });

    const result = await hydrateGlobalBrowserStore(store);

    expect(result.failed).toEqual([{ browserId: "gone", code: "BROWSER_NOT_FOUND" }]);
    expect(result.hydrated).toEqual(["alive"]);
    expect(store.getState().tabs.gone.hydrationError).toBe("BROWSER_NOT_FOUND");
    expect(store.getState().tabs.alive.hydrationError).toBeUndefined();
  });

  it("updates a background tab from browser_state_changed through the sync hook", async () => {
    const store = testStore();
    const { background } = nineTabsAcrossThreeSpaces(store);

    const { unmount } = renderHook(() => useGlobalBrowserStoreSync(store));
    await act(async () => {});

    expect(tauriMocks.ensureBrowser).toHaveBeenCalledTimes(3);

    const backgroundId = background[0];
    act(() => {
      tauriMocks.stateChangedListener?.({
        browserId: backgroundId,
        url: "https://background.test/next",
        title: "Background page",
      });
    });

    const updated = store.getState().tabs[backgroundId];
    expect(updated.url).toBe("https://background.test/next");
    expect(updated.title).toBe("Background page");

    unmount();
    expect(tauriMocks.stateChangedListener).toBeNull();
  });

  it("ignores state changes for browser sessions the store does not know", () => {
    const store = testStore();
    store.upsertTab({ browserId: "known", url: "https://known.test/" });

    expect(applyBrowserStateChangedToStore(store, { browserId: "cli-only", url: "https://cli.test/" })).toBe(false);
    expect(store.getState().tabs["cli-only"]).toBeUndefined();
  });

  it("marks agent-controlled tabs from live controller grants", () => {
    const store = testStore();
    store.upsertTab({ browserId: "granted", url: "https://granted.test/page" });
    store.upsertTab({ browserId: "other", url: "https://other.test/page" });
    store.upsertTab({ browserId: "secret", url: "https://granted.test/private", private: true });

    const overview: BrowserAgentOverview = {
      settings: { requirePairing: true },
      pending: [],
      controllers: [
        {
          id: "controller-1",
          label: "QA agent",
          revoked: false,
          grants: [{ origin: "https://granted.test", scopes: ["read"], persistent: false }],
        },
        {
          id: "controller-2",
          label: "Revoked agent",
          revoked: true,
          grants: [{ origin: "https://other.test", scopes: ["read"], persistent: false }],
        },
      ],
    };

    expect(applyAgentOverviewToStore(store, overview)).toBe(1);
    const state = store.getState();
    expect(state.tabs.granted.agentControlled).toBe(true);
    expect(state.tabs.other.agentControlled).toBe(false);
    expect(state.tabs.secret.agentControlled).toBe(false);

    // A second application with nothing new writes nothing.
    expect(applyAgentOverviewToStore(store, overview)).toBe(0);
  });
});
