import { cn } from '../../lib/cn';
import { StatusGlyph } from './StatusGlyph';
import type { StatusLineProps } from './types';

export function SupervisorMark({ className }: { className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        'grid size-5 shrink-0 place-items-center rounded-md bg-primary text-xs font-semibold text-primary-foreground',
        className,
      )}
    >
      S
    </span>
  );
}

export function StatusLine({ summary, headline, detail, clear, paused, onTogglePause, touch }: StatusLineProps) {
  return (
    <div className="flex flex-col gap-3" data-testid="supervisor-status-line">
      <div className="flex items-center gap-2.5">
        <SupervisorMark className="size-7 rounded-lg text-sm" />
        <div className="min-w-0 flex-1">
          <p className="text-sm font-semibold text-foreground">Supervisor</p>
          <p className="truncate text-xs text-muted-foreground">{summary}</p>
        </div>
        <button
          type="button"
          onClick={onTogglePause}
          className={cn(
            'shrink-0 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
            touch ? 'min-h-11 min-w-11 px-3 text-sm' : 'min-h-8',
          )}
        >
          {paused ? 'Resume' : 'Pause'}
        </button>
      </div>
      <p className={cn('flex items-center gap-2 text-muted-foreground', touch ? 'text-sm' : 'text-[13px]')}>
        {clear && <StatusGlyph state="done" />}
        <span>
          <span className="font-semibold text-foreground">{headline}</span>
          {detail ? ` ${detail}` : null}
        </span>
      </p>
    </div>
  );
}
