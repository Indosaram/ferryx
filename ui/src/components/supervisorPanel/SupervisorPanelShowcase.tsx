import { useState, type ReactNode } from 'react';

import { panelSnapshotFixture } from '../../lib/supervisorPanel/fixtures';
import type { ActionEntry, FleetSession, PanelSnapshot } from '../../lib/supervisorPanel/contract';
import { ActionLog } from './ActionLog';
import { Composer } from './Composer';
import { DecisionCard } from './DecisionCard';
import { RulesLine } from './RulesLine';
import { SetupCard } from './SetupCard';
import { StatusGlyph } from './StatusGlyph';
import { StatusLine } from './StatusLine';
import { HandlingRow, SessionRow } from './Rows';
import { GLYPH_LABEL, type ActionLogLine, type GlyphState, type MentionOption } from './types';

// Showcase only: the real mapping from PanelSnapshot lives in the reducer (todo 10).
const noop = () => undefined;

function glyphFor(session: FleetSession): GlyphState {
  if (session.state === 'blocked') return 'needs';
  if (session.state === 'working') return 'working';
  if (session.state === 'idle') return 'done';
  return 'idle';
}

function timeLabel(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}`;
}

function logLine(entry: ActionEntry): ActionLogLine {
  return {
    id: entry.id,
    summary: entry.summary,
    timeLabel: timeLabel(entry.createdAtMs),
    basis: entry.basis,
    unverified: entry.evidence.length === 0,
    undoable: entry.undo !== null,
  };
}

function Block({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 rounded-xl border border-border p-4">
      <h2 className="text-xs font-medium uppercase text-muted-foreground">{title}</h2>
      {children}
    </section>
  );
}

function Surface({ touch, snapshot }: { touch: boolean; snapshot: PanelSnapshot }) {
  const byRef = new Map(snapshot.sessions.map((s) => [s.sessionRef, s]));
  const mentions: MentionOption[] = snapshot.sessions.map((s) => ({
    sessionRef: s.sessionRef,
    title: s.title,
    computer: s.hostLabel,
  }));
  const handlingRefs = new Set(snapshot.handling.map((h) => h.sessionRef));
  const decisionRefs = new Set(snapshot.decisions.map((d) => d.sessionRef));
  const [paused, setPaused] = useState(snapshot.paused);
  const log = snapshot.log.map(logLine);
  // Demonstrate the Unverified chip on one line.
  if (log[1]) log[1] = { ...log[1], unverified: true };

  if (snapshot.setupRequired) {
    return <SetupCard computer="This Mac" touch={touch} onTurnOn={noop} />;
  }
  return (
    <div className="flex flex-col gap-4">
      <StatusLine
        touch={touch}
        summary={paused ? 'Paused · nothing is answered or retried' : '2 running · handled 4 things today'}
        headline={snapshot.decisions.length > 0 ? `${snapshot.decisions.length} decisions for you.` : 'Nothing needs you.'}
        detail={snapshot.decisions.length > 0 ? 'The rest is running.' : undefined}
        clear={snapshot.decisions.length === 0}
        paused={paused}
        onTogglePause={() => setPaused((p) => !p)}
      />
      {snapshot.decisions.map((d) => {
        const s = byRef.get(d.sessionRef);
        return (
          <DecisionCard
            key={d.sessionRef}
            touch={touch}
            sessionName={s?.title ?? d.sessionRef}
            computer={s?.hostLabel ?? ''}
            question={d.prompt.question}
            command={d.prompt.command}
            options={d.prompt.options}
            recommendation={{ index: d.recommendedIndex, reason: d.reason }}
            onAnswer={noop}
            onOpenTerminal={noop}
          />
        );
      })}
      {snapshot.handling.map((h) => (
        <HandlingRow key={h.id} touch={touch} title={byRef.get(h.sessionRef ?? '')?.title ?? h.summary} detail={h.summary} onOpen={noop} onStop={noop} />
      ))}
      <div className="flex flex-col gap-1">
        {snapshot.sessions
          .filter((s) => !handlingRefs.has(s.sessionRef) && !decisionRefs.has(s.sessionRef))
          .map((s) => (
            <SessionRow
              key={s.sessionRef}
              touch={touch}
              title={s.title}
              detail={s.lastAssistantLine ?? ''}
              computer={s.hostLabel}
              state={glyphFor(s)}
              onOpen={noop}
            />
          ))}
      </div>
      <ActionLog entries={log} touch={touch} onUndo={noop} />
      <RulesLine
        text="Retry upstream errors every 10 min · restart after 5 min with no output · resume after usage limits · ask you only before irreversible steps"
        onChange={noop}
      />
      <Composer sessions={mentions} touch={touch} onSend={noop} />
    </div>
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
