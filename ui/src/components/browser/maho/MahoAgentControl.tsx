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

import { ChevronDown, ChevronRight, Hand, ShieldQuestion } from "lucide-react";
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
import { Button } from "../../ui/button";

/** A tab an agent currently controls, named by its store title and controller. */
export type BrowserAgentTask = {
  browserId: string;
  title: string;
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
  takeOver: (browserId: string) => Promise<void>;
  doneHandoff: (browserId: string) => Promise<void>;
  revokeController: (controllerId: string) => Promise<void>;
  resolveApproval: (approvalId: string, resolution: ApprovalResolution) => Promise<void>;
};

export type BrowserAgentActivityOptions = {
  store?: BrowserStore;
  /** Per-tab controller names, for a caller that tracks which controller leases which tab. */
  controllerLabels?: Record<string, string>;
};

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
  const [pending, setPending] = useState<BrowserAgentApproval[]>([]);
  const [controllers, setControllers] = useState<BrowserAgentController[]>([]);
  const [handoffs, setHandoffs] = useState<HandoffRequestPayload[]>([]);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const controlledTabs = useBrowserStore(selectAgentControlledTabs, store);

  useEffect(() => {
    let cancelled = false;
    // The events are authoritative; the overview read seeds the snapshot and keeps
    // controller names current, mirroring the approval card's ordering.
    const refresh = () =>
      getBrowserAgentOverview().then(
        (overview) => {
          if (cancelled || !overview) return;
          setPending(overview.pending);
          setControllers(overview.controllers);
        },
        (failure: unknown) => {
          if (!cancelled) setErrorCode(extractBrowserErrorCode(failure));
        },
      );
    // Subscribe before the first read so a request arriving in between is not missed.
    const approvals = onBrowserAgentApprovals((next) => {
      if (cancelled) return;
      setPending(next);
      void refresh();
    });
    const help = onBrowserAgentHelpRequest((request) => {
      if (cancelled) return;
      setHandoffs((list) => [...list.filter((item) => item.browserId !== request.browserId), request]);
    });
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

  const activeTasks = useMemo<BrowserAgentTask[]>(
    () =>
      controlledTabs.map((tab) => ({
        browserId: tab.browserId,
        title: tabTitle(tab),
        controllerLabel: controllerLabelFor(tab.browserId),
      })),
    [controlledTabs, controllerLabelFor],
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
        store.upsertTab({ browserId, agentControlled: false });
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
        await revokeBrowserAgent(controllerId);
        const overview = await getBrowserAgentOverview();
        setPending(overview.pending);
        setControllers(overview.controllers);
      }),
    [run],
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

function MahoAgentError({ activity }: { activity: BrowserAgentActivity }) {
  if (!activity.errorCode) return null;
  return (
    <p role="alert" data-testid="maho-agent-error" className="mt-1 text-[11px] text-destructive">
      {activity.errorCode}
    </p>
  );
}

/** Compact chip for `MahoSidebarTopBar`'s `controlActivity` slot. */
export function MahoControlActivityChip({ activity }: { activity: BrowserAgentActivity }) {
  if (activity.badgeCount === 0) return null;
  const label = activity.activeTasks[0]?.controllerLabel ?? activity.handoffRequests[0]?.controllerLabel ?? null;
  const helpCount = activity.handoffRequests.length;
  return (
    <span
      role="status"
      aria-live="polite"
      data-testid="maho-control-activity-chip"
      className="inline-flex items-center gap-1 rounded-full border border-border bg-muted/60 px-2 py-0.5 text-[11px] text-foreground"
      title={helpCount > 0 ? `${agentLabel(label)} needs you on ${helpCount} tab(s)` : `${agentLabel(label)} controls ${activity.badgeCount} tab(s)`}
    >
      <Hand aria-hidden="true" className="h-3 w-3" />
      <span className="max-w-[88px] truncate font-medium">{agentLabel(label)}</span>
      <span className="text-muted-foreground">{activity.badgeCount}</span>
      {helpCount > 0 ? (
        <span data-testid="maho-control-activity-help" className="font-medium text-primary">
          {helpCount} needs you
        </span>
      ) : null}
    </span>
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

  return (
    <section aria-label="Agent tasks" data-testid="maho-agent-tasks" className="text-[12px]">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
        className="flex w-full items-center gap-1 rounded-md px-1 py-1 text-left text-[11px] font-medium uppercase tracking-wide text-muted-foreground hover:bg-muted/60"
      >
        {open ? <ChevronDown aria-hidden="true" className="h-3 w-3" /> : <ChevronRight aria-hidden="true" className="h-3 w-3" />}
        Agent tasks
        <span data-testid="maho-agent-tasks-count" className="ml-auto text-foreground">
          {activity.badgeCount}
        </span>
      </button>
      {open ? (
        <div className="mt-1 flex flex-col gap-1">
          {activity.activeTasks.map((task) => (
            <div key={task.browserId} className="flex items-center gap-1 rounded-md px-1 py-0.5">
              <button
                type="button"
                onClick={() => onSelectTab?.(task.browserId)}
                aria-label={`Focus ${task.title}`}
                className="flex min-w-0 flex-1 items-center gap-1 text-left hover:underline"
              >
                <Hand aria-hidden="true" className="h-3 w-3 shrink-0" />
                <span className="truncate">{task.title}</span>
              </button>
              <MahoTabBorrowChip browserId={task.browserId} controllerLabel={task.controllerLabel} />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-5 px-1.5 text-[10px]"
                disabled={activity.busy}
                onClick={() => void activity.takeOver(task.browserId)}
              >
                Revoke
              </Button>
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
          <MahoAgentError activity={activity} />
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
      className="flex flex-col gap-1 border-b border-border bg-muted/70 px-3 py-1.5 text-[12px]"
    >
      <div className="flex items-center gap-2">
        <Hand aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
        <span className="min-w-0 flex-1 truncate">
          <span className="font-medium">{agentLabel(label)}</span> is controlling this tab
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
            Take over
          </Button>
        )}
      </div>
      {handoff ? (
        <div
          data-testid="maho-agent-help-bar"
          className="flex items-center gap-2 rounded-md border border-border bg-background px-2 py-1"
        >
          <span className="min-w-0 flex-1 truncate">
            <span className="font-medium">{agentLabel(handoff.controllerLabel ?? label)}</span> needs help
            {handoff.reason ? `: ${handoff.reason}` : ""}
          </span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-6 px-2 text-[11px]"
            disabled={activity.busy}
            onClick={() => void activity.takeOver(browserId)}
          >
            Take over
          </Button>
          <Button
            type="button"
            size="sm"
            className="h-6 px-2 text-[11px]"
            disabled={activity.busy}
            onClick={() => void activity.doneHandoff(browserId)}
          >
            Done
          </Button>
        </div>
      ) : null}
      <MahoAgentError activity={activity} />
    </div>
  );
}
