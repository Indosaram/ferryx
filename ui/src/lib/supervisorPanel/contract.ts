// Frozen shared contract for the autonomous supervisor panel. Mirrors
// crates/ferryx-hub/src/supervisor/panel_contract.rs (wire shape, camelCase).
// Routes, commands and events: docs/plans/SUPERVISOR_PANEL_CONTRACT.md.

/** `<hostId>:<backendSessionId>` */
export type SessionRef = string;

export type HealthKind =
  | { type: 'healthy' }
  | { type: 'upstreamError'; status: number | null }
  | { type: 'quota'; resetAtMs: number | null }
  | { type: 'stalled'; sinceMs: number }
  | { type: 'failed'; reason: string };

export interface SessionHealth {
  sessionRef: SessionRef;
  kind: HealthKind;
  observedAtMs: number;
}

export type PromptKind = 'question' | 'permission' | 'approval';

export interface PendingPrompt {
  promptId: string;
  kind: PromptKind;
  question: string;
  command: string | null;
  options: string[];
  screenRevision: string | null;
}

export type FleetSessionState = 'working' | 'blocked' | 'idle' | 'unknown';

export interface FleetSession {
  sessionRef: SessionRef;
  hostId: string;
  hostLabel: string;
  title: string;
  cwd: string;
  agent: string;
  state: FleetSessionState;
  stateSource: string;
  /** ms epoch when the current state was entered (daemon `stateSince`). */
  stateSinceMs: number | null;
  /** ms epoch when the screen revision last changed (daemon `screenChangedAt`). */
  screenChangedAtMs: number | null;
  pendingPrompt: PendingPrompt | null;
  lastAssistantLine: string | null;
  updatedAtMs: number;
  stale: boolean;
}

export type ActionKind =
  | 'answered'
  | 'retried'
  | 'resumed'
  | 'cancelled'
  | 'stopped'
  | 'ruleChanged'
  | 'finished'
  | 'escalated';

export interface EvidenceRef {
  kind: string;
  id: string;
}

export interface UndoHandle {
  kind: string;
  payload: unknown;
}

export interface ActionEntry {
  id: number;
  incidentKey: string;
  sessionRef: SessionRef | null;
  kind: ActionKind;
  summary: string;
  basis: string;
  evidence: EvidenceRef[];
  undo: UndoHandle | null;
  /** True while the incident is still being handled. */
  live: boolean;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface Escalation {
  sessionRef: SessionRef;
  prompt: PendingPrompt;
  /** Mandatory: index into `prompt.options`. */
  recommendedIndex: number;
  reason: string;
  createdAtMs: number;
}

export type AnswerDecision =
  | { type: 'answer'; optionIndex: number; basis: string }
  | { type: 'escalate'; recommendedIndex: number; reason: string };

export type RuleKind =
  | 'upstreamRetry'
  | 'stallRestart'
  | 'quotaResume'
  | 'askBeforeIrreversible';

export interface RuleScope {
  repo: string | null;
  hostId: string | null;
  agent: string | null;
}

export interface StandingRule {
  id: number;
  scope: RuleScope;
  kind: RuleKind;
  /**
   * upstreamRetry {backoffSecs:number[], thenEverySecs} | stallRestart {thresholdSecs, maxRestarts}
   * | quotaResume {fallbackSecs} | askBeforeIrreversible {enabled}
   */
  params: Record<string, unknown>;
  createdFromMessageId: string | null;
  createdAtMs: number;
}

export interface PanelSnapshot {
  decisions: Escalation[];
  /** Live action entries (`live === true`). */
  handling: ActionEntry[];
  sessions: FleetSession[];
  log: ActionEntry[];
  rules: StandingRule[];
  paused: boolean;
  setupRequired: boolean;
}

// Route payloads.
export interface FleetSessionsResponse {
  sessions: FleetSession[];
}

export interface ConverseRequest {
  text: string;
  /** SessionRefs of `@session` mentions. */
  mentions: SessionRef[];
  requestId: string;
}

export interface PauseRequest {
  paused: boolean;
}

export interface AnswerRequest {
  promptId: string;
  screenRevision: string | null;
  optionIndex: number;
}

// Owner event frames on /api/v1/events.
export type PanelEvent =
  | { type: 'panel.decision'; sessionRef: SessionRef; decision: Escalation | null }
  | { type: 'panel.action'; action: ActionEntry }
  | { type: 'panel.session'; session: FleetSession }
  | { type: 'panel.rules'; rules: StandingRule[] };

/** Text sent to resume a session; null means unknown agent, escalate instead. */
export function resumeText(agent: string): string | null {
  switch (agent) {
    case 'claude':
    case 'claude-code':
    case 'codex':
      return 'continue';
    default:
      return null;
  }
}
