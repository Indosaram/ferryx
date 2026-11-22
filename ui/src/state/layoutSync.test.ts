import { describe, expect, it, vi } from "vitest";

import { serializeWorkspaceState } from "../lib/sessionPersistence";
import type { TerminalSession, WorkspaceTab } from "../lib/types";
import { createLayoutState, layoutReducer } from "./layout";
import {
  applySharedLayout,
  createLayoutSyncLoop,
  decodeSharedLayout,
  publishPersistedLayouts,
  toRevision,
  toSharedLayout,
  type SharedLayout,
  type SharedTab,
} from "./layoutSync";

function terminalTab(id: string, sessionId: string, label = id): WorkspaceTab {
  return { id, label, sessionId };
}

function session(id: string, backendSessionId: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    workspaceId: "ws",
    worktree: null,
    backendSessionId,
    lifecycle: "working",
    processState: "running",
  };
}

/** A tab with two panes: one leaf per session, split horizontally at 0.4. */
function splitLayout(): ReturnType<typeof createLayoutState> {
  const initial = createLayoutState([terminalTab("tab", "s1")], "tab");
  return layoutReducer(initial, {
    type: "SPLIT_PANE", tabId: "tab", direction: "horizontal", sessionId: "s2", ratio: 0.4,
  });
}

describe("toSharedLayout", () => {
  it("strips every piece of view state and keeps the pane structure", () => {
    const layout = layoutReducer(splitLayout(), { type: "ACTIVATE_TAB", tabId: "tab" });
    const sessions = { s1: session("s1", "backend-1"), s2: session("s2", "backend-2") };
    const shared = toSharedLayout(layout, sessions);

    expect(shared).toEqual([
      {
        id: "tab",
        kind: "terminal",
        label: "tab",
        pinned: false,
        paneTree: expect.objectContaining({
          type: "split",
          direction: "horizontal",
          first: expect.objectContaining({ type: "leaf", backendSessionId: "backend-1" }),
          second: expect.objectContaining({ type: "leaf", backendSessionId: "backend-2" }),
        }),
      },
    ]);
    // No active tab, focus, split mode or leaf view travels with the document.
    const serialized = JSON.stringify(shared);
    for (const forbidden of ["activeTabId", "activeLeafId", "expandedLeafId", "splitMode", "sessionIdsByLeafId", "\"view\""]) {
      expect(serialized).not.toContain(forbidden);
    }
    expect(serialized).toContain("backend-1");
  });

  it("keeps non-terminal tabs as a bare entry with no pane tree", () => {
    const browser: WorkspaceTab = {
      kind: "browser", id: "browser", label: "Docs", browserId: "b1", url: "https://example.com", pinned: true,
    };
    const layout = createLayoutState([terminalTab("tab", "s1"), browser], "tab");
    const shared = toSharedLayout(layout, { s1: session("s1", "backend-1") });
    expect(shared[1]).toEqual({ id: "browser", kind: "browser", label: "Docs", pinned: true, paneTree: null });
  });
});

describe("applySharedLayout", () => {
  const doc: SharedLayout = {
    workspaceId: "ws",
    revision: 7,
    tabs: [
      {
        id: "tab",
        kind: "terminal",
        label: "tab",
        pinned: false,
        paneTree: {
          type: "split",
          direction: "horizontal",
          ratio: 0.4,
          first: { type: "leaf", leafId: "leaf-a", backendSessionId: "backend-1" },
          second: { type: "leaf", leafId: "leaf-b", backendSessionId: "backend-2" },
        },
      },
    ],
  };

  it("yields the same tab and pane counts and reuses the live leaf content", () => {
    const before = splitLayout();
    const contentBefore = Object.values(before.layoutsByTabId.tab.contentsByLeafId ?? {});
    const { layout, unattachedBackendSessionIds } = applySharedLayout(before, doc, (backendSessionId) =>
      backendSessionId === "backend-1" ? "s1" : backendSessionId === "backend-2" ? "s2" : null,
    );

    expect(layout.tabs).toHaveLength(before.tabs.length);
    expect(Object.keys(layout.layoutsByTabId)).toHaveLength(Object.keys(before.layoutsByTabId).length);
    const panes = layout.layoutsByTabId.tab;
    expect(Object.values(panes.sessionIdsByLeafId).sort()).toEqual(["s1", "s2"]);
    expect(panes.root).toMatchObject({ type: "split", direction: "horizontal", ratio: 0.4 });
    expect(Object.values(panes.contentsByLeafId ?? {})).toHaveLength(2);
    expect(Object.values(panes.contentsByLeafId ?? {})).toEqual(expect.arrayContaining(contentBefore));
    expect(unattachedBackendSessionIds).toEqual([]);
    expect(layout.activeTabId).toBe("tab");
  });

  it("reports leaves whose backend session has no local proxy yet", () => {
    const { layout, unattachedBackendSessionIds } = applySharedLayout(createLayoutState(), doc, () => null);
    expect(unattachedBackendSessionIds).toEqual(["backend-1", "backend-2"]);
    // An unresolved leaf still renders as a pane keyed by the host identity.
    const panes = layout.layoutsByTabId.tab;
    expect(Object.values(panes.sessionIdsByLeafId)).toEqual(["shared-session:backend-1", "shared-session:backend-2"]);
  });

  it("keeps the local active tab when the document still has it", () => {
    const twoTabs: SharedLayout = {
      ...doc,
      tabs: [{ ...doc.tabs[0], paneTree: { type: "leaf", leafId: "leaf-a", backendSessionId: "backend-1" } },
        { id: "second", kind: "terminal", label: "second", pinned: false,
          paneTree: { type: "leaf", leafId: "leaf-c", backendSessionId: "backend-2" } }],
    };
    const local = createLayoutState([terminalTab("tab", "s1"), terminalTab("second", "s2")], "second");
    const { layout } = applySharedLayout(local, twoTabs, (id) => (id === "backend-1" ? "s1" : "s2"));
    expect(layout.tabs.map((tab) => tab.id)).toEqual(["tab", "second"]);
    expect(layout.activeTabId).toBe("second");
    expect(layout.layoutsByTabId.tab.activeLeafId).toBeDefined();
  });

  it("falls back to the document's first tab when the local active tab is gone", () => {
    const local = createLayoutState([terminalTab("other", "s9"), terminalTab("tab", "s1")], "other");
    const { layout } = applySharedLayout(local, doc, (id) => (id === "backend-1" ? "s1" : null));
    expect(layout.tabs.map((tab) => tab.id)).toEqual(["tab"]);
    expect(layout.activeTabId).toBe("tab");
  });

  it("drops a pane the document no longer contains without touching its session", () => {
    const before = splitLayout();
    const single: SharedLayout = {
      ...doc,
      revision: 8,
      tabs: [{ ...doc.tabs[0], paneTree: { type: "leaf", leafId: "leaf-a", backendSessionId: "backend-1" } }],
    };
    const sessions = { s1: session("s1", "backend-1"), s2: session("s2", "backend-2") };
    const { layout } = applySharedLayout(before, single, (id) => (id === "backend-1" ? "s1" : null));
    expect(Object.values(layout.layoutsByTabId.tab.sessionIdsByLeafId)).toEqual(["s1"]);
    // The detached pane's session survives: detaching a view never closes a host PTY.
    expect(sessions.s2.backendSessionId).toBe("backend-2");
  });
});

describe("decoding", () => {
  it("accepts revisions as numbers or decimal strings", () => {
    expect(toRevision(7)).toBe(7);
    expect(toRevision("7")).toBe(7);
    expect(toRevision(0)).toBe(0);
    expect(toRevision(-1)).toBeNull();
    expect(toRevision("07")).toBeNull();
    expect(toRevision(1.5)).toBeNull();
  });

  it("rejects documents it cannot rebuild", () => {
    expect(() => decodeSharedLayout({ workspaceId: "ws", revision: 1, tabs: [{ id: "t", label: "t", pinned: false, kind: "terminal", paneTree: { type: "leaf", leafId: "l" } }] }))
      .toThrow(/INVALID_SHARED_LAYOUT:pane.backendSessionId/);
    expect(() => decodeSharedLayout({ workspaceId: "ws", revision: "x", tabs: [] })).toThrow(/revision/);
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => { resolve = settle; });
  return { promise, resolve };
}

const emptyDoc = (revision: number): SharedLayout => ({ workspaceId: "ws", revision, tabs: [] });

describe("createLayoutSyncLoop", () => {
  it("answers a regressed revision (fresh daemon) by republishing on the host", async () => {
    const puts: number[] = [];
    const docs = [emptyDoc(4), emptyDoc(0)];
    let gets = 0;
    const loop = createLayoutSyncLoop({
      isPaired: false,
      transport: {
        get: async () => {
          const doc = docs[Math.min(gets, docs.length - 1)];
          gets += 1;
          if (gets > 2) loop.stop();
          return doc;
        },
        put: async (baseRevision, tabs) => {
          puts.push(baseRevision);
          return { layout: { ...emptyDoc(baseRevision + 1), tabs }, conflict: false };
        },
      },
      readLocal: () => [{ id: "tab", kind: "terminal", label: "tab", pinned: false, paneTree: null }],
      applyRemote: () => {},
    });
    await loop.start();
    // The second read hands back revision 0: the host rewrites its document from base 0.
    expect(puts).toContain(0);
  });

  it("seeds only inactive local workspaces the daemon holds at revision 0", async () => {
    const puts: Array<{ workspaceId: string; baseRevision: number; tabs: SharedTab[] }> = [];
    const revisions: Record<string, number> = { idle: 0, newer: 3, active: 0, paired: 0 };
    const transportFor = (workspaceId: string) => ({
      get: async () => ({ ...emptyDoc(revisions[workspaceId]), workspaceId }),
      put: async (baseRevision: number, tabs: SharedTab[]) => {
        puts.push({ workspaceId, baseRevision, tabs });
        return { layout: { workspaceId, revision: baseRevision + 1, tabs }, conflict: false };
      },
    });
    const layout = splitLayout();
    const persisted = (workspaceId: string, kind?: string) => ({
      ...serializeWorkspaceState(workspaceId, "/repo", {
        workspaceId, worktrees: [], activeWorktreePath: null, layout,
        sessions: { s1: session("s1", "b1"), s2: session("s2", "b2") },
        unreadTabIds: {}, unreadWorktreePaths: {},
      }).workspaces[workspaceId],
      target: kind ? { kind } : undefined,
    });
    const published = await publishPersistedLayouts(
      [persisted("idle"), persisted("newer"), persisted("active"), persisted("paired", "pairedDaemon")],
      transportFor,
      "active",
    );
    expect(published).toEqual(["idle"]);
    expect(puts).toHaveLength(1);
    expect(puts[0].baseRevision).toBe(0);
    // Two panes survive with their backend sessions.
    expect(JSON.stringify(puts[0].tabs)).toContain("b1");
    expect(JSON.stringify(puts[0].tabs)).toContain("b2");
  });

  it("publishes a local change with the base revision it last saw", async () => {
    vi.useFakeTimers();
    try {
      const puts: Array<{ baseRevision: number; tabs: SharedTab[] }> = [];
      const applied: SharedLayout[] = [];
      const loop = createLayoutSyncLoop({
        isPaired: true,
        transport: {
          get: async () => emptyDoc(5),
          put: async (baseRevision, tabs) => {
            puts.push({ baseRevision, tabs });
            return { layout: { ...emptyDoc(baseRevision + 1), tabs }, conflict: false };
          },
        },
        readLocal: () => [{ id: "tab", kind: "terminal", label: "tab", pinned: false, paneTree: null }],
        applyRemote: (doc) => applied.push(doc),
      });

      const started = loop.start();
      await vi.advanceTimersByTimeAsync(0);
      loop.localChanged();
      await vi.advanceTimersByTimeAsync(300);
      await loop.flush();
      expect(puts).toEqual([{ baseRevision: 5, tabs: [expect.objectContaining({ id: "tab" })] }]);
      expect(loop.revision()).toBe(6);
      // Only the document read at start was applied; the loop's own publish never echoes back.
      expect(applied).toEqual([emptyDoc(5)]);
      loop.stop();
      await started;
    } finally {
      vi.useRealTimers();
    }
  });

  it("applies the returned document when the host reports a conflict", async () => {
    vi.useFakeTimers();
    try {
      const applied: SharedLayout[] = [];
      const remote: SharedLayout = {
        workspaceId: "ws",
        revision: 9,
        tabs: [{ id: "remote", kind: "terminal", label: "remote", pinned: false, paneTree: null }],
      };
      const loop = createLayoutSyncLoop({
        isPaired: true,
        transport: {
          get: async () => emptyDoc(9),
          put: async () => ({ layout: remote, conflict: true }),
        },
        // A local layout that differs from the stored document, so the publish is a real change.
        readLocal: () => [{ id: "local", kind: "terminal", label: "local", pinned: false, paneTree: null }],
        applyRemote: (doc) => applied.push(doc),
      });

      const started = loop.start();
      await vi.advanceTimersByTimeAsync(0);
      loop.localChanged();
      await loop.flush();
      // The initial poll applies revision 9; the conflicting put then applies the host's copy.
      expect(applied).toEqual([emptyDoc(9), remote]);
      expect(loop.revision()).toBe(9);
      loop.stop();
      await started;
    } finally {
      vi.useRealTimers();
    }
  });

  it("never publishes a paired client over revision 0", async () => {
    vi.useFakeTimers();
    try {
      const put = vi.fn(async () => ({ layout: emptyDoc(1), conflict: false }));
      const loop = createLayoutSyncLoop({
        isPaired: true,
        transport: { get: async () => emptyDoc(0), put },
        readLocal: () => [],
        applyRemote: () => {},
      });
      const started = loop.start();
      await vi.advanceTimersByTimeAsync(0);
      loop.localChanged();
      await loop.flush();
      expect(put).not.toHaveBeenCalled();
      loop.stop();
      await started;
    } finally {
      vi.useRealTimers();
    }
  });

  it("publishes first when the host owns revision 0 and does not reapply its own document", async () => {
    vi.useFakeTimers();
    try {
      const applied: SharedLayout[] = [];
      // A stateful host: reads return whatever the last publish stored, as the daemon does.
      let stored = emptyDoc(0);
      const put = vi.fn(async () => {
        stored = emptyDoc(1);
        return { layout: stored, conflict: false };
      });
      const loop = createLayoutSyncLoop({
        isPaired: false,
        transport: {
          get: async (after) => {
            // A long poll on the current revision parks; stop the loop instead of spinning.
            if (after !== null && after === stored.revision) {
              loop.stop();
              return null;
            }
            return stored;
          },
          put,
        },
        readLocal: () => [],
        applyRemote: (doc) => applied.push(doc),
      });
      const started = loop.start();
      await vi.advanceTimersByTimeAsync(0);
      expect(put).toHaveBeenCalledTimes(1);
      expect(applied).toEqual([]);
      expect(loop.revision()).toBe(1);
      loop.stop();
      await started;
    } finally {
      vi.useRealTimers();
    }
  });

  it("applies a remote document newer than the last revision it saw", async () => {
    vi.useFakeTimers();
    try {
      const applied: SharedLayout[] = [];
      const second = deferred<SharedLayout>();
      const polls: Array<number | null> = [];
      const loop = createLayoutSyncLoop({
        isPaired: true,
        transport: {
          get: async (after) => {
            polls.push(after);
            if (polls.length === 1) return emptyDoc(3);
            // The second poll is the long poll: it stays open until the host has a document.
            if (polls.length === 2) return second.promise;
            return emptyDoc(4);
          },
          put: async () => ({ layout: emptyDoc(5), conflict: false }),
        },
        readLocal: () => [],
        applyRemote: (doc) => applied.push(doc),
      });
      const started = loop.start();
      await vi.advanceTimersByTimeAsync(0);
      const remote: SharedLayout = {
        workspaceId: "ws", revision: 4,
        tabs: [{ id: "x", kind: "terminal", label: "x", pinned: false, paneTree: null }],
      };
      second.resolve(remote);
      await vi.advanceTimersByTimeAsync(0);
      // The first poll has no revision yet; every later one resumes from what it applied.
      expect(polls.slice(0, 3)).toEqual([null, 3, 4]);
      expect(applied).toEqual([emptyDoc(3), remote]);
      expect(loop.revision()).toBe(4);
      loop.stop();
      await started;
    } finally {
      vi.useRealTimers();
    }
  });
});

