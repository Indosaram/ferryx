// The unified supervisor's pure model, shared by the desktop panel and the phone home: the
// snapshot's `supervisor` part (identity, engine health, projects, goals), the wait/next-action
// wording, request envelopes and typed results. No Tauri or transport imports.
//
// Wire shapes mirror the hub's request service (crates/ferryx-hub/src/supervisor/request.rs) and
// engine health (meta/mhc_supervisor.rs `SupervisorHealth`, meta/mhc_fault.rs `SupervisorFault`).

export type SupervisorRunState =
  | "ready"
  | "dispatching"
  | "running"
  | "waiting_children"
  | "waiting_event"
  | "awaiting_owner"
  | "verifying"
  | "completed"
  | "paused"
  | "degraded"
  | "delivery_uncertain"
  | "cancelled";

export type UnifiedWait = {
  waitId: string;
  kind: string;
  childGoalId: string | null;
  eventKey: string | null;
};

/** One goal as `goal_receipt` reports it. */
export type UnifiedGoal = {
  goalId: string;
  supervisorId: string;
  projectId: string;
  laneKey: string;
  title: string;
  runState: string;
  stateDetail: string | null;
  revision: number;
  pendingWaits: UnifiedWait[];
  nextTrigger: string | null;
  updatedAt: string;
  mode: string | null;
  turnLimit: number | null;
  turnsUsed: number | null;
  /** The persisted operation grant; null when the hub could not read it. */
  grant: string[] | null;
};

/** `SupervisorFault`, tagged by `kind`. */
export type EngineFault = { kind: string } & Record<string, unknown>;

/** `SupervisorHealth`, or null when this daemon runs no engine. */
export type EngineHealth = {
  state: string;
  generation: number;
  pid: number | null;
  capabilities: string[];
  serverVersion: string | null;
  fault: EngineFault | null;
  detail: string | null;
  queuedDecisions: number;
};

export type UnifiedProject = { projectId: string; name: string };

export type UnifiedSupervisorView = {
  identity: { supervisorId: string; ownerPrincipal: string } | null;
  projects: UnifiedProject[];
  goals: UnifiedGoal[];
  health: EngineHealth | null;
  /** The snapshot read failed: `{code, message}` from the hub. */
  error: { code: string; message: string } | null;
};

/** How a surface reaches the designated supervisor host right now. */
export type SupervisorConnection = "live" | "unavailable" | "disconnected";

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function text(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function integer(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) ? value : null;
}

function normalizeWait(value: unknown): UnifiedWait | null {
  const raw = record(value);
  const waitId = text(raw?.waitId);
  if (!raw || !waitId) return null;
  return { waitId, kind: text(raw.kind) ?? "", childGoalId: text(raw.childGoalId), eventKey: text(raw.eventKey) };
}

export function normalizeGoal(value: unknown): UnifiedGoal | null {
  const raw = record(value);
  const goalId = text(raw?.goalId);
  const revision = integer(raw?.revision);
  if (!raw || !goalId || revision === null) return null;
  return {
    goalId,
    supervisorId: text(raw.supervisorId) ?? "",
    projectId: text(raw.projectId) ?? "",
    laneKey: text(raw.laneKey) ?? "",
    title: text(raw.title) ?? "",
    runState: text(raw.runState) ?? "",
    stateDetail: text(raw.stateDetail),
    revision,
    pendingWaits: Array.isArray(raw.pendingWaits) ? raw.pendingWaits.flatMap((wait) => normalizeWait(wait) ?? []) : [],
    nextTrigger: text(raw.nextTrigger),
    updatedAt: text(raw.updatedAt) ?? "",
    mode: text(raw.mode),
    turnLimit: integer(raw.turnLimit),
    turnsUsed: integer(raw.turnsUsed),
    grant: Array.isArray(raw.grant) ? raw.grant.filter((item): item is string => typeof item === "string") : null,
  };
}

function normalizeHealth(value: unknown): EngineHealth | null {
  const raw = record(value);
  const state = text(raw?.state);
  if (!raw || !state) return null;
  const fault = record(raw.fault);
  return {
    state,
    generation: integer(raw.generation) ?? 0,
    pid: integer(raw.pid),
    capabilities: Array.isArray(raw.capabilities) ? raw.capabilities.filter((item): item is string => typeof item === "string") : [],
    serverVersion: text(raw.serverVersion),
    fault: fault && typeof fault.kind === "string" ? (fault as EngineFault) : null,
    detail: text(raw.detail),
    queuedDecisions: integer(raw.queuedDecisions) ?? 0,
  };
}

/** The snapshot's `supervisor` part; null for an older daemon or a closed hub. */
export function normalizeUnified(value: unknown): UnifiedSupervisorView | null {
  const raw = record(value);
  if (!raw) return null;
  const identity = record(raw.identity);
  const supervisorId = text(identity?.supervisorId);
  const ownerPrincipal = text(identity?.ownerPrincipal);
  const error = record(raw.error);
  return {
    identity: supervisorId && ownerPrincipal ? { supervisorId, ownerPrincipal } : null,
    projects: Array.isArray(raw.projects)
      ? raw.projects.flatMap((item) => {
          const project = record(item);
          const projectId = text(project?.projectId);
          return projectId ? [{ projectId, name: text(project?.name) ?? projectId }] : [];
        })
      : [],
    goals: Array.isArray(raw.goals) ? raw.goals.flatMap((goal) => normalizeGoal(goal) ?? []) : [],
    health: normalizeHealth(raw.health),
    error: error && typeof error.code === "string" ? { code: error.code, message: text(error.message) ?? "" } : null,
  };
}

/** Stable wire code of each `SupervisorFault` kind (mirrors `SupervisorFault::code`). */
export const ENGINE_FAULT_CODES: Record<string, string> = {
  missing_executable: "MHC_MISSING_EXECUTABLE",
  capability: "MHC_CAPABILITY_MISSING",
  auth: "MHC_AUTH",
  quota: "MHC_QUOTA",
  provider: "MHC_PROVIDER",
  protocol: "MHC_PROTOCOL",
  launch: "MHC_LAUNCH_FAILED",
  engine_exited: "MHC_ENGINE_EXITED",
  session_closed: "MHC_SESSION_CLOSED",
  session_mismatch: "MHC_SESSION_MISMATCH",
  timeout: "MHC_TIMEOUT",
  not_running: "MHC_NOT_RUNNING",
  budget: "MHC_BUDGET",
  profile_busy: "MHC_PROFILE_BUSY",
  cancelled: "MHC_CANCELLED",
};

const ENGINE_FAULT_LABELS: Record<string, string> = {
  missing_executable: "the mhc executable was not found",
  capability: "the engine lacks a required capability",
  auth: "the model provider refused the credentials",
  quota: "the provider quota or rate limit is reached",
  provider: "the model provider failed",
  protocol: "the engine broke the protocol",
  launch: "the engine could not start",
  engine_exited: "the engine exited",
  session_closed: "the engine closed a session",
  session_mismatch: "a resumed session had another identity",
  timeout: "the engine timed out",
  not_running: "the engine is not running",
  budget: "the configured supervisor budget is spent",
  profile_busy: "another supervisor holds the engine profile",
  cancelled: "the decision was cancelled",
};

function faultDetail(fault: EngineFault): string | null {
  if (fault.kind === "capability" && Array.isArray(fault.missing)) return `missing: ${fault.missing.join(", ")}`;
  if (fault.kind === "missing_executable") return text(fault.program);
  if (fault.kind === "session_closed") return text(fault.reason);
  if (fault.kind === "session_mismatch") return text(fault.role) ? `role ${String(fault.role)}` : null;
  if (fault.kind === "timeout") return text(fault.operation);
  return text(fault.detail);
}

export type EngineStatus = {
  /** `absent` when no engine runs in this daemon; otherwise the health state. */
  readonly state: "absent" | "ready" | "waiting" | "stopped" | "degraded" | "exited" | string;
  readonly degraded: boolean;
  readonly label: string;
  readonly code: string | null;
  readonly detail: string | null;
};

/** What the engine banner says, with the named degraded cause. */
export function engineStatus(health: EngineHealth | null): EngineStatus {
  if (!health) {
    return { state: "absent", degraded: true, label: "No supervisor engine runs in this daemon; nothing is decided automatically", code: null, detail: null };
  }
  const fault = health.fault;
  const code = fault ? (ENGINE_FAULT_CODES[fault.kind] ?? fault.kind.toUpperCase()) : null;
  const cause = fault ? (ENGINE_FAULT_LABELS[fault.kind] ?? fault.kind.replace(/_/g, " ")) : null;
  const detail = (fault ? faultDetail(fault) : null) ?? health.detail;
  switch (health.state) {
    case "ready":
      return { state: "ready", degraded: false, label: "Engine ready", code: null, detail: null };
    case "waiting":
      return { state: "waiting", degraded: true, label: "Engine waiting for another supervisor to release its profile", code, detail };
    case "stopped":
      return { state: "stopped", degraded: true, label: "Engine stopped", code, detail };
    case "exited":
      return { state: "exited", degraded: true, label: `Engine exited${cause ? `: ${cause}` : ""}; it relaunches on the next use`, code, detail };
    case "degraded":
      return { state: "degraded", degraded: true, label: `Engine degraded${cause ? `: ${cause}` : ""}. No automatic action runs`, code, detail };
    default:
      return { state: health.state, degraded: true, label: `Engine ${health.state.replace(/_/g, " ")}`, code, detail };
  }
}

export const CLOSED_RUN_STATES: ReadonlySet<string> = new Set(["completed", "cancelled"]);

/** Lane-key prefix of lanes the supervisor created (supervisor.rs `LANE_KEY_PREFIX`). */
export const LANE_KEY_PREFIX = "sv-";

function clockOf(iso: string): string | null {
  const at = Date.parse(iso);
  if (Number.isNaN(at)) return null;
  const date = new Date(at);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/** The concrete wait or next action of one goal, in one sentence. */
export function goalWaitLine(goal: UnifiedGoal): string {
  const detail = goal.stateDetail ? ` (${goal.stateDetail})` : "";
  const children = goal.pendingWaits.filter((wait) => wait.kind === "child_goal").map((wait) => wait.childGoalId ?? wait.waitId);
  const events = goal.pendingWaits.filter((wait) => wait.kind === "external_event").map((wait) => wait.eventKey ?? wait.waitId);
  let line: string;
  switch (goal.runState) {
    case "ready":
      line = "Queued: the hub dispatcher sends its next instruction to the worker.";
      break;
    case "dispatching":
      line = "Sending the instruction to the worker now.";
      break;
    case "running":
      line = "The worker is running; the supervisor waits for its turn to settle.";
      break;
    case "waiting_children":
      line = `Waiting on ${children.length} child goal${children.length === 1 ? "" : "s"}${children.length ? `: ${children.join(", ")}` : ""}. No model call until one settles.`;
      break;
    case "waiting_event":
      line = `Waiting on ${events.length ? events.join(", ") : "an external event"}. No model call until it arrives.`;
      break;
    case "awaiting_owner":
      return `Needs your decision${detail}. Nothing is sent to a worker until you approve.`;
    case "verifying":
      line = "Checking the result against the goal's requirements.";
      break;
    case "paused":
      return `Paused${detail}. Nothing is sent until you resume; the worker keeps running.`;
    case "degraded":
      return `Degraded${detail}. Nothing is sent automatically.`;
    case "delivery_uncertain":
      return `The last instruction may not have arrived${detail}. It is never resent on its own.`;
    case "completed":
      return "Completed: the requirements were verified.";
    case "cancelled":
      return `Cancelled${detail}.`;
    default:
      return `${goal.runState.replace(/_/g, " ") || "Unknown state"}${detail}.`;
  }
  const next = goal.nextTrigger ? clockOf(goal.nextTrigger) : null;
  return next ? `${line} Next check at ${next}.` : line;
}

export type GoalControls = { pause: boolean; resume: boolean; cancel: boolean; approve: boolean; correct: boolean };

/** Which owner controls a goal offers in its current state. The hub still decides. */
export function goalControls(goal: UnifiedGoal): GoalControls {
  const open = !CLOSED_RUN_STATES.has(goal.runState);
  const detail = goal.stateDetail ?? "";
  return {
    pause: open && goal.runState !== "paused",
    resume: goal.runState === "paused",
    cancel: open,
    // An approval never restores a revoked grant nor raises a spent budget (request.rs `approve`).
    approve: goal.runState === "awaiting_owner" && !detail.startsWith("GRANT_REVOKED") && !detail.startsWith("TURN_LIMIT"),
    correct: open,
  };
}

/** A request envelope `{requestId, projectId?, goalId?, expectedRevision?, action, payload}`. */
export type SupervisorEnvelope = {
  requestId: string;
  projectId?: string;
  goalId?: string;
  expectedRevision?: number;
  action: string;
  payload: Record<string, unknown>;
};

export type SupervisorSurface = "desktop" | "phone";

let requestCounter = 0;

/** A fresh request id (1-256 printable characters); the hub dedupes admissions by it. */
export function newRequestId(surface: SupervisorSurface, action: string): string {
  requestCounter += 1;
  const random = globalThis.crypto?.randomUUID?.() ?? `${Date.now().toString(36)}-${requestCounter}`;
  return `${surface}-${action}-${random}`;
}

export type GoalAction = "pause" | "resume" | "cancel" | "approve" | "correct";

/** A goal action always carries the goal's revision as the compare-and-set. */
export function goalEnvelope(
  surface: SupervisorSurface,
  goal: Pick<UnifiedGoal, "goalId" | "projectId" | "revision">,
  action: GoalAction,
  payload: Record<string, unknown> = {},
): SupervisorEnvelope {
  return {
    requestId: newRequestId(surface, action),
    ...(goal.projectId ? { projectId: goal.projectId } : {}),
    goalId: goal.goalId,
    expectedRevision: goal.revision,
    action,
    payload,
  };
}

/** Operations an owner's natural-language request may use by itself (admission BASELINE). */
export const BASELINE_OPERATIONS = ["edit", "test", "commit"] as const;
/** The verifier a request gets when the owner names no check: the owner decides completion. */
export const OWNER_REVIEW_VERIFIER = "owner review";

export type SubmitDraft = { text: string; doneWhen: string; verifier: string };

function titleOf(text: string): string {
  const first = text.trim().split(/\r?\n/)[0]?.trim() ?? "";
  return first.length > 80 ? `${first.slice(0, 79)}…` : first;
}

/**
 * A submit envelope from the composer. The owner's words go verbatim; the single requirement is
 * the owner's "done when" (or the request itself) checked by the named verifier.
 */
export function submitEnvelope(requestId: string, projectId: string | null, draft: SubmitDraft): SupervisorEnvelope {
  const request = draft.text.trim();
  return {
    requestId,
    ...(projectId ? { projectId } : {}),
    action: "submit",
    payload: {
      request,
      work: {
        title: titleOf(request),
        instruction: request,
        requirements: [{ id: "r1", text: draft.doneWhen.trim() || request, verifier: draft.verifier.trim() || OWNER_REVIEW_VERIFIER }],
        operations: [...BASELINE_OPERATIONS],
      },
    },
  };
}

export function adoptEnvelope(surface: SupervisorSurface, laneKey: string, projectId: string | null): SupervisorEnvelope {
  return { requestId: newRequestId(surface, "adopt"), ...(projectId ? { projectId } : {}), action: "adopt", payload: { laneKey } };
}

export function teachEnvelope(surface: SupervisorSurface, textValue: string): SupervisorEnvelope {
  return { requestId: newRequestId(surface, "teach"), action: "teach", payload: { text: textValue.trim() } };
}

export function forgetEnvelope(surface: SupervisorSurface, memoryId: string): SupervisorEnvelope {
  return { requestId: newRequestId(surface, "forget"), action: "forget", payload: { memoryId } };
}

/** One call to the supervisor host: its JSON answer, or the typed refusal. */
export type SupervisorCallResult =
  | { kind: "ok"; value: Record<string, unknown> }
  | { kind: "error"; code: string; message: string; status: number | null };

/** The wire code of a stale `expectedRevision` (also a state that moved under the request). */
export const REVISION_CONFLICT = "REVISION_CONFLICT";
/** The transport could not reach the supervisor host. */
export const SUPERVISOR_HOST_OFFLINE = "OFFLINE";

/** The designated supervisor host as a surface reaches it. */
export type UnifiedSupervisorClient = {
  readonly request: (envelope: SupervisorEnvelope) => Promise<SupervisorCallResult>;
  readonly conversation: (projectId: string, after: number) => Promise<SupervisorCallResult>;
  readonly memory: () => Promise<SupervisorCallResult>;
  /** `teach` and `forget`, through the memory route where the transport has one. */
  readonly remember: (envelope: SupervisorEnvelope) => Promise<SupervisorCallResult>;
  /** Refetch the snapshot now (after a mutation or a conflict). */
  readonly refresh: () => void;
};

export type SubmitReceipt =
  | { outcome: "admitted" | "awaiting_owner"; goalId: string; revision: number; runState: string; acknowledgment: string; duplicate: boolean }
  | { outcome: "conversational"; reason: string }
  | { outcome: "needs_choice"; missing: string; detail: string }
  | { outcome: "unknown" };

/** An admission outcome (`SubmitOutcome`, tagged by `outcome`; its receipt is snake_case). */
export function parseSubmitReceipt(value: Record<string, unknown>): SubmitReceipt {
  const outcome = value.outcome;
  if (outcome === "admitted" || outcome === "awaiting_owner") {
    const receipt = record(value.receipt);
    const goalId = text(receipt?.goal_id);
    const revision = integer(value.revision);
    if (!goalId || revision === null) return { outcome: "unknown" };
    return {
      outcome,
      goalId,
      revision,
      runState: text(value.runState) ?? "",
      acknowledgment: text(value.acknowledgment) ?? "",
      duplicate: value.duplicate === true,
    };
  }
  if (outcome === "conversational") return { outcome, reason: text(value.reason) ?? "" };
  if (outcome === "needs_choice") return { outcome, missing: text(value.missing) ?? "", detail: text(value.detail) ?? "" };
  return { outcome: "unknown" };
}

export type ConversationMessage = {
  id: number;
  sourceAdapter: string;
  sourceMessageId: string;
  body: string;
  goalId: string;
  admittedRevision: number;
  createdAt: string;
};

export type ConversationPage = { messages: ConversationMessage[]; next: number };

export function parseConversationPage(value: Record<string, unknown>, after: number): ConversationPage {
  const messages = Array.isArray(value.messages)
    ? value.messages.flatMap((item): ConversationMessage[] => {
        const raw = record(item);
        const id = integer(raw?.id);
        if (!raw || id === null) return [];
        return [{
          id,
          sourceAdapter: text(raw.sourceAdapter) ?? "",
          sourceMessageId: text(raw.sourceMessageId) ?? "",
          body: text(raw.body) ?? "",
          goalId: text(raw.goalId) ?? "",
          admittedRevision: integer(raw.admittedRevision) ?? 0,
          createdAt: text(raw.createdAt) ?? "",
        }];
      })
    : [];
  return { messages, next: integer(value.next) ?? after };
}

/** One memory receipt as the owner sees it: its state and the source message it came from. */
export type MemoryReceipt = { state: string; sourceMessageId: string; duplicate: boolean; text: string };

/**
 * The receipt of a `teach` answer (`{action: "teach", memory: {state, sourceMessageId,
 * duplicate}}`), with the taught text the owner sent. Null for any other shape.
 */
export function parseTeachReceipt(value: Record<string, unknown>, taught: string): MemoryReceipt | null {
  const memory = record(value.memory);
  const state = text(memory?.state);
  const sourceMessageId = text(memory?.sourceMessageId);
  if (!memory || !state || !sourceMessageId) return null;
  return { state, sourceMessageId, duplicate: memory.duplicate === true, text: taught };
}
