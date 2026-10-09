/**
 * All-sessions machine inventory.
 *
 * The paired machine's session list (`/api/v1/sessions`, and the same `sessions` object
 * inside every `inventoryInvalidated` boundary on `/api/v1/events`) is the only
 * authoritative list of the sessions that exist there: workspace, worktree (root is the
 * `null` slug), daemon epoch and session id. It carries no activity — `running` only
 * means the PTY is alive, never that an agent is working.
 *
 * Activity is additive and optional. It comes from the published desktop context
 * (`activeContext.tabs[].activityState`, which covers the desktop's current publish
 * only) or from an `agent_state` frame on an attached terminal socket. Everything else
 * stays `activityState: null` and must be rendered as an honest unknown.
 *
 * Identity rules that this module never bends: a session is machine + session id, its
 * daemon epoch is part of that identity, a payload row whose target names another
 * machine is not ours, and an answer that skipped rows is not authoritative enough to
 * prune what the UI already knows.
 */

export type SessionActivityState = "working" | "waiting" | "done";

export type SessionActivitySource = "context_tabs" | "agent_state";

export type SessionInventoryCompleteness = "complete" | "partial" | "unknown";

export type RemoteSessionInventoryEntry = {
  readonly machineId: string;
  readonly workspaceId: string;
  readonly worktreeSlug: string | null;
  readonly worktreeLabel: string | null;
  readonly sessionId: string;
  readonly daemonEpoch: string | null;
  readonly title: string | null;
  readonly running: boolean;
  readonly agentType: string | null;
  readonly providerSessionId: string | null;
  readonly activityState: SessionActivityState | null;
  readonly activitySource?: SessionActivitySource;
  readonly activityObservedAt?: number;
};

export type RemoteSessionInventory = {
  machineId: string;
  revision: string | null;
  completeness: SessionInventoryCompleteness;
  entries: RemoteSessionInventoryEntry[];
  unavailableWorkspaceIds: string[];
  fetchedAt: number;
  /** Rows the machine sent that failed identity/shape validation in this fetch. */
  rejectedRowCount?: number;
};

export type SessionActivityObservation = {
  sessionId: string;
  state: SessionActivityState;
  source: SessionActivitySource;
  machineId?: string | null;
  daemonEpoch?: string | null;
  agentType?: string | null;
  observedAt?: number;
};

export type SessionLookup = {
  workspaceId: string;
  worktreeSlug: string | null;
  sessionId: string;
  daemonEpoch?: string | null;
  /** Attaching requires a live PTY; set false only to inspect a stopped session's row. */
  requireRunning?: boolean;
};

export type SessionLookupResult = {
  entry: RemoteSessionInventoryEntry;
  /** False when either side lacks a daemon epoch; an unverified epoch is not a match. */
  epochVerified: boolean;
};

type UnknownRecord = Record<string, unknown>;

function record(value: unknown): UnknownRecord | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as UnknownRecord)
    : null;
}

function text(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function epochText(value: unknown): string | null {
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  if (typeof value === "string" && value.trim().length > 0) return value.trim();
  return null;
}

function activityState(value: unknown): SessionActivityState | null {
  return value === "working" || value === "waiting" || value === "done" ? value : null;
}

/**
 * `complete` is the only authoritative wire value. `full` is a legacy synonym used by
 * older fixtures; anything else stays `unknown` and never prunes.
 */
function completenessOf(value: unknown): SessionInventoryCompleteness {
  if (value === "complete" || value === "full") return "complete";
  if (value === "partial") return "partial";
  return "unknown";
}

/**
 * Session identity. The epoch is part of the key wherever the caller knows it, because a
 * daemon handover can reuse a session id for a different terminal.
 */
export function sessionKey(machineId: string, sessionId: string, daemonEpoch?: string | null): string {
  return `${machineId}\u0000${sessionId}\u0000${daemonEpoch ?? ""}`;
}

function identityKey(entry: RemoteSessionInventoryEntry): string {
  return sessionKey(entry.machineId, entry.sessionId);
}

function epochMatches(a: string | null, b: string | null): boolean {
  return a !== null && b !== null && a === b;
}

export function worktreeKey(workspaceId: string, worktreeSlug: string | null): string {
  return `${workspaceId}\u0000${worktreeSlug ?? ""}`;
}

export function emptyInventory(machineId: string): RemoteSessionInventory {
  return {
    machineId,
    revision: null,
    completeness: "unknown",
    entries: [],
    unavailableWorkspaceIds: [],
    fetchedAt: 0,
  };
}

export type ParseSessionsResult =
  | { ok: true; inventory: RemoteSessionInventory }
  | { ok: false; error: string };

/**
 * Parses a `sessions` object into inventory rows. Never throws and never synthesises a
 * row: a row without a resolvable id, workspace, foreign target machine or boolean
 * `running` is rejected and the answer is downgraded, because a fetch that dropped rows
 * must not be mistaken for a machine that no longer has them.
 */
export function parseSessionsPayload(
  machineId: string,
  payload: unknown,
  now: number = Date.now(),
): ParseSessionsResult {
  const body = record(payload);
  if (!body || !Array.isArray(body.sessions)) {
    return { ok: false, error: "invalid sessions payload" };
  }
  const entries: RemoteSessionInventoryEntry[] = [];
  const seen = new Set<string>();
  let rejected = 0;
  for (const raw of body.sessions) {
    const row = record(raw);
    if (!row) {
      rejected += 1;
      continue;
    }
    const target = record(row.target);
    const sessionId = text(target?.sessionId ?? target?.session_id) ?? text(row.sessionId ?? row.session_id);
    const workspaceId = text(row.workspaceId ?? row.workspace_id);
    const targetMachineId = text(target?.machineId ?? target?.machine_id);
    if (!sessionId || !workspaceId) {
      rejected += 1;
      continue;
    }
    if (targetMachineId && targetMachineId !== machineId) {
      rejected += 1;
      continue;
    }
    if (typeof row.running !== "boolean") {
      rejected += 1;
      continue;
    }
    const daemonEpoch =
      epochText(target?.daemonEpoch ?? target?.daemon_epoch) ??
      epochText(row.daemonEpoch ?? row.daemon_epoch);
    const key = sessionKey(machineId, sessionId, daemonEpoch);
    if (seen.has(key)) {
      rejected += 1;
      continue;
    }
    seen.add(key);
    const worktree = record(row.worktree);
    const worktreeSlug = text(worktree?.slug);
    entries.push({
      machineId,
      workspaceId,
      worktreeSlug,
      worktreeLabel: worktreeSlug,
      sessionId,
      daemonEpoch,
      title: text(row.title),
      running: row.running,
      agentType: text(row.agentType ?? row.agent_type),
      providerSessionId: text(record(row.providerSession ?? row.provider_session)?.id),
      activityState: null,
    });
  }
  const unavailable = Array.isArray(body.unavailableWorkspaceIds)
    ? body.unavailableWorkspaceIds.map(text).filter((id): id is string => id !== null)
    : [];
  const declared = completenessOf(body.completeness);
  return {
    ok: true,
    inventory: {
      machineId,
      revision: epochText(body.revision),
      completeness: rejected > 0 && declared === "complete" ? "partial" : declared,
      entries,
      unavailableWorkspaceIds: unavailable,
      fetchedAt: now,
      ...(rejected > 0 ? { rejectedRowCount: rejected } : {}),
    },
  };
}

/**
 * Merges a fresh fetch into the inventory the UI already holds.
 *
 * A `complete` fetch is authoritative for the workspaces it covers, so rows it did not
 * see are dropped — except rows of workspaces it declared unavailable, which it never
 * scanned. A `partial`/`unknown` fetch never removes a row: absence in a partial answer
 * is not evidence that a session is gone. Activity is carried forward only when the
 * machine, session id **and** daemon epoch all still agree, so a reused session id after
 * a handover cannot inherit the previous terminal's state.
 */
export function mergeSessionInventory(
  previous: RemoteSessionInventory | null,
  incoming: RemoteSessionInventory,
): RemoteSessionInventory {
  if (!previous) return incoming;
  if (previous.machineId !== incoming.machineId) return incoming;

  const learned = new Map<string, RemoteSessionInventoryEntry>();
  for (const entry of previous.entries) {
    if (entry.activityState) learned.set(identityKey(entry), entry);
  }

  const incomingKeys = new Set(incoming.entries.map(identityKey));
  const unavailable = new Set(incoming.unavailableWorkspaceIds);
  const authoritative = incoming.completeness === "complete";

  const entries: RemoteSessionInventoryEntry[] = incoming.entries.map((entry) => {
    const before = learned.get(identityKey(entry));
    if (!before?.activityState || !epochMatches(before.daemonEpoch, entry.daemonEpoch)) return entry;
    return {
      ...entry,
      activityState: before.activityState,
      ...(before.activitySource ? { activitySource: before.activitySource } : {}),
      ...(before.activityObservedAt !== undefined ? { activityObservedAt: before.activityObservedAt } : {}),
    };
  });

  for (const entry of previous.entries) {
    const key = identityKey(entry);
    if (incomingKeys.has(key)) continue;
    if (!authoritative || unavailable.has(entry.workspaceId)) entries.push(entry);
  }

  /* One row per machine + session id. A retained previous row whose epoch the machine has
     since replaced is stale, so the incoming row wins. */
  const bySession = new Map<string, RemoteSessionInventoryEntry>();
  for (const entry of entries) {
    const key = identityKey(entry);
    const existing = bySession.get(key);
    if (!existing) {
      bySession.set(key, entry);
      continue;
    }
    const existingFromAnswer = incomingKeys.has(identityKey(existing));
    const candidateFromAnswer = incomingKeys.has(key) && !existingFromAnswer;
    if (candidateFromAnswer) bySession.set(key, entry);
  }
  const mergedEntries = [...bySession.values()];

  return {
    machineId: incoming.machineId,
    revision: incoming.revision ?? previous.revision,
    completeness: incoming.completeness,
    entries: mergedEntries,
    unavailableWorkspaceIds: incoming.unavailableWorkspaceIds,
    fetchedAt: incoming.fetchedAt,
    ...(incoming.rejectedRowCount ? { rejectedRowCount: incoming.rejectedRowCount } : {}),
  };
}

export type InventoryEvent =
  | { kind: "inventory"; inventory: RemoteSessionInventory; sequence: string | null; reason: string | null }
  | { kind: "partial"; sequence: string | null; error: string | null }
  | { kind: "ignored"; type: string | null };

/**
 * Decodes one `/api/v1/events` frame. The socket sends a full `inventoryInvalidated`
 * boundary on subscribe and on every change; when the boundary itself was too large the
 * server degrades it to `{completeness:"partial"}` with no rows, which callers answer by
 * keeping the inventory they already have.
 */
export function parseInventoryEvent(
  machineId: string,
  raw: unknown,
  now: number = Date.now(),
): InventoryEvent {
  let value: unknown = raw;
  if (typeof raw === "string") {
    try {
      value = JSON.parse(raw);
    } catch {
      return { kind: "ignored", type: null };
    }
  }
  const event = record(value);
  if (!event) return { kind: "ignored", type: null };
  const type = text(event.type);
  if (type !== "inventoryInvalidated") return { kind: "ignored", type };
  const sequence = epochText(event.sequence);
  const reason = text(event.reason);
  const payload = record(event.payload);
  if (!payload) return { kind: "partial", sequence, error: null };
  const sessions = parseSessionsPayload(machineId, payload.sessions, now);
  if (!sessions.ok) {
    return { kind: "partial", sequence, error: text(payload.error) };
  }
  const declaredPartial = completenessOf(payload.completeness) === "partial";
  const entries = sessions.inventory.entries.length > 0
    ? applyWorktreeLabels(sessions.inventory.entries, payload.projects)
    : sessions.inventory.entries;
  return {
    kind: "inventory",
    sequence,
    reason,
    inventory: {
      ...sessions.inventory,
      completeness: declaredPartial ? "partial" : sessions.inventory.completeness,
      entries,
    },
  };
}

/**
 * Enriches rows with the human worktree label the project catalog publishes (branch
 * name, with only the worktree slug as fallback). Unknown slugs keep their slug.
 */
export function applyWorktreeLabels(
  entries: RemoteSessionInventoryEntry[],
  projects: unknown,
): RemoteSessionInventoryEntry[] {
  const catalog = record(projects);
  if (!catalog || !Array.isArray(catalog.projects)) return entries;
  const labels = new Map<string, string>();
  for (const raw of catalog.projects) {
    const project = record(raw);
    const workspaceId = text(project?.workspaceId ?? project?.workspace_id);
    if (!project || !workspaceId) continue;
    const worktrees = Array.isArray(project.worktrees) ? project.worktrees : [];
    for (const rawWorktree of worktrees) {
      const worktree = record(rawWorktree);
      if (!worktree) continue;
      const identity = record(worktree.identity);
      const slug = text(identity?.slug);
      if (!slug) continue;
      const branch = text(worktree.branch)?.replace(/^refs\/heads\//, "") ?? null;
      labels.set(worktreeKey(workspaceId, slug), branch ?? slug);
    }
  }
  if (labels.size === 0) return entries;
  return entries.map((entry) => {
    const label = labels.get(worktreeKey(entry.workspaceId, entry.worktreeSlug));
    return label && label !== entry.worktreeLabel ? { ...entry, worktreeLabel: label } : entry;
  });
}

function basenameOf(value: string | null): string | null {
  if (!value) return null;
  const trimmed = value.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/);
  const last = parts[parts.length - 1];
  return last && last.trim().length > 0 ? last : null;
}

/**
 * Human workspace label for a project: repository basename, or the repository name
 * carried by the git remote. Never the absolute repo root, so remote clients cannot
 * leak machine paths into the UI.
 */
export function workspaceLabelsFromCatalog(projects: unknown): Map<string, string> {
  const labels = new Map<string, string>();
  const catalog = record(projects);
  if (!catalog || !Array.isArray(catalog.projects)) return labels;
  for (const raw of catalog.projects) {
    const project = record(raw);
    const workspaceId = text(project?.workspaceId ?? project?.workspace_id);
    if (!project || !workspaceId) continue;
    const fromRoot = basenameOf(text(project.repoRoot ?? project.repo_root));
    const remote = text(project.gitRemote ?? project.git_remote);
    const fromRemote = remote ? basenameOf(remote.replace(/\.git$/, "")) : null;
    const label = fromRoot ?? fromRemote;
    if (label) labels.set(workspaceId, label);
  }
  return labels;
}

/**
 * Extracts activity observations from published desktop context tabs. Only tabs with
 * both a session id and an explicit state count; the desktop publishes just its focused
 * workspace, so absence here means "not published", never "not working".
 */
export function observationsFromContextTabs(
  tabs: unknown,
  now: number = Date.now(),
): SessionActivityObservation[] {
  if (!Array.isArray(tabs)) return [];
  const observations: SessionActivityObservation[] = [];
  for (const raw of tabs) {
    const tab = record(raw);
    if (!tab) continue;
    const sessionId = text(tab.sessionId ?? tab.session_id);
    const state = activityState(tab.activityState ?? tab.activity_state ?? tab.state);
    if (!sessionId || !state) continue;
    observations.push({
      sessionId,
      state,
      source: "context_tabs",
      machineId: text(tab.machineId ?? tab.machine_id),
      daemonEpoch: epochText(tab.daemonEpoch ?? tab.daemon_epoch),
      agentType: text(tab.agentType ?? tab.agent_type),
      observedAt: now,
    });
  }
  return observations;
}

/**
 * Applies activity observations to inventory rows, matching the same identity the
 * observation was published for. An observation whose epoch or machine disagrees with
 * the row it would touch is dropped rather than written onto a different terminal, and
 * an observation without a matching row is ignored rather than creating a session the
 * machine never reported.
 */
export function applySessionActivity(
  inventory: RemoteSessionInventory | null,
  observations: readonly SessionActivityObservation[],
  now: number = Date.now(),
): RemoteSessionInventory | null {
  if (!inventory || observations.length === 0) return inventory;
  let changed = false;
  const entries = inventory.entries.map((entry) => {
    let matched: SessionActivityObservation | null = null;
    for (const observation of observations) {
      if (observation.sessionId !== entry.sessionId) continue;
      if (observation.machineId && observation.machineId !== entry.machineId) continue;
      if (observation.daemonEpoch && !epochMatches(observation.daemonEpoch, entry.daemonEpoch)) continue;
      matched = observation;
      break;
    }
    if (!matched) return entry;
    const agentType = matched.agentType ?? entry.agentType;
    const same =
      entry.activityState === matched.state &&
      (entry.activitySource ?? null) === matched.source &&
      (entry.agentType ?? null) === (agentType ?? null);
    if (same) return entry;
    changed = true;
    return {
      ...entry,
      agentType,
      activityState: matched.state,
      activitySource: matched.source,
      activityObservedAt: matched.observedAt ?? now,
    };
  });
  return changed ? { ...inventory, entries } : inventory;
}

export type MachineAgentStateFrame = {
  machineId: string | null;
  sessionId: string;
  daemonEpoch: string | null;
  state: SessionActivityState;
  agent: string | null;
};

/**
 * Decodes an `agent_state` control frame from an attached terminal socket. Frames are
 * only accepted for the three known states, so an unfamiliar vocabulary stays
 * unreported instead of being guessed at.
 */
export function parseMachineAgentStateFrame(raw: unknown): MachineAgentStateFrame | null {
  let value: unknown = raw;
  if (typeof raw === "string") {
    try {
      value = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  const frame = record(value);
  if (!frame || frame.type !== "agent_state") return null;
  const target = record(frame.target);
  const sessionId = text(target?.sessionId ?? target?.session_id);
  const state = activityState(frame.state);
  if (!sessionId || !state) return null;
  return {
    machineId: text(target?.machineId ?? target?.machine_id),
    sessionId,
    daemonEpoch: epochText(target?.daemonEpoch ?? target?.daemon_epoch),
    state,
    agent: text(frame.agent),
  };
}

/**
 * Exact-match lookup for attaching to a session that already exists.
 *
 * Returns `null` — never a neighbouring session — when the id is unknown, when the
 * workspace/worktree does not match, when both sides know a daemon epoch and they
 * disagree, or when the row is stopped (unless the caller only wants to inspect it).
 * Callers must treat `null` as "cannot attach" and must not fall back to a spawn: only an
 * explicit new-terminal request may create a session.
 */
export function selectInventorySession(
  inventory: RemoteSessionInventory | null,
  lookup: SessionLookup,
): SessionLookupResult | null {
  if (!inventory) return null;
  const expectedSlug = lookup.worktreeSlug ?? null;
  const requireRunning = lookup.requireRunning !== false;
  for (const entry of inventory.entries) {
    if (entry.sessionId !== lookup.sessionId) continue;
    if (entry.workspaceId !== lookup.workspaceId) continue;
    if ((entry.worktreeSlug ?? null) !== expectedSlug) continue;
    if (requireRunning && !entry.running) continue;
    if (lookup.daemonEpoch && entry.daemonEpoch && lookup.daemonEpoch !== entry.daemonEpoch) continue;
    return {
      entry,
      epochVerified: Boolean(
        lookup.daemonEpoch && entry.daemonEpoch && lookup.daemonEpoch === entry.daemonEpoch,
      ),
    };
  }
  return null;
}

/**
 * Deterministic attach target for a worktree row that carries no session id. Earlier
 * preferences win, then the lowest session id, so repeated polls always resolve to the
 * same live session. Stopped rows are never returned: attaching to them would type into
 * a dead PTY.
 */
export function preferredSessionForWorktree(
  inventory: RemoteSessionInventory | null,
  workspaceId: string,
  worktreeSlug: string | null,
  preferred: readonly (string | null | undefined)[] = [],
): RemoteSessionInventoryEntry | null {
  const rows = inventorySessionsFor(inventory, workspaceId, worktreeSlug).filter((row) => row.running);
  if (rows.length === 0) return null;
  for (const candidate of preferred) {
    if (!candidate) continue;
    const match = rows.find((row) => row.sessionId === candidate);
    if (match) return match;
  }
  return rows[0] ?? null;
}

export function inventorySessionsFor(
  inventory: RemoteSessionInventory | null,
  workspaceId: string,
  worktreeSlug: string | null,
): RemoteSessionInventoryEntry[] {
  if (!inventory) return [];
  const slug = worktreeSlug ?? null;
  return inventory.entries
    .filter((entry) => entry.workspaceId === workspaceId && (entry.worktreeSlug ?? null) === slug)
    .sort((a, b) => (a.sessionId < b.sessionId ? -1 : a.sessionId > b.sessionId ? 1 : 0));
}

export type InventoryWorktreeGroup = {
  workspaceId: string;
  workspaceLabel: string | null;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
  sessions: RemoteSessionInventoryEntry[];
};

/**
 * Groups rows by worktree for the picker tree. Root worktrees keep their `null` slug as
 * a distinct group, and the same workspace id on two machines stays separate because
 * every group is built from one machine-scoped inventory.
 */
export function groupInventoryByWorktree(
  inventory: RemoteSessionInventory | null,
  projects?: unknown,
): InventoryWorktreeGroup[] {
  if (!inventory) return [];
  const workspaceLabels = projects ? workspaceLabelsFromCatalog(projects) : new Map<string, string>();
  const groups = new Map<string, InventoryWorktreeGroup>();
  for (const entry of inventory.entries) {
    const key = worktreeKey(entry.workspaceId, entry.worktreeSlug);
    const existing = groups.get(key);
    if (existing) {
      existing.sessions.push(entry);
      continue;
    }
    groups.set(key, {
      workspaceId: entry.workspaceId,
      workspaceLabel: workspaceLabels.get(entry.workspaceId) ?? null,
      worktreeSlug: entry.worktreeSlug,
      worktreeLabel: entry.worktreeLabel,
      sessions: [entry],
    });
  }
  const list = [...groups.values()];
  for (const group of list) {
    group.sessions.sort((a, b) => (a.sessionId < b.sessionId ? -1 : a.sessionId > b.sessionId ? 1 : 0));
  }
  return list.sort((a, b) => {
    if (a.workspaceId !== b.workspaceId) return a.workspaceId < b.workspaceId ? -1 : 1;
    const left = a.worktreeSlug ?? "";
    const right = b.worktreeSlug ?? "";
    return left < right ? -1 : left > right ? 1 : 0;
  });
}

export type InventoryActivityLabel = {
  state: SessionActivityState | null;
  detail: "working" | "waiting" | "done" | "running" | "stopped";
  source: SessionActivitySource | "machine";
};

/**
 * Display label for one row: `running` is reported as `running`, never as `working`, so
 * an idle live shell cannot look like an agent making progress.
 */
export function inventoryActivityLabel(entry: RemoteSessionInventoryEntry): InventoryActivityLabel {
  if (entry.activityState) {
    return {
      state: entry.activityState,
      detail: entry.activityState,
      source: entry.activitySource ?? "machine",
    };
  }
  return {
    state: null,
    detail: entry.running ? "running" : "stopped",
    source: "machine",
  };
}
