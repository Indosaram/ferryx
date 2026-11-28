// Maho agent control surfaces: the Agent Tasks section, the tab borrow chip, the
// top controlled banner and the human handoff help bar.
//
// One hook (`useBrowserAgentActivity`) owns the data: it seeds from the backend's
// agent overview, follows the approval change event and the agent's help request,
// and reads which tabs the store marks `agentControlled`. Every surface below is
// presentational over that snapshot, so the sidebar slots (`controlActivity`,
// `agentSection`) and the browser canvas all render the same truth.
//
// The activity model mirrors Maho's `MahoControlActivityIndicatorModel`:
//
//   - a lifecycle status per task (`queued` -> `running` -> `waiting` -> `done` /
//     `failed`), with the last receipt summary the agent reported;
//   - the reading/acting distinction, because "reading this tab" must never look
//     like the agent is about to type;
//   - the control plane (`CLI` / `MCP`) the agent holds its grant through;
//   - cross-surface attribution: a controlled background tab, or a tab in another
//     window, says so instead of claiming to act on the tab in front of the user;
//   - the disconnected and error states, which keep the kill switch but drop the
//     "acting" claim.
//
// IPC failures arrive as structured `{ code, message, details }`; they are read
// through `extractBrowserErrorCode`, never by matching message text.

import {
  ChevronDown,
  ChevronRight,
  CircleCheck,
  CircleX,
  Clock,
  Glasses,
  Hammer,
  Hand,
  Pause,
  ShieldQuestion,
  ShieldX,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { extractBrowserErrorCode } from "../../../lib/browserTauri";
import {
  controllerName,
  getBrowserAgentOverview,
  onBrowserAgentApprovals,
  onBrowserAgentHelpRequest,
  reclaimBrowserAgent,
  resolveBrowserAgentApproval,
  revokeBrowserAgent,
  type ApprovalResolution,
  type BrowserAgentApproval,
  type BrowserAgentController,
  type HandoffRequestPayload,
} from "../../../lib/browserAgent";
import {
  browserStore,
  useBrowserStore,
  type BrowserStore,
  type BrowserStoreState,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { Button } from "../../ui/button";

/**
 * Where a task sits in its life, matching `AgentTaskStatus` in Maho's
 * `maho_sidebar_agent_tasks_view.cc`: a queued task has not started, `waiting` needs the human,
 * `done`/`failed` are terminal receipts that stay visible until dismissed.
 */
export const MAHO_AGENT_TASK_STATUSES = ["queued", "running", "waiting", "done", "failed"] as const;
export type MahoAgentTaskStatus = (typeof MAHO_AGENT_TASK_STATUSES)[number];

/** The plane a controller drives the browser through; `embedded` is the built-in agent. */
export type MahoControlPlane = "embedded" | "cli" | "mcp";

/**
 * The indicator state, matching `MahoControlActivityState`. `reading` and `acting` are the two
 * live states; `waitingApproval`/`paused` still hold a grant, so their action is Revoke.
 */
export type MahoControlActivityState =
  | "idle"
  | "reading"
  | "acting"
  | "waitingApproval"
  | "paused"
  | "disconnected"
  | "error";

/** A tab an agent currently controls, named by its store title and controller. */
export type BrowserAgentTask = {
  browserId: string;
  title: string;
  controllerId: string | null;
  controllerLabel: string | null;
  status: MahoAgentTaskStatus;
  plane: MahoControlPlane;
  /** One line of what the agent last finished on this tab, shown under the row. */
  receiptSummary: string | null;
  /** True while the controlled tab is not the tab the user is looking at. */
  isBackgroundTab: boolean;
};

/** The agent asked the human for help on a tab; the help bar waits for an answer. */
export type HandoffRequest = HandoffRequestPayload & { controllerLabel: string | null };

export type BrowserAgentActivity = {
  activeTasks: BrowserAgentTask[];
  pendingApprovals: BrowserAgentApproval[];
  controlledBrowserIds: string[];
  handoffRequests: HandoffRequest[];
  /** Agent-controlled tabs plus handoffs waiting on the human. */
  badgeCount: number;
  errorCode: string | null;
  busy: boolean;
  controllers: BrowserAgentController[];
  /** Controller label for a tab, from the caller's map or the single active controller. */
  controllerLabelFor: (browserId: string) => string | null;
  /** Controller id for a tab, so Revoke can address it; null when it cannot be attributed. */
  controllerIdFor: (browserId: string) => string | null;
  /** Lifecycle status of a tab's task; `running` unless the host reported otherwise. */
  taskStatusFor: (browserId: string) => MahoAgentTaskStatus;
  /** The plane the controller drives this tab through. */
  planeFor: (browserId: string) => MahoControlPlane;
  /** The last receipt summary for a tab, or null while the agent reported none. */
  receiptFor: (browserId: string) => string | null;
  /** The combined indicator state for one tab: handoff and approvals outrank the live verb. */
  stateFor: (browserId: string) => MahoControlActivityState;
  takeOver: (browserId: string) => Promise<void>;
  doneHandoff: (browserId: string) => Promise<void>;
  /**
   * Answers one handoff by its request id (falling back to the tab id for a payload that
   * carries none): `complete` hands the lease back to the agent, `cancel` keeps it reclaimed.
   */
  respondToHelp: (requestId: string, outcome: "complete" | "cancel") => Promise<void>;
  revokeController: (controllerId: string) => Promise<void>;
  resolveApproval: (approvalId: string, resolution: ApprovalResolution) => Promise<void>;
};

/** What a host knows about one controlled tab that the store does not carry. */
export type MahoAgentTaskHint = {
  status?: MahoAgentTaskStatus;
  plane?: MahoControlPlane;
  /** Last receipt summary, the one line the Agent Tasks row shows under the title. */
  receipt?: string | null;
  /** The live verb: reading the page is not acting on it. */
  verb?: "reading" | "acting";
};

export type BrowserAgentActivityOptions = {
  store?: BrowserStore;
  /** Per-tab controller names, for a caller that tracks which controller leases which tab. */
  controllerLabels?: Record<string, string>;
  /** Per-tab controller ids, the companion of `controllerLabels`. */
  controllerIds?: Record<string, string>;
  /** Per-tab lifecycle/plane/receipt hints, from the surface that owns the agent session. */
  taskHints?: Record<string, MahoAgentTaskHint>;
  /** The tab the user is looking at, for background-tab attribution. */
  visibleBrowserId?: string | null;
};

/**
 * A subscription that may be unavailable. `listen` rejects when the host exposes no event
 * bridge (a web or remote build), and it does so asynchronously, so the rejection has to be
 * swallowed here: this hook is mounted by the shell in every mode, and a missing bridge has to
 * mean "no live events", not an unhandled rejection on every mount.
 */
function subscribeOrNothing(subscribe: () => Promise<UnlistenFn>): Promise<UnlistenFn> {
  try {
    return subscribe().catch(() => () => undefined);
  } catch {
    return Promise.resolve(() => undefined);
  }
}

function selectAgentControlledTabs(state: BrowserStoreState): BrowserStoreTab[] {
  return Object.values(state.tabs)
    .filter((tab) => tab.agentControlled === true)
    .sort((left, right) => left.browserId.localeCompare(right.browserId));
}

function tabTitle(tab: BrowserStoreTab): string {
  return tab.title.trim() || tab.url;
}

/** The status phrase a row's accessible name and label start with. */
export function agentTaskStatusText(status: MahoAgentTaskStatus): string {
  switch (status) {
    case "queued":
      return "Queued";
    case "running":
      return "Running";
    case "waiting":
      return "Needs you";
    case "done":
      return "Done";
    case "failed":
      return "Failed";
  }
}

/** The short plane tag the chip shows next to `AI`; the embedded agent shows none. */
export function planeLabel(plane: MahoControlPlane): string {
  switch (plane) {
    case "embedded":
      return "";
    case "cli":
      return "CLI";
    case "mcp":
      return "MCP";
  }
}

/**
 * The state phrase, in the order the reference resolves it: cross-surface attribution first
 * (a controlled background tab must not claim "this tab"), then the specific verb.
 */
export function controlStatePhrase(
  state: MahoControlActivityState,
  attribution: { background?: boolean; otherWindow?: boolean } = {},
): string {
  if (state === "reading" || state === "acting") {
    if (attribution.otherWindow) return "Working in another Maho window";
    if (attribution.background) return "Working in another tab";
  }
  switch (state) {
    case "idle":
      return "";
    case "reading":
      return "Reading this tab";
    case "acting":
      return "Acting on this tab";
    case "waitingApproval":
      return "Waiting for approval";
    case "paused":
      return "Paused";
    case "disconnected":
      return "Disconnected";
    case "error":
      return "Action failed";
  }
}

/** The visible status line: `AI - <plane> - <phrase>`, with the plane omitted when embedded. */
export function controlStatusText(
  state: MahoControlActivityState,
  plane: MahoControlPlane,
  attribution: { background?: boolean; otherWindow?: boolean } = {},
): string {
  const phrase = controlStatePhrase(state, attribution);
  if (!phrase) return "";
  const tag = planeLabel(plane);
  return tag ? `AI - ${tag} - ${phrase}` : `AI - ${phrase}`;
}

/** A stopped/blocked controller withdraws authority (Revoke); a live one is interrupted (Stop). */
export function stopLabelForState(state: MahoControlActivityState): "Stop" | "Revoke" {
  return state === "paused" || state === "waitingApproval" ? "Revoke" : "Stop";
}

/** A halted or finished state has nothing left to interrupt. */
export function stateAllowsStop(state: MahoControlActivityState): boolean {
  return state === "reading" || state === "acting" || state === "waitingApproval" || state === "paused";
}

/** The glyph for a state, matching `IconForState` in the reference view. */
function stateIcon(state: MahoControlActivityState) {
  switch (state) {
    case "idle":
    case "reading":
      return Glasses;
    case "acting":
      return Hammer;
    case "waitingApproval":
      return ShieldQuestion;
    case "paused":
      return Pause;
    case "disconnected":
    case "error":
      return ShieldX;
  }
}

function statusIcon(status: MahoAgentTaskStatus) {
  switch (status) {
    case "queued":
      return Clock;
    case "running":
      return Hammer;
    case "waiting":
      return Pause;
    case "done":
      return CircleCheck;
    case "failed":
      return CircleX;
  }
}

/**
 * The live agent activity of the browser: controlled tabs, pending consents and
 * help requests, plus the actions the surfaces can take. Reads the store for the
 * `agentControlled` marks so closing or reclaiming a tab updates every chip.
 */
export function useBrowserAgentActivity(options: BrowserAgentActivityOptions = {}): BrowserAgentActivity {
  const store = options.store ?? browserStore;
  const labels = options.controllerLabels;
  const ids = options.controllerIds;
  const hints = options.taskHints;
  const visibleBrowserId = options.visibleBrowserId ?? null;
  const [pending, setPending] = useState<BrowserAgentApproval[]>([]);
  const [controllers, setControllers] = useState<BrowserAgentController[]>([]);
  const [handoffs, setHandoffs] = useState<HandoffRequestPayload[]>([]);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const controlledTabs = useBrowserStore(selectAgentControlledTabs, store);

  useEffect(() => {
    let cancelled = false;
    // The change event carries the authoritative pending list; the overview read only seeds
    // it until the first event and keeps controller names current (as the approval card does).
    let heardEvent = false;
    const refresh = () =>
      getBrowserAgentOverview().then(
        (overview) => {
          if (cancelled || !overview) return;
          if (!heardEvent) setPending(overview.pending);
          setControllers(overview.controllers);
        },
        (failure: unknown) => {
          if (!cancelled) setErrorCode(extractBrowserErrorCode(failure));
        },
      );
    // Subscribe before the first read so a request arriving in between is not missed.
    // `listen` throws synchronously when the host exposes no event bridge (a web or remote
    // build), and this hook is mounted by the shell in every mode: a missing bridge must
    // leave the badge off, not tear down the mount.
    const approvals = subscribeOrNothing(() => onBrowserAgentApprovals((next) => {
      if (cancelled) return;
      heardEvent = true;
      setPending(next);
      // A pairing just approved adds a controller name the surfaces need for later requests.
      void refresh();
    }));
    const help = subscribeOrNothing(() => onBrowserAgentHelpRequest((request) => {
      if (cancelled) return;
      setHandoffs((list) => [...list.filter((item) => item.browserId !== request.browserId), request]);
    }));
    void refresh();
    return () => {
      cancelled = true;
      void approvals.then((stop) => stop());
      void help.then((stop) => stop());
    };
  }, []);

  const activeControllers = useMemo(() => controllers.filter((controller) => !controller.revoked), [controllers]);

  const controllerLabelFor = useCallback(
    (browserId: string): string | null => {
      const explicit = labels?.[browserId];
      if (explicit) return explicit;
      // The overview names controllers, not their leases; with one active controller
      // every controlled tab belongs to it, otherwise the caller must attribute tabs.
      if (activeControllers.length === 1) return controllerName(controllers, activeControllers[0].id);
      return null;
    },
    [labels, activeControllers, controllers],
  );

  const controllerIdFor = useCallback(
    (browserId: string): string | null => {
      const explicit = ids?.[browserId];
      if (explicit) return explicit;
      // The overview names controllers, not their leases; with one active controller
      // every controlled tab belongs to it, otherwise the caller must attribute tabs.
      if (activeControllers.length === 1) return activeControllers[0].id;
      return null;
    },
    [ids, activeControllers],
  );

  const taskStatusFor = useCallback(
    (browserId: string): MahoAgentTaskStatus => {
      const hint = hints?.[browserId]?.status;
      if (hint) return hint;
      // A handoff is the agent saying it cannot continue alone: the task is waiting, not running.
      if (handoffs.some((request) => request.browserId === browserId)) return "waiting";
      return "running";
    },
    [hints, handoffs],
  );

  const planeFor = useCallback(
    (browserId: string): MahoControlPlane => hints?.[browserId]?.plane ?? "embedded",
    [hints],
  );

  const receiptFor = useCallback(
    (browserId: string): string | null => {
      const receipt = hints?.[browserId]?.receipt;
      return receipt && receipt.trim() ? receipt.trim() : null;
    },
    [hints],
  );

  const stateFor = useCallback(
    (browserId: string): MahoControlActivityState => {
      const status = taskStatusFor(browserId);
      if (status === "failed") return "error";
      if (status === "waiting") return handoffs.some((request) => request.browserId === browserId) ? "paused" : "waitingApproval";
      if (status === "done" || status === "queued") return "idle";
      if (!controlledTabs.some((tab) => tab.browserId === browserId)) return "disconnected";
      // A lease no live controller can hold is a controller that went away: say so instead of
      // claiming the agent is acting on the tab. An unattributed lease with controllers alive is
      // only a missing name, so it keeps the live verb.
      if (activeControllers.length === 0) return "disconnected";
      return hints?.[browserId]?.verb === "reading" ? "reading" : "acting";
    },
    [controlledTabs, activeControllers, handoffs, hints, taskStatusFor],
  );

  const activeTasks = useMemo<BrowserAgentTask[]>(
    () =>
      controlledTabs.map((tab) => ({
        browserId: tab.browserId,
        title: tabTitle(tab),
        controllerId: controllerIdFor(tab.browserId),
        controllerLabel: controllerLabelFor(tab.browserId),
        status: taskStatusFor(tab.browserId),
        plane: planeFor(tab.browserId),
        receiptSummary: receiptFor(tab.browserId),
        isBackgroundTab: visibleBrowserId !== null && visibleBrowserId !== tab.browserId,
      })),
    [controlledTabs, controllerIdFor, controllerLabelFor, planeFor, receiptFor, taskStatusFor, visibleBrowserId],
  );

  const controlledBrowserIds = useMemo(() => controlledTabs.map((tab) => tab.browserId), [controlledTabs]);

  const handoffRequests = useMemo<HandoffRequest[]>(
    () => handoffs.map((request) => ({ ...request, controllerLabel: controllerLabelFor(request.browserId) })),
    [handoffs, controllerLabelFor],
  );

  const run = useCallback(async (action: () => Promise<void>) => {
    setBusy(true);
    setErrorCode(null);
    try {
      await action();
    } catch (failure) {
      setErrorCode(extractBrowserErrorCode(failure));
    } finally {
      setBusy(false);
    }
  }, []);

  const takeOver = useCallback(
    (browserId: string) =>
      run(async () => {
        // Reclaims the lease: the agent stops writing, the page is not closed or reloaded.
        await reclaimBrowserAgent(browserId, true);
        // A tab the store no longer holds must not be resurrected as a phantom `about:blank`
        // row by the reclaim: the lease is the only thing left to clear.
        if (store.getState().tabs[browserId]) {
          store.upsertTab({ browserId, agentControlled: false });
        }
        setHandoffs((list) => list.filter((item) => item.browserId !== browserId));
      }),
    [run, store],
  );

  const doneHandoff = useCallback(
    (browserId: string) =>
      run(async () => {
        // The human answered the request; the agent may control the tab again.
        await reclaimBrowserAgent(browserId, false);
        setHandoffs((list) => list.filter((item) => item.browserId !== browserId));
      }),
    [run],
  );

  const respondToHelp = useCallback(
    (requestId: string, outcome: "complete" | "cancel") =>
      run(async () => {
        // The help bar addresses one request: resolve it to its tab first, so two handoffs on
        // two tabs can never be answered by the wrong button.
        const request = handoffs.find((item) => item.requestId === requestId)
          ?? handoffs.find((item) => item.browserId === requestId);
        if (!request) return;
        if (outcome === "complete") {
          await reclaimBrowserAgent(request.browserId, false);
        } else {
          await reclaimBrowserAgent(request.browserId, true);
          if (store.getState().tabs[request.browserId]) {
            store.upsertTab({ browserId: request.browserId, agentControlled: false });
          }
        }
        // Identify the answered request by its id when both sides carry one, and by its tab
        // otherwise: a payload without a request id is only addressable by the tab.
        setHandoffs((list) => list.filter((item) => (
          request.requestId && item.requestId
            ? item.requestId !== request.requestId
            : item.browserId !== request.browserId
        )));
      }),
    [handoffs, run, store],
  );

  const revokeController = useCallback(
    (controllerId: string) =>
      run(async () => {
        // The tabs this controller held, read before the revoke changes the overview: their
        // chips have to clear with the lease instead of waiting for each tab to close.
        const attributed = controlledTabs
          .filter((tab) => controllerIdFor(tab.browserId) === controllerId)
          .map((tab) => tab.browserId);
        await revokeBrowserAgent(controllerId);
        for (const browserId of attributed) {
          if (store.getState().tabs[browserId]) {
            store.upsertTab({ browserId, agentControlled: false });
          }
        }
        const overview = await getBrowserAgentOverview();
        setPending(overview.pending);
        setControllers(overview.controllers);
      }),
    [controlledTabs, controllerIdFor, run, store],
  );

  const resolveApproval = useCallback(
    (approvalId: string, resolution: ApprovalResolution) =>
      run(async () => {
        await resolveBrowserAgentApproval(approvalId, resolution);
        setPending((list) => list.filter((item) => item.id !== approvalId));
      }),
    [run],
  );

  return {
    activeTasks,
    pendingApprovals: pending,
    controlledBrowserIds,
    handoffRequests,
    badgeCount: activeTasks.length + handoffRequests.length,
    errorCode,
    busy,
    controllers,
    controllerLabelFor,
    controllerIdFor,
    taskStatusFor,
    planeFor,
    receiptFor,
    stateFor,
    takeOver,
    doneHandoff,
    respondToHelp,
    revokeController,
    resolveApproval,
  };
}

export const AGENT_LABEL_FALLBACK = "a paired agent";

function agentLabel(label: string | null): string {
  return label ?? AGENT_LABEL_FALLBACK;
}

function MahoAgentError({ activity, testId }: { activity: BrowserAgentActivity; testId: string }) {
  if (!activity.errorCode) return null;
  return (
    <p role="alert" data-testid={testId} className="mt-1 text-[11px] text-destructive">
      {activity.errorCode}
    </p>
  );
}

/** One row of the Agent Tasks section: status, title, receipt summary and the row actions. */
export type MahoAgentTaskRow = {
  browserId: string;
  title: string;
  status: MahoAgentTaskStatus;
  /** `Running: <title>` - the row's visible and accessible label. */
  label: string;
  receiptSummary: string | null;
  attention: boolean;
  canDisconnect: boolean;
};

/**
 * Folds the activity into render rows, the way `BuildAgentTaskRows` does in Maho: a terminal
 * status is history, not work, so it neither needs the human nor offers Disconnect.
 */
export function buildAgentTaskRows(activity: BrowserAgentActivity): MahoAgentTaskRow[] {
  return activity.activeTasks.map((task) => ({
    browserId: task.browserId,
    title: task.title,
    status: task.status,
    label: `${agentTaskStatusText(task.status)}: ${task.title}`,
    receiptSummary: task.receiptSummary,
    attention: task.status === "waiting" || task.status === "failed",
    canDisconnect: task.status === "queued" || task.status === "running" || task.status === "waiting",
  }));
}

/** Compact chip for `MahoSidebarTopBar`'s `controlActivity` slot. */
export function MahoControlActivityChip({
  activity,
  onDetails,
  onSelectTab,
  showDetails = true,
  compact = false,
}: {
  activity: BrowserAgentActivity;
  /** What Details does when the rows live elsewhere (the drawer); omit to expand them inline. */
  onDetails?: () => void;
  onSelectTab?: (browserId: string) => void;
  /** False when the surrounding surface already shows the Agent Tasks rows, so no button lies. */
  showDetails?: boolean;
  /** Strip mode: the glyph, the plane tag and the kill switch stay; the prose elides away. */
  compact?: boolean;
}) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const task = activity.activeTasks[0];
  const handoff = activity.handoffRequests[0];
  const pending = activity.pendingApprovals.length > 0;
  if (activity.badgeCount === 0 && !pending) return null;

  const browserId = task?.browserId ?? handoff?.browserId;
  // The tab's own state outranks the approval list: a disconnected task stays disconnected even
  // while an unrelated consent is pending.
  const state: MahoControlActivityState = handoff
    ? "paused"
    : pending
      ? "waitingApproval"
      : browserId
        ? activity.stateFor(browserId)
        : "acting";
  const plane = browserId ? activity.planeFor(browserId) : "embedded";
  const phrase = controlStatePhrase(state, { background: task?.isBackgroundTab === true });
  const target = task?.title || "Untitled tab";
  const label = task?.controllerLabel ?? handoff?.controllerLabel ?? "Maho AI";
  // The chip's own line carries the verb; the plane gets its own tag beside it, as the reference
  // view does, so a CLI caller never reads as the embedded agent.
  const statusText = phrase ? `AI - ${phrase}` : (task ? `AI - ${agentTaskStatusText(task.status)}` : "");
  const accessibleStatus = phrase || (task ? agentTaskStatusText(task.status) : "");
  // The hover title spells the plane out in the same line the tag abbreviates.
  const titleText = controlStatusText(state, plane, { background: task?.isBackgroundTab === true }) || statusText;
  const Icon = stateIcon(state);
  const stop = () => {
    if (pending && task?.controllerId) void activity.revokeController(task.controllerId);
    else if (pending) for (const approval of activity.pendingApprovals) void activity.resolveApproval(approval.id, "deny");
    else if (browserId) void activity.takeOver(browserId);
  };

  return (
    <div className="maho-control-activity" data-compact={compact ? "true" : "false"}>
      <div
        role="status"
        aria-live="polite"
        aria-label={`${label} - ${accessibleStatus} - ${target}`}
        title={titleText || undefined}
        data-testid="maho-control-activity-chip"
        data-state={state}
        data-plane={plane}
        data-compact={compact ? "true" : "false"}
        className="maho-control-activity__chip"
      >
        <Icon aria-hidden="true" />
        {compact ? null : <span className="maho-control-activity__status">{statusText}</span>}
        {plane !== "embedded" ? (
          <span className="maho-control-activity__plane" data-testid="maho-control-activity-plane">
            {planeLabel(plane)}
          </span>
        ) : null}
        {compact ? null : <span className="maho-control-activity__target">{target}</span>}
        {!compact && task?.receiptSummary ? (
          <span className="maho-control-activity__receipt" data-testid="maho-control-activity-receipt" title={task.receiptSummary}>
            {task.receiptSummary}
          </span>
        ) : null}
        {compact || !showDetails ? null : (
          <button
            type="button"
            aria-expanded={onDetails ? undefined : detailsOpen}
            onClick={() => onDetails ? onDetails() : setDetailsOpen((value) => !value)}
          >
            Details
          </button>
        )}
        {stateAllowsStop(state) ? (
          <button type="button" disabled={activity.busy} onClick={stop}>{stopLabelForState(state)}</button>
        ) : null}
      </div>
      {!compact && showDetails && detailsOpen ? <MahoAgentTasksSection activity={activity} onSelectTab={onSelectTab} /> : null}
    </div>
  );
}

/** Compact badge for a tab row: which controller borrows this tab. */
export function MahoTabBorrowChip({
  browserId,
  controllerLabel,
}: {
  browserId: string;
  controllerLabel: string | null;
}) {
  if (!controllerLabel) return null;
  return (
    <span
      data-testid="maho-tab-borrow-chip"
      data-browser-id={browserId}
      className="inline-flex max-w-[84px] items-center gap-0.5 rounded-full border border-border px-1.5 text-[10px] text-muted-foreground"
      title={`Borrowed by ${controllerLabel}`}
    >
      <Hand aria-hidden="true" className="h-2.5 w-2.5" />
      <span className="truncate">{controllerLabel}</span>
    </span>
  );
}

/**
 * Collapsible "Agent tasks" sidebar section: controlled tabs (click to focus) and
 * handoffs with quick Revoke / Approve / Deny.
 */
export function MahoAgentTasksSection({
  activity,
  onSelectTab,
}: {
  activity: BrowserAgentActivity;
  onSelectTab?: (browserId: string) => void;
}) {
  const [open, setOpen] = useState(true);
  const rows = buildAgentTaskRows(activity);
  const taskCount = rows.length;
  const hasActivity = taskCount > 0 || activity.handoffRequests.length > 0 || activity.pendingApprovals.length > 0;
  const hasAttention = rows.some((row) => row.attention);
  // A row that needs the human opens the section by itself, exactly like Maho's
  // `expand_for_attention`: a queued "Needs you" behind a collapsed header is a missed request.
  useEffect(() => {
    if (activity.handoffRequests.length || activity.pendingApprovals.length || hasAttention) setOpen(true);
  }, [activity.handoffRequests.length, activity.pendingApprovals.length, hasAttention]);
  if (!hasActivity && !activity.errorCode) return null;

  return (
    <section aria-label="Agent tasks" data-testid="maho-agent-tasks" className="maho-agent-tasks">
      <button
        type="button"
        aria-expanded={open}
        aria-label="Tasks"
        onClick={() => setOpen((current) => !current)}
        className="maho-agent-tasks__header"
      >
        {open ? <ChevronDown aria-hidden="true" className="h-3 w-3" /> : <ChevronRight aria-hidden="true" className="h-3 w-3" />}
        Tasks ({open ? "expanded" : "collapsed"})
        <span data-testid="maho-agent-tasks-count" className="sr-only">
          {activity.badgeCount}
        </span>
      </button>
      {open ? (
        <div className="flex flex-col gap-0.5">
          {rows.map((row) => {
            const Icon = statusIcon(row.status);
            return (
              <div key={row.browserId} className="maho-agent-tasks__row" data-testid="maho-agent-task-row" data-status={row.status}>
                <button
                  type="button"
                  onClick={() => onSelectTab?.(row.browserId)}
                  aria-label={row.label}
                  className="flex min-w-0 flex-1 items-center gap-1 text-left hover:underline"
                >
                  <Icon aria-hidden="true" className="h-3 w-3 shrink-0" />
                  <span className="truncate">{row.label}</span>
                </button>
                {row.receiptSummary ? (
                  <span
                    className="maho-agent-tasks__receipt"
                    data-testid="maho-agent-task-receipt"
                    aria-label={`Last update: ${row.receiptSummary}`}
                    title={row.receiptSummary}
                  >
                    {row.receiptSummary}
                  </span>
                ) : null}
                {row.canDisconnect ? (
                  <button type="button" disabled={activity.busy} aria-label={`Disconnect ${row.title}`} onClick={() => void activity.takeOver(row.browserId)}>
                    Disconnect
                  </button>
                ) : null}
              </div>
            );
          })}
          {activity.handoffRequests.map((request) => (
            <div
              key={`handoff-${request.requestId ?? request.browserId}`}
              data-testid="maho-agent-task-handoff"
              className="flex flex-col gap-0.5 rounded-md border border-border px-1 py-1"
            >
              <span className="truncate text-[11px]">
                {agentLabel(request.controllerLabel)} needs help
                {request.reason ? `: ${request.reason}` : ""}
              </span>
              <span className="flex justify-end gap-1">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-5 px-1.5 text-[10px]"
                  disabled={activity.busy}
                  onClick={() => void activity.takeOver(request.browserId)}
                >
                  Take over
                </Button>
                <Button
                  type="button"
                  size="sm"
                  className="h-5 px-1.5 text-[10px]"
                  disabled={activity.busy}
                  onClick={() => void activity.doneHandoff(request.browserId)}
                >
                  Done
                </Button>
              </span>
            </div>
          ))}
          {activity.pendingApprovals.map((approval) => (
            <div
              key={approval.id}
              data-testid="maho-agent-task-approval"
              className="flex flex-col gap-0.5 rounded-md border border-border px-1 py-1"
            >
              <span className="flex items-center gap-1 text-[11px]">
                <ShieldQuestion aria-hidden="true" className="h-3 w-3 shrink-0" />
                <span className="truncate">{describePending(approval, activity.controllers)}</span>
              </span>
              <span className="flex justify-end gap-1">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-5 px-1.5 text-[10px]"
                  disabled={activity.busy}
                  onClick={() => void activity.resolveApproval(approval.id, "deny")}
                >
                  Deny
                </Button>
                <Button
                  type="button"
                  size="sm"
                  className="h-5 px-1.5 text-[10px]"
                  disabled={activity.busy}
                  onClick={() => void activity.resolveApproval(approval.id, "approveOnce")}
                >
                  Approve
                </Button>
              </span>
            </div>
          ))}
          {taskCount === 0 && activity.handoffRequests.length === 0 && activity.pendingApprovals.length === 0 ? (
            <p className="px-1 text-[11px] text-muted-foreground">No agent activity</p>
          ) : null}
          <MahoAgentError activity={activity} testId="maho-agent-tasks-error" />
        </div>
      ) : null}
    </section>
  );
}

function describePending(approval: BrowserAgentApproval, controllers: BrowserAgentController[]): string {
  switch (approval.kind) {
    case "pairController":
      return `Allow "${approval.label}" to control the browser`;
    case "originGrant":
      return `Let ${controllerName(controllers, approval.controller)} use ${approval.origin}`;
    case "fileCapability":
      return `Let ${controllerName(controllers, approval.controller)} run ${approval.tool}`;
    case "nativeCapability":
      return `Let ${controllerName(controllers, approval.controller)} run ${approval.tool}`;
  }
}

/**
 * Banner above an agent-controlled tab plus, when the agent asked for help, the
 * handoff help bar with the reason and Complete / Cancel. Take over only reclaims
 * the lease; the page is neither closed nor reloaded. The banner floats over the
 * page's top edge and slides in (see `.maho-agent-banner-wrap` in `maho.css`).
 */
export function MahoAgentControlledBanner({
  activity,
  browserId,
}: {
  activity: BrowserAgentActivity;
  browserId: string;
}) {
  const controlled = activity.controlledBrowserIds.includes(browserId);
  const handoff = activity.handoffRequests.find((request) => request.browserId === browserId) ?? null;
  if (!controlled && !handoff) return null;
  const label = activity.controllerLabelFor(browserId) ?? handoff?.controllerLabel ?? null;
  const state = activity.stateFor(browserId);
  const plane = activity.planeFor(browserId);
  const task = activity.activeTasks.find((candidate) => candidate.browserId === browserId);
  // The banner sits over this very tab, so only a card in another window can be "elsewhere";
  // it never claims background attribution for the tab it is drawn on.
  const phrase = controlStatePhrase(state);
  const requestId = handoff?.requestId ?? handoff?.browserId ?? "";

  return (
    <div
      role="status"
      data-testid="maho-agent-controlled-banner"
      data-browser-id={browserId}
      data-state={state}
      className="maho-agent-banner-wrap"
    >
      <div className="maho-agent-banner">
        <span className="maho-agent-banner__dot" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate">
          This tab is controlled by {label ?? "the agent"}
        </span>
        {plane !== "embedded" ? (
          <span className="maho-control-activity__plane" data-testid="maho-agent-banner-plane">
            {planeLabel(plane)}
          </span>
        ) : null}
        {phrase ? <span className="shrink-0 text-[11px] text-muted-foreground" data-testid="maho-agent-banner-state">{phrase}</span> : null}
        {task?.receiptSummary ? (
          <span className="maho-agent-banner__receipt shrink-0 truncate text-[11px] text-muted-foreground" title={task.receiptSummary}>
            {task.receiptSummary}
          </span>
        ) : null}
        {handoff || !stateAllowsStop(state) ? null : (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-6 px-2 text-[11px]"
            disabled={activity.busy}
            onClick={() => void activity.takeOver(browserId)}
          >
            {stopLabelForState(state)}
          </Button>
        )}
      </div>
      {handoff ? (
        <div
          data-testid="maho-agent-help-bar"
          data-request-id={requestId}
          className="maho-agent-help-bar"
        >
          <span className="min-w-0 flex-1">
            {handoff.reason || "Agent is waiting for your help"}
          </span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-6 px-2 text-[11px]"
            disabled={activity.busy}
            onClick={() => void activity.respondToHelp(requestId, "complete")}
          >
            Complete
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-6 px-2 text-[11px]"
            disabled={activity.busy}
            onClick={() => void activity.respondToHelp(requestId, "cancel")}
          >
            Cancel
          </Button>
        </div>
      ) : null}
      <MahoAgentError activity={activity} testId="maho-agent-banner-error" />
    </div>
  );
}
