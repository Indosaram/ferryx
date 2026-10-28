// The phone's default home: what the hub supervisor on each computer drives, its pause, and
// what it typed. Mirrors the desktop SupervisorPanelView, laid out as touch cards per machine.
import { Eye, Pause, Play } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import { actingSessionIds, type SupervisorLane } from "../../lib/supervisorModel";
import { UnifiedSupervisorSection } from "../../components/supervisor/UnifiedSupervisorSection";
import type { LanePauseResult } from "../remoteSupervisor";
import type { UnifiedHostView } from "./usePhoneSupervisor";
import {
  actionText,
  clock,
  headlineSentence,
  laneName,
  laneStatus,
  recentActions,
  supervisorHeadline,
  type SupervisorMachineView,
} from "./phoneHomeModel";

export type SupervisorHomeProps = {
  readonly machines: readonly SupervisorMachineView[];
  readonly now: number;
  /** `machineId\u0000sessionId` of sessions waiting on the person. */
  readonly needsYou: ReadonlySet<string>;
  readonly onSetPaused: (machineId: string, laneKey: string, paused: boolean) => Promise<LanePauseResult>;
  readonly onOpenLane: (machineId: string, sessionId: string) => void;
  /** The unified supervisor host(s), live or last seen. Absent: the lane view only. */
  readonly unified?: readonly UnifiedHostView[];
};

const SECTION_LABEL = "px-1 text-[12px] font-medium text-chat-foreground-secondary";

function MachineStateNote({ machine }: { machine: SupervisorMachineView }) {
  const text =
    machine.state === "loading"
      ? "Loading…"
      : machine.state === "offline"
        ? "Offline. Open Ferryx on this computer."
        : machine.state === "not-connected"
          ? "Not connected yet."
          : machine.state === "unsupported"
            ? "Update Ferryx on this computer to see its supervisor."
            : machine.state === "error"
              ? `Could not read the supervisor${machine.error ? ` (${machine.error.code})` : ""}.`
              : null;
  if (!text) return null;
  return (
    <p data-testid="supervisor-machine-state" data-state={machine.state} className="rounded-xl bg-chat-surface px-3 py-3 text-[13px] leading-snug text-chat-foreground-secondary">
      {text}
    </p>
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
  const body = (
    <>
      <span className="flex min-w-0 items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-[15px] font-medium text-chat-foreground">{laneName(lane)}</span>
        <span
          data-testid="supervisor-lane-status"
          data-status={status.kind}
          className={cn(
            "shrink-0 rounded-full px-2 py-0.5 text-[12px] font-medium",
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
      <span className="mt-1 flex min-w-0 gap-1.5 text-[13px] leading-snug">
        <span className="shrink-0 text-chat-foreground-secondary">Goal</span>
        <span className="min-w-0 flex-1 truncate text-chat-foreground">{lane.goal ?? lane.title}</span>
      </span>
      {lane.lastReason ? (
        <span data-testid="supervisor-lane-reason" className="mt-0.5 block min-w-0 truncate text-[13px] leading-snug text-chat-foreground-secondary">
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
          aria-label={`Open ${laneName(lane)}, ${status.label}`}
          onClick={onOpen}
          className="flex min-h-11 min-w-0 flex-1 flex-col justify-center rounded-lg px-2.5 py-2 text-left transition-colors hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
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
        className="flex size-11 shrink-0 items-center justify-center self-center rounded-lg text-chat-foreground-secondary transition-colors hover:bg-chat-row-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
      >
        {lane.paused ? <Play aria-hidden="true" className="size-4" /> : <Pause aria-hidden="true" className="size-4" />}
      </button>
    </li>
  );
}

export function SupervisorHome({ machines, now, needsYou, onSetPaused, onOpenLane, unified = [] }: SupervisorHomeProps) {
  const [pending, setPending] = useState<ReadonlySet<string>>(() => new Set());
  // The lane's paused value the person asked for, shown until the next snapshot settles it.
  const [optimistic, setOptimistic] = useState<ReadonlyMap<string, boolean>>(() => new Map());
  const headline = supervisorHeadline(machines, now);
  const recent = recentActions(machines);
  const readable = machines.filter((machine) => machine.snapshot);
  const anySupervisor = readable.some((machine) => machine.snapshot!.hub.available);

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
      toast.error(next ? "Could not pause the lane" : "Could not resume the lane", { description: `${result.code}: ${result.message}` });
    }
  };

  return (
    <div data-testid="supervisor-home" data-headline={headline} className="flex min-h-0 flex-1 flex-col overflow-y-auto bg-chat-screen scrollbar-sleek">
      <div className="flex w-full flex-col gap-5 px-4 pb-6 pt-4">
        <section aria-labelledby="supervisor-home-title" className="rounded-2xl bg-chat-surface-raised px-4 py-3.5">
          <div className="flex items-center gap-2">
            <Eye aria-hidden="true" className="size-4 text-chat-foreground-secondary" />
            <h1 id="supervisor-home-title" className="text-[17px] font-semibold text-chat-foreground">Supervisor</h1>
            <span data-testid="supervisor-home-headline" className={cn("ml-auto text-[13px] font-medium", headline === "typing" ? "text-chat-foreground" : "text-chat-foreground-secondary")}>
              {headline}
            </span>
          </div>
          <p className="mt-1.5 text-[14px] leading-snug text-chat-foreground-secondary">{headlineSentence(headline)}</p>
        </section>

        {unified.map((host) => (
          <UnifiedSupervisorSection
            key={host.machineId}
            surface="phone"
            hostLabel={host.label}
            connection={host.connection}
            connectionError={host.error}
            view={host.view}
            client={host.client}
          />
        ))}

        {machines.length > 0 && machines.every((machine) => machine.state !== "loading") && !anySupervisor ? (
          <p data-testid="supervisor-home-none" className="px-1 text-[13px] leading-snug text-chat-foreground-secondary">
            None of your computers reports a running supervisor right now.
          </p>
        ) : null}

        {machines.length === 0 ? (
          <p data-testid="supervisor-home-empty" className="rounded-xl bg-chat-surface px-3 py-4 text-[14px] leading-snug text-chat-foreground-secondary">
            No computers are linked to this account yet. Open Ferryx on your computer and sign in there.
          </p>
        ) : null}

        {machines.map((machine) => {
          const snapshot = machine.snapshot;
          const acting = snapshot ? actingSessionIds(snapshot, now) : new Set<string>();
          const hubOff = snapshot && (!snapshot.hub.available || (!snapshot.hub.running && snapshot.lanes.length === 0));
          return (
            <section key={machine.machineId} data-testid={`supervisor-machine-${machine.machineId}`} aria-label={machine.label} className="flex flex-col gap-2">
              <h2 className={cn(SECTION_LABEL, "truncate")}>{machine.label}</h2>
              {snapshot ? null : <MachineStateNote machine={machine} />}
              {hubOff ? (
                <p data-testid="supervisor-hub-off" className="rounded-xl bg-chat-surface px-3 py-3 text-[13px] leading-snug text-chat-foreground-secondary">
                  The supervisor is not running on {machine.label}. Start it with <span className="font-mono text-chat-foreground">ferryx hub start</span>.
                </p>
              ) : snapshot && snapshot.lanes.length === 0 ? (
                <p className="rounded-xl bg-chat-surface px-3 py-3 text-[13px] leading-snug text-chat-foreground-secondary">
                  {snapshot.watched.length > 0
                    ? `No lanes yet; watching ${snapshot.watched.length} session${snapshot.watched.length === 1 ? "" : "s"}.`
                    : "No lanes yet. Hand it a goal from the hub and it appears here."}
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


        {readable.length > 0 ? (
          <section aria-labelledby="supervisor-recent-title" className="flex flex-col gap-1">
            <h2 id="supervisor-recent-title" className={SECTION_LABEL}>Recent</h2>
            {recent.length === 0 ? (
              <p className="px-1 text-[13px] text-chat-foreground-secondary">It has not typed anything yet.</p>
            ) : (
              <ol className="flex flex-col">
                {recent.map((action) => (
                  <li
                    key={`${action.machineId}-${action.atMs}-${action.requestId ?? action.op}-${action.sessionId ?? ""}`}
                    data-testid="supervisor-recent-action"
                    className="grid min-w-0 grid-cols-[40px_52px_minmax(0,1fr)] items-baseline gap-2 px-1 py-1.5 text-[13px] text-chat-foreground-secondary"
                  >
                    <span className="font-mono text-[12px] tabular-nums">{clock(action.atMs)}</span>
                    <span className="truncate font-mono text-[12px] text-chat-foreground">{action.code === "CONTROL_CONFLICT" ? "held" : action.op}</span>
                    <span className="min-w-0 truncate">{actionText(action)}</span>
                  </li>
                ))}
              </ol>
            )}
          </section>
        ) : null}
      </div>
    </div>
  );
}
