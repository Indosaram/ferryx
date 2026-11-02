// The strip under the Session screen's top bar while the supervisor watches or drives that
// session. Same priority as paneSupervision: paused > left-to-you > held > acting > watching.
import { Bot, Eye, Hand, Keyboard, Pause, Play } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import type { PaneSupervision } from "../../lib/supervisorModel";
import type { LanePauseResult } from "../remoteSupervisor";
import { userFacingError } from "../userFacingError";
import { relativeTime } from "./phoneHomeModel";

export type SupervisorSessionStripProps = {
  readonly state: PaneSupervision;
  readonly now: number;
  readonly onSetPaused: (laneKey: string, paused: boolean) => Promise<LanePauseResult>;
  /** Hands the keyboard to the person: focuses the chat composer or the terminal input sink. */
  readonly onFocusInput: () => void;
};

const BUTTON =
  "flex h-11 shrink-0 items-center gap-1.5 rounded-lg bg-chat-surface-raised px-3 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

/** The one short sentence screen readers hear when the strip's state changes. */
function announcement(state: PaneSupervision): string {
  switch (state.kind) {
    case "acting":
      return "Supervisor is typing in this session";
    case "held":
      return "Supervisor is holding off while you type";
    case "paused":
      return "Supervisor paused";
    case "awaiting-approval":
      return "Supervisor is waiting for your approval";
    case "left-to-you":
      return "Supervisor left this one for you";
    case "watching":
      return "Supervisor is watching this session";
    default: {
      const unreachable: never = state;
      return unreachable;
    }
  }
}

export function SupervisorSessionStrip({ state, now, onSetPaused, onFocusInput }: SupervisorSessionStripProps) {
  const [busy, setBusy] = useState(false);
  const [takeOverFailed, setTakeOverFailed] = useState(false);
  const lane = state.lane;

  const setPaused = async (paused: boolean): Promise<boolean> => {
    if (!lane || busy) return false;
    setBusy(true);
    const result = await onSetPaused(lane.key, paused);
    setBusy(false);
    if (result.kind === "error") {
      toast.error(userFacingError(result, { action: paused ? "pause the supervisor" : "resume the supervisor" }));
      return false;
    }
    return true;
  };

  const takeOver = async () => {
    // Pause first so no keystroke from the hub lands after the person starts typing; a refused
    // pause keeps the keyboard where it is, because the supervisor may still be sending.
    if (lane && !(await setPaused(true))) {
      setTakeOverFailed(true);
      return;
    }
    setTakeOverFailed(false);
    onFocusInput();
  };

  return (
    <div
      data-testid="supervisor-session-strip"
      data-state={state.kind}
      className={cn(
        "flex min-h-14 shrink-0 items-center gap-2 border-b border-chat-border px-3 py-1 text-sm",
        state.kind === "acting" ? "bg-accent text-chat-foreground" : "bg-chat-surface text-chat-foreground-secondary",
      )}
    >
      <span role="status" className="sr-only">{announcement(state)}</span>
      {state.kind === "acting" ? (
        <>
          <Bot aria-hidden="true" className="size-4 shrink-0" />
          <span className="flex min-w-0 flex-1 flex-col">
            {/* Primary ink: the acting strip sits on the light accent, where secondary grey drops to 4:1. */}
            <span className="text-xs text-chat-foreground">
              {takeOverFailed ? "Couldn't pause the supervisor. Try again." : `Supervisor typed ${relativeTime(state.atMs, now)}`}
            </span>
            <span className="min-w-0 truncate font-mono text-chat-foreground">{state.text ?? "Typed into this session"}</span>
          </span>
          <button type="button" data-testid="supervisor-take-over" disabled={busy} onClick={() => void takeOver()} className={BUTTON}>
            {takeOverFailed ? "Try again" : "Take over"}
          </button>
        </>
      ) : state.kind === "held" ? (
        <>
          <Keyboard aria-hidden="true" className="size-4 shrink-0" />
          <span className="min-w-0 flex-1 line-clamp-2">You're typing. The supervisor waits until you stop.</span>
        </>
      ) : state.kind === "paused" ? (
        <>
          <Pause aria-hidden="true" className="size-4 shrink-0" />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-chat-foreground">Supervisor paused</span>
            <span className="min-w-0 truncate text-xs">Still reads this session, sends nothing</span>
          </span>
          <button type="button" data-testid="supervisor-resume" disabled={busy} onClick={() => void setPaused(false)} className={BUTTON}>
            <Play aria-hidden="true" className="size-4" />
            Resume
          </button>
        </>
      ) : state.kind === "left-to-you" ? (
        <>
          <Hand aria-hidden="true" className="size-4 shrink-0 text-chat-foreground" />
          <span className="min-w-0 flex-1 line-clamp-2">
            <span className="text-chat-foreground">The supervisor left this one for you.</span> Answer it below.
          </span>
          <button type="button" data-testid="supervisor-answer" onClick={onFocusInput} className={BUTTON}>
            Answer
          </button>
        </>
      ) : (
        <>
          <Eye aria-hidden="true" className="size-4 shrink-0" />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-chat-foreground">Supervised</span>
            <span className="min-w-0 truncate text-xs">{lane ? `Goal: ${lane.goal ?? lane.title}` : "Watching this session"}</span>
          </span>
          {lane ? (
            <button type="button" data-testid="supervisor-pause-here" disabled={busy} onClick={() => void setPaused(true)} className={BUTTON}>
              Pause supervisor
            </button>
          ) : null}
        </>
      )}
    </div>
  );
}
