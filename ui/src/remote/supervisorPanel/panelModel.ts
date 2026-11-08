// What the phone panel renders, derived from one PanelSnapshot: each session appears in exactly
// one section (decisions, handling, sessions), and `panel.*` owner frames fold into the snapshot.
import type {
  ActionEntry,
  Escalation,
  FleetSession,
  PanelEvent,
  PanelSnapshot,
  PendingPrompt,
  SessionRef,
} from "../../lib/supervisorPanel/contract";

// The phone folds frames with the same reducer the desktop does.
export { applyPanelEvent } from "../../lib/supervisorPanel/model";

export type PanelSections = {
  readonly decisions: readonly Escalation[];
  readonly handling: readonly ActionEntry[];
  /** Sessions that are neither waiting on a decision nor under live handling. */
  readonly sessions: readonly FleetSession[];
  readonly log: readonly ActionEntry[];
};

export function panelSections(snapshot: PanelSnapshot): PanelSections {
  const decided = new Set(snapshot.decisions.map((decision) => decision.sessionRef));
  const handled = new Set<SessionRef>();
  for (const entry of snapshot.handling) if (entry.sessionRef) handled.add(entry.sessionRef);
  return {
    decisions: [...snapshot.decisions].sort((a, b) => a.createdAtMs - b.createdAtMs),
    handling: snapshot.handling,
    sessions: snapshot.sessions.filter((session) => !decided.has(session.sessionRef) && !handled.has(session.sessionRef)),
    log: snapshot.log,
  };
}

/** The recommended option first, then the rest in their own order. */
export function orderedOptions(prompt: PendingPrompt, recommendedIndex: number): Array<{ index: number; label: string; recommended: boolean }> {
  const all = prompt.options.map((label, index) => ({ index, label, recommended: index === recommendedIndex }));
  return [...all.filter((option) => option.recommended), ...all.filter((option) => !option.recommended)];
}

export type SessionStatus = { readonly kind: "working" | "waiting" | "idle"; readonly label: string };

/** Unknown state is Idle, said plainly; never a guess of Working. */
export function sessionStatus(session: FleetSession): SessionStatus {
  if (session.stale) return { kind: "idle", label: "Offline" };
  switch (session.state) {
    case "working":
      return { kind: "working", label: "Working" };
    case "blocked":
      return { kind: "waiting", label: "Waiting" };
    case "idle":
      return { kind: "idle", label: "Idle" };
    default:
      return { kind: "idle", label: "Idle · status unknown" };
  }
}

export function statusLine(snapshot: PanelSnapshot): string {
  if (snapshot.paused) return "Paused · the supervisor is not acting";
  const decided = new Set(snapshot.decisions.map((decision) => decision.sessionRef));
  const decisions = snapshot.decisions.length;
  if (decisions > 0) return decisions === 1 ? "1 decision needs you" : `${decisions} decisions need you`;
  // A blocked session no card covers is still the owner's work: never report all-clear for it.
  const waiting = snapshot.sessions.filter(
    (session) => !decided.has(session.sessionRef) && !session.stale && (session.state === "blocked" || session.health?.type === "failed"),
  ).length;
  if (waiting > 0) {
    // A prompt the hub holds but has not escalated means the supervisor is still deciding, not
    // that the lane is stuck; say so instead of reporting a silent wait.
    const judging = snapshot.sessions.filter(
      (session) => !decided.has(session.sessionRef) && !session.stale && session.pendingPrompt !== null,
    ).length;
    const tail = judging > 0 ? " The supervisor is still deciding." : "";
    return (waiting === 1 ? "1 lane is waiting for you" : `${waiting} lanes are waiting for you`) + "." + tail;
  }
  const handling = snapshot.handling.length;
  if (handling > 0) return handling === 1 ? "Handling 1 problem" : `Handling ${handling} problems`;
  const working = snapshot.sessions.filter((session) => session.state === "working" && !session.stale).length;
  if (working > 0) return `All good · ${working} working`;
  return "All quiet";
}

export function rulesLine(count: number): string {
  return count === 1 ? "1 standing rule" : `${count} standing rules`;
}

function objectOf(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

/** A `panel.*` frame from the events socket (raw text or parsed), or null for any other frame. */
export function parsePanelFrame(frame: unknown): PanelEvent | null {
  let value: unknown = frame;
  if (typeof frame === "string") {
    try {
      value = JSON.parse(frame);
    } catch {
      return null;
    }
  }
  const object = objectOf(value);
  const type = object?.type;
  if (typeof type !== "string" || !type.startsWith("panel.")) return null;
  switch (type) {
    case "panel.decision":
      return typeof object?.sessionRef === "string" ? (object as unknown as PanelEvent) : null;
    case "panel.action":
      return objectOf(object?.action) ? (object as unknown as PanelEvent) : null;
    case "panel.session":
      return objectOf(object?.session) ? (object as unknown as PanelEvent) : null;
    case "panel.rules":
      return Array.isArray(object?.rules) ? (object as unknown as PanelEvent) : null;
    default:
      return null;
  }
}
