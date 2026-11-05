import { Check, Eye, Keyboard, Pause, Play, X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import {
  setSupervisorLanePaused,
  supervisorGoal,
  supervisorStopReasonLabel,
  type PaneSupervision,
  type SupervisorLane,
} from "../../lib/supervisor";

function ago(atMs: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - atMs) / 1000));
  return seconds < 60 ? `${seconds}s ago` : `${Math.round(seconds / 60)}m ago`;
}

async function setPaused(laneKey: string, paused: boolean) {
  try {
    await setSupervisorLanePaused(laneKey, paused);
  } catch (error) {
    toast.error(paused ? "Could not pause the supervisor" : "Could not resume the supervisor", {
      description: error instanceof Error ? error.message : String((error as { message?: unknown })?.message ?? error),
    });
  }
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String((error as { message?: unknown })?.message ?? error);
}

function BarButton({ children, onClick, ghost, disabled }: { children: React.ReactNode; onClick: () => void; ghost?: boolean; disabled?: boolean }) {
  return (
    <button
      type="button"
      disabled={disabled}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
      className={cn(
        "inline-flex h-5 shrink-0 items-center gap-1 rounded-[5px] px-2 text-[10.5px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
        ghost ? "text-muted-foreground hover:text-foreground" : "bg-card text-foreground hover:bg-black/40",
      )}
    >
      {children}
    </button>
  );
}

/** Height of the bar; the pane toolbar reveals below it so neither covers the other. */
export const SUPERVISOR_PANE_BAR_PX = 26;

/** Approve or discard the supervisor's proposed next step; buttons stay disabled while the call runs. */
function ApprovalButtons({ lane }: { lane: SupervisorLane }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const decide = async (action: "approve" | "discard") => {
    setBusy(true);
    setError(null);
    try {
      await supervisorGoal({ action, laneKey: lane.key });
    } catch (caught) {
      const message = errorText(caught);
      setError(message);
      toast.error(action === "approve" ? "Could not approve the next step" : "Could not discard the next step", { description: message });
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      {error ? <span data-testid="supervisor-pane-bar-error" role="alert" className="shrink-0 text-destructive" title={error}>Failed</span> : null}
      <BarButton disabled={busy} onClick={() => void decide("approve")}><Check aria-hidden className="size-3" />Approve</BarButton>
      <BarButton ghost disabled={busy} onClick={() => void decide("discard")}><X aria-hidden className="size-3" />Discard</BarButton>
    </>
  );
}

/** The bar a supervised pane carries: quiet while watching, filled while the supervisor types. */
export function SupervisorPaneBarView({
  state,
  now,
  onTakeOver,
}: {
  state: PaneSupervision;
  now: number;
  onTakeOver?: () => void;
}) {
  const lane = state.lane;
  const acting = state.kind === "acting";
  const stopReason = lane?.goalContract?.stopReason ?? null;
  return (
    <div
      data-testid="supervisor-pane-bar"
      data-supervision={state.kind}
      role="status"
      className={cn(
        "flex h-[26px] shrink-0 items-center gap-2 overflow-hidden whitespace-nowrap pl-2.5 pr-1.5 text-[11px]",
        acting ? "bg-accent text-foreground" : "bg-card text-muted-foreground",
      )}
    >
      {state.kind === "acting" ? (
        <>
          <span className="flex shrink-0 items-center gap-1.5"><Eye aria-hidden className="size-3" />Supervisor typed</span>
          <span className="min-w-0 flex-1 truncate font-mono text-foreground" title={state.text ?? undefined}>{state.text ?? "input"}</span>
          <span className="shrink-0">{ago(state.atMs, now)}</span>
          <BarButton onClick={() => {
            if (lane) void setPaused(lane.key, true);
            onTakeOver?.();
          }}>Take over</BarButton>
        </>
      ) : state.kind === "held" ? (
        <>
          <span className="flex shrink-0 items-center gap-1.5"><Keyboard aria-hidden className="size-3" />You're typing</span>
          <span className="min-w-0 flex-1 truncate">Supervisor holds off until 3s after you stop</span>
        </>
      ) : state.kind === "paused" ? (
        <>
          <span className="flex shrink-0 items-center gap-1.5"><Pause aria-hidden className="size-3" />Supervisor paused</span>
          <span className="min-w-0 flex-1 truncate">Still reads this pane, sends nothing</span>
          <BarButton onClick={() => void setPaused(state.lane.key, false)}><Play aria-hidden className="size-3" />Resume</BarButton>
        </>
      ) : state.kind === "awaiting-approval" ? (
        <>
          <span className="flex shrink-0 items-center gap-1.5 text-foreground"><Eye aria-hidden className="size-3" />Next step ready</span>
          <span className="min-w-0 flex-1 truncate font-mono" title={state.lane.goalContract?.proposal?.text ?? undefined}>
            {state.lane.goalContract?.proposal?.text ?? ""}
          </span>
          <ApprovalButtons lane={state.lane} />
        </>
      ) : state.kind === "left-to-you" && stopReason ? (
        <>
          <span className="flex shrink-0 items-center gap-1.5 text-foreground"><Eye aria-hidden className="size-3" />Left to you</span>
          <span data-testid="supervisor-stop-reason" data-stop-reason={stopReason} className="min-w-0 flex-1 truncate">{supervisorStopReasonLabel(stopReason)}</span>
        </>
      ) : state.kind === "left-to-you" ? (
        <>
          <span className="flex shrink-0 items-center gap-1.5 text-foreground"><Eye aria-hidden className="size-3" />Left to you</span>
          <span className="min-w-0 flex-1 truncate">The supervisor would not answer this one; it is in your Inbox</span>
        </>
      ) : (
        <>
          <span className="flex shrink-0 items-center gap-1.5"><Eye aria-hidden className="size-3" />Supervised</span>
          <span className="min-w-0 flex-1 truncate" title={lane?.goal ?? lane?.title}>
            {lane ? `Goal: ${lane.goal ?? lane.title}` : "Watching this pane"}
          </span>
          {lane ? <BarButton ghost onClick={() => void setPaused(lane.key, true)}>Pause here</BarButton> : null}
        </>
      )}
    </div>
  );
}


