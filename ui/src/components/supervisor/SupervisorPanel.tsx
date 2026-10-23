import { Eye, Pause, Play } from "lucide-react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import {
  actingSessionIds,
  setSupervisorLanePaused,
  useSupervisor,
  type SupervisorAction,
  type SupervisorLane,
  type SupervisorSnapshot,
} from "../../lib/supervisor";

function clock(atMs: number): string {
  const date = new Date(atMs);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function laneName(lane: SupervisorLane): string {
  const leaf = lane.cwd.split(/[\\/]/).filter(Boolean).pop();
  return leaf ? `${leaf} · ${lane.title}` : lane.title;
}

function actionLine(action: SupervisorAction, lanes: readonly SupervisorLane[]): string {
  const lane = lanes.find((item) => item.sessionId === action.sessionId);
  const where = lane ? lane.title : (action.sessionId ?? "pane").slice(0, 8);
  if (action.code === "CONTROL_CONFLICT") return `${where} · held, you were typing`;
  if (action.code !== "OK") return `${where} · refused (${action.code.toLowerCase()})`;
  return action.text ? `${where} · “${action.text}”` : where;
}

async function togglePause(lane: SupervisorLane) {
  try {
    await setSupervisorLanePaused(lane.key, !lane.paused);
  } catch (error) {
    toast.error(lane.paused ? "Could not resume" : "Could not pause", {
      description: String((error as { message?: unknown })?.message ?? error),
    });
  }
}

/** The Supervisor sidebar view: what the hub supervisor drives, its pause, and what it typed. */
export function SupervisorPanelView({ snapshot, now }: { snapshot: SupervisorSnapshot; now: number }) {
  const { hub, lanes, watched, actions } = snapshot;
  const acting = actingSessionIds(snapshot, now);
  const allPaused = lanes.length > 0 && lanes.every((lane) => lane.paused);
  const headline = !hub.running
    ? "off"
    : allPaused
      ? "paused"
      : lanes.some((lane) => lane.sessionId && acting.has(lane.sessionId))
        ? "typing"
        : `watching ${lanes.length + watched.length}`;

  if (!hub.available || (!hub.running && lanes.length === 0)) {
    return (
      <div data-testid="supervisor-panel" data-supervisor-state="off" className="flex flex-col gap-2.5 px-4 py-4 text-[11px] leading-relaxed text-muted-foreground">
        <b className="text-[12px] font-semibold text-foreground">{hub.available ? "The supervisor is stopped" : "The supervisor is not running"}</b>
        <span>
          {hub.available
            ? "Nothing is typed into your panes. Start it with ferryx hub start; it watches the lanes it is given and leaves anything risky in your Inbox."
            : hub.error
              ? "The daemon did not report a supervisor, so nothing here can be trusted as watched or driven."
              : "This daemon has no supervisor hub open, so no pane is watched or driven."}
        </span>
        {hub.error ? <span className="font-mono text-[10.5px]">{hub.error}</span> : null}
      </div>
    );
  }

  return (
    <div data-testid="supervisor-panel" data-supervisor-state={headline} className="flex min-h-0 flex-1 flex-col overflow-y-auto scrollbar-sleek">
      <div className="mx-2 mb-1.5 mt-0.5 flex flex-col gap-1.5 rounded-md bg-card p-2">
        <div className="flex items-center gap-1.5 text-[12px] font-semibold text-foreground">
          <Eye aria-hidden className="size-3.5" />
          <span>Supervisor</span>
          <span className={cn("ml-auto text-[10px] font-normal", headline === "typing" ? "text-foreground" : "text-muted-foreground")}>{headline}</span>
        </div>
        <p className="text-[11px] leading-snug text-muted-foreground">
          {allPaused ? (
            <><b className="font-medium text-foreground">Paused.</b> It still reads panes but sends nothing until you resume.</>
          ) : hub.running ? (
            "Acts only in the lanes below. It holds input while you type and for 3s after."
          ) : (
            "Stopped. The lanes below keep their state; nothing is typed."
          )}
        </p>
      </div>

      <div className="flex h-8 shrink-0 items-center justify-between px-3.5 text-[11px] font-medium text-muted-foreground">
        <span>Supervising</span><span>{lanes.length}</span>
      </div>
      {hub.error ? (
        <p data-testid="supervisor-panel-error" role="alert" className="mx-2 mb-1.5 rounded-md bg-destructive/10 px-2 py-1.5 text-[11px] leading-snug text-foreground">
          The supervisor could not read its lanes, so this list may be incomplete.{" "}
          <span className="font-mono text-[10.5px] text-muted-foreground">{hub.error}</span>
        </p>
      ) : null}
      {lanes.length === 0 && !hub.error ? (
        <p className="px-4 pb-2 text-[11px] text-muted-foreground">
          {watched.length > 0 ? `No lanes yet; watching ${watched.length} pane${watched.length === 1 ? "" : "s"}.` : "No lanes yet. Hand it a goal from the hub and it appears here."}
        </p>
      ) : (
        <ul className="flex flex-col gap-0.5 px-2">
          {lanes.map((lane) => {
            const typing = Boolean(lane.sessionId && acting.has(lane.sessionId));
            return (
              <li key={lane.key} data-testid="supervisor-lane" data-lane-key={lane.key} className="rounded-md py-1 pb-1.5 hover:bg-worktree-sidebar-accent">
                <div className="flex h-6 items-center gap-1.5 px-2">
                  <span className="min-w-0 truncate text-[12px] font-semibold text-worktree-sidebar-foreground" title={lane.cwd}>{laneName(lane)}</span>
                  <span className={cn("ml-auto shrink-0 text-[10px]", typing ? "text-foreground" : "text-muted-foreground")}>
                    {lane.paused ? "paused" : typing ? "typing" : lane.status.replace(/_/g, " ")}
                  </span>
                  <button
                    type="button"
                    aria-label={lane.paused ? `Resume ${lane.title}` : `Pause ${lane.title}`}
                    onClick={() => void togglePause(lane)}
                    className="flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-white/[0.08] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {lane.paused ? <Play className="size-3" /> : <Pause className="size-3" />}
                  </button>
                </div>
                <dl className="grid grid-cols-[34px_1fr] gap-x-1.5 gap-y-0.5 px-2 text-[11px] leading-snug">
                  <dt className="pt-px text-[10px] text-muted-foreground">Goal</dt>
                  <dd className="min-w-0 truncate text-foreground/90" title={lane.goal ?? lane.title}>{lane.goal ?? lane.title}</dd>
                </dl>
              </li>
            );
          })}
        </ul>
      )}

      <div className="mt-1 flex h-8 shrink-0 items-center justify-between px-3.5 text-[11px] font-medium text-muted-foreground">
        <span>Recent</span><span className="font-mono text-[10px]">audit</span>
      </div>
      {actions.length === 0 ? (
        <p className="px-4 pb-2 text-[11px] text-muted-foreground">It has not typed anything yet.</p>
      ) : (
        <ol className="pb-2">
          {actions.slice(0, 20).map((action) => (
            <li key={`${action.atMs}-${action.requestId ?? action.op}-${action.sessionId ?? ""}`} data-testid="supervisor-action" className="grid grid-cols-[36px_44px_1fr] items-baseline gap-1.5 px-2 py-[3px] text-[11px] text-muted-foreground">
              <span className="font-mono text-[10px]">{clock(action.atMs)}</span>
              <span className="font-mono text-[10px] text-foreground">{action.code === "CONTROL_CONFLICT" ? "held" : action.op}</span>
              <span className="min-w-0 truncate" title={action.text ?? undefined}>{actionLine(action, lanes)}</span>
            </li>
          ))}
        </ol>
      )}
      <p className="mt-auto px-4 pb-2.5 pt-2 text-[10px] leading-snug text-muted-foreground">
        Pausing a lane stops its input at once. Take over on a pane bar pauses that lane and hands you the keyboard.
      </p>
    </div>
  );
}

export function SupervisorPanel() {
  const { snapshot, now } = useSupervisor();
  return <SupervisorPanelView snapshot={snapshot} now={now} />;
}

/** Segment-label indicator: true while the supervisor typed somewhere moments ago. */
export function useSupervisorTyping(): boolean {
  const { snapshot, now } = useSupervisor();
  return actingSessionIds(snapshot, now).size > 0;
}
