// The attention group a session is drawn under. The daemon judges it once (Rust
// `attention_group` in crates/ferryx-hub/src/supervisor/panel_contract.rs); every surface reads it
// through `groupOf`. The state mapping below exists only for older peers and for panes the daemon
// sent no entry for, and it is the only state-to-group mapping in ui/src.
import type { TerminalActivity } from "./activity";
import type { Attention, AttentionGroup, FleetSession } from "./supervisorPanel/contract";

export type { Attention, AttentionGroup } from "./supervisorPanel/contract";

/** The daemon's judgment per backend session id; a session without one is absent. */
export type AttentionBySessionId = Readonly<Record<string, Pick<Attention, "group">>>;

export type GroupSource = {
  /** The daemon's judgment; wins whenever present. */
  readonly attention?: Pick<Attention, "group"> | null;
  /** Old-peer fallback: a FleetSession state or a local pane activity state. */
  readonly state?: string | null;
  /** A local pane activity the owner has already looked at. */
  readonly seen?: boolean;
  /** A fresh failed health report (old peers only; the daemon folds health into its axes). */
  readonly failed?: boolean;
};

export function groupOf(source: GroupSource): AttentionGroup {
  if (source.attention) return source.attention.group;
  if (source.failed) return "needsYou";
  switch (source.state) {
    case "working":
      return "working";
    case "blocked":
    case "waiting":
      return source.seen ? "seen" : "needsYou";
    case "done":
      return source.seen ? "seen" : "unread";
    default:
      return "seen";
  }
}

/** The pane activity state a group is drawn with; a seen session draws no state. */
export function activityStateOfGroup(group: AttentionGroup): "working" | "waiting" | "done" | null {
  switch (group) {
    case "needsYou":
      return "waiting";
    case "unread":
      return "done";
    case "working":
      return "working";
    case "seen":
      return null;
  }
}

/** The backend session id a `<hostId>:<backendSessionId>` ref names. */
export function backendSessionIdOf(sessionRef: string): string {
  return sessionRef.slice(sessionRef.indexOf(":") + 1);
}

/** The judged sessions of a panel snapshot, keyed by backend session id. */
export function attentionBySessionIdOf(sessions: readonly FleetSession[]): AttentionBySessionId {
  const result: Record<string, Pick<Attention, "group">> = {};
  for (const session of sessions) {
    if (session.attention) result[backendSessionIdOf(session.sessionRef)] = { group: session.attention.group };
  }
  return result;
}

/**
 * A pane's activity with the daemon's judgment for its backend session laid over it. A judged
 * session the pane has no local activity for still shows (as the state its group draws); a seen
 * one needs no entry.
 */
export function withAttention(
  activity: TerminalActivity | undefined,
  attention: Pick<Attention, "group"> | undefined,
): TerminalActivity | undefined {
  if (!attention) return activity;
  if (activity) return { ...activity, attention };
  const state = activityStateOfGroup(attention.group);
  return state === null ? undefined : { state, title: "", isAgent: true, attention };
}
