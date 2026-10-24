// The strip under the Session screen's top bar while the supervisor watches or drives that
// session. Same priority as paneSupervision: paused > left-to-you > held > acting > watching.
import { Eye, Keyboard, Pause, Play } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import type { PaneSupervision } from "../../lib/supervisorModel";
import type { LanePauseResult } from "../remoteSupervisor";
import { relativeTime } from "./phoneHomeModel";

export type SupervisorSessionStripProps = {
  readonly state: PaneSupervision;
  readonly now: number;
  readonly onSetPaused: (laneKey: string, paused: boolean) => Promise<LanePauseResult>;
  /** Hands the keyboard to the person: focuses the chat composer or the terminal input sink. */
  readonly onFocusInput: () => void;
};

const BUTTON =
  "flex h-11 shrink-0 items-center gap-1.5 rounded-lg px-3 text-[13px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

export function SupervisorSessionStrip({ state, now, onSetPaused, onFocusInput }: SupervisorSessionStripProps) {
  const [busy, setBusy] = useState(false);
  const lane = state.lane;

  const setPaused = async (paused: boolean): Promise<boolean> => {
    if (!lane || busy) return false;
    setBusy(true);
    const result = await onSetPaused(lane.key, paused);
    setBusy(false);
    if (result.kind === "error") {
      toast.error(paused ? "Could not pause the supervisor" : "Could not resume the supervisor", {
        description: `${result.code}: ${result.message}`,
      });
      return false;
    }
    return true;
  };

  const takeOver = async () => {
    // Pause first so no keystroke from the hub lands after the person starts typing.
    if (lane) await setPaused(true);
    onFocusInput();
  };

  return (
    <div
      data-testid="supervisor-session-strip"
      data-state={state.kind}
      role="status"
      className={cn(
        "flex min-h-[52px] shrink-0 items-center gap-2 border-b border-chat-border px-3 py-1 text-[13px]",
        state.kind === "acting" ? "bg-accent text-chat-foreground" : "bg-chat-surface text-chat-foreground-secondary",
      )}
    >
      {state.kind === "acting" ? (
        <>
          <Eye aria-hidden="true" className="size-4 shrink-0" />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-[12px] text-chat-foreground-secondary">Supervisor typed · {relativeTime(state.atMs, now)}</span>
            <span className="min-w-0 truncate font-mono text-chat-foreground">“{state.text ?? "input"}”</span>
          </span>
          <button type="button" data-testid="supervisor-take-over" disabled={busy} onClick={() => void takeOver()} className={cn(BUTTON, "bg-chat-surface-raised text-chat-foreground hover:bg-chat-surface-hover")}>
            Take over
          </button>
        </>
      ) : state.kind === "held" ? (
        <>
          <Keyboard aria-hidden="true" className="size-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate">You're typing. The supervisor holds off until 3s after you stop.</span>
        </>
      ) : state.kind === "paused" ? (
        <>
          <Pause aria-hidden="true" className="size-4 shrink-0" />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-chat-foreground">Supervisor paused</span>
            <span className="min-w-0 truncate text-[12px]">Still reads this session, sends nothing</span>
          </span>
          <button type="button" data-testid="supervisor-resume" disabled={busy} onClick={() => void setPaused(false)} className={cn(BUTTON, "bg-chat-surface-raised text-chat-foreground hover:bg-chat-surface-hover")}>
            <Play aria-hidden="true" className="size-3.5" />
            Resume
          </button>
        </>
      ) : state.kind === "left-to-you" ? (
        <>
          <Eye aria-hidden="true" className="size-4 shrink-0 text-chat-foreground" />
          <span className="min-w-0 flex-1 truncate">
            <span className="text-chat-foreground">Left to you.</span> The supervisor would not answer this one.
          </span>
        </>
      ) : (
        <>
          <Eye aria-hidden="true" className="size-4 shrink-0" />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-chat-foreground">Supervised</span>
            <span className="min-w-0 truncate text-[12px]">{lane ? `Goal: ${lane.goal ?? lane.title}` : "Watching this session"}</span>
          </span>
          {lane ? (
            <button type="button" data-testid="supervisor-pause-here" disabled={busy} onClick={() => void setPaused(true)} className={cn(BUTTON, "text-chat-foreground-secondary hover:text-chat-foreground")}>
              Pause here
            </button>
          ) : null}
        </>
      )}
    </div>
  );
}
