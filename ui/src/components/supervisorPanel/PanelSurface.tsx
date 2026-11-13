import { useMemo, useState } from 'react';

import { cn } from '../../lib/cn';
import {
  STALE_DECISION_MESSAGE,
  type PanelActionResult,
  type PanelActions,
} from '../../lib/supervisorPanel/actions';
import type { PanelSnapshot, SessionHealth, SessionRef } from '../../lib/supervisorPanel/contract';
import { buildPrLaneView, type WorktreeOnBranch } from '../../lib/supervisorPanel/prLane';
import type { PrLane } from '../../lib/supervisorPanel/contract';
import type { LegacyGoal } from '../../lib/supervisorPanel/legacyGoals';
import {
  buildPanelView,
  type ActionView,
  type DecisionView,
  type SessionGlyph,
} from '../../lib/supervisorPanel/model';
import { ActionLog } from './ActionLog';
import { Composer } from './Composer';
import type { VoiceConversationProps } from '../supervisor/VoiceConversationButton';
import { DecisionCard } from './DecisionCard';
import { RulesLine } from './RulesLine';
import { HandlingRow, SessionRow } from './Rows';
import { PrSection } from './PrSection';
import { SetupCard } from './SetupCard';
import { StatusLine } from './StatusLine';
import { TranscriptSection } from './TranscriptSection';
import type { TranscriptSource } from '../../lib/supervisorPanel/useTranscript';
import { toComposerResult } from './composerResult';
import type { ActionLogLine, GlyphState, MentionOption } from './types';

export type PanelSurfaceProps = {
  snapshot: PanelSnapshot;
  actions: PanelActions;
  /** True on the phone: 44px targets and rem text. */
  touch?: boolean;
  nowMs: number;
  /** Latest health per session; a fresh `failed` report gives that row the Failed glyph. */
  health?: ReadonlyMap<SessionRef, SessionHealth>;
  /** Legacy supervisor goals by session (D14): shown on the row, paused from its menu. */
  goals?: ReadonlyMap<SessionRef, LegacyGoal>;
  /** What setup names: "this computer" unless the host's label is known. */
  computerLabel?: string;
  /**
   * Omit the composer (the phone pins its own below the scroller). The rules line's "Change"
   * then hands its starter text to `onComposerText` instead of seeding an inline composer.
   */
  hideComposer?: boolean;
  onComposerText?: (text: string) => void;
  /** Free-form voice for the supervisor conversation; absent hides the talk button. */
  voiceConversation?: VoiceConversationProps;
  /** The stored owner/supervisor conversation; omitted, the panel shows no conversation. */
  transcript?: Pick<TranscriptSource, 'messages' | 'exportFile'>;
  /** The hub's pull-request lane; omitted, the panel shows no Pull requests section. */
  prLane?: PrLane | null;
  /**
   * The worktrees the panel can hand a PR to, already read by the host. A PR whose head branch
   * no worktree holds shows its refusal instead of spawning into a checkout that lacks it.
   */
  worktrees?: readonly WorktreeOnBranch[];
};

const SESSION_GLYPH: Record<SessionGlyph, GlyphState> = {
  needsYou: 'needs',
  failed: 'failed',
  unread: 'done',
  working: 'working',
  idle: 'idle',
};

/** Local wall-clock time, never UTC. */
export function localTimeLabel(ms: number): string {
  const date = new Date(ms);
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

function logLine(entry: ActionView): ActionLogLine {
  return {
    id: entry.id,
    summary: entry.summary,
    timeLabel: localTimeLabel(entry.createdAtMs),
    basis: entry.basis,
    unverified: entry.unverified,
    undoable: entry.undoable,
  };
}

/** "2 decisions for you. The rest is running." -> headline + detail. */
function splitStatus(statusLine: string): { headline: string; detail?: string } {
  const split = statusLine.indexOf('. ');
  return split < 0 ? { headline: statusLine } : { headline: statusLine.slice(0, split + 1), detail: statusLine.slice(split + 2) };
}

/** A visible section title, so each list says what it is (the region keeps its short name). */
function SectionHeading({ title, count, touch }: { title: string; count: number; touch?: boolean }) {
  return (
    <h2 className={cn('flex items-baseline gap-1.5 font-medium text-muted-foreground', touch ? 'text-sm' : 'text-xs')}>
      <span className="flex-1">{title}</span>
      <span className="tabular-nums">{count}</span>
    </h2>
  );
}

function Decision({
  decision,
  actions,
  touch,
}: {
  decision: DecisionView;
  actions: PanelActions;
  touch?: boolean;
}) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const options = useMemo(() => {
    const byIndex = [...decision.options].sort((a, b) => a.index - b.index);
    return byIndex.map((option) => option.label);
  }, [decision.options]);
  const answer = async (optionIndex: number) => {
    if (pending) return;
    setPending(true);
    setError(null);
    const result = await actions.answer(
      { sessionRef: decision.sessionRef, promptId: decision.promptId, screenRevision: decision.screenRevision },
      optionIndex,
    );
    setPending(false);
    if (!result.ok) setError(result.stale ? STALE_DECISION_MESSAGE : result.message);
  };
  const recommended = decision.options.find((option) => option.recommended);
  return (
    <DecisionCard
      touch={touch}
      sessionName={decision.sessionTitle}
      computer={decision.hostLabel ?? ''}
      question={decision.question}
      command={decision.command}
      options={options}
      recommendation={recommended ? { index: recommended.index, reason: decision.reason } : null}
      onAnswer={(index) => void answer(index)}
      onOpenTerminal={() => actions.openSession(decision.sessionRef)}
      pending={pending}
      error={error}
    />
  );
}

function SetupSection({ actions, touch, computerLabel }: Pick<PanelSurfaceProps, 'actions' | 'touch' | 'computerLabel'>) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const turnOn = async () => {
    if (pending) return;
    setPending(true);
    setError(null);
    const result = await actions.turnOn();
    setPending(false);
    if (!result.ok) setError(result.message);
  };
  return (
    <SetupCard
      touch={touch}
      computer={computerLabel ?? 'this computer'}
      onTurnOn={() => void turnOn()}
      pending={pending}
      error={error}
    />
  );
}

/**
 * The supervisor panel, composed once for both surfaces: the desktop sidebar renders it as is
 * and the phone renders it with `touch`. All content comes from `buildPanelView`; all writes go
 * through `actions`, and a refused write shows its message where the user acted.
 */
export function PanelSurface({ snapshot, actions, touch, nowMs, health, goals, computerLabel, hideComposer, onComposerText, voiceConversation, transcript, prLane, worktrees }: PanelSurfaceProps) {
  const view = useMemo(() => buildPanelView(snapshot, { nowMs, health }), [snapshot, nowMs, health]);
  const prView = useMemo(
    () => (prLane ? buildPrLaneView(prLane, worktrees ?? []) : null),
    [prLane, worktrees],
  );
  const [actionError, setActionError] = useState<string | null>(null);
  const [pausing, setPausing] = useState(false);
  const [pausingLanes, setPausingLanes] = useState<ReadonlySet<string>>(() => new Set());
  const [seed, setSeed] = useState<{ text: string; nonce: number } | null>(null);
  const titleOf = useMemo(() => new Map(snapshot.sessions.map((s) => [s.sessionRef, s])), [snapshot.sessions]);
  const mentions: MentionOption[] = useMemo(
    () => snapshot.sessions.map((s) => ({ sessionRef: s.sessionRef, title: s.title, computer: s.hostLabel })),
    [snapshot.sessions],
  );

  if (view.setupRequired) {
    return <SetupSection actions={actions} touch={touch} computerLabel={computerLabel} />;
  }

  const report = async (call: Promise<PanelActionResult>) => {
    const result = await call;
    setActionError(result.ok ? null : result.message);
  };
  const togglePause = async () => {
    if (pausing) return;
    setPausing(true);
    await report(actions.setPaused(!view.paused));
    setPausing(false);
  };
  const { headline, detail } = splitStatus(view.statusLine);
  const toggleLane = async (goal: LegacyGoal) => {
    if (pausingLanes.has(goal.laneKey)) return;
    setPausingLanes((current) => new Set(current).add(goal.laneKey));
    await report(actions.setLanePaused(goal.laneKey, !goal.paused));
    setPausingLanes((current) => {
      const next = new Set(current);
      next.delete(goal.laneKey);
      return next;
    });
  };
  // A waiting or failed session is the owner's work: it sits with the decisions under "Needs you",
  // not among the running sessions where the headline's count could not be traced to a row.
  const waitingRows = view.sessions.filter((row) => row.glyph === 'needsYou' || row.glyph === 'failed');
  const otherRows = view.sessions.filter((row) => row.glyph !== 'needsYou' && row.glyph !== 'failed');
  const needsYouCount = view.decisions.length + waitingRows.length;
  const sessionRow = (row: (typeof view.sessions)[number]) => {
    const goal = goals?.get(row.sessionRef);
    return (
      <SessionRow
        key={row.sessionRef}
        touch={touch}
        title={row.title}
        // The reason is the row's second line on desktop and phone alike; the wait age follows it.
        detail={row.reason ? (row.age ? `${row.reason} · ${row.age}` : row.reason) : (row.age ?? row.why)}
        computer={row.hostLabel}
        state={SESSION_GLYPH[row.glyph]}
        onOpen={() => actions.openSession(row.sessionRef)}
        goal={
          goal
            ? {
                text: goal.text,
                paused: goal.paused,
                pending: pausingLanes.has(goal.laneKey),
                onTogglePaused: () => void toggleLane(goal),
              }
            : undefined
        }
      />
    );
  };

  return (
    <div data-testid="supervisor-panel-surface" className="flex flex-col gap-4">
      <StatusLine
        touch={touch}
        summary={view.paused ? 'Paused · nothing is answered or retried' : view.headerLine}
        headline={headline}
        detail={detail}
        // A blocked row the hub has not escalated still needs the owner: never an all-clear.
        clear={view.decisions.length === 0 && view.waitingCount === 0}
        paused={view.paused}
        onTogglePause={() => void togglePause()}
      />
      {actionError ? (
        <p role="alert" data-testid="supervisor-panel-error" className={cn('text-status-warning', touch ? 'text-sm' : 'text-xs')}>
          {actionError}
        </p>
      ) : null}
      {needsYouCount > 0 ? (
        <section aria-label="Needs you" className="flex flex-col gap-3">
          <SectionHeading title="Needs you" count={needsYouCount} touch={touch} />
          {view.decisions.map((decision) => (
            <Decision key={decision.sessionRef} decision={decision} actions={actions} touch={touch} />
          ))}
          {waitingRows.length > 0 ? <div className="flex flex-col gap-1">{waitingRows.map(sessionRow)}</div> : null}
        </section>
      ) : null}
      {view.handling.length > 0 ? (
        <section aria-label="Handling" className="flex flex-col gap-1">
          <SectionHeading title="The supervisor is on it" count={view.handling.length} touch={touch} />
          {view.handling.map((entry) => {
            const ref = entry.sessionRef;
            const session = ref ? titleOf.get(ref) : undefined;
            return (
              <HandlingRow
                key={entry.id}
                touch={touch}
                title={session?.title ?? entry.summary}
                detail={entry.summary}
                computer={session?.hostLabel}
                onOpen={() => {
                  if (ref) actions.openSession(ref);
                }}
                onStop={ref ? () => void report(actions.stop(ref)) : undefined}
              />
            );
          })}
        </section>
      ) : null}
      {otherRows.length > 0 ? (
        <section aria-label="Sessions" className="flex flex-col gap-1">
          <SectionHeading title="Sessions" count={otherRows.length} touch={touch} />
          {otherRows.map(sessionRow)}
        </section>
      ) : null}
      {prView ? <PrSection view={prView} actions={actions} touch={touch} /> : null}
      {view.log.total > 0 ? (
        <ActionLog
          touch={touch}
          entries={view.log.all.map(logLine)}
          onUndo={(id) => void report(actions.undo(id))}
        />
      ) : null}
      {transcript && transcript.messages.length > 0 ? (
        <TranscriptSection touch={touch} messages={transcript.messages} onExport={transcript.exportFile} />
      ) : null}
      <RulesLine
        touch={touch}
        text={view.rulesLine}
        onChange={() => {
          const text = 'Change the rules: ';
          if (hideComposer) onComposerText?.(text);
          else setSeed({ text, nonce: (seed?.nonce ?? 0) + 1 });
        }}
      />
      {hideComposer ? null : (
        <Composer
          touch={touch}
          sessions={mentions}
          seed={seed}
          voiceConversation={voiceConversation}
          onSend={async (text, refs) => toComposerResult(await actions.converse(text, refs))}
        />
      )}
    </div>
  );
}
