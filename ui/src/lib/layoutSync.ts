import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LayoutAction } from "../state/layout";
import { normalizeLayout } from "../state/layout";
import type { PaneNode } from "../state/paneTree";
import type { WorkspaceState } from "../state/workspaceStore";
import type { LayoutState, PaneContent, TabGroupLayoutNode, WorkspaceTab } from "./types";
import { createBrowserPaneContent } from "./types";

type Direction = "horizontal" | "vertical";
export type DaemonContent =
  | { kind: "terminal"; backendSessionId: string }
  | { kind: "file"; path: string; line: number | null; col: number | null }
  | { kind: "browser"; url: string }
  | { kind: "dag" } | { kind: "changes" }
  | { kind: "remoteDesktop"; hostId: string };
export type DaemonNode =
  | { type: "leaf"; nodeId: number; content: DaemonContent }
  | { type: "split"; dir: Direction; ratio: number; a: DaemonNode; b: DaemonNode };
type DaemonGroup =
  | { type: "group"; groupId: string; tabIds: string[]; activeTabId: string | null }
  | { type: "split"; dir: Direction; ratio: number; a: DaemonGroup; b: DaemonGroup };
export type DaemonLayout = {
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
    case "file": return { kind: "file", path: content.path, line: content.line, col: content.col };
    case "browser": return { kind: "browser", url: content.browser?.url ?? content.url ?? "" };
    case "remoteDesktop": return { kind: "remoteDesktop", hostId: content.hostId };
    default: return { kind: content.kind };
  }
}

export class LayoutSync {
  private view: LayoutView | null = null;
  private pending: Promise<void> = Promise.resolve();
  private generation = 0;
  constructor(private readonly io: LayoutTransport = transport) {}
  get enabled(): boolean { return this.view !== null; }
  get revision(): number | null { return this.view?.revision ?? null; }
  async refresh(): Promise<boolean> {
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
  send(intent: () => LayoutIntent | null, reconciled: () => void): Promise<void> {
    const generation = this.generation;
    const run = this.pending.then(async () => {
      if (!this.view || generation !== this.generation) return;
      const wire = intent();
      if (!wire) return;
      const reply = await this.io.apply(this.view.revision, wire);
      if (reply.type === "unsupported") throw new Error("Daemon layout capability disappeared");
      if (reply.type === "conflict") this.generation++;
      this.accept(reply);
      reconciled();
    });
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
  return listen<{ window: string; revision: number }>("LayoutChanged", () => refresh());
}
