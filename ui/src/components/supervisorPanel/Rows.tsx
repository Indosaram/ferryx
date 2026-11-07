import { useEffect, useRef, useState } from 'react';

import { cn } from '../../lib/cn';
import { StatusGlyph } from './StatusGlyph';
import type { HandlingRowProps, SessionGoalProps, SessionRowProps } from './types';

const ROW_BASE =
  'flex min-w-0 flex-1 items-start gap-2.5 rounded-lg px-2 py-1.5 text-left text-[13px] transition-colors hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring';

function RowText({ title, detail }: { title: string; detail: string }) {
  return (
    <span className="min-w-0 flex-1">
      <span className="block truncate font-medium text-foreground">{title}</span>
      <span className="block truncate text-xs text-muted-foreground">{detail}</span>
    </span>
  );
}

/**
 * The row menu. It is rendered in place (no portal) so no overlay sits above the native
 * terminal surfaces. It carries the session's own actions, independent of any legacy goal
 * (D10 Stop, D13 per-session mute), plus pause/resume when a legacy goal exists.
 */
function SessionMenu({
  title, goal, touch, onStop, mute,
}: {
  title: string;
  goal?: SessionGoalProps;
  touch?: boolean;
  onStop?: () => void;
  mute?: { muted: boolean; onToggleMuted: () => void };
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: Event) => {
      if (event instanceof KeyboardEvent ? event.key === 'Escape' : !ref.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('pointerdown', close);
    document.addEventListener('keydown', close);
    return () => {
      document.removeEventListener('pointerdown', close);
      document.removeEventListener('keydown', close);
    };
  }, [open]);
  const item = cn(
    'flex w-full items-center rounded-sm px-2 text-left text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50',
    touch ? 'min-h-11 text-sm' : 'min-h-8 text-xs',
  );
  return (
    <div ref={ref} className="relative shrink-0">
      <button
        type="button"
        aria-label={`Actions for ${title}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={cn(
          'grid place-items-center rounded-md px-2 text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
          touch ? 'min-h-11 min-w-11 text-base' : 'min-h-8 min-w-8 text-sm',
        )}
      >
        <span aria-hidden="true">⋯</span>
      </button>
      {open ? (
        <div
          role="menu"
          data-testid="session-row-menu"
          className="absolute right-0 top-full z-10 mt-1 flex w-44 flex-col rounded-md border border-border bg-popover p-1 shadow-md"
        >
          {onStop ? (
            <button
              type="button"
              role="menuitem"
              data-testid="session-row-stop"
              onClick={() => {
                setOpen(false);
                onStop();
              }}
              className={item}
            >
              Stop
            </button>
          ) : null}
          {mute ? (
            <button
              type="button"
              role="menuitem"
              data-testid="session-row-mute"
              aria-pressed={mute.muted}
              onClick={() => {
                setOpen(false);
                mute.onToggleMuted();
              }}
              className={item}
            >
              {mute.muted ? 'Unmute phone alerts' : 'Mute phone alerts'}
            </button>
          ) : null}
          {goal ? (
            <button
              type="button"
              role="menuitem"
              disabled={goal.pending}
              onClick={() => {
                setOpen(false);
                goal.onTogglePaused();
              }}
              className={item}
            >
              {goal.paused ? 'Resume supervisor' : 'Pause supervisor'}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

export function SessionRow({ title, detail, computer, state, selected, touch, onOpen, goal, onStop, mute }: SessionRowProps) {
  const button = (
    <button
      type="button"
      onClick={onOpen}
      data-testid="session-row"
      data-state={state}
      aria-current={selected ? 'true' : undefined}
      className={cn(ROW_BASE, 'w-full', touch && 'min-h-11 py-2 text-sm', selected && 'bg-worktree-sidebar-accent')}
    >
      <StatusGlyph state={state} className="mt-0.5" />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-medium text-foreground">{title}</span>
        <span className="block truncate text-xs text-muted-foreground">{detail}</span>
        {goal ? (
          <span data-testid="session-goal" data-paused={goal.paused ? 'true' : 'false'} className="block truncate text-xs text-muted-foreground">
            {goal.paused ? 'Supervisor paused: ' : 'Supervisor: '}
            {goal.text}
          </span>
        ) : null}
      </span>
      <span className="mt-0.5 shrink-0 text-xs text-muted-foreground">{computer}</span>
    </button>
  );
  return (
    <div className="flex items-start gap-1">
      {button}
      <SessionMenu title={title} goal={goal} touch={touch} onStop={onStop} mute={mute} />
    </div>
  );
}

export function HandlingRow({ title, detail, computer, touch, onOpen, onStop }: HandlingRowProps) {
  return (
    <div className="flex items-start gap-1" data-testid="handling-row">
      <button type="button" onClick={onOpen} className={cn(ROW_BASE, touch && 'min-h-11 py-2 text-sm')}>
        <StatusGlyph state="working" className="mt-0.5" />
        <RowText title={title} detail={detail} />
        {computer ? <span className="mt-0.5 shrink-0 text-xs text-muted-foreground">{computer}</span> : null}
      </button>
      {onStop ? (
        <button
          type="button"
          onClick={onStop}
          aria-label={`Stop ${title}`}
          className={cn(
            'shrink-0 rounded-md px-2 text-xs text-muted-foreground underline underline-offset-2 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
            touch ? 'min-h-11 min-w-11 text-sm' : 'min-h-8',
          )}
        >
          Stop
        </button>
      ) : null}
    </div>
  );
}
