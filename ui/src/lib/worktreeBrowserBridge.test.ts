import { describe, expect, it, vi } from "vitest";

import type { BrowserSessionCreatedPayload } from "./browserTauri";
import {
  routeBrowserModeShortcut,
  SHORTCUTS,
  type BrowserModeShortcutEvent,
  type ShortcutActivityMode,
  type ShortcutBinding,
} from "./shortcuts";
import type { LayoutState } from "./types";
import {
  adoptCliBrowserSession,
  closeBrowserSessionFromSplit,
  detachBrowserFromWorktreeSplit,
  handleWorktreeDeletedForBrowsers,
  openBrowserInWorktreeSplit,
  resolveBrowserTabSwitch,
} from "./worktreeBrowserBridge";
import { createBrowserStore, type BrowserStore } from "../state/browserStore";

function memoryStorage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    clear: () => entries.clear(),
    getItem: (key: string) => entries.get(key) ?? null,
    key: (index: number) => [...entries.keys()][index] ?? null,
    removeItem: (key: string) => void entries.delete(key),
    setItem: (key: string, value: string) => void entries.set(key, String(value)),
  };
}

function makeStore(): BrowserStore {
  return createBrowserStore({ storage: memoryStorage(), now: () => 1_700_000_000_000 });
}

const OPEN_INPUT = {
  browserId: "browser-split-1",
  url: "https://example.com/docs",
  title: "Docs",
  workspaceId: "ws-a",
  worktreePath: "/repo/wt-a",
  worktreeLabel: "wt-a",
  tabId: "tab-main",
  leafId: "leaf-right",
} as const;

function sampleSession(overrides: Partial<BrowserSessionCreatedPayload["browser"]> = {}): BrowserSessionCreatedPayload["browser"] {
  return {
    browserId: "browser-cli-1",
    webviewLabel: "browser-view-cli-1",
    workspaceId: "ws-a",
    worktreePath: "/repo/wt-a",
    profileId: "default",
    generation: 1,
    url: "https://cli.test/page",
    title: "CLI page",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomFactor: 1,
    loadError: null,
    visible: true,
    ...overrides,
  };
}

describe("openBrowserInWorktreeSplit", () => {
  it("registers the tab globally with a worktreeTag and records the split attachment", () => {
    const store = makeStore();

    const result = openBrowserInWorktreeSplit(OPEN_INPUT, store);

    expect(result.ok).toBe(true);
    const state = store.getState();
    expect(state.tabs["browser-split-1"]).toMatchObject({
      browserId: "browser-split-1",
      url: "https://example.com/docs",
      title: "Docs",
      worktreeTag: { workspaceId: "ws-a", worktreePath: "/repo/wt-a", label: "wt-a" },
    });
    expect(state.splitAttachments["browser-split-1"]).toEqual({
      workspaceId: "ws-a",
      tabId: "tab-main",
      leafId: "leaf-right",
    });
  });

  it("is idempotent: re-opening the same split keeps one tab and one attachment", () => {
    const store = makeStore();
    openBrowserInWorktreeSplit(OPEN_INPUT, store);

    openBrowserInWorktreeSplit({ ...OPEN_INPUT, leafId: "leaf-other" }, store);

    const state = store.getState();
    expect(Object.keys(state.tabs)).toEqual(["browser-split-1"]);
    expect(state.splitAttachments["browser-split-1"].leafId).toBe("leaf-other");
  });
});

describe("detachBrowserFromWorktreeSplit", () => {
  it("keeps the live tab in the store and never asks the backend to close it", () => {
    const store = makeStore();
    openBrowserInWorktreeSplit(OPEN_INPUT, store);
    const closeBackend = vi.fn();

    expect(detachBrowserFromWorktreeSplit("browser-split-1", store)).toBe(true);

    const state = store.getState();
    expect(state.splitAttachments["browser-split-1"]).toBeUndefined();
    expect(state.tabs["browser-split-1"]).toBeDefined();
    expect(state.archive).toHaveLength(0);
    expect(closeBackend).not.toHaveBeenCalled();
  });

  it("reports false when nothing was attached", () => {
    const store = makeStore();
    expect(detachBrowserFromWorktreeSplit("browser-absent", store)).toBe(false);
  });
});

describe("closeBrowserSessionFromSplit", () => {
  it("detaches, archives the tab, and closes the backend session exactly once", () => {
    const store = makeStore();
    openBrowserInWorktreeSplit(OPEN_INPUT, store);
    const closeBackend = vi.fn(async () => undefined);

    const result = closeBrowserSessionFromSplit("browser-split-1", closeBackend, store);

    expect(result).toEqual({ detached: true, closed: true });
    const state = store.getState();
    expect(state.splitAttachments["browser-split-1"]).toBeUndefined();
    expect(state.tabs["browser-split-1"]).toBeUndefined();
    expect(state.archive.map((entry) => entry.tab.browserId)).toEqual(["browser-split-1"]);
    expect(closeBackend).toHaveBeenCalledOnce();
    expect(closeBackend).toHaveBeenCalledWith("browser-split-1");
  });

  it("does not call the backend when the tab was already gone", () => {
    const store = makeStore();
    const closeBackend = vi.fn();

    const result = closeBrowserSessionFromSplit("browser-absent", closeBackend, store);

    expect(result).toEqual({ detached: false, closed: false });
    expect(closeBackend).not.toHaveBeenCalled();
  });
});

describe("handleWorktreeDeletedForBrowsers", () => {
  it("detaches only that workspace's splits and keeps every tab in the store", () => {
    const store = makeStore();
    openBrowserInWorktreeSplit(OPEN_INPUT, store);
    openBrowserInWorktreeSplit(
      { ...OPEN_INPUT, browserId: "browser-split-b", workspaceId: "ws-b", worktreePath: "/repo/wt-b", leafId: "leaf-b" },
      store,
    );

    const detached = handleWorktreeDeletedForBrowsers("ws-a", store);

    const state = store.getState();
    expect(detached).toBe(1);
    expect(state.splitAttachments["browser-split-1"]).toBeUndefined();
    expect(state.splitAttachments["browser-split-b"]).toEqual({ workspaceId: "ws-b", tabId: "tab-main", leafId: "leaf-b" });
    // The tab survives deletion: Browser mode still shows the page, no reload.
    expect(Object.keys(state.tabs).sort()).toEqual(["browser-split-1", "browser-split-b"]);
  });
});

describe("adoptCliBrowserSession", () => {
  it("adopts in every Activity mode, because adoption never depends on the layout", () => {
    const modes: ShortcutActivityMode[] = ["supervisor", "worktrees", "browser", "remoteDesktop"];
    for (const mode of modes) {
      const store = makeStore();
      const outcome = adoptCliBrowserSession(sampleSession(), "ws-a", null, store);
      expect(outcome, mode).toMatchObject({ adopted: true, browserId: "browser-cli-1" });
      expect(store.getState().tabs["browser-cli-1"]).toBeDefined();
    }
  });

  it("refuses with remote-host-active and writes nothing when a remote host is active", () => {
    const store = makeStore();

    const outcome = adoptCliBrowserSession(sampleSession(), "ws-a", "host-1", store);

    expect(outcome).toEqual({ adopted: false, reason: "remote-host-active" });
    expect(store.getState().tabs["browser-cli-1"]).toBeUndefined();
  });

  it("tags the worktree and marks an agent-spawned session agentControlled", () => {
    const store = makeStore();

    const outcome = adoptCliBrowserSession(
      { ...sampleSession(), agentControlled: true } as BrowserSessionCreatedPayload["browser"],
      "ws-a",
      null,
      store,
    );

    expect(outcome.adopted).toBe(true);
    expect(store.getState().tabs["browser-cli-1"]).toMatchObject({
      worktreeTag: { workspaceId: "ws-a", worktreePath: "/repo/wt-a" },
      agentControlled: true,
    });
  });

  it("adopts without a worktreeTag when the session names no worktree", () => {
    const store = makeStore();

    const outcome = adoptCliBrowserSession(
      sampleSession({ workspaceId: null, worktreePath: null }),
      null,
      null,
      store,
    );

    expect(outcome.adopted).toBe(true);
    expect(store.getState().tabs["browser-cli-1"].worktreeTag).toBeUndefined();
  });
});

describe("resolveBrowserTabSwitch", () => {
  function layoutWithPane(browserId: string): LayoutState {
    return {
      tabs: [{ id: "tab-main", label: "main", sessionId: "sess-1" }],
      activeTabId: "tab-main",
      layoutsByTabId: {
        "tab-main": {
          root: { type: "leaf", leafId: "leaf-left" },
          activeLeafId: "leaf-left",
          expandedLeafId: null,
          sessionIdsByLeafId: {},
          contentsByLeafId: {
            "leaf-left": { kind: "browser", browserId },
          },
        },
      },
    } as unknown as LayoutState;
  }

  it("finds a mounted worktree split first", () => {
    const store = makeStore();
    openBrowserInWorktreeSplit(OPEN_INPUT, store);

    const target = resolveBrowserTabSwitch("browser-split-1", store, layoutWithPane("browser-split-1"));

    expect(target).toEqual({
      target: "split",
      tabId: "tab-main",
      leafId: "leaf-left",
      workspaceId: "ws-a",
    });
  });

  it("finds a standalone browser layout tab", () => {
    const store = makeStore();
    const layout = {
      tabs: [{ id: "tab-browser", kind: "browser", label: "Docs", browserId: "browser-standalone" }],
      activeTabId: "tab-browser",
    } as unknown as LayoutState;

    expect(resolveBrowserTabSwitch("browser-standalone", store, layout)).toEqual({
      target: "split",
      tabId: "tab-browser",
      leafId: "",
      workspaceId: "",
    });
  });

  it("falls back to a global browser tab and reports Browser mode", () => {
    const store = makeStore();
    openBrowserInWorktreeSplit(OPEN_INPUT, store);
    store.detachSplit("browser-split-1");

    expect(resolveBrowserTabSwitch("browser-split-1", store, { tabs: [], activeTabId: null } as unknown as LayoutState))
      .toEqual({ target: "browser-mode", spaceId: store.getState().tabs["browser-split-1"].spaceId, browserId: "browser-split-1" });
  });

  it("returns not-found for an unknown id", () => {
    const store = makeStore();
    expect(resolveBrowserTabSwitch("browser-unknown", store, { tabs: [], activeTabId: null } as unknown as LayoutState))
      .toEqual({ target: "not-found" });
    expect(resolveBrowserTabSwitch("", store, null)).toEqual({ target: "not-found" });
  });
});

function canonicalChord(binding: ShortcutBinding): string {
  const parts: string[] = [];
  if (binding.mod) parts.push("Mod");
  if (binding.control) parts.push("Control");
  if (binding.alt) parts.push("Alt");
  if (binding.shift) parts.push("Shift");
  parts.push(binding.key);
  return parts.join("+");
}

/** The effective chord for one platform, with `nonMac` applied the way `resolveBinding` does. */
function platformChord(binding: ShortcutBinding, isMac: boolean): string {
  const { nonMac, ...base } = binding;
  return canonicalChord(isMac ? base : { ...base, ...nonMac });
}

describe("activity mode shortcut table", () => {
  const MODE_IDS = ["activity.mode1", "activity.mode2", "activity.mode3", "activity.mode4"] as const;

  it("declares the four activity mode actions in order", () => {
    const ids = SHORTCUTS.filter((shortcut) => shortcut.id.startsWith("activity.")).map((shortcut) => shortcut.id);
    expect(ids).toEqual([...MODE_IDS]);
  });

  it("defaults to Ctrl+Cmd+1..4 on macOS and Ctrl+Alt+Shift+1..4 elsewhere", () => {
    for (const [index, id] of MODE_IDS.entries()) {
      const shortcut = SHORTCUTS.find((candidate) => candidate.id === id);
      expect(shortcut).toBeDefined();
      expect(platformChord(shortcut!.binding, true)).toBe(`Mod+Control+${index + 1}`);
      expect(platformChord(shortcut!.binding, false)).toBe(`Control+Alt+Shift+${index + 1}`);
    }
  });

  it("has zero duplicate chords within either platform's table", () => {
    for (const isMac of [true, false]) {
      const seen = new Map<string, string[]>();
      for (const shortcut of SHORTCUTS) {
        const chord = platformChord(shortcut.binding, isMac);
        seen.set(chord, [...(seen.get(chord) ?? []), shortcut.id]);
      }
      const collisions = [...seen.entries()].filter(([, ids]) => ids.length > 1);
      // The only allowed collisions are the documented browser/terminal surface split.
      for (const [chord, ids] of collisions) {
        expect(ids.some((id) => id.startsWith("browser.")), `chord ${chord}`).toBe(true);
        expect(
          ids.some((id) => id.startsWith("terminal.") || id.startsWith("zoom.")),
          `chord ${chord}`,
        ).toBe(true);
      }
    }
  });

  it("is disjoint from the OS-reserved macOS Cmd+Shift+3/4/5 screenshot row", () => {
    for (const digit of ["3", "4", "5"]) {
      const reserved = canonicalChord({ key: digit, mod: true, shift: true });
      for (const shortcut of SHORTCUTS) {
        expect(platformChord(shortcut.binding, true), shortcut.id).not.toBe(reserved);
      }
    }
  });

  it("is disjoint from workspace.select*, tab.select* and the terminal-first preset chords", () => {
    for (const digit of ["1", "2", "3", "4"]) {
      const activity = SHORTCUTS.find((shortcut) => shortcut.id === `activity.mode${digit}`)!;
      const mac = platformChord(activity.binding, true);
      const nonMac = platformChord(activity.binding, false);
      for (const other of SHORTCUTS) {
        if (other.id.startsWith("activity.")) continue;
        expect(platformChord(other.binding, true), `${other.id} vs macOS ${digit}`).not.toBe(mac);
        expect(platformChord(other.binding, false), `${other.id} vs non-Mac ${digit}`).not.toBe(nonMac);
      }
    }
  });
});

describe("routeBrowserModeShortcut", () => {
  /** A keydown as the router sees it: every modifier spelled out, so a literal cannot drift. */
  const chord = (patch: Partial<BrowserModeShortcutEvent>): BrowserModeShortcutEvent => ({
    key: "",
    code: "",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...patch,
  });

  const modEvent = (key: string, overrides: Partial<BrowserModeShortcutEvent> = {}, isMac = true): BrowserModeShortcutEvent => chord({
    key,
    code: `Key${key.toUpperCase()}`,
    metaKey: isMac,
    ctrlKey: !isMac,
    ...overrides,
  });

  it("routes Cmd/Ctrl+T, W and L to Maho in Browser mode", () => {
    for (const isMac of [true, false]) {
      const mode: ShortcutActivityMode = "browser";
      expect(routeBrowserModeShortcut(modEvent("t", {}, isMac), mode, isMac)).toBe("new-tab");
      expect(routeBrowserModeShortcut(modEvent("w", {}, isMac), mode, isMac)).toBe("close-tab");
      expect(routeBrowserModeShortcut(modEvent("l", {}, isMac), mode, isMac)).toBe("command-bar");
    }
  });

  it("routes Ctrl+Tab and Ctrl+Shift+Tab to the Maho switcher in Browser mode", () => {
    expect(routeBrowserModeShortcut(chord({ key: "Tab", code: "Tab", ctrlKey: true }), "browser", true)).toBe("switcher-next");
    expect(routeBrowserModeShortcut(chord({ key: "Tab", code: "Tab", ctrlKey: true, shiftKey: true }), "browser", false))
      .toBe("switcher-prev");
  });

  it("never claims Cmd+Tab on macOS, which belongs to the OS window switcher", () => {
    expect(routeBrowserModeShortcut(chord({ key: "Tab", code: "Tab", metaKey: true }), "browser", true)).toBeNull();
  });

  it("returns null in Worktrees mode so Ctrl+Tab stays tab-next and Cmd+Shift+I stays Supervisor", () => {
    expect(routeBrowserModeShortcut(chord({ key: "Tab", code: "Tab", ctrlKey: true }), "worktrees", true)).toBeNull();
    expect(routeBrowserModeShortcut(modEvent("t"), "worktrees", true)).toBeNull();
    expect(routeBrowserModeShortcut(modEvent("w"), "worktrees", true)).toBeNull();
    expect(routeBrowserModeShortcut(modEvent("l"), "worktrees", true)).toBeNull();
    // Mod+Shift+I is notifications.toggle (Supervisor) and has no browser-mode meaning.
    expect(routeBrowserModeShortcut(chord({ key: "i", code: "KeyI", metaKey: true, shiftKey: true }), "worktrees", true))
      .toBeNull();
    expect(routeBrowserModeShortcut(chord({ key: "i", code: "KeyI", metaKey: true, shiftKey: true }), "browser", true))
      .toBeNull();
  });

  it("returns null in the other two modes", () => {
    for (const mode of ["supervisor", "remoteDesktop"] as const) {
      expect(routeBrowserModeShortcut(modEvent("t"), mode, true)).toBeNull();
      expect(routeBrowserModeShortcut(chord({ key: "Tab", code: "Tab", ctrlKey: true }), mode, true)).toBeNull();
    }
  });

  it("leaves Alt-carrying and shifted letter chords alone", () => {
    expect(routeBrowserModeShortcut(modEvent("t", { altKey: true }), "browser", true)).toBeNull();
    expect(routeBrowserModeShortcut(modEvent("t", { shiftKey: true }), "browser", true)).toBeNull();
    expect(routeBrowserModeShortcut(chord({ key: "r", code: "KeyR" }), "browser", true)).toBeNull();
  });
});
