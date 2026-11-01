// The hub supervisor's pure model: snapshot shape, per-pane derivation, and expiry timing.
// No Tauri or transport imports, so the desktop feed and the remote phone client share it.
import { normalizeUnified, type UnifiedSupervisorView } from "./unifiedSupervisor";
/** How long a supervisor keystroke keeps its pane bar in the active fill. */
export const SUPERVISOR_ACTING_MS = 8_000;
/** A refusal because a person typed stays visible this long (HUMAN_TYPING_WINDOW_MS plus margin). */
export const SUPERVISOR_HELD_MS = 5_000;

export type SupervisorGoalMode = "off" | "supervised" | "full";
export type SupervisorRequirementStatus = "pending" | "met" | "unmet" | "waived";

/** The lane's goal contract as the daemon reports it (SupervisedLane.goal_contract, camelCase). */
export type SupervisorGoalView = {
  mode: SupervisorGoalMode;
  turnsUsed: number;
  turnLimit: number;
  revision: number;
  requirements: Array<{ id: string; text: string; verifier: string; status: SupervisorRequirementStatus; evidenceRefs: string[] }>;
  stopReason: string | null;
  proposal?: { text: string; state: string; gapIds: string[] } | null;
  pendingRevisions: Array<{ id: number; change: unknown; instructionQuote: string }>;
};

/** Every StopReason string the hub emits, mirrored from the Rust enum. */
export const SUPERVISOR_STOP_REASONS = [
  "no_further_progress",
  "high_stakes_user_required",
  "completion_evidence_missing",
  "turn_limit",
  "delivery_uncertain",
  "judge_unavailable",
  "supervisor_degraded",
] as const;
export type SupervisorStopReason = (typeof SUPERVISOR_STOP_REASONS)[number];

export const SUPERVISOR_STOP_REASON_LABELS: Record<SupervisorStopReason, string> = {
  no_further_progress: "Supervisor stopped: no further progress",
  high_stakes_user_required: "Needs your decision",
  completion_evidence_missing: "Done claimed without proof",
  turn_limit: "Turn limit reached",
  delivery_uncertain: "Instruction may not have arrived",
  judge_unavailable: "Supervisor checker unavailable",
  supervisor_degraded: "Supervisor engine degraded",
};
export const SUPERVISOR_AWAITING_APPROVAL_LABEL = "Next step ready for review";
/** An unknown reason from a newer daemon still reads as a stop. */
export const SUPERVISOR_STOPPED_FALLBACK_LABEL = "Supervisor stopped";

/**
 * The one label a supervisor hand-off shows; a stop outranks an awaiting proposal. A string
 * reason always yields a label; null with no awaiting proposal yields null.
 */
export function supervisorStopReasonLabel(stopReason: string, awaitingApproval?: boolean | "awaiting_approval"): string;
export function supervisorStopReasonLabel(
  stopReason: string | null | undefined,
  awaitingApproval?: boolean | "awaiting_approval",
): string | null;
export function supervisorStopReasonLabel(
  stopReason: string | null | undefined,
  awaitingApproval: boolean | "awaiting_approval" = false,
): string | null {
  if (typeof stopReason === "string" && stopReason !== "") {
    return (SUPERVISOR_STOP_REASON_LABELS as Record<string, string>)[stopReason] ?? SUPERVISOR_STOPPED_FALLBACK_LABEL;
  }
  if (awaitingApproval === true || awaitingApproval === "awaiting_approval") return SUPERVISOR_AWAITING_APPROVAL_LABEL;
  return typeof stopReason === "string" ? SUPERVISOR_STOPPED_FALLBACK_LABEL : null;
}

export type SupervisorHandoff = {
  laneKey: string;
  laneTitle: string;
  /** The hub's PTY session id the lane drives. */
  sessionId: string;
  revision: number;
  stopReason: string | null;
  awaitingApproval: boolean;
};

/** Lanes that currently hand something to the owner: a stop, or a proposal awaiting approval. */
export function supervisorHandoffs(snapshot: SupervisorSnapshot): SupervisorHandoff[] {
  const handoffs: SupervisorHandoff[] = [];
  for (const lane of snapshot.lanes) {
    const contract = lane.goalContract;
    if (!contract || typeof contract !== "object" || !lane.sessionId) continue;
    const stopReason = typeof contract.stopReason === "string" && contract.stopReason ? contract.stopReason : null;
    const awaitingApproval = proposalAwaitingApproval(lane);
    if (!stopReason && !awaitingApproval) continue;
    handoffs.push({
      laneKey: lane.key,
      laneTitle: typeof lane.title === "string" ? lane.title : lane.key,
      sessionId: lane.sessionId,
      revision: Number.isSafeInteger(contract.revision) ? contract.revision : 0,
      stopReason,
      awaitingApproval,
    });
  }
  return handoffs;
}

export function proposalAwaitingApproval(lane: SupervisorLane | undefined): boolean {
  return lane?.goalContract?.proposal?.state === "awaiting_approval";
}

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
  /** The lane's goal contract; absent for lanes without one and for older daemons. */
  goalContract?: SupervisorGoalView | null;
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
  hub: SupervisorHubState;
  lanes: SupervisorLane[];
  watched: string[];
  /** Newest first. */
  actions: SupervisorAction[];
  /** The unified supervisor (identity, engine health, projects, goals); null or absent for an older daemon or a closed hub. */
  supervisor?: UnifiedSupervisorView | null;
};

/** The account supervisor lease role this machine reports (`supervisor_snapshot` hub.role). */
export type SupervisorLeaseRole = "holder" | "provisional" | "standby" | "unleased";

/** An open goal of this machine's archived pre-lease supervisor the owner may adopt. */
export type ArchivedGoal = { goalId: string; title: string; supervisorId: string; fromMachine: string };

export type SupervisorHubState = {
  available: boolean;
  running: boolean;
  error?: string | null;
  /** Absent from an older daemon: it runs its own supervisor as before. */
  role?: SupervisorLeaseRole | null;
  holderMachineId?: string | null;
  holderLabel?: string | null;
  /** False when this standby holds no agent-control grant on the holder. */
  holderPaired?: boolean | null;
  /** Host ids an imported checkpoint names that this machine cannot map. */
  unmappedHosts?: string[];
  /** Open goals of this machine's archived supervisor, not adopted yet. */
  adoptable?: ArchivedGoal[];
  /** Lease fence term this machine last saw. Absent from a daemon without the lease status. */
  fence?: number;
  /** Seconds until the holder's lease expires unless renewed. */
  leaseExpiresIn?: number;
  /** The newest supervisor checkpoint this machine stores. Absent: none or an older daemon. */
  checkpointId?: string;
  /** Age of that checkpoint in seconds. */
  checkpointAge?: number;
  /** Pane observations this standby dropped because no paired holder took them. */
  observeDropped?: number;
  /** Why this standby would not take over now. Absent: nothing blocks a promotion. */
  promotionBlockReason?: string;
  /** Machines the holder could not push its fence to; their sessions wait. */
  unreachableHosts?: string[];
};

const LEASE_ROLES: ReadonlySet<string> = new Set(["holder", "provisional", "standby", "unleased"]);

/** Standby or unleased: another computer runs the account supervisor; this one must not. */
export function isStandbyRole(role: SupervisorLeaseRole | null | undefined): boolean {
  return role === "standby" || role === "unleased";
}

/** A lease warning a surface shows next to the machine that reported it. */
export type LeaseNotice = { kind: "unreachable-hosts" | "observe-dropped" | "promotion-blocked"; text: string };

/**
 * Warnings from the lease status of one machine (`here` names it: "This computer" or its
 * label). Fields the daemon did not send add nothing.
 */
export function leaseNotices(hub: SupervisorHubState, here: string): LeaseNotice[] {
  const notices: LeaseNotice[] = [];
  const unreachable = hub.unreachableHosts ?? [];
  if (unreachable.length > 0) {
    notices.push({ kind: "unreachable-hosts", text: `The supervisor cannot reach ${unreachable.join(", ")}. Sessions there wait until it is reachable again.` });
  }
  const dropped = hub.observeDropped ?? 0;
  if (dropped > 0) {
    notices.push({ kind: "observe-dropped", text: `${here} dropped ${dropped} pane update${dropped === 1 ? "" : "s"} because no paired supervisor took ${dropped === 1 ? "it" : "them"}.` });
  }
  if (hub.promotionBlockReason) {
    notices.push({ kind: "promotion-blocked", text: `${here} will not take over the supervisor: ${hub.promotionBlockReason}` });
  }
  return notices;
}

function optionalText(value: unknown): string | null {
  return typeof value === "string" && value ? value : null;
}

function optionalCount(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
}

/** A block reason as text: the daemon may send the sentence or `{code, message}`. */
function blockReason(value: unknown): string | undefined {
  const text = optionalText(value);
  if (text) return text;
  const raw = value && typeof value === "object" ? (value as Record<string, unknown>) : null;
  return optionalText(raw?.message) ?? optionalText(raw?.reason) ?? optionalText(raw?.code) ?? undefined;
}

/**
 * Host labels from `["host"]`, `[[host, reason]]` or `[{host, label, reason}]`; a null host is
 * this machine's daemon.
 */
function hostList(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  return value.flatMap((item): string[] => {
    if (typeof item === "string") return item ? [item] : [];
    if (Array.isArray(item)) {
      const host = optionalText(item[0]);
      return host ? [host] : item[0] === null ? ["this computer"] : [];
    }
    const raw = item && typeof item === "object" ? (item as Record<string, unknown>) : null;
    if (!raw) return [];
    const host = optionalText(raw.label) ?? optionalText(raw.host);
    return host ? [host] : raw.host === null ? ["this computer"] : [];
  });
}

/** The optional lease status fields, each omitted when the daemon did not send it. */
function leaseStatus(raw: Record<string, unknown>): Partial<SupervisorHubState> {
  const status: Partial<SupervisorHubState> = {};
  const fence = optionalCount(raw.fence);
  if (fence !== undefined) status.fence = fence;
  const leaseExpiresIn = optionalCount(raw.leaseExpiresIn);
  if (leaseExpiresIn !== undefined) status.leaseExpiresIn = leaseExpiresIn;
  const checkpointId = optionalText(raw.checkpointId);
  if (checkpointId) status.checkpointId = checkpointId;
  const checkpointAge = optionalCount(raw.checkpointAge);
  if (checkpointAge !== undefined) status.checkpointAge = checkpointAge;
  const observeDropped = optionalCount(raw.observeDropped);
  if (observeDropped !== undefined) status.observeDropped = observeDropped;
  const promotionBlockReason = blockReason(raw.promotionBlockReason);
  if (promotionBlockReason) status.promotionBlockReason = promotionBlockReason;
  const unreachableHosts = hostList(raw.unreachableHosts);
  if (unreachableHosts) status.unreachableHosts = unreachableHosts;
  return status;
}

function normalizeHub(value: unknown): SupervisorHubState {
  const raw = (value && typeof value === "object" ? value : {}) as Record<string, unknown>;
  const role = typeof raw.role === "string" && LEASE_ROLES.has(raw.role) ? (raw.role as SupervisorLeaseRole) : null;
  const adoptable = Array.isArray(raw.adoptable)
    ? raw.adoptable.flatMap((item): ArchivedGoal[] => {
        const goal = (item && typeof item === "object" ? item : {}) as Record<string, unknown>;
        const goalId = optionalText(goal.goalId);
        if (!goalId) return [];
        return [{ goalId, title: optionalText(goal.title) ?? goalId, supervisorId: optionalText(goal.supervisorId) ?? "", fromMachine: optionalText(goal.fromMachine) ?? "" }];
      })
    : [];
  return {
    available: Boolean(raw.available),
    running: Boolean(raw.running),
    error: typeof raw.error === "string" ? raw.error : null,
    role,
    holderMachineId: optionalText(raw.holderMachineId),
    holderLabel: optionalText(raw.holderLabel),
    holderPaired: typeof raw.holderPaired === "boolean" ? raw.holderPaired : null,
    unmappedHosts: Array.isArray(raw.unmappedHosts) ? raw.unmappedHosts.filter((host): host is string => typeof host === "string" && host !== "") : [],
    adoptable,
    ...leaseStatus(raw),
  };
}

export type PaneSupervision =
  | { kind: "acting"; text: string | null; atMs: number; lane?: SupervisorLane }
  | { kind: "held"; atMs: number; lane?: SupervisorLane }
  | { kind: "paused"; lane: SupervisorLane }
  | { kind: "awaiting-approval"; lane: SupervisorLane }
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
  // A proposed next step waits for the owner; a stopped goal is left to them with its reason.
  if (lane && proposalAwaitingApproval(lane)) return { kind: "awaiting-approval", lane };
  if (needsYou || lane?.goalContract?.stopReason) return { kind: "left-to-you", lane };
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
    hub: normalizeHub(raw.hub),
    lanes: Array.isArray(raw.lanes)
      ? raw.lanes.map((lane) => ({ ...lane, lastReason: typeof lane?.lastReason === "string" ? lane.lastReason : null }))
      : [],
    watched: Array.isArray(raw.watched) ? raw.watched : [],
    actions: Array.isArray(raw.actions) ? raw.actions : [],
    supervisor: normalizeUnified((value as { supervisor?: unknown }).supervisor),
  };
}
