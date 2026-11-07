// The account supervisor lease in the panel: a computer that does not hold the lease is on
// standby. It says where the supervisor runs and offers "Move supervisor here" (a forced move
// shows its data-loss window and needs a second confirm), the pairing and host-mapping notices,
// and the open goals of its archived pre-lease supervisor with Adopt. It never starts a second
// supervisor. The lease warnings of a holder (hosts it cannot reach, ...) use `LeaseNotices`.
import { useState } from 'react';

import { cn } from '../../lib/cn';
import { leaseNotices, type SupervisorHubState } from '../../lib/supervisorModel';
import { forceMoveWarning, type SupervisorMoveResult } from '../../lib/unifiedSupervisor';

/** What an Adopt answers: the desktop IPC and the remote route both fit. */
export type LeaseAdoptResult = { readonly kind: 'ok' } | { readonly kind: 'error'; readonly message: string };

/** Local wall-clock time of the checkpoint a forced move would fall back to. */
function clockLabel(ms: number): string {
  const date = new Date(ms);
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

const BUTTON =
  'inline-flex items-center justify-center rounded-md border border-input bg-secondary px-3 text-sm font-medium text-foreground transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50';

/** Lease warnings of one computer (`here` names it): unreachable hosts, dropped observations, a blocked takeover. */
export function LeaseNotices({ hub, here, touch }: { hub: SupervisorHubState; here: string; touch?: boolean }) {
  const notices = leaseNotices(hub, here);
  if (notices.length === 0) return null;
  return (
    <div className="flex flex-col gap-1">
      {notices.map((notice) => (
        <p
          key={notice.kind}
          data-testid="supervisor-lease-notice"
          data-kind={notice.kind}
          role="status"
          className={cn('rounded-md bg-status-warning/15 px-2 py-1.5 leading-snug text-foreground', touch ? 'text-sm' : 'text-xs')}
        >
          {notice.text}
        </p>
      ))}
    </div>
  );
}

export type StandbyCardProps = {
  hub: SupervisorHubState;
  /** This computer's name in the text: "This computer" on the desktop, the machine label on the phone. */
  here: string;
  /** The holder's name when the hub does not report one (the account host the surface knows). */
  fallbackHolder?: string | null;
  /** Absent: no move is offered. */
  moveHere?: (force: boolean) => Promise<SupervisorMoveResult>;
  /** Absent: the archived goals are listed without Adopt. */
  adoptArchived?: (goalIds: readonly string[]) => Promise<LeaseAdoptResult>;
  touch?: boolean;
  /** Distinguishes several standby cards on one surface. */
  machineId?: string;
};

export function StandbyCard({ hub, here, fallbackHolder, moveHere, adoptArchived, touch, machineId }: StandbyCardProps) {
  const holder = hub.holderLabel || fallbackHolder || hub.holderMachineId || 'another computer';
  const [moving, setMoving] = useState(false);
  const [forceWarning, setForceWarning] = useState<string | null>(null);
  const [adopting, setAdopting] = useState<ReadonlySet<string>>(() => new Set());
  const [error, setError] = useState<string | null>(null);

  const move = async (force: boolean) => {
    if (!moveHere || moving) return;
    setMoving(true);
    setError(null);
    const result = await moveHere(force);
    setMoving(false);
    if (result.kind === 'force-required') {
      setForceWarning(forceMoveWarning(result, holder, clockLabel));
      return;
    }
    setForceWarning(null);
    if (result.kind === 'error') setError(`Could not move the supervisor: ${result.message}`);
  };

  const adopt = async (goalId: string) => {
    if (!adoptArchived || adopting.has(goalId)) return;
    setAdopting((prev) => new Set(prev).add(goalId));
    setError(null);
    const result = await adoptArchived([goalId]);
    setAdopting((prev) => {
      const copy = new Set(prev);
      copy.delete(goalId);
      return copy;
    });
    if (result.kind === 'error') setError(`Could not adopt the goal: ${result.message}`);
  };

  const text = touch ? 'text-sm' : 'text-xs';
  const button = cn(BUTTON, touch ? 'min-h-11' : 'min-h-8');
  const adoptable = hub.adoptable ?? [];
  const unmapped = hub.unmappedHosts ?? [];
  return (
    <section
      aria-label={`${here} on standby`}
      data-testid="supervisor-standby"
      data-machine-id={machineId}
      className={cn('flex flex-col gap-2 rounded-xl border border-border bg-card p-3 leading-snug text-muted-foreground', text)}
    >
      <p data-testid="supervisor-standby-text" role="status" className="text-foreground">
        Supervisor runs on {holder}. {here} is on standby.
      </p>
      {moveHere && hub.available && forceWarning === null ? (
        <button
          type="button"
          data-testid="supervisor-move-here"
          aria-busy={moving || undefined}
          disabled={moving}
          onClick={() => void move(false)}
          className={cn(button, 'self-start')}
        >
          {moving ? 'Moving…' : 'Move supervisor here'}
        </button>
      ) : null}
      {forceWarning !== null ? (
        <div data-testid="supervisor-move-force" role="alert" className="flex flex-col gap-2 rounded-md bg-status-warning/15 p-2 text-foreground">
          <span data-testid="supervisor-move-force-warning">{forceWarning}</span>
          <div className="flex gap-2">
            <button type="button" data-testid="supervisor-move-force-confirm" disabled={moving} onClick={() => void move(true)} className={button}>
              Move anyway
            </button>
            <button type="button" data-testid="supervisor-move-force-cancel" disabled={moving} onClick={() => setForceWarning(null)} className={button}>
              Cancel
            </button>
          </div>
        </div>
      ) : null}
      {hub.holderPaired === false ? (
        <p data-testid="supervisor-standby-unpaired">
          {here} is not paired to {holder}, so its sessions are not supervised yet.
        </p>
      ) : null}
      {unmapped.length > 0 ? (
        <p data-testid="supervisor-standby-unmapped">
          Some goals name a computer {here} does not know ({unmapped.join(', ')}). They stay held until it is paired.
        </p>
      ) : null}
      {adoptable.length > 0 ? (
        <ul data-testid="supervisor-adoptable" className="flex flex-col gap-1">
          {adoptable.map((goal) => (
            <li key={goal.goalId} data-testid="supervisor-adoptable-goal" data-goal-id={goal.goalId} className="flex min-w-0 items-center gap-2">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-foreground">{goal.title}</span>
                <span className="block truncate">Adoptable from {goal.fromMachine || here}</span>
              </span>
              {adoptArchived ? (
                <button
                  type="button"
                  data-testid="supervisor-adopt-archived"
                  aria-label={`Adopt ${goal.title}`}
                  aria-busy={adopting.has(goal.goalId) || undefined}
                  disabled={adopting.has(goal.goalId)}
                  onClick={() => void adopt(goal.goalId)}
                  className={button}
                >
                  Adopt
                </button>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      <LeaseNotices hub={hub} here={here} touch={touch} />
      {error ? (
        <p role="alert" data-testid="supervisor-standby-error" className="text-status-warning">
          {error}
        </p>
      ) : null}
    </section>
  );
}
