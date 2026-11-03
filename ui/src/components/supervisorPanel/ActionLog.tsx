import { useState } from 'react';

import { cn } from '../../lib/cn';
import { StatusGlyph } from './StatusGlyph';
import type { ActionLogLine, ActionLogProps } from './types';

const LINK =
  'rounded-sm text-muted-foreground underline underline-offset-2 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring';

function LogLine({ line, touch, onUndo }: { line: ActionLogLine; touch?: boolean; onUndo: (id: number) => void }) {
  const [why, setWhy] = useState(false);
  return (
    <li data-testid="action-log-line" className={cn('flex flex-col gap-1 leading-snug text-muted-foreground', touch ? 'text-sm' : 'text-xs')}>
      <div className="flex items-start gap-2">
        <StatusGlyph state="done" className="mt-0.5 text-muted-foreground" />
        <span className="min-w-0 flex-1">
          <span className="text-foreground">{line.summary}</span>
          {line.unverified ? (
            <span className="ml-1.5 rounded-sm bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">Unverified</span>
          ) : null}
        </span>
        <span className="shrink-0 whitespace-nowrap text-xs">{line.timeLabel}</span>
      </div>
      <div className={cn('flex items-center gap-3 pl-6', touch && 'gap-1')}>
        <button
          type="button"
          aria-expanded={why}
          aria-label={`Why: ${line.summary}`}
          onClick={() => setWhy((v) => !v)}
          className={cn(LINK, touch && 'min-h-11 min-w-11 px-2')}
        >
          Why
        </button>
        {line.undoable ? (
          <button
            type="button"
            aria-label={`Undo: ${line.summary}`}
            onClick={() => onUndo(line.id)}
            className={cn(LINK, touch && 'min-h-11 min-w-11 px-2')}
          >
            Undo
          </button>
        ) : null}
      </div>
      {why ? <p className="pl-6">{line.basis}</p> : null}
    </li>
  );
}

export function ActionLog({ entries, touch, onUndo }: ActionLogProps) {
  const [all, setAll] = useState(false);
  const shown = all ? entries : entries.slice(0, 2);
  return (
    <section aria-labelledby="supervisor-log-heading" className="flex flex-col gap-2.5">
      <div className="flex items-center gap-1.5">
        <h2 id="supervisor-log-heading" className={cn('flex-1 font-medium text-muted-foreground', touch ? 'text-sm' : 'text-xs')}>
          Done for you today
        </h2>
        {entries.length > 2 ? (
          <button
            type="button"
            aria-expanded={all}
            onClick={() => setAll((v) => !v)}
            className={cn(LINK, touch ? 'min-h-11 min-w-11 px-2 text-sm' : 'text-xs')}
          >
            {all ? 'Less' : `All ${entries.length}`}
          </button>
        ) : null}
      </div>
      <ul className="flex flex-col gap-2">
        {shown.map((line) => (
          <LogLine key={line.id} line={line} touch={touch} onUndo={onUndo} />
        ))}
      </ul>
    </section>
  );
}
