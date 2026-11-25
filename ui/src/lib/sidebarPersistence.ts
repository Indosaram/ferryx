import { worktreeIdentity, type Worktree } from "./types";
import {
  ACTIVITY_MODE_STORAGE_KEY,
  getMigratedItem,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  sidebarWidthStorageKey,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
} from "./storageKeys";

export const DEFAULT_SIDEBAR_WIDTH = 236;
export const MIN_SIDEBAR_WIDTH = 220;
export const MAX_SIDEBAR_WIDTH = 420;

export type SidebarWidthRange = { readonly min: number; readonly max: number; readonly initial: number };

export const DEFAULT_SIDEBAR_RANGE: SidebarWidthRange = {
  min: MIN_SIDEBAR_WIDTH,
  max: MAX_SIDEBAR_WIDTH,
  initial: DEFAULT_SIDEBAR_WIDTH,
};

/** The sidebar that carries the Supervisor panel: 380px, resizable between 320 and 480. */
export const SUPERVISOR_SIDEBAR_RANGE: SidebarWidthRange = { min: 320, max: 480, initial: 380 };

/**
 * The four desktop Activity Bar modes, in rail order.
 *
 * The strings are the persisted values in `ferryx.activity.mode` AND the suffix of each per-mode
 * width key (`ferryx.sidebar.width.<mode>`), so renaming one orphans a user's stored width: they
 * are treated as a storage format, not internal identifiers.
 */
export const ACTIVITY_MODES = ["supervisor", "worktrees", "browser", "remoteDesktop"] as const;

export type ActivityMode = (typeof ACTIVITY_MODES)[number];

/**
 * The width each mode's drawer remembers, kept per mode because the panels differ: the supervisor
 * panel needs 320px before its sessions read well, while a browser sidebar of 380px would eat the
 * page. Worktrees keep the historical range, so an upgrading user sees no jump.
 */
export const SIDEBAR_RANGE_BY_MODE: Record<ActivityMode, SidebarWidthRange> = {
  supervisor: SUPERVISOR_SIDEBAR_RANGE,
  worktrees: DEFAULT_SIDEBAR_RANGE,
  browser: { min: 240, max: 420, initial: 300 },
  remoteDesktop: { min: 240, max: 400, initial: 280 },
};

export type WorktreeOrder = Record<string, string[]>;

/**
 * Marker that the `localStorage` worktree order was already handed to the server.
 *
 * The order is server-owned from that moment on: `persistWorktreeOrder` never writes
 * `SIDEBAR_WORKTREE_ORDER_STORAGE_KEY` again, so a stale copy can never fight the server order.
 */
export const LANE_LIFECYCLE_IMPORT_STORAGE_KEY = "ferryx.sidebar.lifecycleImported";

/** One row's lifecycle as the hub stores it. Shape mirrors `lanes::lifecycle::LaneLifecycle`. */
export type LaneLifecycleView = {
  worktreeKey: string;
  workspaceId: string;
  worktreePath: string;
  /** The stored state; a snooze past `snoozedUntil` still reads as stored. */
  state: LaneLifecycleState;
  /** What the sidebar renders: expired snooze reads `active`, a pin set reads `pinned`. */
  effectiveState: LaneLifecycleState;
  pinnedTabs: string[];
  snoozedUntil: string | null;
  position: number;
  revision: number;
  updatedAt: string;
  /**
   * The row's pull request, as the hub's last read-only `gh pr view` observed it, or `null` when
   * the row has none (or no sync has looked yet). Read-only: nothing here writes to GitHub.
   */
  pullRequest?: LanePullRequestView | null;
};

/** One observed pull request. Shape mirrors `lanes::pull_request::LanePullRequest`. */
export type LanePullRequestView = {
  /** The lane key the row was recorded under, not the row's worktree key. */
  laneKey: string;
  /** `owner/repository` the PR belongs to. */
  repo: string;
  prNumber: number;
  /** GitHub's own lifecycle, as last observed. */
  state: "open" | "closed" | "merged";
  url: string;
  title: string;
  headBranch: string;
  /** When this state was first observed (RFC 3339); unchanged while the state is unchanged. */
  observedAt: string;
};

export type LaneLifecycleState = "active" | "pinned" | "settled" | "snoozed" | "archived";

/** The lifecycle fields carried into desktop and published sidebar rows. */
export type SidebarLaneLifecycle = {
  readonly state: LaneLifecycleState;
  readonly pinnedTabs?: readonly string[];
  readonly pullRequest?: LanePullRequestView | null;
};

export type LaneLifecycleSection = { workspaceId: string; rows: LaneLifecycleView[] };

/** The server-owned model: sections by workspace, rows already in render order. */
export type LaneLifecycleModel = { sections: LaneLifecycleSection[]; revision: number };

/** Boundary guard for lifecycle frames; older/malformed siblings must not clear the store. */
export function isLaneLifecycleModel(value: unknown): value is LaneLifecycleModel {
  if (!value || typeof value !== "object") return false;
  const model = value as Partial<LaneLifecycleModel>;
  const states: readonly unknown[] = ["active", "pinned", "settled", "snoozed", "archived"];
  return Number.isSafeInteger(model.revision) && Array.isArray(model.sections) && model.sections.every((section) =>
    section && typeof section.workspaceId === "string" && Array.isArray(section.rows) && section.rows.every((row) => {
      if (!row || typeof row.worktreeKey !== "string" || !/^[wp]:[^/]*\/.+/.test(row.worktreeKey) ||
          typeof row.workspaceId !== "string" || row.workspaceId !== section.workspaceId || typeof row.worktreePath !== "string" ||
          !states.includes(row.state) || !states.includes(row.effectiveState) ||
          !Array.isArray(row.pinnedTabs) || !row.pinnedTabs.every((tab) => typeof tab === "string") ||
          !Number.isSafeInteger(row.revision) || !Number.isFinite(row.position) || typeof row.updatedAt !== "string") return false;
      const pr = row.pullRequest;
      return pr == null || (typeof pr.laneKey === "string" && typeof pr.repo === "string" && Number.isSafeInteger(pr.prNumber) &&
        ["open", "closed", "merged"].includes(pr.state) && typeof pr.url === "string" && typeof pr.title === "string" &&
        typeof pr.headBranch === "string" && typeof pr.observedAt === "string");
    }),
  );
}

export type LaneLifecycleWrite = {
  worktreeKey: string;
  workspaceId: string;
  path: string;
  action: "pin" | "settle" | "snooze" | "archive" | "activate";
  /** Required by the existing apply contract: `pin` alone defaults to unpin. */
  pinned?: boolean;
  until?: string | null;
  /** The undo guard: a write carrying a revision the server moved past is refused, not applied. */
  expectedRevision?: number;
};

/**
 * The lifecycle key of one sidebar row, mirroring `lanes::lifecycle::worktree_key` exactly.
 *
 * A managed worktree is keyed by its `orca/<wsId>/<slug>` identity, so the row keeps its
 * lifecycle when the checkout moves on disk; a project root, a plain folder or an unmanaged
 * worktree has no slug and is keyed by workspace + normalised path.
 */
export function laneLifecycleKey(row: {
  workspaceId: string;
  path: string;
  worktreeSlug?: string | null;
}): string {
  const slug = row.worktreeSlug?.trim();
  if (slug) return `w:${row.workspaceId}/${slug}`;
  return `p:${row.workspaceId}/${normalizeLanePath(row.path)}`;
}

/** One row identity for rendering, row actions, tab pins and drag persistence. */
export function worktreeLifecycleIdentity(row: Worktree, projectWorkspaceId: string | undefined) {
  const identity = worktreeIdentity(row);
  const workspaceId = identity?.wsId ?? row.workspaceId ?? projectWorkspaceId;
  if (!workspaceId) throw new Error(`Cannot identify lane without a workspace ID: ${row.path}`);
  return {
    workspaceId,
    worktreeKey: laneLifecycleKey({ workspaceId, path: row.path, worktreeSlug: identity?.slug }),
  };
}

/**
 * The path part of a lifecycle key, mirroring `normalize_path` in
 * `crates/ferryx-hub/src/lanes/lifecycle.rs` EXACTLY: forward slashes, empty segments dropped, no
 * leading or trailing separator, drive letter kept (`C:/code/ferryx` keeps its `C:`).
 *
 * The key is the row's identity and the sidebar looks its state up by exact string equality, so
 * this must agree with the server character for character: `/code/ferryx`, `code/ferryx` and
 * `C:\code\ferryx\` are one row, and none of them may produce `p:ferryx//code/ferryx` while the
 * server stores `p:ferryx/code/ferryx`.
 */
export function normalizeLanePath(path: string): string {
  return path
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .filter((segment) => segment.length > 0)
    .join("/");
}

/**
 * Folds one `SET_TAB_PINNED` into a worktree's pin set.
 *
 * The set is the per-worktree pin set the server stores (`pinned_tabs`), so pinning a tab is what
 * makes the owning row read `pinned`. Kept here, used by both the reducer (which paints it) and
 * the writer (which persists it), so the two cannot disagree about what a pin means.
 */
export function foldTabPin(current: readonly string[], tabId: string, pinned: boolean): string[] {
  return pinned
    ? [...new Set([...current, tabId])]
    : current.filter((id) => id !== tabId);
}

/** The transport the client uses to reach the server-owned order. Injected by the shell/tests. */
export type LaneLifecycleTransport = {
  read(): Promise<LaneLifecycleModel>;
  /** Promote legacy path rows using the authoritative worktree listing before rendering. */
  reconcileIdentities?(rows: readonly { worktreeKey: string; workspaceId: string; path: string }[]): Promise<LaneLifecycleModel>;
  write(request: LaneLifecycleWrite): Promise<LaneLifecycleView>;
  /** Replaces one row's folded pin set (`pinned_tabs`); an empty set drops it back to `active`. */
  writePinTabs(request: {
    worktreeKey: string;
    workspaceId: string;
    path: string;
    pinnedTabs: readonly string[];
  }): Promise<LaneLifecycleView>;
  /** The one-time import: `true` the first time, `false` when the server already imported. */
  importOrder(order: Record<string, string[]>): Promise<boolean>;
  /**
   * The server-owned section order for one workspace. Identifiers are whatever the sidebar's
   * drag handler has (`sidebar-worktree:<ws>:<path>` sortable ids or plain paths); the server
   * resolves each to a lifecycle key, preferring an exact key then a known worktree path.
   */
  reorder(workspaceId: string, identifiers: readonly string[]): Promise<void>;
};

/**
 * Same-origin by default: the hub serves this surface beside the page, mounted at
 * `/api/lanes/lifecycle` in `crates/ferryx-hub/src/http.rs` (sub-routes `/apply`, `/pin-tabs`,
 * `/import`, `/order`). A shell that is not served by the hub injects its own transport with
 * `setLaneLifecycleTransport`; until it does, `read` rejects and the sidebar keeps the imported
 * order, which is the same rows in the same order with no lifecycle state yet.
 */
export const LANE_LIFECYCLE_PATH = "/api/lanes/lifecycle";

let laneLifecycleTransport: LaneLifecycleTransport | null = null;

/** Installs the transport (desktop shell, remote web, tests). */
export function setLaneLifecycleTransport(transport: LaneLifecycleTransport | null): void {
  laneLifecycleTransport = transport;
}

async function httpJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${LANE_LIFECYCLE_PATH}${path}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
  });
  if (!response.ok) throw new Error(`lane lifecycle ${path}: ${response.status}`);
  return (await response.json()) as T;
}

export function laneLifecycleTransportOrDefault(): LaneLifecycleTransport {
  return laneLifecycleTransport ?? httpLaneLifecycleTransport();
}

/**
 * The transport a shell installed with `setLaneLifecycleTransport`, or null when none did.
 *
 * Only the desktop shell declares one (`installDesktopLaneLifecycleTransport`). Every other
 * surface - the hub-served page, a unit test - has no endpoint behind the same-origin path, so
 * asking it for the order would issue a request that cannot succeed and report it as an error to
 * the owner. Those surfaces keep the imported `localStorage` order instead.
 */
export function declaredLaneLifecycleTransport(): LaneLifecycleTransport | null {
  return laneLifecycleTransport;
}

/** The predecessor transport, also used when a daemon lacks lifecycle IPC support. */
export function httpLaneLifecycleTransport(): LaneLifecycleTransport {
  return {
    read: () => httpJson<LaneLifecycleModel>(""),
    reconcileIdentities: (rows) => httpJson<LaneLifecycleModel>("/identities", {
      method: "POST", body: JSON.stringify(rows),
    }),
    write: async (request) => (await httpJson<{ row: LaneLifecycleView }>("/apply", { method: "POST", body: JSON.stringify(request) })).row,
    writePinTabs: (request) =>
      httpJson<LaneLifecycleView>("/pin-tabs", { method: "POST", body: JSON.stringify(request) }),
    importOrder: async (order) => {
      const answer = await httpJson<{ imported: boolean }>("/import", { method: "POST", body: JSON.stringify({ order }) });
      return answer.imported;
    },
    reorder: async (workspaceId, identifiers) => {
      await httpJson<{ sections: unknown }>("/order", {
        method: "POST",
        body: JSON.stringify({ workspaceId, identifiers }),
      });
    },
  };
}

/** Rows the server has no lifecycle for read as `active`, so a read answer is never partial. */
export function lifecycleFor(
  model: LaneLifecycleModel | null | undefined,
  worktreeKey: string,
): LaneLifecycleView | undefined {
  if (!model) return undefined;
  for (const section of model.sections) {
    const found = section.rows.find((row) => row.worktreeKey === worktreeKey);
    if (found) return found;
  }
  return undefined;
}

/**
 * `workspaceId -> worktree keys` in the server's render order: the exact input
 * `buildSidebarModel` orders rows with. Absent workspaces keep their local order.
 */
export function laneLifecycleOrder(model: LaneLifecycleModel | null | undefined): Record<string, string[]> {
  if (!model) return {};
  const order: Record<string, string[]> = {};
  for (const section of model.sections) {
    order[section.workspaceId] = section.rows.map((row) => row.worktreeKey);
  }
  return order;
}

/**
 * The ONE-TIME import of the existing `localStorage` order into the server.
 *
 * Two guards, and either one alone is enough to skip:
 *  1. `LANE_LIFECYCLE_IMPORT_STORAGE_KEY` in this window's `localStorage`;
 *  2. the server's own `hub_meta` marker, which `importOrder` reports as `imported: false`.
 * After it succeeds the marker is written and `persistWorktreeOrder` stops writing localStorage,
 * so from then on the server order is the only order this window reads.
 */
export async function importLocalWorktreeOrderOnce(
  transport: LaneLifecycleTransport | null = declaredLaneLifecycleTransport(),
  storage: Pick<Storage, "getItem" | "setItem"> | null = typeof window !== "undefined" ? window.localStorage : null,
): Promise<boolean> {
  if (!transport) return false;
  if (localWorktreeOrderImported(storage)) return false;
  const local = loadWorktreeOrderRaw(storage);
  const imported = await transport.importOrder(local);
  storage?.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, imported ? "imported" : "server-had-it");
  return imported;
}

/** True once the localStorage order was handed to the server and must not be read again. */
export function localWorktreeOrderImported(
  storage: Pick<Storage, "getItem"> | null = typeof window !== "undefined" ? window.localStorage : null,
): boolean {
  const marker = storage?.getItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY);
  // Older clients wrote `empty` without contacting the hub; it proves no handover.
  return marker === "imported" || marker === "server-had-it";
}

const persistenceListeners = new Set<() => void>();

/** Fires after the sidebar persists its worktree order or collapsed set in this window. */
export function subscribeSidebarPersistence(listener: () => void): () => void {
  persistenceListeners.add(listener);
  return () => { persistenceListeners.delete(listener); };
}

function notifySidebarPersistence() {
  for (const listener of persistenceListeners) listener();
}

export function clampSidebarWidth(width: number, range: SidebarWidthRange = DEFAULT_SIDEBAR_RANGE) {
  return Math.min(range.max, Math.max(range.min, Math.round(width)));
}

/** Where the rail starts when nothing was stored, or when the stored value is not a mode. */
export const DEFAULT_ACTIVITY_MODE: ActivityMode = "worktrees";

export function loadActivityMode(): ActivityMode {
  try {
    const raw = getMigratedItem(ACTIVITY_MODE_STORAGE_KEY);
    return raw !== null && (ACTIVITY_MODES as readonly string[]).includes(raw)
      ? (raw as ActivityMode)
      : DEFAULT_ACTIVITY_MODE;
  } catch {
    return DEFAULT_ACTIVITY_MODE;
  }
}

export function saveActivityMode(mode: ActivityMode): void {
  try {
    window.localStorage.setItem(ACTIVITY_MODE_STORAGE_KEY, mode);
  } catch {
    // A storage failure should not break mode switching for this session.
  }
}

/**
 * Copies the pre-Activity-Bar single width into the two modes that existed before it.
 *
 * Idempotent by construction: a per-mode key that is already present is never overwritten, so the
 * first run seeds and every later run (a second window, a reload, a resize in between) is a no-op.
 * The shared key is LEFT IN PLACE because the sidebar's current single-width loader still reads it;
 * the per-mode keys take over when the Activity Bar is wired in, and the shared key becomes inert.
 */
function migrateLegacySidebarWidth(): void {
  const storage = typeof window !== "undefined" ? window.localStorage : null;
  if (!storage) return;
  const legacy = getMigratedItem(SIDEBAR_WIDTH_STORAGE_KEY, storage);
  const stored = Number(legacy);
  if (legacy === null || !Number.isFinite(stored) || stored <= 0) return;
  for (const mode of ["supervisor", "worktrees"] as const) {
    const key = sidebarWidthStorageKey(mode);
    if (storage.getItem(key) !== null) continue;
    storage.setItem(key, String(clampSidebarWidth(stored, SIDEBAR_RANGE_BY_MODE[mode])));
  }
}

/** The width this mode's drawer remembers, clamped to the mode's own range. */
export function loadSidebarWidthForMode(mode: ActivityMode): number {
  const range = SIDEBAR_RANGE_BY_MODE[mode];
  try {
    migrateLegacySidebarWidth();
    const raw = window.localStorage.getItem(sidebarWidthStorageKey(mode));
    const stored = Number(raw);
    return raw !== null && Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored, range) : range.initial;
  } catch {
    return range.initial;
  }
}

export function saveSidebarWidthForMode(mode: ActivityMode, width: number): void {
  try {
    window.localStorage.setItem(sidebarWidthStorageKey(mode), String(clampSidebarWidth(width, SIDEBAR_RANGE_BY_MODE[mode])));
  } catch {
    // A storage failure should not break pointer resizing for this session.
  }
}

export function loadWorktreeOrder(): WorktreeOrder {
  return loadWorktreeOrderRaw(typeof window !== "undefined" ? window.localStorage : null);
}

/** The stored order as written before the server owned it. Only the import reads it. */
function loadWorktreeOrderRaw(
  storage: Pick<Storage, "getItem"> | null,
): WorktreeOrder {
  try {
    const raw = storage && typeof window !== "undefined" && storage === window.localStorage
      ? getMigratedItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY)
      : storage?.getItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY) ?? null;
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).flatMap(([workspaceId, paths]) => {
        if (!Array.isArray(paths)) return [];
        const uniquePaths = [...new Set(paths.filter((path): path is string => typeof path === "string"))];
        return uniquePaths.length > 0 ? [[workspaceId, uniquePaths]] : [];
      }),
    );
  } catch {
    return {};
  }
}

/**
 * Writes the server-owned order for one workspace after a drag.
 *
 * `keys` are lifecycle keys (`lanes::lifecycle::worktree_key`), which is the only identifier the
 * server can join a rendered row to; a path or a sortable id would be a second, drifting key.
 * Rejects on failure so the caller can keep the optimistic order visible and report the error.
 */
export async function persistLaneOrder(
  workspaceId: string,
  keys: readonly string[],
  transport: LaneLifecycleTransport | null = declaredLaneLifecycleTransport(),
): Promise<void> {
  // No declared transport means no server owns the order yet: the drag stays local.
  if (!transport) return;
  await transport.reorder(workspaceId, keys);
}

/**
 * Persists a new worktree order locally. It is a no-op once the one-time import landed: the
 * order is server-owned from then on (`persistLaneOrder`), and a second local copy is exactly the
 * fight this transition exists to end.
 */
export function persistWorktreeOrder(order: WorktreeOrder) {
  if (localWorktreeOrderImported()) return;
  try {
    window.localStorage.setItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY, JSON.stringify(order));
  } catch {
    // A storage failure should not break drag reordering for this session.
  }
  notifySidebarPersistence();
}

/** Returns `null` when the accordion has never been persisted, so callers can apply defaults. */
export function loadCollapsedProjects(): Set<string> | null {
  try {
    const raw = getMigratedItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return null;
    return new Set(parsed.filter((id): id is string => typeof id === "string"));
  } catch {
    return null;
  }
}

export function persistCollapsedProjects(collapsed: Set<string>) {
  try {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([...collapsed]));
  } catch {
    // A storage failure should not break accordion toggling for this session.
  }
  notifySidebarPersistence();
}

export function loadSidebarWidth(range: SidebarWidthRange = DEFAULT_SIDEBAR_RANGE) {
  try {
    const raw = getMigratedItem(SIDEBAR_WIDTH_STORAGE_KEY);
    const stored = Number(raw);
    return Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored, range) : range.initial;
  } catch {
    return range.initial;
  }
}

export function persistSidebarWidth(width: number, range: SidebarWidthRange = DEFAULT_SIDEBAR_RANGE) {
  try {
    window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(clampSidebarWidth(width, range)));
  } catch {
    // A storage failure should not break pointer resizing for this session.
  }
}
