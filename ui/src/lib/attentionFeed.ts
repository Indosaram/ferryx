// The daemon's attention group per backend session, read from the same supervisor panel snapshot
// and `panel.session` frames the panel renders. The badge and the desktop notifications count
// from it; a session the daemon reports no attention for (an older peer) is absent here.
import { getSupervisorPanel, onSupervisorPanelEvent } from "./tauri";
import type { AttentionGroup, FleetSession, PanelEvent } from "./supervisorPanel/contract";

export type DaemonAttention = {
  group: AttentionGroup;
  reason: string | null;
  updatedAtMs: number;
};

export type AttentionChange = {
  backendSessionId: string;
  previous: DaemonAttention | undefined;
  next: DaemonAttention | undefined;
};

const byBackendSessionId = new Map<string, DaemonAttention>();
/** `baseline` is true for the snapshot load: what was already there is state, not news. */
type AttentionListener = (changes: readonly AttentionChange[], baseline: boolean) => void;
const listeners = new Set<AttentionListener>();
let started: Promise<void> | null = null;

/** `<hostId>:<backendSessionId>` -> backend id; a relay host id (`https://.../host/<id>`) keeps its colons. */
export function backendSessionIdOfRef(sessionRef: string): string | null {
  const first = sessionRef.indexOf(":");
  if (first < 0) return null;
  let at = first;
  if (sessionRef.startsWith("://", first)) {
    const marker = sessionRef.indexOf("/host/", first + 3);
    if (marker < 0) return null;
    at = sessionRef.indexOf(":", marker + "/host/".length);
    if (at < 0) return null;
  }
  return sessionRef.slice(at + 1) || null;
}

export function getDaemonAttention(backendSessionId: string | undefined | null): DaemonAttention | undefined {
  return backendSessionId ? byBackendSessionId.get(backendSessionId) : undefined;
}

export function subscribeDaemonAttention(listener: AttentionListener): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function entryOf(session: FleetSession): DaemonAttention | undefined {
  const group = session.attention?.group;
  if (!group) return undefined;
  return { group, reason: session.reason ?? null, updatedAtMs: session.updatedAtMs };
}

/** Applies daemon sessions; an older report never replaces a newer one. Returns what changed. */
export function applyDaemonSessions(sessions: readonly FleetSession[], baseline = false): AttentionChange[] {
  const changes: AttentionChange[] = [];
  for (const session of sessions) {
    const backendSessionId = backendSessionIdOfRef(session.sessionRef);
    if (!backendSessionId) continue;
    const previous = byBackendSessionId.get(backendSessionId);
    if (previous && session.updatedAtMs < previous.updatedAtMs) continue;
    const next = entryOf(session);
    if (next) byBackendSessionId.set(backendSessionId, next);
    else byBackendSessionId.delete(backendSessionId);
    if (previous?.group === next?.group && previous?.reason === next?.reason) continue;
    changes.push({ backendSessionId, previous, next });
  }
  if (changes.length > 0) {
    for (const listener of [...listeners]) listener(changes, baseline);
  }
  return changes;
}

export function applyAttentionEvent(event: PanelEvent): AttentionChange[] {
  return event.type === "panel.session" ? applyDaemonSessions([event.session]) : [];
}

/** Loads the snapshot and follows `panel.session` frames, once per app. */
export function startDaemonAttentionFeed(): Promise<void> {
  if (started) return started;
  started = (async () => {
    try {
      // Subscribe first so a frame racing the load is not lost; the load never replaces it.
      await onSupervisorPanelEvent((event) => { applyAttentionEvent(event); });
      const snapshot = await getSupervisorPanel();
      if (Array.isArray(snapshot.sessions)) applyDaemonSessions(snapshot.sessions, true);
    } catch (error) {
      // A build or peer without the supervisor panel: every pane keeps its own state mapping.
      console.warn("daemon attention feed unavailable", error);
    }
  })();
  return started;
}

export function resetDaemonAttentionForTest(): void {
  byBackendSessionId.clear();
  listeners.clear();
  started = null;
}
