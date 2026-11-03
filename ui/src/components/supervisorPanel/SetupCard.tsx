import { cn } from '../../lib/cn';
import { SupervisorMark } from './StatusLine';
import type { SetupCardProps } from './types';

export function SetupCard({ computer, touch, onTurnOn, pending, error }: SetupCardProps) {
  return (
    <section
      aria-label="Supervisor setup"
      data-testid="setup-card"
      className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4"
    >
      <div className="flex items-center gap-2">
        <SupervisorMark />
        <h2 className="text-sm font-semibold text-foreground">Turn on the supervisor on {computer}</h2>
      </div>
      <p className={cn('text-muted-foreground', touch ? 'text-sm' : 'text-xs')}>
        It answers routine questions, retries errors and brings you only the decisions that need you.
      </p>
      <button
        type="button"
        onClick={onTurnOn}
        disabled={pending}
        className={cn(
          'inline-flex items-center justify-center self-start rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50',
          touch ? 'min-h-11 w-full' : 'min-h-8',
        )}
      >
        Turn on
      </button>
      {error ? (
        <p role="alert" className={cn('text-status-warning', touch ? 'text-sm' : 'text-xs')}>
          {error}
        </p>
      ) : null}
    </section>
  );
}
