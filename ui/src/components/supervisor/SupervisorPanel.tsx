import { Eye, Pause, Play } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import {
  actingSessionIds,
  adoptArchivedGoals,
  desktopUnifiedClient,
  moveSupervisorHere,
  setSupervisorLanePaused,
  supervisorGoal,
  supervisorStopReasonLabel,
  useSupervisor,
  type SupervisorAction,
  type SupervisorGoalMode,
  type SupervisorGoalRequest,
  type SupervisorGoalView,
  type SupervisorLane,
  type SupervisorRequirementStatus,
  type SupervisorSnapshot,
} from "../../lib/supervisor";
import { isStandbyRole, leaseNotices, type SupervisorHubState } from "../../lib/supervisorModel";
import {
  forceMoveWarning,
  LANE_KEY_PREFIX,
  type SupervisorCallResult,
  type SupervisorConnection,
  type SupervisorMoveResult,
  type UnifiedSupervisorClient,
  type UnifiedSupervisorView,
} from "../../lib/unifiedSupervisor";
import { SegmentedControl } from "../ui/SegmentedControl";
import { UnifiedSupervisorSection, type AdoptableLane } from "./UnifiedSupervisorSection";

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

const GOAL_MODES: ReadonlyArray<{ value: SupervisorGoalMode; label: string }> = [
  { value: "off", label: "Off" },
  { value: "supervised", label: "Supervised" },
  { value: "full", label: "Full" },
];

const CHIP_TONE: Record<SupervisorRequirementStatus, string> = {
  met: "bg-accent text-foreground",
  unmet: "bg-destructive/10 text-foreground",
  pending: "bg-white/[0.06] text-muted-foreground",
  waived: "bg-white/[0.04] text-muted-foreground line-through",
};

function GoalButton({ children, onClick, disabled, ghost, label }: { children: React.ReactNode; onClick: () => void; disabled: boolean; ghost?: boolean; label?: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "inline-flex h-5 shrink-0 items-center rounded-[5px] px-2 text-[10.5px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
        ghost ? "text-muted-foreground hover:text-foreground" : "bg-card text-foreground hover:bg-black/40",
      )}
    >
      {children}
    </button>
  );
}

/** One lane's goal contract: requirements, turn budget, mode, the proposed next step and owner revisions. */
function LaneGoal({ lane, goal }: { lane: SupervisorLane; goal: SupervisorGoalView }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (request: SupervisorGoalRequest, failure: string) => {
    setBusy(true);
    setError(null);
    try {
      await supervisorGoal(request);
    } catch (caught) {
      const message = String((caught as { message?: unknown })?.message ?? caught);
      setError(message);
      toast.error(failure, { description: message });
    } finally {
      setBusy(false);
    }
  };
  const awaiting = goal.proposal?.state === "awaiting_approval" ? goal.proposal : null;
  return (
    <div data-testid="supervisor-lane-goal" className="flex flex-col gap-1 px-2 pt-1 text-[11px] leading-snug">
      <div className="flex items-center gap-1.5 text-[10px] text-muted-foreground">
        <span>Requirements</span>
        <span data-testid="supervisor-goal-turns" className="ml-auto font-mono">{goal.turnsUsed} / {goal.turnLimit} turns</span>
      </div>
      {goal.requirements.length > 0 ? (
        <ul className="flex flex-col gap-0.5">
          {goal.requirements.map((requirement) => (
            <li key={requirement.id} data-testid="supervisor-requirement" className="flex items-baseline gap-1.5">
              <span data-testid="supervisor-requirement-status" data-status={requirement.status} className={cn("shrink-0 rounded-[4px] px-1 text-[10px]", CHIP_TONE[requirement.status] ?? CHIP_TONE.pending)}>
                {requirement.status}
              </span>
              <span className="min-w-0 truncate text-foreground/90" title={requirement.verifier ? `${requirement.text} (${requirement.verifier})` : requirement.text}>{requirement.text}</span>
            </li>
          ))}
        </ul>
      ) : (
        <span className="text-muted-foreground">No requirements yet.</span>
      )}
      {goal.stopReason ? (
        <span data-testid="supervisor-goal-stop" className="text-foreground">{supervisorStopReasonLabel(goal.stopReason)}</span>
      ) : null}
      <SegmentedControl
        label={`Supervision mode for ${lane.title}`}
        value={goal.mode}
        options={GOAL_MODES}
        onChange={(mode) => {
          if (busy || mode === goal.mode) return;
          void run({ action: "mode", laneKey: lane.key, mode }, "Could not change the supervision mode");
        }}
      />
      {awaiting ? (
        <div data-testid="supervisor-goal-proposal" className="flex flex-col gap-1 rounded-md bg-card p-1.5">
          <span className="text-[10px] text-muted-foreground">Next step ready</span>
          <q className="min-w-0 break-words font-mono text-foreground">{awaiting.text}</q>
          <div className="flex gap-1">
            <GoalButton disabled={busy} onClick={() => void run({ action: "approve", laneKey: lane.key }, "Could not approve the next step")}>Approve</GoalButton>
            <GoalButton ghost disabled={busy} onClick={() => void run({ action: "discard", laneKey: lane.key }, "Could not discard the next step")}>Discard</GoalButton>
          </div>
        </div>
      ) : null}
      {goal.pendingRevisions.map((revision) => (
        <div key={revision.id} data-testid="supervisor-goal-revision" className="flex flex-col gap-1 rounded-md bg-card p-1.5">
          <span className="text-[10px] text-muted-foreground">Goal change from your words</span>
          <q className="min-w-0 break-words text-foreground">{revision.instructionQuote}</q>
          <div className="flex gap-1">
            <GoalButton disabled={busy} onClick={() => void run({ action: "acceptRevision", id: revision.id }, "Could not accept the goal change")}>Accept</GoalButton>
            <GoalButton ghost disabled={busy} onClick={() => void run({ action: "rejectRevision", id: revision.id }, "Could not reject the goal change")}>Reject</GoalButton>
          </div>
        </div>
      ))}
      {error ? <span data-testid="supervisor-goal-error" role="alert" className="font-mono text-[10.5px] text-destructive">{error}</span> : null}
    </div>
  );
}

/**
 * The desktop's own daemon is the supervisor host it reaches. With no hub open there it says so
 * and keeps the last view it saw; it never starts another supervisor. A daemon that does not hold
 * the account supervisor lease is on standby.
 */
export function desktopConnection(snapshot: SupervisorSnapshot): SupervisorConnection {
  if (isStandbyRole(snapshot.hub.role)) return "standby";
  if (snapshot.supervisor) return "live";
  return snapshot.hub.error ? "disconnected" : "unavailable";
}

/** Lease warnings of this computer: unreachable hosts, dropped observations, a blocked takeover. */
function LeaseNotices({ hub, className }: { hub: SupervisorHubState; className?: string }) {
  const notices = leaseNotices(hub, "This computer");
  if (notices.length === 0) return null;
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      {notices.map((notice) => (
        <p key={notice.kind} data-testid="supervisor-lease-notice" data-kind={notice.kind} role="status" className="rounded-md bg-destructive/10 px-2 py-1.5 text-[11px] leading-snug text-foreground">
          {notice.text}
        </p>
      ))}
    </div>
  );
}

/**
 * This computer on standby: where the account supervisor runs, "Move supervisor here" (a forced
 * move shows the data-loss window and needs a second confirm), pairing and host-mapping notices,
 * and the open goals of this computer's archived supervisor with Adopt.
 */
function DesktopStandby({
  snapshot,
  moveHere,
  adoptArchived,
}: {
  snapshot: SupervisorSnapshot;
  moveHere: (force: boolean) => Promise<SupervisorMoveResult>;
  adoptArchived: (goalIds: readonly string[]) => Promise<SupervisorCallResult>;
}) {
  const { hub } = snapshot;
  const holder = hub.holderLabel || hub.holderMachineId || "another computer";
  const [moving, setMoving] = useState(false);
  const [forceWarning, setForceWarning] = useState<string | null>(null);
  const [adopting, setAdopting] = useState<ReadonlySet<string>>(() => new Set());

  const move = async (force: boolean) => {
    if (moving) return;
    setMoving(true);
    const result = await moveHere(force);
    setMoving(false);
    if (result.kind === "force-required") {
      setForceWarning(forceMoveWarning(result, holder, clock));
      return;
    }
    setForceWarning(null);
    if (result.kind === "error") toast.error("Could not move the supervisor", { description: result.message });
  };

  const adopt = async (goalId: string) => {
    if (adopting.has(goalId)) return;
    setAdopting((prev) => new Set(prev).add(goalId));
    const result = await adoptArchived([goalId]);
    setAdopting((prev) => {
      const copy = new Set(prev);
      copy.delete(goalId);
      return copy;
    });
    if (result.kind === "error") toast.error("Could not adopt the goal", { description: result.message });
  };

  const adoptable = hub.adoptable ?? [];
  const unmapped = hub.unmappedHosts ?? [];
  return (
    <div data-testid="supervisor-standby" className="flex flex-col gap-2 text-[11px] leading-snug">
      <p data-testid="supervisor-standby-text" role="status" className="text-foreground">
        Supervisor runs on {holder}. This computer is on standby.
      </p>
      {hub.available && forceWarning === null ? (
        <GoalButton disabled={moving} onClick={() => void move(false)} label="Move supervisor here">
          {moving ? "Moving…" : "Move supervisor here"}
        </GoalButton>
      ) : null}
      {forceWarning !== null ? (
        <div data-testid="supervisor-move-force" role="alert" className="flex flex-col gap-1 rounded-md bg-destructive/10 p-1.5 text-foreground">
          <span data-testid="supervisor-move-force-warning">{forceWarning}</span>
          <div className="flex gap-1">
            <GoalButton disabled={moving} onClick={() => void move(true)}>Move anyway</GoalButton>
            <GoalButton ghost disabled={moving} onClick={() => setForceWarning(null)}>Cancel</GoalButton>
          </div>
        </div>
      ) : null}
      {hub.holderPaired === false ? (
        <span data-testid="supervisor-standby-unpaired">
          This computer is not paired to {holder}, so its panes are not supervised yet.
        </span>
      ) : null}
      <LeaseNotices hub={hub} />
      {unmapped.length > 0 ? (
        <span data-testid="supervisor-standby-unmapped">
          Some goals name a computer this one does not know ({unmapped.join(", ")}). They stay held until it is paired.
        </span>
      ) : null}
      {adoptable.length > 0 ? (
        <ul data-testid="supervisor-adoptable" className="flex flex-col gap-0.5">
          {adoptable.map((goal) => (
            <li key={goal.goalId} data-testid="supervisor-adoptable-goal" data-goal-id={goal.goalId} className="flex min-w-0 items-center gap-1.5">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-foreground">{goal.title}</span>
                <span className="block truncate text-[10px]">Adoptable from {goal.fromMachine || "this computer"}</span>
              </span>
              <GoalButton disabled={adopting.has(goal.goalId)} onClick={() => void adopt(goal.goalId)} label={`Adopt ${goal.title}`}>
                Adopt
              </GoalButton>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

/**
 * Legacy lanes that may be adopted: not a supervisor lane, and a goal contract that is on and has
 * requirements (adoption keeps the legacy terms; an `off` loop is refused LEGACY_GOAL_OFF).
 */
export function adoptableLanes(snapshot: SupervisorSnapshot, unified: UnifiedSupervisorView | null): AdoptableLane[] {
  if (!unified) return [];
  const owned = new Set(unified.goals.map((goal) => goal.laneKey));
  return snapshot.lanes
    .filter((lane) => !lane.key.startsWith(LANE_KEY_PREFIX) && !owned.has(lane.key)
      && lane.goalContract != null && lane.goalContract.mode !== "off" && lane.goalContract.requirements.length > 0)
    .map((lane) => ({ key: lane.key, title: lane.title }));
}

function UnifiedForDesktop({ snapshot, lastUnified, client }: { snapshot: SupervisorSnapshot; lastUnified: UnifiedSupervisorView | null; client: UnifiedSupervisorClient }) {
  const view = snapshot.supervisor ?? lastUnified;
  if (!view) return null;
  const connection = desktopConnection(snapshot);
  return (
    <div className="mx-2 mb-1.5">
      <UnifiedSupervisorSection
        surface="desktop"
        hostLabel="this computer"
        connection={connection}
        connectionError={connection === "live" ? null : { code: connection === "disconnected" ? "DAEMON_UNREACHABLE" : "SUPERVISOR_UNAVAILABLE", message: snapshot.hub.error ?? "" }}
        view={view}
        client={connection === "live" ? client : null}
        adoptable={connection === "live" ? adoptableLanes(snapshot, view) : []}
      />
    </div>
  );
}

/** The Supervisor sidebar view: what the hub supervisor drives, its pause, and what it typed. */
export function SupervisorPanelView({
  snapshot,
  now,
  lastUnified = null,
  client = desktopUnifiedClient,
  moveHere = moveSupervisorHere,
  adoptArchived = adoptArchivedGoals,
}: {
  snapshot: SupervisorSnapshot;
  now: number;
  lastUnified?: UnifiedSupervisorView | null;
  client?: UnifiedSupervisorClient;
  moveHere?: (force: boolean) => Promise<SupervisorMoveResult>;
  adoptArchived?: (goalIds: readonly string[]) => Promise<SupervisorCallResult>;
}) {
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

  if (desktopConnection(snapshot) === "standby") {
    return (
      <div data-testid="supervisor-panel" data-supervisor-state="standby" className="flex flex-col gap-2.5 px-4 py-4 text-[11px] leading-relaxed text-muted-foreground">
        <b className="text-[12px] font-semibold text-foreground">Supervisor on standby</b>
        <DesktopStandby snapshot={snapshot} moveHere={moveHere} adoptArchived={adoptArchived} />
      </div>
    );
  }

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
        <div className="-mx-2">
          <UnifiedForDesktop snapshot={snapshot} lastUnified={lastUnified} client={client} />
        </div>
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

      <UnifiedForDesktop snapshot={snapshot} lastUnified={lastUnified} client={client} />

      <div className="flex h-8 shrink-0 items-center justify-between px-3.5 text-[11px] font-medium text-muted-foreground">
        <span>Supervising</span><span>{lanes.length}</span>
      </div>
      {hub.error ? (
        <p data-testid="supervisor-panel-error" role="alert" className="mx-2 mb-1.5 rounded-md bg-destructive/10 px-2 py-1.5 text-[11px] leading-snug text-foreground">
          The supervisor could not read its lanes, so this list may be incomplete.{" "}
          <span className="font-mono text-[10.5px] text-muted-foreground">{hub.error}</span>
        </p>
      ) : null}
      <LeaseNotices hub={hub} className="mx-2 mb-1.5" />
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
                {lane.goalContract ? <LaneGoal lane={lane} goal={lane.goalContract} /> : null}
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
  const { snapshot, now, lastUnified } = useSupervisor();
  return <SupervisorPanelView snapshot={snapshot} now={now} lastUnified={lastUnified} />;
}

/** Segment-label indicator: true while the supervisor typed somewhere moments ago. */
export function useSupervisorTyping(): boolean {
  const { snapshot, now } = useSupervisor();
  return actingSessionIds(snapshot, now).size > 0;
}
