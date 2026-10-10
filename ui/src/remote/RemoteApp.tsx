import { ChevronDown } from "lucide-react";
import React, { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Toaster } from "../components/ui/sonner";
import {
  DEFAULT_PROBE_TIMEOUT_MS,
  normalizeDirectCandidateOrigin,
  type CandidateEndpoint,
  type CandidateEndpointType,
} from "../lib/directPathUpgrade";
import {
  clearRemoteAuthToken,
  getRemoteAuthToken,
  setRemoteAuthToken,
} from "../lib/remoteClient";
import { remoteHostKey, remoteHostStore, selectActiveHost } from "../state/remoteHostStore";
import { getOrCreateInstallationId } from "../lib/storageKeys";
import { MobileHostDrawer } from "./MobileHostDrawer";
import { suggestDeviceName } from "./deviceIdentity";
import {
  contextName,
  getRemoteDocumentTitle,
  mirrorInventory,
  normalizeRemoteWorkspaceState,
  renderedPaneOrder,
  RemoteWorkspaceMirror,
  type RemoteContextOption,
  type RemoteTerminalTabInfo,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";
import { fetchAgentConversation, ConversationFetchError, mapAgentConversation, formatWorkedDuration, capRetainedMessages } from "./agentConversation";
import type { MobileChatMessageProps } from "./chat/MobileChatMessage";
import type { ChatAttachment as ComposerAttachment } from "./chat/MobileChatComposer";
import { hostTransportUrl, remoteApiUrl as apiUrl, remoteSocketUrl } from "./remoteClient";
import type { WebSocketLike } from "./RemoteTerminal";
import { AccountLoginPage } from "./AccountLoginPage";
import {
  useAccountWorktrees,
  type AccountWorktreeOption,
} from "./useAccountWorktrees";
import {
  mergeSessionInventory,
  parseInventoryEvent,
  parseMachineAgentStateFrame,
  parseSessionsPayload,
  preferredSessionForWorktree,
  selectInventorySession,
  type RemoteSessionInventory,
  type RemoteSessionInventoryEntry,
} from "./remoteSessionInventory";

const RemoteTerminal = lazy(() =>
  import("./RemoteTerminal").then((m) => ({ default: m.RemoteTerminal }))
);

const RemoteBrowserWorkspace = lazy(() =>
  import("./RemoteBrowserWorkspace").then((m) => ({ default: m.RemoteBrowserWorkspace }))
);

const MobileChatWorkspace = lazy(() =>
  import("./chat/MobileChatWorkspace").then((m) => ({ default: m.MobileChatWorkspace }))
);
import {
  getStoredAccountSessionToken,
  getStoredAccountTokenOrigin,
  clearStoredAccountSessionToken,
  clearAccountPreferredSessions,
  getAccountPreferredSessionId,
  setAccountPreferredSessionId,
  getAccountLastSelectedTarget,
  setAccountLastSelectedTarget,
  clearAccountLastSelectedTarget,
  logoutAccountSession,
  createAccountConnection,
  type AccountConnection,
} from "./accountSession";
import type { TunnelWebSocket } from "./attachTunnel";

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
        const worktreeSlug = tab.worktreeSlug ?? currentWorktreeSlug;
        const key = `${currentWorkspaceId}\u0000${worktreeSlug ?? ""}\u0000${tab.id}`;
        if (!seen.has(key)) {
          seen.add(key);
          targets.push({
            tabId: tab.id,
            sessionId: tab.sessionId,
            label: tab.label,
            workspaceId: currentWorkspaceId,
            worktreeSlug,
            worktreeLabel: tab.worktreeLabel ?? currentWorktreeLabel,
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

export { formatWorkedDuration } from "./agentConversation";

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

/* Read-only desktop-context sync policy: the machine's published active context may
   only follow the machine the current context belongs to. It never runs while a stored
   target is still in flight, never replaces an independently chosen session, and never
   reads through a connection for a different machine (a lingering root tab from another
   host must not leak into the chosen machine's picker). */
export function desktopContextSyncDecision(input: {
  hasToken: boolean;
  connectionMachineId: string | null;
  contextMachineId: string | null;
  targetInFlight: boolean;
  independentSessionChosen: boolean;
}): boolean {
  return (
    input.hasToken &&
    input.connectionMachineId !== null &&
    input.connectionMachineId === input.contextMachineId &&
    !input.targetInFlight &&
    !input.independentSessionChosen
  );
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
 * or a previously stored hint). The gateway health response proves reachability but
 * not possession of the paired machine identity, so these hints remain untrusted and
 * the active transport stays on the relay.
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

function hasMagicLinkCode(): boolean {
  if (typeof window === "undefined") return false;
  const parseCode = (searchOrHash: string): string | null => {
    if (!searchOrHash) return null;
    const str = searchOrHash.startsWith("#") || searchOrHash.startsWith("?") ? searchOrHash.slice(1) : searchOrHash;
    const params = new URLSearchParams(str);
    const code = params.get("code") ?? params.get("login") ?? params.get("account_token");
    return code && code.trim().length > 0 ? code.trim() : null;
  };

  return Boolean(parseCode(window.location.hash) || parseCode(window.location.search));
}

export const RemoteApp: React.FC = () => {
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const [pairingHash, setPairingHash] = useState(window.location.hash);
  const [magicLinkOverridden, setMagicLinkOverridden] = useState(() => hasMagicLinkCode());
  useEffect(() => {
    const onHashChange = () => setPairingHash(window.location.hash);
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);
  const pairingRequested = /^#pair=([0-9a-fA-F]{32}|[0-9]{6})(?:&|$)/i.test(pairingHash);
  const bypassSavedHost = pairingRequested || magicLinkOverridden;
  const host = bypassSavedHost ? null : selectActiveHost(state);
  const hostId = (bypassSavedHost ? null : state.activeHostId) ?? `local:${window.location.origin}`;
  const address = host?.address ?? window.location.origin;
  const relayUrl = new URL(address.includes("://") ? address : `http://${address}`).origin;
  return (
    <RemoteHostConnection
      key={`${hostId}:${relayUrl}:${pairingRequested ? pairingHash : ""}`}
      hostId={hostId}
      relayUrl={relayUrl}
      readUrlHints={pairingRequested || state.activeHostId === null || magicLinkOverridden}
      initialMagicLinkRequested={magicLinkOverridden}
      onMagicLinkConsumed={() => {
        remoteHostStore.setActiveHost(null);
        setMagicLinkOverridden(false);
      }}
    />
  );
};

interface MinimalMachineSessionTarget {
  machineId: string;
  sessionId: string;
  daemonEpoch?: string | number | null;
}

interface MinimalMachineSession {
  workspaceId: string;
  worktree: { wsId: string; slug: string } | null;
  target?: MinimalMachineSessionTarget | null;
  sessionId?: string | null;
  daemonEpoch?: string | number | null;
  running?: boolean;
  title?: string | null;
  agentType?: string | null;
}

/** Tab strip for one machine worktree, built from the merged inventory so every session
 * the machine still reports is listed and each state stays attached to its own epoch. */
function machineTabsFromInventory(
  entries: readonly RemoteSessionInventoryEntry[],
  workspaceId: string | null,
  worktreeSlug: string | null,
  worktreeLabel: string | null,
): RemoteTerminalTabInfo[] {
  return entries.flatMap((entry) => {
    if (workspaceId && entry.workspaceId !== workspaceId) return [];
    if ((entry.worktreeSlug ?? null) !== (worktreeSlug ?? null)) return [];
    return [{
      id: entry.sessionId,
      label: entry.title ?? "Terminal",
      sessionId: entry.sessionId,
      ...(worktreeSlug ? { worktreeSlug } : {}),
      ...(worktreeLabel ? { worktreeLabel } : {}),
      ...(entry.daemonEpoch ? { daemonEpoch: entry.daemonEpoch } : {}),
      ...(entry.activityState ? { activityState: entry.activityState } : {}),
      ...(entry.agentType ? { agentType: entry.agentType } : {}),
    }];
  });
}

export const RemoteHostConnection: React.FC<{
  hostId: string;
  relayUrl: string;
  readUrlHints: boolean;
  initialMagicLinkRequested?: boolean;
  onMagicLinkConsumed?: () => void;
}> = ({ hostId, relayUrl, readUrlHints, initialMagicLinkRequested = false, onMagicLinkConsumed }) => {
  const [magicLinkActive, setMagicLinkActive] = useState(initialMagicLinkRequested);
  const [token, setToken] = useState<string | null>(() => {
    if (initialMagicLinkRequested) return null;
    if (readUrlHints && /^#pair=([0-9a-fA-F]{32}|[0-9]{6})(?:&|$)/i.test(window.location.hash)) return null;
    const storedHost = remoteHostStore.getState().hosts[hostId];
    const scoped = storedHost?.deviceToken ?? getRemoteAuthToken(hostId);
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
  const [directHints] = useState(() => {
    const stored = remoteHostStore.getState().hosts[hostId]?.directHints;
    return stored?.length ? stored : readDirectCandidateHints(hostId, readUrlHints);
  });
  const [model, setModel] = useState<RemoteWorkspaceModel>(EMPTY_MODEL);
  const modelRef = useRef(model);
  const modelMachineIdRef = useRef<string | null>(null);
  // Set by every user-driven selection that names a session or tab: the read-only
  // desktop-context sync must never replace an independently chosen session.
  const independentSessionRef = useRef(false);
  modelRef.current = model;
  const [pending, setPending] = useState<RemoteContextOption | null>(null);
  const [selectorOpen, setSelectorOpen] = useState(false);
  const [creationError, setCreationError] = useState<string | null>(null);
  const creationSessionsRef = useRef<Set<string> | null>(null);
  const [viewportHeight, setViewportHeight] = useState(() => window.visualViewport?.height);
  useEffect(() => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const resize = () => setViewportHeight(viewport.height);
    viewport.addEventListener("resize", resize);
    return () => viewport.removeEventListener("resize", resize);
  }, []);
  const [hostDrawerOpen, setHostDrawerOpen] = useState(false);
  const [viewMode, setViewMode] = useState<"chat" | "terminal" | "browser">(() => (typeof window !== "undefined" && window.innerWidth > 0 && window.innerWidth < 768 ? "chat" : "terminal"));
  const [chatMessages, setChatMessages] = useState<MobileChatMessageProps[]>([]);
  const chatAttachmentUrlsRef = useRef<Set<string>>(new Set());

  const revokeChatAttachmentUrls = useCallback(() => {
    chatAttachmentUrlsRef.current.forEach((url) => {
      URL.revokeObjectURL(url);
    });
    chatAttachmentUrlsRef.current.clear();
  }, []);

  useEffect(() => {
    return () => {
      revokeChatAttachmentUrls();
    };
  }, [revokeChatAttachmentUrls]);
  const lastConversationSessionRef = useRef<string | null>(null);
  const retentionTruncatedRef = useRef(false);
  const [chatIsRunning, setChatIsRunning] = useState(false);
  const [chatWarnings, setChatWarnings] = useState<readonly string[]>([]);
  const [browserSessions, setBrowserSessions] = useState<Array<{ browserId: string; title?: string; url?: string }>>([]);
  const [selectedBrowserId, setSelectedBrowserId] = useState<string | null>(null);
  // First render always speaks to the relay; a verified probe swaps this for a direct endpoint.
  const [transport, setTransport] = useState<CandidateEndpoint>(() => relayEndpoint(relayUrl));
  const remoteHostState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const activeHost = remoteHostState.hosts[hostId] ?? null;
  const pairingBaseUrl = hostTransportUrl(activeHost, relayUrl);
  const [optimisticSessionId, setOptimisticSessionId] = useState<string | null>(null);
  const [terminalRetryGeneration, setTerminalRetryGeneration] = useState(0);
  const pendingSelectionRef = useRef<RemoteContextOption | null>(null);
  const optimisticSocketSessionIdRef = useRef<string | null>(null);
  const optimisticSocketClosedRef = useRef(false);
  const selectionRequestAcceptedRef = useRef(false);
  const selectionEventReceivedRef = useRef(false);
  const confirmationInFlightRef = useRef(false);
  const confirmationRequeuedRef = useRef(false);
  const workspaceRefreshVersionRef = useRef(0);
  const confirmationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const assistantTurnStartedAtRef = useRef<number | null>(null);
  const lastAgentActivityRef = useRef<string | null>(null);

  const [initialAccountTarget, setInitialAccountTarget] = useState<{
    machineId: string;
    workspaceId: string;
    worktreeSlug: string | null;
    worktreeLabel: string | null;
    /* Ephemeral: the exact session the user picked. It is never persisted (the stored
       last-selected target stays the canonical four fields), but it must survive the
       acquire-connection hop so the issued selection attaches to that session instead
       of the worktree's first one. */
    sessionId?: string | null;
    daemonEpoch?: string | null;
  } | null>(null);
  // Read by selection callbacks so their identities (and the event socket that depends
  // on them) do not churn every time the account target changes.
  const initialAccountTargetRef = useRef(initialAccountTarget);
  initialAccountTargetRef.current = initialAccountTarget;

  const [accountSessionToken, setAccountSessionToken] = useState<string | null>(
    () => (initialMagicLinkRequested ? null : getStoredAccountSessionToken(relayUrl)),
  );
  const [activeTunnelConnection, setActiveTunnelConnection] = useState<AccountConnection | null>(null);
  // Account mode has no remoteHostStore record, so hostTransportUrl would leave this
  // pointing at the relay root. Every machine API then 404s there (the relay only serves
  // them under /host/<machineId>), which is why the remote chat view and workspace refresh
  // stayed empty. Derive the host-scoped base URL from the connected machine instead.
  const accountHostBaseUrl = activeTunnelConnection && !activeHost
    ? `${relayUrl}/host/${encodeURIComponent(activeTunnelConnection.machine.machineId)}`
    : null;
  const transportBaseUrl = accountHostBaseUrl ?? hostTransportUrl(activeHost, transport.url);
  // The transferred tunnel is owned here: close it when it is replaced or on unmount.
  // AccountConnection.close is idempotent, so explicit closes elsewhere are safe.
  useEffect(() => {
    if (!activeTunnelConnection) return;
    return () => activeTunnelConnection.close();
  }, [activeTunnelConnection]);

  const disconnect = useCallback(() => {
    if (activeTunnelConnection) {
      activeTunnelConnection.close();
      setActiveTunnelConnection(null);
    }
    setInitialAccountTarget(null);
    clearRemoteAuthToken(hostId);
    const host = remoteHostStore.getState().hosts[hostId];
    if (host) remoteHostStore.upsertHost({ ...host, deviceToken: null, authStatus: "unpaired" });
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
  }, [activeTunnelConnection, hostId]);

  const accountSelectionGenerationRef = useRef(0);
  const accountSessionTokenRef = useRef<string | null>(accountSessionToken);
  accountSessionTokenRef.current = accountSessionToken;

  const handleLogout = useCallback(() => {
    accountSelectionGenerationRef.current += 1;
    accountSessionTokenRef.current = null;
    clearStoredAccountSessionToken();
    clearAccountPreferredSessions();
    clearAccountLastSelectedTarget();
    setAccountSessionToken(null);
    disconnect();
  }, [disconnect]);

  const handleSignOut = useCallback(() => {
    if (accountSessionToken) {
      const issuer = getStoredAccountTokenOrigin() || relayUrl;
      void logoutAccountSession(issuer, accountSessionToken);
    }
    handleLogout();
  }, [accountSessionToken, handleLogout, relayUrl]);

  const accountDiscovery = useAccountWorktrees(
    relayUrl,
    accountSessionToken,
    Boolean(accountSessionToken),
    handleLogout,
  );
  const {
    applyInventoryEvent,
    noteSessionActivity,
    noteSessionsPayload,
  } = accountDiscovery;
  const accountDiscoveryRef = useRef(accountDiscovery);
  accountDiscoveryRef.current = accountDiscovery;

  /* The machine inventory is read from effects and socket callbacks that are declared
     before the derived model below, so it is mirrored into a ref every render. */
  const inventoryRef = useRef<RemoteSessionInventory | null>(null);
  const inventoryRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scheduleInventoryRefreshRef = useRef<() => void>(() => {});
  // Re-arms the read-only desktop-context sync when a same-machine inventory refresh
  // (session started/exited boundary) lands: same-context tab changes are re-read
  // without ever selecting desktop focus.
  const [desktopSyncTick, setDesktopSyncTick] = useState(0);
  // Always-follow: bumped when the machine publishes a desktop focus change, and when a
  // follow attempt had to wait for an in-flight selection to settle.
  const [desktopFollowTick, setDesktopFollowTick] = useState(0);
  const desktopFollowDeferredRef = useRef(false);
  const lastFollowedRef = useRef<{ connection: unknown; key: string } | null>(null);
  // Set only by the automatic page-load restore: the desktop focus wins over the stored
  // target. An explicit pick instead baselines the desktop focus it connected under.
  const restoreFollowRef = useRef(false);

  // Auto-restore last explicitly selected account target upon page load / refresh
  const restoreAttemptInFlightRef = useRef(false);
  const restoreAttemptedForTokenRef = useRef<string | null>(null);
  useEffect(() => {
    if (!accountSessionToken) {
      restoreAttemptedForTokenRef.current = null;
      restoreAttemptInFlightRef.current = false;
      return;
    }
    if (token || activeTunnelConnection || restoreAttemptInFlightRef.current) return;
    if (!accountDiscovery.initialized || accountDiscovery.loading || accountDiscovery.error) return;

    const storedTarget = getAccountLastSelectedTarget(relayUrl);
    if (!storedTarget) return;

    // Check if machine is known
    const targetMachine = accountDiscovery.machines.find((m) => m.machineId === storedTarget.machineId);
    if (!targetMachine) {
      // Complete successful authoritative inventory with zero error: target machine does not exist on this account
      if (accountDiscovery.initialized && !accountDiscovery.loading && !accountDiscovery.error) {
        clearAccountLastSelectedTarget(relayUrl);
      }
      return;
    }

    // If target machine is currently offline, preserve target without clearing
    if (targetMachine.online === false) {
      return;
    }

    const machineStatus = accountDiscovery.machineStatuses[storedTarget.machineId];
    // If still probing/tunneling/idle, wait for completion
    if (!machineStatus || machineStatus.status === "tunneling" || machineStatus.status === "idle") {
      return;
    }

    // If machine probe failed transiently (error / offline), preserve target without clearing
    if (machineStatus.status === "error" || machineStatus.status === "offline") {
      return;
    }

    // Machine probe succeeded ("ready"): check for matching worktree option
    if (machineStatus.status === "ready") {
      const matchingOpt = accountDiscovery.accountOptions.find(
        (opt) =>
          opt.machineId === storedTarget.machineId &&
          opt.workspaceId === storedTarget.workspaceId &&
          (opt.worktreeSlug ?? null) === (storedTarget.worktreeSlug ?? null),
      );

      if (matchingOpt) {
        if (restoreAttemptedForTokenRef.current !== accountSessionToken) {
          restoreAttemptInFlightRef.current = true;
          restoreFollowRef.current = true;
          void selectAccountOption(matchingOpt).then((ok) => {
            restoreAttemptInFlightRef.current = false;
            if (ok) {
              restoreAttemptedForTokenRef.current = accountSessionToken;
            }
          });
        }
      }
    }
  }, [
    accountSessionToken,
    token,
    activeTunnelConnection,
    accountDiscovery.initialized,
    accountDiscovery.loading,
    accountDiscovery.error,
    accountDiscovery.machines,
    accountDiscovery.machineStatuses,
    accountDiscovery.accountOptions,
    relayUrl,
  ]);

  const sessionEpochsRef = useRef<Map<string, string>>(new Map());
  const [sessionEpochs, setSessionEpochs] = useState<Record<string, string>>({});
  const sessionEpochMissesRef = useRef<Map<string, number>>(new Map());

  const getSessionDaemonEpoch = useCallback(async (sessionId: string): Promise<string | null> => {
    const cached = sessionEpochsRef.current.get(sessionId);
    if (cached) return cached;

    const now = Date.now();
    const lastMiss = sessionEpochMissesRef.current.get(sessionId);
    if (lastMiss && now - lastMiss < 5000) {
      return null;
    }

    if (activeTunnelConnection && token) {
      try {
        const sessRes = await activeTunnelConnection.transport.fetchLike("/api/v1/sessions", {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (sessRes.status >= 200 && sessRes.status < 300) {
          const sessText = new TextDecoder().decode(sessRes.body);
          const sessData = JSON.parse(sessText);
          const rows = Array.isArray(sessData) ? sessData : Array.isArray(sessData?.sessions) ? sessData.sessions : [];
          const newEpochs: Record<string, string> = {};
          for (const s of rows) {
            const sid = s.sessionId ?? s.session_id ?? s.target?.sessionId;
            const epoch = s.daemonEpoch ?? s.target?.daemonEpoch;
            if (sid && epoch !== undefined && epoch !== null) {
              sessionEpochsRef.current.set(sid, String(epoch));
              newEpochs[sid] = String(epoch);
            }
          }
          if (Object.keys(newEpochs).length > 0) {
            setSessionEpochs((prev) => ({ ...prev, ...newEpochs }));
          }
        }
      } catch (err) {
        console.warn("Failed to fetch session daemonEpoch", err);
      }
    }

    const resolved = sessionEpochsRef.current.get(sessionId);
    if (resolved) {
      sessionEpochMissesRef.current.delete(sessionId);
      setSessionEpochs((prev) => (prev[sessionId] === resolved ? prev : { ...prev, [sessionId]: resolved }));
      return resolved;
    }
    sessionEpochMissesRef.current.set(sessionId, now);
    return null;
  }, [activeTunnelConnection, token]);

  // Read-only desktop-context sync (account route): after a fresh machine context is
  // established, follow that machine's published active context — validate the exact
  // session and daemon epoch against authoritative /api/v1/sessions, enrich tab activity
  // through noteContextTabs, and apply the active session only inside the workspace and
  // worktree the user already chose. Never POSTs /workspace/select; an explicit workspace,
  // a stored target in flight, or an independently chosen session always wins.
  useEffect(() => {
    if (!token || !activeTunnelConnection) return;
    const machineId = activeTunnelConnection.machine.machineId;
    const chosenWorkspace = model.context.workspaceId;
    const chosenWorktree = model.context.worktreeSlug;
    // Intentional empty initial account picker: no chosen context, no sync.
    if (!chosenWorkspace) return;
    if (
      !desktopContextSyncDecision({
        hasToken: true,
        connectionMachineId: machineId,
        contextMachineId: modelMachineIdRef.current,
        targetInFlight: initialAccountTargetRef.current !== null,
        independentSessionChosen: independentSessionRef.current,
      })
    ) return;
    let cancelled = false;
    void (async () => {
      try {
        const res = await activeTunnelConnection.transport.fetchLike("/api/v1/workspace/state", {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (cancelled || res.status < 200 || res.status >= 300) return;
        const state = normalizeRemoteWorkspaceState(JSON.parse(new TextDecoder().decode(res.body)));
        // Explicit workspace/worktree choice wins — compared exactly, including null:
        // a desktop active context from another worktree (or another shape of context)
        // is never applied.
        if (state.context.workspaceId !== chosenWorkspace) return;
        if (state.context.worktreeSlug !== chosenWorktree) return;
        const active = state.context.activeTerminal;
        const activeSessionId = active?.sessionId ?? null;
        const activeEpoch =
          state.context.daemonEpoch === null || state.context.daemonEpoch === undefined
            ? null
            : String(state.context.daemonEpoch);
        // Tab labels sync independently as soon as the active context matches the chosen
        // scope; they are activity observations and can never select a target.
        if (
          !cancelled &&
          desktopContextSyncDecision({
            hasToken: true,
            connectionMachineId: machineId,
            contextMachineId: modelMachineIdRef.current,
            targetInFlight: initialAccountTargetRef.current !== null,
            independentSessionChosen: independentSessionRef.current,
          })
        ) {
          accountDiscovery.noteContextTabs(machineId, state.context.terminalTabs);
        }
        // No epoch on the boundary means no fence: the apply path stays closed.
        if (!activeSessionId || activeEpoch === null) return;
        const sessRes = await activeTunnelConnection.transport.fetchLike("/api/v1/sessions", {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (cancelled || sessRes.status < 200 || sessRes.status >= 300) return;
        const sessData = JSON.parse(new TextDecoder().decode(sessRes.body));
        // Canonical parser: machine attribution, required running, canonical epoch.
        const parsedSessions = parseSessionsPayload(machineId, sessData);
        if (!parsedSessions.ok) return;
        const validated = parsedSessions.inventory.entries.some(
          (entry) =>
            entry.sessionId === activeSessionId &&
            entry.machineId === machineId &&
            entry.workspaceId === chosenWorkspace &&
            entry.worktreeSlug === chosenWorktree &&
            entry.running &&
            entry.daemonEpoch === activeEpoch,
        );
        // The user may have chosen a session (or a stored target may surface) while the
        // reads were in flight — refs are rechecked immediately before applying.
        if (
          !validated ||
          cancelled ||
          !desktopContextSyncDecision({
            hasToken: true,
            connectionMachineId: machineId,
            contextMachineId: modelMachineIdRef.current,
            targetInFlight: initialAccountTargetRef.current !== null,
            independentSessionChosen: independentSessionRef.current,
          })
        ) return;
        // Scope may have moved while the reads were in flight; React's updater rejects
        // that on prev, so reject it here before any trusted write as well.
        if (
          modelRef.current.context.workspaceId !== chosenWorkspace ||
          modelRef.current.context.worktreeSlug !== chosenWorktree
        ) return;
        // The pick confirmation persists its resolved default as this worktree's preferred
        // entry, and the follow-up refresh derives candidateSid from that preference ahead
        // of the model's own request. Persist the validated desktop active now, and mirror
        // the same context into modelRef for readers that run before the next render.
        setAccountPreferredSessionId(relayUrl, machineId, chosenWorkspace, chosenWorktree, activeSessionId);
        modelRef.current = {
          ...modelRef.current,
          context: {
            ...modelRef.current.context,
            activeTabId: state.context.activeTabId ?? modelRef.current.context.activeTabId,
            activeTerminal: active,
            daemonEpoch: state.context.daemonEpoch ?? modelRef.current.context.daemonEpoch,
          },
        };
        setModel((prev) => {
          if (prev.context.workspaceId !== chosenWorkspace) return prev;
          if (prev.context.worktreeSlug !== chosenWorktree) return prev;
          if (
            prev.context.activeTerminal?.sessionId === activeSessionId &&
            prev.context.activeTabId === state.context.activeTabId
          ) return prev;
          return {
            ...prev,
            context: {
              ...prev.context,
              activeTabId: state.context.activeTabId ?? prev.context.activeTabId,
              activeTerminal: active,
              daemonEpoch: state.context.daemonEpoch ?? prev.context.daemonEpoch,
            },
          };
        });
      } catch (error) {
        // Best effort by design: a transient or malformed read-only answer leaves the
        // current context untouched, but it is never silently swallowed.
        console.warn("[remote] desktop-context sync skipped", error);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    token,
    activeTunnelConnection,
    model.context.workspaceId,
    model.context.worktreeSlug,
    accountDiscovery.noteContextTabs,
    relayUrl,
    desktopSyncTick,
  ]);

  const terminalSocketRef = useRef<WebSocketLike | WebSocket | null>(null);
  const terminalSocketSessionIdRef = useRef<string | null>(null);

  const handlePaired = useCallback((newToken: string, metadata?: { machineId?: unknown; displayName?: unknown }) => {
    const machineId = optionalString(metadata?.machineId);
    const displayName = optionalString(metadata?.displayName);
    if (machineId && displayName) {
      remoteHostStore.upsertHost({
        machineId,
        displayName,
        relayOrigin: relayUrl,
        deviceToken: newToken,
        lastSeenAt: Date.now(),
        directHints,
      });
      remoteHostStore.setActiveHost(remoteHostKey(relayUrl, machineId));
      clearRemoteAuthToken(hostId);
      return;
    }
    const host = remoteHostStore.getState().hosts[hostId];
    if (host) {
      remoteHostStore.upsertHost({ ...host, deviceToken: newToken, lastSeenAt: Date.now(), authStatus: "paired" });
      clearRemoteAuthToken(hostId);
    } else setRemoteAuthToken(newToken, hostId);
    setToken(newToken);
  }, [directHints, hostId, relayUrl]);

  const rollbackTransport = useCallback(() => {
    setTransport((current) => current.url === relayUrl ? current : relayEndpoint(relayUrl));
  }, [relayUrl]);

  const fetchBrowserSessions = useCallback(async () => {
    if (!token) return;
    try {
      const wsId = model.context.workspaceId || "";
      const wtSlug = model.context.worktreeSlug || "";
      const params = new URLSearchParams();
      if (wsId) params.set("workspaceId", wsId);
      if (wtSlug) params.set("worktreeSlug", wtSlug);
      const query = params.toString() ? `?${params.toString()}` : "";
      const response = await fetch(apiUrl(transportBaseUrl, `/api/v1/browser/sessions${query}`), {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (response.ok) {
        const data = await response.json();
        if (Array.isArray(data)) {
          setBrowserSessions(data);
          if (data.length > 0 && !selectedBrowserId) {
            setSelectedBrowserId(data[0].browserId);
          }
        }
      }
    } catch {
      // Ignore network errors
    }
  }, [token, model.context.workspaceId, model.context.worktreeSlug, transportBaseUrl, selectedBrowserId]);

  useEffect(() => () => {
    workspaceRefreshVersionRef.current += 1;
    pendingSelectionRef.current = null;
    if (confirmationTimeoutRef.current !== null) clearTimeout(confirmationTimeoutRef.current);
  }, []);

  const loadWorkspace = useCallback(async (): Promise<RemoteWorkspaceModel | null> => {
    if (!token) return null;
    try {
      if (activeTunnelConnection) {
        // In machine account mode, load state strictly from machine projects, worktrees and sessions.
        // Never query or overwrite with legacy desktop mirror /api/v1/workspace/state.
        const [projRes, sessRes] = await Promise.all([
          activeTunnelConnection.transport.fetchLike("/api/v1/workspace/projects", {
            headers: { Authorization: `Bearer ${token}` },
          }),
          activeTunnelConnection.transport.fetchLike("/api/v1/sessions", {
            headers: { Authorization: `Bearer ${token}` },
          }),
        ]);

        if (projRes.status < 200 || projRes.status >= 300 || sessRes.status < 200 || sessRes.status >= 300) {
          return null;
        }

        const projsData = projRes.status >= 200 && projRes.status < 300
          ? JSON.parse(new TextDecoder().decode(projRes.body))
          : null;
        const sessData = sessRes.status >= 200 && sessRes.status < 300
          ? JSON.parse(new TextDecoder().decode(sessRes.body))
          : null;

        if (!Array.isArray(sessData) && !Array.isArray(sessData?.sessions)) {
          return null;
        }

        const projectsList = Array.isArray(projsData?.projects) ? projsData.projects : [];
        const sessionRows: MinimalMachineSession[] = Array.isArray(sessData)
          ? sessData
          : sessData.sessions;

        const newEpochs: Record<string, string> = {};
        for (const s of sessionRows) {
          const sid = s.sessionId ?? s.target?.sessionId;
          const epoch = s.daemonEpoch ?? s.target?.daemonEpoch;
          if (sid && epoch !== undefined && epoch !== null) {
            sessionEpochsRef.current.set(sid, String(epoch));
            newEpochs[sid] = String(epoch);
          }
        }
        if (Object.keys(newEpochs).length > 0) {
          setSessionEpochs((prev) => ({ ...prev, ...newEpochs }));
        }

        const machineId = activeTunnelConnection.machine.machineId;
        modelMachineIdRef.current = machineId;
        noteSessionsPayload(machineId, sessData);
        const parsedInventory = parseSessionsPayload(machineId, sessData);
        const previousInventory = accountDiscoveryRef.current.sessionInventories[machineId] ?? null;
        /* Selection and tabs come from the merged inventory, never from this one partial
           answer: otherwise a response that omits the selected session would move the
           user to a different terminal. */
        const inventory = parsedInventory.ok
          ? mergeSessionInventory(previousInventory, parsedInventory.inventory)
          : previousInventory;

        // If an initial account target or selected model exists, retain or focus that machine context
        const currentTarget = initialAccountTargetRef.current;
        const currentModel = modelRef.current;
        const currentWs = currentTarget?.workspaceId ?? currentModel.context.workspaceId ?? projectsList[0]?.workspaceId ?? null;
        const currentSlug = currentTarget?.worktreeSlug ?? currentModel.context.worktreeSlug ?? null;
        const currentLabel = currentTarget?.worktreeLabel ?? currentModel.context.worktreeLabel ?? currentSlug ?? "main";
        const requestedSessionId = currentModel.context.activeTerminal?.sessionId ?? null;

        // The pending target's exact session outranks any remembered preference: a
        // background refresh must never resolve a chosen session to the worktree's first
        // one while the selection is still being issued.
        const preferredSid = currentWs
          ? getAccountPreferredSessionId(relayUrl, machineId, currentWs, currentSlug)
          : null;
        const pendingSid = currentTarget?.sessionId ?? null;
        const candidateSid = pendingSid ?? preferredSid ?? requestedSessionId;
        const candidateEpoch = pendingSid
          ? currentTarget?.daemonEpoch ?? sessionEpochsRef.current.get(pendingSid) ?? null
          : sessionEpochsRef.current.get(candidateSid ?? "") ?? null;

        const chosen = candidateSid && inventory
          ? selectInventorySession(inventory, {
              workspaceId: currentWs ?? "",
              worktreeSlug: currentSlug,
              sessionId: candidateSid,
              daemonEpoch: candidateEpoch,
              requireRunning: false,
            })
          : null;

        let sid: string | null = null;
        let activeTitle: string | null = null;
        let isRunning = false;
        if (candidateSid && chosen) {
          sid = chosen.entry.sessionId;
          activeTitle = chosen.entry.title;
          isRunning = chosen.entry.running;
        } else if (pendingSid) {
          /* The pending selection is not confirmed for this epoch. Stay gated: never
             synthesize an active terminal that the same-id confirmation could accept, and
             never fall through to a sibling session. A previously confirmed terminal for a
             different session stays only if it is the one already shown. */
          const previous = currentModel.context.activeTerminal;
          const samePrevious = previous && previous.sessionId === pendingSid ? previous : null;
          sid = samePrevious?.sessionId ?? null;
          activeTitle = samePrevious?.title ?? null;
          isRunning = samePrevious ? samePrevious.running !== false : false;
        } else if (candidateSid && inventory?.completeness !== "complete") {
          /* Absence in a partial or unknown answer is not evidence the terminal is gone,
             so the session the UI already shows stays selected. */
          sid = requestedSessionId;
          activeTitle = currentModel.context.activeTerminal?.title ?? null;
          isRunning = currentModel.context.activeTerminal?.running !== false;
        } else if (!candidateSid && currentWs) {
          const deterministic = inventory
            ? preferredSessionForWorktree(inventory, currentWs, currentSlug, [preferredSid, requestedSessionId])
            : null;
          if (deterministic) {
            sid = deterministic.sessionId;
            activeTitle = deterministic.title;
            isRunning = deterministic.running;
          }
        }

        return {
          context: {
            workspaceId: currentWs,
            worktreeSlug: currentSlug,
            worktreeLabel: currentLabel,
            activeTerminal: sid
              ? {
                  sessionId: sid,
                  running: isRunning,
                  title: activeTitle ?? "Terminal",
                  workspaceId: currentWs,
                  worktreeLabel: currentLabel,
                }
              : null,
            activeTabId: sid ?? null,
            terminalTabs: machineTabsFromInventory(inventory?.entries ?? [], currentWs, currentSlug, currentLabel),
          },
          options: accountDiscoveryRef.current.accountOptions,
        };
      }
      const response = await fetch(apiUrl(transportBaseUrl, "/api/v1/workspace/state"), {
        headers: { Authorization: `Bearer ${token}` },
      });
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
  }, [activeTunnelConnection, disconnect, relayUrl, token, transportBaseUrl]);

  const refreshWorkspace = useCallback(async (): Promise<RemoteWorkspaceModel | null> => {
    const refreshVersion = workspaceRefreshVersionRef.current;
    const next = await loadWorkspace();
    if (!next || workspaceRefreshVersionRef.current !== refreshVersion) return null;
    setModel(next);
    return next;
  }, [loadWorkspace]);

  /* An inventory boundary can arrive in bursts while worktrees change; coalescing keeps
     one workspace read in flight instead of one per frame. */
  const scheduleInventoryRefresh = useCallback(() => {
    if (inventoryRefreshTimerRef.current !== null) return;
    inventoryRefreshTimerRef.current = setTimeout(() => {
      inventoryRefreshTimerRef.current = null;
      void refreshWorkspace();
    }, 150);
  }, [refreshWorkspace]);
  scheduleInventoryRefreshRef.current = scheduleInventoryRefresh;

  useEffect(() => {
    const hash = window.location.hash;
    if (!readUrlHints || !hash.startsWith("#pair=")) return;

    const code = new URLSearchParams(hash.slice(1)).get("pair");
    if (!code || !/^([0-9a-fA-F]{32}|[0-9]{6})$/i.test(code)) return;
    let cancelled = false;
    const installationId = getOrCreateInstallationId();
    fetch(apiUrl(pairingBaseUrl, "/api/v1/pair/exchange"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        code,
        deviceName: suggestDeviceName(),
        installationId,
      }),
    })
      .then((response) => {
        if (!response.ok) throw new Error(`Pairing failed (${response.status})`);
        return response.json();
      })
      .then((data) => {
        if (cancelled || typeof data.token !== "string") return;
        handlePaired(data.token, data);
        if (window.history && typeof window.history.replaceState === "function") {
          window.history.replaceState(null, "", window.location.pathname + window.location.search);
        }
        window.location.hash = "";
      })
      .catch((error) => {
        console.warn("QR pairing failed", error);
        if (window.history && typeof window.history.replaceState === "function") {
          window.history.replaceState(null, "", window.location.pathname + window.location.search);
        }
        window.location.hash = "";
      });
    return () => { cancelled = true; };
  }, [handlePaired, readUrlHints, pairingBaseUrl]);

  useEffect(() => {
    if (token) void refreshWorkspace();
  }, [refreshWorkspace, token]);

  // Candidate discovery is deliberately credential-free. The current health contract
  // only proves reachability, not paired-machine identity, so a successful probe must
  // not release the device token or upgrade away from the relay.
  useEffect(() => {
    if (!token || typeof fetch !== "function") return;
    const candidates = directHints.filter((candidate) =>
      normalizeDirectCandidateOrigin(candidate.url) !== null);
    if (candidates.length === 0) return;
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), DEFAULT_PROBE_TIMEOUT_MS);
    void Promise.all(candidates.map(async (candidate) => {
      try {
        await fetch(`${candidate.url}/api/v1/health`, {
          signal: controller.signal,
          cache: "no-store",
          credentials: "omit",
          redirect: "error",
          mode: "cors",
        });
      } catch (error) {
        if (!controller.signal.aborted) console.warn("Direct host reachability probe failed", error);
      }
    })).finally(() => clearTimeout(timeout));
    return () => {
      controller.abort();
      clearTimeout(timeout);
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
    creationSessionsRef.current = null;
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
    // A selection event can land while a confirmation read is in flight; it invalidates that
    // read (refresh version bump), so it must re-run the read instead of being dropped.
    if (confirmationInFlightRef.current) {
      confirmationRequeuedRef.current = true;
      return;
    }
    confirmationInFlightRef.current = true;
    let confirmed: RemoteWorkspaceModel | null;
    do {
      confirmationRequeuedRef.current = false;
      confirmed = await refreshWorkspace();
    } while (confirmationRequeuedRef.current && pendingSelectionRef.current === option);
    confirmationInFlightRef.current = false;
    if (pendingSelectionRef.current !== option) return;
    const creating = creationSessionsRef.current;
    const newSession = confirmed?.context.activeTerminal?.sessionId;
    if (confirmed && modelConfirmsSelection(option, confirmed)
      && (!creating || (newSession && !creating.has(newSession)))) {
      const tgt = initialAccountTargetRef.current;
      if (tgt) {
        const confirmedSlug = confirmed.context.worktreeSlug ?? null;
        const requestedSlug = tgt.worktreeSlug ?? null;
        if (
          confirmed.context.workspaceId === tgt.workspaceId &&
          confirmedSlug === requestedSlug
        ) {
          // A restored target that names an exact session is a manual choice: keep it
          // independent across the target-clear so sync cannot take the session over.
          independentSessionRef.current = Boolean(tgt.sessionId);
          setInitialAccountTarget(null);
        }
      }
      clearPendingSelection(true);
    }
  }, [clearPendingSelection, refreshWorkspace]);

  useEffect(() => {
    if (!token) return;
    if (!activeTunnelConnection && typeof WebSocket === "undefined") return;
    let socket: WebSocket | TunnelWebSocket | null = null;
    let retry: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    let disposed = false;
    let connecting = false;
    const abort = new AbortController();
    const onMessage = (raw: any) => {
      const text = typeof raw === "string"
        ? raw
        : raw instanceof Uint8Array
        ? new TextDecoder().decode(raw)
        : String(raw ?? "");

      if (activeTunnelConnection) {
        const inventoryEvent = parseInventoryEvent(activeTunnelConnection.machine.machineId, text);
        if (inventoryEvent.kind === "inventory") {
          applyInventoryEvent(activeTunnelConnection.machine.machineId, text);
          scheduleInventoryRefreshRef.current();
          setDesktopSyncTick((tick) => tick + 1);
        }
        if (inventoryEvent.kind === "ignored" && inventoryEvent.type === "desktopSelectionChanged") {
          setDesktopFollowTick((tick) => tick + 1);
          return;
        }
        if (inventoryEvent.kind !== "ignored") return;
      }

      const change = parseActiveSelectionEvent(text);
      if (!change) return;

      // In machine account mode, ignore legacy desktop activeSelection focus events.
      // Machine context is owned by the mobile client's machine session.
      if (activeTunnelConnection) return;

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
    const connect = async () => {
      if (disposed || connecting) return;
      connecting = true;
      retry = null;
      let current: WebSocket | TunnelWebSocket;
      try {
        if (activeTunnelConnection) {
          current = await activeTunnelConnection.openWebSocket("/api/v1/events");
        } else {
          const url = await remoteSocketUrl(transportBaseUrl, "/api/v1/events", token, abort.signal);
          if (disposed) return;
          current = new WebSocket(url);
        }
      } catch (error) {
        if (disposed) return;
        if (!activeTunnelConnection && transport.url !== relayUrl) rollbackTransport();
        else {
          console.warn("Event socket connection failed", error);
          retry = setTimeout(connect, Math.min(10000, 1000 * 2 ** attempt));
          attempt = Math.min(attempt + 1, 4);
        }
        return;
      } finally {
        connecting = false;
      }
      current.onerror = () => {
        if (!disposed && socket === current && !activeTunnelConnection && transport.url !== relayUrl) rollbackTransport();
      };
      socket = current;
      current.onmessage = (event: any) => {
        if (!disposed && socket === current) onMessage(event.data);
      };
      current.onopen = () => {
        if (disposed || socket !== current) return;
        attempt = 0;
        workspaceRefreshVersionRef.current += 1;
        const pendingSelection = pendingSelectionRef.current;
        if (pendingSelection) void confirmSelection(pendingSelection);
        else void refreshWorkspace();
      };
      if (current.readyState === 1) {
        queueMicrotask(() => {
          if (!disposed && socket === current && current.readyState === 1) {
            const handler = current.onopen;
            if (handler) {
              (handler as (ev: Event) => void).call(current, new Event("open"));
            }
          }
        });
      }
      current.onclose = () => {
        if (disposed || socket !== current) return;
        socket = null;
        if (!activeTunnelConnection && transport.url !== relayUrl) {
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
      abort.abort();
      if (retry !== null) clearTimeout(retry);
      socket?.close();
      window.removeEventListener("online", recover);
      document.removeEventListener("visibilitychange", recover);
    };
  }, [activeTunnelConnection, clearPendingSelection, confirmSelection, refreshWorkspace, token, transport.url, transportBaseUrl, relayUrl, rollbackTransport]);

  // A desktop that never republishes a matching selection (stale listener,
  // closed window) must not strand the picker: every chip is disabled while a
  // selection is pending, so without a terminal state the phone can only retry
  // by reloading the page.
  const armConfirmationTimeout = useCallback((option: RemoteContextOption) => {
    if (confirmationTimeoutRef.current !== null) clearTimeout(confirmationTimeoutRef.current);
    confirmationTimeoutRef.current = setTimeout(() => {
      confirmationTimeoutRef.current = null;
      if (pendingSelectionRef.current !== option) return;
      if (creationSessionsRef.current) {
        setCreationError("Desktop did not confirm a new terminal. Check the desktop before retrying.");
      } else if (initialAccountTargetRef.current) {
        setCreationError("Desktop did not confirm the selected worktree. Please retry or go back.");
      }
      clearPendingSelection();
    }, CONFIRMATION_TIMEOUT_MS);
  }, [clearPendingSelection]);

  const selectContext = useCallback(async (requestedOption: RemoteContextOption, createTerminal = false): Promise<boolean> => {
    // A user selection naming a session (or an explicit tab) is an independent choice:
    // the read-only desktop-context sync must never replace it afterwards.
    independentSessionRef.current = Boolean(requestedOption.sessionId || requestedOption.tabId);
    if (!token || pendingSelectionRef.current) return false;
    // A picker can reuse an option object; each attempt needs its own identity.
    const option = { ...requestedOption };
    setCreationError(null);
    creationSessionsRef.current = createTerminal ? new Set([
      ...(model.context.terminalTabs?.flatMap((tab) => tab.sessionId ? [tab.sessionId] : []) ?? []),
      ...(model.context.activeTerminal ? [model.context.activeTerminal.sessionId] : []),
    ]) : null;
    pendingSelectionRef.current = option;
    selectionRequestAcceptedRef.current = false;
    selectionEventReceivedRef.current = false;
    optimisticSocketClosedRef.current = false;
    setPending(option);
    if (option.sessionId) {
      optimisticSocketSessionIdRef.current = option.sessionId;
      setOptimisticSessionId(option.sessionId);
    }
    // Bound the whole request, including a server that never sends headers.
    armConfirmationTimeout(option);

    try {
      let ok = false;
      let status = 0;
      const selectPayload = {
        workspaceId: option.workspaceId,
        ...(option.worktreeSlug ? { worktreeSlug: option.worktreeSlug } : {}),
        ...(option.tabId ? { tabId: option.tabId } : {}),
        ...(!option.tabId && option.sessionId ? { sessionId: option.sessionId } : {}),
        ...(createTerminal ? { createTerminal: true } : {}),
      };

      if (activeTunnelConnection) {
        // In machine account mode, discover or create the machine session directly via /api/v1/sessions
        let targetSession: MinimalMachineSession | null = null;

        const sessListRes = await activeTunnelConnection.transport.fetchLike("/api/v1/sessions", {
          headers: { Authorization: `Bearer ${token}` },
        });

        if (sessListRes.status < 200 || sessListRes.status >= 300) {
          throw new Error(`Machine sessions query failed (${sessListRes.status})`);
        }

        const sessListData = JSON.parse(new TextDecoder().decode(sessListRes.body));
        if (!sessListData || typeof sessListData !== "object" || !Array.isArray(sessListData.sessions)) {
          throw new Error("Machine sessions query returned invalid payload");
        }
        const isComplete = sessListData.completeness === "complete";
        const rows: MinimalMachineSession[] = sessListData.sessions;
        const machineId = activeTunnelConnection.machine.machineId;
        const parsedListing = parseSessionsPayload(machineId, sessListData);
        const inventory = parsedListing.ok
          ? mergeSessionInventory(
              accountDiscoveryRef.current.sessionInventories[machineId] ?? null,
              parsedListing.inventory,
            )
          : accountDiscoveryRef.current.sessionInventories[machineId] ?? null;

        if (option.machineId && option.machineId !== machineId) {
          throw new Error("Selected session belongs to a different machine");
        }

        if (!createTerminal) {
          if (option.sessionId) {
            const chosen = inventory
              ? selectInventorySession(inventory, {
                  workspaceId: option.workspaceId,
                  worktreeSlug: option.worktreeSlug ?? null,
                  sessionId: option.sessionId,
                  daemonEpoch: sessionEpochsRef.current.get(option.sessionId) ?? null,
                })
              : null;
            const exact = chosen
              ? rows.find((r) => (r.target?.sessionId ?? r.sessionId) === chosen.entry.sessionId) ?? null
              : rows.find((r) => {
                  if (r.running === false) return false;
                  const targetSid = r.target?.sessionId ?? r.sessionId;
                  if (targetSid !== option.sessionId) return false;
                  if (r.workspaceId !== option.workspaceId) return false;
                  const slug = r.worktree?.slug ?? null;
                  return slug === (option.worktreeSlug || null);
                }) ?? null;
            /* A daemon epoch the client already knows and that disagrees with the machine's
               answer means the cached terminal is gone; attaching anyway would type into a
               different process. */
            const expectedEpoch =
              (option as { daemonEpoch?: string | null }).daemonEpoch ??
              sessionEpochsRef.current.get(option.sessionId) ??
              null;
            const rowEpoch = exact?.target?.daemonEpoch ?? exact?.daemonEpoch ?? null;
            if (exact && expectedEpoch && rowEpoch !== null && rowEpoch !== undefined && String(rowEpoch) !== expectedEpoch) {
              throw new Error(`Machine session ${option.sessionId} was replaced (daemon epoch changed)`);
            }
            if (exact) {
              targetSession = exact;
            } else {
              throw new Error(`Requested machine session ${option.sessionId} is unavailable`);
            }
          } else {
            const cachedId = getAccountPreferredSessionId(relayUrl, machineId, option.workspaceId, option.worktreeSlug);
            const deterministic = inventory
              ? preferredSessionForWorktree(inventory, option.workspaceId, option.worktreeSlug ?? null, [
                  cachedId,
                  model.context.activeTerminal?.sessionId ?? null,
                ])
              : null;
            if (cachedId && !deterministic) {
              setAccountPreferredSessionId(relayUrl, machineId, option.workspaceId, option.worktreeSlug, "");
            }
            if (deterministic) {
              targetSession = rows.find((r) => (r.target?.sessionId ?? r.sessionId) === deterministic.sessionId) ?? null;
            } else {
              const matching = rows.find((r) => {
                if (r.running === false) return false;
                if (r.workspaceId !== option.workspaceId) return false;
                const slug = r.worktree?.slug ?? null;
                return slug === (option.worktreeSlug || null);
              });
              targetSession = matching ?? null;
            }
          }
        }

        if (!targetSession) {
          if (!isComplete) {
            throw new Error("Cannot create machine session: session inventory is incomplete");
          }
          const createPayload = {
            requestId: crypto.randomUUID(),
            workspaceId: option.workspaceId,
            worktree: option.worktreeSlug
              ? { wsId: option.workspaceId, slug: option.worktreeSlug }
              : null,
            cols: 80,
            rows: 24,
            inheritFromSessionId: null,
            cwdRelative: null,
            startup: { kind: "shell" },
          };
          const createRes = await activeTunnelConnection.transport.fetchLike("/api/v1/sessions", {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Authorization: `Bearer ${token}`,
            },
            body: JSON.stringify(createPayload),
          });

          if (createRes.status >= 200 && createRes.status < 300) {
            targetSession = JSON.parse(new TextDecoder().decode(createRes.body));
          } else {
            status = createRes.status;
            throw new Error(`Failed to create machine session (${createRes.status})`);
          }
        }

        if (targetSession) {
          ok = true;
          const sid = targetSession.target?.sessionId ?? targetSession.sessionId;
          const epoch = targetSession.target?.daemonEpoch ?? targetSession.daemonEpoch;
          if (sid && epoch !== undefined && epoch !== null) {
            sessionEpochsRef.current.set(sid, String(epoch));
            setSessionEpochs((prev) => ({ ...prev, [sid]: String(epoch) }));
          }
          if (sid) {
            option.sessionId = sid;
            const machineId = activeTunnelConnection.machine.machineId;
            setAccountPreferredSessionId(relayUrl, machineId, option.workspaceId, option.worktreeSlug, sid);
          }

          const isRunning = targetSession.running !== false;
          const contextLabel = option.worktreeLabel ?? option.worktreeSlug ?? "main";

          /* The tab strip is every session the machine still reports for this worktree,
             merged so a partial answer cannot hide the others. */
          const contextTabs = machineTabsFromInventory(
            inventory?.entries ?? [],
            option.workspaceId,
            option.worktreeSlug ?? null,
            contextLabel,
          );
          const activeContext = {
            workspaceId: option.workspaceId,
            worktreeSlug: option.worktreeSlug ?? null,
            worktreeLabel: contextLabel,
            activeTerminal: sid
              ? {
                  sessionId: sid,
                  running: isRunning,
                  title: targetSession.title ?? "Terminal",
                  workspaceId: option.workspaceId,
                  worktreeLabel: contextLabel,
                }
              : null,
            activeTabId: sid ?? null,
            terminalTabs: contextTabs,
          };

          // Invalidate any in-flight background refresh to prevent stale overwrites
          workspaceRefreshVersionRef.current += 1;

          modelRef.current = {
            ...modelRef.current,
            context: activeContext,
          };

          setModel((prev) => ({
            ...prev,
            context: activeContext,
          }));

          setInitialAccountTarget(null);
          clearPendingSelection(true);
          return true;
        }
      } else {
        const response = await fetch(
          apiUrl(transportBaseUrl, "/api/v1/workspace/select"),
          {
            method: "POST",
            headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
            body: JSON.stringify(selectPayload),
          },
        );
        ok = response.ok;
        status = response.status;
      }
      if (pendingSelectionRef.current !== option) return false;
      if (!ok) throw new Error(`Selection failed (${status})`);

      selectionRequestAcceptedRef.current = true;
      if (initialAccountTargetRef.current || selectionEventReceivedRef.current) void confirmSelection(option);
      return true;
    } catch (error) {
      if (pendingSelectionRef.current !== option) return false;
      const message = error instanceof Error ? error.message : "Selection request failed";
      setCreationError(message);
      clearPendingSelection();
      return false;
    }
  }, [activeTunnelConnection, armConfirmationTimeout, clearPendingSelection, confirmSelection, model, token, transportBaseUrl]);

  // Account picker choices only record the target; the selection is issued here once the
  // committed token/connection for the chosen machine are visible to selectContext.
  const initialAccountSelectionAttemptedRef = useRef(false);
  const accountAcquireInFlightRef = useRef(false);
  useEffect(() => {
    if (!token || !activeTunnelConnection || !initialAccountTarget) return;
    if (activeTunnelConnection.machine.machineId !== initialAccountTarget.machineId) return;
    if (initialAccountSelectionAttemptedRef.current) return;
    initialAccountSelectionAttemptedRef.current = true;

    const target = initialAccountTarget;
    void selectContext({
      machineId: target.machineId,
      workspaceId: target.workspaceId,
      worktreeSlug: target.worktreeSlug,
      worktreeLabel: target.worktreeLabel,
      ...(target.sessionId ? { sessionId: target.sessionId } : {}),
      ...(target.daemonEpoch ? { daemonEpoch: target.daemonEpoch } : {}),
    });
  }, [token, activeTunnelConnection, initialAccountTarget, selectContext]);

  /* Always-follow (account route): the machine's published desktop focus is the view.
     After a connection is established and whenever the desktop publishes a focus change,
     the browser re-reads the authoritative active context and follows it. The session is
     validated against the machine's own session list, which is also where its daemon epoch
     comes from: the workspace-state epoch path can answer null, and a follow must not
     depend on it. A manual pick stays until the desktop focus changes again.
     Read-only: never POSTs /workspace/select. */
  const selectContextRef = useRef(selectContext);
  selectContextRef.current = selectContext;
  useEffect(() => {
    if (!initialAccountTarget && !pending && desktopFollowDeferredRef.current) {
      desktopFollowDeferredRef.current = false;
      setDesktopFollowTick((tick) => tick + 1);
    }
  }, [initialAccountTarget, pending]);
  useEffect(() => {
    if (!token || !activeTunnelConnection) return;
    const connection = activeTunnelConnection;
    const machineId = connection.machine.machineId;
    // Decided before any await: a pick that is still settling when this connection comes
    // up is an explicit choice, and only a later desktop focus change may replace it.
    const explicitPick = Boolean(initialAccountTargetRef.current || pendingSelectionRef.current)
      && !restoreFollowRef.current;
    let cancelled = false;
    void (async () => {
      try {
        const res = await connection.transport.fetchLike("/api/v1/workspace/state", {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (cancelled || res.status < 200 || res.status >= 300) return;
        const state = normalizeRemoteWorkspaceState(JSON.parse(new TextDecoder().decode(res.body)));
        const workspaceId = state.context.workspaceId;
        const worktreeSlug = state.context.worktreeSlug ?? null;
        const active = state.context.activeTerminal;
        const sessionId = active?.sessionId ?? null;
        // No exact session means no desktop view to follow.
        if (!workspaceId || !sessionId) return;
        const key = `${machineId}\u0000${workspaceId}\u0000${worktreeSlug ?? ""}\u0000${sessionId}`;
        const last = lastFollowedRef.current;
        if (last && last.connection === connection && last.key === key) return;
        const sessRes = await connection.transport.fetchLike("/api/v1/sessions", {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (cancelled || sessRes.status < 200 || sessRes.status >= 300) return;
        const parsed = parseSessionsPayload(machineId, JSON.parse(new TextDecoder().decode(sessRes.body)));
        if (!parsed.ok) return;
        // The machine's own list is the authority for identity, scope and liveness; its row
        // also carries the daemon epoch this client must fence the attach with.
        const entry = parsed.inventory.entries.find(
          (row) =>
            row.sessionId === sessionId &&
            row.machineId === machineId &&
            row.workspaceId === workspaceId &&
            row.worktreeSlug === worktreeSlug &&
            row.running,
        );
        if (!entry || cancelled) return;
        const epoch = entry.daemonEpoch;
        if (explicitPick) {
          lastFollowedRef.current = { connection, key };
          return;
        }
        if (initialAccountTargetRef.current || pendingSelectionRef.current) {
          // The automatic restore is settling; retry once it clears instead of racing it.
          desktopFollowDeferredRef.current = true;
          return;
        }
        restoreFollowRef.current = false;
        accountDiscovery.noteContextTabs(machineId, state.context.terminalTabs);
        const current = modelRef.current.context;
        const alreadyShown =
          current.workspaceId === workspaceId &&
          (current.worktreeSlug ?? null) === worktreeSlug &&
          current.activeTerminal?.sessionId === sessionId;
        lastFollowedRef.current = { connection, key };
        if (alreadyShown) return;
        const applied = await selectContextRef.current({
          machineId,
          workspaceId,
          worktreeSlug,
          worktreeLabel: state.context.worktreeLabel ?? worktreeSlug,
          sessionId,
          ...(epoch ? { daemonEpoch: epoch } : {}),
        } as RemoteContextOption);
        // A refused or failed apply is retried on the next focus change or reconnect.
        if (!applied && !cancelled) lastFollowedRef.current = null;
      } catch (error) {
        console.warn("[remote] desktop follow skipped", error);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [token, activeTunnelConnection, desktopFollowTick, accountDiscovery.noteContextTabs]);

  const activeMachineId = activeTunnelConnection?.machine.machineId ?? null;
  // Swipe order mirrors the inventory the sidebar renders, so the gesture, the position
  // header, and the aria-selected row describe one sequence. -1 means no rendered row
  // holds the selection: swipes are no-ops instead of first-row jumps.
  const swipePanes = useMemo(
    () => renderedPaneOrder(mirrorInventory(model, activeMachineId)),
    [activeMachineId, model],
  );
  const currentIndex = swipePanes.findIndex((tab) => tab.id === model.context.activeTabId);

  const handleSwipePreviousTab = useCallback(() => {
    if (swipePanes.length <= 1 || currentIndex <= 0 || !model.context.workspaceId) return;
    const prevTab = swipePanes[currentIndex - 1];
    if (prevTab) {
      void selectContext({
        workspaceId: model.context.workspaceId,
        worktreeSlug: prevTab.worktreeSlug ?? model.context.worktreeSlug,
        worktreeLabel: prevTab.worktreeLabel ?? model.context.worktreeLabel,
        tabId: prevTab.id,
        sessionId: prevTab.sessionId,
      });
    }
  }, [currentIndex, model.context.workspaceId, model.context.worktreeLabel, model.context.worktreeSlug, selectContext, swipePanes]);

  const handleSwipeNextTab = useCallback(() => {
    if (swipePanes.length <= 1 || currentIndex < 0 || currentIndex >= swipePanes.length - 1 || !model.context.workspaceId) return;
    const nextTab = swipePanes[currentIndex + 1];
    if (nextTab) {
      void selectContext({
        workspaceId: model.context.workspaceId,
        worktreeSlug: nextTab.worktreeSlug ?? model.context.worktreeSlug,
        worktreeLabel: nextTab.worktreeLabel ?? model.context.worktreeLabel,
        tabId: nextTab.id,
        sessionId: nextTab.sessionId,
      });
    }
  }, [currentIndex, model.context.workspaceId, model.context.worktreeLabel, model.context.worktreeSlug, selectContext, swipePanes]);

  const activeTerminal = model.context.activeTerminal;
  const effectiveSessionId = initialAccountTarget
    ? null
    : (optimisticSessionId ?? activeTerminal?.sessionId ?? null);

  const turnDurationsRef = useRef<Map<string, string>>(new Map());

  const finalizeAssistantTurnDuration = useCallback(() => {
    const startedAt = assistantTurnStartedAtRef.current;
    if (startedAt !== null) {
      const elapsedMs = Date.now() - startedAt;
      const durationLabel = formatWorkedDuration(elapsedMs);
      setChatMessages((prev) => {
        const last = prev[prev.length - 1];
        if (!last || last.role !== "assistant") return prev;
        turnDurationsRef.current.set(last.id, durationLabel);
        return [...prev.slice(0, -1), { ...last, durationLabel }];
      });
      assistantTurnStartedAtRef.current = null;
    }
  }, []);

  const terminalCreateWebSocket = useMemo(() => {
    if (!activeTunnelConnection) return undefined;
    return async (path: string) => {
      let targetPath = path;
      if (!targetPath.includes("daemonEpoch=") && effectiveSessionId) {
        const epoch = await getSessionDaemonEpoch(effectiveSessionId);
        if (!epoch) {
          throw new Error(
            `Cannot connect terminal: daemonEpoch is missing for session ${effectiveSessionId}`,
          );
        }
        const sep = targetPath.includes("?") ? "&" : "?";
        targetPath = `${targetPath}${sep}daemonEpoch=${encodeURIComponent(epoch)}`;
      }
      return activeTunnelConnection.openWebSocket(targetPath);
    };
  }, [activeTunnelConnection, effectiveSessionId, getSessionDaemonEpoch]);

  const activeTunnelMachineId = activeTunnelConnection?.machine.machineId ?? null;
  const sessionEpochsActiveMachine = activeTunnelMachineId;
  useEffect(() => {
    /* Epochs are keyed by session id only, so they must not survive a machine change:
       the same id on another machine is a different terminal. */
    sessionEpochsRef.current.clear();
    setSessionEpochs({});
  }, [sessionEpochsActiveMachine]);

  const handleMachineAgentStateFrame = useCallback((raw: string) => {
    const frame = parseMachineAgentStateFrame(raw);
    if (!frame) return;
    const machineId = activeTunnelMachineId;
    if (!machineId || (frame.machineId && frame.machineId !== machineId)) return;
    const knownEpoch =
      inventoryRef.current?.entries.find((entry) => entry.sessionId === frame.sessionId)?.daemonEpoch ?? null;
    if (frame.daemonEpoch && knownEpoch && frame.daemonEpoch !== knownEpoch) return;
    noteSessionActivity(machineId, {
      sessionId: frame.sessionId,
      state: frame.state,
      source: "agent_state",
      machineId,
      daemonEpoch: frame.daemonEpoch,
      agentType: frame.agent,
    });
  }, [activeTunnelMachineId, noteSessionActivity]);

  useEffect(() => {
    const tabs = model.context.terminalTabs ?? [];
    const activeSessionId = model.context.activeTerminal?.sessionId ?? null;
    const tabForActiveSession = activeSessionId
      ? tabs.find((tab) => tab.sessionId === activeSessionId)
      : undefined;
    /* Tabs are filtered to the active worktree, but the active session must never be
       inferred from the first tab: that would report another session's activity. */
    const activeTab = tabForActiveSession
      ?? tabs.find((tab) => tab.id === model.context.activeTabId)
      ?? (activeSessionId ? undefined : tabs[0]);
    const inventoryEntry = activeSessionId
      ? inventoryRef.current?.entries.find((entry) => entry.sessionId === activeSessionId) ?? null
      : null;
    const nextState = activeTab?.activityState ?? inventoryEntry?.activityState ?? null;
    const previousState = lastAgentActivityRef.current;
    lastAgentActivityRef.current = nextState;

    if (nextState === "working") {
      if (assistantTurnStartedAtRef.current === null) {
        assistantTurnStartedAtRef.current = Date.now();
      }
      setChatIsRunning(true);
      return;
    }

    if (previousState === "working") {
      finalizeAssistantTurnDuration();
      setChatIsRunning(false);
    }
  }, [model.context.activeTabId, model.context.terminalTabs, finalizeAssistantTurnDuration]);

  useEffect(() => {
    if (activeTunnelConnection && effectiveSessionId && !(sessionEpochs[effectiveSessionId] ?? sessionEpochsRef.current.get(effectiveSessionId))) {
      void getSessionDaemonEpoch(effectiveSessionId);
    }
  }, [activeTunnelConnection, effectiveSessionId, getSessionDaemonEpoch, sessionEpochs]);

  useEffect(() => {
    if (!effectiveSessionId || !token || viewMode !== "chat") {
      if (terminalSocketRef.current) {
        terminalSocketRef.current.close();
        terminalSocketRef.current = null;
        terminalSocketSessionIdRef.current = null;
      }
      return;
    }

    if (terminalSocketSessionIdRef.current === effectiveSessionId && terminalSocketRef.current) {
      return;
    }

    if (terminalSocketRef.current) {
      terminalSocketRef.current.close();
      terminalSocketRef.current = null;
    }

    let disposed = false;
    const abort = new AbortController();

    if (activeTunnelConnection) {
      getSessionDaemonEpoch(effectiveSessionId).then((epoch) => {
        if (disposed) return;
        if (!epoch) {
          const errMsg = `Cannot connect terminal: daemonEpoch is missing for session ${effectiveSessionId}`;
          console.error(errMsg);
          setCreationError(errMsg);
          finalizeAssistantTurnDuration();
          setChatIsRunning(false);
          return;
        }
        const pathAndQuery = `/api/v1/terminal/${encodeURIComponent(effectiveSessionId)}?daemonEpoch=${encodeURIComponent(epoch)}`;
        activeTunnelConnection
          .openWebSocket(pathAndQuery)
          .then((ws) => {
            if (disposed) {
              ws.close();
              return;
            }
            terminalSocketRef.current = ws;
            terminalSocketSessionIdRef.current = effectiveSessionId;
            ws.onmessage = (event) => {
              if (typeof event.data === "string") handleMachineAgentStateFrame(event.data);
            };
            ws.onclose = () => {
              if (terminalSocketRef.current === ws) {
                terminalSocketRef.current = null;
                terminalSocketSessionIdRef.current = null;
                finalizeAssistantTurnDuration();
                setChatIsRunning(false);
              }
            };
            ws.onerror = () => {
              if (terminalSocketRef.current === ws) {
                terminalSocketRef.current = null;
                terminalSocketSessionIdRef.current = null;
                finalizeAssistantTurnDuration();
                setChatIsRunning(false);
              }
            };
          })
          .catch((err) => {
            console.warn("Failed to connect terminal WebSocket via tunnel", err);
            setCreationError(err instanceof Error ? err.message : String(err));
            finalizeAssistantTurnDuration();
            setChatIsRunning(false);
          });
      });
    } else {
      remoteSocketUrl(transportBaseUrl, `/api/v1/terminal/${encodeURIComponent(effectiveSessionId)}`, token, abort.signal)
        .then((url) => {
          if (disposed) return;
          const ws = new WebSocket(url);
          ws.binaryType = "arraybuffer";
          terminalSocketRef.current = ws;
          terminalSocketSessionIdRef.current = effectiveSessionId;
          ws.onclose = () => {
            if (terminalSocketRef.current === ws) {
              terminalSocketRef.current = null;
              terminalSocketSessionIdRef.current = null;
              finalizeAssistantTurnDuration();
              setChatIsRunning(false);
            }
          };
          ws.onerror = () => {
            if (terminalSocketRef.current === ws) {
              terminalSocketRef.current = null;
              terminalSocketSessionIdRef.current = null;
              finalizeAssistantTurnDuration();
              setChatIsRunning(false);
            }
          };
        })
        .catch((err) => {
          if (!disposed) {
            console.warn("Failed to resolve terminal socket URL", err);
          }
        });
    }

    return () => {
      disposed = true;
      abort.abort();
      if (terminalSocketRef.current) {
        terminalSocketRef.current.close();
        terminalSocketRef.current = null;
        terminalSocketSessionIdRef.current = null;
      }
    };
  }, [effectiveSessionId, token, activeTunnelConnection, transportBaseUrl, viewMode, finalizeAssistantTurnDuration]);

  useEffect(() => {
    if (viewMode !== "chat" || !effectiveSessionId || !token) return;

    let cancelled = false;
    let fetching = false;
    const controller = new AbortController();

    if (lastConversationSessionRef.current !== effectiveSessionId) {
      lastConversationSessionRef.current = effectiveSessionId;
      retentionTruncatedRef.current = false;
      revokeChatAttachmentUrls();
      setChatMessages([]);
      setChatWarnings([]);
      setChatIsRunning(false);
      assistantTurnStartedAtRef.current = null;
      turnDurationsRef.current.clear();
    }

    const poll = async () => {
      if (cancelled || fetching) return;
      if (typeof document !== "undefined" && document.hidden) return;
      fetching = true;
      try {
        const page = await fetchAgentConversation({
          baseUrl: transportBaseUrl,
          sessionId: effectiveSessionId,
          token,
          limit: 200,
          signal: controller.signal,
          transport: activeTunnelConnection?.transport,
        });
        if (cancelled) return;
        const retentionWarning = "Older messages are hidden to keep the phone view responsive.";
        setChatWarnings(
          retentionTruncatedRef.current && !page.warnings.includes(retentionWarning)
            ? [...page.warnings, retentionWarning]
            : [...page.warnings],
        );
        setChatMessages((prev) => {
          const mapped = mapAgentConversation(page.items, {
            activeTurnStartedAt: assistantTurnStartedAtRef.current,
            previousMessages: prev,
            turnDurationsMap: turnDurationsRef.current,
          });
          const optimisticText = new Set(
            prev
              .filter((message) => message.role === "user" && /^user-\d{13}$/.test(message.id))
              .map((message) => message.content),
          );
          const coveredOrdinals = new Set(page.items.map((item) => item.ordinal));
          const kept = prev.filter((message) => {
            const match = /^(?:user|assistant)-(\d+)$/.exec(message.id);
            return match === null || !coveredOrdinals.has(Number(match[1]));
          });
          const fresh = mapped.filter((message) =>
            !(message.role === "user" && optimisticText.has(message.content)),
          );
          const sorted = [...kept, ...fresh].sort((a, b) => {
            const aMatch = /^(?:user|assistant)-(\d+)$/.exec(a.id);
            const bMatch = /^(?:user|assistant)-(\d+)$/.exec(b.id);
            const aOrdinal = aMatch === null ? Number.POSITIVE_INFINITY : Number(aMatch[1]);
            const bOrdinal = bMatch === null ? Number.POSITIVE_INFINITY : Number(bMatch[1]);
            return aOrdinal - bOrdinal;
          });
          const { messages: capped, truncated } = capRetainedMessages(sorted);
          if (truncated) {
            retentionTruncatedRef.current = true;
            const hiddenWarning = "Older messages are hidden to keep the phone view responsive.";
            setChatWarnings((prev) =>
              prev.includes(hiddenWarning) ? prev : [...prev, hiddenWarning],
            );
          }
          return capped;
        });
      } catch (error) {
        if (cancelled) return;
        if (error instanceof ConversationFetchError && error.code === "TRANSCRIPT_NOT_FOUND") {
          revokeChatAttachmentUrls();
          retentionTruncatedRef.current = false;
          setChatWarnings((prev) =>
            prev.filter((warning) => warning !== "Older messages are hidden to keep the phone view responsive."),
          );
          setChatMessages([]);
        } else {
          const pollFailedWarning = "Transcript refresh failed; showing the last known state.";
          setChatWarnings((prev) =>
            prev.includes(pollFailedWarning) ? prev : [...prev, pollFailedWarning],
          );
        }
      } finally {
        fetching = false;
      }
    };

    void poll();
    const timer = setInterval(() => {
      void poll();
    }, 3000);

    return () => {
      cancelled = true;
      controller.abort();
      clearInterval(timer);
    };
  }, [viewMode, effectiveSessionId, token, transportBaseUrl, activeTunnelConnection]);

  /* Declared above the auth early return so the hook count is identical on the
     login screen and after pairing; a hook below the guard changes the order. */
  const isAccountMode = Boolean(accountSessionToken);
  const activeMachineInventory = activeTunnelConnection
    ? accountDiscovery.sessionInventories[activeTunnelConnection.machine.machineId] ?? null
    : null;
  inventoryRef.current = activeMachineInventory;

  /* Pane rows render from the model's tab strip, which is a snapshot taken when the model
     was built. Observations arrive afterwards, so the live inventory wins over the snapshot
     for the matching epoch: a pane must not be frozen on the state it was built with.
     An observation whose epoch contradicts the pane is never applied. */
  const paneTabsWithObservedActivity = (tabs: RemoteTerminalTabInfo[] | undefined) => {
    if (!tabs || tabs.length === 0) return tabs;
    const entries = activeMachineInventory?.entries ?? [];
    if (entries.length === 0) return tabs;
    return tabs.map((tab) => {
      const sessionId = tab.sessionId;
      if (!sessionId) return tab;
      const entry = entries.find((row) => row.sessionId === sessionId);
      if (!entry) return tab;
      const paneEpoch = tab.daemonEpoch === null || tab.daemonEpoch === undefined ? null : String(tab.daemonEpoch);
      const entryEpoch = entry.daemonEpoch ?? null;
      if (paneEpoch && entryEpoch && paneEpoch !== entryEpoch) return tab;
      const observed = entry.activityState ?? null;
      const snapshot = tab.activityState ?? null;
      if (!observed && !snapshot) return tab;
      if (!observed && snapshot) return tab;
      if (observed === snapshot && (!entry.agentType || entry.agentType === tab.agentType)) return tab;
      return {
        ...tab,
        ...(observed ? { activityState: observed } : {}),
        ...(entry.agentType ? { agentType: entry.agentType } : {}),
        ...(entryEpoch ? { daemonEpoch: entryEpoch } : {}),
      };
    });
  };

  const effectiveModel: RemoteWorkspaceModel = isAccountMode && accountDiscovery.accountOptions.length > 0
    ? {
        ...model,
        options: accountDiscovery.accountOptions,
        context: {
          ...model.context,
          terminalTabs: paneTabsWithObservedActivity(model.context.terminalTabs),
        },
      }
    : model;

  const createWorktree = useCallback(async () => {
    const workspaceId = model.context.workspaceId;
    if (!workspaceId || !token) return;
    const entered = window.prompt("New worktree slug (letters, numbers and dashes)");
    if (!entered) return;
    const slug = entered.trim().toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-+|-+$/g, "");
    if (!slug) {
      setCreationError("Enter a slug using letters, numbers or dashes.");
      return;
    }
    setCreationError(null);
    try {
      const response = await fetch(apiUrl(transportBaseUrl, "/api/v1/workspace/worktrees"), {
        method: "POST",
        headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
        body: JSON.stringify({
          requestId: crypto.randomUUID(),
          workspaceId,
          worktree: { wsId: workspaceId, slug },
        }),
      });
      if (!response.ok) {
        const reason = (await response.text()).replace(/\/[^\s"]+/g, "<path>").slice(0, 140);
        setCreationError(`Could not create worktree (HTTP ${response.status}). ${reason}`);
        return;
      }
      await selectContext({ workspaceId, worktreeSlug: slug, worktreeLabel: null });
    } catch (error) {
      setCreationError(error instanceof Error ? error.message : "Worktree creation request failed");
    }
  }, [model.context.workspaceId, token, transportBaseUrl, selectContext]);

  if ((!token && !accountSessionToken) || magicLinkActive) {
    return (
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={(tok) => {
          setAccountSessionToken(tok);
          setMagicLinkActive(false);
          onMagicLinkConsumed?.();
        }}
      />
    );
  }

  const waitingTargets = collectWaitingTargets(model);
  const firstWaiting = waitingTargets.length > 0 ? waitingTargets[0] : null;
  const waitingCount = waitingTargets.length;
  const attentionAriaLabel = firstWaiting
    ? formatAttentionAriaLabel(firstWaiting, model.context, waitingCount)
    : "";
  // Signed in to the account but no worktree chosen yet: only the collapsed top picker
  // is shown and the body stays blank until an explicit choice.
  const accountPreselection = isAccountMode && !token;

  // Aggregate inventory status only: the picker lists worktrees like the desktop and never
  // names, groups, or offers a choice between machines.
  const accountStatuses = Object.values(accountDiscovery.machineStatuses);
  const accountInventoryLoading =
    accountDiscovery.loading || accountStatuses.some((status) => status.status === "tunneling");
  const failedStatuses = accountStatuses.filter((status) => status.status === "error");
  const failedMachineIds = failedStatuses.map((status) => status.machine.machineId);
  const failureMessages = [
    ...new Set(
      failedStatuses.map((status) =>
        status.errorCode ? `${status.errorCode}: ${status.error ?? ""}` : (status.error ?? "Worktrees could not be loaded."),
      ),
    ),
  ];
  const accountInventorySettled =
    isAccountMode && !accountInventoryLoading && !accountDiscovery.error && accountDiscovery.accountOptions.length === 0;
  // Name the known cause of an empty picker instead of a generic line; never names machines.
  const accountEmptyReason = !accountInventorySettled || failedMachineIds.length > 0
    ? null
    : accountDiscovery.machines.length === 0
      ? "No desktops linked to this account."
      : accountStatuses.some((status) => status.status === "offline")
        ? "Desktop is offline. Open Ferryx on your computer."
        : null;
  const accountPickerStatus =
    isAccountMode &&
    (accountInventoryLoading || accountDiscovery.error || failedMachineIds.length > 0 || accountEmptyReason) ? (
    <div data-testid="remote-account-inventory-status" className="space-y-0.5 pb-1">
      {accountInventoryLoading ? (
        <p className="px-2 py-1 text-[11px] text-muted-foreground">Loading worktrees...</p>
      ) : null}
      {accountDiscovery.error ? (
        <p role="alert" className="px-2 py-1 text-[11px] text-destructive">{accountDiscovery.error}</p>
      ) : null}
      {accountEmptyReason ? (
        <p className="px-2 py-6 text-center text-[11px] text-muted-foreground">{accountEmptyReason}</p>
      ) : null}
      {failedMachineIds.length > 0 ? (
        <div className="flex min-h-[24px] items-center gap-1.5 px-2 text-[11px] text-muted-foreground">
          <span role="alert" className="min-w-0 flex-1 break-words text-destructive">
            {failureMessages.join(" / ")}
          </span>
          <button
            type="button"
            aria-label="Retry loading worktrees"
            onClick={() => {
              for (const machineId of failedMachineIds) void accountDiscovery.retryMachine(machineId);
            }}
            className="shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium text-worktree-sidebar-foreground hover:bg-worktree-sidebar-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            Retry
          </button>
        </div>
      ) : null}
    </div>
  ) : null;

  const selectAccountOption = async (accountOpt: AccountWorktreeOption): Promise<boolean> => {
    if (accountAcquireInFlightRef.current || pendingSelectionRef.current) return false;
    accountSelectionGenerationRef.current += 1;
    const currentSelectionGen = accountSelectionGenerationRef.current;
    const expectedToken = accountSessionToken;

    const target = {
      machineId: accountOpt.machineId,
      workspaceId: accountOpt.workspaceId,
      worktreeSlug: accountOpt.worktreeSlug,
      worktreeLabel: accountOpt.worktreeLabel,
    };
    /* The exact-terminal identity is captured while the row is still trusted: the option
       may carry it, and the inventory snapshot from the same moment is the fallback. A
       later machine answer is compared against this, never used to define it. */
    const inventoryEpoch = accountOpt.sessionId
      ? accountDiscoveryRef.current.sessionInventories[accountOpt.machineId]?.entries.find(
          (entry) => entry.sessionId === accountOpt.sessionId,
        )?.daemonEpoch ?? null
      : null;
    const selectionTarget = {
      ...target,
      sessionId: accountOpt.sessionId ?? null,
      daemonEpoch: (accountOpt as { daemonEpoch?: string | null }).daemonEpoch ?? inventoryEpoch,
    };
    setCreationError(null);

    if (activeTunnelConnection && token && activeMachineId === accountOpt.machineId) {
      // Same machine: keep the connection, gate the body until the exact target confirms.
      initialAccountSelectionAttemptedRef.current = false;
      setInitialAccountTarget(selectionTarget);
      setAccountLastSelectedTarget(relayUrl, target);
      return true;
    }

    accountAcquireInFlightRef.current = true;
    let conn;
    try {
      conn = await accountDiscovery.acquireConnection(accountOpt.machineId);
    } finally {
      accountAcquireInFlightRef.current = false;
    }

    if (
      accountSelectionGenerationRef.current !== currentSelectionGen ||
      accountSessionTokenRef.current !== expectedToken ||
      !expectedToken
    ) {
      // Selection was invalidated while acquire was in flight (e.g. user logged out or switched selection).
      if (conn) {
        try {
          conn.close();
        } catch {}
      }
      return false;
    }

    if (!conn) {
      const worktree = accountOpt.worktreeLabel ?? accountOpt.worktreeSlug;
      setCreationError(`Could not open ${worktree ? `${accountOpt.workspaceId} / ${worktree}` : accountOpt.workspaceId}. Try again.`);
      return false;
    }

    // acquireConnection transferred ownership, so this only closes exploratory tunnels.
    accountDiscovery.closeAllExcept(null);
    // Everything below belonged to the previous machine; the replaced connection is
    // closed by the activeTunnelConnection effect cleanup.
    clearPendingSelection();
    workspaceRefreshVersionRef.current += 1;
    setModel(EMPTY_MODEL);
    sessionEpochsRef.current.clear();
    sessionEpochMissesRef.current.clear();
    setSessionEpochs({});
    setActiveTunnelConnection(createAccountConnection({
      relayUrl,
      accountSessionToken: accountSessionToken!,
      machine: conn.machine,
      deviceToken: conn.deviceToken,
      httpTransport: conn.transport,
      httpClose: conn.close,
    }));
    setToken(conn.deviceToken);
    initialAccountSelectionAttemptedRef.current = false;
    setInitialAccountTarget(selectionTarget);
    setAccountLastSelectedTarget(relayUrl, target);
    return true;
  };

  const renderLazy = (node: React.ReactNode) => (
    <Suspense
      fallback={
        <div
          data-testid="remote-workspace-loading"
          aria-live="polite"
          className="flex-1 flex flex-col items-center justify-center p-6 text-center space-y-2 bg-background text-foreground"
        >
          <div className="flex items-center space-x-2">
            <span className="px-2 py-0.5 text-[11px] font-medium bg-muted text-muted-foreground rounded animate-pulse">
              Loading workspace...
            </span>
          </div>
        </div>
      }
    >
      {node}
    </Suspense>
  );

  return (
    <div className="flex h-[100dvh] min-h-0 min-w-0 flex-col overflow-hidden remote-app-root bg-background text-foreground" style={viewportHeight ? { height: viewportHeight } : undefined}>
      <Toaster />
      <header className="flex h-7 shrink-0 items-center justify-between border-b border-chat-border bg-chat-surface px-2.5">
        <button
          type="button"
          aria-label="Change workspace context"
          aria-expanded={selectorOpen}
          onClick={() => setSelectorOpen((open) => !open)}
          className="flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden rounded px-1 py-0.5 -mx-1 text-left transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <img src="/icon-192.png" alt="Ferryx" width={16} height={16} className="size-4 shrink-0 rounded object-contain" />
          {/* The brand word is the first thing to go when the status cluster grows;
              the workspace context stays legible longer than the app name. */}
          <span className="hidden shrink-0 text-xs font-semibold leading-none sm:inline">Ferryx Remote</span>
          <span className="min-w-0 truncate font-mono text-[11px] leading-none text-chat-foreground-secondary" aria-label="Current desktop context">{contextName(model.context)}</span>
          <ChevronDown aria-hidden="true" className={`size-3 shrink-0 text-chat-foreground-secondary transition-transform ${selectorOpen ? "rotate-180" : ""}`} />
        </button>
        {accountPreselection ? null : (
        <div className="flex shrink-0 items-center gap-1.5">
          <span
            data-testid="remote-connection-badge"
            data-connection={transport.type}
            aria-label={`Connection: ${CONNECTION_BADGE_LABEL[transport.type]}`}
            title={`Connection: ${CONNECTION_BADGE_LABEL[transport.type]}`}
            className={`hidden h-5 shrink-0 items-center gap-1 rounded px-1.5 text-[11px] font-medium leading-none sm:flex ${
              transport.type === "relay"
                ? "bg-status-idle/15 text-chat-foreground-secondary"
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

          <div className="hidden items-center gap-1 border-l border-chat-border/40 pl-2 sm:flex">
            <button
              type="button"
              data-testid="remote-view-mode-chat"
              onClick={() => setViewMode("chat")}
              className={`flex h-5 items-center rounded px-1.5 text-[11px] font-medium transition-colors ${
                viewMode === "chat"
                  ? "bg-chat-surface-raised text-chat-foreground"
                  : "text-chat-foreground-secondary hover:text-chat-foreground"
              }`}
            >
              Chat
            </button>
            <button
              type="button"
              data-testid="remote-view-mode-terminal"
              onClick={() => setViewMode("terminal")}
              className={`flex h-5 items-center rounded px-1.5 text-[11px] font-medium transition-colors ${
                viewMode === "terminal"
                  ? "bg-chat-surface-raised text-chat-foreground"
                  : "text-chat-foreground-secondary hover:text-chat-foreground"
              }`}
            >
              Terminal
            </button>
          </div>
        </div>
        )}
      </header>

      <RemoteWorkspaceMirror
        model={effectiveModel}
        pending={pending}
        selectorOpen={selectorOpen}
        onSelectorOpenChange={setSelectorOpen}
        // Account mode never exposes a machine chooser; sign-out stays reachable in the picker.
        onOpenHosts={isAccountMode ? undefined : () => setHostDrawerOpen(true)}
        onSignOut={isAccountMode ? handleSignOut : undefined}
        activeMachineId={activeMachineId}
        pickerStatus={
          accountPreselection && creationError ? (
            <>
              {accountPickerStatus}
              <p role="alert" className="px-2 py-1 text-[11px] text-destructive">{creationError}</p>
            </>
          ) : accountPickerStatus
        }
        onSelect={(option) => {
          const accountOpt = (option as Partial<AccountWorktreeOption>).machineId
            ? (option as AccountWorktreeOption)
            : null;
          if (accountOpt) {
            void selectAccountOption(accountOpt).then((ok) => {
              // Pre-selection failures are reported inside the reopened picker, not the body.
              if (!ok) setSelectorOpen(true);
            });
            return;
          }
          void selectContext(option);
        }}
        onCreateTerminal={() => {
          if (!model.context.workspaceId) return;
          void selectContext({ workspaceId: model.context.workspaceId, worktreeSlug: model.context.worktreeSlug, worktreeLabel: model.context.worktreeLabel }, true);
        }}
        onCreateWorktree={createWorktree}
        creationError={initialAccountTarget || accountPreselection ? null : creationError}
      >
        {accountPreselection ? (
          <div data-testid="remote-account-empty-body" className="flex-1 flex flex-col items-center justify-center gap-2 p-6 text-center">
            <p className="text-xs text-muted-foreground">
              Choose a worktree to open its terminal.
            </p>
            {accountPickerStatus}
          </div>
        ) : initialAccountTarget ? (
            <div className="flex-1 flex flex-col items-center justify-center p-6 text-center space-y-4 bg-background text-foreground">
              <div className="space-y-1">
                <h3 className="text-sm font-semibold tracking-tight">Activating Selected Worktree</h3>
                <p className="text-xs text-muted-foreground">
                  Connecting to {initialAccountTarget.worktreeLabel ?? initialAccountTarget.worktreeSlug ?? initialAccountTarget.workspaceId}...
                </p>
              </div>
              {creationError && (
                <div role="alert" className="p-3 text-xs bg-destructive/10 text-destructive rounded-lg max-w-sm text-left space-y-2">
                  <p>{creationError}</p>
                  <div className="flex items-center gap-2 pt-1">
                    <button
                      type="button"
                      onClick={() => {
                        setCreationError(null);
                        initialAccountSelectionAttemptedRef.current = false;
                        void selectContext({
                          workspaceId: initialAccountTarget.workspaceId,
                          worktreeSlug: initialAccountTarget.worktreeSlug,
                          worktreeLabel: initialAccountTarget.worktreeLabel,
                          ...(initialAccountTarget.sessionId ? { sessionId: initialAccountTarget.sessionId } : {}),
                          ...(initialAccountTarget.daemonEpoch ? { daemonEpoch: initialAccountTarget.daemonEpoch } : {}),
                        });
                      }}
                      className="px-2.5 py-1 text-[11px] font-medium bg-primary text-primary-foreground rounded hover:bg-primary/90 transition-colors"
                    >
                      Retry Selection
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setCreationError(null);
                        setInitialAccountTarget(null);
                        clearPendingSelection();
                        workspaceRefreshVersionRef.current += 1;
                        setModel(EMPTY_MODEL);
                        if (activeTunnelConnection) {
                          activeTunnelConnection.close();
                          setActiveTunnelConnection(null);
                        }
                        setToken(null);
                        setSelectorOpen(true);
                      }}
                      className="px-2.5 py-1 text-[11px] font-medium border border-border text-muted-foreground hover:text-foreground rounded transition-colors"
                    >
                      Back to Worktrees
                    </button>
                  </div>
                </div>
              )}
            </div>
          ) : viewMode === "chat" ? renderLazy(
            <div className="flex-1 flex flex-col min-h-0 bg-chat-screen overflow-hidden">
              <MobileChatWorkspace
                headerTitle={
                  model.context.terminalTabs?.find((t) => t.id === model.context.activeTabId)?.label ??
                  model.context.activeTerminal?.title ??
                  model.context.workspaceId ??
                  undefined
                }
                headerSubtitle={model.context.worktreeLabel ?? model.context.workspaceId ?? undefined}
                onBack={() => setSelectorOpen(true)}
                messages={chatMessages}
                warnings={chatWarnings}
                isRunning={chatIsRunning}
                onSendMessage={(text: string, attachments: readonly ComposerAttachment[]) => {
                  // Attachments are blocked by the mobile composer until remote upload is supported.
                  if (attachments.length > 0) return;
                  const ws = terminalSocketRef.current;
                  const isSocketOpen = Boolean(ws && ws.readyState === 1 /* OPEN */);

                  if (!isSocketOpen) {
                    const socketClosedWarning =
                      "Message not sent: the terminal connection is closed. Reopen the terminal and try again.";
                    setChatWarnings((prev) =>
                      prev.includes(socketClosedWarning) ? prev : [...prev, socketClosedWarning],
                    );
                    console.warn("Terminal WebSocket is not open for input");
                    return;
                  }

                  const userMsg: MobileChatMessageProps = {
                    id: `user-${Date.now()}`,
                    role: "user",
                    content: text,
                    timestamp: Date.now(),
                    attachments: [],
                  };
                  setChatMessages((prev) => [...prev, userMsg]);
                  assistantTurnStartedAtRef.current = Date.now();
                  setChatIsRunning(true);

                  if (effectiveSessionId && token) {
                    const commandPayload = text.endsWith("\n") ? text : `${text}\n`;
                    if (activeTunnelConnection) {
                      ws!.send(new TextEncoder().encode(commandPayload));
                    } else {
                      ws!.send(commandPayload);
                    }
                  }
                }}
                onStopExecution={() => {
                  const ws = terminalSocketRef.current;
                  const isSocketOpen = Boolean(ws && ws.readyState === 1 /* OPEN */);
                  if (effectiveSessionId && token) {
                    if (isSocketOpen) {
                      if (activeTunnelConnection) {
                        ws!.send(new TextEncoder().encode("\x03"));
                      } else {
                        ws!.send("\x03");
                      }
                    } else {
                      const interruptFailedWarning =
                        "Interrupt not sent: the terminal connection is closed. Reopen the terminal and try again.";
                      setChatWarnings((prev) =>
                        prev.includes(interruptFailedWarning) ? prev : [...prev, interruptFailedWarning],
                      );
                      console.warn("Terminal WebSocket is not open for interrupt");
                    }
                  } else if (!isSocketOpen) {
                    const interruptFailedWarning =
                      "Interrupt not sent: the terminal connection is closed. Reopen the terminal and try again.";
                    setChatWarnings((prev) =>
                      prev.includes(interruptFailedWarning) ? prev : [...prev, interruptFailedWarning],
                    );
                    console.warn("Terminal WebSocket is not open for interrupt");
                  }
                  finalizeAssistantTurnDuration();
                  setChatIsRunning(false);
                }}
                sessionId={effectiveSessionId ?? undefined}
                token={token ?? undefined}
                transportUrl={transportBaseUrl}
                isAccountSession={Boolean(activeTunnelConnection)}
                createWebSocket={terminalCreateWebSocket}
              />
            </div>
          ) : viewMode === "browser" ? renderLazy(
            <div className="flex-1 flex flex-col min-h-0 bg-chat-screen overflow-hidden">
              {browserSessions.length > 0 && (
                <div className="flex items-center gap-1.5 px-2 py-1 bg-chat-surface border-b border-chat-border text-xs overflow-x-auto shrink-0">
                  <span className="text-chat-foreground-secondary text-[11px] shrink-0">Browsers:</span>
                  {browserSessions.map((s) => (
                    <button
                      key={s.browserId}
                      type="button"
                      data-testid={`select-browser-${s.browserId}`}
                      onClick={() => setSelectedBrowserId(s.browserId)}
                      className={`px-2 py-0.5 rounded text-[11px] font-medium transition shrink-0 ${
                        selectedBrowserId === s.browserId
                          ? "bg-chat-primary text-white"
                          : "bg-chat-surface-raised text-chat-foreground-secondary hover:bg-chat-surface-hover hover:text-chat-foreground"
                      }`}
                    >
                      {s.title || s.browserId}
                    </button>
                  ))}
                  <button
                    type="button"
                    onClick={() => void fetchBrowserSessions()}
                    className="ml-auto text-[11px] text-chat-foreground-secondary hover:text-chat-foreground"
                  >
                    Refresh
                  </button>
                </div>
              )}

              {selectedBrowserId && token ? (
                <RemoteBrowserWorkspace
                  baseUrl={transportBaseUrl}
                  browserId={selectedBrowserId}
                  deviceToken={token}
                  onBack={() => setViewMode("terminal")}
                />
              ) : (
                <div className="flex flex-col items-center justify-center flex-1 p-4 text-center text-chat-foreground-secondary gap-3">
                  <p className="text-sm font-medium">No active browser session selected</p>
                  <div className="flex items-center gap-2">
                    <input
                      type="text"
                      placeholder="Enter browser ID..."
                      data-testid="remote-manual-browser-id-input"
                      className="px-2 py-1 text-xs rounded bg-chat-surface border border-chat-border text-chat-foreground font-mono"
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && (e.target as HTMLInputElement).value.trim()) {
                          setSelectedBrowserId((e.target as HTMLInputElement).value.trim());
                        }
                      }}
                    />
                    <button
                      type="button"
                      data-testid="remote-fetch-browsers-btn"
                      onClick={() => void fetchBrowserSessions()}
                      className="px-2.5 py-1 text-xs rounded bg-chat-surface-raised text-chat-foreground font-medium hover:bg-chat-surface-hover transition"
                    >
                      Refresh Sessions
                    </button>
                  </div>
                </div>
              )}
            </div>
          ) : effectiveSessionId && token ? renderLazy(
            <RemoteTerminal
              key={`${effectiveSessionId}:${terminalRetryGeneration}`}
              sessionId={effectiveSessionId}
              token={token}
              title={model.context.worktreeLabel ?? model.context.workspaceId ?? undefined}
              transportUrl={transportBaseUrl}
              onTransportFailure={transport.url !== relayUrl ? rollbackTransport : undefined}
              activeTabId={model.context.activeTabId}
              onBack={() => setViewMode("chat")}
              embedded
              onSwipePreviousTab={handleSwipePreviousTab}
              onSwipeNextTab={handleSwipeNextTab}
              onSocketLifecycle={handleTerminalSocketLifecycle}
              isAccountSession={Boolean(activeTunnelConnection)}
              daemonEpoch={sessionEpochs[effectiveSessionId] ?? sessionEpochsRef.current.get(effectiveSessionId)}
              onAgentStateFrame={handleMachineAgentStateFrame}
              createWebSocket={terminalCreateWebSocket}
            />
          ) : null}
      </RemoteWorkspaceMirror>

      <MobileHostDrawer
        open={hostDrawerOpen}
        onOpenChange={setHostDrawerOpen}
        onDisconnect={disconnect}
        onSignOut={handleSignOut}
        isAccountSession={Boolean(activeTunnelConnection)}
      />
    </div>
  );
};
