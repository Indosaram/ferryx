import { getAgentReconnectAffordance } from "./agentResumeAffordance";
import type { TerminalSession, TerminalTab } from "./types";
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
      if (token.startsWith(`${workspaceId}:`)) {
        executedRestoreTokens.delete(token);
      }
    }
  } else {
    executedRestoreTokens.clear();
  }
}

export function clearPendingAutoResumes(): void {
  for (const timeoutId of pendingTimeouts) {
    clearTimeout(timeoutId);
  }
  pendingTimeouts.clear();
}

export function collectAutoResumeCandidates(
  state: WorkspaceState,
  allSessions?: Readonly<Record<string, TerminalSession>>,
): string[] {
  const sessions = allSessions ?? state.sessions;
  const rawCandidates = Object.values(sessions).filter((session) => {
    return (
      session.backendSessionId === null &&
      typeof session.agentType === "string" &&
      session.agentType.trim().length > 0
    );
  });

  const validCandidates: TerminalSession[] = [];
  for (const session of rawCandidates) {
    const affordance = getAgentReconnectAffordance(session, sessions);
    if (affordance.canReconnect) {
      validCandidates.push(session);
    }
  }

  if (validCandidates.length === 0) return [];

  const candidateIdSet = new Set(validCandidates.map((s) => s.id));
  const orderedIds: string[] = [];
  const seen = new Set<string>();

  const activeTabId = state.layout.focusedGroupId
    ? state.layout.tabGroups?.[state.layout.focusedGroupId]?.activeTabId ?? state.layout.activeTabId
    : state.layout.activeTabId ?? state.layout.tabs[0]?.id;

  const activeTab = activeTabId
    ? (state.layout.tabs.find((t) => t.id === activeTabId && t.kind !== "browser") as TerminalTab | undefined)
    : null;

  // 1. Active tab session(s) first
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
      for (const sessId of Object.values(tabLayout.sessionIdsByLeafId)) {
        if (candidateIdSet.has(sessId) && !seen.has(sessId)) {
          orderedIds.push(sessId);
          seen.add(sessId);
        }
      }
    }
    if (activeTab.sessionId && candidateIdSet.has(activeTab.sessionId) && !seen.has(activeTab.sessionId)) {
      orderedIds.push(activeTab.sessionId);
      seen.add(activeTab.sessionId);
    }
  }

  // 2. Open tabs in layout order
  for (const tab of state.layout.tabs) {
    if (tab.kind === "browser") continue;
    const termTab = tab as TerminalTab;
    const tabLayout = state.layout.layoutsByTabId?.[termTab.id];
    if (tabLayout?.sessionIdsByLeafId) {
      for (const sessId of Object.values(tabLayout.sessionIdsByLeafId)) {
        if (candidateIdSet.has(sessId) && !seen.has(sessId)) {
          orderedIds.push(sessId);
          seen.add(sessId);
        }
      }
    }
    if (termTab.sessionId && candidateIdSet.has(termTab.sessionId) && !seen.has(termTab.sessionId)) {
      orderedIds.push(termTab.sessionId);
      seen.add(termTab.sessionId);
    }
  }

  // 3. Any remaining valid candidates
  for (const session of validCandidates) {
    if (!seen.has(session.id)) {
      orderedIds.push(session.id);
      seen.add(session.id);
    }
  }

  return orderedIds.slice(0, MAX_AUTO_RESUME_CANDIDATES);
}

export type ScheduleAgentAutoResumeOptions = {
  workspaceId: string;
  state: WorkspaceState;
  recoveredFromHmr: boolean;
  reconnect: (sessionId: string) => Promise<unknown>;
  staggerIntervalMs?: number;
  maxCandidates?: number;
};

export function scheduleAgentAutoResume({
  workspaceId,
  state,
  recoveredFromHmr,
  reconnect,
  staggerIntervalMs = AUTO_RESUME_STAGGER_INTERVAL_MS,
  maxCandidates = MAX_AUTO_RESUME_CANDIDATES,
}: ScheduleAgentAutoResumeOptions): () => void {
  if (recoveredFromHmr) {
    return () => {};
  }

  const token = computeRestoreToken(workspaceId, state);
  if (hasExecutedRestoreToken(token)) {
    return () => {};
  }
  markRestoreTokenExecuted(token);

  const candidates = collectAutoResumeCandidates(state).slice(0, maxCandidates);
  if (candidates.length === 0) {
    return () => {};
  }

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
