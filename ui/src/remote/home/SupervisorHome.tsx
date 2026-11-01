// The phone's default home: the one account supervisor, what it drives on each computer, its
// pause, and what it typed. Mirrors the desktop SupervisorPanelView, laid out as touch cards per
// machine. A computer that does not hold the supervisor lease is on standby: it offers "Move
// supervisor here", never a second supervisor.
import { ArrowRightLeft, LoaderCircle, RotateCw } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import { actingSessionIds, isStandbyRole, leaseNotices, type SupervisorLane, type SupervisorSnapshot } from "../../lib/supervisorModel";
import { forceMoveWarning, type SupervisorMoveResult } from "../../lib/unifiedSupervisor";
import { UnifiedSupervisorSection } from "../../components/supervisor/UnifiedSupervisorSection";
import type { AdoptArchivedResult, LanePauseResult } from "../remoteSupervisor";
import { userFacingError } from "../userFacingError";
import { PhoneHomeHeader } from "./PhoneHomeHeader";
import type { UnifiedHostView } from "./usePhoneSupervisor";
import {
  actionText,
  headlineSentence,
  laneFolder,
  laneStatus,
  recentActions,
  relativeTime,
  supervisorHeadline,
  type RecentAction,
  type SupervisorMachineView,
} from "./phoneHomeModel";

export type SupervisorHomeProps = {
  readonly machines: readonly SupervisorMachineView[];
  readonly now: number;
  /** `machineId\u0000sessionId` of sessions waiting on the person. */
  readonly needsYou: ReadonlySet<string>;
  readonly onSetPaused: (machineId: string, laneKey: string, paused: boolean) => Promise<LanePauseResult>;
  readonly onOpenLane: (machineId: string, sessionId: string) => void;
  /** Moves the account supervisor to a standby machine. Absent: standby machines offer no move. */
  readonly onMoveHere?: (machineId: string, force: boolean) => Promise<SupervisorMoveResult>;
  /** Adopts a standby machine's archived open goals. Absent: the list shows without the action. */
  readonly onAdoptArchived?: (machineId: string, goalIds: readonly string[]) => Promise<AdoptArchivedResult>;
  /** The one account supervisor host, live or last seen. Absent: the lane view only. */
  readonly host?: UnifiedHostView | null;
  /** Account discovery has not answered yet: an empty machine list means "still looking". */
  readonly discovering?: boolean;
  /** Asks one computer for its supervisor again. Absent: unreadable computers offer no retry. */
  readonly onRetry?: (machineId: string) => void;
  /** Renders the phone home header (title plus `headerAction`). Off where a host surface titles it. */
  readonly phoneHeader?: boolean;
  /** Trailing header action (the Account entry). */
  readonly headerAction?: ReactNode;
};

const RECENT_COLLAPSED = 3;

const SECONDARY_BUTTON =
  "flex h-11 shrink-0 items-center justify-center gap-1.5 rounded-lg bg-chat-surface-raised px-3 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover active:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

const NOTE = "flex min-w-0 flex-col gap-2 rounded-xl bg-chat-surface px-3 py-2 text-sm leading-snug text-chat-foreground-secondary";

/** The holder a standby machine names, or the account host the home already knows. */
function holderName(snapshot: SupervisorSnapshot, host: UnifiedHostView | null): string {
  return snapshot.hub.holderLabel || host?.label || snapshot.hub.holderMachineId || "another computer";
}

function clockTime(atMs: number): string {
  const date = new Date(atMs);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/**
 * A machine that does not hold the account supervisor lease: where the supervisor runs, the
 * move (a forced one shows its data-loss window and needs a second confirm), what does not
 * line up with the holder, and the open goals of its archived pre-lease supervisor.
 */
function StandbyNote({
  machineId,
  label,
  snapshot,
  host,
  onMoveHere,
  onAdoptArchived,
}: {
  machineId: string;
  label: string;
  snapshot: SupervisorSnapshot;
  host: UnifiedHostView | null;
  onMoveHere?: (machineId: string, force: boolean) => Promise<SupervisorMoveResult>;
  onAdoptArchived?: (machineId: string, goalIds: readonly string[]) => Promise<AdoptArchivedResult>;
}) {
  const [moving, setMoving] = useState(false);
  const [forceWarning, setForceWarning] = useState<string | null>(null);
  const [adopting, setAdopting] = useState<ReadonlySet<string>>(() => new Set());
  const holder = holderName(snapshot, host);
  const { hub } = snapshot;

  const move = async (force: boolean) => {
    if (!onMoveHere || moving) return;
    setMoving(true);
    const result = await onMoveHere(machineId, force);
    setMoving(false);
    if (result.kind === "force-required") {
      setForceWarning(forceMoveWarning(result, holder, clockTime));
      return;
    }
    setForceWarning(null);
    if (result.kind === "error") toast.error(userFacingError(result, { action: "move the supervisor" }));
  };

  const adopt = async (goalId: string) => {
    if (!onAdoptArchived || adopting.has(goalId)) return;
    setAdopting((prev) => new Set(prev).add(goalId));
    const result = await onAdoptArchived(machineId, [goalId]);
    setAdopting((prev) => {
      const copy = new Set(prev);
      copy.delete(goalId);
      return copy;
    });
    if (result.kind === "error") toast.error(userFacingError(result, { action: "adopt the goal" }));
  };

  const adoptable = hub.adoptable ?? [];
  const unmapped = hub.unmappedHosts ?? [];
  return (
    <div data-testid="supervisor-standby" data-machine-id={machineId} className={NOTE}>
      <div className="flex min-w-0 items-center gap-2">
        <span data-testid="supervisor-standby-text" className="min-w-0 flex-1 py-1">Standby - supervisor runs on {holder}</span>
        {onMoveHere && hub.available && forceWarning === null ? (
          <button
            type="button"
            data-testid="supervisor-move-here"
            aria-label={`Move supervisor to ${label}`}
            aria-busy={moving || undefined}
            disabled={moving}
            onClick={() => void move(false)}
            className={SECONDARY_BUTTON}
          >
            {moving ? (
              <LoaderCircle aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
            ) : (
              <ArrowRightLeft aria-hidden="true" className="size-4" />
            )}
            {moving ? "Moving…" : "Move supervisor here"}
          </button>
        ) : null}
      </div>
      {forceWarning !== null ? (
        <div data-testid="supervisor-move-force" role="alert" className="flex min-w-0 flex-col gap-2 rounded-lg bg-status-warning/15 px-2 py-2 text-chat-foreground">
          <p data-testid="supervisor-move-force-warning">{forceWarning}</p>
          <div className="flex gap-2">
            <button
              type="button"
              data-testid="supervisor-move-force-confirm"
              aria-busy={moving || undefined}
              disabled={moving}
              onClick={() => void move(true)}
              className={SECONDARY_BUTTON}
            >
              Move anyway
            </button>
            <button type="button" data-testid="supervisor-move-force-cancel" disabled={moving} onClick={() => setForceWarning(null)} className={SECONDARY_BUTTON}>
              Cancel
            </button>
          </div>
        </div>
      ) : null}
      {hub.holderPaired === false ? (
        <p data-testid="supervisor-standby-unpaired">
          {label} is not paired to {holder}, so its sessions are not supervised yet. Pair them from Ferryx on {label}.
        </p>
      ) : null}
      {unmapped.length > 0 ? (
        <p data-testid="supervisor-standby-unmapped">
          Some goals name a computer {label} does not know ({unmapped.join(", ")}). They stay held until it is paired.
        </p>
      ) : null}
      {adoptable.length > 0 ? (
        <ul data-testid="supervisor-adoptable" className="flex flex-col gap-1">
          {adoptable.map((goal) => (
            <li key={goal.goalId} data-testid="supervisor-adoptable-goal" data-goal-id={goal.goalId} className="flex min-w-0 items-center gap-2">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-chat-foreground">{goal.title}</span>
                <span className="block truncate text-xs">Adoptable from {goal.fromMachine || label}</span>
              </span>
              {onAdoptArchived ? (
                <button
                  type="button"
                  data-testid="supervisor-adopt-archived"
                  aria-label={`Adopt ${goal.title}`}
                  aria-busy={adopting.has(goal.goalId) || undefined}
                  disabled={adopting.has(goal.goalId)}
                  onClick={() => void adopt(goal.goalId)}
                  className={SECONDARY_BUTTON}
                >
                  Adopt
                </button>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function MachineStateNote({ machine, onRetry }: { machine: SupervisorMachineView; onRetry?: () => void }) {
  const text =
    machine.state === "loading"
      ? `Checking ${machine.label}…`
      : machine.state === "offline"
        ? `${machine.label} is offline. Open Ferryx on it to see its supervisor.`
        : machine.state === "not-connected"
          ? `Waiting for ${machine.label} to connect.`
          : machine.state === "unsupported"
            ? "Update Ferryx on this computer to see its supervisor."
            : machine.state === "error"
              ? userFacingError(machine.error, { action: "read the supervisor" })
              : null;
  if (!text) return null;
  const retry = onRetry && (machine.state === "error" || machine.state === "offline" || machine.state === "not-connected");
  return (
    <div
      data-testid="supervisor-machine-state"
      data-state={machine.state}
      className="flex min-w-0 items-center gap-2 rounded-xl bg-chat-surface px-3 py-2 text-sm leading-snug text-chat-foreground-secondary"
    >
      <p className="min-w-0 flex-1 py-1">{text}</p>
      {retry ? (
        <button type="button" data-testid="supervisor-machine-retry" onClick={onRetry} className={SECONDARY_BUTTON}>
          <RotateCw aria-hidden="true" className="size-4" />
          Retry
        </button>
      ) : null}
    </div>
  );
}

function LaneCard({
  machineId,
  lane,
  acting,
  needsYou,
  pending,
  onTogglePause,
  onOpen,
}: {
  machineId: string;
  lane: SupervisorLane;
  acting: ReadonlySet<string>;
  needsYou: boolean;
  pending: boolean;
  onTogglePause: () => void;
  onOpen: (() => void) | null;
}) {
  const status = laneStatus(lane, acting, needsYou);
  const folder = laneFolder(lane);
  const body = (
    <>
      <span className="flex min-w-0 items-start gap-2">
        <span className="min-w-0 flex-1 line-clamp-2 break-words text-base font-medium text-chat-foreground">{lane.title}</span>
        <span
          data-testid="supervisor-lane-status"
          data-status={status.kind}
          className={cn(
            "shrink-0 rounded-full px-2 py-0.5 text-xs font-medium",
            status.kind === "typing"
              ? "bg-accent text-chat-foreground"
              : status.kind === "needs-you"
                ? "bg-status-warning/15 text-status-warning"
                : "bg-chat-surface-raised text-chat-foreground-secondary",
          )}
        >
          {status.label}
        </span>
      </span>
      {folder ? <span className="mt-0.5 block min-w-0 truncate text-xs text-chat-foreground">{folder}</span> : null}
      {lane.goal && lane.goal !== lane.title ? (
        <span className="mt-1 line-clamp-2 min-w-0 break-words text-sm leading-snug text-chat-foreground">{lane.goal}</span>
      ) : null}
      {lane.lastReason ? (
        <span data-testid="supervisor-lane-reason" className="mt-1 line-clamp-3 min-w-0 break-words text-sm leading-snug text-chat-foreground-secondary">
          Stopped: {lane.lastReason}
        </span>
      ) : null}
    </>
  );
  return (
    <li
      data-testid="supervisor-lane-card"
      data-lane-key={lane.key}
      data-machine-id={machineId}
      className={cn("flex min-w-0 items-stretch gap-1 rounded-xl p-1", status.kind === "typing" ? "bg-accent" : "bg-chat-surface")}
    >
      {onOpen ? (
        <button
          type="button"
          data-testid="supervisor-lane-open"
          aria-label={`Open ${lane.title}, ${status.label}`}
          onClick={onOpen}
          className="flex min-h-11 min-w-0 flex-1 flex-col justify-center rounded-lg px-2.5 py-2 text-left transition-colors hover:bg-chat-row-hover active:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {body}
        </button>
      ) : (
        <div className="flex min-h-11 min-w-0 flex-1 flex-col justify-center px-2.5 py-2">{body}</div>
      )}
      <button
        type="button"
        data-testid="supervisor-lane-pause"
        aria-label={lane.paused ? `Resume ${lane.title}` : `Pause ${lane.title}`}
        aria-busy={pending || undefined}
        disabled={pending}
        onClick={onTogglePause}
        className={cn(SECONDARY_BUTTON, "self-center")}
      >
        {lane.paused ? "Resume" : "Pause"}
      </button>
    </li>
  );
}

function RecentRow({ action, now, showMachine, onOpen }: { action: RecentAction; now: number; showMachine: boolean; onOpen: (() => void) | null }) {
  const where = [action.where, showMachine ? action.machineLabel : null].filter(Boolean).join(" · ");
  const content = (
    <>
      <span className="flex min-w-0 items-baseline gap-2">
        <span className="min-w-0 flex-1 truncate text-sm text-chat-foreground">{actionText(action)}</span>
        <span className="shrink-0 text-xs tabular-nums text-chat-foreground-secondary">{relativeTime(action.atMs, now)}</span>
      </span>
      {where ? <span className="block min-w-0 truncate text-xs text-chat-foreground-secondary">{where}</span> : null}
    </>
  );
  const classes = "flex min-h-11 w-full min-w-0 flex-col justify-center rounded-lg px-2 py-1.5 text-left";
  return (
    <li data-testid="supervisor-recent-action">
      {onOpen ? (
        <button
          type="button"
          onClick={onOpen}
          className={cn(classes, "transition-colors hover:bg-chat-row-hover active:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring")}
        >
          {content}
        </button>
      ) : (
        <div className={classes}>{content}</div>
      )}
    </li>
  );
}

export function SupervisorHome({
  machines,
  now,
  needsYou,
  onSetPaused,
  onOpenLane,
  onMoveHere,
  onAdoptArchived,
  host = null,
  discovering = false,
  onRetry,
  phoneHeader = false,
  headerAction,
}: SupervisorHomeProps) {
  const [pending, setPending] = useState<ReadonlySet<string>>(() => new Set());
  const [recentExpanded, setRecentExpanded] = useState(false);
  // The lane's paused value the person asked for, shown until the next snapshot settles it.
  const [optimistic, setOptimistic] = useState<ReadonlyMap<string, boolean>>(() => new Map());
  // Once a snapshot shows the asked-for value the override has done its job: drop it, so a later
  // change from another surface (desktop, Slack) shows instead of the stale request.
  useEffect(() => {
    if (optimistic.size === 0) return;
    const settled = [...optimistic].filter(([id, paused]) => {
      const [machineId, laneKey] = id.split("\u0000");
      const lane = machines.find((m) => m.machineId === machineId)?.snapshot?.lanes.find((item) => item.key === laneKey);
      return lane?.paused === paused;
    });
    if (settled.length === 0) return;
    setOptimistic((prev) => {
      const copy = new Map(prev);
      for (const [id, paused] of settled) if (copy.get(id) === paused) copy.delete(id);
      return copy;
    });
  }, [machines, optimistic]);
  const headline = supervisorHeadline(machines, now, discovering);
  const recent = recentActions(machines);
  const shownRecent = recentExpanded ? recent : recent.slice(0, RECENT_COLLAPSED);
  const readable = machines.filter((machine) => machine.snapshot);
  const severalMachines = machines.length > 1;

  const togglePause = async (machineId: string, lane: SupervisorLane) => {
    const id = `${machineId}\u0000${lane.key}`;
    if (pending.has(id)) return;
    const next = !lane.paused;
    setPending((prev) => new Set(prev).add(id));
    setOptimistic((prev) => new Map(prev).set(id, next));
    const result = await onSetPaused(machineId, lane.key, next);
    setPending((prev) => {
      const copy = new Set(prev);
      copy.delete(id);
      return copy;
    });
    if (result.kind === "error") {
      setOptimistic((prev) => {
        const copy = new Map(prev);
        copy.delete(id);
        return copy;
      });
      toast.error(userFacingError(result, { action: next ? "pause the lane" : "resume the lane" }));
    }
  };

  const openRecent = (action: RecentAction) => {
    const sessionId = action.sessionId;
    return sessionId && action.where ? () => onOpenLane(action.machineId, sessionId) : null;
  };

  return (
    <div data-testid="supervisor-home" data-headline={headline.kind} className="flex min-h-0 flex-1 flex-col bg-chat-screen">
      {phoneHeader ? <PhoneHomeHeader title="Supervisor" trailing={headerAction} /> : null}
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto scrollbar-sleek">
        <div className={cn("flex w-full flex-col gap-5 px-4 pb-6", phoneHeader ? null : "pt-4")}>
          {machines.length > 0 ? (
            <p data-testid="supervisor-home-headline" className="px-1 text-sm leading-snug text-chat-foreground-secondary">
              {headlineSentence(headline)}
            </p>
          ) : null}

          {host ? (
            <UnifiedSupervisorSection
              surface="phone"
              hostLabel={host.label}
              connection={host.connection}
              connectionError={host.error}
              view={host.view}
              client={host.client}
            />
          ) : null}

          {machines.length === 0 ? (
            discovering ? (
              <p data-testid="supervisor-home-discovering" role="status" className="flex items-center gap-2 rounded-xl bg-chat-surface px-3 py-4 text-sm text-chat-foreground-secondary">
                <LoaderCircle aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
                Finding your computers…
              </p>
            ) : (
              <p data-testid="supervisor-home-empty" className="rounded-xl bg-chat-surface px-3 py-4 text-sm leading-snug text-chat-foreground-secondary">
                No computers are linked to this account yet. Open Ferryx on your computer and sign in there.
              </p>
            )
          ) : null}

          {machines.map((machine) => {
            const snapshot = machine.snapshot;
            const acting = snapshot ? actingSessionIds(snapshot, now) : new Set<string>();
            const standby = snapshot !== null && isStandbyRole(snapshot.hub.role);
            const hubOff = snapshot && !standby && (!snapshot.hub.available || (!snapshot.hub.running && snapshot.lanes.length === 0));
            const notices = snapshot ? leaseNotices(snapshot.hub, machine.label) : [];
            return (
              <section key={machine.machineId} data-testid={`supervisor-machine-${machine.machineId}`} aria-label={machine.label} className="flex flex-col gap-2">
                <h2 className="truncate px-1 text-sm font-semibold text-chat-foreground">{machine.label}</h2>
                {snapshot ? null : <MachineStateNote machine={machine} {...(onRetry ? { onRetry: () => onRetry(machine.machineId) } : {})} />}
                {standby && snapshot ? (
                  <StandbyNote
                    machineId={machine.machineId}
                    label={machine.label}
                    snapshot={snapshot}
                    host={host}
                    {...(onMoveHere ? { onMoveHere } : {})}
                    {...(onAdoptArchived ? { onAdoptArchived } : {})}
                  />
                ) : null}
                {notices.map((notice) => (
                  <p
                    key={notice.kind}
                    data-testid="supervisor-lease-notice"
                    data-kind={notice.kind}
                    role="status"
                    className="rounded-xl bg-status-warning/15 px-3 py-2 text-sm leading-snug text-chat-foreground"
                  >
                    {notice.text}
                  </p>
                ))}
                {standby && snapshot?.lanes.length === 0 ? null : hubOff ? (
                  <div data-testid="supervisor-hub-off" className="flex min-w-0 items-center gap-2 rounded-xl bg-chat-surface px-3 py-2 text-sm leading-snug text-chat-foreground-secondary">
                    <span className="min-w-0 py-1">The supervisor isn't running. Start it from Ferryx on {machine.label}.</span>
                  </div>
                ) : snapshot && snapshot.lanes.length === 0 ? (
                  <p className="rounded-xl bg-chat-surface px-3 py-3 text-sm leading-snug text-chat-foreground-secondary">
                    {snapshot.watched.length > 0
                      ? `No lanes yet. Watching ${snapshot.watched.length} session${snapshot.watched.length === 1 ? "" : "s"}.`
                      : "No lanes yet. Give the supervisor a goal from Ferryx on this computer."}
                  </p>
                ) : snapshot ? (
                  <ul className="flex flex-col gap-2">
                    {snapshot.lanes.map((raw) => {
                      const id = `${machine.machineId}\u0000${raw.key}`;
                      const override = optimistic.get(id);
                      const lane = override === undefined || override === raw.paused ? raw : { ...raw, paused: override };
                      const sessionId = lane.sessionId;
                      return (
                        <LaneCard
                          key={raw.key}
                          machineId={machine.machineId}
                          lane={lane}
                          acting={acting}
                          needsYou={Boolean(sessionId && needsYou.has(`${machine.machineId}\u0000${sessionId}`))}
                          pending={pending.has(id)}
                          onTogglePause={() => void togglePause(machine.machineId, raw)}
                          onOpen={sessionId ? () => onOpenLane(machine.machineId, sessionId) : null}
                        />
                      );
                    })}
                  </ul>
                ) : null}
              </section>
            );
          })}

          {readable.length > 0 && recent.length > 0 ? (
            <section aria-labelledby="supervisor-recent-title" className="flex flex-col gap-1">
              <h2 id="supervisor-recent-title" className="px-1 text-sm font-semibold text-chat-foreground">Recent</h2>
              <ol className="flex flex-col">
                {shownRecent.map((action) => (
                  <RecentRow
                    key={`${action.machineId}-${action.atMs}-${action.requestId ?? action.op}-${action.sessionId ?? ""}`}
                    action={action}
                    now={now}
                    showMachine={severalMachines}
                    onOpen={openRecent(action)}
                  />
                ))}
              </ol>
              {recent.length > RECENT_COLLAPSED ? (
                <button
                  type="button"
                  data-testid="supervisor-recent-toggle"
                  aria-expanded={recentExpanded}
                  onClick={() => setRecentExpanded((value) => !value)}
                  className="flex h-11 items-center self-start rounded-lg px-2 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  {recentExpanded ? "Show less" : `Show all ${recent.length}`}
                </button>
              ) : null}
            </section>
          ) : null}
        </div>
      </div>
    </div>
  );
}
