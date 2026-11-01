// The supervisor setup readiness model shared by the desktop and the phone: the hub's
// SetupStatus (one shape for `cmd_supervisor_setup_status`, the snapshot's `setup` field and
// `GET /api/v1/supervisor/setup`), what it means for the person, and the next action to offer.
// Pure: no Tauri or transport imports, so the phone bundle can use it.
import { SUPERVISOR_SETUP_BANNER_DISMISSED_STORAGE_KEY } from "./storageKeys";

export const SETUP_NEXT_ACTIONS = [
  "enable_engine",
  "restart_required",
  "wait_engine",
  "fix_engine",
  "create_owner",
  "map_projects",
  "cutover",
  "import_memory",
  "adopt",
  "none",
] as const;

export type SetupNextAction = (typeof SETUP_NEXT_ACTIONS)[number];

export type SetupStatus = {
  engineEnabled: boolean;
  engineInstalled: boolean;
  /** "absent" | "starting" | "ready" | "waiting" | "degraded" | "stopped", or a newer state. */
  engineState: string;
  engineFault: { code: string; message: string } | null;
  missingCapabilities: string[];
  restartRequired: boolean;
  owner: { supervisorId: string; ownerPrincipal: string } | null;
  projects: number;
  intake: {
    unifiedOpen: boolean;
    legacy: string;
    origin: string;
    legacyHistory: boolean;
  };
  autoCutover: boolean;
  memoryImport: { enabled: boolean; sessions: number; turns: number; lastAt: string | null };
  adoptable: number;
  nextAction: SetupNextAction;
};

/** One registered local project handed to setup for mapping. */
export type SetupProject = { name: string; path: string };

export type RunSetupOptions = { importMemory: boolean; projects: SetupProject[] };

/** A refusal keeps its wire code; the UI branches on `code`, never on the message. */
export type SetupCallError = { kind: "error"; code: string; message: string };
export type SetupCallResult = { kind: "ok"; status: SetupStatus } | SetupCallError;

export type AdoptAllRefusal = { lane: string; code: string; message: string };
export type AdoptAllResult = { kind: "ok"; adopted: string[]; refused: AdoptAllRefusal[] } | SetupCallError;

/** How a surface runs setup and adoption: the desktop through IPC, the phone over HTTP. */
export type SetupActions = {
  readonly run: (opts: RunSetupOptions) => Promise<SetupCallResult>;
  readonly adopt: () => Promise<AdoptAllResult>;
};

export type SetupReadiness = "ready" | "needs_setup" | "working" | "blocked" | "unknown";

export type SetupPrimary = "run_setup" | "adopt" | null;

export type SetupAction = {
  kind: SetupNextAction;
  label: string;
  description: string;
  primary: SetupPrimary;
};

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function bool(value: unknown): boolean {
  return value === true;
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0;
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function isNextAction(value: unknown): value is SetupNextAction {
  return typeof value === "string" && (SETUP_NEXT_ACTIONS as readonly string[]).includes(value);
}

/**
 * The hub's SetupStatus, or null when it is absent (an older daemon) or not understood (no
 * object, or a nextAction this build does not know). Null always reads as "unknown": never nag.
 */
export function normalizeSetupStatus(raw: unknown): SetupStatus | null {
  const value = record(raw);
  if (!value || !isNextAction(value.nextAction)) return null;
  const fault = record(value.engineFault);
  const owner = record(value.owner);
  const intake = record(value.intake);
  const memory = record(value.memoryImport);
  return {
    engineEnabled: bool(value.engineEnabled),
    engineInstalled: bool(value.engineInstalled),
    engineState: text(value.engineState, "absent") || "absent",
    engineFault: fault ? { code: text(fault.code), message: text(fault.message) } : null,
    missingCapabilities: Array.isArray(value.missingCapabilities)
      ? value.missingCapabilities.filter((item): item is string => typeof item === "string" && item.length > 0)
      : [],
    restartRequired: bool(value.restartRequired),
    owner:
      owner && typeof owner.supervisorId === "string" && owner.supervisorId
        ? { supervisorId: owner.supervisorId, ownerPrincipal: text(owner.ownerPrincipal) }
        : null,
    projects: count(value.projects),
    intake: {
      unifiedOpen: bool(intake?.unifiedOpen),
      legacy: text(intake?.legacy),
      origin: text(intake?.origin),
      legacyHistory: bool(intake?.legacyHistory),
    },
    autoCutover: bool(value.autoCutover),
    memoryImport: {
      enabled: bool(memory?.enabled),
      sessions: count(memory?.sessions),
      turns: count(memory?.turns),
      lastAt: typeof memory?.lastAt === "string" && memory.lastAt ? memory.lastAt : null,
    },
    adoptable: count(value.adoptable),
    nextAction: value.nextAction,
  };
}

/** A setup answer as a call result; a status this build does not understand is an invalid response. */
export function setupStatusResult(raw: unknown): SetupCallResult {
  const status = normalizeSetupStatus(raw);
  return status ? { kind: "ok", status } : { kind: "error", code: "INVALID_RESPONSE", message: "Setup status is not understood by this version" };
}

/** Parses the adopt-all answer `{adopted, refused}`; anything else is an invalid response. */
export function parseAdoptAll(raw: unknown): AdoptAllResult {
  const value = record(raw);
  if (!value || !Array.isArray(value.adopted)) {
    return { kind: "error", code: "INVALID_RESPONSE", message: "Adopt response is not understood by this version" };
  }
  const refused = Array.isArray(value.refused) ? value.refused : [];
  return {
    kind: "ok",
    adopted: value.adopted.filter((lane): lane is string => typeof lane === "string"),
    refused: refused.flatMap((item) => {
      const row = record(item);
      if (!row || typeof row.lane !== "string") return [];
      return [{ lane: row.lane, code: text(row.code, "REFUSED") || "REFUSED", message: text(row.message) }];
    }),
  };
}

/** One line summarising an adopt-all answer. */
export function adoptSummary(result: Extract<AdoptAllResult, { kind: "ok" }>): string {
  const handed = result.adopted.length;
  const head = handed === 0 ? "No jobs were handed over." : `Handed ${handed} job${handed === 1 ? "" : "s"} to the supervisor.`;
  if (result.refused.length === 0) return head;
  const codes = [...new Set(result.refused.map((row) => row.code))].join(", ");
  return `${head} ${result.refused.length} could not be handed over (${codes}).`;
}

/**
 * True after the person rolled the supervisor back. The hub then reports nextAction "none"; an
 * older daemon still says "cutover". Either way the rollback was deliberate, so setup must not
 * offer to finish it again: only `ferryx hub cutover` hands requests back.
 */
function isRolledBack(status: SetupStatus): boolean {
  return status.intake.origin === "rollback" && (status.nextAction === "none" || status.nextAction === "cutover");
}

/** What the setup state means for the person. Unknown (null) never asks for anything. */
export function readiness(status: SetupStatus | null): SetupReadiness {
  if (!status) return "unknown";
  switch (status.nextAction) {
    case "none":
      return "ready";
    case "restart_required":
    case "wait_engine":
    case "import_memory":
      return "working";
    case "fix_engine":
      return "blocked";
    case "enable_engine":
    case "create_owner":
    case "map_projects":
    case "adopt":
      return "needs_setup";
    case "cutover":
      return isRolledBack(status) ? "ready" : "needs_setup";
  }
}

/**
 * True once the supervisor runs on its own, or was deliberately rolled back. Earlier jobs
 * waiting for adoption are an offer, not unfinished setup, so onboarding counts them as done
 * (the banner still offers them).
 */
export function setupFinished(status: SetupStatus | null): boolean {
  return status !== null && (status.nextAction === "none" || status.nextAction === "adopt" || isRolledBack(status));
}

/** True when a setup banner should ask: needs setup or blocked, never unknown, ready or working. */
export function setupNeedsAttention(status: SetupStatus | null): boolean {
  const state = readiness(status);
  return state === "needs_setup" || state === "blocked";
}

function jobs(n: number): string {
  return `${n} earlier job${n === 1 ? "" : "s"}`;
}

export type SetupChecklistRowId = "engine_on" | "engine_ready" | "owner" | "projects" | "cutover" | "memory" | "earlier_jobs";

/** Done, waiting on something, cannot continue, or the one row in progress right now. */
export type SetupChecklistState = "done" | "pending" | "blocked" | "active";

export type SetupChecklistRow = {
  id: SetupChecklistRowId;
  label: string;
  state: SetupChecklistState;
  detail: string;
};

function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

function engineOnRow(status: SetupStatus): SetupChecklistRow {
  const base = { id: "engine_on" as const, label: "Engine on" };
  if (status.restartRequired) {
    return { ...base, state: status.nextAction === "restart_required" ? "active" : "pending", detail: "Restarting to apply the change" };
  }
  if (status.engineEnabled && status.engineInstalled) return { ...base, state: "done", detail: "On" };
  if (status.engineEnabled && !status.engineInstalled) return { ...base, state: "blocked", detail: "Not included in this install" };
  return { ...base, state: "pending", detail: "Off" };
}

function engineReadyRow(status: SetupStatus): SetupChecklistRow {
  const base = { id: "engine_ready" as const, label: "Engine ready" };
  if (status.engineState === "ready" && !status.engineFault) return { ...base, state: "done", detail: "Ready" };
  if (status.engineFault) return { ...base, state: "blocked", detail: status.engineFault.code || status.engineState };
  if (status.nextAction === "fix_engine") return { ...base, state: "blocked", detail: status.engineState };
  if (status.nextAction === "wait_engine") return { ...base, state: "active", detail: "Starting" };
  return { ...base, state: "pending", detail: status.engineEnabled ? status.engineState : "Waits for the engine to be on" };
}

function cutoverRow(status: SetupStatus): SetupChecklistRow {
  const base = { id: "cutover" as const, label: "Cut over to the supervisor" };
  const { unifiedOpen, legacy, legacyHistory, origin } = status.intake;
  if (unifiedOpen && legacy === "stopped") return { ...base, state: "done", detail: "Done" };
  // A fresh install has no earlier job runner to stop: open intake is the whole cutover.
  if (unifiedOpen && !legacyHistory) return { ...base, state: "done", detail: "Taking requests" };
  if (origin === "rollback") return { ...base, state: "pending", detail: "Rolled back; it will not cut over on its own" };
  if (status.autoCutover && !unifiedOpen) return { ...base, state: "pending", detail: "waiting for the engine" };
  if (unifiedOpen) return { ...base, state: "pending", detail: "The earlier job runner is still on" };
  return { ...base, state: "pending", detail: "Not yet" };
}

/**
 * Every setup stage as a checklist row, derived only from the SetupStatus. Memory import is
 * left out when it was not chosen. Only a stage the supervisor is working on right now is
 * "active"; every other row is done, pending or blocked.
 */
export function setupChecklist(status: SetupStatus): SetupChecklistRow[] {
  const rows: SetupChecklistRow[] = [
    engineOnRow(status),
    engineReadyRow(status),
    status.owner
      ? { id: "owner", label: "Owner", state: "done", detail: status.owner.ownerPrincipal || status.owner.supervisorId }
      : { id: "owner", label: "Owner", state: "pending", detail: "Not set yet" },
    status.projects > 0
      ? { id: "projects", label: "Projects", state: "done", detail: plural(status.projects, "project") }
      : { id: "projects", label: "Projects", state: "pending", detail: "None connected yet" },
    cutoverRow(status),
  ];
  if (status.memoryImport.enabled) {
    const { sessions, turns, lastAt } = status.memoryImport;
    const importing = status.nextAction === "import_memory";
    rows.push(
      lastAt !== null
        ? { id: "memory", label: "Learned from your sessions", state: "done", detail: `${plural(sessions, "session")}, ${plural(turns, "note")}` }
        : { id: "memory", label: "Learned from your sessions", state: importing ? "active" : "pending", detail: importing ? "Reading what you typed" : "Not yet" },
    );
  }
  rows.push(
    status.adoptable > 0
      ? { id: "earlier_jobs", label: "Earlier jobs", state: "pending", detail: `${status.adoptable} can be handed over` }
      : { id: "earlier_jobs", label: "Earlier jobs", state: "done", detail: "None waiting" },
  );
  return rows;
}

function faultDescription(status: SetupStatus): string {
  const parts: string[] = [];
  if (status.engineFault?.message) parts.push(status.engineFault.message);
  if (status.missingCapabilities.length > 0) {
    parts.push(`The bundled engine is missing: ${status.missingCapabilities.join(", ")}. Update Ferryx to get a newer engine.`);
  }
  if (status.engineFault?.code) parts.push(`Fault code: ${status.engineFault.code}`);
  return parts.length > 0 ? parts.join(" ") : `The engine reports "${status.engineState}". Nothing is decided automatically until it is fixed.`;
}

/** The one thing to tell (and offer) the person for this setup state. */
export function actionFor(status: SetupStatus): SetupAction {
  const kind = status.nextAction;
  if (isRolledBack(status)) {
    return {
      kind,
      label: "Supervisor is rolled back",
      description: "Earlier jobs run the old way. Run `ferryx hub cutover` to hand new requests to the supervisor again.",
      primary: null,
    };
  }
  switch (kind) {
    case "enable_engine":
      return {
        kind,
        label: "Turn on the supervisor",
        description: "The supervisor engine ships with Ferryx but is off. Turning it on keeps your terminals running.",
        primary: "run_setup",
      };
    case "restart_required":
      return {
        kind,
        label: "Restarting the supervisor...",
        description: "It restarts to pick up its new settings. Your terminals keep running.",
        primary: null,
      };
    case "wait_engine":
      return {
        kind,
        label: "Starting the supervisor engine",
        description: "This usually takes a few seconds.",
        primary: null,
      };
    case "fix_engine":
      return { kind, label: "The supervisor engine needs attention", description: faultDescription(status), primary: null };
    case "create_owner":
      return {
        kind,
        label: "Finish supervisor setup",
        description: "It needs to know who it works for before it accepts requests.",
        primary: "run_setup",
      };
    case "map_projects":
      return {
        kind,
        label: "Finish supervisor setup",
        description: "Your projects are not connected to the supervisor yet.",
        primary: "run_setup",
      };
    case "cutover":
      return {
        kind,
        label: "Finish supervisor setup",
        description: "It is ready but has not taken over new requests yet.",
        primary: "run_setup",
      };
    case "import_memory":
      return {
        kind,
        label: "Learning from your recent sessions",
        description: "It reads only what you typed, so it knows how you like to work.",
        primary: null,
      };
    case "adopt":
      return {
        kind,
        label: `${jobs(status.adoptable)} can be handed to the supervisor`,
        description: "Hand them over and it tracks them with the rest of your work.",
        primary: "adopt",
      };
    case "none":
      return {
        kind,
        label: "Supervisor is running on its own",
        description: "It takes requests, runs your projects, and asks you when something needs a decision.",
        primary: null,
      };
  }
}

/** Local registered projects as setup maps them: one entry per path, named by its folder. */
export function setupProjectsFrom(projects: ReadonlyArray<{ repoRoot: string; target?: { kind: string } | null }>): SetupProject[] {
  const seen = new Set<string>();
  const out: SetupProject[] = [];
  for (const project of projects) {
    if ((project.target?.kind ?? "local") !== "local" || !project.repoRoot || seen.has(project.repoRoot)) continue;
    seen.add(project.repoRoot);
    const name = project.repoRoot.split(/[\\/]/).filter(Boolean).pop() ?? project.repoRoot;
    out.push({ name, path: project.repoRoot });
  }
  return out;
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The next-action kinds whose setup banner the person dismissed. */
export function loadDismissedSetupKinds(storage: Storage | null = browserStorage()): SetupNextAction[] {
  if (!storage) return [];
  try {
    const parsed: unknown = JSON.parse(storage.getItem(SUPERVISOR_SETUP_BANNER_DISMISSED_STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter(isNextAction) : [];
  } catch {
    return [];
  }
}

/** Window event fired when a setup banner is dismissed, so every mounted banner hides at once. */
export const SETUP_BANNER_DISMISSED_EVENT = "ferryx:supervisor-setup-dismissed";

/** Hides the banner for this next action only; a later, different action shows it again. */
export function dismissSetupKind(kind: SetupNextAction, storage: Storage | null = browserStorage()): SetupNextAction[] {
  const next = [...new Set([...loadDismissedSetupKinds(storage), kind])];
  try {
    storage?.setItem(SUPERVISOR_SETUP_BANNER_DISMISSED_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // A full or blocked storage only means the banner comes back next launch.
  }
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(SETUP_BANNER_DISMISSED_EVENT));
  return next;
}
