// Pure view model for the supervisor panel (plan D5/D9/D11/D12).
// `applyPanelEvent` folds owner event frames into a PanelSnapshot and
// `buildPanelView` derives the sections the panel renders. No I/O, no clocks:
// callers pass `nowMs`.
import { groupOf, type AttentionGroup } from "../attentionGroup";
import type {
  ActionEntry,
  ActionKind,
  Escalation,
  EvidenceRef,
  FleetSession,
  PanelEvent,
  PanelSnapshot,
  PendingPrompt,
  RuleKind,
  SessionHealth,
  SessionRef,
  StandingRule,
} from "./contract";

const MINUTE_MS = 60_000;
const DEFAULT_STALL_THRESHOLD_SECS = 300;
const LOG_COLLAPSED_COUNT = 2;

/**
 * D12 glyphs a session row can carry, drawn from the session's attention group (`groupOf`).
 * `failed` is a needs-you row whose health says the agent died: a fresh `failed` health
 * observation passed in `PanelViewOptions.health`, or the session's own `health`.
 */
export type SessionGlyph = "needsYou" | "failed" | "unread" | "working" | "idle";

export interface DecisionOptionView {
  /** Index into the prompt's original `options` (what the answer route takes). */
  index: number;
  label: string;
  recommended: boolean;
}

export interface DecisionView {
  sessionRef: SessionRef;
  sessionTitle: string;
  hostLabel: string | null;
  promptId: string;
  screenRevision: string | null;
  question: string;
  command: string | null;
  reason: string;
  /** Recommended option first, the rest in their original order. */
  options: DecisionOptionView[];
  createdAtMs: number;
}

/** An escalation dropped from `decisions` because it breaks the contract. */
export interface RejectedEscalation {
  sessionRef: SessionRef;
  reason: string;
}

export interface ActionView {
  id: number;
  sessionRef: SessionRef | null;
  kind: ActionKind;
  summary: string;
  basis: string;
  /** D9: a supervisor claim with no evidence renders the grey "Unverified" chip. */
  unverified: boolean;
  undoable: boolean;
  live: boolean;
  createdAtMs: number;
}

export interface SessionRowView {
  sessionRef: SessionRef;
  title: string;
  hostLabel: string;
  glyph: SessionGlyph;
  /** Provider-reported Working whose screen has not changed past the stall threshold. */
  stalled: boolean;
  /** "screen unchanged for 7 min" (stalled) / "waiting 23 min" (blocked) / null. */
  age: string | null;
  /** Why the session stopped, in one line (daemon `reason`); null when it has nothing to say. */
  reason: string | null;
  /** One-line why: last assistant line, or "status unknown". */
  why: string;
  stale: boolean;
  /** Blocked with a prompt the hub has not turned into a decision card (yet). */
  judging: boolean;
  /** Blocked rows are waiting; an inject-now nudge makes sense. */
  injectable: boolean;
}

export interface LogView {
  /** Newest first, collapsed to the last two. */
  visible: ActionView[];
  /** Newest first, every non-live entry ("All N"). */
  all: ActionView[];
  total: number;
}

export interface PanelView {
  statusLine: string;
  headerLine: string;
  runningCount: number;
  handledToday: number;
  /** Sessions waiting on the owner that no decision card covers: the headline may not say "all clear". */
  waitingCount: number;
  paused: boolean;
  setupRequired: boolean;
  decisions: DecisionView[];
  handling: ActionView[];
  sessions: SessionRowView[];
  log: LogView;
  rulesLine: string;
  rejected: RejectedEscalation[];
}

export interface PanelViewOptions {
  nowMs: number;
  /** Start of the owner's local day; defaults to local midnight of `nowMs`. */
  startOfDayMs?: number;
  /** Latest health per session, when the caller has it (not part of `PanelSnapshot`). */
  health?: ReadonlyMap<SessionRef, SessionHealth>;
}

// ---------------------------------------------------------------------------
// Reducer

/** Sorted newest first, the order the hub lists them in. */
function sortedActions(list: ActionEntry[]): ActionEntry[] {
  return [...list].sort(newestFirst);
}

/**
 * Whether `next` may replace `current` (same action id). Older reports never win; at the same
 * instant a completion is terminal, so a repeated live frame cannot reopen a finished action.
 */
function supersedes(next: ActionEntry, current: ActionEntry): boolean {
  if (next.updatedAtMs !== current.updatedAtMs) return next.updatedAtMs > current.updatedAtMs;
  return !(next.live && !current.live);
}

function replaceOrAppend<T>(list: T[], match: (item: T) => boolean, next: T): T[] {
  const index = list.findIndex(match);
  if (index < 0) return [...list, next];
  const copy = list.slice();
  copy[index] = next;
  return copy;
}

/**
 * The one reducer for owner `panel.*` frames, used by the desktop host and the phone alike.
 *
 * - `panel.action`: upsert by action `id`. A live frame lives in `handling`, a finished one in
 *   `log` (newest first); the other list drops it, so an action is never in both. A frame older
 *   than the stored one (`updatedAtMs`) is ignored, so a late live frame cannot resurrect a
 *   completed action; a genuinely newer live frame (the incident came back) reopens it.
 * - `panel.decision`: `null` removes the session's decision; otherwise it replaces it in place
 *   (new ones append). An older prompt never replaces a newer one, and an escalation that breaks
 *   the contract (no usable recommendation, blank reason) is ignored, never shown.
 * - `panel.session`: upsert by ref; an older report never replaces a newer one.
 * - `panel.rules`: replaces the list.
 */
export function applyPanelEvent(
  snapshot: PanelSnapshot,
  event: PanelEvent,
): PanelSnapshot {
  switch (event.type) {
    case "panel.decision": {
      const { decision } = event;
      if (decision === null) {
        return {
          ...snapshot,
          decisions: snapshot.decisions.filter((item) => item.sessionRef !== event.sessionRef),
        };
      }
      if (decision.sessionRef !== event.sessionRef || validateEscalation(decision) !== null) {
        return snapshot;
      }
      const current = snapshot.decisions.find((item) => item.sessionRef === event.sessionRef);
      if (current && decision.createdAtMs < current.createdAtMs) return snapshot;
      return {
        ...snapshot,
        decisions: replaceOrAppend(
          snapshot.decisions,
          (item) => item.sessionRef === event.sessionRef,
          decision,
        ),
      };
    }
    case "panel.action": {
      const { action } = event;
      const known = [...snapshot.handling, ...snapshot.log].filter((entry) => entry.id === action.id);
      if (!known.every((entry) => supersedes(action, entry))) return snapshot;
      const handling = snapshot.handling.filter((entry) => entry.id !== action.id);
      const log = snapshot.log.filter((entry) => entry.id !== action.id);
      return action.live
        ? { ...snapshot, handling: sortedActions([...handling, action]), log }
        : { ...snapshot, handling, log: sortedActions([...log, action]) };
    }
    case "panel.session": {
      const { session } = event;
      const current = snapshot.sessions.find((item) => item.sessionRef === session.sessionRef);
      if (current && session.updatedAtMs < current.updatedAtMs) return snapshot;
      return {
        ...snapshot,
        sessions: replaceOrAppend(
          snapshot.sessions,
          (item) => item.sessionRef === session.sessionRef,
          session,
        ),
      };
    }
    case "panel.rules":
      return { ...snapshot, rules: event.rules };
  }
}

// ---------------------------------------------------------------------------
// Selectors

export function formatStatusLine(decisionCount: number, waitingCount = 0, judging = 0): string {
  if (decisionCount > 0) {
    const noun = decisionCount === 1 ? "decision" : "decisions";
    return `${decisionCount} ${noun} for you. The rest is running.`;
  }
  // A blocked session the hub has not escalated is still work the owner must do: never report
  // the all-clear while one is on screen, and say plainly when the hub is still deciding.
  if (waitingCount > 0) {
    const noun = waitingCount === 1 ? "lane is" : "lanes are";
    const tail = judging > 0 ? " The supervisor is still deciding." : "";
    return `${waitingCount} ${noun} waiting for you.${tail}`;
  }
  return "Nothing needs you.";
}

function formatMinutes(ms: number): string {
  const minutes = Math.max(1, Math.floor(ms / MINUTE_MS));
  return minutes < 120 ? `${minutes} min` : `${Math.floor(minutes / 60)} h`;
}

export function validateEscalation(escalation: Escalation): string | null {
  const index: unknown = escalation.recommendedIndex;
  if (typeof index !== "number" || !Number.isInteger(index)) {
    return "missing recommendedIndex";
  }
  if (index < 0 || index >= escalation.prompt.options.length) {
    return "recommendedIndex out of range";
  }
  const reason: unknown = escalation.reason;
  if (typeof reason !== "string" || reason.trim() === "") {
    return "missing reason";
  }
  return null;
}

function orderedOptions(
  prompt: PendingPrompt,
  recommendedIndex: number,
): DecisionOptionView[] {
  const all = prompt.options.map((label, index) => ({
    index,
    label,
    recommended: index === recommendedIndex,
  }));
  return [
    ...all.filter((option) => option.recommended),
    ...all.filter((option) => !option.recommended),
  ];
}

const CLAIM_KINDS: ReadonlySet<ActionKind> = new Set<ActionKind>([
  "answered",
  "retried",
  "resumed",
  "cancelled",
  "stopped",
  "finished",
]);

/**
 * D9: what counts as a delivery event. A ref to a prompt, a session or a goal names what the
 * claim is about, and an answer-history entry explains why an answer was chosen; neither proves
 * anything happened. Only a delivery receipt, a state-transition sequence or a commit does.
 */
export const DELIVERY_EVIDENCE_KINDS: ReadonlySet<string> = new Set([
  "receipt",
  "delivery",
  "stateTransition",
  "commit",
]);

export function hasDeliveryEvidence(evidence: readonly EvidenceRef[]): boolean {
  return evidence.some((ref) => DELIVERY_EVIDENCE_KINDS.has(ref.kind));
}

/** A supervisor chat reply is a claim unless it carries a delivery event or the rule it wrote. */
export function isReplyUnverified(evidence: readonly EvidenceRef[]): boolean {
  return !(hasDeliveryEvidence(evidence) || evidence.some((ref) => ref.kind === "rule"));
}

function actionView(entry: ActionEntry): ActionView {
  return {
    id: entry.id,
    sessionRef: entry.sessionRef,
    kind: entry.kind,
    summary: entry.summary,
    basis: entry.basis,
    unverified: CLAIM_KINDS.has(entry.kind) && !hasDeliveryEvidence(entry.evidence),
    undoable: entry.undo !== null,
    live: entry.live,
    createdAtMs: entry.createdAtMs,
  };
}

function newestFirst(a: ActionEntry, b: ActionEntry): number {
  return b.createdAtMs - a.createdAtMs || b.id - a.id;
}

function latestGlobalRule(
  rules: StandingRule[],
  kind: RuleKind,
): StandingRule | null {
  let found: StandingRule | null = null;
  for (const rule of rules) {
    const { repo, hostId, agent } = rule.scope;
    if (rule.kind !== kind || repo || hostId || agent) continue;
    if (!found || rule.id > found.id) found = rule;
  }
  return found;
}

function stallThresholdMs(rules: StandingRule[]): number {
  const params = latestGlobalRule(rules, "stallRestart")?.params;
  const secs = params?.thresholdSecs;
  return (typeof secs === "number" && secs > 0 ? secs : DEFAULT_STALL_THRESHOLD_SECS) * 1000;
}

export function formatRulesLine(rules: StandingRule[]): string {
  const parts: string[] = [];
  const retry = latestGlobalRule(rules, "upstreamRetry");
  if (retry) {
    // A plain-language rule change ("retry every 5 min") stores one fixed interval; show it, so
    // the owner sees the line change. The default rule keeps its backoff wording.
    const backoff = retry.params.backoffSecs;
    const every = retry.params.thenEverySecs;
    const fixed =
      typeof every === "number" && every > 0 && Array.isArray(backoff) && backoff.every((secs) => secs === every);
    parts.push(fixed ? `retries upstream errors every ${formatMinutes(every * 1000)}` : "retries upstream errors on a backoff");
  }
  const stall = latestGlobalRule(rules, "stallRestart");
  if (stall) {
    parts.push(`restarts sessions silent for ${formatMinutes(stallThresholdMs(rules))}`);
  }
  if (latestGlobalRule(rules, "quotaResume")) {
    parts.push("resumes after usage limits reset");
  }
  const ask = latestGlobalRule(rules, "askBeforeIrreversible");
  if (ask && ask.params.enabled !== false) {
    parts.push("asks before anything irreversible");
  }
  const scoped = rules.filter(
    (rule) => rule.scope.repo || rule.scope.hostId || rule.scope.agent,
  ).length;
  const extra =
    scoped > 0 ? ` (+${scoped} ${scoped === 1 ? "rule" : "rules"} for specific projects)` : "";
  if (parts.length === 0) return `No standing rules${extra}.`;
  const head = parts.slice(0, -1).join(", ");
  const body = head ? `${head}, and ${parts[parts.length - 1]}` : parts[0];
  return `${body.charAt(0).toUpperCase()}${body.slice(1)}${extra}.`;
}

const GROUP_GLYPH: Record<AttentionGroup, SessionGlyph> = {
  needsYou: "needsYou",
  unread: "unread",
  working: "working",
  seen: "idle",
};

const GLYPH_RANK: Record<SessionGlyph, number> = { needsYou: 0, failed: 1, unread: 2, working: 3, idle: 4 };

/** A failed health report counts only while the session has not changed state since it. */
function hasFailed(session: FleetSession, health: SessionHealth | undefined): boolean {
  // Telemetry on the session itself: the daemon says the agent died mid-turn and no newer state
  // has replaced the report since (a new turn clears it on the daemon).
  if (!session.stale && session.health?.type === "failed" && session.state !== "working") return true;
  return (
    !session.stale &&
    health?.kind.type === "failed" &&
    health.observedAtMs >= (session.stateSinceMs ?? 0)
  );
}

function sessionRow(
  session: FleetSession,
  nowMs: number,
  stallMs: number,
  health: SessionHealth | undefined,
): SessionRowView {
  const failed = hasFailed(session, health);
  const group = groupOf({ attention: session.attention, state: session.state, failed });
  const glyph = group === "needsYou" && failed ? "failed" : GROUP_GLYPH[group];
  // A prompt the hub holds but has not escalated: the supervisor is judging this lane.
  const judging = session.pendingPrompt !== null && !session.stale;
  const stalled =
    session.state === "working" &&
    session.stateSource === "provider" &&
    session.screenChangedAtMs !== null &&
    nowMs - session.screenChangedAtMs >= stallMs;
  let age: string | null = null;
  if (stalled && session.screenChangedAtMs !== null) {
    age = `screen unchanged for ${formatMinutes(nowMs - session.screenChangedAtMs)}`;
  } else if (session.state === "blocked") {
    const waited =
      session.stateSinceMs !== null ? `waiting ${formatMinutes(nowMs - session.stateSinceMs)}` : null;
    // Plain words, not the hub's term: the supervisor holds this prompt and has not decided yet.
    age = judging ? (waited ? `supervisor deciding \u00b7 ${waited}` : "supervisor deciding") : waited;
  }
  return {
    sessionRef: session.sessionRef,
    title: session.title,
    hostLabel: session.hostLabel,
    glyph,
    stalled,
    age,
    reason: session.reason?.trim() ? session.reason : null,
    why: session.state === "unknown" ? "status unknown" : (session.lastAssistantLine ?? ""),
    stale: session.stale,
    judging,
    injectable: session.state === "blocked",
  };
}

const UUID_RE = /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;
/** A hex token with at least one digit and one letter: a commit hash or an internal id. */
const HEX_ID_RE = /\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{4,40}\b/gi;

/**
 * Plan "Must NOT have": no raw ids in default views. A reason is free text from the hub (model
 * output, error strings), so a commit hash or uuid in it is elided rather than shown.
 */
export function plainReason(reason: string): string {
  return reason.replace(UUID_RE, "\u2026").replace(HEX_ID_RE, "\u2026");
}

/** Of several live incidents on one session, the one updated last is what is happening now. */
function moreCurrent(a: ActionEntry, b: ActionEntry): boolean {
  return (a.updatedAtMs - b.updatedAtMs || a.createdAtMs - b.createdAtMs || a.id - b.id) > 0;
}

function localStartOfDay(nowMs: number): number {
  const day = new Date(nowMs);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
}

export function buildPanelView(
  snapshot: PanelSnapshot,
  options: PanelViewOptions,
): PanelView {
  const { nowMs } = options;
  const startOfDayMs = options.startOfDayMs ?? localStartOfDay(nowMs);

  // One FleetSession per ref; the freshest report wins.
  const fleet = new Map<SessionRef, FleetSession>();
  for (const session of snapshot.sessions) {
    const prev = fleet.get(session.sessionRef);
    if (!prev || session.updatedAtMs >= prev.updatedAtMs) {
      fleet.set(session.sessionRef, session);
    }
  }

  const rejected: RejectedEscalation[] = [];
  const decisionByRef = new Map<SessionRef, Escalation>();
  for (const escalation of snapshot.decisions) {
    const problem = validateEscalation(escalation);
    if (problem) {
      rejected.push({ sessionRef: escalation.sessionRef, reason: problem });
      continue;
    }
    const prev = decisionByRef.get(escalation.sessionRef);
    if (!prev || escalation.createdAtMs >= prev.createdAtMs) {
      decisionByRef.set(escalation.sessionRef, escalation);
    }
  }
  const decisions: DecisionView[] = [...decisionByRef.values()].map((escalation) => {
    const session = fleet.get(escalation.sessionRef);
    return {
      sessionRef: escalation.sessionRef,
      sessionTitle: session?.title ?? escalation.sessionRef,
      hostLabel: session?.hostLabel ?? null,
      promptId: escalation.prompt.promptId,
      screenRevision: escalation.prompt.screenRevision,
      question: escalation.prompt.question,
      command: escalation.prompt.command,
      reason: plainReason(escalation.reason),
      options: orderedOptions(escalation.prompt, escalation.recommendedIndex),
      createdAtMs: escalation.createdAtMs,
    };
  });

  // A session is in exactly one section: Needs you > Handling > Sessions. A session with a
  // decision leaves Handling, and several live incidents on one session collapse to one row.
  const fleetWide: ActionEntry[] = [];
  const handlingByRef = new Map<SessionRef, ActionEntry>();
  for (const entry of snapshot.handling) {
    if (!entry.live) continue;
    const ref = entry.sessionRef;
    if (ref === null) {
      fleetWide.push(entry);
      continue;
    }
    if (decisionByRef.has(ref)) continue;
    const prev = handlingByRef.get(ref);
    if (!prev || moreCurrent(entry, prev)) handlingByRef.set(ref, entry);
  }
  const liveHandling = [...fleetWide, ...handlingByRef.values()];
  const handling = [...liveHandling].sort(newestFirst).map(actionView);

  const shownRefs = new Set<SessionRef>(decisionByRef.keys());
  for (const entry of liveHandling) {
    if (entry.sessionRef) shownRefs.add(entry.sessionRef);
  }
  const stallMs = stallThresholdMs(snapshot.rules);
  const sessions = [...fleet.values()]
    .filter((session) => !shownRefs.has(session.sessionRef))
    .map((session) => sessionRow(session, nowMs, stallMs, options.health?.get(session.sessionRef)))
    .sort(
      (a, b) =>
        GLYPH_RANK[a.glyph] - GLYPH_RANK[b.glyph] || a.title.localeCompare(b.title),
    );

  const doneEntries = snapshot.log.filter((entry) => !entry.live).sort(newestFirst);
  const all = doneEntries.map(actionView);
  const handledToday = doneEntries.filter(
    (entry) => entry.createdAtMs >= startOfDayMs,
  ).length;
  const runningCount = [...fleet.values()].filter(
    (session) => groupOf({ attention: session.attention, state: session.state }) === "working",
  ).length;

  const waitingCount = sessions.filter(
    (row) => row.glyph === "needsYou" || row.glyph === "failed",
  ).length;
  const judgingCount = sessions.filter((row) => row.judging).length;

  return {
    statusLine: formatStatusLine(decisions.length, waitingCount, judgingCount),
    headerLine: `${runningCount} running · handled ${handledToday} ${
      handledToday === 1 ? "thing" : "things"
    } today`,
    runningCount,
    handledToday,
    waitingCount,
    paused: snapshot.paused,
    setupRequired: snapshot.setupRequired,
    decisions,
    handling,
    sessions,
    log: { visible: all.slice(0, LOG_COLLAPSED_COUNT), all, total: all.length },
    rulesLine: formatRulesLine(snapshot.rules),
    rejected,
  };
}

// ---------------------------------------------------------------------------
// Mentions (D11)

/** What a mention can name: the fields the resolver reads. */
export type MentionTarget = Pick<FleetSession, "sessionRef" | "title" | "hostLabel">;

export interface AmbiguousMention {
  /** The matched text including the leading "@". */
  mention: string;
  candidates: SessionRef[];
}

export interface ParsedMentions {
  /** Uniquely resolved sessions, in order of first mention. */
  targets: SessionRef[];
  /** Mentions matching several sessions; never guessed. */
  ambiguous: AmbiguousMention[];
}

const NAME_CHAR = /[A-Za-z0-9_-]/;

function slug(text: string): string {
  return text.trim().toLowerCase().replace(/\s+/g, "-");
}

function backendId(ref: SessionRef): string {
  return ref.slice(ref.indexOf(":") + 1);
}

function exactNames(session: MentionTarget): string[] {
  return [
    `${session.title} · ${session.hostLabel}`,
    session.title,
    session.sessionRef,
    slug(session.title),
    backendId(session.sessionRef),
  ].map((name) => name.toLowerCase());
}

function resolveAt(
  rest: string,
  sessions: readonly MentionTarget[],
): { length: number; refs: SessionRef[] } | null {
  const lower = rest.toLowerCase();

  // 1) Longest exact name followed by a word boundary ("@Roblox build please").
  let best = 0;
  let refs: SessionRef[] = [];
  for (const session of sessions) {
    for (const name of exactNames(session)) {
      if (!name || !lower.startsWith(name)) continue;
      if (NAME_CHAR.test(rest.charAt(name.length))) continue;
      if (name.length > best) {
        best = name.length;
        refs = [session.sessionRef];
      } else if (name.length === best && !refs.includes(session.sessionRef)) {
        refs.push(session.sessionRef);
      }
    }
  }
  if (best > 0) return { length: best, refs };

  // 2) Unique-or-ambiguous prefix of the typed token ("@roblox").
  const token = /^[^\s@]+/.exec(rest)?.[0];
  if (!token) return null;
  const prefix = token.toLowerCase();
  const matches = sessions
    .filter((session) =>
      [slug(session.title), backendId(session.sessionRef).toLowerCase()].some((name) =>
        name.startsWith(prefix),
      ),
    )
    .map((session) => session.sessionRef);
  return matches.length > 0 ? { length: token.length, refs: [...new Set(matches)] } : null;
}

export function parseMentions(text: string, sessions: readonly MentionTarget[]): ParsedMentions {
  const targets: SessionRef[] = [];
  const ambiguous: AmbiguousMention[] = [];
  let i = 0;
  while (i < text.length) {
    const at = text.indexOf("@", i);
    if (at < 0) break;
    const resolved = resolveAt(text.slice(at + 1), sessions);
    if (!resolved) {
      i = at + 1;
      continue;
    }
    const mention = text.slice(at, at + 1 + resolved.length);
    if (resolved.refs.length === 1) {
      if (!targets.includes(resolved.refs[0])) targets.push(resolved.refs[0]);
    } else {
      ambiguous.push({ mention, candidates: resolved.refs });
    }
    i = at + 1 + resolved.length;
  }
  return { targets, ambiguous };
}
