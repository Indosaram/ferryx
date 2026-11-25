import type { TerminalSession } from "./types";
import type { WorkspaceState, CloseBatchMode } from "../state/workspaceStore";
import {
  getCloseBatchTabIds,
  getLiveSessionsForPane,
  getLiveSessionsForTab,
  getLiveSessionsForTabs,
  getTabById,
  getTabLayoutById,
  getTabSessionIds,
} from "../state/workspaceStore";

/**
 * What the user is asking to close. Every close entry point funnels into this shape so the
 * guard decision is made in exactly one place instead of being re-derived per call site.
 */
export type CloseTarget =
  | { kind: "tab"; tabId: string }
  | { kind: "pane"; tabId: string; leafId: string }
  | { kind: "batch"; mode: CloseBatchMode; tabId: string };

export type CloseAssessment =
  /** The store would refuse this close, so nothing may be executed. */
  | { decision: "refuse"; target: CloseTarget }
  /** Nothing is at risk: close immediately. */
  | { decision: "close"; target: CloseTarget }
  | {
      decision: "confirm";
      target: CloseTarget;
      label: string;
      /** Tabs the close would remove (1 for a single-tab or single-pane close). */
      tabCount: number;
      /** Live backend PTYs that the confirmed close would destroy, deduplicated by backend id. */
      liveSessions: TerminalSession[];
      runningAgentCount: number;
    };

type AssessmentContext = {
  state: WorkspaceState;
  /** `true` when the user enabled "always confirm closing a tab". */
  confirmAlways: boolean;
  runningSessionIds: ReadonlySet<string>;
  labelForTab: (tabId: string) => string;
};

function dedupeByBackend(sessions: readonly TerminalSession[]): TerminalSession[] {
  const seen = new Set<string>();
  const result: TerminalSession[] = [];
  for (const session of sessions) {
    const key = session.backendSessionId ?? session.id;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(session);
  }
  return result;
}

/**
 * Running agents among the sessions a close would dispose of.
 *
 * This deliberately reads the *unfiltered* session set: a session whose PTY record already
 * vanished can still report `working` activity, and closing under a running agent must not
 * become silent just because the liveness check no longer sees a backend id.
 */
function countRunningAgents(sessions: readonly TerminalSession[], runningSessionIds: ReadonlySet<string>): number {
  const keys = new Set<string>();
  for (const session of sessions) {
    if (runningSessionIds.has(session.id)) keys.add(session.id);
  }
  return keys.size;
}

function sessionsForTarget(
  state: WorkspaceState,
  target: CloseTarget,
  runningSessionIds: ReadonlySet<string>,
): TerminalSession[] {
  if (target.kind === "tab") return getLiveSessionsForTab(state, target.tabId);
  // A split pane is closed without a prompt, exactly as the previous handler did: only a
  // single-leaf tab (whose pane close is really a tab close) was ever gated.
  if (target.kind === "pane") {
    if (getTabLayoutById(state, target.tabId)?.root.type !== "leaf") return [];
    const owned = ownedSessionsForTarget(state, target);
    const running = owned.filter((session) => runningSessionIds.has(session.id));
    if (running.length > 0) return running;
    return getLiveSessionsForPane(state, target.tabId, target.leafId);
  }
  return getLiveSessionsForTabs(state, getCloseBatchTabIds(state, target.tabId, target.mode));
}

/** Every session the target would dispose of, live or not. */
function ownedSessionsForTarget(state: WorkspaceState, target: CloseTarget): TerminalSession[] {
  // The running-agent count mirrors the pre-existing handler, which counted every leaf session
  // of the tab regardless of whether another tab shares it: a shared session still runs, and
  // closing this tab is still the moment the user should be warned about it.
  const leafSessions = (tabId: string): TerminalSession[] => {
    const result: TerminalSession[] = [];
    for (const sessionId of getTabSessionIds(state, tabId)) {
      const session = state.sessions[sessionId];
      if (session) result.push(session);
    }
    return result;
  };
  if (target.kind === "tab") return leafSessions(target.tabId);
  if (target.kind === "pane") {
    if (getTabLayoutById(state, target.tabId)?.root.type !== "leaf") return [];
    return leafSessions(target.tabId);
  }
  const batch: TerminalSession[] = [];
  for (const tabId of getCloseBatchTabIds(state, target.tabId, target.mode)) batch.push(...leafSessions(tabId));
  return batch;
}

/**
 * Whether the target still names something. A tab close for a vanished tab is refused, which
 * mirrors the previous "no tab, no close" guard; a pane close is still handed to the store,
 * which returns early for a leaf that is already gone.
 */
function targetExists(state: WorkspaceState, target: CloseTarget): boolean {
  if (target.kind === "batch") return getCloseBatchTabIds(state, target.tabId, target.mode).length > 0;
  if (target.kind === "tab") return Boolean(getTabById(state, target.tabId));
  const tabLayout = getTabLayoutById(state, target.tabId);
  if (!tabLayout) return false;
  if (tabLayout.root.type === "leaf") return true;
  return Boolean(tabLayout.sessionIdsByLeafId?.[target.leafId]) || Boolean(tabLayout.contentsByLeafId?.[target.leafId]);
}

/**
 * Whether closing `target` removes at most a single pane (closing one leaf of a split, or
 * closing a tab that only has a single pane left). Whole multi-pane tabs and multi-tab batches
 * destroy several panes at once and keep the live-session / running-agent guard.
 */
function isSinglePaneClose(state: WorkspaceState, target: CloseTarget): boolean {
  if (target.kind === "pane") return true;
  if (target.kind === "tab") {
    return getTabLayoutById(state, target.tabId)?.root.type !== "split";
  }
  return false;
}

/**
 * Decides whether closing `target` needs the user's confirmation, and why.
 *
 * The batch case deliberately assesses the union of every tab in the batch: sessions shared
 * between tabs inside the batch survive a per-tab assessment but are destroyed by the batch,
 * so asking per tab would both prompt twice and miss the shared ones.
 *
 * Returns the carried `target` untouched when no confirmation is needed, so callers execute
 * precisely what was assessed rather than a re-derived guess.
 */
export function assessClose(context: AssessmentContext, target: CloseTarget): CloseAssessment {
  const { state, confirmAlways, runningSessionIds, labelForTab } = context;
  // A pane close is dispatched even when nothing can happen, because the store owns that
  // decision; a tab or batch close short-circuits like the previous handler did.
  // An empty batch is the same no-op the store performs, so it is not a refusal.
  const inertDecision = target.kind === "tab" ? "refuse" : "close";
  if (!targetExists(state, target)) return { decision: inertDecision, target };
  // `closeTab` returns early for a pinned tab, so prompting there would ask the user to
  // confirm a close that can never run. A pinned tab's split pane is still closable, so only
  // the whole-tab target is refused. `closePane` has no such refusal: it is always dispatched,
  // and the store no-ops when the leaf is the pinned tab's only pane.
  if (target.kind === "tab" && getTabById(state, target.tabId)?.pinned) return { decision: "refuse", target };
  // `closePane` on a pinned tab's only pane delegates to the pinned `closeTab`, which returns
  // early. Dispatching keeps the previous "a pane close is always sent" contract without asking
  // the user to confirm a close that cannot run.
  if (
    target.kind === "pane" &&
    getTabById(state, target.tabId)?.pinned &&
    getTabLayoutById(state, target.tabId)?.root.type === "leaf"
  ) {
    return { decision: "close", target };
  }
  if (!confirmAlways && isSinglePaneClose(state, target)) {
    return { decision: "close", target };
  }
  const liveSessions = dedupeByBackend(sessionsForTarget(state, target, runningSessionIds));
  const runningAgentCount = countRunningAgents(ownedSessionsForTarget(state, target), runningSessionIds);
  if (!confirmAlways && liveSessions.length === 0 && runningAgentCount === 0) {
    return { decision: "close", target };
  }
  const tabCount = target.kind === "batch" ? getCloseBatchTabIds(state, target.tabId, target.mode).length : 1;
  return { decision: "confirm", target, label: labelForTab(target.tabId), tabCount, liveSessions, runningAgentCount };
}

/**
 * Re-runs an assessment against current state right before executing a confirmed close.
 * Returns `null` when the target no longer refers to anything closable, so a stale dialog
 * cannot execute a close the user never saw.
 */
export function reassessConfirmedClose(context: AssessmentContext, target: CloseTarget): TerminalSession[] | null {
  if (target.kind === "batch") {
    const tabIds = getCloseBatchTabIds(context.state, target.tabId, target.mode);
    // An empty batch is a no-op, not a stale target: the store would close nothing either way.
    if (tabIds.length === 0) return [];
    return getLiveSessionsForTabs(context.state, tabIds);
  }
  if (target.kind === "tab") {
    if (!getTabById(context.state, target.tabId)) return null;
    return dedupeByBackend(getLiveSessionsForTab(context.state, target.tabId));
  }
  const tabLayout = getTabLayoutById(context.state, target.tabId);
  if (!tabLayout) return null;
  if (!tabLayout.sessionIdsByLeafId?.[target.leafId] && !tabLayout.contentsByLeafId?.[target.leafId] && tabLayout.root.type !== "leaf") {
    return null;
  }
  return dedupeByBackend(getLiveSessionsForPane(context.state, target.tabId, target.leafId));
}
