// One realistic PanelSnapshot matching docs/mockups/supervisor-chat/panel.html.
import type { FleetSession, PanelSnapshot, StandingRule } from './contract';

const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);
const MIN = 60_000;

const scope = { repo: null, hostId: null, agent: null };

export const defaultRulesFixture: StandingRule[] = [
  { id: 1, scope, kind: 'upstreamRetry', params: { backoffSecs: [60, 120, 300, 600], thenEverySecs: 600 }, createdFromMessageId: null, createdAtMs: 0 },
  { id: 2, scope, kind: 'stallRestart', params: { thresholdSecs: 300, maxRestarts: 1 }, createdFromMessageId: null, createdAtMs: 0 },
  { id: 3, scope, kind: 'quotaResume', params: { fallbackSecs: 600 }, createdFromMessageId: null, createdAtMs: 0 },
  { id: 4, scope, kind: 'askBeforeIrreversible', params: { enabled: true }, createdFromMessageId: null, createdAtMs: 0 },
];

function session(
  hostId: string,
  hostLabel: string,
  backendId: string,
  title: string,
  cwd: string,
  agent: string,
  state: FleetSession['state'],
  ageMin: number,
  lastAssistantLine: string | null,
  extra: Partial<FleetSession> = {},
): FleetSession {
  return {
    sessionRef: `${hostId}:${backendId}`,
    hostId,
    hostLabel,
    title,
    cwd,
    agent,
    state,
    stateSource: 'provider',
    stateSinceMs: NOW - ageMin * MIN,
    screenChangedAtMs: NOW - Math.min(ageMin, 1) * MIN,
    pendingPrompt: null,
    lastAssistantLine,
    updatedAtMs: NOW,
    stale: false,
    ...extra,
  };
}

const relayPrompt = {
  promptId: 'prompt-relay-1',
  kind: 'approval' as const,
  question: 'Run the relay database migration?',
  command: 'sqlx migrate run --database-url $RELAY_DB',
  options: ['Allow', 'Deny'],
  screenRevision: 'rev-relay-41',
};

const robloxPrompt = {
  promptId: 'prompt-roblox-1',
  kind: 'question' as const,
  question: 'Still 3 failing tests after 2 fix attempts. What next?',
  command: null,
  options: ['Revert the physics change', 'Keep trying', 'Stop'],
  screenRevision: 'rev-roblox-17',
};

export const panelSnapshotFixture: PanelSnapshot = {
  decisions: [
    {
      sessionRef: 'mac:relay',
      prompt: relayPrompt,
      recommendedIndex: 0,
      reason: 'Adds one nullable column. The relay development database on this Mac is used, so production is not touched.',
      createdAtMs: NOW - 2 * MIN,
    },
    {
      sessionRef: 'win:roblox',
      prompt: robloxPrompt,
      recommendedIndex: 0,
      reason: 'All 3 failures started with the physics change in a4c1. Reverting it passed locally.',
      createdAtMs: NOW - 4 * MIN,
    },
  ],
  handling: [
    {
      id: 5,
      incidentKey: 'upstream:proxy-502:2026-11-02T14',
      sessionRef: 'win:wintests',
      kind: 'retried',
      summary: 'Windows installer tests · Proxy 502 · tried 2 times · next try 14:20',
      basis: 'Default rule: retry upstream errors on backoff, then every 10 min',
      evidence: [{ kind: 'stateTransition', id: 'seq-8841' }],
      undo: null,
      live: true,
      createdAtMs: NOW - 9 * MIN,
      updatedAtMs: NOW - MIN,
    },
  ],
  sessions: [
    session('mac', 'This Mac', 'relay', 'Fix relay reconnect', '/work/ferryx/relay-fix', 'claude', 'blocked', 2, 'Running reconnect tests needs the new column.', { pendingPrompt: relayPrompt }),
    session('win', 'maho-win', 'roblox', 'Roblox build', '/work/roblox-game-forge', 'codex', 'blocked', 4, '3 tests failed in PlayerController after the physics change.', { pendingPrompt: robloxPrompt }),
    session('win', 'maho-win', 'wintests', 'Windows installer tests', '/work/ferryx/win-tests', 'omo', 'working', 9, 'cargo test --test msix_install', { screenChangedAtMs: NOW - 9 * MIN }),
    session('omarchy', 'omarchy', 'notes', 'Release notes for 11.2', '/work/site', 'codex', 'working', 6, 'Tagging v2026.1102.1'),
    session('mac', 'This Mac', 'wtlist', 'Worktree list refresh', '/work/ferryx', 'claude', 'idle', 60, 'Done. The worktree list now refreshes on focus and after git operations.'),
  ],
  log: [
    {
      id: 4,
      incidentKey: 'answer:notes:version-tag',
      sessionRef: 'omarchy:notes',
      kind: 'answered',
      summary: 'Answered Release notes: v2026.1102.1',
      basis: 'You picked the stable tag the last 4 times',
      evidence: [{ kind: 'receipt', id: 'sup-answer-notes-1' }, { kind: 'answerHistory', id: 'hist-12' }],
      undo: { kind: 'reopenPrompt', payload: { promptId: 'prompt-notes-1' } },
      live: false,
      createdAtMs: NOW - 13 * MIN,
      updatedAtMs: NOW - 13 * MIN,
    },
    {
      id: 3,
      incidentKey: 'stall:mac:gestures',
      sessionRef: 'mac:gestures',
      kind: 'resumed',
      summary: 'Restarted Sheet gestures',
      basis: 'No output for 5 min while reporting Working',
      evidence: [{ kind: 'stateTransition', id: 'seq-8790' }],
      undo: null,
      live: false,
      createdAtMs: NOW - 16 * MIN,
      updatedAtMs: NOW - 16 * MIN,
    },
    {
      id: 2,
      incidentKey: 'quota:omarchy:mhc',
      sessionRef: 'omarchy:mhc',
      kind: 'resumed',
      summary: 'Resumed mhc cleanup when its usage limit reset',
      basis: 'Default rule: resume when the usage limit resets',
      evidence: [{ kind: 'receipt', id: 'receipt-221' }],
      undo: null,
      live: false,
      createdAtMs: NOW - 19 * MIN,
      updatedAtMs: NOW - 19 * MIN,
    },
    {
      id: 1,
      incidentKey: 'upstream:proxy-502:2026-11-02T13',
      sessionRef: null,
      kind: 'resumed',
      summary: 'Proxy errors 13:20-13:50 · retried every 10 min · resumed 4 sessions',
      basis: 'Default rule: retry upstream errors on backoff, then every 10 min',
      evidence: [{ kind: 'receipt', id: 'receipt-198' }],
      undo: null,
      live: false,
      createdAtMs: NOW - 57 * MIN,
      updatedAtMs: NOW - 27 * MIN,
    },
  ],
  rules: defaultRulesFixture,
  paused: false,
  setupRequired: false,
};
