// Maho agent control surfaces: the Agent Tasks section, the tab borrow chip, the
// top controlled banner and the human handoff help bar.
//
// One hook (`useBrowserAgentActivity`) owns the data: it seeds from the backend's
// agent overview, follows the approval change event and the agent's help request,
// and reads which tabs the store marks `agentControlled`. Every surface below is
// presentational over that snapshot, so the sidebar slots (`controlActivity`,
// `agentSection`) and the browser canvas all render the same truth.
//
// IPC failures arrive as structured `{ code, message, details }`; they are read
// through `extractBrowserErrorCode`, never by matching message text.

import { ChevronDown, ChevronRight, Hammer, Hand, Pause, ShieldQuestion } from "lucide-react";
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

/** A tab an agent currently controls, named by its store title and controller. */
export type BrowserAgentTask = {
  browserId: string;
  title: string;
  controllerId: string | null;
  controllerLabel: string | null;
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
  takeOver: (browserId: string) => Promise<void>;
  doneHandoff: (browserId: string) => Promise<void>;
  revokeController: (controllerId: string) => Promise<void>;
  resolveApproval: (approvalId: string, resolution: ApprovalResolution) => Promise<void>;
};

export type BrowserAgentActivityOptions = {
  store?: BrowserStore;
  /** Per-tab controller names, for a caller that tracks which controller leases which tab. */
  controllerLabels?: Record<string, string>;
  /** Per-tab controller ids, the companion of `controllerLabels`. */
  controllerIds?: Record<string, string>;
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

/**
 * The live agent activity of the browser: controlled tabs, pending consents and
 * help requests, plus the four actions the surfaces can take. Reads the store for
 * the `agentControlled` marks so closing or reclaiming a tab updates every chip.
 */
export function useBrowserAgentActivity(options: BrowserAgentActivityOptions = {}): BrowserAgentActivity {
  const store = options.store ?? browserStore;
  const labels = options.controllerLabels;
  const ids = options.controllerIds;
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

  const activeTasks = useMemo<BrowserAgentTask[]>(
    () =>
      controlledTabs.map((tab) => ({
        browserId: tab.browserId,
        title: tabTitle(tab),
        controllerId: controllerIdFor(tab.browserId),
        controllerLabel: controllerLabelFor(tab.browserId),
      })),
    [controlledTabs, controllerLabelFor, controllerIdFor],
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
    takeOver,
    doneHandoff,
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

/** Compact chip for `MahoSidebarTopBar`'s `controlActivity` slot. */
export function MahoControlActivityChip({ activity, onDetails, onSelectTab }: { activity: BrowserAgentActivity; onDetails?: () => void; onSelectTab?: (browserId: string) => void }) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const task = activity.activeTasks[0];
  const handoff = activity.handoffRequests[0];
  const pending = activity.pendingApprovals.length > 0;
  if (activity.badgeCount === 0 && !pending) return null;
  const phrase = pending ? "Waiting for approval" : handoff ? "Paused" : "Acting on this tab";
  const target = task?.title || "Untitled tab";
  const label = task?.controllerLabel ?? handoff?.controllerLabel ?? "Maho AI";
  const browserId = task?.browserId ?? handoff?.browserId;
  const stop = () => {
    if (pending && task?.controllerId) void activity.revokeController(task.controllerId);
    else if (browserId) void activity.takeOver(browserId);
    else for (const approval of activity.pendingApprovals) void activity.resolveApproval(approval.id, "deny");
  };
  return (
    <div className="maho-control-activity">
      <div role="status" aria-live="polite" aria-label={`${label} - ${phrase} - ${target}`} data-testid="maho-control-activity-chip" className="maho-control-activity__chip">
        {pending ? <ShieldQuestion aria-hidden="true" /> : handoff ? <Pause aria-hidden="true" /> : <Hammer aria-hidden="true" />}
        <span className="maho-control-activity__status">AI - {phrase}</span>
        <span className="maho-control-activity__target">{target}</span>
        <button type="button" aria-expanded={detailsOpen} onClick={() => onDetails ? onDetails() : setDetailsOpen((value) => !value)}>Details</button>
        <button type="button" disabled={activity.busy} onClick={stop}>{pending || handoff ? "Revoke" : "Stop"}</button>
      </div>
      {detailsOpen ? <MahoAgentTasksSection activity={activity} onSelectTab={onSelectTab} /> : null}
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
  const taskCount = activity.activeTasks.length;
  const hasActivity = taskCount > 0 || activity.handoffRequests.length > 0 || activity.pendingApprovals.length > 0;
  useEffect(() => {
    if (activity.handoffRequests.length || activity.pendingApprovals.length) setOpen(true);
  }, [activity.handoffRequests.length, activity.pendingApprovals.length]);
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
          {activity.activeTasks.map((task) => (
            <div key={task.browserId} className="maho-agent-tasks__row">
              <button
                type="button"
                onClick={() => onSelectTab?.(task.browserId)}
                aria-label={`Running: ${task.title}`}
                className="flex min-w-0 flex-1 items-center gap-1 text-left hover:underline"
              >
                <span className="truncate">Running: {task.title}</span>
              </button>
              <button type="button" disabled={activity.busy} aria-label={`Disconnect ${task.title}`} onClick={() => void activity.takeOver(task.browserId)}>
                Disconnect
              </button>
            </div>
          ))}
          {activity.handoffRequests.map((request) => (
            <div
              key={`handoff-${request.browserId}`}
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
 * handoff help bar with the reason and Take over / Done. Take over only reclaims
 * the lease; the page is neither closed nor reloaded.
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

  return (
    <div
      role="status"
      data-testid="maho-agent-controlled-banner"
      data-browser-id={browserId}
      className="maho-agent-banner-wrap"
    >
      <div className="maho-agent-banner">
        <span className="maho-agent-banner__dot" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate">
          This tab is controlled by {label ?? "the agent"}
        </span>
        {handoff ? null : (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-6 px-2 text-[11px]"
            disabled={activity.busy}
            onClick={() => void activity.takeOver(browserId)}
          >
            Stop
          </Button>
        )}
      </div>
      {handoff ? (
        <div
          data-testid="maho-agent-help-bar"
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
            onClick={() => void activity.doneHandoff(browserId)}
          >
            Complete
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-6 px-2 text-[11px]"
            disabled={activity.busy}
            onClick={() => void activity.takeOver(browserId)}
          >
            Cancel
          </Button>
        </div>
      ) : null}
      <MahoAgentError activity={activity} testId="maho-agent-banner-error" />
    </div>
  );
}
