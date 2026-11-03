import { cn } from '../../lib/cn';
import { StatusGlyph } from './StatusGlyph';
import { SupervisorMark } from './StatusLine';
import type { DecisionCardProps } from './types';

export function DecisionCard({
  sessionName,
  computer,
  question,
  command,
  options,
  recommendation,
  onAnswer,
  onOpenTerminal,
  touch,
}: DecisionCardProps) {
  // A decision without a recommended option and its reason is never shown.
  if (
    !recommendation ||
    recommendation.reason.trim() === '' ||
    recommendation.index < 0 ||
    recommendation.index >= options.length
  ) {
    return null;
  }
  const order = [recommendation.index, ...options.map((_, i) => i).filter((i) => i !== recommendation.index)];

  return (
    <article
      data-testid="decision-card"
      aria-label={`Decision for ${sessionName}`}
      className="flex flex-col gap-2 rounded-xl border border-border bg-card p-3"
    >
      <button
        type="button"
        onClick={onOpenTerminal}
        aria-label={`Open ${sessionName} terminal`}
        className={cn(
          'flex w-full items-center gap-1.5 rounded-md text-left text-xs text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
          touch && 'min-h-11 text-sm',
        )}
      >
        <StatusGlyph state="needs" />
        <span className="truncate font-medium text-foreground">{sessionName}</span>
        <span className="shrink-0">· {computer}</span>
        <span className="flex-1" />
        <span className="shrink-0 underline underline-offset-2">Terminal</span>
      </button>
      <p className={cn('font-medium leading-snug text-foreground', touch ? 'text-base' : 'text-[15px]')}>{question}</p>
      {command ? (
        <p
          title={command}
          className="truncate rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-muted-foreground"
        >
          {command}
        </p>
      ) : null}
      <p className={cn('flex gap-2 leading-snug text-muted-foreground', touch ? 'text-sm' : 'text-xs')}>
        <SupervisorMark className="mt-px" />
        <span>
          <span className="sr-only">Recommended: </span>
          {recommendation.reason}
        </span>
      </p>
      <div className="flex flex-wrap gap-2">
        {order.map((i) => {
          const primary = i === recommendation.index;
          return (
            <button
              key={i}
              type="button"
              data-primary={primary ? 'true' : 'false'}
              onClick={() => onAnswer(i)}
              className={cn(
                'inline-flex items-center justify-center rounded-md px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                touch ? 'min-h-11 flex-1' : 'min-h-8',
                primary
                  ? 'bg-primary text-primary-foreground hover:bg-primary/90'
                  : 'border border-input bg-secondary text-foreground hover:bg-accent',
              )}
            >
              {options[i]}
            </button>
          );
        })}
      </div>
    </article>
  );
}
