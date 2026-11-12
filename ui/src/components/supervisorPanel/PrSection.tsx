// The panel's Pull requests section (todo 10): who waits on the owner first, each row's check
// state, and one action that hands a PR to an agent in the worktree already on its branch. The
// lane order is the hub's (`prLane.ts`); this component only draws rows and reports refusals.
import { useState } from 'react';

import { cn } from '../../lib/cn';
import type { PanelActions } from '../../lib/supervisorPanel/actions';
import {
  handOffPrompt,
  noWorktreeMessage,
  type PrLaneView,
  type PullRequestRowView,
} from '../../lib/supervisorPanel/prLane';
import type { TouchProps } from './types';

export interface PrSectionProps extends TouchProps {
  view: PrLaneView;
  actions: PanelActions;
}

function CheckBadge({ row, touch }: { row: PullRequestRowView; touch?: boolean }) {
  const tone =
    row.checks === 'failure'
      ? 'text-status-warning'
      : row.checks === 'success'
        ? 'text-muted-foreground'
        : 'text-muted-foreground';
  return (
    <span data-testid={`pr-checks-${row.number}`} className={cn('shrink-0 tabular-nums', tone, touch ? 'text-sm' : 'text-xs')}>
      {row.checksLabel}
    </span>
  );
}

function PrRow({ row, actions, touch }: { row: PullRequestRowView; actions: PanelActions } & TouchProps) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const handOff = actions.handOffPr;

  const hand = async () => {
    if (pending || !handOff) return;
    // No worktree on the head branch: say so and spawn nothing. Never create one here.
    if (!row.worktreePath) {
      setError(noWorktreeMessage({ headRefName: row.headRefName, number: row.number }));
      return;
    }
    setPending(true);
    setError(null);
    const result = await handOff({
      number: row.number,
      headRefName: row.headRefName,
      worktreePath: row.worktreePath,
      prompt: handOffPrompt({ number: row.number }),
    });
    setPending(false);
    if (!result.ok) setError(result.message);
  };

  return (
    <li data-testid={`pr-row-${row.number}`} className="flex flex-col gap-1 rounded-lg px-2 py-1.5">
      <div className="flex min-w-0 items-start gap-2.5">
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium text-foreground">{row.title}</span>
          <span className="block truncate text-xs text-muted-foreground">
            {row.label} · {row.laneLabel}
            {row.draft ? ' · Draft' : ''}
            {row.reviewRequested ? ' · Review asked' : ''}
          </span>
        </span>
        <CheckBadge row={row} touch={touch} />
        {handOff ? (
          <button
            type="button"
            disabled={pending}
            onClick={() => void hand()}
            className={cn(
              'shrink-0 rounded-md border border-border px-2 text-foreground transition-colors hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50',
              touch ? 'min-h-11 text-sm' : 'min-h-8 text-xs',
            )}
          >
            Hand to agent
          </button>
        ) : null}
      </div>
      {error ? (
        <p role="alert" data-testid={`pr-error-${row.number}`} className={cn('text-status-warning', touch ? 'text-sm' : 'text-xs')}>
          {error}
        </p>
      ) : null}
    </li>
  );
}

/**
 * The lane, or the reason it is empty. An outage is never shown as "no pull requests wait on
 * you": `gh` failing and nothing waiting look the same only if the reason is dropped.
 */
export function PrSection({ view, actions, touch }: PrSectionProps) {
  if (view.rows.length === 0 && !view.outageLabel) return null;
  return (
    <section aria-label="Pull requests" className="flex flex-col gap-2" data-testid="supervisor-prs">
      <h2 className={cn('flex items-baseline gap-1.5 font-medium text-muted-foreground', touch ? 'text-sm' : 'text-xs')}>
        <span className="flex-1">Pull requests</span>
        <span className="tabular-nums">{view.rows.length}</span>
      </h2>
      {view.outageLabel ? (
        <p data-testid="supervisor-prs-outage" className={cn('text-muted-foreground', touch ? 'text-sm' : 'text-xs')}>
          {view.outageLabel}
        </p>
      ) : null}
      {view.rows.length > 0 ? (
        <ul className="flex flex-col gap-1">
          {view.rows.map((row) => (
            <PrRow key={row.url} row={row} actions={actions} touch={touch} />
          ))}
        </ul>
      ) : null}
    </section>
  );
}
