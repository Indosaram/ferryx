// The hub supervisor's pure model: snapshot shape, per-pane derivation, and expiry timing.
// No Tauri or transport imports, so the desktop feed and the remote phone client share it.
/** How long a supervisor keystroke keeps its pane bar in the active fill. */
export const SUPERVISOR_ACTING_MS = 8_000;
/** A refusal because a person typed stays visible this long (HUMAN_TYPING_WINDOW_MS plus margin). */
export const SUPERVISOR_HELD_MS = 5_000;

export type SupervisorLane = {
  key: string;
  title: string;
  status: string;
  sessionId: string | null;
  cwd: string;
  goal: string | null;
  paused: boolean;
  lastActivityAt: string;
  /** Why the hub last acted or held off on this lane; null when it has not said. */
  lastReason?: string | null;
};

export type SupervisorAction = {
  deviceId: string;
  op: string;
  sessionId: string | null;
  requestId: string | null;
  code: string;
  atMs: number;
  text: string | null;
};

export type SupervisorSnapshot = {
  hub: { available: boolean; running: boolean; error?: string | null };
  lanes: SupervisorLane[];
  watched: string[];
  /** Newest first. */
  actions: SupervisorAction[];
};

export type PaneSupervision =
  | { kind: "acting"; text: string | null; atMs: number; lane?: SupervisorLane }
  | { kind: "held"; atMs: number; lane?: SupervisorLane }
  | { kind: "paused"; lane: SupervisorLane }
  | { kind: "left-to-you"; lane?: SupervisorLane }
  | { kind: "watching"; lane?: SupervisorLane };

export const EMPTY_SNAPSHOT: SupervisorSnapshot = {
  hub: { available: false, running: false },
  lanes: [],
  watched: [],
  actions: [],
};

/** Refusal code the daemon records when a person typed in the pane (CONTROL_CONFLICT). */
const HUMAN_TYPING_CODE = "CONTROL_CONFLICT";

/**
 * What one pane's bar says. Paused beats everything: it is the person's own decision. Then a
 * hand-off, then a refusal because the person is typing, then a fresh keystroke, then watching.
 * A pane the supervisor neither drives nor watches gets nothing.
 */
export function paneSupervision(
  snapshot: SupervisorSnapshot,
  sessionId: string | undefined | null,
  now: number,
  needsYou = false,
): PaneSupervision | null {
  if (!sessionId || !snapshot.hub.available) return null;
  const lane = snapshot.lanes.find((item) => item.sessionId === sessionId);
  const watched = snapshot.watched.includes(sessionId);
  const latest = snapshot.actions.find((action) => action.sessionId === sessionId);
  if (!lane && !watched && !latest) return null;
  if (lane?.paused) return { kind: "paused", lane };
  if (!lane && !watched) {
    // An input with no lane or watch is still the supervisor typing here; show it while fresh only.
    if (!latest || now - latest.atMs > SUPERVISOR_ACTING_MS) return null;
  }
  if (needsYou) return { kind: "left-to-you", lane };
  if (latest && latest.code === HUMAN_TYPING_CODE && now - latest.atMs <= SUPERVISOR_HELD_MS) {
    return { kind: "held", atMs: latest.atMs, lane };
  }
  if (latest && latest.code === "OK" && now - latest.atMs <= SUPERVISOR_ACTING_MS) {
    return { kind: "acting", text: latest.text, atMs: latest.atMs, lane };
  }
  // A stopped hub watches nothing; claiming "Supervised" there would be a lie.
  if (!snapshot.hub.running) return null;
  return { kind: "watching", lane };
}

/** Session ids the supervisor typed into within the acting window. */
export function actingSessionIds(snapshot: SupervisorSnapshot, now: number): Set<string> {
  const ids = new Set<string>();
  for (const action of snapshot.actions) {
    if (action.code === "OK" && action.sessionId && now - action.atMs <= SUPERVISOR_ACTING_MS) ids.add(action.sessionId);
  }
  return ids;
}

/** The next moment a derived state expires, so one timer can re-render exactly then. */
export function nextSupervisionExpiry(snapshot: SupervisorSnapshot, now: number): number | null {
  let next: number | null = null;
  for (const action of snapshot.actions) {
    const window = action.code === HUMAN_TYPING_CODE ? SUPERVISOR_HELD_MS : SUPERVISOR_ACTING_MS;
    const end = action.atMs + window;
    if (end > now && (next === null || end < next)) next = end;
  }
  return next;
}

export function normalizeSnapshot(value: unknown): SupervisorSnapshot {
  if (!value || typeof value !== "object") return EMPTY_SNAPSHOT;
  const raw = value as Partial<SupervisorSnapshot>;
  return {
    hub: {
      available: Boolean(raw.hub?.available),
      running: Boolean(raw.hub?.running),
      error: raw.hub?.error ?? null,
    },
    lanes: Array.isArray(raw.lanes)
      ? raw.lanes.map((lane) => ({ ...lane, lastReason: typeof lane?.lastReason === "string" ? lane.lastReason : null }))
      : [],
    watched: Array.isArray(raw.watched) ? raw.watched : [],
    actions: Array.isArray(raw.actions) ? raw.actions : [],
  };
}
