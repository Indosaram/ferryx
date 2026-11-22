import {
  createTerminalPaneContent,
  isTerminalTab,
  type LayoutState,
  type PaneContent,
  type PersistedTab,
  type TabPaneLayout,
  type TerminalSession,
  type WorkspaceTab,
} from "../lib/types";
import { normalizeLayout } from "./layout";
import { clampRatio, collectLeafIds, type PaneDirection } from "./paneTree";

/**
 * Wire document shared between a host workspace and its paired clients.
 * Camel case matches the Rust `SharedLayout`; view state (active tab, focus, scroll,
 * split mode) is deliberately absent - each window keeps its own.
 */
export type SharedPane =
  | { type: "leaf"; leafId: string; backendSessionId: string }
  | {
      type: "split";
      direction: PaneDirection;
      ratio: number;
      first: SharedPane;
      second: SharedPane;
    };

export type SharedTab = {
  id: string;
  kind: string;
  label: string;
  pinned: boolean;
  /** `null` for non-terminal tabs: only terminal tabs carry a pane tree. */
  paneTree: SharedPane | null;
};

export type SharedLayout = {
  workspaceId: string;
  revision: number;
  tabs: SharedTab[];
};

/** Minimal structural session shape the converter needs; full `TerminalSession` satisfies it. */
export type LayoutSyncSession = {
  readonly id: string;
  readonly backendSessionId: string | null;
};

export type LayoutSyncSessions = Readonly<Record<string, LayoutSyncSession | undefined>>;

export const EMPTY_SHARED_LAYOUT = (workspaceId: string): SharedLayout => ({
  workspaceId,
  revision: 0,
  tabs: [],
});

/** u64 revisions arrive as JSON numbers from Tauri and as decimal strings from the machine API. */
export function toRevision(value: unknown): number | null {
  if (typeof value === "number" && Number.isSafeInteger(value) && value >= 0) return value;
  if (typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value)) {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) ? parsed : null;
  }
  return null;
}

/**
 * Strips view state from a workspace layout: leaf kind/view, active/focused leaf, tab
 * groups and the active tab all stay local. A leaf becomes the backend session it shows.
 */
export function toSharedLayout(
  workspaceLayout: LayoutState,
  sessions: LayoutSyncSessions,
  backendForLeaf?: (frontendSessionId: string) => string | null,
): SharedTab[] {
  const resolveBackend = (frontendSessionId: string): string => {
    const explicit = backendForLeaf?.(frontendSessionId);
    if (typeof explicit === "string") return explicit;
    return sessions[frontendSessionId]?.backendSessionId ?? "";
  };
  return workspaceLayout.tabs.map((tab): SharedTab => {
    const base = {
      id: tab.id,
      kind: tab.kind ?? "terminal",
      label: tab.label,
      pinned: tab.pinned === true,
    };
    if (!isTerminalTab(tab)) return { ...base, paneTree: null };
    const tabLayout = workspaceLayout.layoutsByTabId?.[tab.id];
    if (!tabLayout) return { ...base, paneTree: null };
    return { ...base, paneTree: toSharedPane(tabLayout.root, tabLayout.sessionIdsByLeafId, resolveBackend) };
  });
}

function toSharedPane(
  node: TabPaneLayout["root"],
  sessionIdsByLeafId: Record<string, string>,
  resolveBackend: (frontendSessionId: string) => string,
): SharedPane {
  if (node.type === "leaf") {
    const frontendSessionId = sessionIdsByLeafId[node.leafId] ?? "";
    return {
      type: "leaf",
      leafId: node.leafId,
      backendSessionId: frontendSessionId ? resolveBackend(frontendSessionId) : "",
    };
  }
  return {
    type: "split",
    direction: node.direction,
    ratio: clampRatio(node.ratio),
    first: toSharedPane(node.first, sessionIdsByLeafId, resolveBackend),
    second: toSharedPane(node.second, sessionIdsByLeafId, resolveBackend),
  };
}

export type ApplySharedLayoutResult = {
  layout: LayoutState;
  /** Leaves whose backend session has no local proxy yet; the caller attaches them. */
  unattachedBackendSessionIds: string[];
};

export type ResolveLayoutSession = (backendSessionId: string) => string | null;

/**
 * Rebuilds local tabs and pane trees from a shared document. Leaves keep the frontend
 * session (and content object) that already shows the same backend session, so applying an
 * unrelated remote change never rewires live panes. `activeTabId` stays local when its tab
 * still exists.
 */
export function applySharedLayout(
  currentLayout: LayoutState,
  doc: SharedLayout,
  resolveSession: ResolveLayoutSession,
): ApplySharedLayoutResult {
  const reuse = indexLocalLeaves(currentLayout);
  const unattached = new Set<string>();
  const existingTabs = new Map(currentLayout.tabs.map((tab) => [tab.id, tab]));
  const tabs: WorkspaceTab[] = [];
  const layoutsByTabId: Record<string, TabPaneLayout> = {};

  for (const sharedTab of doc.tabs) {
    const existing = existingTabs.get(sharedTab.id);
    if (!sharedTab.paneTree) {
      // Non-terminal tabs carry no pane tree, so only a tab already present locally can
      // be rebuilt: browser/file panes own local objects the document cannot describe.
      if (!existing || isTerminalTab(existing)) continue;
      const existingLayout = currentLayout.layoutsByTabId?.[sharedTab.id];
      tabs.push({ ...existing, label: sharedTab.label || existing.label, pinned: sharedTab.pinned || undefined });
      if (existingLayout) layoutsByTabId[sharedTab.id] = existingLayout;
      continue;
    }
    const tab = rebuildTab(existing, sharedTab);
    tabs.push(tab);
    layoutsByTabId[sharedTab.id] = rebuildPaneLayout(sharedTab.paneTree, reuse, resolveSession, unattached,
      currentLayout.layoutsByTabId?.[sharedTab.id]);
  }

  const activeTabId = currentLayout.activeTabId && tabs.some((tab) => tab.id === currentLayout.activeTabId)
    ? currentLayout.activeTabId
    : tabs[0]?.id ?? null;

  // Tab groups are view state too, so they stay local: normalizeLayout prunes any group entry
  // whose tab left the document, and a workspace that never had groups stays on its default.
  const layout = normalizeLayout({
    tabs,
    activeTabId,
    layoutsByTabId,
    returnTabIdByTabId: currentLayout.returnTabIdByTabId,
    tabGroups: currentLayout.tabGroups,
    tabGroupLayout: currentLayout.tabGroupLayout,
    focusedGroupId: currentLayout.focusedGroupId,
  });

  return { layout, unattachedBackendSessionIds: [...unattached] };
}

/** The part of a workspace state the converters read and write; `WorkspaceState` satisfies it. */
export type LayoutSyncState = {
  workspaceId?: string;
  sessions: Record<string, TerminalSession>;
  layout: LayoutState;
};

/**
 * Frontend session minted for a leaf this window has never shown. It carries only the host
 * backend identity, so the ordinary attach/retry admission adopts it - the pane is never
 * created by the sync path and no host PTY is ever spawned from it.
 */
export function layoutSyncSessionId(backendSessionId: string): string {
  return `layout-sync:${backendSessionId}`;
}

/** A pane for a leaf this window has never shown; the caller fills in the host's details. */
export function detachedLayoutSession(workspaceId: string, backendSessionId: string): TerminalSession {
  return {
    id: layoutSyncSessionId(backendSessionId),
    cwd: "",
    workspaceId,
    worktree: null,
    backendSessionId,
    lifecycle: "starting",
    processState: "standby",
  };
}

function findFrontendSessionId(
  sessions: Record<string, TerminalSession>,
  backendSessionId: string,
): string | null {
  for (const session of Object.values(sessions)) {
    if (session.backendSessionId === backendSessionId) return session.id;
  }
  return null;
}

/**
 * Applies a document to a whole workspace state. A leaf whose backend session this window has
 * no record of mints a detached one, so the layout can always be rebuilt; nothing is spawned.
 */
export function applySharedLayoutToState<T extends LayoutSyncState>(
  state: T,
  doc: SharedLayout,
): { state: T; mintedSessions: TerminalSession[]; unattachedBackendSessionIds: string[] } {
  const workspaceId = state.workspaceId ?? doc.workspaceId;
  const minted = new Map<string, TerminalSession>();
  const resolveSession = (backendSessionId: string): string | null => {
    const known = findFrontendSessionId(state.sessions, backendSessionId);
    if (known) return known;
    const session = detachedLayoutSession(workspaceId, backendSessionId);
    minted.set(session.id, session);
    return session.id;
  };
  const { layout, unattachedBackendSessionIds } = applySharedLayout(state.layout, doc, resolveSession);
  const sessions = minted.size === 0
    ? state.sessions
    : { ...state.sessions, ...Object.fromEntries(minted) };
  return {
    state: { ...state, layout, sessions } as T,
    mintedSessions: [...minted.values()],
    unattachedBackendSessionIds,
  };
}

type ReusableLeaf = { sessionId: string; content?: PaneContent };

/** Existing local leaves keyed by frontend session id: a remote leaf that resolves to the
 * same frontend session keeps the live content object instead of minting a new pane. */
function indexLocalLeaves(layout: LayoutState): Map<string, ReusableLeaf> {
  const index = new Map<string, ReusableLeaf>();
  for (const tabLayout of Object.values(layout.layoutsByTabId ?? {})) {
    if (!tabLayout) continue;
    for (const [leafId, frontendSessionId] of Object.entries(tabLayout.sessionIdsByLeafId ?? {})) {
      if (!frontendSessionId || index.has(frontendSessionId)) continue;
      index.set(frontendSessionId, { sessionId: frontendSessionId, content: tabLayout.contentsByLeafId?.[leafId] });
    }
  }
  return index;
}

function rebuildTab(existing: WorkspaceTab | undefined, sharedTab: SharedTab): WorkspaceTab {
  const label = sharedTab.label || existing?.label || sharedTab.id;
  const pinned = sharedTab.pinned ? true : undefined;
  if (existing && isTerminalTab(existing)) {
    return { ...existing, label, pinned };
  }
  return { kind: "terminal", id: sharedTab.id, label, sessionId: "", pinned };
}

function rebuildPaneLayout(
  paneTree: SharedPane,
  reuse: Map<string, ReusableLeaf>,
  resolveSession: ResolveLayoutSession,
  unattached: Set<string>,
  previous: TabPaneLayout | undefined,
): TabPaneLayout {
  const sessionIdsByLeafId: Record<string, string> = {};
  const contentsByLeafId: Record<string, PaneContent> = {};

  const visit = (node: SharedPane): TabPaneLayout["root"] => {
    if (node.type === "leaf") {
      const backendSessionId = node.backendSessionId ?? "";
      let frontendSessionId = backendSessionId ? resolveSession(backendSessionId) : null;
      if (!frontendSessionId && backendSessionId) unattached.add(backendSessionId);
      const reusable = frontendSessionId ? reuse.get(frontendSessionId) : undefined;
      const sessionId = frontendSessionId ?? `shared-session:${backendSessionId || node.leafId}`;
      sessionIdsByLeafId[node.leafId] = sessionId;
      contentsByLeafId[node.leafId] = reusable?.content ?? createTerminalPaneContent(sessionId);
      return { type: "leaf", leafId: node.leafId };
    }
    return {
      type: "split",
      direction: node.direction,
      ratio: clampRatio(node.ratio),
      first: visit(node.first),
      second: visit(node.second),
    };
  };

  const root = visit(paneTree);
  const leaves = collectLeafIds(root);
  const activeLeafId = previous?.activeLeafId && leaves.includes(previous.activeLeafId)
    ? previous.activeLeafId
    : leaves[0] ?? null;
  return {
    root,
    activeLeafId,
    expandedLeafId: null,
    sessionIdsByLeafId,
    contentsByLeafId,
  };
}

/** Strict-ish decoder for a `SharedLayout` payload; rejects anything the UI cannot rebuild. */
export function decodeSharedLayout(value: unknown): SharedLayout {
  const record = asRecord(value, "layout");
  const workspaceId = typeof record.workspaceId === "string" && record.workspaceId
    ? record.workspaceId
    : invalid("layout.workspaceId");
  const revision = toRevision(record.revision);
  if (revision === null) invalid("layout.revision");
  if (!Array.isArray(record.tabs)) invalid("layout.tabs");
  const tabs = record.tabs.map((raw, index): SharedTab => {
    const tab = asRecord(raw, `layout.tabs[${index}]`);
    if (typeof tab.id !== "string" || !tab.id) invalid("layout.tab.id");
    if (typeof tab.label !== "string") invalid("layout.tab.label");
    if (typeof tab.pinned !== "boolean") invalid("layout.tab.pinned");
    return {
      id: tab.id,
      kind: typeof tab.kind === "string" ? tab.kind : "terminal",
      label: tab.label,
      pinned: tab.pinned,
      paneTree: tab.paneTree === null || tab.paneTree === undefined ? null : decodeSharedPane(tab.paneTree),
    };
  });
  return { workspaceId, revision: revision as number, tabs };
}

function decodeSharedPane(value: unknown): SharedPane {
  const node = asRecord(value, "pane");
  if (node.type === "leaf") {
    if (typeof node.leafId !== "string" || !node.leafId) invalid("pane.leafId");
    if (typeof node.backendSessionId !== "string") invalid("pane.backendSessionId");
    return { type: "leaf", leafId: node.leafId, backendSessionId: node.backendSessionId };
  }
  if (node.type === "split") {
    if (node.direction !== "horizontal" && node.direction !== "vertical") invalid("pane.direction");
    const ratio = typeof node.ratio === "number" && Number.isFinite(node.ratio) ? node.ratio : invalid("pane.ratio");
    return {
      type: "split",
      direction: node.direction,
      ratio,
      first: decodeSharedPane(node.first),
      second: decodeSharedPane(node.second),
    };
  }
  return invalid("pane.type");
}

function asRecord(value: unknown, what: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return invalid(what);
  return value as Record<string, unknown>;
}

function invalid(what: string): never {
  throw new Error(`INVALID_SHARED_LAYOUT:${what}`);
}
/** Pause after a poll that produced no document, so a broken transport cannot spin. */
const POLL_RETRY_MS = 1_000;

/** Transport the loop drives: the Tauri commands for a host workspace, the paired adapter otherwise. */
export type LayoutSyncTransport = {
  get: (afterRevision: number | null) => Promise<SharedLayout | null>;
  put: (baseRevision: number, tabs: SharedTab[]) => Promise<{ layout: SharedLayout; conflict: boolean }>;
};

/** The persisted shape of one workspace, as far as layout publishing needs it. */
export type PersistedLayoutSource = {
  workspaceId: string;
  target?: { kind: string } | null;
  layout: { tabs: ReadonlyArray<PersistedTab>; layoutsByTabId?: LayoutState["layoutsByTabId"] };
  terminalSessions: Record<string, { backendSessionId?: string | null; lastBackendSessionId?: string | null } | undefined>;
};

/**
 * Seeds the daemon with the layout of every local workspace the window is not showing. Only a
 * workspace the daemon holds at revision 0 is written: a newer document may have come from a
 * paired client, and it stays authoritative until this window opens that workspace and syncs it.
 * Paired workspaces and `skipWorkspaceId` (the active one, which has its own loop) are left alone.
 * Returns the workspace ids that were published.
 */
export async function publishPersistedLayouts(
  workspaces: Iterable<PersistedLayoutSource>,
  transportFor: (workspaceId: string) => LayoutSyncTransport,
  skipWorkspaceId: string | null,
): Promise<string[]> {
  const published: string[] = [];
  for (const workspace of workspaces) {
    if (workspace.workspaceId === skipWorkspaceId) continue;
    if (workspace.target?.kind === "pairedDaemon") continue;
    const transport = transportFor(workspace.workspaceId);
    const current = await transport.get(null);
    if (!current || current.revision !== 0) continue;
    const sessions: Record<string, LayoutSyncSession> = {};
    for (const [id, session] of Object.entries(workspace.terminalSessions)) {
      sessions[id] = { id, backendSessionId: session?.backendSessionId ?? session?.lastBackendSessionId ?? null };
    }
    const tabs = workspace.layout.tabs.map((tab): SharedTab => {
      const base = { id: tab.id, kind: tab.kind ?? "terminal", label: tab.label, pinned: tab.pinned === true };
      if (base.kind !== "terminal") return { ...base, paneTree: null };
      const legacy = workspace.layout.layoutsByTabId?.[tab.id];
      const root = tab.terminal?.paneTree ?? legacy?.root;
      const sessionIds = tab.terminal?.sessionIdsByLeafId ?? legacy?.sessionIdsByLeafId;
      return {
        ...base,
        paneTree: root && sessionIds
          ? toSharedPane(root, sessionIds, (id) => sessions[id]?.backendSessionId ?? "")
          : null,
      };
    });
    if (tabs.length === 0) continue;
    const result = await transport.put(0, tabs);
    if (!result.conflict) published.push(workspace.workspaceId);
  }
  return published;
}

export type LayoutSyncLoopOptions = {
  transport: LayoutSyncTransport;
  /** Host-local workspaces publish first; a paired client never publishes over revision 0. */
  isPaired: boolean;
  /** Reads the current local document (already stripped of view state). */
  readLocal: () => SharedTab[];
  /** Applies a remote document to the local layout. */
  applyRemote: (doc: SharedLayout) => void;
  onError?: (error: unknown) => void;
  debounceMs?: number;
};

export type LayoutSyncLoop = {
  /** Long-polls for remote documents and publishes the host's layout when it owns revision 0. */
  start: () => Promise<void>;
  stop: () => void;
  /** A local layout change: publish it once the debounce elapses. */
  localChanged: () => void;
  /** Publishes now; resolves once this publish settled. */
  flush: () => Promise<void>;
  revision: () => number;
};

/**
 * Keeps one document in step with the local layout. Local changes are published against the
 * last revision seen; every publish and poll updates that revision, and a document this loop
 * itself produced is never re-applied to the layout it came from.
 *
 * A failed poll or publish is reported and the loop keeps going: a transient host or transport
 * failure must not silently stop syncing for the rest of the session.
 */
export function createLayoutSyncLoop({
  transport,
  isPaired,
  readLocal,
  applyRemote,
  onError,
  debounceMs = 300,
}: LayoutSyncLoopOptions): LayoutSyncLoop {
  let revision = 0;
  let discovered = false;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let publishing: Promise<void> | null = null;
  // A local change that still has to be published.
  let pending = false;
  let publishedRevision: number | null = null;
  // Tabs of the document the daemon last handed us or that we last sent it. Publishing the
  // very same document again would only bump the revision and wake every other window.
  let knownTabs: string | null = null;
  // A pending back-off after an empty poll. Its timer is separate from the publish debounce so a
  // local change can never cancel the pause and strand the poll loop.
  let paused: (() => void) | null = null;

  // A host that is down would otherwise report the same failure every second.
  let failing = false;
  function fail(error: unknown) {
    if (failing) return;
    failing = true;
    onError?.(error);
  }
  function healthy() {
    failing = false;
  }

  function clearTimer() {
    if (timer === null) return;
    clearTimeout(timer);
    timer = null;
  }

  function record(doc: SharedLayout) {
    revision = doc.revision;
    discovered = true;
    knownTabs = JSON.stringify(doc.tabs);
  }

  /** `null` when the local layout could not be read: publishing then would blank the document. */
  function localDocument(): SharedTab[] | null {
    try {
      return readLocal();
    } catch (error) {
      fail(error);
      return null;
    }
  }

  async function publishNow(): Promise<void> {
    if (stopped) return;
    // A paired client has nothing to publish over: the host owns the first document.
    if (isPaired && revision === 0) return;
    const tabs = localDocument();
    if (tabs === null) return;
    // The daemon already holds exactly this document. Revision 0 is always published once: it
    // is the document a paired client waits for before it can show anything at all.
    if (revision > 0 && JSON.stringify(tabs) === knownTabs) return;
    const baseRevision = revision;
    let result: { layout: SharedLayout; conflict: boolean };
    try {
      result = await transport.put(baseRevision, tabs);
    } catch (error) {
      fail(error);
      throw error;
    }
    if (stopped) return;
    if (result.conflict) {
      // The local change was computed against a document that no longer exists. Accept the
      // host's document instead of re-putting: the other writer's change stays authoritative.
      record(result.layout);
      applyRemote(result.layout);
      return;
    }
    healthy();
    publishedRevision = result.layout.revision;
    record(result.layout);
  }

  function flush(): Promise<void> {
    clearTimer();
    if (!pending) return publishing ?? Promise.resolve();
    pending = false;
    if (!publishing) {
      publishing = publishNow().finally(() => {
        publishing = null;
      });
    }
    return publishing;
  }

  function localChanged() {
    if (stopped) return;
    pending = true;
    clearTimer();
    timer = setTimeout(() => {
      timer = null;
      void flush().catch(() => {});
    }, debounceMs);
  }

  /** One long poll: resolves once a document arrived, the poll ended, or the loop stopped. */
  async function pollOnce(): Promise<SharedLayout | null> {
    const after = discovered ? revision : null;
    let doc: SharedLayout | null;
    try {
      doc = await transport.get(after);
    } catch (error) {
      if (!stopped) fail(error);
      return null;
    }
    if (stopped) return null;
    if (!doc) return null;
    // Any successful read proves the transport works, even when the document is unchanged.
    healthy();
    discovered = true;
    if (doc.revision === revision) return null;
    // Layouts live in daemon memory only, so a restart or a handover hands back a lower
    // revision - usually 0. Any revision that differs from the last one seen is authoritative,
    // a lower one included: keeping the old number would leave the loop waiting forever on a
    // store that only ever holds revision 0.
    const regressed = doc.revision < revision;
    if (regressed) publishedRevision = null;
    const own = doc.revision === publishedRevision;
    if (own) publishedRevision = null;
    record(doc);
    if (regressed && doc.revision === 0) {
      // A fresh daemon forgot every layout. The host owns the document and rewrites it here; a
      // paired client must not publish over revision 0, and applying the empty document would
      // only blank its view until the host's publish arrives.
      if (!isPaired) await publishNow().catch(() => {});
      return doc;
    }
    if (!own) applyRemote(doc);
    return doc;
  }

  function pause(): Promise<void> {
    return new Promise<void>((resolve) => {
      const end = () => {
        if (paused === end) paused = null;
        clearTimeout(pauseTimer);
        resolve();
      };
      const pauseTimer = setTimeout(end, POLL_RETRY_MS);
      paused = end;
    });
  }

  async function start(): Promise<void> {
    if (stopped) return;
    if (!discovered) {
      // Learn the current revision before deciding who publishes the first document.
      await pollOnce();
      if (stopped) return;
      // The host owns the empty document and publishes it; a paired client never does.
      if (!isPaired && revision === 0) {
        await publishNow().catch(() => {});
        if (stopped) return;
      }
    }
    while (!stopped) {
      const doc = await pollOnce();
      if (stopped) return;
      // A poll that returned nothing either failed or timed out host-side; either way a
      // bounded pause keeps a broken transport from spinning.
      if (!doc) await pause();
    }
  }


  return {
    start,
    stop() {
      stopped = true;
      clearTimer();
      paused?.();
    },
    localChanged,
    flush,
    revision: () => revision,
  };
}
