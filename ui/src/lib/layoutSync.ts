import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createLayoutState, normalizeLayout } from "../state/layout";
import { resolveSeam } from "../state/paneTree";
import type { PaneNode } from "../state/paneTree";
import type { WorkspaceState } from "../state/workspaceStore";
import type { LayoutState, PaneContent, TabGroupLayoutNode, WorkspaceTab } from "./types";
import { createBrowserPaneContent } from "./types";
import type { WorkspaceAction } from "../state/workspaceStore";
import { currentWindowLabel } from "./windowIdentity";

type Direction = "horizontal" | "vertical";
export type DaemonContent =
  | { kind: "terminal"; backendSessionId: string }
  | ({ kind: "file"; path: string; line: number | null; col: number | null } & Partial<Extract<PaneContent, { kind: "file" }>>)
  | { kind: "browser"; url: string; browserId?: string; worktreePath?: string; profileId?: string }
  | { kind: "dag" } | ({ kind: "changes" } & Partial<Extract<PaneContent, { kind: "changes" }>>)
  | { kind: "remoteDesktop"; hostId: string };
export type DaemonNode =
  | { type: "leaf"; nodeId: number; content: DaemonContent }
  | { type: "split"; dir: Direction; ratio: number; a: DaemonNode; b: DaemonNode };
type DaemonGroup =
  | { type: "group"; groupId: string; tabIds: string[]; activeTabId: string | null }
  | { type: "split"; dir: Direction; ratio: number; a: DaemonGroup; b: DaemonGroup };
export type DaemonLayout = {
  scope?: string;
  revision: number;
  tabs: { tabId: string; label: string; pinned: boolean; paneTree: DaemonNode;
    activeNodeId: number; expandedNodeId: number | null }[];
  tabGroupLayout: DaemonGroup;
  activeTabId: string | null;
  focusedGroupId: string;
  parkedLayouts?: Record<string, DaemonLayout>;
};
export type LayoutView = { revision: number; layout: DaemonLayout; migrationMap?: Record<string, number> };
export type LayoutReply = { type: "ready"; view: LayoutView }
  | { type: "conflict"; code: "LAYOUT_CONFLICT"; revision: number; layout: DaemonLayout }
  | { type: "unsupported" };
export type LayoutIntent = { type: string; [key: string]: unknown };
export interface LayoutTransport {
  get(): Promise<LayoutReply>;
  apply(baseRevision: number, intent: LayoutIntent): Promise<LayoutReply>;
}
const transport: LayoutTransport = {
  get: () => invoke("cmd_layout_get"),
  apply: (baseRevision, intent) => invoke("cmd_layout_apply", { baseRevision, intent }),
};

// Only identities and surface-bearing content survive module replacement here. The
// daemon tree is fetched again; no HMR copy of its structure becomes authoritative.
type Identities = { leaves: Record<string, string>; contents: Record<string, PaneContent> };
const identityHost = globalThis as typeof globalThis & { __ferryxLayoutIdentities?: Identities };
const identities = identityHost.__ferryxLayoutIdentities ??= { leaves: {}, contents: {} };
const identityKey = (scope: string, tab: string, node: number) => JSON.stringify([scope, tab, node]);

export function daemonContent(content: PaneContent, state: WorkspaceState): DaemonContent {
  switch (content.kind) {
    case "terminal": {
      const backendSessionId = state.sessions[content.sessionId]?.backendSessionId;
      if (!backendSessionId) throw new Error("Layout terminal has no admitted backend session");
      return { kind: "terminal", backendSessionId };
    }
    case "file": return { ...content };
    case "changes": return { ...content };
    case "browser": return { kind: "browser", ...content.browser, browserId: content.browser?.browserId ?? content.browserId, url: content.browser?.url ?? content.url ?? "" };
    case "remoteDesktop": return { kind: "remoteDesktop", hostId: content.hostId };
    default: return { kind: content.kind };
  }
}

export class LayoutSync {
  private view: LayoutView | null = null;
  private pending: Promise<void> = Promise.resolve();
  private generation = 0;
  private queued = 0;
  constructor(private readonly io: LayoutTransport = transport) {}
  get enabled(): boolean { return this.view !== null; }
  get revision(): number | null { return this.view?.revision ?? null; }
  get busy(): boolean { return this.queued > 0; }
  async refresh(): Promise<boolean> {
    await this.pending;
    const reply = await this.io.get();
    if (reply.type === "unsupported") { this.view = null; return false; }
    this.accept(reply);
    return true;
  }
  private accept(reply: Exclude<LayoutReply, { type: "unsupported" }>): void {
    const view = reply.type === "ready" ? reply.view : { revision: reply.revision, layout: reply.layout };
    if (this.view && view.revision < this.view.revision) return;
    this.view = view;
  }
  /** Serialize writes. A conflict cancels queued stale gestures instead of replaying
   * them against a different tree (in particular, never replay a close). */
  send(intent: () => LayoutIntent | null, reconciled: (publish: boolean) => void): Promise<void> {
    const generation = this.generation;
    this.queued++;
    const run = this.pending.then(async () => {
      if (!this.view || generation !== this.generation) return;
      const wire = intent();
      if (!wire) { reconciled(this.queued === 1); return; }
      const reply = await this.io.apply(this.view.revision, wire);
      if (reply.type === "unsupported") throw new Error("Daemon layout capability disappeared");
      if (reply.type === "conflict") this.generation++;
      this.accept(reply);
      reconciled(reply.type === "conflict" || this.queued === 1);
    }).finally(() => { this.queued--; });
    // Keep the queue usable, but return the error to the caller for reporting.
    this.pending = run.catch(() => {});
    return run;
  }
  current(): LayoutView | null { return this.view; }

  nodeId(scope: string, tab: string, leaf: string): number {
    const prefix = JSON.stringify([scope, tab]).slice(0, -1) + ",";
    const key = Object.keys(identities.leaves).find((key) => key.startsWith(prefix) && identities.leaves[key] === leaf);
    if (!key) throw new Error("Layout pane identity is not acknowledged by the daemon");
    return (JSON.parse(key) as [string, string, number])[2];
  }
}

export const layoutSync = new LayoutSync();
export function onLayoutChanged(refresh: () => void): Promise<() => void> {
  return listen<{ window: string; revision: number }>("LayoutChanged", ({ payload }) => {
    if (payload.window === currentWindowLabel()) refresh();
  });
}

export function layoutScope(state: WorkspaceState): string {
  return JSON.stringify([state.workspaceId ?? "", state.activeWorktreePath ?? null]);
}

/** Reuse both the visual identity and the surface-bearing content. Reading a
 * daemon revision never calls terminal close, browser close, or native detach. */
export function hydrateLayout(view: LayoutView, state: WorkspaceState, scope = layoutScope(state)): LayoutState {
  const saved = view.layout;
  const result: LayoutState = { tabs: [], activeTabId: saved.activeTabId, layoutsByTabId: {},
    tabGroups: {}, focusedGroupId: saved.focusedGroupId };
  for (const tab of saved.tabs) {
    const previous = state.layout.layoutsByTabId[tab.tabId];
    const contents: Record<string, PaneContent> = {};
    const sessions: Record<string, string> = {};
    const used = new Set<string>();
    const leafFor = (node: number) => identities.leaves[identityKey(scope, tab.tabId, node)] ?? `daemon-leaf-${node}`;
    const walk = (node: DaemonNode): PaneNode => {
      if (node.type === "split") return { type: "split", direction: node.dir, ratio: node.ratio,
        first: walk(node.a), second: walk(node.b) };
      const key = identityKey(scope, tab.tabId, node.nodeId);
      // Migration maps are scoped tuples; no legacy visual id is sent back.
      const migrated = Object.entries(view.migrationMap ?? {}).find(([key, id]) => {
        const tuple = JSON.parse(key) as [string, string, string];
        return id === node.nodeId && tuple[0] === scope && tuple[1] === tab.tabId;
      });
      let leaf = identities.leaves[key] ?? (migrated ? (JSON.parse(migrated[0]) as string[])[2] : undefined);
      if (!leaf) leaf = Object.keys(previous?.sessionIdsByLeafId ?? {}).find((id) => {
        if (used.has(id)) return false;
        const content = previous?.contentsByLeafId?.[id];
        return node.content.kind === "terminal"
          ? state.sessions[previous.sessionIdsByLeafId[id]]?.backendSessionId === node.content.backendSessionId
          : content?.kind === node.content.kind;
      });
      leaf ??= `daemon-leaf-${node.nodeId}`;
      identities.leaves[key] = leaf;
      used.add(leaf);
      const old = previous?.contentsByLeafId?.[leaf] ?? identities.contents[key];
      const wire = node.content;
      let content: PaneContent;
      switch (wire.kind) {
        case "terminal": {
          const session = Object.values(state.sessions).find((session) => session.backendSessionId === wire.backendSessionId);
          content = { kind: "terminal", sessionId: session?.id ?? wire.backendSessionId };
          break;
        }
        case "browser": content = createBrowserPaneContent({
          ...(old?.kind === "browser" ? old.browser : {}),
          ...wire,
          browserId: wire.browserId ?? (old?.kind === "browser" ? old.browser?.browserId ?? old.browserId ?? "" : ""),
          url: wire.url,
        }); break;
        case "file": content = { ...(old?.kind === "file" ? old : {
          kind: "file" as const, backendSessionId: "", workspaceId: state.workspaceId ?? null,
          previewId: `daemon-preview-${node.nodeId}` }),
          ...wire, path: wire.path, line: wire.line, col: wire.col }; break;
        case "changes": content = { kind: "changes", workspaceId: state.workspaceId ?? "",
          worktreePath: state.activeWorktreePath ?? "", worktree: null, ...wire }; break;
        default: content = wire;
      }
      identities.contents[key] = content;
      contents[leaf] = content;
      sessions[leaf] = content.kind === "terminal" ? content.sessionId : "";
      return { type: "leaf", leafId: leaf };
    };
    const root = walk(tab.paneTree);
    const activeLeafId = leafFor(tab.activeNodeId);
    const primary = contents[activeLeafId] ?? Object.values(contents)[0];
    const oldTab = state.layout.tabs.find((candidate) => candidate.id === tab.tabId);
    let frontend: WorkspaceTab;
    const base = { id: tab.tabId, label: tab.label, pinned: tab.pinned };
    if (primary.kind === "terminal") frontend = { ...base, sessionId: primary.sessionId };
    else if (primary.kind === "browser") frontend = { ...base, kind: "browser", browserId: primary.browser?.browserId ?? "", url: primary.browser?.url ?? "" };
    else if (primary.kind === "remoteDesktop") frontend = { ...base, kind: "remoteDesktop", hostId: primary.hostId, sessionId: "" };
    else if (primary.kind === "file" || primary.kind === "changes") frontend = { ...primary, ...base };
    else frontend = oldTab ?? { ...base, sessionId: "" };
    result.tabs.push({ ...oldTab, ...frontend });
    result.layoutsByTabId[tab.tabId] = { root, activeLeafId, expandedLeafId: tab.expandedNodeId === null ? null : leafFor(tab.expandedNodeId),
      contentsByLeafId: contents, sessionIdsByLeafId: sessions };
  }
  const group = (node: DaemonGroup): TabGroupLayoutNode => {
    if (node.type === "split") return { type: "split", direction: node.dir, ratio: node.ratio, first: group(node.a), second: group(node.b) };
    result.tabGroups![node.groupId] = { id: node.groupId, tabIds: node.tabIds, activeTabId: node.activeTabId };
    return { type: "group", groupId: node.groupId };
  };
  result.tabGroupLayout = group(saved.tabGroupLayout);
  return normalizeLayout(result);
}

export function intentForAction(sync: LayoutSync, action: WorkspaceAction, before: WorkspaceState, after: WorkspaceState): LayoutIntent | null {
  const intent = unscopedIntentForAction(sync, action, before, after);
  return intent ? { type: "SCOPED", scope: layoutScope(before), intent } : null;
}

function unscopedIntentForAction(sync: LayoutSync, action: WorkspaceAction, before: WorkspaceState, after: WorkspaceState): LayoutIntent | null {
  const scope = layoutScope(before);
  const node = (tab: string, leaf: string) => sync.nodeId(scope, tab, leaf);
  const content = (tab: string, leaf?: string): DaemonContent => {
    const panes = after.layout.layoutsByTabId[tab];
    const id = leaf ?? panes.activeLeafId;
    const pane = panes.contentsByLeafId?.[id] ?? { kind: "terminal" as const, sessionId: panes.sessionIdsByLeafId[id] };
    return daemonContent(pane, after);
  };
  switch (action.type) {
    case "ADD_TAB_WITH_SESSION": return { type: action.type, tabId: action.tab.id,
      label: action.tab.label, activate: action.activate !== false, content: content(action.tab.id) };
    case "CLOSE_TAB": return { type: action.type, tabId: action.tabId,
      replacement: action.replacement && after.layout.tabs.some((tab) => tab.id === action.replacement!.tab.id) ? { tabId: action.replacement.tab.id, label: action.replacement.tab.label,
        content: content(action.replacement.tab.id) } : null };
    case "SET_PANE_RATIO": return { ...action, seam: action.isolated ? null : action.seam ?? resolveSeam(before.layout.layoutsByTabId[action.tabId].root, action.path) };
    case "ACTIVATE_TAB": case "REORDER_TAB": case "RENAME_TAB":
    case "SET_TAB_PINNED": case "SET_TAB_GROUP_RATIO":
    case "EQUALIZE_TAB_GROUP_RUN": case "EQUALIZE_PANE_RUN":
      return { ...action };
    case "SPLIT_PANE": return { type: action.type, tabId: action.tabId,
      targetNodeId: node(action.tabId, action.targetLeafId ?? before.layout.layoutsByTabId[action.tabId].activeLeafId),
      dir: action.direction, first: action.position === "first", content: content(action.tabId, action.newLeafId) };
    case "MOVE_TAB_TO_GROUP": return { ...action, targetIndex: action.targetIndex ?? Number.MAX_SAFE_INTEGER };
    case "MOVE_TAB_TO_SPLIT": return { type: action.type, sourceTabId: action.sourceTabId,
      targetGroupId: action.targetGroupId, dir: action.direction, first: action.position === "first" };
    case "MOVE_TAB_TO_PANE_SPLIT": return { type: action.type, sourceTabId: action.sourceTabId,
      targetTabId: action.targetTabId, targetNodeId: node(action.targetTabId, action.targetLeafId),
      dir: action.direction, first: action.position === "first" };
    case "DETACH_PANE_TO_TAB": return { type: action.type, sourceTabId: action.sourceTabId,
      nodeId: node(action.sourceTabId, action.leafId), tabId: action.newTab.id, label: action.newTab.label,
      targetGroupId: action.targetGroupId ?? before.layout.focusedGroupId ?? "main" };
    case "CLOSE_PANE": case "FOCUS_PANE":
      return { type: action.type, tabId: action.tabId, nodeId: node(action.tabId, action.leafId) };
    case "SWAP_PANES": return { type: action.type, tabId: action.tabId,
      sourceNodeId: node(action.tabId, action.sourceLeafId), targetNodeId: node(action.tabId, action.targetLeafId) };
    case "UPDATE_FILE_TAB": {
      const pane = before.layout.layoutsByTabId[action.tabId];
      return { type: action.type, tabId: action.tabId, nodeId: node(action.tabId, pane.activeLeafId),
        ...content(action.tabId), line: action.line, col: action.col };
    }
    case "UPDATE_BROWSER_TAB": {
      if (action.updates.url === undefined) return null;
      const pane = before.layout.layoutsByTabId[action.tabId];
      return { type: action.type, tabId: action.tabId, nodeId: node(action.tabId, pane.activeLeafId), url: action.updates.url };
    }
    default: return null;
  }
}

export function ownsDaemonRoot(_state: WorkspaceState, view = layoutSync.current()): boolean {
  return view !== null;
}

export function isLayoutIntentAction(action: WorkspaceAction): boolean {
  return ["ADD_TAB_WITH_SESSION", "CLOSE_TAB", "ACTIVATE_TAB", "REORDER_TAB", "RENAME_TAB", "SET_TAB_PINNED",
    "SET_PANE_RATIO", "SET_TAB_GROUP_RATIO", "EQUALIZE_TAB_GROUP_RUN", "EQUALIZE_PANE_RUN", "SPLIT_PANE",
    "MOVE_TAB_TO_GROUP", "MOVE_TAB_TO_SPLIT", "MOVE_TAB_TO_PANE_SPLIT", "DETACH_PANE_TO_TAB", "CLOSE_PANE",
    "FOCUS_PANE", "SWAP_PANES", "UPDATE_FILE_TAB", "UPDATE_BROWSER_TAB"].includes(action.type);
}

export function rehydrateWorkspace(state: WorkspaceState, view = layoutSync.current()): WorkspaceState {
  if (!view) return state;
  const sessions = { ...state.sessions };
  const admit = (node: DaemonNode) => {
    if (node.type === "split") { admit(node.a); admit(node.b); return; }
    if (node.content.kind !== "terminal") return;
    const id = node.content.backendSessionId;
    if (!Object.values(sessions).some((session) => session.backendSessionId === id)) {
      sessions[id] = { id, backendSessionId: id, workspaceId: state.workspaceId ?? "", cwd: state.activeWorktreePath ?? "",
        worktree: null, lifecycle: "working" };
    }
  };
  const find = (layout: DaemonLayout, scope: string): DaemonLayout | undefined => {
    if (layout.scope === scope || !layout.scope && scope === JSON.stringify([state.workspaceId ?? "", null])) return layout;
    if (layout.parkedLayouts?.[scope]) return layout.parkedLayouts[scope];
    for (const parked of Object.values(layout.parkedLayouts ?? {})) { const found = find(parked, scope); if (found) return found; }
    return undefined;
  };
  const saved = find(view.layout, layoutScope(state));
  if (!saved) return { ...state, layout: createLayoutState() };
  for (const tab of saved.tabs) admit(tab.paneTree);
  state = { ...state, sessions };
  const layout = hydrateLayout({ ...view, layout: saved }, state);
  const worktreeLayouts = { ...state.worktreeLayouts };
  for (const [scope, parked] of Object.entries(saved.parkedLayouts ?? {})) {
    const [workspace, path] = JSON.parse(scope) as [string, string | null];
    if (workspace !== state.workspaceId || path === null) continue;
    worktreeLayouts[path] = hydrateLayout({ ...view, layout: parked }, { ...state, layout: worktreeLayouts[path] ?? state.layout }, scope);
  }
  return { ...state, layout, worktreeLayouts };
}
