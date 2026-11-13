import { type ReactNode } from 'react';

import type { PanelActions } from '../../lib/supervisorPanel/actions';
import { panelSnapshotFixture } from '../../lib/supervisorPanel/fixtures';
import type { PanelSnapshot, SessionHealth } from '../../lib/supervisorPanel/contract';
import type { PrLane, PullRequest } from '../../lib/supervisorPanel/contract';
import type { WorktreeOnBranch } from '../../lib/supervisorPanel/prLane';
import type { LegacyGoal } from '../../lib/supervisorPanel/legacyGoals';
import type { TranscriptMessage } from '../../lib/supervisorPanel/transcript';
import type { SupervisorHubState } from '../../lib/supervisorModel';
import { DecisionCard } from './DecisionCard';
import { LeaseNotices, StandbyCard } from './LeaseStatus';
import { PanelSurface } from './PanelSurface';
import { StatusGlyph } from './StatusGlyph';
import { GLYPH_LABEL, type GlyphState } from './types';

// Showcase only: every write is inert. The mapping from PanelSnapshot is the shared model's.
const noop = () => undefined;
const ok = () => Promise.resolve({ ok: true } as const);
const inertActions: PanelActions = {
  answer: ok,
  setPaused: ok,
  setLanePaused: ok,
  converse: ok,
  stop: ok,
  undo: ok,
  turnOn: ok,
  openSession: noop,
  handOffPr: ok,
};

// The fixture's clock, so ages read the same on every render.
const SHOWCASE_NOW = Date.UTC(2026, 10, 2, 14, 17, 0);

/** One finished session reads Idle; a Failed row needs a fresh failed health report. */
const failedHealth: ReadonlyMap<string, SessionHealth> = new Map([
  [
    'mac:wtlist',
    { sessionRef: 'mac:wtlist', kind: { type: 'failed', reason: 'exit 1' }, observedAtMs: SHOWCASE_NOW },
  ],
]);

/** A legacy goal on a session row (D14): its why line and the row menu. */
const showcaseGoals: ReadonlyMap<string, LegacyGoal> = new Map([
  ['mac:wtlist', { laneKey: 'lane-wtlist', text: 'Keep the worktree list fresh after git operations', paused: false }],
]);

/** One owner prompt, one Reply and one long worker report (opened in the Viewer). */
const showcaseTranscript: { messages: readonly TranscriptMessage[]; exportFile: () => Promise<{ ok: true; summary: string }> } = {
  messages: [
    { id: 1, role: 'owner', text: 'retry every 5 min', evidence: [], requestId: 'req-1', createdAtMs: SHOWCASE_NOW - 6 * 60_000 },
    { id: 2, role: 'supervisor', text: 'Upstream errors now retry every 5 min.', evidence: [{ kind: 'rule', id: 'rule-5' }], requestId: 'req-1', createdAtMs: SHOWCASE_NOW - 6 * 60_000 },
    {
      id: 3,
      role: 'supervisor',
      text: '# Relay reconnect\nThe reconnect tests pass on this Mac.\nThe migration adds one nullable column.\nNothing else changed.\nThe Windows run is still queued.',
      evidence: [],
      requestId: 'report:wake-41',
      createdAtMs: SHOWCASE_NOW - 2 * 60_000,
    },
  ],
  exportFile: () => Promise.resolve({ ok: true, summary: 'Saved 3 rows.' } as const),
};

/** A claim without a delivery event reads "Unverified" in the log. */
const unverifiedSnapshot: PanelSnapshot = {
  ...panelSnapshotFixture,
  decisions: [],
  log: [
    {
      id: 21,
      incidentKey: 'answer:wintests:retry',
      sessionRef: 'win:wintests',
      kind: 'answered',
      summary: 'Answered Windows installer tests: Retry',
      basis: 'You picked Retry the last 3 times',
      evidence: [],
      undo: { kind: 'reopenPrompt', payload: { promptId: 'prompt-win-1' } },
      live: false,
      createdAtMs: SHOWCASE_NOW - 3 * 60_000,
      updatedAtMs: SHOWCASE_NOW - 3 * 60_000,
    },
    ...panelSnapshotFixture.log,
  ],
};

/** Another computer holds the supervisor lease: this one is on standby. */
const standbyHub: SupervisorHubState = {
  available: true,
  running: true,
  role: 'standby',
  holderLabel: 'omarchy',
  holderPaired: true,
  unmappedHosts: ['old-laptop'],
  adoptable: [{ goalId: 'goal-7', title: 'Finish the release notes', supervisorId: 'sup-old', fromMachine: 'This Mac' }],
  observeDropped: 2,
};

/** The holder's own lease warnings. */
const holderHub: SupervisorHubState = {
  available: true,
  running: true,
  role: 'holder',
  unreachableHosts: ['maho-win'],
  machinesWithoutLease: ['old-laptop'],
};

/** Todo 10: one PR per lane, so the order and the check states are visible in the showcase. */
const showcasePr = (overrides: Partial<PullRequest>): PullRequest => ({
  url: 'https://github.com/o/r/pull/1',
  title: 'Relay reconnect backoff',
  number: 1,
  repo: 'ferryx/ferryx',
  headRefName: 'fix/relay-backoff',
  isDraft: false,
  reviewDecision: null,
  checks: 'success',
  reviewRequested: false,
  updatedAt: '2026-11-12T00:00:00Z',
  lane: 'waits_on_owner',
  ...overrides,
});

const showcasePrLane: PrLane = {
  prs: [
    showcasePr({ number: 412, title: 'Relay reconnect backoff', checks: 'success', reviewRequested: true }),
    showcasePr({ number: 408, title: 'Daemon handover keeps the lease', lane: 'agent_working', headRefName: 'feat/handover', checks: 'pending', url: 'u408' }),
    showcasePr({ number: 401, title: 'Windows path quoting', lane: 'failing', headRefName: 'fix/win-quote', checks: 'failure', url: 'u401' }),
    showcasePr({ number: 399, title: 'Docs: transport notes', lane: 'pending', headRefName: 'docs/transport', checks: 'none', isDraft: true, url: 'u399' }),
  ],
  outage: null,
  refreshedAtMs: SHOWCASE_NOW,
};

/** Only #412 has a worktree; the rest show their refusal when handed off. */
const showcaseWorktrees: readonly WorktreeOnBranch[] = [
  { path: '/Users/me/ferryx-wt/relay-backoff', branch: 'fix/relay-backoff' },
];

function Block({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex min-w-0 flex-col gap-3 rounded-xl border border-border p-4">
      <h2 className="text-xs font-medium uppercase text-muted-foreground">{title}</h2>
      {children}
    </section>
  );
}

function Surface({
  touch,
  snapshot,
  health,
  goals,
  transcript,
  prLane,
  worktrees,
}: {
  touch: boolean;
  snapshot: PanelSnapshot;
  health?: ReadonlyMap<string, SessionHealth>;
  goals?: ReadonlyMap<string, LegacyGoal>;
  transcript?: typeof showcaseTranscript;
  prLane?: PrLane | null;
  worktrees?: readonly WorktreeOnBranch[];
}) {
  return (
    <PanelSurface
      touch={touch}
      snapshot={snapshot}
      actions={inertActions}
      nowMs={SHOWCASE_NOW}
      health={health}
      goals={goals}
      transcript={transcript}
      prLane={prLane}
      worktrees={worktrees}
      computerLabel="This Mac"
    />
  );
}

const moveOk = () => Promise.resolve({ kind: 'ok', moved: true } as const);
const adoptOk = () => Promise.resolve({ kind: 'ok' } as const);

/**
 * Every supervisor panel component and state, rendered from the shared fixtures. `touch` forces
 * the phone scale on every block (the 390 px capture); without it the desktop and phone surfaces
 * sit side by side.
 */
export function SupervisorPanelShowcase({ touch }: { touch?: boolean } = {}) {
  const compact = touch ?? false;
  return (
    <div className="flex min-h-full flex-col gap-6 bg-background p-4 text-foreground sm:p-6" data-testid="supervisor-panel-showcase">
      <Block title="Status glyphs">
        <ul className="flex flex-wrap gap-4 text-sm">
          {(Object.keys(GLYPH_LABEL) as GlyphState[]).map((state) => (
            <li key={state} className="flex items-center gap-2">
              <StatusGlyph state={state} />
              {GLYPH_LABEL[state]}
            </li>
          ))}
        </ul>
      </Block>
      <div className="grid gap-6 lg:grid-cols-[380px_390px_minmax(0,1fr)]">
        {touch ? null : (
          <Block title="Desktop 380">
            <Surface touch={false} snapshot={panelSnapshotFixture} goals={showcaseGoals} transcript={showcaseTranscript} prLane={showcasePrLane} worktrees={showcaseWorktrees} />
          </Block>
        )}
        <Block title="Phone 390">
          <Surface touch snapshot={panelSnapshotFixture} goals={showcaseGoals} transcript={showcaseTranscript} prLane={showcasePrLane} worktrees={showcaseWorktrees} />
        </Block>
        <div className="flex min-w-0 flex-col gap-6">
          <Block title="A failed session">
            <Surface touch={compact} snapshot={{ ...panelSnapshotFixture, decisions: [] }} health={failedHealth} />
          </Block>
          <Block title="Nothing needs you, paused">
            <Surface touch={compact} snapshot={{ ...panelSnapshotFixture, decisions: [], paused: true }} />
          </Block>
          <Block title="An unverified claim in the log">
            <Surface touch={compact} snapshot={unverifiedSnapshot} />
          </Block>
          <Block title="Setup required">
            <Surface touch={compact} snapshot={{ ...panelSnapshotFixture, setupRequired: true }} />
          </Block>
          <Block title="Standby: the supervisor runs on another computer">
            <StandbyCard hub={standbyHub} here="This computer" moveHere={moveOk} adoptArchived={adoptOk} touch={compact} />
          </Block>
          <Block title="Lease warnings on the holder">
            <LeaseNotices hub={holderHub} here="This computer" touch={compact} />
          </Block>
          <Block title="Decision without a command (phone)">
            <DecisionCard
              touch
              sessionName="Roblox build"
              computer="maho-win"
              question="Still 3 failing tests after 2 fix attempts. What next?"
              command={null}
              options={['Revert the physics change', 'Keep trying', 'Stop']}
              recommendation={{ index: 1, reason: 'Recommended option need not be first in the list; it renders first.' }}
              onAnswer={noop}
              onOpenTerminal={noop}
            />
          </Block>
        </div>
      </div>
    </div>
  );
}
