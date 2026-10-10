import { useSyncExternalStore } from "react";

import type { TerminalActivityState } from "./activity";
import { loadGeneralSettings } from "./generalSettings";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "./remoteProject";
import { describeTerminal, getTerminalHistorySnapshot, hibernateTerminal, onNativeTerminalAgentState, suspendTerminal, resumeTerminal, closeTerminal, spawnTerminalDetailed, toIpcError } from "./tauri";
import type { SessionProcessState, TerminalSession } from "./types";
import { safeRandomUUID } from "./uuid";

export type { SessionProcessState } from "./types";
export type SessionLifecycleAction = "suspend" | "resume" | "restart" | "hibernate";

export const STANDBY_BACKEND_PREFIX = "standby:";
/**
 * Per-session cap on the scrollback carried in the persisted session state.
 *
 * This text is only used to redisplay recent output when a suspended session is reopened, but it
 * lives inside `session_state.json`, which is saved by rewriting the WHOLE file atomically. At the
 * previous 256 KB the file reached 8.4 MB across 63 sessions -- 97% of it scrollback -- and a burst
 * of session churn rewrote it about 255 times in 142 seconds, dirtying 2.1 GB and tripping macOS's
 * disk-write limit while the app sat unresponsive.
 *
 * 24 KB still covers several screens of a standard terminal, which is all the redisplay needs, and
 * it bounds the whole file to well under 2 MB at the same session count.
 */
const MAX_PERSISTED_SCROLLBACK_CHARS = 24_000;

export function createStandbyBackendSessionId(sessionId: string): string {
  return `${STANDBY_BACKEND_PREFIX}${sessionId}`;
}

export function isStandbyBackendSessionId(sessionId: string | null | undefined): boolean {
  return typeof sessionId === "string" && sessionId.startsWith(STANDBY_BACKEND_PREFIX);
}

export function getSessionProcessState(session: TerminalSession | null | undefined): SessionProcessState {
  if (!session) return "standby";
  if (session.backendSessionId && !isStandbyBackendSessionId(session.backendSessionId)) {
    if (isSessionSleeping(session.id) || session.processState === "suspended") {
      return "suspended";
    }
    return "running";
  }
  if (isStandbyBackendSessionId(session.backendSessionId)) {
    if (session.processState === "suspended") return "suspended";
    return session.processState === "hibernated" ? "hibernated" : "standby";
  }
  if (session.processState === "suspended") return "suspended";
  if (isSessionSleeping(session.id) || session.processState === "hibernated") return "hibernated";
  return "standby";
}

type RegisteredSession = {
  session: TerminalSession;
  active: boolean;
  idleSince: number | null;
  /** Last activity state observed for this session; a working session is never a sweep candidate. */
  activityState: TerminalActivityState | "idle" | "blocked" | null;
};

const registeredSessions = new Map<string, RegisteredSession>();
const recentScrollbackBySessionId = new Map<string, string>();
const sleepingSessionIds = new Set<string>();
const manualHibernateHoldIds = new Set<string>();
const sleepingListeners = new Set<() => void>();
const actionListeners = new Set<(action: SessionLifecycleAction, sessionId: string) => void>();
const inFlightResumes = new Map<string, Promise<void>>();
/** `${sessionId}\0${backendSessionId}` pairs whose suspension was already read from the daemon. */
const reconciledSuspendBindings = new Set<string>();
const suspensionDetails = new Map<string, import("./tauri").TerminalDescribeResult>();
export function getDaemonSuspension(sessionId: string): import("./tauri").TerminalDescribeResult | undefined {
  return suspensionDetails.get(sessionId);
}
let sleepingSnapshot = "";
let monitoringStarted = false;
let idleSweepTimer: ReturnType<typeof setInterval> | null = null;

export type SessionRebindHandler = (
  sessionId: string,
  backendSessionId: string,
  cwd?: string,
  daemonEpoch?: string | null,
  incarnation?: string | null,
) => Promise<void> | void;

let globalRebindHandler: SessionRebindHandler | null = null;

export function setSessionRebindHandler(handler: SessionRebindHandler | null): void {
  globalRebindHandler = handler;
}

function emitSleepingChange(): void {
  sleepingSnapshot = [...sleepingSessionIds].sort().join("\u0000");
  for (const listener of [...sleepingListeners]) {
    if (sleepingListeners.has(listener)) listener();
  }
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
    activityState: activityState ?? previous?.activityState ?? null,
  });
  ensureLifecycleMonitoring();
  reconcileDaemonSuspension(session);
}

/**
 * The sleeping set and `processState` live only in this GUI process, but the suspended process
 * lives in the daemon. After a GUI restart (or a suspend issued by another client) the pane would
 * otherwise render a stopped process as running: no overlay, no auto-resume, a frozen terminal.
 * Ask the daemon once per backend binding and adopt its kernel-observed answer.
 */
function reconcileDaemonSuspension(session: TerminalSession): void {
  const backendSessionId = session.backendSessionId;
  if (!backendSessionId || isStandbyBackendSessionId(backendSessionId)) return;
  if (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)) return;
  const key = `${session.id}\u0000${backendSessionId}\u0000${session.incarnation ?? ""}\u0000${session.daemonEpoch ?? ""}`;
  if (reconciledSuspendBindings.has(key)) return;
  reconciledSuspendBindings.add(key);
  void Promise.resolve()
    .then(() => describeTerminal(backendSessionId))
    .then((details) => {
      if (!details) return;
      const entry = registeredSessions.get(session.id);
      // The pane may have been rebound or resumed while the query was in flight.
      if (!entry || entry.session.backendSessionId !== backendSessionId ||
        entry.session.incarnation !== session.incarnation || entry.session.daemonEpoch !== session.daemonEpoch ||
        inFlightResumes.has(session.id)) return;
      suspensionDetails.set(session.id, details);
      const stopped = details.suspended === true || details.kernelStopped === true || details.readerPaused === true;
      entry.session = { ...entry.session, processState: stopped ? "suspended" : "running" };
      entry.idleSince = null;
      setSessionSleeping(session.id, stopped);
    })
    .catch((error) => {
      // Unknown is not suspended; allow a later registration to ask again.
      reconciledSuspendBindings.delete(key);
      console.warn("Failed to read daemon suspension state:", error);
    });
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
  entry.activityState = state;
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

export async function suspendRegisteredSession(sessionId: string): Promise<void> {
  const entry = registeredSessions.get(sessionId);
  if (!entry) {
    setSessionSleeping(sessionId, true);
    return;
  }
  const session = entry.session;
  if (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)) return;
  const backendSessionId = session.backendSessionId;
  if (!backendSessionId || isStandbyBackendSessionId(backendSessionId)) {
    entry.session = { ...session, processState: "suspended" };
    setSessionSleeping(sessionId, true);
    return;
  }

  await captureRecentScrollback(sessionId, backendSessionId);
  setSessionSleeping(sessionId, true);
  try {
    await suspendTerminal(backendSessionId);
  } catch (error) {
    setSessionSleeping(sessionId, false);
    throw error;
  }
  entry.session = { ...session, processState: "suspended" };
  entry.idleSince = null;
}

export async function resumeRegisteredSession(sessionId: string): Promise<void> {
  const existing = inFlightResumes.get(sessionId);
  if (existing) return existing;

  const promise = (async () => {
    const entry = registeredSessions.get(sessionId);
    manualHibernateHoldIds.delete(sessionId);
    if (!entry) {
      setSessionSleeping(sessionId, false);
      return;
    }
    const session = entry.session;
    const backendSessionId = session.backendSessionId;
    if (!backendSessionId || isStandbyBackendSessionId(backendSessionId)) {
      setSessionSleeping(sessionId, false);
      entry.session = { ...session, processState: "standby" };
      return;
    }

    try {
      await resumeTerminal(backendSessionId);
      if (registeredSessions.get(sessionId) !== entry || entry.session.backendSessionId !== backendSessionId ||
        entry.session.incarnation !== session.incarnation || entry.session.daemonEpoch !== session.daemonEpoch) return;
      suspensionDetails.delete(sessionId);
      setSessionSleeping(sessionId, false);
      entry.session = { ...session, processState: "running" };
      entry.idleSince = Date.now();
    } catch (error) {
      const ipcError = toIpcError(error);
      if (ipcError.code === "SESSION_NOT_FOUND" || ipcError.code === "PROCESS_NOT_FOUND") {
        setSessionSleeping(sessionId, false);
        suspensionDetails.delete(sessionId);
        entry.session = {
          ...session,
          backendSessionId: null,
          processState: "standby",
          lifecycle: "exited",
        };
      }
      throw error;
    }
  })();

  inFlightResumes.set(sessionId, promise);
  try {
    await promise;
  } finally {
    inFlightResumes.delete(sessionId);
  }
}

export async function restartRegisteredSession(
  sessionId: string,
  onRebind?: SessionRebindHandler,
): Promise<void> {
  const entry = registeredSessions.get(sessionId);
  manualHibernateHoldIds.delete(sessionId);
  if (!entry) {
    setSessionSleeping(sessionId, false);
    return;
  }
  const session = entry.session;
  const oldBackendId = session.backendSessionId;
  if (oldBackendId && !isStandbyBackendSessionId(oldBackendId)) {
    try {
      await closeTerminal(oldBackendId);
    } catch (err) {
      console.warn(`Failed to close old terminal ${oldBackendId} during restart:`, err);
    }
  }

  setSessionSleeping(sessionId, false);

  if (!isRemoteWorkspaceId(session.workspaceId) && !isPairedWorkspaceId(session.workspaceId)) {
    try {
      const clientRequestId = `restart-${sessionId}-${safeRandomUUID()}`;
      const spawnResult = await spawnTerminalDetailed({
        workspaceId: session.workspaceId,
        worktree: session.worktree,
        cwd: session.cwd,
        clientRequestId,
        startup: null,
      });
      entry.session = {
        ...session,
        backendSessionId: spawnResult.sessionId,
        processState: "running",
        lifecycle: "working",
      };
      entry.idleSince = Date.now();

      const rebind = onRebind ?? globalRebindHandler;
      if (rebind) {
        if (spawnResult.session.incarnation != null) {
          await rebind(
            sessionId,
            spawnResult.sessionId,
            spawnResult.session.cwd ?? session.cwd,
            spawnResult.daemonEpoch,
            spawnResult.session.incarnation,
          );
        } else {
          await rebind(
            sessionId,
            spawnResult.sessionId,
            spawnResult.session.cwd ?? session.cwd,
            spawnResult.daemonEpoch,
          );
        }
      }
    } catch (error) {
      entry.session = {
        ...session,
        backendSessionId: null,
        processState: "standby",
        lifecycle: "failed",
      };
      throw error;
    }
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
  // 0 (or negative) is the explicit "off" setting: never auto-suspend.
  if (timeoutMs <= 0) return Promise.resolve();
  const now = Date.now();
  const candidates = [...registeredSessions.entries()].filter(([, entry]) =>
    !entry.active &&
    entry.idleSince !== null &&
    now - entry.idleSince >= timeoutMs &&
    // A session whose latest known activity is working/waiting/blocked is by
    // definition not idle, even if a stale idleSince survived an event gap.
    entry.activityState !== "working" &&
    entry.activityState !== "waiting" &&
    entry.activityState !== "blocked" &&
    Boolean(entry.session.backendSessionId) &&
    !isStandbyBackendSessionId(entry.session.backendSessionId) &&
    !isRemoteWorkspaceId(entry.session.workspaceId) &&
    !isPairedWorkspaceId(entry.session.workspaceId),
  );
  // Ground truth before pulling the trigger: recent PTY output vetoes
  // suspension regardless of what any screen/extension classifier reported.
  // A working agent keeps writing (spinners, redraws), so this closes the
  // "working session got auto-suspended" hole. Query failure also vetoes:
  // never suspend a session whose activity we could not verify.
  const verdicts = await Promise.all(
    candidates.map(async ([sessionId, entry]) => {
      const backendSessionId = entry.session.backendSessionId;
      if (!backendSessionId || isStandbyBackendSessionId(backendSessionId)) return null;
      try {
        const details = await describeTerminal(backendSessionId);
        if (!details) return null;
        const lastOutputAgeMs = details.lastOutputAgeMs;
        if (lastOutputAgeMs != null && lastOutputAgeMs < timeoutMs) return null;
        return sessionId;
      } catch {
        return null;
      }
    }),
  );
  const confirmed = verdicts.filter((sessionId): sessionId is string => sessionId !== null);
  await Promise.allSettled(confirmed.map((sessionId) => suspendRegisteredSession(sessionId)));
}

function ensureLifecycleMonitoring(): void {
  if (monitoringStarted) return;
  monitoringStarted = true;
  void Promise.resolve(onNativeTerminalAgentState((payload) => {
    const entry = findRegisteredByBackend(payload.sessionId);
    if (!entry) return;
    entry.activityState = payload.state;
    entry.idleSince = payload.state === "idle" ? entry.idleSince ?? Date.now() : null;
  })).catch(() => undefined);
  idleSweepTimer = setInterval(() => {
    void sweepIdleSessions();
  }, 30_000);
}

export function requestSessionLifecycleAction(action: SessionLifecycleAction, sessionId: string): void {
  if (action === "suspend" || action === "hibernate") {
    manualHibernateHoldIds.add(sessionId);
  } else {
    manualHibernateHoldIds.delete(sessionId);
  }
  for (const listener of actionListeners) listener(action, sessionId);
  if (action === "suspend") {
    void suspendRegisteredSession(sessionId).catch((error) => {
      manualHibernateHoldIds.delete(sessionId);
      console.warn(`Failed to suspend session`, error);
    });
  } else if (action === "resume") {
    void resumeRegisteredSession(sessionId).catch((error) => {
      console.warn(`Failed to resume session`, error);
    });
  } else if (action === "restart") {
    void restartRegisteredSession(sessionId).catch((error) => {
      console.warn(`Failed to restart session`, error);
    });
  } else if (action === "hibernate") {
    void hibernateRegisteredSession(sessionId).catch((error) => {
      manualHibernateHoldIds.delete(sessionId);
      console.warn(`Failed to hibernate session`, error);
    });
  }
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
  inFlightResumes.clear();
  reconciledSuspendBindings.clear();
  suspensionDetails.clear();
  sleepingSnapshot = "";
  monitoringStarted = false;
  if (idleSweepTimer) clearInterval(idleSweepTimer);
  idleSweepTimer = null;
}
