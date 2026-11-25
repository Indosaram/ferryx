import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BROWSER_STORE_MIGRATION_KEY, BROWSER_STORE_STORAGE_KEY } from "../lib/storageKeys";
import {
  createBrowserStore,
  createDefaultBrowserState,
  DEFAULT_SPACE_ID,
  MAX_FAVORITES,
  migrateLegacyBrowserTabs,
  runBrowserStoreMigrationOnce,
  selectFavoriteTabs,
  selectFolders,
  selectMruTabs,
  selectPinnedTabs,
  selectSpaceList,
  selectTabsInSection,
  selectTodayTabs,
  type BrowserStore,
} from "./browserStore";

/** jsdom's `localStorage` is not writable on every runtime, so each store gets its own Storage. */
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

const NOW = 1_700_000_000_000;

function createTestStore(storage: Storage | null = memoryStorage()): BrowserStore {
  let clock = NOW;
  let counter = 0;
  return createBrowserStore({
    storage,
    now: () => {
      clock += 1;
      return clock;
    },
    createId: (prefix) => {
      counter += 1;
      return `${prefix}-${counter}`;
    },
  });
}

/** Persisted workspace session shape the legacy layout lives in (workspaces -> layout). */
function persistedWorkspace(entries: {
  workspaceId: string;
  activeWorktreePath?: string | null;
  layoutTabs?: unknown[];
  layoutsByTabId?: Record<string, { contentsByLeafId: Record<string, unknown> }>;
  worktreeLayouts?: Record<string, { tabs: unknown[] }>;
}) {
  return {
    version: 3,
    timestamp: NOW,
    activeWorkspaceId: entries.workspaceId,
    workspaces: {
      [entries.workspaceId]: {
        workspaceId: entries.workspaceId,
        repoRoot: "/repo",
        activeWorktreePath: entries.activeWorktreePath ?? null,
        worktrees: [],
        layout: {
          tabs: entries.layoutTabs ?? [],
          activeTabId: null,
          layoutsByTabId: entries.layoutsByTabId ?? {},
        },
        worktreeLayouts: entries.worktreeLayouts,
        terminalSessions: {},
      },
    },
  };
}

function legacyBrowserTab(browserId: string, url: string, title: string, worktreePath?: string) {
  return {
    id: `tab-${browserId}`,
    kind: "browser",
    label: title,
    worktreePath,
    browser: { browserId, url, title, worktreePath },
  };
}

describe("browserStore", () => {
  let storage: Storage;
  let store: BrowserStore;

  beforeEach(() => {
    storage = memoryStorage();
    store = createTestStore(storage);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("sections", () => {
    it("creates tabs into Today, then keeps favorite, pinned and folder groups ordered independently", () => {
      const today = store.upsertTab({ browserId: "b-today", url: "https://a.test", title: "A" });
      expect(today.ok).toBe(true);
      expect(selectTodayTabs(store.getState())).toHaveLength(1);

      const favorite = store.upsertTab({ browserId: "b-fav", url: "https://f.test", title: "F", section: "favorite" });
      expect(favorite.ok).toBe(true);
      expect(selectFavoriteTabs(store.getState()).map((tab) => tab.browserId)).toEqual(["b-fav"]);

      const folder = store.createFolder({ name: "Docs" });
      expect(folder.ok).toBe(true);
      if (!folder.ok) return;

      const pinned = store.upsertTab({
        browserId: "b-pin",
        url: "https://p.test",
        title: "P",
        section: "pinned",
        folderId: folder.value.id,
      });
      expect(pinned.ok).toBe(true);
      expect(selectPinnedTabs(store.getState())).toHaveLength(0);
      expect(selectTabsInSection(store.getState(), DEFAULT_SPACE_ID, "pinned", folder.value.id)).toHaveLength(1);
      expect(selectTodayTabs(store.getState())).toHaveLength(1);
      expect(selectFolders(store.getState())).toHaveLength(1);
    });

    it("moves a tab between sections and spaces while renumbering both groups", () => {
      store.upsertTab({ browserId: "b1", url: "https://1.test", title: "1" });
      store.upsertTab({ browserId: "b2", url: "https://2.test", title: "2" });
      store.upsertTab({ browserId: "b3", url: "https://3.test", title: "3" });
      const space = store.createSpace({ name: "Work", color: "#FF8800" });
      if (!space.ok) throw new Error("space create failed");

      const moved = store.moveTab("b1", { section: "favorite" });
      expect(moved.ok).toBe(true);
      expect(selectTodayTabs(store.getState()).map((tab) => [tab.browserId, tab.order])).toEqual([
        ["b2", 0],
        ["b3", 1],
      ]);

      const toSpace = store.moveTab("b2", { spaceId: space.value.id, section: "today", index: 0 });
      expect(toSpace.ok).toBe(true);
      expect(selectTodayTabs(store.getState(), space.value.id).map((tab) => tab.browserId)).toEqual(["b2"]);
      expect(selectTodayTabs(store.getState()).map((tab) => [tab.browserId, tab.order])).toEqual([["b3", 0]]);
    });

    it("drops the folder reference when a pinned tab leaves the pinned section", () => {
      const folder = store.createFolder({ name: "Work" });
      if (!folder.ok) throw new Error("folder create failed");
      store.upsertTab({ browserId: "b1", section: "pinned", folderId: folder.value.id, url: "https://1.test" });

      const moved = store.moveTab("b1", { section: "today" });
      expect(moved.ok).toBe(true);
      if (!moved.ok) return;
      expect(moved.value.folderId).toBeUndefined();
    });

    it("rejects an unknown folder and an unknown space with typed errors", () => {
      expect(store.upsertTab({ browserId: "b1", section: "pinned", folderId: "nope" })).toEqual({ ok: false, error: "unknown-folder" });
      expect(store.moveTab("b1", { spaceId: "nope" })).toEqual({ ok: false, error: "unknown-tab" });
      store.upsertTab({ browserId: "b1", url: "https://1.test" });
      expect(store.moveTab("b1", { spaceId: "nope" })).toEqual({ ok: false, error: "unknown-space" });
    });

    it("caps favorites at 12 with a typed error and refuses the 13th", () => {
      for (let index = 0; index < MAX_FAVORITES; index += 1) {
        expect(store.upsertTab({ browserId: `fav-${index}`, section: "favorite", url: `https://${index}.test` }).ok).toBe(true);
      }
      expect(store.upsertTab({ browserId: "fav-extra", section: "favorite", url: "https://extra.test" })).toEqual({
        ok: false,
        error: "favorite-limit",
      });
      store.upsertTab({ browserId: "b-move", url: "https://move.test" });
      expect(store.moveTab("b-move", { section: "favorite" })).toEqual({ ok: false, error: "favorite-limit" });
      expect(selectFavoriteTabs(store.getState())).toHaveLength(MAX_FAVORITES);
      // Re-saving an existing favorite is not a new tile and must keep working.
      expect(store.upsertTab({ browserId: "fav-0", section: "favorite", title: "renamed" }).ok).toBe(true);
    });

    it("deletes a folder, reparenting child folders and releasing its tabs", () => {
      const parent = store.createFolder({ name: "Parent" });
      if (!parent.ok) throw new Error("folder create failed");
      const child = store.createFolder({ name: "Child", parentId: parent.value.id });
      if (!child.ok) throw new Error("folder create failed");
      store.upsertTab({ browserId: "b1", section: "pinned", folderId: parent.value.id, url: "https://1.test" });

      expect(store.deleteFolder(parent.value.id).ok).toBe(true);
      expect(selectFolders(store.getState()).map((folder) => [folder.id, folder.parentId])).toEqual([[child.value.id, undefined]]);
      expect(store.getState().tabs.b1.folderId).toBeUndefined();
    });
  });

  describe("spaces", () => {
    it("renames, recolors, reorders and reassigns tabs when a Space is deleted", () => {
      const work = store.createSpace({ name: "Work", color: "#111111" });
      if (!work.ok) throw new Error("space create failed");
      expect(store.renameSpace(work.value.id, "Office").ok).toBe(true);
      expect(store.recolorSpace(work.value.id, "#222222").ok).toBe(true);
      store.upsertTab({ browserId: "b1", spaceId: work.value.id, url: "https://1.test" });

      const reordered = store.reorderSpaces(work.value.id, 0);
      expect(reordered.ok).toBe(true);
      expect(selectSpaceList(store.getState()).map((space) => space.name)).toEqual(["Office", "Personal"]);

      expect(store.deleteSpace(work.value.id).ok).toBe(true);
      expect(store.getState().tabs.b1.spaceId).toBe(DEFAULT_SPACE_ID);
      expect(store.getState().activeSpaceId).toBe(DEFAULT_SPACE_ID);
    });

    it("refuses to delete the last Space and refuses an empty name", () => {
      expect(store.deleteSpace(DEFAULT_SPACE_ID)).toEqual({ ok: false, error: "cannot-delete-last-space" });
      expect(store.renameSpace(DEFAULT_SPACE_ID, "   ")).toEqual({ ok: false, error: "invalid-name" });
      expect(store.createSpace({ name: "" })).toEqual({ ok: false, error: "invalid-name" });
    });
  });

  describe("MRU", () => {
    it("keeps most-recently-used order and skips closed tabs", () => {
      store.upsertTab({ browserId: "b1", url: "https://1.test" });
      store.upsertTab({ browserId: "b2", url: "https://2.test" });
      store.upsertTab({ browserId: "b3", url: "https://3.test" });

      store.touch("b1");
      store.touch("b3");
      expect(store.getState().mru).toEqual(["b3", "b1"]);
      expect(selectMruTabs(store.getState()).map((tab) => tab.browserId)).toEqual(["b3", "b1"]);

      store.closeTab("b3");
      expect(store.getState().mru).toEqual(["b1"]);
    });
  });

  describe("archive", () => {
    it("archives a closed tab and restores it into Today", () => {
      store.upsertTab({ browserId: "b1", url: "https://1.test", title: "One", section: "pinned" });
      const closed = store.closeTab("b1");
      expect(closed.ok && closed.value.archived).toBe(true);
      expect(store.getState().tabs.b1).toBeUndefined();
      expect(store.getState().archive).toHaveLength(1);

      const restored = store.restoreFromArchive("b1");
      expect(restored.ok).toBe(true);
      if (!restored.ok) return;
      expect(restored.value.section).toBe("today");
      expect(store.getState().archive).toHaveLength(0);
      expect(selectTodayTabs(store.getState()).map((tab) => tab.browserId)).toEqual(["b1"]);
    });

    it("clears every Today tab of the active Space into the archive, leaving pinned tabs alone", () => {
      store.upsertTab({ browserId: "b1", url: "https://1.test" });
      store.upsertTab({ browserId: "b2", url: "https://2.test" });
      store.upsertTab({ browserId: "pin", section: "pinned", url: "https://p.test" });

      expect(store.clearToday()).toBe(2);
      expect(selectTodayTabs(store.getState())).toHaveLength(0);
      expect(selectPinnedTabs(store.getState()).map((tab) => tab.browserId)).toEqual(["pin"]);
      expect(store.getState().archive.map((entry) => [entry.tab.browserId, entry.reason])).toEqual([
        ["b1", "cleared"],
        ["b2", "cleared"],
      ]);
    });

    it("reports a missing archive entry instead of throwing", () => {
      expect(store.restoreFromArchive("nope")).toEqual({ ok: false, error: "unknown-archive-entry" });
    });
  });

  describe("mini slot and splits", () => {
    it("promotes the mini tab into a Space and clears the slot", () => {
      store.upsertTab({ browserId: "b1", url: "https://1.test" });
      expect(store.setMini("b1").ok).toBe(true);
      const promoted = store.promoteMini();
      expect(promoted.ok).toBe(true);
      expect(store.getState().miniBrowserId).toBeNull();
      expect(selectTodayTabs(store.getState()).map((tab) => tab.browserId)).toEqual(["b1"]);
    });

    it("keeps tabs when a worktree is deleted, dropping only its split references", () => {
      store.upsertTab({ browserId: "b1", url: "https://1.test" });
      store.upsertTab({ browserId: "b2", url: "https://2.test" });
      expect(store.attachSplit("b1", { workspaceId: "ws-a", tabId: "t1", leafId: "l1" }).ok).toBe(true);
      expect(store.attachSplit("b2", { workspaceId: "ws-b", tabId: "t2", leafId: "l2" }).ok).toBe(true);

      expect(store.detachAllForWorkspace("ws-a")).toBe(1);
      expect(store.getState().splitAttachments.b1).toBeUndefined();
      expect(store.getState().splitAttachments.b2).toEqual({ workspaceId: "ws-b", tabId: "t2", leafId: "l2" });
      expect(Object.keys(store.getState().tabs).sort()).toEqual(["b1", "b2"]);
    });
  });

  describe("persistence", () => {
    it("never persists private tabs and never surfaces a stored private tab", () => {
      store.upsertTab({ browserId: "public", url: "https://public.test" });
      store.upsertTab({ browserId: "secret", url: "https://secret.test", private: true });

      const raw = storage.getItem(BROWSER_STORE_STORAGE_KEY) ?? "";
      expect(raw).toContain("public");
      expect(raw).not.toContain("secret");

      const reloaded = createTestStore(storage);
      expect(Object.keys(reloaded.getState().tabs)).toEqual(["public"]);

      const handcrafted = JSON.parse(raw) as { tabs: Array<Record<string, unknown>> };
      handcrafted.tabs.push({ browserId: "injected", url: "https://x.test", private: true });
      storage.setItem(BROWSER_STORE_STORAGE_KEY, JSON.stringify(handcrafted));
      const fromTampered = createTestStore(storage);
      expect(Object.keys(fromTampered.getState().tabs)).toEqual(["public"]);
    });

    it("restores persisted Spaces, folders, MRU and splits across a reload", () => {
      const space = store.createSpace({ name: "Work", color: "#334455" });
      if (!space.ok) throw new Error("space create failed");
      const folder = store.createFolder({ name: "Docs", spaceId: space.value.id });
      if (!folder.ok) throw new Error("folder create failed");
      store.upsertTab({ browserId: "b1", spaceId: space.value.id, section: "pinned", folderId: folder.value.id, url: "https://1.test" });
      store.attachSplit("b1", { workspaceId: "ws-a", tabId: "t1", leafId: "l1" });
      store.touch("b1");

      const reloaded = createTestStore(storage);
      const state = reloaded.getState();
      expect(selectSpaceList(state).map((candidate) => candidate.name)).toEqual(["Personal", "Work"]);
      expect(selectFolders(state, space.value.id).map((candidate) => candidate.name)).toEqual(["Docs"]);
      expect(state.tabs.b1.folderId).toBe(folder.value.id);
      expect(state.mru).toEqual(["b1"]);
      expect(state.splitAttachments.b1).toEqual({ workspaceId: "ws-a", tabId: "t1", leafId: "l1" });
    });

    it("falls back to an empty default store with a console warning on malformed JSON", () => {
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      storage.setItem(BROWSER_STORE_STORAGE_KEY, "{not json at all");

      const broken = createTestStore(storage);
      expect(() => broken.getState()).not.toThrow();
      expect(broken.getState()).toEqual(createDefaultBrowserState());
      expect(warn).toHaveBeenCalled();
    });

    it("falls back to the default store when the JSON is valid but has no usable Spaces", () => {
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      storage.setItem(BROWSER_STORE_STORAGE_KEY, JSON.stringify({ spaces: [], tabs: [{ browserId: "b1" }] }));

      const broken = createTestStore(storage);
      expect(broken.getState()).toEqual(createDefaultBrowserState());
      expect(warn).toHaveBeenCalled();
    });
  });

  describe("legacy migration", () => {
    const twoWorktreesTwoTabs = persistedWorkspace({
      workspaceId: "ws-1",
      activeWorktreePath: "/repo/main",
      layoutTabs: [legacyBrowserTab("b-main-1", "https://main-1.test", "Main 1", "/repo/main")],
      layoutsByTabId: {
        "tab-main": {
          contentsByLeafId: {
            "leaf-main": {
              kind: "browser",
              browser: { browserId: "b-leaf-main", url: "https://leaf-main.test", title: "Leaf", worktreePath: "/repo/main", worktreeLabel: "main" },
            },
          },
        },
      },
      worktreeLayouts: {
        "/repo/feature": {
          tabs: [
            legacyBrowserTab("b-feat-1", "https://feat-1.test", "Feat 1", "/repo/feature"),
            legacyBrowserTab("b-feat-2", "https://feat-2.test", "Feat 2", "/repo/feature"),
          ],
        },
      },
    });

    it("collects worktree tabs and browser leaves into Today of the default Space with worktree tags", () => {
      const state = migrateLegacyBrowserTabs(twoWorktreesTwoTabs, { now: NOW });
      const today = selectTodayTabs(state, DEFAULT_SPACE_ID);
      expect(today.map((tab) => tab.browserId).sort()).toEqual(["b-feat-1", "b-feat-2", "b-leaf-main", "b-main-1"]);
      expect(today.every((tab) => tab.worktreeTag !== undefined)).toBe(true);
      expect(today.every((tab) => tab.section === "today")).toBe(true);
      expect(today.find((tab) => tab.browserId === "b-feat-1")?.worktreeTag).toEqual({
        workspaceId: "ws-1",
        worktreePath: "/repo/feature",
      });
      expect(today.find((tab) => tab.browserId === "b-leaf-main")?.worktreeTag).toEqual({
        workspaceId: "ws-1",
        worktreePath: "/repo/main",
        label: "main",
      });
      // Leaves keep their split reference so the existing worktree split points at the same session.
      expect(state.splitAttachments["b-leaf-main"]).toEqual({ workspaceId: "ws-1", tabId: "tab-main", leafId: "leaf-main" });
    });

    it("skips private-profile legacy rows", () => {
      const state = migrateLegacyBrowserTabs(
        persistedWorkspace({
          workspaceId: "ws-1",
          layoutTabs: [
            { id: "t1", kind: "browser", label: "Secret", browser: { browserId: "b-secret", url: "https://s.test", profileId: "private" } },
            legacyBrowserTab("b-public", "https://p.test", "Public"),
          ],
        }),
        { now: NOW },
      );
      expect(Object.keys(state.tabs)).toEqual(["b-public"]);
    });

    it("is idempotent: migrating twice yields identical state", () => {
      const first = migrateLegacyBrowserTabs(twoWorktreesTwoTabs, { now: NOW });
      const second = migrateLegacyBrowserTabs(twoWorktreesTwoTabs, { now: NOW });
      expect(second).toEqual(first);
    });

    it("applies once through the guard and leaves the store untouched on the second run", () => {
      const migrated = createTestStore(storage);
      expect(runBrowserStoreMigrationOnce({ persistedWorkspaceState: twoWorktreesTwoTabs, store: migrated, storage, now: NOW })).toBe(true);
      const snapshot = migrated.getState();
      expect(selectTodayTabs(snapshot).map((tab) => tab.browserId).sort()).toEqual(["b-feat-1", "b-feat-2", "b-leaf-main", "b-main-1"]);
      expect(storage.getItem(BROWSER_STORE_MIGRATION_KEY)).toBe("1");

      expect(runBrowserStoreMigrationOnce({ persistedWorkspaceState: twoWorktreesTwoTabs, store: migrated, storage, now: NOW })).toBe(false);
      expect(migrated.getState()).toBe(snapshot);
    });

    it("does nothing without persisted state and stays silent on unreadable input", () => {
      const migrated = createTestStore(storage);
      expect(runBrowserStoreMigrationOnce({ store: migrated, storage })).toBe(false);
      expect(migrated.getState()).toEqual(createDefaultBrowserState());
      expect(migrateLegacyBrowserTabs(null).tabs).toEqual({});
      expect(migrateLegacyBrowserTabs("nonsense").tabs).toEqual({});
    });
  });
});
