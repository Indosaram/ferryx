import { getAgentReconnectAffordance } from "./agentResumeAffordance";
import { loadGeneralSettings } from "./generalSettings";
import { isStandbyBackendSessionId } from "./sessionLifecycle";
import { isTerminalTab, type TerminalSession } from "./types";
import type { WorkspaceState } from "../state/workspaceStore";

export const MAX_AUTO_RESUME_CANDIDATES = 8;
export const AUTO_RESUME_STAGGER_INTERVAL_MS = 400;

const executedRestoreTokens = new Set<string>();
const pendingTimeouts = new Set<ReturnType<typeof setTimeout>>();

export function computeRestoreToken(workspaceId: string, state: WorkspaceState): string {
  const sessionSignatures = Object.entries(state.sessions)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([id, s]) => `${id}:${s.agentType ?? ""}:${s.agentSessionId ?? ""}:${s.providerSession?.id ?? ""}:${s.backendSessionId ?? "null"}`)
    .join(";");
  return `${workspaceId}:${sessionSignatures}`;
}

export function hasExecutedRestoreToken(token: string): boolean {
  return executedRestoreTokens.has(token);
}

export function markRestoreTokenExecuted(token: string): void {
  executedRestoreTokens.add(token);
}

export function resetAgentAutoResumeGuard(workspaceId?: string): void {
  if (workspaceId) {
    for (const token of executedRestoreTokens) {
      if (token.startsWith(`${workspaceId}:`)) executedRestoreTokens.delete(token);
    }
  } else {
    executedRestoreTokens.clear();
  }
}

export function clearPendingAutoResumes(): void {
  for (const timeoutId of pendingTimeouts) clearTimeout(timeoutId);
  pendingTimeouts.clear();
}

function activeTabSessionIds(state: WorkspaceState): Set<string> {
  const result = new Set<string>();
  const focusedGroupId = state.layout.focusedGroupId;
  const activeTabId = focusedGroupId
    ? state.layout.tabGroups?.[focusedGroupId]?.activeTabId ?? state.layout.activeTabId
    : state.layout.activeTabId;
  if (!activeTabId) return result;
  const tab = state.layout.tabs.find((candidate) => candidate.id === activeTabId);
  if (!tab || !isTerminalTab(tab)) return result;
  result.add(tab.sessionId);
  const tabLayout = state.layout.layoutsByTabId?.[activeTabId];
  for (const sessionId of Object.values(tabLayout?.sessionIdsByLeafId ?? {})) {
    if (sessionId) result.add(sessionId);
  }
  return result;
}

export function collectAutoResumeCandidates(
  state: WorkspaceState,
  allSessions?: Readonly<Record<string, TerminalSession>>,
  limit?: number,
  allowedSessionIds?: ReadonlySet<string>,
): string[] {
  const sessions = allSessions ?? state.sessions;
  const rawCandidates = Object.values(sessions).filter((session) =>
    (allowedSessionIds === undefined || allowedSessionIds.has(session.id)) &&
    (session.backendSessionId === null || isStandbyBackendSessionId(session.backendSessionId)) &&
    typeof session.agentType === "string" &&
    session.agentType.trim().length > 0,
  );

  const validCandidates: TerminalSession[] = [];
  for (const session of rawCandidates) {
    const affordance = getAgentReconnectAffordance(session, sessions);
    if (affordance.canReconnect) validCandidates.push(session);
  }
  if (validCandidates.length === 0) return [];

  const candidateIdSet = new Set(validCandidates.map((s) => s.id));
  const orderedIds: string[] = [];
  const seen = new Set<string>();
  const activeTabId = state.layout.focusedGroupId
    ? state.layout.tabGroups?.[state.layout.focusedGroupId]?.activeTabId ?? state.layout.activeTabId
    : state.layout.activeTabId ?? state.layout.tabs[0]?.id;
  const activeCandidate = activeTabId
    ? state.layout.tabs.find((t) => t.id === activeTabId)
    : null;
  const activeTab = activeCandidate && isTerminalTab(activeCandidate) ? activeCandidate : null;

  if (activeTab) {
    const tabLayout = state.layout.layoutsByTabId?.[activeTab.id];
    const activeLeafId = tabLayout?.activeLeafId;
    if (activeLeafId && tabLayout?.sessionIdsByLeafId?.[activeLeafId]) {
      const activeLeafSessionId = tabLayout.sessionIdsByLeafId[activeLeafId];
      if (candidateIdSet.has(activeLeafSessionId) && !seen.has(activeLeafSessionId)) {
        orderedIds.push(activeLeafSessionId);
        seen.add(activeLeafSessionId);
      }
    }
    if (tabLayout?.sessionIdsByLeafId) {
      for (const sessionId of Object.values(tabLayout.sessionIdsByLeafId)) {
        if (candidateIdSet.has(sessionId) && !seen.has(sessionId)) {
          orderedIds.push(sessionId);
          seen.add(sessionId);
        }
      }
    }
    if (activeTab.sessionId && candidateIdSet.has(activeTab.sessionId) && !seen.has(activeTab.sessionId)) {
      orderedIds.push(activeTab.sessionId);
      seen.add(activeTab.sessionId);
    }
  }

  for (const tab of state.layout.tabs) {
    if (!isTerminalTab(tab)) continue;
    const terminalTab = tab;
    const tabLayout = state.layout.layoutsByTabId?.[terminalTab.id];
    if (tabLayout?.sessionIdsByLeafId) {
      for (const sessionId of Object.values(tabLayout.sessionIdsByLeafId)) {
        if (candidateIdSet.has(sessionId) && !seen.has(sessionId)) {
          orderedIds.push(sessionId);
          seen.add(sessionId);
        }
      }
    }
    if (terminalTab.sessionId && candidateIdSet.has(terminalTab.sessionId) && !seen.has(terminalTab.sessionId)) {
      orderedIds.push(terminalTab.sessionId);
      seen.add(terminalTab.sessionId);
    }
  }

  for (const session of validCandidates) {
    if (!seen.has(session.id)) {
      orderedIds.push(session.id);
      seen.add(session.id);
    }
  }
  return typeof limit === "number" ? orderedIds.slice(0, limit) : orderedIds;
}

export type ScheduleAgentAutoResumeOptions = {
  workspaceId: string;
  state: WorkspaceState;
  recoveredFromHmr: boolean;
  reconnect: (sessionId: string) => Promise<unknown>;
  staggerIntervalMs?: number;
  maxCandidates?: number;
  allowedSessionIds?: ReadonlySet<string>;
  ignorePolicy?: boolean;
};

export function scheduleAgentAutoResume({
  workspaceId,
  state,
  recoveredFromHmr,
  reconnect,
  staggerIntervalMs = AUTO_RESUME_STAGGER_INTERVAL_MS,
  maxCandidates,
  allowedSessionIds,
  ignorePolicy = false,
}: ScheduleAgentAutoResumeOptions): () => void {
  if (recoveredFromHmr) return () => {};

  const policy = loadGeneralSettings().sessionRestorePolicy;
  if (!ignorePolicy && policy === "lazy") return () => {};

  let effectiveAllowed = allowedSessionIds;
  if (!ignorePolicy && policy === "activeOnly") {
    const activeIds = activeTabSessionIds(state);
    effectiveAllowed = allowedSessionIds
      ? new Set([...activeIds].filter((sessionId) => allowedSessionIds.has(sessionId)))
      : activeIds;
  }

  const token = computeRestoreToken(workspaceId, state);
  if (hasExecutedRestoreToken(token)) return () => {};
  markRestoreTokenExecuted(token);

  const candidates = collectAutoResumeCandidates(state, undefined, maxCandidates, effectiveAllowed);
  if (candidates.length === 0) return () => {};

  const localTimeouts: ReturnType<typeof setTimeout>[] = [];
  for (let index = 0; index < candidates.length; index++) {
    const sessionId = candidates[index];
    const delay = index * staggerIntervalMs;
    const timeoutId = setTimeout(() => {
      pendingTimeouts.delete(timeoutId);
      void reconnect(sessionId).catch(() => undefined);
    }, delay);
    pendingTimeouts.add(timeoutId);
    localTimeouts.push(timeoutId);
  }

  return () => {
    for (const timeoutId of localTimeouts) {
      clearTimeout(timeoutId);
      pendingTimeouts.delete(timeoutId);
    }
  };
}
