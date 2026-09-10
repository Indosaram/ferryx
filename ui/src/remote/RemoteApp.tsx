import { ChevronDown, Laptop } from "lucide-react";
import React, { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Toaster } from "../components/ui/sonner";
import {
  selectBestDirectCandidate,
  normalizeDirectCandidateOrigin,
  type CandidateEndpoint,
  type CandidateEndpointType,
} from "../lib/directPathUpgrade";
import {
  clearRemoteAuthToken,
  getRemoteAuthToken,
  setRemoteAuthToken,
} from "../lib/remoteClient";
import { remoteHostStore, selectActiveHost } from "../state/remoteHostStore";
import { hostAgentTotals, MobileHostDrawer } from "./MobileHostDrawer";
import { PairingPage } from "./PairingPage";
import {
  contextName,
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
  readonly sessionId?: string | null;
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
            sessionId: tab.sessionId,
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
            sessionId: option.sessionId ?? null,
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
  if (option.sessionId && !option.tabId) {
    return model.context.workspaceId === option.workspaceId
      && model.context.activeTerminal?.sessionId === option.sessionId;
  }
  const confirmedWorktree = model.context.worktreeSlug ?? model.context.worktreeLabel;
  const requestedWorktree = option.worktreeSlug ?? option.worktreeLabel;
  const workspaceMatches = model.context.workspaceId === option.workspaceId;
  const worktreeMatches = requestedWorktree === null || confirmedWorktree === requestedWorktree;
  const tabMatches = !option.tabId || model.context.activeTabId === option.tabId;
  return workspaceMatches && worktreeMatches && tabMatches;
}

/**
 * Direct-path upgrade
 * ===================
 * The page is always served over the relay, so the relay endpoint is the only one
 * guaranteed to work and is what the first render connects through. LAN / Tailscale
 * endpoints are *hints* published by the desktop (query string on the pairing link,
 * or a previously stored hint) and are only trusted once a probe confirms them, at
 * which point the active transport URL flips to the direct endpoint.
 *
 * Probing costs a request per candidate, so it only runs when at least one hint
 * exists: a relay-only client never issues a probe.
 */

const DIRECT_HINT_STORAGE_KEY = "ferryx_remote_direct_candidates";
/** LAN beats Tailscale beats relay; see `selectBestDirectCandidate`. */
const CANDIDATE_PRIORITY: Record<CandidateEndpointType, number> = {
  lan: 30,
  tailscale: 20,
  relay: 10,
};
const CONNECTION_BADGE_LABEL: Record<CandidateEndpointType, string> = {
  relay: "Relay (Proxy)",
  lan: "LAN (Direct)",
  tailscale: "Tailscale (Direct)",
};

function directCandidate(type: "lan" | "tailscale", value: unknown): CandidateEndpoint | null {
  const url = normalizeDirectCandidateOrigin(value);
  return url ? { type, url, priority: CANDIDATE_PRIORITY[type] } : null;
}

/**
 * Reads direct-endpoint hints from the current URL first (a freshly scanned pairing
 * link carries the desktop's addresses) and falls back to the last hints this device
 * stored. Any hint found in the URL is persisted so later loads keep the fast path.
 */
function readDirectCandidateHints(hostId: string, readUrl: boolean): CandidateEndpoint[] {
  const storageKey = `${DIRECT_HINT_STORAGE_KEY}_${hostId}`;
  const params = new URLSearchParams(readUrl ? window.location.search : "");
  const fragment = new URLSearchParams(readUrl ? window.location.hash.slice(1) : "");
  const fromUrl = [
    ...(fragment.get("hints") ?? params.get("hints") ?? "").split(",").map((value) => {
      const origin = normalizeDirectCandidateOrigin(value);
      const type = origin && /^100\.(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(new URL(origin).hostname)
        ? "tailscale" : "lan";
      return directCandidate(type, value);
    }),
    directCandidate("lan", params.get("lan")),
    directCandidate("tailscale", params.get("ts") ?? params.get("tailscale")),
  ].filter((candidate): candidate is CandidateEndpoint => candidate !== null);

  if (fromUrl.length > 0) {
    try {
      localStorage.setItem(storageKey, JSON.stringify(fromUrl));
    } catch {
      // Private-mode storage denial must not block the upgrade for this session.
    }
    return fromUrl;
  }

  let stored: unknown;
  try {
    stored = JSON.parse(localStorage.getItem(storageKey) ?? "null");
  } catch {
    return [];
  }
  if (!Array.isArray(stored)) return [];
  return stored
    .map((entry) => {
      const row = record(entry);
      const type = row?.type;
      if (type !== "lan" && type !== "tailscale") return null;
      return directCandidate(type, row?.url);
    })
    .filter((candidate): candidate is CandidateEndpoint => candidate !== null);
}

function relayEndpoint(url: string): CandidateEndpoint {
  return { type: "relay", url, priority: CANDIDATE_PRIORITY.relay };
}

function eventsSocketUrl(token: string, transportUrl: string): string {
  const base = new URL(transportUrl);
  const protocol = base.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${base.host}/api/v1/events?token=${encodeURIComponent(token)}`;
}

/**
 * Same-origin transport keeps request paths relative so nothing changes for the
 * relay case; only a verified direct upgrade produces absolute URLs.
 */
function apiUrl(transportUrl: string, path: string): string {
  return transportUrl === window.location.origin ? path : `${transportUrl}${path}`;
}

export const RemoteApp: React.FC = () => {
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const host = selectActiveHost(state);
  const hostId = state.activeHostId ?? `local:${window.location.origin}`;
  const address = host?.address ?? window.location.origin;
  const relayUrl = new URL(address.includes("://") ? address : `http://${address}`).origin;
  return <RemoteHostConnection key={`${hostId}:${relayUrl}`} hostId={hostId} relayUrl={relayUrl} readUrlHints={state.activeHostId === null} />;
};

const RemoteHostConnection: React.FC<{ hostId: string; relayUrl: string; readUrlHints: boolean }> = ({ hostId, relayUrl, readUrlHints }) => {
  const [token, setToken] = useState<string | null>(() => {
    const scoped = getRemoteAuthToken(hostId);
    if (scoped || !readUrlHints) return scoped;
    // Legacy credentials belong only to the original same-origin connection.
    const legacy = getRemoteAuthToken();
    if (legacy) {
      setRemoteAuthToken(legacy, hostId);
      clearRemoteAuthToken();
    }
    return legacy;
  });
  // Capture fragment hints before successful pairing removes the fragment.
  const [directHints] = useState(() => readDirectCandidateHints(hostId, readUrlHints));
  const [model, setModel] = useState<RemoteWorkspaceModel>(EMPTY_MODEL);
  const [pending, setPending] = useState<RemoteContextOption | null>(null);
  const [selectorOpen, setSelectorOpen] = useState(false);
  const [hostDrawerOpen, setHostDrawerOpen] = useState(false);
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  // First render always speaks to the relay; a verified probe swaps this for a direct endpoint.
  const [transport, setTransport] = useState<CandidateEndpoint>(() => relayEndpoint(relayUrl));
  const remoteHostState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const activeHost = useMemo(() => selectActiveHost(remoteHostState), [remoteHostState]);
  const hostAgentSummary = useMemo(() => hostAgentTotals(remoteHostState), [remoteHostState]);
  const [optimisticSessionId, setOptimisticSessionId] = useState<string | null>(null);
  const [terminalRetryGeneration, setTerminalRetryGeneration] = useState(0);
  const pendingSelectionRef = useRef<RemoteContextOption | null>(null);
  const optimisticSocketSessionIdRef = useRef<string | null>(null);
  const optimisticSocketClosedRef = useRef(false);
  const selectionRequestAcceptedRef = useRef(false);
  const selectionEventReceivedRef = useRef(false);
  const confirmationInFlightRef = useRef(false);
  const workspaceRefreshVersionRef = useRef(0);
  const confirmationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const disconnect = useCallback(() => {
    clearRemoteAuthToken(hostId);
    setToken(null);
    setModel(EMPTY_MODEL);
    setPending(null);
    setOptimisticSessionId(null);
    optimisticSocketSessionIdRef.current = null;
    optimisticSocketClosedRef.current = false;
    pendingSelectionRef.current = null;
    selectionRequestAcceptedRef.current = false;
    selectionEventReceivedRef.current = false;
    confirmationInFlightRef.current = false;
    workspaceRefreshVersionRef.current += 1;
  }, [hostId]);

  const handlePaired = useCallback((newToken: string) => {
    setRemoteAuthToken(newToken, hostId);
    setToken(newToken);
  }, [hostId]);

  const rollbackTransport = useCallback(() => {
    setTransport((current) => current.url === relayUrl ? current : relayEndpoint(relayUrl));
  }, [relayUrl]);

  useEffect(() => () => {
    workspaceRefreshVersionRef.current += 1;
    if (confirmationTimeoutRef.current !== null) clearTimeout(confirmationTimeoutRef.current);
  }, []);

  const loadWorkspace = useCallback(async (): Promise<RemoteWorkspaceModel | null> => {
    if (!token) return null;
    try {
      const response = await fetch(
        apiUrl(transport.url, `/api/v1/workspace/state?token=${encodeURIComponent(token)}`),
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
  }, [disconnect, token, transport.url]);

  const refreshWorkspace = useCallback(async (): Promise<RemoteWorkspaceModel | null> => {
    const refreshVersion = workspaceRefreshVersionRef.current;
    const next = await loadWorkspace();
    if (!next || workspaceRefreshVersionRef.current !== refreshVersion) return null;
    setModel(next);
    return next;
  }, [loadWorkspace]);

  useEffect(() => {
    const hash = window.location.hash;
    if (!readUrlHints || !hash.startsWith("#pair=")) return;

    const code = new URLSearchParams(hash.slice(1)).get("pair");
    if (!code || !/^\d{6}$/.test(code)) return;
    let cancelled = false;
    fetch(apiUrl(relayUrl, "/api/v1/pair/exchange"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        code,
        deviceName: navigator.userAgent.includes("Mobile") ? "Mobile Device" : "Browser Device",
      }),
    })
      .then((response) => response.json())
      .then((data) => {
        if (cancelled || typeof data.token !== "string") return;
        handlePaired(data.token);
        window.location.hash = "";
      })
      .catch((error) => console.warn("QR pairing failed", error));
    return () => { cancelled = true; };
  }, [handlePaired, readUrlHints, relayUrl]);

  useEffect(() => {
    if (token) void refreshWorkspace();
  }, [refreshWorkspace, token]);

  // Background upgrade: the relay session above is already live, so a failed or slow
  // probe costs nothing but a stay on the relay. Runs once per token, and not at all
  // when the desktop published no direct endpoint hints.
  useEffect(() => {
    if (!token || typeof fetch !== "function") return;
    const candidates = directHints;
    if (candidates.length === 0) return;
    let cancelled = false;
    void selectBestDirectCandidate(candidates).then((best) => {
      if (cancelled || !best) return;
      setTransport((current) => (current.url === best.url ? current : best));
    });
    return () => {
      cancelled = true;
    };
  }, [directHints, token]);

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

  const clearPendingSelection = useCallback((retryFailedSocket = false) => {
    if (confirmationTimeoutRef.current !== null) {
      clearTimeout(confirmationTimeoutRef.current);
      confirmationTimeoutRef.current = null;
    }
    pendingSelectionRef.current = null;
    selectionRequestAcceptedRef.current = false;
    selectionEventReceivedRef.current = false;
    confirmationInFlightRef.current = false;
    setPending(null);
    setOptimisticSessionId(null);
    if (retryFailedSocket && optimisticSocketClosedRef.current) {
      setTerminalRetryGeneration((generation) => generation + 1);
    }
    if (!retryFailedSocket) optimisticSocketSessionIdRef.current = null;
    optimisticSocketClosedRef.current = false;
  }, []);

  // Switching the active host is a connection change: any pending selection or optimistic
  // terminal socket belonged to the previous connection and must be dropped before the
  // workspace state for the newly active host is loaded. The initial mount is skipped since
  // the token-load effect above already fetches the first workspace snapshot.
  const previousHostIdRef = useRef(remoteHostState.activeHostId);
  useEffect(() => {
    if (previousHostIdRef.current === remoteHostState.activeHostId) return;
    previousHostIdRef.current = remoteHostState.activeHostId;
    if (!token) return;
    clearPendingSelection();
    setModel(EMPTY_MODEL);
    workspaceRefreshVersionRef.current += 1;
    void refreshWorkspace();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [remoteHostState.activeHostId]);

  const handleTerminalSocketLifecycle = useCallback((sessionId: string, state: "open" | "closed") => {
    if (optimisticSocketSessionIdRef.current !== sessionId) return;
    if (state === "open") {
      optimisticSocketClosedRef.current = false;
      return;
    }
    if (pendingSelectionRef.current) {
      optimisticSocketClosedRef.current = true;
    } else {
      optimisticSocketSessionIdRef.current = null;
      setTerminalRetryGeneration((generation) => generation + 1);
    }
  }, []);

  const confirmSelection = useCallback(async (option: RemoteContextOption) => {
    if (confirmationInFlightRef.current) return;
    confirmationInFlightRef.current = true;
    const confirmed = await refreshWorkspace();
    confirmationInFlightRef.current = false;
    if (pendingSelectionRef.current !== option) return;
    if (confirmed && modelConfirmsSelection(option, confirmed)) {
      clearPendingSelection(true);
    }
  }, [clearPendingSelection, refreshWorkspace]);

  useEffect(() => {
    if (!token || typeof WebSocket === "undefined") return;
    let socket: WebSocket | null = null;
    let retry: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    let disposed = false;
    const onMessage = (event: MessageEvent) => {
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
        clearPendingSelection();
        void refreshWorkspace();
        return;
      }
      selectionEventReceivedRef.current = true;
      if (selectionRequestAcceptedRef.current) void confirmSelection(pendingSelection);
    };
    const connect = () => {
      if (disposed) return;
      retry = null;
      let current: WebSocket;
      try {
        current = new WebSocket(eventsSocketUrl(token, transport.url));
      } catch (error) {
        if (transport.url !== relayUrl) rollbackTransport();
        else console.warn("Event socket connection failed", error);
        return;
      }
      current.onerror = () => {
        if (!disposed && socket === current && transport.url !== relayUrl) rollbackTransport();
      };
      socket = current;
      current.onmessage = (event) => {
        if (!disposed && socket === current) onMessage(event);
      };
      current.onopen = () => {
        if (disposed || socket !== current) return;
        attempt = 0;
        workspaceRefreshVersionRef.current += 1;
        const pendingSelection = pendingSelectionRef.current;
        if (pendingSelection) void confirmSelection(pendingSelection);
        else void refreshWorkspace();
      };
      current.onclose = () => {
        if (disposed || socket !== current) return;
        socket = null;
        if (transport.url !== relayUrl) {
          rollbackTransport();
          return;
        }
        retry = setTimeout(connect, Math.min(10000, 1000 * 2 ** attempt));
        attempt = Math.min(attempt + 1, 4);
      };
    };
    const recover = () => {
      if (document.visibilityState === "hidden") return;
      if (!socket) {
        if (retry !== null) clearTimeout(retry);
        connect();
      } else {
        void refreshWorkspace();
      }
    };
    connect();
    window.addEventListener("online", recover);
    document.addEventListener("visibilitychange", recover);
    return () => {
      disposed = true;
      if (retry !== null) clearTimeout(retry);
      socket?.close();
      window.removeEventListener("online", recover);
      document.removeEventListener("visibilitychange", recover);
    };
  }, [clearPendingSelection, confirmSelection, refreshWorkspace, token, transport.url, relayUrl, rollbackTransport]);

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
    optimisticSocketClosedRef.current = false;
    setPending(option);
    if (option.sessionId) {
      optimisticSocketSessionIdRef.current = option.sessionId;
      setOptimisticSessionId(option.sessionId);
    }

    try {
      const response = await fetch(
        apiUrl(transport.url, `/api/v1/workspace/select?token=${encodeURIComponent(token)}`),
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            workspaceId: option.workspaceId,
            ...(option.worktreeSlug ? { worktreeSlug: option.worktreeSlug } : {}),
            ...(option.tabId ? { tabId: option.tabId } : {}),
            ...(!option.tabId && option.sessionId ? { sessionId: option.sessionId } : {}),
          }),
        },
      );
      if (!response.ok) throw new Error(`Selection failed (${response.status})`);

      selectionRequestAcceptedRef.current = true;
      armConfirmationTimeout(option);
      if (selectionEventReceivedRef.current) void confirmSelection(option);
    } catch {
      clearPendingSelection();
    }
  }, [armConfirmationTimeout, clearPendingSelection, confirmSelection, pending, token, transport.url]);

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
        sessionId: prevTab.sessionId,
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
        sessionId: nextTab.sessionId,
      });
    }
  }, [currentIndex, model.context.workspaceId, model.context.worktreeLabel, model.context.worktreeSlug, selectContext, tabs]);

  if (!token) return <PairingPage onPaired={handlePaired} transportUrl={relayUrl} />;

  const activeTerminal = model.context.activeTerminal;
  const effectiveSessionId = optimisticSessionId ?? activeTerminal?.sessionId ?? null;
  const waitingTargets = collectWaitingTargets(model);
  const firstWaiting = waitingTargets.length > 0 ? waitingTargets[0] : null;
  const waitingCount = waitingTargets.length;
  const attentionAriaLabel = firstWaiting
    ? formatAttentionAriaLabel(firstWaiting, model.context, waitingCount)
    : "";

  return (
    <div className="flex h-[100dvh] min-h-screen flex-col overflow-hidden bg-background text-foreground">
      <Toaster />
      <header className="flex h-7 shrink-0 items-center justify-between border-b border-border bg-card px-2.5">
        <button
          type="button"
          aria-label="Change workspace context"
          aria-expanded={selectorOpen}
          onClick={() => setSelectorOpen((open) => !open)}
          className="flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden rounded px-1 py-0.5 -mx-1 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <span className="flex size-4 shrink-0 items-center justify-center rounded bg-primary text-[10px] font-bold text-primary-foreground" aria-hidden="true">F</span>
          {/* The brand word is the first thing to go when the status cluster grows;
              the workspace context stays legible longer than the app name. */}
          <span className="hidden shrink-0 text-xs font-semibold leading-none sm:inline">Ferryx Remote</span>
          <span className="min-w-0 truncate font-mono text-[11px] leading-none text-muted-foreground" aria-label="Current desktop context">{contextName(model.context)}</span>
          <ChevronDown aria-hidden="true" className={`size-3 shrink-0 text-muted-foreground transition-transform ${selectorOpen ? "rotate-180" : ""}`} />
        </button>
        <div className="flex shrink-0 items-center gap-1.5">
          <span
            data-testid="remote-connection-badge"
            data-connection={transport.type}
            aria-label={`Connection: ${CONNECTION_BADGE_LABEL[transport.type]}`}
            title={`Connection: ${CONNECTION_BADGE_LABEL[transport.type]}`}
            className={`flex h-5 shrink-0 items-center gap-1 rounded px-1.5 text-[11px] font-medium leading-none ${
              transport.type === "relay"
                ? "bg-status-idle/15 text-muted-foreground"
                : "bg-status-success/15 text-status-success"
            }`}
          >
            <span
              className={`size-1.5 shrink-0 rounded-full ${
                transport.type === "relay" ? "bg-status-idle" : "bg-status-success"
              }`}
              aria-hidden="true"
            />
            <span className="hidden sm:inline">{CONNECTION_BADGE_LABEL[transport.type]}</span>
            <span className="sm:hidden">
              {transport.type === "relay" ? "Relay" : transport.type === "lan" ? "LAN" : "Tailscale"}
            </span>
          </span>
          <button
            type="button"
            aria-label="Switch host"
            aria-haspopup="dialog"
            aria-expanded={hostDrawerOpen}
            data-testid="mobile-host-drawer-trigger"
            onClick={() => setHostDrawerOpen(true)}
            className="flex h-5 items-center gap-1 rounded px-1.5 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            {activeHost ? (
              <span
                data-testid="active-host-online-indicator"
                className={`size-1.5 shrink-0 rounded-full ${activeHost.online ? "bg-status-success" : "bg-status-idle"}`}
                aria-hidden="true"
              />
            ) : (
              <Laptop className="size-3 shrink-0" aria-hidden="true" />
            )}
            <span data-testid="active-host-name" className="max-w-20 truncate sm:max-w-32">
              {activeHost ? activeHost.name : "Local"}
            </span>
            {hostAgentSummary.waiting > 0 ? (
              <span
                data-testid="host-agent-status-pill"
                aria-label={`${hostAgentSummary.waiting} agent${hostAgentSummary.waiting === 1 ? "" : "s"} waiting`}
                className="flex items-center gap-1 rounded bg-status-warning/15 px-1 text-[10px] font-mono leading-tight text-status-warning"
              >
                <span className="size-1.5 rounded-full bg-status-warning ring-2 ring-status-warning/20" aria-hidden="true" />
                {hostAgentSummary.waiting}
              </span>
            ) : hostAgentSummary.running > 0 ? (
              <span
                data-testid="host-agent-status-pill"
                aria-label={`${hostAgentSummary.running} agent${hostAgentSummary.running === 1 ? "" : "s"} running`}
                className="flex items-center gap-1 rounded bg-status-working/15 px-1 text-[10px] font-mono leading-tight text-status-working"
              >
                {hostAgentSummary.running}
              </span>
            ) : null}
          </button>
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
                  sessionId: firstWaiting.sessionId,
                });
                document.querySelector<HTMLTextAreaElement>('textarea[data-testid="remote-terminal-input-sink"]')?.focus({ preventScroll: true });
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
          {confirmDisconnect ? (
            <>
              <button
                type="button"
                aria-label="Confirm disconnect. This removes pairing from this device; re-pair with a QR code to reconnect."
                onClick={disconnect}
                className="flex h-5 items-center rounded px-1.5 text-[11px] font-medium text-destructive transition-colors hover:bg-destructive/10 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                Remove pairing?
              </button>
              <button
                type="button"
                onClick={() => setConfirmDisconnect(false)}
                className="flex h-5 items-center rounded px-1.5 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                Cancel
              </button>
            </>
          ) : (
            <button
              type="button"
              onClick={() => setConfirmDisconnect(true)}
              className="flex h-5 items-center rounded px-1.5 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              Disconnect
            </button>
          )}
        </div>
      </header>

      <RemoteWorkspaceMirror
        model={model}
        pending={pending}
        selectorOpen={selectorOpen}
        onSelectorOpenChange={setSelectorOpen}
        onSelect={(option) => void selectContext(option)}
      >
        {effectiveSessionId ? (
          <RemoteTerminal
            key={`${effectiveSessionId}:${terminalRetryGeneration}`}
            sessionId={effectiveSessionId}
            token={token}
            transportUrl={transport.url}
            onTransportFailure={transport.url !== relayUrl ? rollbackTransport : undefined}
            activeTabId={model.context.activeTabId}
            onBack={() => undefined}
            embedded
            onSwipePreviousTab={handleSwipePreviousTab}
            onSwipeNextTab={handleSwipeNextTab}
            onSocketLifecycle={handleTerminalSocketLifecycle}
          />
        ) : null}
      </RemoteWorkspaceMirror>

      <MobileHostDrawer open={hostDrawerOpen} onOpenChange={setHostDrawerOpen} />
    </div>
  );
};
