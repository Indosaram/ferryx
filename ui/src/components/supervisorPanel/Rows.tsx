import { cn } from '../../lib/cn';
import { StatusGlyph } from './StatusGlyph';
import type { HandlingRowProps, SessionRowProps } from './types';

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

export function SessionRow({ title, detail, computer, state, selected, touch, onOpen }: SessionRowProps) {
  return (
    <button
      type="button"
      onClick={onOpen}
      data-testid="session-row"
      data-state={state}
      aria-current={selected ? 'true' : undefined}
      className={cn(ROW_BASE, 'w-full', touch && 'min-h-11 py-2 text-sm', selected && 'bg-worktree-sidebar-accent')}
    >
      <StatusGlyph state={state} className="mt-0.5" />
      <RowText title={title} detail={detail} />
      <span className="mt-0.5 shrink-0 text-xs text-muted-foreground">{computer}</span>
    </button>
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
    </div>
  );
}
