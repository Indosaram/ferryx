import { type ReactNode } from 'react';

import type { PanelActions } from '../../lib/supervisorPanel/actions';
import { panelSnapshotFixture } from '../../lib/supervisorPanel/fixtures';
import type { PanelSnapshot, SessionHealth } from '../../lib/supervisorPanel/contract';
import { DecisionCard } from './DecisionCard';
import { PanelSurface } from './PanelSurface';
import { StatusGlyph } from './StatusGlyph';
import { GLYPH_LABEL, type GlyphState } from './types';

// Showcase only: every write is inert. The mapping from PanelSnapshot is the shared model's.
const noop = () => undefined;
const ok = () => Promise.resolve({ ok: true } as const);
const inertActions: PanelActions = {
  answer: ok,
  setPaused: ok,
  converse: ok,
  stop: ok,
  undo: ok,
  turnOn: ok,
  openSession: noop,
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

function Block({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 rounded-xl border border-border p-4">
      <h2 className="text-xs font-medium uppercase text-muted-foreground">{title}</h2>
      {children}
    </section>
  );
}

function Surface({
  touch,
  snapshot,
  health,
}: {
  touch: boolean;
  snapshot: PanelSnapshot;
  health?: ReadonlyMap<string, SessionHealth>;
}) {
  return (
    <PanelSurface
      touch={touch}
      snapshot={snapshot}
      actions={inertActions}
      nowMs={SHOWCASE_NOW}
      health={health}
      computerLabel="This Mac"
    />
  );
}

/** Every supervisor panel component and state, rendered from the shared fixtures. */
export function SupervisorPanelShowcase() {
  return (
    <div className="flex min-h-full flex-col gap-6 bg-background p-6 text-foreground" data-testid="supervisor-panel-showcase">
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
      <div className="grid gap-6 lg:grid-cols-[380px_390px_1fr]">
        <Block title="Desktop 380">
          <Surface touch={false} snapshot={panelSnapshotFixture} />
        </Block>
        <Block title="Phone 390">
          <Surface touch snapshot={panelSnapshotFixture} />
        </Block>
        <div className="flex flex-col gap-6">
          <Block title="A failed session">
            <Surface touch={false} snapshot={{ ...panelSnapshotFixture, decisions: [] }} health={failedHealth} />
          </Block>
          <Block title="Nothing needs you, paused">
            <Surface touch={false} snapshot={{ ...panelSnapshotFixture, decisions: [], paused: true }} />
          </Block>
          <Block title="Setup required">
            <Surface touch={false} snapshot={{ ...panelSnapshotFixture, setupRequired: true }} />
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
