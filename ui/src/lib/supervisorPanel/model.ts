// Pure view model for the supervisor panel (plan D5/D9/D11/D12).
// `applyPanelEvent` folds owner event frames into a PanelSnapshot and
// `buildPanelView` derives the sections the panel renders. No I/O, no clocks:
// callers pass `nowMs`.
import type {
  ActionEntry,
  ActionKind,
  Escalation,
  FleetSession,
  FleetSessionState,
  PanelEvent,
  PanelSnapshot,
  PendingPrompt,
  RuleKind,
  SessionRef,
  StandingRule,
} from "./contract";

const MINUTE_MS = 60_000;
const DEFAULT_STALL_THRESHOLD_SECS = 300;
const LOG_COLLAPSED_COUNT = 2;

/** D12 glyphs a session row can carry. */
export type SessionGlyph = "needsYou" | "working" | "idle";

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
  /** One-line why: last assistant line, or "status unknown". */
  why: string;
  stale: boolean;
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
}

// ---------------------------------------------------------------------------
// Reducer

function withoutAction(list: ActionEntry[], id: number): ActionEntry[] {
  return list.filter((entry) => entry.id !== id);
}

export function applyPanelEvent(
  snapshot: PanelSnapshot,
  event: PanelEvent,
): PanelSnapshot {
  switch (event.type) {
    case "panel.decision": {
      const rest = snapshot.decisions.filter(
        (decision) => decision.sessionRef !== event.sessionRef,
      );
      return {
        ...snapshot,
        decisions: event.decision ? [...rest, event.decision] : rest,
      };
    }
    case "panel.action": {
      const handling = withoutAction(snapshot.handling, event.action.id);
      const log = withoutAction(snapshot.log, event.action.id);
      return event.action.live
        ? { ...snapshot, handling: [...handling, event.action], log }
        : { ...snapshot, handling, log: [...log, event.action] };
    }
    case "panel.session": {
      const rest = snapshot.sessions.filter(
        (session) => session.sessionRef !== event.session.sessionRef,
      );
      return { ...snapshot, sessions: [...rest, event.session] };
    }
    case "panel.rules":
      return { ...snapshot, rules: event.rules };
  }
}

// ---------------------------------------------------------------------------
// Selectors

export function formatStatusLine(decisionCount: number): string {
  if (decisionCount <= 0) return "Nothing needs you.";
  const noun = decisionCount === 1 ? "decision" : "decisions";
  return `${decisionCount} ${noun} for you. The rest is running.`;
}

function formatMinutes(ms: number): string {
  const minutes = Math.max(1, Math.floor(ms / MINUTE_MS));
  return minutes < 120 ? `${minutes} min` : `${Math.floor(minutes / 60)} h`;
}

function validateEscalation(escalation: Escalation): string | null {
  const index: unknown = escalation.recommendedIndex;
  if (typeof index !== "number" || !Number.isInteger(index)) {
    return "missing recommendedIndex";
  }
  if (index < 0 || index >= escalation.prompt.options.length) {
    return "recommendedIndex out of range";
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

function actionView(entry: ActionEntry): ActionView {
  return {
    id: entry.id,
    sessionRef: entry.sessionRef,
    kind: entry.kind,
    summary: entry.summary,
    basis: entry.basis,
    unverified: CLAIM_KINDS.has(entry.kind) && entry.evidence.length === 0,
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
  if (latestGlobalRule(rules, "upstreamRetry")) {
    parts.push("retries upstream errors on a backoff");
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

function glyphFor(state: FleetSessionState): SessionGlyph {
  switch (state) {
    case "working":
      return "working";
    case "blocked":
      return "needsYou";
    // D12: unknown never claims Working.
    case "idle":
    case "unknown":
      return "idle";
  }
}

const GLYPH_RANK: Record<SessionGlyph, number> = { needsYou: 0, working: 1, idle: 2 };

function sessionRow(
  session: FleetSession,
  nowMs: number,
  stallMs: number,
): SessionRowView {
  const glyph = glyphFor(session.state);
  const stalled =
    session.state === "working" &&
    session.stateSource === "provider" &&
    session.screenChangedAtMs !== null &&
    nowMs - session.screenChangedAtMs >= stallMs;
  let age: string | null = null;
  if (stalled && session.screenChangedAtMs !== null) {
    age = `screen unchanged for ${formatMinutes(nowMs - session.screenChangedAtMs)}`;
  } else if (session.state === "blocked" && session.stateSinceMs !== null) {
    age = `waiting ${formatMinutes(nowMs - session.stateSinceMs)}`;
  }
  return {
    sessionRef: session.sessionRef,
    title: session.title,
    hostLabel: session.hostLabel,
    glyph,
    stalled,
    age,
    why: session.state === "unknown" ? "status unknown" : (session.lastAssistantLine ?? ""),
    stale: session.stale,
    injectable: session.state === "blocked",
  };
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
      reason: escalation.reason,
      options: orderedOptions(escalation.prompt, escalation.recommendedIndex),
      createdAtMs: escalation.createdAtMs,
    };
  });

  const liveHandling = snapshot.handling.filter((entry) => entry.live);
  const handling = [...liveHandling].sort(newestFirst).map(actionView);

  const shownRefs = new Set<SessionRef>(decisionByRef.keys());
  for (const entry of liveHandling) {
    if (entry.sessionRef) shownRefs.add(entry.sessionRef);
  }
  const stallMs = stallThresholdMs(snapshot.rules);
  const sessions = [...fleet.values()]
    .filter((session) => !shownRefs.has(session.sessionRef))
    .map((session) => sessionRow(session, nowMs, stallMs))
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
    (session) => session.state === "working",
  ).length;

  return {
    statusLine: formatStatusLine(decisions.length),
    headerLine: `${runningCount} running · handled ${handledToday} ${
      handledToday === 1 ? "thing" : "things"
    } today`,
    runningCount,
    handledToday,
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

function exactNames(session: FleetSession): string[] {
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
  sessions: FleetSession[],
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

export function parseMentions(text: string, sessions: FleetSession[]): ParsedMentions {
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
