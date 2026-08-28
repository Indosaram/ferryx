import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  clearRemoteAuthToken,
  getRemoteAuthToken,
  setRemoteAuthToken,
} from "../lib/remoteClient";
import { PairingPage } from "./PairingPage";
import {
  getRemoteDocumentTitle,
  normalizeRemoteWorkspaceState,
  RemoteWorkspaceMirror,
  type RemoteContextOption,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";
import { RemoteTerminal } from "./RemoteTerminal";

const REMOTE_ACTIVE_SELECTION_CHANGED_EVENT = "remote_active_selection_changed";
/// How long a selection may stay unconfirmed before the picker is released for
/// a retry. The desktop normally republishes within one refresh round-trip.
const CONFIRMATION_TIMEOUT_MS = 6000;

type RemoteActiveSelectionEvent = {
  readonly workspaceId: string | null;
  readonly worktreeSlug: string | null;
  readonly tabId?: string | null;
};

type RemoteActiveSelectionChange = {
  readonly selection: RemoteActiveSelectionEvent | null;
};

export type WaitingTabTarget = {
  readonly tabId?: string | null;
  readonly label: string;
  readonly workspaceId: string;
  readonly worktreeSlug: string | null;
  readonly worktreeLabel: string | null;
};

function collectWaitingTargets(model: RemoteWorkspaceModel): WaitingTabTarget[] {
  const targets: WaitingTabTarget[] = [];
  const seen = new Set<string>();

  const currentWorkspaceId = model.context.workspaceId;
  const currentWorktreeSlug = model.context.worktreeSlug;
  const currentWorktreeLabel = model.context.worktreeLabel;
  const activeTabId = model.context.activeTabId;

  if (currentWorkspaceId && model.context.terminalTabs) {
    for (const tab of model.context.terminalTabs) {
      if (tab.activityState === "waiting" && tab.id !== activeTabId) {
        const key = `${currentWorkspaceId}\u0000${currentWorktreeSlug ?? ""}\u0000${tab.id}`;
        if (!seen.has(key)) {
          seen.add(key);
          targets.push({
            tabId: tab.id,
            label: tab.label,
            workspaceId: currentWorkspaceId,
            worktreeSlug: currentWorktreeSlug,
            worktreeLabel: currentWorktreeLabel,
          });
        }
      }
    }
  }

  for (const option of model.options) {
    if (option.attention === "waiting") {
      const isCurrent =
        option.workspaceId === currentWorkspaceId &&
        (option.worktreeSlug ?? option.worktreeLabel) === (currentWorktreeSlug ?? currentWorktreeLabel) &&
        (!option.tabId || option.tabId === activeTabId);
      if (!isCurrent) {
        const key = `${option.workspaceId}\u0000${option.worktreeSlug ?? ""}\u0000${option.tabId ?? ""}`;
        if (!seen.has(key)) {
          seen.add(key);
          targets.push({
            tabId: option.tabId ?? null,
            label: option.worktreeLabel ?? option.worktreeSlug ?? option.workspaceId,
            workspaceId: option.workspaceId,
            worktreeSlug: option.worktreeSlug,
            worktreeLabel: option.worktreeLabel,
          });
        }
      }
    }
  }

  return targets;
}

function formatAttentionAriaLabel(
  target: WaitingTabTarget,
  currentContext: RemoteWorkspaceModel["context"],
  waitingCount: number,
): string {
  const currentWorktree = currentContext.worktreeSlug ?? currentContext.worktreeLabel;
  const targetWorktree = target.worktreeLabel ?? target.worktreeSlug;
  const differentWorktree =
    (target.workspaceId && currentContext.workspaceId && target.workspaceId !== currentContext.workspaceId) ||
    (Boolean(targetWorktree) && Boolean(currentWorktree) && targetWorktree !== currentWorktree);

  const location = differentWorktree && targetWorktree ? ` (${targetWorktree})` : "";
  const countSuffix = waitingCount > 1 ? ` (${waitingCount} waiting)` : " (waiting)";
  return `${target.label}${location}${countSuffix}`;
}

const EMPTY_MODEL: RemoteWorkspaceModel = {
  context: {
    workspaceId: null,
    worktreeSlug: null,
    worktreeLabel: null,
    activeTerminal: null,
  },
  options: [],
};

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function optionalString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function parseActiveSelectionEvent(raw: unknown): RemoteActiveSelectionChange | null {
  if (typeof raw !== "string") return null;
  let message: unknown;
  try {
    message = JSON.parse(raw);
  } catch (error) {
    if (error instanceof SyntaxError) return null;
    throw error;
  }
  const event = record(message);
  if (event?.event !== REMOTE_ACTIVE_SELECTION_CHANGED_EVENT) return null;
  const payload = record(event.payload);
  if (!payload) return { selection: null };
  return {
    selection: {
      workspaceId: optionalString(payload.workspaceId),
      worktreeSlug: optionalString(payload.worktreeSlug),
      tabId: optionalString(payload.tabId ?? payload.activeTabId),
    },
  };
}

function selectionMatchesActiveContext(
  option: RemoteContextOption,
  selection: RemoteActiveSelectionEvent,
): boolean {
  if (selection.workspaceId !== option.workspaceId) return false;
  if (option.worktreeSlug !== null && selection.worktreeSlug !== null && selection.worktreeSlug !== option.worktreeSlug) {
    return false;
  }
  if (option.tabId && selection.tabId && selection.tabId !== option.tabId) {
    return false;
  }
  return true;
}

function modelConfirmsSelection(option: RemoteContextOption, model: RemoteWorkspaceModel): boolean {
  const confirmedWorktree = model.context.worktreeSlug ?? model.context.worktreeLabel;
  const requestedWorktree = option.worktreeSlug ?? option.worktreeLabel;
  const workspaceMatches = model.context.workspaceId === option.workspaceId;
  const worktreeMatches = requestedWorktree === null || confirmedWorktree === requestedWorktree;
  const tabMatches = !option.tabId || model.context.activeTabId === option.tabId;
  return workspaceMatches && worktreeMatches && tabMatches;
}

function eventsSocketUrl(token: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/v1/events?token=${encodeURIComponent(token)}`;
}

export const RemoteApp: React.FC = () => {
  const [token, setToken] = useState<string | null>(getRemoteAuthToken);
  const [model, setModel] = useState<RemoteWorkspaceModel>(EMPTY_MODEL);
  const [pending, setPending] = useState<RemoteContextOption | null>(null);
  const pendingSelectionRef = useRef<RemoteContextOption | null>(null);
  const selectionRequestAcceptedRef = useRef(false);
  const selectionEventReceivedRef = useRef(false);
  const confirmationInFlightRef = useRef(false);
  const workspaceRefreshVersionRef = useRef(0);
  const confirmationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const disconnect = useCallback(() => {
    clearRemoteAuthToken();
    setToken(null);
    setModel(EMPTY_MODEL);
    setPending(null);
    pendingSelectionRef.current = null;
    selectionRequestAcceptedRef.current = false;
    selectionEventReceivedRef.current = false;
    confirmationInFlightRef.current = false;
    workspaceRefreshVersionRef.current += 1;
  }, []);

  const handlePaired = useCallback((newToken: string) => {
    setRemoteAuthToken(newToken);
    setToken(newToken);
  }, []);

  const loadWorkspace = useCallback(async (): Promise<RemoteWorkspaceModel | null> => {
    if (!token) return null;
    try {
      const response = await fetch(
        `/api/v1/workspace/state?token=${encodeURIComponent(token)}`,
      );
      if (!response.ok) {
        if (response.status === 401 || response.status === 403) {
          disconnect();
        }
        return null;
      }
      return normalizeRemoteWorkspaceState(await response.json());
    } catch {
      return null;
    }
  }, [disconnect, token]);

  const refreshWorkspace = useCallback(async (): Promise<RemoteWorkspaceModel | null> => {
    const refreshVersion = workspaceRefreshVersionRef.current;
    const next = await loadWorkspace();
    if (!next || workspaceRefreshVersionRef.current !== refreshVersion) return null;
    setModel(next);
    return next;
  }, [loadWorkspace]);

  useEffect(() => {
    const hash = window.location.hash;
    if (!hash.startsWith("#pair=")) return;

    const code = hash.slice("#pair=".length);
    fetch("/api/v1/pair/exchange", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        code,
        deviceName: navigator.userAgent.includes("Mobile") ? "Mobile Device" : "Browser Device",
      }),
    })
      .then((response) => response.json())
      .then((data) => {
        if (typeof data.token !== "string") return;
        handlePaired(data.token);
        window.location.hash = "";
      })
      .catch(() => undefined);
  }, [handlePaired]);

  useEffect(() => {
    if (token) void refreshWorkspace();
  }, [refreshWorkspace, token]);

  useEffect(() => {
    if (!token) {
      document.title = "Ferryx";
      return;
    }
    document.title = getRemoteDocumentTitle(model);
    return () => {
      document.title = "Ferryx";
    };
  }, [model, token]);

  const clearPendingSelection = useCallback(() => {
    if (confirmationTimeoutRef.current !== null) {
      clearTimeout(confirmationTimeoutRef.current);
      confirmationTimeoutRef.current = null;
    }
    pendingSelectionRef.current = null;
    selectionRequestAcceptedRef.current = false;
    selectionEventReceivedRef.current = false;
    confirmationInFlightRef.current = false;
    setPending(null);
  }, []);

  const confirmSelection = useCallback(async (option: RemoteContextOption) => {
    if (confirmationInFlightRef.current) return;
    confirmationInFlightRef.current = true;
    const confirmed = await refreshWorkspace();
    confirmationInFlightRef.current = false;
    if (pendingSelectionRef.current !== option) return;
    if (confirmed && modelConfirmsSelection(option, confirmed)) {
      clearPendingSelection();
    }
  }, [clearPendingSelection, refreshWorkspace]);

  useEffect(() => {
    if (!token || typeof WebSocket === "undefined") return;
    const socket = new WebSocket(eventsSocketUrl(token));
    socket.onmessage = (event) => {
      const change = parseActiveSelectionEvent(event.data);
      if (!change) return;
      workspaceRefreshVersionRef.current += 1;
      const selection = change.selection;
      const pendingSelection = pendingSelectionRef.current;
      if (!pendingSelection) {
        void refreshWorkspace();
        return;
      }
      if (!selection || !selectionMatchesActiveContext(pendingSelection, selection)) {
        void refreshWorkspace();
        return;
      }
      selectionEventReceivedRef.current = true;
      if (selectionRequestAcceptedRef.current) void confirmSelection(pendingSelection);
    };
    return () => socket.close();
  }, [confirmSelection, token]);

  // A desktop that never republishes a matching selection (stale listener,
  // closed window) must not strand the picker: every chip is disabled while a
  // selection is pending, so without a terminal state the phone can only retry
  // by reloading the page.
  const armConfirmationTimeout = useCallback((option: RemoteContextOption) => {
    if (confirmationTimeoutRef.current !== null) clearTimeout(confirmationTimeoutRef.current);
    confirmationTimeoutRef.current = setTimeout(() => {
      confirmationTimeoutRef.current = null;
      if (pendingSelectionRef.current !== option) return;
      clearPendingSelection();
    }, CONFIRMATION_TIMEOUT_MS);
  }, [clearPendingSelection]);

  const selectContext = useCallback(async (option: RemoteContextOption) => {
    if (!token || pending) return;
    pendingSelectionRef.current = option;
    selectionRequestAcceptedRef.current = false;
    selectionEventReceivedRef.current = false;
    setPending(option);

    try {
      const response = await fetch(
        `/api/v1/workspace/select?token=${encodeURIComponent(token)}`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            workspaceId: option.workspaceId,
            ...(option.worktreeSlug ? { worktreeSlug: option.worktreeSlug } : {}),
            ...(option.tabId ? { tabId: option.tabId } : {}),
          }),
        },
      );
      if (!response.ok) throw new Error(`Selection failed (${response.status})`);

      selectionRequestAcceptedRef.current = true;
      workspaceRefreshVersionRef.current += 1;
      confirmationInFlightRef.current = true;
      const immediatelyObserved = await refreshWorkspace();
      confirmationInFlightRef.current = false;
      if (immediatelyObserved && modelConfirmsSelection(option, immediatelyObserved)) {
        clearPendingSelection();
        return;
      }
      armConfirmationTimeout(option);
      if (selectionEventReceivedRef.current) void confirmSelection(option);
    } catch {
      confirmationInFlightRef.current = false;
      clearPendingSelection();
    }
  }, [armConfirmationTimeout, clearPendingSelection, confirmSelection, pending, refreshWorkspace, token]);

  const tabs = model.context.terminalTabs;
  const activeIndex = tabs && model.context.activeTabId
    ? tabs.findIndex((tab) => tab.id === model.context.activeTabId)
    : 0;
  const currentIndex = activeIndex >= 0 ? activeIndex : 0;

  const handleSwipePreviousTab = useCallback(() => {
    if (!tabs || tabs.length <= 1 || currentIndex <= 0 || !model.context.workspaceId) return;
    const prevTab = tabs[currentIndex - 1];
    if (prevTab) {
      void selectContext({
        workspaceId: model.context.workspaceId,
        worktreeSlug: model.context.worktreeSlug,
        worktreeLabel: model.context.worktreeLabel,
        tabId: prevTab.id,
      });
    }
  }, [currentIndex, model.context.workspaceId, model.context.worktreeLabel, model.context.worktreeSlug, selectContext, tabs]);

  const handleSwipeNextTab = useCallback(() => {
    if (!tabs || tabs.length <= 1 || currentIndex >= tabs.length - 1 || !model.context.workspaceId) return;
    const nextTab = tabs[currentIndex + 1];
    if (nextTab) {
      void selectContext({
        workspaceId: model.context.workspaceId,
        worktreeSlug: model.context.worktreeSlug,
        worktreeLabel: model.context.worktreeLabel,
        tabId: nextTab.id,
      });
    }
  }, [currentIndex, model.context.workspaceId, model.context.worktreeLabel, model.context.worktreeSlug, selectContext, tabs]);

  if (!token) return <PairingPage onPaired={handlePaired} />;

  const activeTerminal = model.context.activeTerminal;
  const waitingTargets = collectWaitingTargets(model);
  const firstWaiting = waitingTargets.length > 0 ? waitingTargets[0] : null;
  const waitingCount = waitingTargets.length;
  const attentionAriaLabel = firstWaiting
    ? formatAttentionAriaLabel(firstWaiting, model.context, waitingCount)
    : "";

  return (
    <div className="flex h-[100dvh] min-h-screen flex-col overflow-hidden bg-background text-foreground">
      <header className="flex h-7 shrink-0 items-center justify-between border-b border-border bg-card px-2.5">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="flex size-4 items-center justify-center rounded bg-primary text-[10px] font-bold text-primary-foreground" aria-hidden="true">
            F
          </span>
          <h1 className="truncate text-xs font-semibold leading-none">Ferryx Remote</h1>
          <span className="hidden text-[10px] text-muted-foreground sm:inline leading-none">Following Ferryx Desktop</span>
        </div>
        <div className="flex items-center gap-1.5">
          {firstWaiting ? (
            <button
              type="button"
              data-testid="remote-attention-badge"
              aria-label={attentionAriaLabel}
              disabled={pending !== null}
              onClick={() => {
                void selectContext({
                  workspaceId: firstWaiting.workspaceId,
                  worktreeSlug: firstWaiting.worktreeSlug,
                  worktreeLabel: firstWaiting.worktreeLabel,
                  tabId: firstWaiting.tabId,
                });
              }}
              className="flex h-5 items-center gap-1 rounded bg-status-warning/15 px-1.5 text-[11px] font-medium text-status-warning transition-colors hover:bg-status-warning/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60"
            >
              <span className="size-1.5 rounded-full bg-status-warning ring-2 ring-status-warning/20 motion-reduce:animate-none" aria-hidden="true" />
              <span className="truncate max-w-28 sm:max-w-40">{firstWaiting.label}</span>
              {waitingCount > 1 ? (
                <span className="rounded bg-status-warning/20 px-1 text-[10px] font-mono leading-tight">
                  {waitingCount}
                </span>
              ) : null}
            </button>
          ) : null}
          <button
            type="button"
            onClick={disconnect}
            className="flex h-5 items-center rounded px-1.5 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            Disconnect
          </button>
        </div>
      </header>

      <RemoteWorkspaceMirror
        model={model}
        pending={pending}
        onSelect={(option) => void selectContext(option)}
      >
        {activeTerminal ? (
          <RemoteTerminal
            sessionId={activeTerminal.sessionId}
            token={token}
            onBack={() => undefined}
            embedded
            onSwipePreviousTab={handleSwipePreviousTab}
            onSwipeNextTab={handleSwipeNextTab}
          />
        ) : null}
      </RemoteWorkspaceMirror>
    </div>
  );
};
