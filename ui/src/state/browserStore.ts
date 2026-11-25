import { useCallback, useRef, useSyncExternalStore } from "react";

import { isPrivateBrowserProfileId } from "../lib/browserSettings";
import { BROWSER_STORE_MIGRATION_KEY, BROWSER_STORE_STORAGE_KEY } from "../lib/storageKeys";
import type { BrowserEngine } from "../lib/types";

/**
 * Global browser store: one place owns every browser tab, independent of any worktree layout.
 * A worktree split pane holds a `browserId` reference (`splitAttachments`) instead of owning the
 * session, so deleting a worktree never closes the user's tabs and the very same session can be
 * drawn fullscreen in Browser mode or inside a split without a reload.
 *
 * Persistence is `ferryx.browser.store:v1`; private tabs are never written to disk. A malformed
 * blob restores an empty default store with a console warning - this module never throws at the
 * boundary of untrusted storage.
 */

export const DEFAULT_SPACE_ID = "space-default";
export const DEFAULT_SPACE_NAME = "Personal";
export const DEFAULT_SPACE_COLOR = "#6C8CFF";
/** Favorites are one shared 4x3 grid: 12 tiles across every Space. */
export const MAX_FAVORITES = 12;
/** Most-recently-used switcher depth. */
export const MAX_MRU_ENTRIES = 100;

export type BrowserSection = "favorite" | "pinned" | "today";

export type BrowserWorktreeTag = {
  workspaceId: string;
  worktreePath: string;
  label?: string;
};

export type BrowserSpace = {
  id: string;
  name: string;
  /** Hex color driving the Space palette. */
  color: string;
  order: number;
};

export type BrowserStoreTab = {
  browserId: string;
  spaceId: string;
  url: string;
  title: string;
  faviconUrl?: string;
  section: BrowserSection;
  /** Only pinned tabs live inside folders. */
  folderId?: string;
  /** Position inside its own section/folder; renumbered by `moveTab`. */
  order: number;
  lastActiveAt: number;
  worktreeTag?: BrowserWorktreeTag;
  agentControlled?: boolean;
  private?: boolean;
  engine?: BrowserEngine;
};

export type BrowserFolder = {
  id: string;
  spaceId: string;
  name: string;
  parentId?: string;
  order: number;
};

export type BrowserArchiveReason = "closed" | "cleared";

export type BrowserArchiveEntry = {
  tab: BrowserStoreTab;
  archivedAt: number;
  reason: BrowserArchiveReason;
};

export type BrowserSplitAttachment = {
  workspaceId: string;
  tabId: string;
  leafId: string;
};

export type BrowserStoreState = {
  spaces: Readonly<Record<string, BrowserSpace>>;
  folders: Readonly<Record<string, BrowserFolder>>;
  tabs: Readonly<Record<string, BrowserStoreTab>>;
  archive: readonly BrowserArchiveEntry[];
  /** browserIds, most recently used first. */
  mru: readonly string[];
  activeSpaceId: string;
  activeBrowserIdBySpace: Readonly<Record<string, string | null>>;
  miniBrowserId: string | null;
  /** browserId -> the split pane currently showing it. */
  splitAttachments: Readonly<Record<string, BrowserSplitAttachment>>;
};

export type BrowserStoreErrorCode =
  | "favorite-limit"
  | "unknown-tab"
  | "unknown-space"
  | "unknown-folder"
  | "unknown-archive-entry"
  | "cannot-delete-last-space"
  | "invalid-name";

export type BrowserStoreFailure = { ok: false; error: BrowserStoreErrorCode };
export type BrowserStoreSuccess<T> = { ok: true; value: T };
export type BrowserStoreResult<T> = BrowserStoreSuccess<T> | BrowserStoreFailure;

export type CreateSpaceInput = { name: string; color?: string };

export type UpsertBrowserTabInput = {
  browserId: string;
  spaceId?: string;
  url?: string;
  title?: string | null;
  faviconUrl?: string | null;
  section?: BrowserSection;
  folderId?: string | null;
  order?: number;
  lastActiveAt?: number;
  worktreeTag?: BrowserWorktreeTag | null;
  agentControlled?: boolean;
  private?: boolean;
  engine?: BrowserEngine;
};

export type MoveTabTarget = {
  spaceId?: string;
  section?: BrowserSection;
  folderId?: string | null;
  index?: number;
};

export type CreateFolderInput = { name: string; spaceId?: string; parentId?: string | null };

export type BrowserStoreActions = {
  createSpace: (input: CreateSpaceInput) => BrowserStoreResult<BrowserSpace>;
  renameSpace: (spaceId: string, name: string) => BrowserStoreResult<BrowserSpace>;
  recolorSpace: (spaceId: string, color: string) => BrowserStoreResult<BrowserSpace>;
  /** Removes the Space and reassigns its tabs and folders to the next Space. */
  deleteSpace: (spaceId: string) => BrowserStoreResult<BrowserSpace>;
  reorderSpaces: (spaceId: string, toIndex: number) => BrowserStoreResult<readonly BrowserSpace[]>;
  setActiveSpace: (spaceId: string) => BrowserStoreResult<BrowserSpace>;

  upsertTab: (input: UpsertBrowserTabInput) => BrowserStoreResult<BrowserStoreTab>;
  /** Closes a tab; non-private tabs are archived and can be restored. */
  closeTab: (browserId: string) => BrowserStoreResult<{ tab: BrowserStoreTab; archived: boolean }>;
  moveTab: (browserId: string, target: MoveTabTarget) => BrowserStoreResult<BrowserStoreTab>;

  createFolder: (input: CreateFolderInput) => BrowserStoreResult<BrowserFolder>;
  renameFolder: (folderId: string, name: string) => BrowserStoreResult<BrowserFolder>;
  /** Deletes the folder, reparenting its child folders and moving its tabs to the parent. */
  deleteFolder: (folderId: string) => BrowserStoreResult<BrowserFolder>;

  /** Archives every Today tab of the Space. Returns how many tabs were archived. */
  clearToday: (spaceId?: string) => number;
  restoreFromArchive: (browserId: string) => BrowserStoreResult<BrowserStoreTab>;

  touch: (browserId: string) => BrowserStoreResult<BrowserStoreTab>;
  setMini: (browserId: string | null) => BrowserStoreResult<string | null>;
  promoteMini: (spaceId?: string) => BrowserStoreResult<BrowserStoreTab>;

  attachSplit: (browserId: string, attachment: BrowserSplitAttachment) => BrowserStoreResult<BrowserSplitAttachment>;
  detachSplit: (browserId: string) => boolean;
  /** Worktree deletion: only the split references go, the tabs stay. */
  detachAllForWorkspace: (workspaceId: string) => number;
};

export type BrowserStore = BrowserStoreActions & {
  getState: () => BrowserStoreState;
  subscribe: (listener: (state: BrowserStoreState) => void) => () => void;
  setState: (updater: (prev: BrowserStoreState) => BrowserStoreState) => void;
  reset: () => void;
};

export type BrowserStoreOptions = {
  storage?: Storage | null;
  now?: () => number;
  createId?: (prefix: string) => string;
  initialState?: BrowserStoreState;
};

const SECTION_VALUES: readonly BrowserSection[] = ["favorite", "pinned", "today"];
const ARCHIVE_REASON_VALUES: readonly BrowserArchiveReason[] = ["closed", "cleared"];

function failure(error: BrowserStoreErrorCode): BrowserStoreFailure {
  return { ok: false, error };
}

function success<T>(value: T): BrowserStoreSuccess<T> {
  return { ok: true, value };
}

function defaultStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function readString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value : undefined;
}

function readFiniteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function isSection(value: unknown): value is BrowserSection {
  return typeof value === "string" && (SECTION_VALUES as readonly string[]).includes(value);
}

function isArchiveReason(value: unknown): value is BrowserArchiveReason {
  return typeof value === "string" && (ARCHIVE_REASON_VALUES as readonly string[]).includes(value);
}

function isEngine(value: unknown): value is BrowserEngine {
  return value === "system" || value === "chromium";
}

function sortSpaces(spaces: Readonly<Record<string, BrowserSpace>>): BrowserSpace[] {
  return Object.values(spaces).sort((left, right) => left.order - right.order || left.id.localeCompare(right.id));
}

function sortFolders(folders: Readonly<Record<string, BrowserFolder>>): BrowserFolder[] {
  return Object.values(folders).sort((left, right) => left.order - right.order || left.id.localeCompare(right.id));
}

function sortTabs(tabs: Iterable<BrowserStoreTab>): BrowserStoreTab[] {
  return [...tabs].sort((left, right) => left.order - right.order || left.browserId.localeCompare(right.browserId));
}

function tabsInGroup(
  state: BrowserStoreState,
  spaceId: string,
  section: BrowserSection,
  folderId: string | undefined,
): BrowserStoreTab[] {
  return sortTabs(
    Object.values(state.tabs).filter((tab) => {
      if (tab.spaceId !== spaceId || tab.section !== section) return false;
      return section === "pinned" ? tab.folderId === folderId : tab.folderId === undefined;
    }),
  );
}

function clampIndex(index: number, length: number): number {
  if (!Number.isFinite(index)) return length;
  return Math.min(Math.max(Math.trunc(index), 0), length);
}

/** Identity of one ordered list (a section, or one folder inside Pinned). */
function groupOf(tab: BrowserStoreTab): string {
  return groupKey(tab.spaceId, tab.section, tab.folderId);
}

function groupKey(spaceId: string, section: BrowserSection, folderId: string | undefined): string {
  return `${spaceId}::${section}::${folderId ?? ""}`;
}

export function createDefaultBrowserState(spaceId: string = DEFAULT_SPACE_ID): BrowserStoreState {
  const space: BrowserSpace = { id: spaceId, name: DEFAULT_SPACE_NAME, color: DEFAULT_SPACE_COLOR, order: 0 };
  return {
    spaces: { [spaceId]: space },
    folders: {},
    tabs: {},
    archive: [],
    mru: [],
    activeSpaceId: spaceId,
    activeBrowserIdBySpace: {},
    miniBrowserId: null,
    splitAttachments: {},
  };
}

function serializeBrowserState(state: BrowserStoreState): string {
  const tabs = sortTabs(Object.values(state.tabs).filter((tab) => !tab.private));
  const persistedIds = new Set(tabs.map((tab) => tab.browserId));
  const splitAttachments: Record<string, BrowserSplitAttachment> = {};
  for (const [browserId, attachment] of Object.entries(state.splitAttachments)) {
    if (persistedIds.has(browserId)) splitAttachments[browserId] = attachment;
  }
  const activeBrowserIdBySpace: Record<string, string | null> = {};
  for (const [spaceId, browserId] of Object.entries(state.activeBrowserIdBySpace)) {
    if (!state.spaces[spaceId]) continue;
    activeBrowserIdBySpace[spaceId] = browserId && persistedIds.has(browserId) ? browserId : null;
  }
  const miniBrowserId = state.miniBrowserId && persistedIds.has(state.miniBrowserId) ? state.miniBrowserId : null;
  return JSON.stringify({
    version: 1,
    spaces: sortSpaces(state.spaces),
    folders: sortFolders(state.folders),
    tabs,
    archive: state.archive.filter((entry) => !entry.tab.private),
    mru: state.mru.filter((browserId) => persistedIds.has(browserId)).slice(0, MAX_MRU_ENTRIES),
    activeSpaceId: state.spaces[state.activeSpaceId] ? state.activeSpaceId : sortSpaces(state.spaces)[0]?.id ?? DEFAULT_SPACE_ID,
    activeBrowserIdBySpace,
    miniBrowserId,
    splitAttachments,
  });
}

/**
 * Rebuilds a store state from persisted JSON. Returns `null` for a missing or unusable blob so
 * the caller can fall back to the default store; malformed JSON is warned about, never thrown.
 */
function parseBrowserState(raw: string | null, storageStamp: number): BrowserStoreState | null {
  if (!raw) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    console.warn("Unable to parse persisted browser store", error);
    return null;
  }
  const root = asRecord(parsed);
  if (!root) {
    console.warn("Ignoring persisted browser store: expected an object");
    return null;
  }

  const spaces: Record<string, BrowserSpace> = {};
  for (const candidate of Array.isArray(root.spaces) ? root.spaces : []) {
    const record = asRecord(candidate);
    const id = readString(record?.id);
    if (!record || !id) continue;
    spaces[id] = {
      id,
      name: readString(record.name) ?? id,
      color: readString(record.color) ?? DEFAULT_SPACE_COLOR,
      order: readFiniteNumber(record.order) ?? Object.keys(spaces).length,
    };
  }
  if (Object.keys(spaces).length === 0) {
    console.warn("Ignoring persisted browser store: no usable Spaces");
    return null;
  }
  const activeSpaceIdCandidate = readString(root.activeSpaceId);
  const activeSpaceId = activeSpaceIdCandidate && spaces[activeSpaceIdCandidate]
    ? activeSpaceIdCandidate
    : sortSpaces(spaces)[0].id;

  const folders: Record<string, BrowserFolder> = {};
  for (const candidate of Array.isArray(root.folders) ? root.folders : []) {
    const record = asRecord(candidate);
    const id = readString(record?.id);
    const spaceId = readString(record?.spaceId);
    if (!record || !id || !spaceId || !spaces[spaceId]) continue;
    folders[id] = {
      id,
      spaceId,
      name: readString(record.name) ?? id,
      parentId: readString(record.parentId),
      order: readFiniteNumber(record.order) ?? Object.keys(folders).length,
    };
  }
  for (const folder of Object.values(folders)) {
    if (folder.parentId && !folders[folder.parentId]) delete folders[folder.id].parentId;
  }

  const tabs: Record<string, BrowserStoreTab> = {};
  for (const candidate of Array.isArray(root.tabs) ? root.tabs : []) {
    const tab = normalizeTab(candidate, spaces, folders, activeSpaceId, storageStamp);
    if (tab && !tab.private) tabs[tab.browserId] = tab;
  }
  for (const tab of Object.values(tabs)) {
    if (tab.folderId && !folders[tab.folderId]) delete tabs[tab.browserId].folderId;
  }

  const archive: BrowserArchiveEntry[] = [];
  for (const candidate of Array.isArray(root.archive) ? root.archive : []) {
    const record = asRecord(candidate);
    if (!record) continue;
    const tab = normalizeTab(record.tab, spaces, folders, activeSpaceId, storageStamp);
    if (!tab || tab.private) continue;
    archive.push({
      tab,
      archivedAt: readFiniteNumber(record.archivedAt) ?? tab.lastActiveAt,
      reason: isArchiveReason(record.reason) ? record.reason : "closed",
    });
  }

  const mru: string[] = [];
  for (const candidate of Array.isArray(root.mru) ? root.mru : []) {
    const browserId = readString(candidate);
    if (browserId && tabs[browserId] && !mru.includes(browserId)) mru.push(browserId);
    if (mru.length >= MAX_MRU_ENTRIES) break;
  }

  const activeBrowserIdBySpace: Record<string, string | null> = {};
  for (const [spaceId, browserId] of Object.entries(asRecord(root.activeBrowserIdBySpace) ?? {})) {
    if (!spaces[spaceId]) continue;
    activeBrowserIdBySpace[spaceId] =
      typeof browserId === "string" && tabs[browserId]?.spaceId === spaceId ? browserId : null;
  }

  const miniBrowserId = readString(root.miniBrowserId);
  const splitAttachments: Record<string, BrowserSplitAttachment> = {};
  for (const [browserId, candidate] of Object.entries(asRecord(root.splitAttachments) ?? {})) {
    if (!tabs[browserId]) continue;
    const record = asRecord(candidate);
    const workspaceId = readString(record?.workspaceId);
    const tabId = readString(record?.tabId);
    const leafId = readString(record?.leafId);
    if (!workspaceId || !tabId || !leafId) continue;
    splitAttachments[browserId] = { workspaceId, tabId, leafId };
  }

  return {
    spaces,
    folders,
    tabs,
    archive,
    mru,
    activeSpaceId,
    activeBrowserIdBySpace,
    miniBrowserId: miniBrowserId && tabs[miniBrowserId] ? miniBrowserId : null,
    splitAttachments,
  };
}

function normalizeTab(
  candidate: unknown,
  spaces: Record<string, BrowserSpace>,
  folders: Record<string, BrowserFolder>,
  fallbackSpaceId: string,
  fallbackTimestamp: number,
): BrowserStoreTab | null {
  const record = asRecord(candidate);
  const browserId = readString(record?.browserId);
  if (!record || !browserId) return null;
  const spaceId = readString(record.spaceId);
  const tab: BrowserStoreTab = {
    browserId,
    spaceId: spaceId && spaces[spaceId] ? spaceId : fallbackSpaceId,
    url: readString(record.url) ?? "about:blank",
    title: readString(record.title) ?? "",
    section: isSection(record.section) ? record.section : "today",
    order: readFiniteNumber(record.order) ?? 0,
    lastActiveAt: readFiniteNumber(record.lastActiveAt) ?? fallbackTimestamp,
    private: record.private === true,
    agentControlled: record.agentControlled === true,
  };
  const faviconUrl = readString(record.faviconUrl);
  if (faviconUrl) tab.faviconUrl = faviconUrl;
  const folderId = readString(record.folderId);
  if (folderId && folders[folderId]?.spaceId === tab.spaceId && tab.section === "pinned") tab.folderId = folderId;
  if (isEngine(record.engine)) tab.engine = record.engine;
  const worktreeTag = asRecord(record.worktreeTag);
  const workspaceId = readString(worktreeTag?.workspaceId);
  const worktreePath = readString(worktreeTag?.worktreePath);
  if (workspaceId && worktreePath) {
    const tag: BrowserWorktreeTag = { workspaceId, worktreePath };
    const label = readString(worktreeTag?.label);
    if (label) tag.label = label;
    tab.worktreeTag = tag;
  }
  return tab;
}

export function createBrowserStore(options: BrowserStoreOptions = {}): BrowserStore {
  const now = options.now ?? (() => Date.now());
  const createId = options.createId
    ?? ((prefix: string) => `${prefix}-${Math.random().toString(36).slice(2, 10)}`);
  const resolveStorage = (): Storage | null => (options.storage === undefined ? defaultStorage() : options.storage);

  let state: BrowserStoreState | null = options.initialState ?? null;
  const listeners = new Set<(next: BrowserStoreState) => void>();

  function getState(): BrowserStoreState {
    if (state === null) {
      const storage = resolveStorage();
      let raw: string | null = null;
      try {
        raw = storage?.getItem(BROWSER_STORE_STORAGE_KEY) ?? null;
      } catch {
        raw = null;
      }
      state = parseBrowserState(raw, now()) ?? createDefaultBrowserState();
    }
    return state;
  }

  function persist(next: BrowserStoreState): void {
    const storage = resolveStorage();
    if (!storage) return;
    try {
      storage.setItem(BROWSER_STORE_STORAGE_KEY, serializeBrowserState(next));
    } catch (error) {
      console.warn("Unable to persist browser store", error);
    }
  }

  function commit(next: BrowserStoreState): void {
    if (next === state) return;
    state = next;
    persist(next);
    for (const listener of listeners) listener(next);
  }

  function setState(updater: (prev: BrowserStoreState) => BrowserStoreState): void {
    commit(updater(getState()));
  }

  function subscribe(listener: (next: BrowserStoreState) => void): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }

  function spaceById(spaceId: string | undefined): BrowserSpace | undefined {
    return spaceId ? getState().spaces[spaceId] : undefined;
  }

  function firstFavoriteCount(): number {
    return Object.values(getState().tabs).filter((tab) => tab.section === "favorite").length;
  }

  function nextGroupOrder(spaceId: string, section: BrowserSection, folderId: string | undefined): number {
    return tabsInGroup(getState(), spaceId, section, folderId).length;
  }

  function removeTab(tab: BrowserStoreTab): BrowserStoreState {
    const prev = getState();
    const tabs = { ...prev.tabs };
    delete tabs[tab.browserId];
    const splitAttachments = { ...prev.splitAttachments };
    delete splitAttachments[tab.browserId];
    const activeBrowserIdBySpace = { ...prev.activeBrowserIdBySpace };
    for (const [spaceId, browserId] of Object.entries(activeBrowserIdBySpace)) {
      if (browserId === tab.browserId) activeBrowserIdBySpace[spaceId] = null;
    }
    return {
      ...prev,
      tabs,
      splitAttachments,
      activeBrowserIdBySpace,
      mru: prev.mru.filter((browserId) => browserId !== tab.browserId),
      miniBrowserId: prev.miniBrowserId === tab.browserId ? null : prev.miniBrowserId,
    };
  }

  const actions: BrowserStoreActions = {
    createSpace(input) {
      const name = input.name.trim();
      if (!name) return failure("invalid-name");
      const prev = getState();
      const space: BrowserSpace = {
        id: createId("space"),
        name,
        color: input.color?.trim() || DEFAULT_SPACE_COLOR,
        order: sortSpaces(prev.spaces).length,
      };
      setState((current) => ({ ...current, spaces: { ...current.spaces, [space.id]: space } }));
      return success(space);
    },

    renameSpace(spaceId, name) {
      const space = spaceById(spaceId);
      if (!space) return failure("unknown-space");
      const trimmed = name.trim();
      if (!trimmed) return failure("invalid-name");
      const next = { ...space, name: trimmed };
      setState((prev) => ({ ...prev, spaces: { ...prev.spaces, [spaceId]: next } }));
      return success(next);
    },

    recolorSpace(spaceId, color) {
      const space = spaceById(spaceId);
      if (!space) return failure("unknown-space");
      const next = { ...space, color: color.trim() || DEFAULT_SPACE_COLOR };
      setState((prev) => ({ ...prev, spaces: { ...prev.spaces, [spaceId]: next } }));
      return success(next);
    },

    deleteSpace(spaceId) {
      const prev = getState();
      const space = prev.spaces[spaceId];
      if (!space) return failure("unknown-space");
      const fallback = sortSpaces(prev.spaces).find((candidate) => candidate.id !== spaceId);
      if (!fallback) return failure("cannot-delete-last-space");

      const spaces = { ...prev.spaces };
      delete spaces[spaceId];
      const folders: Record<string, BrowserFolder> = {};
      for (const folder of Object.values(prev.folders)) {
        folders[folder.id] = folder.spaceId === spaceId ? { ...folder, spaceId: fallback.id } : folder;
      }
      const tabs: Record<string, BrowserStoreTab> = {};
      for (const tab of Object.values(prev.tabs)) {
        tabs[tab.browserId] = tab.spaceId === spaceId ? { ...tab, spaceId: fallback.id } : tab;
      }
      const activeBrowserIdBySpace = { ...prev.activeBrowserIdBySpace };
      delete activeBrowserIdBySpace[spaceId];
      commit({
        ...prev,
        spaces,
        folders,
        tabs,
        activeBrowserIdBySpace,
        activeSpaceId: prev.activeSpaceId === spaceId ? fallback.id : prev.activeSpaceId,
      });
      return success(space);
    },

    reorderSpaces(spaceId, toIndex) {
      const prev = getState();
      if (!prev.spaces[spaceId]) return failure("unknown-space");
      const ordered = sortSpaces(prev.spaces).filter((candidate) => candidate.id !== spaceId);
      ordered.splice(clampIndex(toIndex, ordered.length), 0, prev.spaces[spaceId]);
      const spaces: Record<string, BrowserSpace> = {};
      ordered.forEach((candidate, index) => {
        spaces[candidate.id] = { ...candidate, order: index };
      });
      setState((current) => ({ ...current, spaces }));
      return success(sortSpaces(spaces));
    },

    setActiveSpace(spaceId) {
      const space = spaceById(spaceId);
      if (!space) return failure("unknown-space");
      setState((prev) => ({ ...prev, activeSpaceId: spaceId }));
      return success(space);
    },

    upsertTab(input) {
      const prev = getState();
      const browserId = input.browserId?.trim();
      if (!browserId) return failure("unknown-tab");
      const existing = prev.tabs[browserId];
      const spaceId = input.spaceId ?? existing?.spaceId ?? prev.activeSpaceId;
      if (!prev.spaces[spaceId]) return failure("unknown-space");
      const section = input.section ?? existing?.section ?? "today";
      let folderId = input.folderId === null ? undefined : input.folderId ?? existing?.folderId;
      if (section !== "pinned") folderId = undefined;
      if (folderId) {
        const folder = prev.folders[folderId];
        if (!folder || folder.spaceId !== spaceId) return failure("unknown-folder");
      }
      if (section === "favorite" && existing?.section !== "favorite" && firstFavoriteCount() >= MAX_FAVORITES) {
        return failure("favorite-limit");
      }

      const tab: BrowserStoreTab = {
        browserId,
        spaceId,
        section,
        url: input.url ?? existing?.url ?? "about:blank",
        title: input.title === null ? "" : input.title ?? existing?.title ?? "",
        order: input.order ?? existing?.order ?? nextGroupOrder(spaceId, section, folderId),
        lastActiveAt: input.lastActiveAt ?? existing?.lastActiveAt ?? now(),
        private: input.private ?? existing?.private ?? false,
        agentControlled: input.agentControlled ?? existing?.agentControlled ?? false,
      };
      const faviconUrl = input.faviconUrl === null ? undefined : input.faviconUrl ?? existing?.faviconUrl;
      if (faviconUrl) tab.faviconUrl = faviconUrl;
      if (folderId) tab.folderId = folderId;
      const worktreeTag = input.worktreeTag === null ? undefined : input.worktreeTag ?? existing?.worktreeTag;
      if (worktreeTag) tab.worktreeTag = worktreeTag;
      const engine = input.engine ?? existing?.engine;
      if (engine) tab.engine = engine;

      setState((current) => ({ ...current, tabs: { ...current.tabs, [browserId]: tab } }));
      return success(tab);
    },

    closeTab(browserId) {
      const prev = getState();
      const tab = prev.tabs[browserId];
      if (!tab) return failure("unknown-tab");
      const next = removeTab(tab);
      const archived = !tab.private;
      commit({
        ...next,
        archive: archived ? [{ tab, archivedAt: now(), reason: "closed" }, ...next.archive] : next.archive,
      });
      return success({ tab, archived });
    },

    moveTab(browserId, target) {
      const prev = getState();
      const tab = prev.tabs[browserId];
      if (!tab) return failure("unknown-tab");
      const spaceId = target.spaceId ?? tab.spaceId;
      if (!prev.spaces[spaceId]) return failure("unknown-space");
      const section = target.section ?? tab.section;
      const sameGroup = spaceId === tab.spaceId && section === tab.section;
      let folderId = target.folderId === null ? undefined : target.folderId ?? (sameGroup ? tab.folderId : undefined);
      if (section !== "pinned") folderId = undefined;
      if (folderId) {
        const folder = prev.folders[folderId];
        if (!folder || folder.spaceId !== spaceId) return failure("unknown-folder");
      }
      if (section === "favorite" && tab.section !== "favorite" && firstFavoriteCount() >= MAX_FAVORITES) {
        return failure("favorite-limit");
      }

      const moved: BrowserStoreTab = { ...tab, spaceId, section };
      if (folderId) moved.folderId = folderId;
      else delete moved.folderId;
      const sourceKey = groupKey(tab.spaceId, tab.section, tab.folderId);
      const targetKey = groupKey(spaceId, section, folderId);

      // Build the target list (without the moved tab), seat it at the requested slot, renumber.
      const tabs = { ...prev.tabs };
      const targetOrder = sortTabs(
        Object.values(tabs).filter((candidate) => groupOf(candidate) === targetKey && candidate.browserId !== browserId),
      );
      targetOrder.splice(clampIndex(target.index ?? targetOrder.length, targetOrder.length), 0, moved);
      targetOrder.forEach((candidate, index) => {
        tabs[candidate.browserId] = { ...candidate, order: index };
      });
      if (sourceKey !== targetKey) {
        const sourceOrder = sortTabs(Object.values(tabs).filter((candidate) => groupOf(candidate) === sourceKey));
        sourceOrder.forEach((candidate, index) => {
          tabs[candidate.browserId] = { ...candidate, order: index };
        });
      }
      commit({ ...prev, tabs });
      return success(tabs[browserId]);
    },

    createFolder(input) {
      const prev = getState();
      const spaceId = input.spaceId ?? prev.activeSpaceId;
      if (!prev.spaces[spaceId]) return failure("unknown-space");
      const name = input.name.trim();
      if (!name) return failure("invalid-name");
      const parentId = input.parentId ?? undefined;
      if (parentId) {
        const parent = prev.folders[parentId];
        if (!parent || parent.spaceId !== spaceId) return failure("unknown-folder");
      }
      const folder: BrowserFolder = {
        id: createId("folder"),
        spaceId,
        name,
        order: sortFolders(prev.folders).filter((candidate) => candidate.spaceId === spaceId && candidate.parentId === parentId).length,
      };
      if (parentId) folder.parentId = parentId;
      setState((current) => ({ ...current, folders: { ...current.folders, [folder.id]: folder } }));
      return success(folder);
    },

    renameFolder(folderId, name) {
      const prev = getState();
      const folder = prev.folders[folderId];
      if (!folder) return failure("unknown-folder");
      const trimmed = name.trim();
      if (!trimmed) return failure("invalid-name");
      const next = { ...folder, name: trimmed };
      setState((current) => ({ ...current, folders: { ...current.folders, [folderId]: next } }));
      return success(next);
    },

    deleteFolder(folderId) {
      const prev = getState();
      const folder = prev.folders[folderId];
      if (!folder) return failure("unknown-folder");
      const folders: Record<string, BrowserFolder> = {};
      for (const candidate of Object.values(prev.folders)) {
        if (candidate.id === folderId) continue;
        folders[candidate.id] = candidate.parentId === folderId
          ? { ...candidate, parentId: folder.parentId }
          : candidate;
      }
      for (const candidate of Object.values(folders)) {
        if (candidate.parentId && !folders[candidate.parentId]) delete folders[candidate.id].parentId;
      }
      const tabs: Record<string, BrowserStoreTab> = {};
      for (const tab of Object.values(prev.tabs)) {
        if (tab.folderId !== folderId) {
          tabs[tab.browserId] = tab;
          continue;
        }
        const next: BrowserStoreTab = { ...tab };
        if (folder.parentId) next.folderId = folder.parentId;
        else delete next.folderId;
        tabs[tab.browserId] = next;
      }
      commit({ ...prev, folders, tabs });
      return success(folder);
    },

    clearToday(spaceId) {
      const prev = getState();
      const targetSpaceId = spaceId ?? prev.activeSpaceId;
      const doomed = tabsInGroup(prev, targetSpaceId, "today", undefined);
      if (doomed.length === 0) return 0;
      const stamp = now();
      const removedIds = new Set(doomed.map((tab) => tab.browserId));
      const tabs: Record<string, BrowserStoreTab> = {};
      for (const tab of Object.values(prev.tabs)) {
        if (!removedIds.has(tab.browserId)) tabs[tab.browserId] = tab;
      }
      const splitAttachments: Record<string, BrowserSplitAttachment> = {};
      for (const [browserId, attachment] of Object.entries(prev.splitAttachments)) {
        if (!removedIds.has(browserId)) splitAttachments[browserId] = attachment;
      }
      const activeBrowserIdBySpace: Record<string, string | null> = {};
      for (const [candidateSpaceId, browserId] of Object.entries(prev.activeBrowserIdBySpace)) {
        activeBrowserIdBySpace[candidateSpaceId] = browserId && removedIds.has(browserId) ? null : browserId;
      }
      commit({
        ...prev,
        tabs,
        splitAttachments,
        activeBrowserIdBySpace,
        mru: prev.mru.filter((browserId) => !removedIds.has(browserId)),
        miniBrowserId: prev.miniBrowserId && removedIds.has(prev.miniBrowserId) ? null : prev.miniBrowserId,
        archive: [
          ...doomed.map((tab) => ({ tab, archivedAt: stamp, reason: "cleared" as const })),
          ...prev.archive,
        ],
      });
      return doomed.length;
    },

    restoreFromArchive(browserId) {
      const prev = getState();
      const entryIndex = prev.archive.findIndex((entry) => entry.tab.browserId === browserId);
      if (entryIndex < 0) return failure("unknown-archive-entry");
      const entry = prev.archive[entryIndex];
      const spaceId = prev.spaces[entry.tab.spaceId] ? entry.tab.spaceId : prev.activeSpaceId;
      const archive = prev.archive.filter((_, index) => index !== entryIndex);
      const restored: BrowserStoreTab = {
        ...entry.tab,
        spaceId,
        section: "today",
        order: tabsInGroup(prev, spaceId, "today", undefined).length,
        lastActiveAt: now(),
      };
      delete restored.folderId;
      commit({
        ...prev,
        archive,
        tabs: { ...prev.tabs, [browserId]: restored },
        mru: [browserId, ...prev.mru.filter((id) => id !== browserId)].slice(0, MAX_MRU_ENTRIES),
        activeBrowserIdBySpace: { ...prev.activeBrowserIdBySpace, [spaceId]: browserId },
      });
      return success(restored);
    },

    touch(browserId) {
      const prev = getState();
      const tab = prev.tabs[browserId];
      if (!tab) return failure("unknown-tab");
      const touched = { ...tab, lastActiveAt: now() };
      setState((current) => ({
        ...current,
        tabs: { ...current.tabs, [browserId]: touched },
        mru: [browserId, ...current.mru.filter((id) => id !== browserId)].slice(0, MAX_MRU_ENTRIES),
        activeBrowserIdBySpace: { ...current.activeBrowserIdBySpace, [tab.spaceId]: browserId },
      }));
      return success(touched);
    },

    setMini(browserId) {
      if (browserId === null) {
        setState((prev) => (prev.miniBrowserId === null ? prev : { ...prev, miniBrowserId: null }));
        return success(null);
      }
      if (!getState().tabs[browserId]) return failure("unknown-tab");
      setState((prev) => (prev.miniBrowserId === browserId ? prev : { ...prev, miniBrowserId: browserId }));
      return success(browserId);
    },

    promoteMini(spaceId) {
      const prev = getState();
      const miniBrowserId = prev.miniBrowserId;
      if (!miniBrowserId) return failure("unknown-tab");
      const tab = prev.tabs[miniBrowserId];
      if (!tab) return failure("unknown-tab");
      const targetSpaceId = spaceId ?? prev.activeSpaceId;
      if (!prev.spaces[targetSpaceId]) return failure("unknown-space");
      const promoted: BrowserStoreTab = {
        ...tab,
        spaceId: targetSpaceId,
        section: "today",
        order: tabsInGroup(prev, targetSpaceId, "today", undefined).filter((t) => t.browserId !== miniBrowserId).length,
        lastActiveAt: now(),
      };
      delete promoted.folderId;
      commit({
        ...prev,
        tabs: { ...prev.tabs, [miniBrowserId]: promoted },
        miniBrowserId: null,
        mru: [miniBrowserId, ...prev.mru.filter((id) => id !== miniBrowserId)].slice(0, MAX_MRU_ENTRIES),
        activeBrowserIdBySpace: { ...prev.activeBrowserIdBySpace, [targetSpaceId]: miniBrowserId },
      });
      return success(promoted);
    },

    attachSplit(browserId, attachment) {
      const prev = getState();
      if (!prev.tabs[browserId]) return failure("unknown-tab");
      const next: BrowserSplitAttachment = {
        workspaceId: attachment.workspaceId,
        tabId: attachment.tabId,
        leafId: attachment.leafId,
      };
      setState((current) => ({ ...current, splitAttachments: { ...current.splitAttachments, [browserId]: next } }));
      return success(next);
    },

    detachSplit(browserId) {
      const prev = getState();
      if (!prev.splitAttachments[browserId]) return false;
      const splitAttachments = { ...prev.splitAttachments };
      delete splitAttachments[browserId];
      setState((current) => ({ ...current, splitAttachments }));
      return true;
    },

    detachAllForWorkspace(workspaceId) {
      const prev = getState();
      const splitAttachments: Record<string, BrowserSplitAttachment> = {};
      let removed = 0;
      for (const [browserId, attachment] of Object.entries(prev.splitAttachments)) {
        if (attachment.workspaceId === workspaceId) {
          removed += 1;
          continue;
        }
        splitAttachments[browserId] = attachment;
      }
      if (removed > 0) setState((current) => ({ ...current, splitAttachments }));
      return removed;
    },
  };

  return {
    ...actions,
    getState,
    subscribe,
    setState,
    reset() {
      commit(createDefaultBrowserState());
    },
  };
}

export function selectSpaceList(state: BrowserStoreState): BrowserSpace[] {
  return sortSpaces(state.spaces);
}

export function selectTabsInSection(
  state: BrowserStoreState,
  spaceId: string,
  section: BrowserSection,
  folderId?: string,
): BrowserStoreTab[] {
  return tabsInGroup(state, spaceId, section, folderId);
}

export function selectFavoriteTabs(state: BrowserStoreState): BrowserStoreTab[] {
  return sortTabs(Object.values(state.tabs).filter((tab) => tab.section === "favorite"));
}

export function selectPinnedTabs(state: BrowserStoreState, spaceId: string = state.activeSpaceId): BrowserStoreTab[] {
  return tabsInGroup(state, spaceId, "pinned", undefined);
}

export function selectTodayTabs(state: BrowserStoreState, spaceId: string = state.activeSpaceId): BrowserStoreTab[] {
  return tabsInGroup(state, spaceId, "today", undefined);
}

export function selectFolders(state: BrowserStoreState, spaceId: string = state.activeSpaceId): BrowserFolder[] {
  return sortFolders(state.folders).filter((folder) => folder.spaceId === spaceId);
}

export function selectMruTabs(state: BrowserStoreState): BrowserStoreTab[] {
  return state.mru.flatMap((browserId) => (state.tabs[browserId] ? [state.tabs[browserId]] : []));
}

export function selectActiveBrowserId(state: BrowserStoreState, spaceId: string = state.activeSpaceId): string | null {
  return state.activeBrowserIdBySpace[spaceId] ?? null;
}

/** The single global store every browser surface reads from. */
export const browserStore = createBrowserStore();

/**
 * Subscribes to the global store with a selector. Selectors must return a stable reference for
 * unchanged state (pick a tab, a list built by the exported selectors, or a primitive) - the
 * snapshot is cached per state object so React does not re-render on identity churn. The
 * selector lives in a ref, so an inline arrow function does not resubscribe on every render.
 */
export function useBrowserStore<T = BrowserStoreState>(
  selector: (state: BrowserStoreState) => T = (state) => state as unknown as T,
  store: BrowserStore = browserStore,
): T {
  const selectorRef = useRef(selector);
  selectorRef.current = selector;
  const cacheRef = useRef<{ state: BrowserStoreState; value: T } | null>(null);
  const getSnapshot = useCallback(() => {
    const current = store.getState();
    const cached = cacheRef.current;
    if (cached && cached.state === current) return cached.value;
    const value = selectorRef.current(current);
    cacheRef.current = { state: current, value };
    return value;
  }, [store]);
  return useSyncExternalStore(store.subscribe, getSnapshot, getSnapshot);
}

/**
 * Legacy sources are the persisted workspace session (`PersistedWorkspaceSession`) or a live
 * `WorkspaceState`; both expose the same layout shape (`tabs`, `layoutsByTabId[..].contentsByLeafId`).
 * Reading them structurally keeps this store decoupled from `workspaceStore.ts`.
 */
type LegacyCollection = {
  tabs: BrowserStoreTab[];
  splitAttachments: Record<string, BrowserSplitAttachment>;
};

type LegacyBrowserRecord = {
  browserId: string;
  url: string;
  title: string;
  worktreePath?: string;
  worktreeLabel?: string;
  engine?: BrowserEngine;
};

function readLegacyBrowser(raw: Record<string, unknown>): LegacyBrowserRecord | null {
  const nested = asRecord(raw.browser) ?? raw;
  const browserId = readString(nested.browserId);
  if (!browserId) return null;
  // Private profiles are never persisted; a legacy row that still carries one must not surface.
  if (isPrivateBrowserProfileId(readString(nested.profileId))) return null;
  const engine = isEngine(nested.engine) ? nested.engine : undefined;
  const worktreePath = readString(raw.worktreePath) ?? readString(nested.worktreePath);
  const worktreeLabel = readString(nested.worktreeLabel) ?? readString(raw.worktreeLabel);
  return {
    browserId,
    url: readString(nested.url) ?? "about:blank",
    title: readString(nested.title) ?? "",
    ...(worktreePath ? { worktreePath } : {}),
    ...(worktreeLabel ? { worktreeLabel } : {}),
    ...(engine ? { engine } : {}),
  };
}

function legacyWorkspaces(persisted: unknown): Array<{ workspaceId: string; workspace: Record<string, unknown> }> {
  const root = asRecord(persisted);
  if (!root) return [];
  const workspaces = asRecord(root.workspaces);
  if (!workspaces) return [{ workspaceId: readString(root.workspaceId) ?? "", workspace: root }];
  const result: Array<{ workspaceId: string; workspace: Record<string, unknown> }> = [];
  for (const [key, value] of Object.entries(workspaces)) {
    const workspace = asRecord(value);
    if (!workspace) continue;
    result.push({ workspaceId: readString(workspace.workspaceId) ?? key, workspace });
  }
  return result;
}

function legacyLayouts(
  workspace: Record<string, unknown>,
): Array<{ worktreePath: string | null; layout: Record<string, unknown> }> {
  const result: Array<{ worktreePath: string | null; layout: Record<string, unknown> }> = [];
  const layout = asRecord(workspace.layout);
  if (layout) result.push({ worktreePath: readString(workspace.activeWorktreePath) ?? null, layout });
  for (const [path, value] of Object.entries(asRecord(workspace.worktreeLayouts) ?? {})) {
    const worktreeLayout = asRecord(value);
    if (worktreeLayout) result.push({ worktreePath: path, layout: worktreeLayout });
  }
  return result;
}

function collectRawLegacyBrowsers(persisted: unknown): {
  tabs: Array<LegacyBrowserRecord & { workspaceId: string; worktreePath?: string }>;
  leaves: Array<{ workspaceId: string; tabId: string; leafId: string; browser: LegacyBrowserRecord }>;
} {
  const tabs: Array<LegacyBrowserRecord & { workspaceId: string; worktreePath?: string }> = [];
  const leaves: Array<{ workspaceId: string; tabId: string; leafId: string; browser: LegacyBrowserRecord }> = [];
  for (const { workspaceId, workspace } of legacyWorkspaces(persisted)) {
    for (const { worktreePath, layout } of legacyLayouts(workspace)) {
      for (const candidate of Array.isArray(layout.tabs) ? layout.tabs : []) {
        const record = asRecord(candidate);
        if (!record || record.kind !== "browser") continue;
        const browser = readLegacyBrowser(record);
        if (!browser) continue;
        const path = browser.worktreePath ?? worktreePath ?? undefined;
        tabs.push({ ...browser, workspaceId, ...(path ? { worktreePath: path } : {}) });
      }
      for (const [tabId, value] of Object.entries(asRecord(layout.layoutsByTabId) ?? {})) {
        const tabLayout = asRecord(value);
        const contents = asRecord(tabLayout?.contentsByLeafId);
        if (!contents) continue;
        for (const [leafId, content] of Object.entries(contents)) {
          const record = asRecord(content);
          if (!record || record.kind !== "browser") continue;
          const browser = readLegacyBrowser(record);
          if (!browser) continue;
          const path = browser.worktreePath ?? worktreePath ?? undefined;
          leaves.push({
            workspaceId,
            tabId,
            leafId,
            browser: { ...browser, ...(path ? { worktreePath: path } : {}) },
          });
        }
      }
    }
  }
  return { tabs, leaves };
}

/**
 * Pure, idempotent collection of legacy worktree browser tabs and browser leaves into Today of the
 * given Space. Tabs keep their `browserId`, so existing worktree splits point at the same session.
 */
export function collectLegacyBrowserTabs(
  persistedWorkspaceState: unknown,
  options: { spaceId?: string; now?: number } = {},
): LegacyCollection {
  const spaceId = options.spaceId ?? DEFAULT_SPACE_ID;
  const stamp = options.now ?? Date.now();
  const { tabs: rawTabs, leaves } = collectRawLegacyBrowsers(persistedWorkspaceState);
  const tabs: BrowserStoreTab[] = [];
  const seen = new Set<string>();
  const splitAttachments: Record<string, BrowserSplitAttachment> = {};

  const push = (browser: LegacyBrowserRecord & { workspaceId: string; worktreePath?: string }) => {
    if (seen.has(browser.browserId)) return;
    seen.add(browser.browserId);
    const tab: BrowserStoreTab = {
      browserId: browser.browserId,
      spaceId,
      url: browser.url,
      title: browser.title,
      section: "today",
      order: tabs.length,
      lastActiveAt: stamp,
    };
    if (browser.worktreePath) {
      tab.worktreeTag = {
        workspaceId: browser.workspaceId,
        worktreePath: browser.worktreePath,
        ...(browser.worktreeLabel ? { label: browser.worktreeLabel } : {}),
      };
    }
    if (browser.engine) tab.engine = browser.engine;
    tabs.push(tab);
  };

  for (const browser of rawTabs) push(browser);

  for (const leaf of leaves) {
    push({ ...leaf.browser, workspaceId: leaf.workspaceId });
    if (splitAttachments[leaf.browser.browserId]) continue;
    splitAttachments[leaf.browser.browserId] = {
      workspaceId: leaf.workspaceId,
      tabId: leaf.tabId,
      leafId: leaf.leafId,
    };
  }

  return { tabs, splitAttachments };
}

/**
 * Pure migration: the full store state a fresh install would get from persisted workspace layouts.
 * Running it twice over the same input yields deep-equal state.
 */
export function migrateLegacyBrowserTabs(
  persistedWorkspaceState: unknown,
  options: { now?: number; spaceId?: string } = {},
): BrowserStoreState {
  const spaceId = options.spaceId ?? DEFAULT_SPACE_ID;
  const base = createDefaultBrowserState(spaceId);
  const collected = collectLegacyBrowserTabs(persistedWorkspaceState, { spaceId, now: options.now });
  const tabs: Record<string, BrowserStoreTab> = {};
  for (const tab of collected.tabs) tabs[tab.browserId] = tab;
  return { ...base, tabs, splitAttachments: collected.splitAttachments };
}

export type BrowserStoreMigrationOptions = {
  persistedWorkspaceState?: unknown;
  store?: BrowserStore;
  storage?: Storage | null;
  now?: number;
};

/**
 * One-shot migration of persisted worktree browser tabs into the global store, guarded by
 * `ferryx.browser.store.migrated:v1`. Returns true only on the call that applied something, so a
 * second call is a no-op and the store keeps exactly the state the first call produced.
 */
export function runBrowserStoreMigrationOnce(options: BrowserStoreMigrationOptions = {}): boolean {
  const store = options.store ?? browserStore;
  const storage = options.storage === undefined ? defaultStorage() : options.storage;
  try {
    if (storage?.getItem(BROWSER_STORE_MIGRATION_KEY) === "1") return false;
  } catch {
    // Unreadable storage: fall through and migrate in memory.
  }
  if (options.persistedWorkspaceState === undefined || options.persistedWorkspaceState === null) return false;

  const collected = collectLegacyBrowserTabs(options.persistedWorkspaceState, {
    spaceId: store.getState().activeSpaceId,
    now: options.now,
  });
  if (collected.tabs.length === 0 && Object.keys(collected.splitAttachments).length === 0) return false;

  for (const tab of collected.tabs) store.upsertTab(tab);
  for (const [browserId, attachment] of Object.entries(collected.splitAttachments)) {
    store.attachSplit(browserId, attachment);
  }
  try {
    storage?.setItem(BROWSER_STORE_MIGRATION_KEY, "1");
  } catch {
    // Without the guard the migration still ran; a later call is harmless (upserts are keyed).
  }
  return true;
}
