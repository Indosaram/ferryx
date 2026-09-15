import { useSyncExternalStore } from "react";

import type { TerminalActivityState } from "./activity";
import { loadGeneralSettings } from "./generalSettings";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "./remoteProject";
import { getTerminalHistorySnapshot, hibernateTerminal, onNativeTerminalAgentState } from "./tauri";
import type { SessionProcessState, TerminalSession } from "./types";

export type { SessionProcessState } from "./types";
export type SessionLifecycleAction = "hibernate" | "restart";

export const STANDBY_BACKEND_PREFIX = "standby:";
const MAX_PERSISTED_SCROLLBACK_CHARS = 256_000;

export function createStandbyBackendSessionId(sessionId: string): string {
  return `${STANDBY_BACKEND_PREFIX}${sessionId}`;
}

export function isStandbyBackendSessionId(sessionId: string | null | undefined): boolean {
  return typeof sessionId === "string" && sessionId.startsWith(STANDBY_BACKEND_PREFIX);
}

export function getSessionProcessState(session: TerminalSession | null | undefined): SessionProcessState {
  if (!session) return "standby";
  if (session.backendSessionId && !isStandbyBackendSessionId(session.backendSessionId)) return "running";
  if (isStandbyBackendSessionId(session.backendSessionId)) {
    return session.processState === "hibernated" ? "hibernated" : "standby";
  }
  if (isSessionSleeping(session.id) || session.processState === "hibernated") return "hibernated";
  return "standby";
}

type RegisteredSession = {
  session: TerminalSession;
  active: boolean;
  idleSince: number | null;
};

const registeredSessions = new Map<string, RegisteredSession>();
const recentScrollbackBySessionId = new Map<string, string>();
const sleepingSessionIds = new Set<string>();
const manualHibernateHoldIds = new Set<string>();
const sleepingListeners = new Set<() => void>();
const actionListeners = new Set<(action: SessionLifecycleAction, sessionId: string) => void>();
let sleepingSnapshot = "";
let monitoringStarted = false;
let idleSweepTimer: ReturnType<typeof setInterval> | null = null;

function emitSleepingChange(): void {
  sleepingSnapshot = [...sleepingSessionIds].sort().join("\u0000");
  for (const listener of sleepingListeners) listener();
}

export function setSessionSleeping(sessionId: string, sleeping: boolean): void {
  const changed = sleeping ? !sleepingSessionIds.has(sessionId) : sleepingSessionIds.has(sessionId);
  if (!changed) return;
  if (sleeping) sleepingSessionIds.add(sessionId);
  else sleepingSessionIds.delete(sessionId);
  emitSleepingChange();
}

export function isSessionSleeping(sessionId: string): boolean {
  return sleepingSessionIds.has(sessionId);
}

export function isSessionAutoResumeHeld(sessionId: string): boolean {
  return manualHibernateHoldIds.has(sessionId);
}

export function clearSleepingSessions(): void {
  if (sleepingSessionIds.size === 0) return;
  sleepingSessionIds.clear();
  emitSleepingChange();
}

export function subscribeSleepingSessions(listener: () => void): () => void {
  sleepingListeners.add(listener);
  return () => sleepingListeners.delete(listener);
}

export function getSleepingSessionsSnapshot(): string {
  return sleepingSnapshot;
}

export function useSleepingSessionIds(): ReadonlySet<string> {
  const snapshot = useSyncExternalStore(
    subscribeSleepingSessions,
    getSleepingSessionsSnapshot,
    getSleepingSessionsSnapshot,
  );
  return new Set(snapshot ? snapshot.split("\u0000") : []);
}

export function getSessionRecentScrollback(sessionId: string): string | undefined {
  return recentScrollbackBySessionId.get(sessionId);
}

export function restoreSessionRecentScrollback(sessionId: string, value: string | null | undefined): void {
  if (!value) return;
  recentScrollbackBySessionId.set(
    sessionId,
    value.length > MAX_PERSISTED_SCROLLBACK_CHARS
      ? value.slice(-MAX_PERSISTED_SCROLLBACK_CHARS)
      : value,
  );
}

function activityIsIdle(state: TerminalActivityState | "idle" | "blocked" | undefined): boolean {
  return state === "done" || state === "idle";
}

export function registerSessionSnapshot(
  session: TerminalSession,
  activityState?: TerminalActivityState | "idle" | "blocked",
): void {
  const previous = registeredSessions.get(session.id);
  const now = Date.now();
  const idleSince = activityIsIdle(activityState)
    ? previous?.idleSince ?? now
    : activityState === "working" || activityState === "waiting" || activityState === "blocked"
      ? null
      : previous?.idleSince ?? null;
  registeredSessions.set(session.id, {
    session,
    active: previous?.active ?? false,
    idleSince,
  });
  ensureLifecycleMonitoring();
}

export function setSessionActive(sessionId: string, active: boolean): void {
  const entry = registeredSessions.get(sessionId);
  if (!entry) return;
  const wasActive = entry.active;
  entry.active = active;
  // Manual Hibernate on an already-active pane must stay asleep. A real focus
  // transition away and back is what releases the hold and permits transparent wakeup.
  if (active && !wasActive) manualHibernateHoldIds.delete(sessionId);
}

export function markSessionActivity(
  sessionId: string,
  state: TerminalActivityState | "idle" | "blocked",
): void {
  const entry = registeredSessions.get(sessionId);
  if (!entry) return;
  entry.idleSince = activityIsIdle(state) ? entry.idleSince ?? Date.now() : null;
}

function findRegisteredByBackend(backendSessionId: string): RegisteredSession | null {
  for (const entry of registeredSessions.values()) {
    if (entry.session.backendSessionId === backendSessionId) return entry;
  }
  return null;
}

async function captureRecentScrollback(sessionId: string, backendSessionId: string): Promise<void> {
  try {
    const history = await getTerminalHistorySnapshot(backendSessionId);
    if (!history) return;
    recentScrollbackBySessionId.set(
      sessionId,
      history.length > MAX_PERSISTED_SCROLLBACK_CHARS
        ? history.slice(-MAX_PERSISTED_SCROLLBACK_CHARS)
        : history,
    );
  } catch {
    // History capture is best-effort; failure must never prevent memory reclamation.
  }
}

export async function hibernateRegisteredSession(sessionId: string): Promise<void> {
  const entry = registeredSessions.get(sessionId);
  if (!entry) {
    setSessionSleeping(sessionId, true);
    return;
  }
  const session = entry.session;
  if (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)) return;
  const backendSessionId = session.backendSessionId;
  if (!backendSessionId || isStandbyBackendSessionId(backendSessionId)) {
    entry.session = { ...session, backendSessionId: null, processState: "hibernated", lifecycle: "exited" };
    setSessionSleeping(sessionId, true);
    return;
  }

  await captureRecentScrollback(sessionId, backendSessionId);
  setSessionSleeping(sessionId, true);
  try {
    await hibernateTerminal(backendSessionId);
  } catch (error) {
    setSessionSleeping(sessionId, false);
    throw error;
  }
  entry.session = { ...session, backendSessionId: null, processState: "hibernated", lifecycle: "exited" };
  entry.idleSince = null;
}

async function sweepIdleSessions(): Promise<void> {
  const timeoutMs = loadGeneralSettings().sessionIdleTimeoutMinutes * 60_000;
  const now = Date.now();
  const candidates = [...registeredSessions.entries()].filter(([, entry]) =>
    !entry.active &&
    entry.idleSince !== null &&
    now - entry.idleSince >= timeoutMs &&
    Boolean(entry.session.backendSessionId) &&
    !isStandbyBackendSessionId(entry.session.backendSessionId) &&
    !isRemoteWorkspaceId(entry.session.workspaceId) &&
    !isPairedWorkspaceId(entry.session.workspaceId),
  );
  await Promise.allSettled(candidates.map(([sessionId]) => hibernateRegisteredSession(sessionId)));
}

function ensureLifecycleMonitoring(): void {
  if (monitoringStarted) return;
  monitoringStarted = true;
  void Promise.resolve(onNativeTerminalAgentState((payload) => {
    const entry = findRegisteredByBackend(payload.sessionId);
    if (!entry) return;
    entry.idleSince = payload.state === "idle" ? entry.idleSince ?? Date.now() : null;
  })).catch(() => undefined);
  idleSweepTimer = setInterval(() => {
    void sweepIdleSessions();
  }, 30_000);
}

export function requestSessionLifecycleAction(action: SessionLifecycleAction, sessionId: string): void {
  if (action === "hibernate") manualHibernateHoldIds.add(sessionId);
  else manualHibernateHoldIds.delete(sessionId);
  for (const listener of actionListeners) listener(action, sessionId);
  void hibernateRegisteredSession(sessionId).catch((error) => {
    if (action === "hibernate") manualHibernateHoldIds.delete(sessionId);
    console.warn(`Failed to ${action} session`, error);
  });
}

export function subscribeSessionLifecycleActions(
  listener: (action: SessionLifecycleAction, sessionId: string) => void,
): () => void {
  actionListeners.add(listener);
  return () => actionListeners.delete(listener);
}

export function resetSessionLifecycleForTests(): void {
  registeredSessions.clear();
  recentScrollbackBySessionId.clear();
  sleepingSessionIds.clear();
  manualHibernateHoldIds.clear();
  sleepingSnapshot = "";
  monitoringStarted = false;
  if (idleSweepTimer) clearInterval(idleSweepTimer);
  idleSweepTimer = null;
}
