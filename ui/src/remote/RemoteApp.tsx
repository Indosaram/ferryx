import { ChevronDown } from "lucide-react";
import React, { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Toaster } from "../components/ui/sonner";
import { createCoalescedRunner } from "./coalescedRunner";
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
import { remoteHostStore, selectActiveHost } from "../state/remoteHostStore";
import { MobileHostDrawer } from "./MobileHostDrawer";
import {
  contextName,
  getRemoteDocumentTitle,
  normalizeRemoteWorkspaceState,
  paneSelectionOption,
  RemoteWorkspaceMirror,
  useRemoteDesktopSidebar,
  type RemoteContextOption,
  type RemoteTerminalTabInfo,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";
import { BUILD_STAMP } from "../lib/buildStamp";
import { MobileTopBar, type MobileTopBarActivityState } from "./chat/MobileTopBar";
import { MobileSessionSwitcherSheet } from "./chat/MobileSessionSwitcherSheet";
import { MobileOverflowSheet } from "./chat/MobileOverflowSheet";
import { capRetainedMessages } from "./agentConversation";
import {
  ReferenceHistoryFetchError,
  parseReferenceHistoryPage,
  referenceHistoryCursorForStream,
  referenceHistoryErrorIsIdentity,
  referenceHistoryFence,
  referenceHistoryNotice,
  type ReferenceHistoryFence,
} from "./agentConversation";
import type { MobileChatMessageProps } from "./chat/MobileChatMessage";
import type { MobileChatComposerHandle } from "./chat/MobileChatComposer";
import { referenceDrafts } from "./chat/referenceDraft";
import { referenceQueues, type HeldMessage } from "./chat/referenceQueue";
import { ReferencePromptCard } from "./chat/ReferencePromptCard";
import {
  ReferenceFileError,
  cancelReferenceChatFile,
  parseReferenceStagedFileList,
  referenceMentionInsertion,
  referenceMentionPathOf,
  stageReferenceChatFile,
  type ReferenceFileStagingTransport,
} from "./chat/referenceFiles";
import {
  referenceAnswerFromText,
  referenceAnswerHint,
  REFERENCE_PROMPT_STALE_MESSAGE,
} from "./chat/referencePromptAnswer";
import {
  REFERENCE_OUTCOME_UNKNOWN_CODE,
  REFERENCE_SUBMIT_MAX_CHARS,
  referenceAnswerIsSingleChoice,
  referenceChatRoute,
  referenceIsOutcomeUnknown,
  referenceMentionFor,
  referenceNativeKindFromRegistryId,
  referenceNativeKindIsNative,
  referencePromptNeedsConfirmation,
  sameReferenceTarget,
  referenceTargetKey,
  type ReferenceFileReceipt,
  type ReferenceHistoryCursor,
  type ReferenceHistoryPage,
  type ReferenceImageRef,
  type ReferencePrompt,
  type ReferencePromptAnswer,
  type ReferencePromptAnswerPayload,
  type ReferenceStopCapability,
  type ReferenceStopPayload,
  type ReferenceSubmitPayload,
  type ReferenceTargetRef,
  type ReferenceTurn,
} from "./chat/referenceTypes";
import {
  formatReferenceDuration,
  type ReferencePartRenderContext,
} from "./chat/MobileChatComponents";
import {
  hostTransportUrl,
  noteTokenRotation,
  onDeviceTokenRotated,
  remoteFetch,
  remoteOpenSocket,
  remoteSocketTarget,
} from "./remoteClient";

import { AccountLoginPage } from "./AccountLoginPage";
import { ApprovePage, isApproveRoute } from "./ApprovePage";
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
  getStoredAccountEntitlementSnapshot,
  storeAccountEntitlementSnapshot,
  clearStoredAccountEntitlementSnapshot,
  planLimitStateFromError,
  planLimitStateFromCloseEvent,
  planLimitStateWithEntitlement,
  type AccountConnection,
  type PlanLimitState,
} from "./accountSession";
import { PlanLimitNotice } from "./PlanLimitNotice";
import { deleteAccount } from "./accountSession";
import { paneSupervision } from "../lib/supervisorModel";
import { toast } from "sonner";
import { AccountSheet, getStoredAccountEmail, storeAccountEmail } from "./home/AccountSheet";
import { createSupervisorFrameBus } from "./home/machineSupervisorSources";
import { DIRECT_SIDEBAR_MACHINE, useRemoteSidebarMachines } from "./useRemoteSidebarMachines";
import { remoteSupervisorTyping } from "./remoteSidebarLive";
import { directSidebarInventory } from "./directSidebarInventory";
import { useRemoteSidebarModel } from "./useRemoteSidebarModel";
import { SupervisorHome } from "./home/SupervisorHome";
import { buildInboxRows, optionForSession, type InboxRow } from "./home/phoneHomeModel";
import { PhoneHomeView } from "./home/PhoneHomeView";
import { SupervisorSessionStrip } from "./home/SupervisorSessionStrip";
import { usePhoneNavigation } from "./home/usePhoneNavigation";
import { usePhoneSupervisor } from "./home/usePhoneSupervisor";
import { WorktreesHome } from "./home/WorktreesHome";
import type { TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import { getOrCreateAttachKey } from "./accountAttach";
import {
  openRelayE2ETransport,
  RELAY_REPAIR_MESSAGE,
  registerRelayE2E,
  RelayRepairRequiredError,
  relayE2EState,
  REPAIR_REQUIRED,
} from "./relayE2E";

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
 * Reads direct-endpoint hints from the current URL query first and falls back to the last
 * hints this device stored. Any hint found in the URL is persisted so later loads keep the
 * fast path.
 */
function readDirectCandidateHints(hostId: string, readUrl: boolean): CandidateEndpoint[] {
  const storageKey = `${DIRECT_HINT_STORAGE_KEY}_${hostId}`;
  const params = new URLSearchParams(readUrl ? window.location.search : "");
  const fromUrl = [
    ...(params.get("hints") ?? "").split(",").map((value) => {
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

/* ------------------------------------------------------------------------- *
 * Herdr reference chat lane (plan task 12)
 *
 * The chat is a lens over the pane's own program. History, prompts, submit, Stop and files all
 * speak to the ORIGINAL session through the frozen reference-chat routes; the raw terminal
 * socket belongs to the explicit terminal mode alone. Nothing here starts a managed child,
 * fabricates an assistant turn, or replays a mutation whose outcome is unknown.
 * ------------------------------------------------------------------------- */

/**
 * The host id a MUTATION names when the gateway publishes none.
 *
 * `reference_chat_target()` compares a caller-named host id against its own `reference_host_id()`
 * — `FERRYX_HOST_ID` when the deployment sets one, `local` otherwise — so `local` is the
 * documented default, never a universal substitute. The lane prefers the id the gateway
 * publishes (see `ChatGatewayIdentity.referenceHostId`), and a READ names no host at all: the
 * gateway then answers from its own identity, which is what keeps a renamed host correct. A
 * deployment that renames its host must publish that id; until it does, a mutation from here
 * answers a typed FORBIDDEN instead of guessing another host.
 */
const REFERENCE_CHAT_DEFAULT_HOST_ID = "local";

/** Where a lane's own host answers: its base URL and the bearer it accepts. */
interface ChatLaneRoute {
  readonly baseUrl: string;
  readonly token: string | null;
}

/**
 * The identity the owning host publishes for itself, as `/api/v1/capabilities` answers it.
 *
 * `daemonEpoch` is the value the reference-chat route compares a target against, so it is the
 * authoritative incarnation on EVERY transport (relay, direct and the account tunnel alike).
 * `referenceHostId` is the gateway's own `reference_host_id()` where it publishes one.
 * `referenceOwnerId` is the OWNER authority for reference-chat targets, published beside the host
 * id by the same gateway and on the same instance/incarnation lifetime as `daemonEpoch`. It is
 * never asserted by this client, never taken from the environment and never derived from the
 * request: a target that does not echo it is refused TARGET_EXPIRED exactly like a stale epoch,
 * and a gateway that publishes none leaves the lane with no target at all rather than an owner
 * this client invented.
 * `machineId` is the host's machine identity: recorded for diagnostics, NEVER substituted for
 * the host id, because the two are different values.
 */
export interface ChatGatewayIdentity {
  readonly daemonEpoch: string;
  readonly referenceHostId: string | null;
  readonly referenceOwnerId: string | null;
  readonly machineId: string | null;
}

/** Field names a gateway may publish its reference host id under. */
const CHAT_HOST_ID_FIELDS: readonly string[] = ["referenceHostId", "hostId"];

/**
 * The one field a gateway publishes its reference OWNER authority under.
 *
 * Deliberately not an alias list, unlike the host id: the route compares a target's owner against
 * this value, so accepting a second spelling would be this client choosing which field is
 * authoritative. Absence is absence - it never falls back to a configured or local owner.
 */
const CHAT_OWNER_ID_FIELD = "referenceOwnerId";

function chatText(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

function chatRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" ? (value as Record<string, unknown>) : null;
}

/**
 * Parse the authenticated capabilities payload. A missing or non-canonical epoch is not an
 * identity: without the exact value the route compares against there is no target at all.
 */
export function parseChatGatewayIdentity(raw: unknown): ChatGatewayIdentity | null {
  const body = chatRecord(raw);
  if (body === null) return null;
  const daemonEpoch = chatText(body.daemonEpoch);
  if (daemonEpoch === null || !/^(?:0|[1-9][0-9]*)$/.test(daemonEpoch)) return null;
  let referenceHostId: string | null = null;
  for (const field of CHAT_HOST_ID_FIELDS) {
    const value = chatText(body[field]);
    if (value !== null) {
      referenceHostId = value;
      break;
    }
  }
  return {
    daemonEpoch,
    referenceHostId,
    referenceOwnerId: chatText(body[CHAT_OWNER_ID_FIELD]),
    machineId: chatText(body.machineId),
  };
}

/** The bearer and host an identity was answered for; any other pair has none until it answers. */
function chatGatewayScope(token: string, baseUrl: string): string {
  return `${baseUrl}\u0000${token}`;
}

/** The gateway's own identity, from the one authenticated route that publishes it. */
export async function readChatGatewayIdentity(args: {
  baseUrl: string;
  token: string;
  signal?: AbortSignal;
}): Promise<ChatGatewayIdentity | null> {
  let response: Response;
  try {
    response = await remoteFetch(args.baseUrl, "/api/v1/capabilities", {
      headers: { Authorization: `Bearer ${args.token}` },
      signal: args.signal,
    });
  } catch {
    return null;
  }
  if (!response.ok) return null;
  noteTokenRotation(args.token, response);
  try {
    return parseChatGatewayIdentity(await response.json());
  } catch {
    return null;
  }
}

/**
 * The provider session id the owning host published for THIS session.
 *
 * Only the machine-scope session row carries one (`providerSession.id`). A row naming another
 * session contributes nothing, and two rows naming this one with DIFFERENT ids are refused: an
 * ambiguous provider identity is never guessed, and the reader then binds by the host's own
 * owner/source rules rather than by a conversation this client picked. Absence means unknown —
 * never permission to read another session's file, and never "the newest" or a same-cwd match.
 */
export function chatProviderSessionId(rows: readonly unknown[], sessionId: string): string | null {
  const found = new Set<string>();
  for (const raw of rows) {
    const row = chatRecord(raw);
    if (row === null) continue;
    const target = chatRecord(row.target);
    const rowSessionId = chatText(row.sessionId) ?? (target === null ? null : chatText(target.sessionId));
    if (rowSessionId !== sessionId) continue;
    const provider = chatRecord(row.providerSession);
    const id = provider === null ? null : chatText(provider.id);
    if (id !== null) found.add(id);
  }
  return found.size === 1 ? [...found][0] : null;
}

/**
 * The daemon incarnation a session row published for THIS session, on the same no-guessing rule:
 * two rows that disagree are ambiguous and yield nothing.
 */
export function chatDaemonEpochFromRows(rows: readonly unknown[], sessionId: string): string | null {
  const found = new Set<string>();
  for (const raw of rows) {
    const row = chatRecord(raw);
    if (row === null) continue;
    const target = chatRecord(row.target);
    const rowSessionId = chatText(row.sessionId) ?? (target === null ? null : chatText(target.sessionId));
    if (rowSessionId !== sessionId) continue;
    const epoch = chatText(row.daemonEpoch) ?? (target === null ? null : chatText(target.daemonEpoch));
    if (epoch !== null) found.add(epoch);
  }
  return found.size === 1 ? [...found][0] : null;
}

/** The owning host's session rows, on whichever transport this connection uses. */
export async function fetchChatSessionRows(args: {
  baseUrl: string;
  token: string;
  tunnel: { transport: TunnelTransport } | null;
  signal?: AbortSignal;
}): Promise<unknown[]> {
  const readRows = (data: unknown): unknown[] => {
    const record = chatRecord(data);
    if (Array.isArray(data)) return data;
    if (record !== null && Array.isArray(record.sessions)) return record.sessions;
    return [];
  };
  try {
    if (args.tunnel !== null) {
      const res = await args.tunnel.transport.fetchLike("/api/v1/sessions", {
        headers: { Authorization: `Bearer ${args.token}` },
      });
      if (res.status < 200 || res.status >= 300) return [];
      return readRows(JSON.parse(new TextDecoder().decode(res.body)));
    }
    const response = await remoteFetch(args.baseUrl, "/api/v1/sessions", {
      headers: { Authorization: `Bearer ${args.token}` },
      signal: args.signal,
    });
    if (!response.ok) return [];
    return readRows(await response.json());
  } catch {
    return [];
  }
}

const CHAT_REFRESH_WARNING = "Could not refresh the transcript; showing the last known state.";
const CHAT_UNAVAILABLE_WARNING = "This session's transcript is not available to this device.";
const CHAT_NO_READER_WARNING =
  "This pane runs no agent, so there is no transcript reader for it. Open the terminal to see its output.";

const CHAT_RETENTION_WARNING = "Older messages are hidden to keep the phone view responsive.";
const CHAT_SEND_FAILED_WARNING = "That message was not delivered. It is back in the message box.";
const CHAT_OUTCOME_UNKNOWN_WARNING =
  "That message may have reached the session, but the host did not confirm it. It is back in the message box and will not be sent again on its own.";
const CHAT_STOP_REFUSED_WARNING =
  "The chat does not know how to stop this pane, so nothing was sent. Open the terminal if you need to interrupt it.";
const CHAT_STOP_FAILED_WARNING = "The stop request was not delivered.";
const CHAT_HELD_WARNING = "Held: this prompt takes one of its own options, so nothing was sent.";
const CHAT_AMBIGUOUS_ANSWER_WARNING = "That answer did not name exactly one option, so it was not sent.";
const CHAT_NO_SESSION_WARNING = "This session is not attached yet, so nothing was sent.";
const CHAT_SUBMIT_TOO_LONG_WARNING = "That message is longer than the composer allows, so it was not sent.";
const CHAT_OLDER_FAILED_WARNING = "Could not load earlier messages.";

/** The reference-chat page size for the newest read and for an older page. */
const CHAT_HISTORY_PAGE_LIMIT = 200;

/** Add a warning once: the same fact repeated is noise, not information. */
function withChatWarning(warnings: readonly string[], added: string): readonly string[] {
  return warnings.includes(added) ? warnings : [...warnings, added];
}

/** Everything a reference turn draws, as one string, cached per message object. */
const chatTurnSignatures = new WeakMap<MobileChatMessageProps, string>();
function chatTurnSignature(message: MobileChatMessageProps): string {
  let signature = chatTurnSignatures.get(message);
  if (signature === undefined) {
    signature = JSON.stringify([
      message.role,
      message.content,
      message.timestamp ?? null,
      message.durationLabel ?? null,
      message.referenceSource ?? null,
      message.referenceAbandoned ?? null,
      message.referenceParts ?? null,
    ]);
    chatTurnSignatures.set(message, signature);
  }
  return signature;
}

function newChatRequestId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
  );
}

/** The pane's own stop behavior, or an honest refusal where the chat cannot know it. */
function chatStopCapabilityFor(agentType: string | null | undefined): ReferenceStopCapability {
  if (!agentType) return "refused";
  return referenceNativeKindIsNative(referenceNativeKindFromRegistryId(agentType))
    ? "providerInterrupt"
    : "refused";
}

/** A turn's recorded duration: last assistant activity minus its start, never inferred. */
function chatTurnDuration(startedAt?: string | null, endedAt?: string | null): string | undefined {
  if (!startedAt || !endedAt) return undefined;
  const start = Date.parse(startedAt);
  const end = Date.parse(endedAt);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return undefined;
  return formatReferenceDuration(end - start);
}

/** A stable identity for a turn, so a prepended older page does not remount the ones below. */
function chatTurnSeed(turn: ReferenceTurn): string {
  const first = turn.parts.find((part) => part.kind === "text") as { text?: string } | undefined;
  return `${turn.role}|${turn.startedAt ?? ""}|${(first?.text ?? "").slice(0, 64)}`;
}

function chatTurnId(seed: string): string {
  let hash = 0;
  for (let index = 0; index < seed.length; index += 1) {
    hash = (hash * 31 + seed.charCodeAt(index)) >>> 0;
  }
  return `turn-${hash.toString(36)}`;
}

/**
 * Where a slid window joins the one before it: the first index whose suffix of `previous` is a
 * prefix of `next`, compared turn by turn. A single id is not enough when turns repeat.
 */
function chatSlideJoin(previous: readonly MobileChatMessageProps[], next: readonly MobileChatMessageProps[]): number {
  for (let start = 0; start < previous.length; start += 1) {
    const overlap = Math.min(previous.length - start, next.length);
    let matches = true;
    for (let index = 0; index < overlap; index += 1) {
      if (previous[start + index].id !== next[index].id) {
        matches = false;
        break;
      }
    }
    if (matches) return start;
  }
  return -1;
}

/**
 * One reference page as chat messages: a turn per message, its rich parts intact, its own
 * abandoned disclosure left for the page to compose. No assistant turn is ever invented.
 */
function mapReferenceTurns(page: ReferenceHistoryPage): MobileChatMessageProps[] {
  return page.turns.map((turn) => {
    const seed = chatTurnSeed(turn);
    const text = turn.parts
      .map((part) =>
        part.kind === "text" || part.kind === "thinking" || part.kind === "compact" ? part.text : null,
      )
      .filter((part): part is string => part !== null)
      .join("\n\n");
    return {
      id: chatTurnId(seed),
      role: turn.role,
      content: text,
      timestamp: turn.startedAt ?? undefined,
      referenceParts: turn.parts,
      referenceSource: turn.source ?? null,
      referenceAbandoned: turn.abandoned ?? null,
      durationLabel:
        turn.role === "assistant" ? chatTurnDuration(turn.startedAt, turn.endedAt) : undefined,
    };
  });
}

/** A typed refusal from the prompt lane; the card reads `code` to tell a moved screen apart. */
class ReferencePromptRefused extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "ReferencePromptRefused";
    this.code = code;
  }
}

/** The prompt the original pane is waiting on, or null. Anything unreadable is no prompt. */
function parseChatPrompt(raw: unknown): ReferencePrompt | null {
  if (raw === null || raw === undefined || typeof raw !== "object") return null;
  const body = raw as Record<string, unknown>;
  const inner = body.prompt === undefined ? body : body.prompt;
  if (inner === null || inner === undefined || typeof inner !== "object") return null;
  const prompt = inner as Record<string, unknown>;
  const id =
    typeof prompt.promptId === "string" ? prompt.promptId : typeof prompt.id === "string" ? prompt.id : null;
  const agent = typeof prompt.agent === "string" ? prompt.agent : null;
  const kind = typeof prompt.kind === "string" ? prompt.kind : null;
  const title = typeof prompt.title === "string" ? prompt.title : null;
  const question = typeof prompt.question === "string" ? prompt.question : null;
  if (id === null || agent === null || kind === null || title === null || question === null) return null;
  if (kind !== "question" && kind !== "approval" && kind !== "plan" && kind !== "menu") return null;
  const rawOptions = Array.isArray(prompt.options) ? prompt.options : [];
  const options = rawOptions.map((option) => {
    const entry = (option ?? {}) as Record<string, unknown>;
    const label = typeof entry.label === "string" ? entry.label : "";
    const description = typeof entry.description === "string" ? entry.description : null;
    return description === null ? { label } : { label, description };
  });
  const steps = Array.isArray(prompt.steps)
    ? prompt.steps.map((step, index) => {
        const entry = (step ?? {}) as Record<string, unknown>;
        return {
          label: typeof entry.label === "string" ? entry.label : `Step ${index + 1}`,
          answered: entry.answered === true,
          current: entry.current === true,
        };
      })
    : null;
  return {
    id,
    agent,
    kind,
    title,
    question,
    options,
    multiSelect: prompt.multiSelect === true,
    ...(typeof prompt.body === "string" ? { body: prompt.body } : {}),
    ...(typeof prompt.customOptionIndex === "number" ? { customOptionIndex: prompt.customOptionIndex } : {}),
    ...(prompt.queued === "open" || prompt.queued === "collapsed" ? { queued: prompt.queued } : {}),
    ...(prompt.fallback === true ? { fallback: true } : {}),
    ...(steps !== null ? { steps } : {}),
  };
}

/** The screen revision the card was rendered from; the answer names it, so a move is refused. */
function parseChatScreenRevision(raw: unknown): string | null {
  if (raw === null || raw === undefined || typeof raw !== "object") return null;
  const body = raw as Record<string, unknown>;
  const revision = body.screenRevision;
  return typeof revision === "string" && revision.length > 0 ? revision : null;
}

/** The status a failed reference-chat read maps to, when the body names no code. */
function chatStatusErrorCode(status: number): string {
  switch (status) {
    case 400:
      return "INVALID_REQUEST";
    case 401:
      return "UNAUTHORIZED";
    case 403:
      return "FORBIDDEN";
    case 404:
      return "NOT_FOUND";
    case 409:
      return "REQUEST_CONFLICT";
    case 410:
      return "TARGET_EXPIRED";
    case 413:
      return "PAYLOAD_TOO_LARGE";
    case 422:
      return "UNSUPPORTED";
    case 503:
      return "INVENTORY_INCOMPLETE";
    case 504:
      return "TIMEOUT";
    default:
      return "REQUEST_FAILED";
  }
}

/** The typed failure inside a machine error envelope or a frozen ScopeResult. */
function chatFailureOf(record: Record<string, unknown>): {
  code: string;
  message: string;
  retryable: boolean;
} | null {
  const raw = record.error;
  if (raw === null || raw === undefined) return null;
  if (typeof raw === "string") return { code: raw, message: raw, retryable: false };
  if (typeof raw !== "object") return null;
  const entry = raw as Record<string, unknown>;
  const code = typeof entry.code === "string" ? entry.code : "REQUEST_FAILED";
  const message = typeof entry.message === "string" ? entry.message : code;
  return { code, message, retryable: entry.retryable === true };
}

type ChatMutationOutcome =
  | { readonly ok: true; readonly data: Record<string, unknown> }
  | { readonly ok: false; readonly code: string; readonly message: string; readonly retryable: boolean };

function submittedFileKey(target: ReferenceTargetRef, id: string): string {
  return `${referenceTargetKey(target)}\u0000${id}`;
}

/** Validated receipts are owned by every durable held row still naming their mention. */
function heldChatFileIsReferenced(target: ReferenceTargetRef, file: ReferenceFileReceipt): boolean {
  referenceQueues.refresh(target);
  const path = referenceMentionPathOf(file.mentionText);
  return path !== null && referenceQueues.read(target).some(
    (row) => row.text.includes(referenceMentionFor(path).trimEnd()),
  );
}

/**
 * One mutation, as the frozen envelope carries it. The route answers the frozen ScopeResult, and
 * a refusal before the mutation answers the machine error envelope; both are read here.
 */
async function postChatMutation(
  baseUrl: string,
  sessionId: string,
  token: string,
  route: string,
  body: unknown,
  signal?: AbortSignal,
): Promise<ChatMutationOutcome> {
  let response: Response;
  try {
    response = await remoteFetch(baseUrl, referenceChatRoute(sessionId, route), {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal,
    });
  } catch (error) {
    return {
      ok: false,
      code: REFERENCE_OUTCOME_UNKNOWN_CODE,
      message: error instanceof Error ? error.message : String(error),
      retryable: false,
    };
  }
  let parsed: unknown = null;
  try {
    parsed = await response.json();
  } catch {
    parsed = null;
  }
  if (parsed !== null && typeof parsed === "object") {
    const record = parsed as Record<string, unknown>;
    if (record.ok === true) {
      const data = record.data;
      return { ok: true, data: data !== null && typeof data === "object" ? (data as Record<string, unknown>) : {} };
    }
    const failure = chatFailureOf(record);
    if (failure !== null) return { ok: false, ...failure };
  }
  return {
    ok: false,
    code: REFERENCE_OUTCOME_UNKNOWN_CODE,
    message: `The reference-chat request failed with status ${response.status}.`,
    retryable: false,
  };
}

/** The query a reference-chat read binds its target with, as the route reads it. */
function referenceChatReadQuery(
  target: ReferenceTargetRef,
  registryId: string,
  limit?: number,
  cursor?: ReferenceHistoryCursor | null,
): string {
  const { ownerId, epoch, backendSessionId } = target.target;
  const params = new URLSearchParams();
  // A READ names no host: the route falls back to the gateway's own `reference_host_id()`, so a
  // renamed host answers from its own identity instead of a client-side guess. A mutation still
  // carries a host id, because the frozen envelope requires one.
  // The owner is queried so the route can compare it: it is the gateway's published authority,
  // and a read that names an owner the live incarnation did not publish is refused, not served.
  params.set("ownerId", ownerId);
  params.set("epoch", epoch);
  params.set("backendSessionId", backendSessionId);
  params.set("registryId", registryId);
  const providerSessionId = target.providerSessionId;
  if (typeof providerSessionId === "string" && providerSessionId.trim().length > 0) {
    params.set("providerSessionId", providerSessionId);
  }
  if (limit !== undefined) params.set("limit", String(limit));
  if (cursor) {
    params.set("cursor", String(cursor.offset));
    params.set("cursorStream", cursor.streamId);
  }
  return params.toString();
}

/** The typed code a failed read answered with, or the status as a code. */
async function readChatErrorCode(response: Response): Promise<string> {
  try {
    const parsed: unknown = JSON.parse(await response.text());
    if (parsed !== null && typeof parsed === "object") {
      const failure = chatFailureOf(parsed as Record<string, unknown>);
      if (failure !== null) return failure.code;
    }
  } catch {
    /* a body that is not JSON is answered by its status */
  }
  return chatStatusErrorCode(response.status);
}

/** One history read, bound to the target and the registry entry the pane runs. */
async function readChatHistory(args: {
  baseUrl: string;
  sessionId: string;
  token: string;
  target: ReferenceTargetRef;
  registryId: string;
  limit: number;
  cursor?: ReferenceHistoryCursor | null;
  signal?: AbortSignal;
  transport?: Pick<TunnelTransport, "fetchLike"> | null;
}): Promise<{ page: ReferenceHistoryPage; fence: ReferenceHistoryFence }> {
  const path = `${referenceChatRoute(args.sessionId, "history")}?${referenceChatReadQuery(
    args.target,
    args.registryId,
    args.limit,
    args.cursor,
  )}`;
  let response: Response;
  try {
    args.signal?.throwIfAborted();
    if (args.transport) {
      const raw = await args.transport.fetchLike(path, {
        headers: { Authorization: `Bearer ${args.token}` },
      });
      args.signal?.throwIfAborted();
      response = new Response(raw.body.slice(), { status: raw.status, headers: raw.headers });
    } else {
      response = await remoteFetch(args.baseUrl, path, {
        headers: { Authorization: `Bearer ${args.token}` },
        signal: args.signal,
      });
    }
  } catch (error) {
    if (args.signal?.aborted) throw error;
    throw new ReferenceHistoryFetchError(
      "NETWORK_ERROR",
      error instanceof Error ? error.message : String(error),
    );
  }
  if (!response.ok) {
    throw new ReferenceHistoryFetchError(
      await readChatErrorCode(response),
      `Reference history request failed with status ${response.status}`,
    );
  }
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    throw new ReferenceHistoryFetchError(
      "MALFORMED_RESPONSE",
      "Reference history response body is not valid JSON",
    );
  }
  const page = parseReferenceHistoryPage(body);
  return { page, fence: referenceHistoryFence(args.sessionId, args.target, page.generation) };
}

export const RemoteApp: React.FC = () => {
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const [magicLinkOverridden, setMagicLinkOverridden] = useState(() => hasMagicLinkCode());
  // The mode the user asked for lives here, ABOVE the keyed connection below: the key is the
  // host identity, so a pairing adoption or a host switch remounts that subtree. Chat stays the
  // default because this state is born chat; an explicit terminal choice now survives the
  // remount instead of being discarded with the component that held it. Nothing is persisted and
  // no store is involved - the owner is simply the first component that does not remount.
  const [viewMode, setViewMode] = useState<"chat" | "terminal" | "browser">("chat");
  // Hooks stay above every early return. The /approve route is fixed for the page's lifetime.
  const [approveRoute] = useState(() => isApproveRoute(window.location.pathname));
  if (approveRoute) {
    // The approval link points at the account origin itself, not at a saved host.
    return <ApprovePage relayUrl={window.location.origin} />;
  }
  const bypassSavedHost = magicLinkOverridden;
  const host = bypassSavedHost ? null : selectActiveHost(state);
  const hostId = (bypassSavedHost ? null : state.activeHostId) ?? `local:${window.location.origin}`;
  const address = host?.address ?? window.location.origin;
  const relayUrl = new URL(address.includes("://") ? address : `http://${address}`).origin;
  return (
    <RemoteHostConnection
      key={`${hostId}:${relayUrl}`}
      hostId={hostId}
      relayUrl={relayUrl}
      readUrlHints={state.activeHostId === null || magicLinkOverridden}
      viewMode={viewMode}
      onViewModeChange={setViewMode}
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
  // The mode owner is above this component when a caller supplies it: this subtree is keyed by
  // the host identity, so anything it owns alone is lost on a host switch. Callers that keep no
  // mode state (the desktop shell, the selection-lifetime harness) leave these out and get the
  // same chat default from the local fallback.
  viewMode?: "chat" | "terminal" | "browser";
  onViewModeChange?: (mode: "chat" | "terminal" | "browser") => void;
  initialMagicLinkRequested?: boolean;
  onMagicLinkConsumed?: () => void;
}> = ({ hostId, relayUrl, readUrlHints, viewMode: controlledViewMode, onViewModeChange, initialMagicLinkRequested = false, onMagicLinkConsumed }) => {
  const desktopSidebar = useRemoteDesktopSidebar();
  const [magicLinkActive, setMagicLinkActive] = useState(initialMagicLinkRequested);
  const [token, setToken] = useState<string | null>(() => {
    if (initialMagicLinkRequested) return null;
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
  // The host rotates a bearer token near its absolute expiry when this client reconnects. Adopt
  // the replacement only for the credential this host actually stores, so a rotation answered
  // for another host's token, or after a re-pair, cannot overwrite it.
  useEffect(() => onDeviceTokenRotated((previous, fresh) => {
    const host = remoteHostStore.getState().hosts[hostId];
    const stored = host?.deviceToken ?? getRemoteAuthToken(hostId);
    if (stored !== previous) return;
    setRemoteAuthToken(fresh, hostId);
    if (host) remoteHostStore.upsertHost({ ...host, deviceToken: fresh });
    setToken((current) => (current === previous ? fresh : current));
  }), [hostId]);
  const [model, setModel] = useState<RemoteWorkspaceModel>(EMPTY_MODEL);
  const modelRef = useRef(model);
  const modelMachineIdRef = useRef<string | null>(null);
  // Set by every user-driven selection that names a session or tab: the read-only
  // desktop-context sync must never replace an independently chosen session.
  const independentSessionRef = useRef(false);
  modelRef.current = model;
  const [pending, setPending] = useState<RemoteContextOption | null>(null);
  const [selectorOpen, setSelectorOpen] = useState(false);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
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
  // Chat is the default at EVERY width: the terminal is a mode the user asks for, never the
  // landing surface of a phone or a desktop. One owner decides the mode, so no nested drawer
  // can mount the same session twice. When the caller owns that mode, the choice outlives this
  // keyed subtree; the local fallback keeps the same default for callers that do not.
  const [localViewMode, setLocalViewMode] = useState<"chat" | "terminal" | "browser">("chat");
  const viewMode = controlledViewMode ?? localViewMode;
  const setViewMode = onViewModeChange ?? setLocalViewMode;
  const [chatMessages, setChatMessages] = useState<MobileChatMessageProps[]>([]);
  const [chatWarnings, setChatWarnings] = useState<readonly string[]>([]);
  // The lane the chat speaks to: the owning target of the session in focus.

  const [chatDraftText, setChatDraftText] = useState("");
  const [chatStagedFiles, setChatStagedFiles] = useState<readonly ReferenceFileReceipt[]>([]);
  const [chatAttaching, setChatAttaching] = useState(false);
  const [chatAttachError, setChatAttachError] = useState<string | null>(null);
  const [chatHeld, setChatHeld] = useState<readonly HeldMessage[]>([]);
  // Every held row whose send is still on its way: one id per row, so a second row's send never
  // re-enables the first, and a settled row never re-enables one still in flight.
  const [chatHeldSendingIds, setChatHeldSendingIds] = useState<readonly string[]>([]);
  const chatHeldSendingRef = useRef<Set<string>>(new Set());
  // Older pages, kept apart from the newest page so the newest-page poll never overwrites them.
  const [chatOlderMessages, setChatOlderMessages] = useState<MobileChatMessageProps[]>([]);
  const [chatOlderLoading, setChatOlderLoading] = useState(false);
  const [chatOlderFailed, setChatOlderFailed] = useState(false);
  const [chatLoadedOlder, setChatLoadedOlder] = useState(false);
  const [chatPage, setChatPage] = useState<ReferenceHistoryPage | null>(null);
  const [chatOlderCursor, setChatOlderCursor] = useState<ReferenceHistoryCursor | null>(null);
  const [chatHasOlder, setChatHasOlder] = useState(false);
  const chatLoadedOlderRef = useRef(false);
  // Where the newest page started when the older pages were joined to it: a later newest page
  // that starts anywhere else no longer meets them, and they are dropped rather than left a gap.
  const chatNewestStartRef = useRef<ReferenceHistoryCursor | null>(null);
  // The turns the last newest page held: a window that slid forward hands the ones it dropped to
  // the older block instead of leaving them out.
  const chatNewestTurnsRef = useRef<MobileChatMessageProps[]>([]);
  // Bumped whenever the older chain's anchor moves or the transcript clears: an older read that
  // started under another generation joins nothing on screen and is dropped when it lands.
  const chatOlderGenerationRef = useRef(0);
  const [chatPrompt, setChatPrompt] = useState<ReferencePrompt | null>(null);
  const chatPromptIdRef = useRef<string | null>(null);
  const chatPromptRevisionRef = useRef<string | null>(null);
  const chatPromptGenerationRef = useRef(0);
  const chatComposerRef = useRef<MobileChatComposerHandle | null>(null);
  const [chatScreenRevision, setChatScreenRevision] = useState<string | null>(null);
  const [chatTypedAnswer, setChatTypedAnswer] = useState<ReferencePromptAnswer | null>(null);
  const [chatPromptError, setChatPromptError] = useState<string | null>(null);
  const [chatPromptRefresh, setChatPromptRefresh] = useState(0);
  /** The owning host's own identity, as its authenticated capabilities answer publishes it. */
  const [chatGatewayAnswer, setChatGatewayAnswer] = useState<{ scope: string; identity: ChatGatewayIdentity } | null>(null);
  /** The provider session id the host published for THIS pane; null means unknown, never guessed. */
  const [chatProviderSession, setChatProviderSession] = useState<string | null>(null);
  /** Bumped when a history read is refused TARGET_EXPIRED, so the pane's session rows are re-read. */
  const [chatSessionRowsTick, setChatSessionRowsTick] = useState(0);
  const [chatComposerNotice, setChatComposerNotice] = useState<string | null>(null);
  // The fence the newest read was issued under: a response for another one is dropped.
  const chatFenceRef = useRef<ReferenceHistoryFence | null>(null);
  const chatRetentionTruncatedRef = useRef(false);
  // How many history turns already carried an echo's exact words when it was sent, and which ones:
  // only a turn beyond that count, or one not on screen then, is the echo's delivery.
  const chatEchoBaselineRef = useRef<Map<string, { ids: Set<string>; count: number }>>(new Map());

  const [browserSessions, setBrowserSessions] = useState<Array<{ browserId: string; title?: string; url?: string }>>([]);
  const [selectedBrowserId, setSelectedBrowserId] = useState<string | null>(null);
  // First render always speaks to the relay; a verified probe swaps this for a direct endpoint.
  const [transport, setTransport] = useState<CandidateEndpoint>(() => relayEndpoint(relayUrl));
  const remoteHostState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const activeHost = remoteHostState.hosts[hostId] ?? null;
  const [optimisticSessionId, setOptimisticSessionId] = useState<string | null>(null);
  const [terminalRetryGeneration, setTerminalRetryGeneration] = useState(0);
  const pendingSelectionRef = useRef<RemoteContextOption | null>(null);
  const optimisticSocketSessionIdRef = useRef<string | null>(null);
  const optimisticSocketClosedRef = useRef(false);
  const selectionRequestAcceptedRef = useRef(false);
  const selectionEventReceivedRef = useRef(false);
  const confirmationInFlightRef = useRef(false);
  const confirmationRequeuedRef = useRef(false);
  /* A timed-out account selection whose machine answer is still in flight. It may land late
     only while no newer selection, back-out or connection change has replaced it. */
  const lateSelectionRef = useRef<RemoteContextOption | null>(null);
  const workspaceRefreshVersionRef = useRef(0);
  const confirmationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);


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
    /** Ephemeral like sessionId: a published Inbox row opens only what already runs. */
    attachOnly?: boolean;
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
  const chatGateway =
    chatGatewayAnswer !== null && token && chatGatewayAnswer.scope === chatGatewayScope(token, transportBaseUrl)
      ? chatGatewayAnswer.identity
      : null;
  // The transferred tunnel is owned here: close it when it is replaced or on unmount.
  // AccountConnection.close is idempotent, so explicit closes elsewhere are safe.
  useEffect(() => {
    if (!activeTunnelConnection) return;
    return () => activeTunnelConnection.close();
  }, [activeTunnelConnection]);

  // Relay traffic is end-to-end encrypted whenever this client can: account mode rides the
  // active tunnel, and a relay-paired host with attach info gets its own encrypted transport.
  // remoteFetch/remoteOpenSocket route every request for the registered base through it.
  const relayE2EMode = activeTunnelConnection ? "e2e" : relayE2EState(activeHost, transport.url);
  const e2eHost = !activeTunnelConnection && relayE2EMode === "e2e" ? activeHost : null;
  const e2eRelayOrigin = e2eHost?.relayOrigin ?? null;
  const e2eMachineId = e2eHost?.machineId ?? null;
  const e2eEnrollmentEpoch = e2eHost?.enrollmentEpoch ?? null;
  const e2eMachineKey = e2eHost?.attachPublicKey ?? null;
  const [relayE2EBase, setRelayE2EBase] = useState<string | null>(null);
  useEffect(() => {
    if (activeTunnelConnection && accountHostBaseUrl) {
      const unregister = registerRelayE2E(accountHostBaseUrl, {
        fetchLike: (pathAndQuery, init) => activeTunnelConnection.transport.fetchLike(pathAndQuery, init),
        openWebSocket: (pathAndQuery) => activeTunnelConnection.openWebSocket(pathAndQuery),
      });
      setRelayE2EBase(accountHostBaseUrl);
      return () => {
        unregister();
        setRelayE2EBase(null);
      };
    }
    if (!e2eRelayOrigin || !e2eMachineId || !e2eEnrollmentEpoch || !e2eMachineKey || !token) return;
    const connection = openRelayE2ETransport({
      relayOrigin: e2eRelayOrigin,
      machineId: e2eMachineId,
      enrollmentEpoch: e2eEnrollmentEpoch,
      machineAttachPublicKey: e2eMachineKey,
      attachKey: getOrCreateAttachKey(),
      bearer: token,
    });
    const unregister = registerRelayE2E(transportBaseUrl, connection);
    setRelayE2EBase(transportBaseUrl);
    return () => {
      unregister();
      connection.close();
      setRelayE2EBase(null);
    };
  }, [
    activeTunnelConnection,
    accountHostBaseUrl,
    e2eRelayOrigin,
    e2eMachineId,
    e2eEnrollmentEpoch,
    e2eMachineKey,
    token,
    transportBaseUrl,
  ]);
  // A relay that refused plaintext (E2E_REQUIRED) for this base; reconnecting stops until re-pair.
  const [relayRepairErrorBase, setRelayRepairErrorBase] = useState<string | null>(null);
  const [relayRepairDismissedBase, setRelayRepairDismissedBase] = useState<string | null>(null);
  const relayRepairError = relayRepairErrorBase !== null && relayRepairErrorBase === transportBaseUrl;
  const showRelayRepairNotice = relayRepairError
    || (relayE2EMode === REPAIR_REQUIRED && relayRepairDismissedBase !== transportBaseUrl);

  const [planLimitNotice, setPlanLimitNotice] = useState<PlanLimitState | null>(null);
  const planLimitSuspendedRef = useRef(false);
  planLimitSuspendedRef.current = planLimitNotice !== null;

  const clearPendingSelectionRef = useRef<(retryFailedSocket?: boolean) => void>(() => {});

  const handlePlanLimit = useCallback((state: PlanLimitState) => {
    // Billing limits only exist for account relay connections; paired and local
    // sessions keep their existing behavior untouched.
    if (!accountSessionToken) return;
    const merged = planLimitStateWithEntitlement(state, getStoredAccountEntitlementSnapshot(relayUrl));
    storeAccountEntitlementSnapshot(relayUrl, {
      plan: merged.plan,
      status: merged.status,
      graceEndsAt: merged.graceEndsAt,
      stoppedAt: merged.stoppedAt,
    });
    setPlanLimitNotice(merged);
    // Explicitly drop active connections and clear tokens immediately so that the UI
    // transitions synchronously out of any active terminal or workspace view. The terminal
    // pane's socket rides the account tunnel, so closing the tunnel closes it too.
    clearPendingSelectionRef.current();
    setInitialAccountTarget(null);
    if (activeTunnelConnection) {
      activeTunnelConnection.close();
      setActiveTunnelConnection(null);
    }
    setToken(null);
    workspaceRefreshVersionRef.current += 1;
  }, [accountSessionToken, relayUrl, activeTunnelConnection]);

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
    lateSelectionRef.current = null;
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
    clearStoredAccountEntitlementSnapshot(relayUrl);
    setAccountSessionToken(null);
    storeAccountEmail(null);
    setAccountEmail(null);
    setPlanLimitNotice(null);
    disconnect();
  }, [disconnect, relayUrl]);

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
    Boolean(accountSessionToken) && planLimitNotice === null,
    handleLogout,
    handlePlanLimit,
  );
  const {
    applyParsedInventoryEvent,
    noteSessionActivity,
    noteSessionsPayload,
  } = accountDiscovery;
  const accountDiscoveryRef = useRef(accountDiscovery);
  accountDiscoveryRef.current = accountDiscovery;
  // Phone home: the signed-in account email (Account sheet) and the per-machine fan-out of
  // `/api/v1/events` frames the supervisor feed listens to.
  const [accountEmail, setAccountEmail] = useState<string | null>(() => getStoredAccountEmail());
  const [accountSheetOpen, setAccountSheetOpen] = useState(false);
  const [supervisorBus] = useState(createSupervisorFrameBus);

  /* The machine inventory is read from effects and socket callbacks that are declared
     before the derived model below, so it is mirrored into a ref every render. */
  const inventoryRef = useRef<RemoteSessionInventory | null>(null);
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

    // The daemon incarnation a target binds is published by the host's own session list; both
    // transports answer it, so the chat lane learns the epoch on the relay path too.
    if (token) {
      try {
        const rows = await fetchChatSessionRows({
          baseUrl: transportBaseUrl,
          token,
          tunnel: activeTunnelConnection,
        });
        const newEpochs: Record<string, string> = {};
        for (const row of rows) {
          const record = chatRecord(row);
          if (record === null) continue;
          const target = chatRecord(record.target);
          const sid = chatText(record.sessionId) ?? (target === null ? null : chatText(target.sessionId));
          const epoch = chatText(record.daemonEpoch) ?? (target === null ? null : chatText(target.daemonEpoch));
          if (sid !== null && epoch !== null) {
            sessionEpochsRef.current.set(sid, epoch);
            newEpochs[sid] = epoch;
          }
        }
        if (Object.keys(newEpochs).length > 0) {
          setSessionEpochs((prev) => ({ ...prev, ...newEpochs }));
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
  }, [activeTunnelConnection, token, transportBaseUrl]);


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
            // The tab strip is keyed by session id, not by the desktop's own pane id.
            activeTabId: activeSessionId,
            activeTerminal: active,
            daemonEpoch: state.context.daemonEpoch ?? modelRef.current.context.daemonEpoch,
          },
        };
        setModel((prev) => {
          if (prev.context.workspaceId !== chosenWorkspace) return prev;
          if (prev.context.worktreeSlug !== chosenWorktree) return prev;
          if (
            prev.context.activeTerminal?.sessionId === activeSessionId &&
            prev.context.activeTabId === activeSessionId
          ) return prev;
          return {
            ...prev,
            context: {
              ...prev.context,
              activeTabId: activeSessionId,
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
      const response = await remoteFetch(transportBaseUrl, `/api/v1/browser/sessions${query}`, {
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

        let projsData = projRes.status >= 200 && projRes.status < 300
          ? JSON.parse(new TextDecoder().decode(projRes.body))
          : null;
        if (projsData?.desktopLocalSnapshotExists === true) {
          const scoped = await activeTunnelConnection.transport.fetchLike("/api/v1/workspace/projects?scope=desktopLocal", {
            headers: { Authorization: `Bearer ${token}` },
          });
          if (scoped.status < 200 || scoped.status >= 300) return null;
          projsData = JSON.parse(new TextDecoder().decode(scoped.body));
        }
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
        const requestedWs = currentTarget?.workspaceId ?? currentModel.context.workspaceId;
        // A refresh never moves an existing selection to another project: the user's
        // terminal and input stay on the workspace they chose even when it is offline.
        // Only the very first pick falls back to a ready desktop member.
        const currentWs = requestedWs ?? (projsData?.desktopLocalSnapshotExists === true
          ? projectsList.find((project: { availability: string }) => project.availability === "ready")?.workspaceId ?? null
          : projectsList[0]?.workspaceId ?? null);
        const retainedWorkspace = currentWs === requestedWs;
        const currentSlug = retainedWorkspace ? currentTarget?.worktreeSlug ?? currentModel.context.worktreeSlug ?? null : null;
        const currentLabel = retainedWorkspace ? currentTarget?.worktreeLabel ?? currentModel.context.worktreeLabel ?? currentSlug ?? "main" : "main";
        const requestedSessionId = retainedWorkspace ? currentModel.context.activeTerminal?.sessionId ?? null : null;

        // The pending target's exact session outranks any remembered preference: a
        // background refresh must never resolve a chosen session to the worktree's first
        // one while the selection is still being issued.
        const preferredSid = currentWs
          ? getAccountPreferredSessionId(relayUrl, machineId, currentWs, currentSlug)
          : null;
        const pendingSid = retainedWorkspace ? currentTarget?.sessionId ?? null : null;
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
      const response = await remoteFetch(transportBaseUrl, "/api/v1/workspace/state", {
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

  /* An inventory boundary can arrive in bursts (a busy watched tree republishes several
     times a second). Every tunnel request shares one serial queue, so a read per boundary
     would starve a selection's own requests past its confirmation deadline. At most one
     refresh cycle (workspace read, then the desktop-context sync it re-arms) is in flight;
     boundaries landing meanwhile collapse into a single trailing cycle. */
  const refreshWorkspaceRef = useRef(refreshWorkspace);
  refreshWorkspaceRef.current = refreshWorkspace;
  const [inventoryRefresher] = useState(() =>
    createCoalescedRunner(
      () => refreshWorkspaceRef.current(),
      150,
      () => setDesktopSyncTick((tick) => tick + 1),
    ),
  );
  useEffect(() => () => inventoryRefresher.cancel(), [inventoryRefresher]);
  scheduleInventoryRefreshRef.current = inventoryRefresher.trigger;

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
    lateSelectionRef.current = null;
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

  // A limited or suspended account must not keep retrying: drop the account connection (and
  // with it the tunnelled terminal socket), stop the workspace refresh, and let the discovery
  // hook (disabled by planLimitNotice) close every retained tunnel. Recovery is the notice's
  // explicit retry.
  useEffect(() => {
    if (!planLimitNotice) return;
    clearPendingSelection();
    setInitialAccountTarget(null);
    if (activeTunnelConnection) {
      activeTunnelConnection.close();
      setActiveTunnelConnection(null);
    }
    setToken(null);
    workspaceRefreshVersionRef.current += 1;
  }, [activeTunnelConnection, clearPendingSelection, planLimitNotice]);

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
    if (planLimitNotice) return;
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
        supervisorBus.emit(activeTunnelConnection.machine.machineId, text);
        const inventoryEvent = parseInventoryEvent(activeTunnelConnection.machine.machineId, text);
        if (inventoryEvent.kind === "inventory") {
          applyParsedInventoryEvent(activeTunnelConnection.machine.machineId, inventoryEvent);
          scheduleInventoryRefreshRef.current();
        }
        if (inventoryEvent.kind === "ignored" && inventoryEvent.type === "desktopSelectionChanged") {
          setDesktopFollowTick((tick) => tick + 1);
          return;
        }
        if (inventoryEvent.kind !== "ignored") return;
      } else {
        supervisorBus.emit(DIRECT_SIDEBAR_MACHINE, text);
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
          if (disposed) { current.close(); return; }
        } else {
          const opened = await remoteSocketTarget(transportBaseUrl, "/api/v1/events", token, abort.signal);
          if (disposed) {
            if (typeof opened !== "string") opened.close();
            return;
          }
          // Dial a ticketed URL here so the handlers below attach in the same tick.
          current = typeof opened === "string" ? new WebSocket(opened) : opened;
        }
      } catch (error) {
        if (disposed) return;
        if (error instanceof RelayRepairRequiredError) {
          // The relay only carries encrypted traffic and this device cannot encrypt to the host:
          // retrying cannot succeed, so surface the re-pair prompt and stop.
          setRelayRepairErrorBase(transportBaseUrl);
          return;
        }
        const limitState = planLimitStateFromError(error);
        if (limitState) {
          handlePlanLimit(limitState);
          return;
        }
        if (!activeTunnelConnection && transport.url !== relayUrl) rollbackTransport();
        else {
          console.warn("Event socket connection failed", error);
          retry = setTimeout(connect, Math.min(10000, 1000 * 2 ** attempt) * (1 - 0.5 * Math.random()));
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
      current.onclose = (event?: unknown) => {
        if (disposed || socket !== current) return;
        socket = null;
        const limitState = planLimitStateFromCloseEvent(event);
        if (limitState) {
          handlePlanLimit(limitState);
          return;
        }
        if (!activeTunnelConnection && transport.url !== relayUrl) {
          rollbackTransport();
          return;
        }
        retry = setTimeout(connect, Math.min(10000, 1000 * 2 ** attempt) * (1 - 0.5 * Math.random()));
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
  }, [activeTunnelConnection, clearPendingSelection, confirmSelection, refreshWorkspace, token, transport.url, transportBaseUrl, relayUrl, rollbackTransport, handlePlanLimit, planLimitNotice]);

  clearPendingSelectionRef.current = clearPendingSelection;

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
      // The picker is released, but the attempt's own bounded request may still answer.
      lateSelectionRef.current = option;
    }, CONFIRMATION_TIMEOUT_MS);
  }, [clearPendingSelection]);

  const selectContext = useCallback(async (requestedOption: RemoteContextOption, createTerminal = false): Promise<boolean> => {
    // A user selection naming a session (or an explicit tab) is an independent choice:
    // the read-only desktop-context sync must never replace it afterwards.
    independentSessionRef.current = Boolean(requestedOption.sessionId || requestedOption.tabId);
    if (!token || pendingSelectionRef.current) return false;
    // A picker can reuse an option object; each attempt needs its own identity.
    const option = { ...requestedOption };
    lateSelectionRef.current = null;
    const attemptIsCurrent = () => pendingSelectionRef.current === option || lateSelectionRef.current === option;
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
        if (!attemptIsCurrent()) return false;

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

        if (!targetSession && option.attachOnly) {
          /* A desktop Inbox row names a pane the desktop already runs. When the machine no longer
             lists one here, open the worktree with no terminal instead of starting a shell. */
          const contextLabel = option.worktreeLabel ?? option.worktreeSlug ?? "main";
          const workspaceOnly = {
            workspaceId: option.workspaceId,
            worktreeSlug: option.worktreeSlug ?? null,
            worktreeLabel: contextLabel,
            activeTerminal: null,
            activeTabId: null,
            terminalTabs: machineTabsFromInventory(inventory?.entries ?? [], option.workspaceId, option.worktreeSlug ?? null, contextLabel),
          };
          workspaceRefreshVersionRef.current += 1;
          modelRef.current = { ...modelRef.current, context: workspaceOnly };
          setModel((prev) => ({ ...prev, context: workspaceOnly }));
          setInitialAccountTarget(null);
          setCreationError(null);
          clearPendingSelection(true);
          return true;
        }

        if (!targetSession) {
          /* The machine API cannot start a terminal on an SSH-host project (the desktop
             answers INVALID_PATH); say so instead of surfacing a bare status code. */
          if (option.workspaceId.startsWith("ssh:")) {
            throw new Error("No running terminal on this SSH project. Open one from the desktop first.");
          }
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
          if (!attemptIsCurrent()) return false;

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
          // A late answer that lands supersedes its own timeout banner.
          setCreationError(null);
          clearPendingSelection(true);
          return true;
        }
      } else {
        const response = await remoteFetch(
          transportBaseUrl,
          "/api/v1/workspace/select",
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
      if (!attemptIsCurrent()) return false;
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
      ...(target.attachOnly ? { attachOnly: true } : {}),
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
        // A newer desktop focus supersedes any timed-out attempt still waiting on its answer.
        lateSelectionRef.current = null;
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
  // Swipe follows the published tab order (terminalTabs), so a swipe can reach a tab whose
  // published worktree differs from the current one.
  const swipePanes = useMemo(() => model.context.terminalTabs ?? [], [model.context.terminalTabs]);
  const activeSwipeIndex = model.context.activeTabId
    ? swipePanes.findIndex((tab) => tab.id === model.context.activeTabId)
    : 0;
  const currentIndex = activeSwipeIndex >= 0 ? activeSwipeIndex : 0;

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
    if (swipePanes.length <= 1 || currentIndex >= swipePanes.length - 1 || !model.context.workspaceId) return;
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





  /* -------------------------------------------------------------------------
     The reference chat lane: one target, one history, one prompt, one draft.

     Everything below speaks to the ORIGINAL session through the frozen reference-chat routes.
     The chat never opens a raw terminal socket: the explicit terminal mode owns that, and the
     chat would otherwise be a second writer into the same pane with no ordering between them.
     ------------------------------------------------------------------------- */
  /* Declared above the auth early return so the hook count is identical on the
     login screen and after pairing; a hook below the guard changes the order. */
  const isAccountMode = Boolean(accountSessionToken);
  // The supervisor-first phone home exists only for a signed-in account on a narrow layout.
  const phoneHome = !desktopSidebar && isAccountMode && planLimitNotice === null;
  const phoneNav = usePhoneNavigation(phoneHome);
  const phoneSupervisor = usePhoneSupervisor({
    // The phone home and the wide sidebar's Supervisor tab both read it.
    enabled: phoneHome || (desktopSidebar && isAccountMode && planLimitNotice === null),
    discovery: accountDiscovery,
    active: activeTunnelConnection,
    bus: supervisorBus,
  });
  const phoneBack = phoneNav.back;
  // A selection backed out of on the Session screen ("Back to Worktrees") leaves no session to
  // show, so the screen returns to the home it came from.
  const phoneSessionLost = phoneHome && phoneNav.screen.kind === "session" && !token && !initialAccountTarget;
  useEffect(() => {
    if (phoneSessionLost) phoneBack();
  }, [phoneBack, phoneSessionLost]);
  const activeMachineInventory = activeTunnelConnection
    ? accountDiscovery.sessionInventories[activeTunnelConnection.machine.machineId] ?? null
    : null;
  inventoryRef.current = activeMachineInventory;

  /* The sidebar draws each machine's desktop-published model with the desktop's own view. */
  const directSidebarFetch = useMemo(
    () =>
      !isAccountMode && token
        ? async (path: string) => {
            const response = await fetch(apiUrl(transportBaseUrl, path), { headers: { Authorization: `Bearer ${token}` } });
            return { status: response.status, body: await response.text() };
          }
        : null,
    [isAccountMode, token, transportBaseUrl],
  );
  const sidebarSelection = model.context.workspaceId
    ? { workspaceId: model.context.workspaceId, worktreeSlug: model.context.worktreeSlug }
    : null;
  const directInventory = useMemo(() => (isAccountMode ? null : directSidebarInventory(model)), [isAccountMode, model]);
  const sidebarMachines = useRemoteSidebarMachines(
    isAccountMode
      ? {
          kind: "account",
          machines: accountDiscovery.machines,
          linkFor: (machineId) =>
            activeTunnelConnection && !activeTunnelConnection.isClosed && activeTunnelConnection.machine.machineId === machineId
              ? { transport: activeTunnelConnection.transport, deviceToken: activeTunnelConnection.deviceToken }
              : accountDiscovery.peekConnection(machineId),
          inventories: accountDiscovery.sessionInventories,
          statuses: accountDiscovery.machineStatuses,
          activeMachineId: activeTunnelConnection?.machine.machineId ?? null,
          bus: supervisorBus,
          selection: sidebarSelection,
        }
      : {
          kind: "direct",
          fetchLike: directSidebarFetch,
          connectionId: `${transportBaseUrl}\u0000${token ?? ""}`,
          inventory: directInventory,
          bus: supervisorBus,
          selection: sidebarSelection,
        },
  );
  const sidebarSupervisors = useMemo(() => Object.fromEntries(phoneSupervisor.machines.flatMap((machine) =>
    machine.snapshot ? [[machine.machineId, { snapshot: machine.snapshot, now: phoneSupervisor.now }]] : [])),
  [phoneSupervisor.machines, phoneSupervisor.now]);
  const sidebarSections = useRemoteSidebarModel({ machines: sidebarMachines, supervisors: sidebarSupervisors });
  const sidebarSupervisor = isAccountMode && desktopSidebar
    ? {
        typing: remoteSupervisorTyping(sidebarSupervisors),
        content: (
          <div data-testid="remote-sidebar-supervisor" className="flex min-h-0 flex-1 flex-col overflow-y-auto">
            <SupervisorHome
              machines={phoneSupervisor.machines}
              now={phoneSupervisor.now}
              needsYou={new Set<string>()}
              onSetPaused={phoneSupervisor.setPaused}
              onOpenLane={(machineId, sessionId) => {
                const option = optionForSession(accountDiscovery.accountOptions, machineId, sessionId);
                if (option) void selectAccountOption(option);
              }}
            />
          </div>
        ),
      }
    : undefined;

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

  const activeObservedTab = effectiveModel.context.terminalTabs?.find((tab) => tab.id === model.context.activeTabId);
  const chatAgentType = activeObservedTab?.agentType ?? null;
  const chatActivity = activeObservedTab?.activityState ?? null;
  // The gateway's own capabilities answer is the exact value the route compares a target
  // against, so it wins; the session row is the fallback for a host that publishes it there.
  const chatDaemonEpoch =
    chatGateway?.daemonEpoch ??
    (effectiveSessionId
      ? sessionEpochs[effectiveSessionId] ?? sessionEpochsRef.current.get(effectiveSessionId) ?? null
      : null);
  const chatAgentIsNative = referenceNativeKindIsNative(
    referenceNativeKindFromRegistryId(chatAgentType ?? ""),
  );
  // The pane's own liveness signal, as the workspace mirror publishes it. Nothing here invents
  // a turn: a running pane simply offers Stop instead of Send.
  const chatIsRunning = chatActivity === "working" || chatPrompt !== null;

  const chatReferenceTarget = useMemo<ReferenceTargetRef | null>(() => {
    // The owner is the authority the gateway PUBLISHED, on the same instance lifetime as the
    // epoch. The workspace this UI happens to be showing is not a substitute: with no published
    // owner there is no target, and the lane reports unavailable instead of asserting one this
    // client invented. A changed owner is a different target key, so the lane resets and a
    // target that outlived its authority earns the same TARGET_EXPIRED a stale epoch earns.
    const referenceOwnerId = chatGateway?.referenceOwnerId ?? null;
    if (!effectiveSessionId || !referenceOwnerId || !chatDaemonEpoch) return null;
    return {
      target: {
        // The id the host publishes for itself, else its documented default. Never the machine
        // identity, which is a different value, and never another host's name.
        hostId: chatGateway?.referenceHostId ?? REFERENCE_CHAT_DEFAULT_HOST_ID,
        ownerId: referenceOwnerId,
        epoch: chatDaemonEpoch,
        backendSessionId: effectiveSessionId,
      },
      // Only the provider session the owning host published for this pane. Absent means unknown,
      // and the reader then binds the conversation by the host's own owner/source rules.
      ...(chatProviderSession !== null ? { providerSessionId: chatProviderSession } : {}),
    };
  }, [chatDaemonEpoch, chatGateway, chatProviderSession, effectiveSessionId]);
  const chatTargetKey = chatReferenceTarget === null ? null : referenceTargetKey(chatReferenceTarget);

  // Read through refs so a poll closure never runs against a pane it was not started for.
  const chatTargetRef = useRef<ReferenceTargetRef | null>(null);
  chatTargetRef.current = chatReferenceTarget;
  const chatSessionIdRef = useRef<string | null>(null);
  chatSessionIdRef.current = effectiveSessionId;
  const chatBaseUrlRef = useRef(transportBaseUrl);
  chatBaseUrlRef.current = transportBaseUrl;
  const chatTokenRef = useRef(token);
  chatTokenRef.current = token;
  const chatAgentTypeRef = useRef<string | null>(null);
  chatAgentTypeRef.current = chatAgentType;
  const chatStagedFilesRef = useRef<readonly ReferenceFileReceipt[]>([]);
  chatStagedFilesRef.current = chatStagedFiles;
  // A dispatched mention may already be in the PTY before its HTTP acknowledgement arrives.
  // Keep its bytes out of every cleanup path until all sends definitely refused it.
  const chatSubmittedFilesRef = useRef(new Map<string, {
    file: ReferenceFileReceipt;
    pending: number;
    retained: boolean;
  }>());
  /** The target the lane was last reset to: the owner of whatever the lane still holds. */
  const chatLaneTargetRef = useRef<ReferenceTargetRef | null>(null);
  /**
   * The base URL and bearer that reach the lane target's host. Refreshed only while the live
   * target IS the lane target (a live target always pairs with the current base and token), so
   * once another host's target replaces it, this still names the host that holds its files.
   */
  const chatLaneRouteRef = useRef<ChatLaneRoute>({ baseUrl: transportBaseUrl, token });
  if (
    chatReferenceTarget !== null &&
    chatLaneTargetRef.current !== null &&
    sameReferenceTarget(chatReferenceTarget, chatLaneTargetRef.current)
  ) {
    chatLaneRouteRef.current = { baseUrl: transportBaseUrl, token };
  }
  /** Aborted when the lane resets, so nothing started for one pane lands in another. */
  const chatLaneAbortRef = useRef(new AbortController());
  /**
   * Is this lane still the one `target` was started for? Asked after every await. The lane owner
   * decides: a target that is only briefly null (a new token or transport not yet re-answered)
   * resets nothing, so a request settling in that gap still belongs here. Only a DIFFERENT
   * target - live already, or bound by the lane - makes it someone else's. Starting a NEW request
   * still needs a live target, and checks `chatTargetRef` for that itself.
   */
  const chatTargetIsCurrent = useCallback((target: ReferenceTargetRef): boolean => {
    const owner = chatLaneTargetRef.current;
    const live = chatTargetRef.current;
    return (
      owner !== null &&
      sameReferenceTarget(target, owner) &&
      (live === null || sameReferenceTarget(target, live))
    );
  }, []);




  // The gateway's own identity: the daemon incarnation every reference-chat request must name,
  // and the host id it publishes. Both come from the one authenticated route that answers them on
  // every transport, so the chat lane is not tied to a single connection mode.
  /** Re-read the owning host's identity: a restarted daemon answers a new incarnation. */
  const refreshChatIdentity = useCallback(async () => {
    const bearer = chatTokenRef.current;
    if (!bearer) return;
    const baseUrl = chatBaseUrlRef.current;
    const identity = await readChatGatewayIdentity({
      baseUrl,
      token: bearer,
    });
    if (identity !== null) setChatGatewayAnswer({ scope: chatGatewayScope(bearer, baseUrl), identity });
  }, []);

  const terminalCreateWebSocket = useMemo(() => {
    if (!activeTunnelConnection) {
      // A relay-paired host reached through its encrypted transport: the gateway terminal route
      // inside the tunnel, bearer in the tunnel instead of a socket ticket.
      if (!token || relayE2EBase === null || relayE2EBase !== transportBaseUrl) return undefined;
      const base = relayE2EBase;
      // The base is registered, so remoteOpenSocket answers with a tunnel socket.
      return (path: string) => remoteOpenSocket(base, path, token) as Promise<TunnelWebSocket>;
    }
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
  }, [activeTunnelConnection, effectiveSessionId, getSessionDaemonEpoch, relayE2EBase, token, transportBaseUrl]);

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
    // The identity belongs to the host that answered it: a new token or transport has none yet.
    setChatGatewayAnswer(null);
    if (!token) return;
    const scope = chatGatewayScope(token, transportBaseUrl);
    const controller = new AbortController();
    let retry: ReturnType<typeof setInterval> | null = null;
    let warned = false;
    // A transient capabilities failure answers null: it keeps the identity this lane already had,
    // says so, and asks again on the history poll's cadence until the host answers.
    const readIdentity = async () => {
      const identity = await readChatGatewayIdentity({ baseUrl: transportBaseUrl, token, signal: controller.signal });
      if (controller.signal.aborted) return;
      if (identity !== null) {
        setChatGatewayAnswer({ scope, identity });
        if (retry !== null) clearInterval(retry);
        retry = null;
        if (warned) setChatWarnings((prev) => prev.filter((warning) => warning !== CHAT_UNAVAILABLE_WARNING));
        warned = false;
        return;
      }
      warned = true;
      setChatWarnings((prev) => withChatWarning(prev, CHAT_UNAVAILABLE_WARNING));
      if (retry === null) {
        retry = setInterval(() => {
          void readIdentity();
        }, 3000);
      }
    };
    void readIdentity();
    return () => {
      controller.abort();
      if (retry !== null) clearInterval(retry);
    };
  }, [token, transportBaseUrl]);

  // The pane's own facts, from the host's session list: its daemon incarnation and the provider
  // session the host published for it. Absence is unknown; a disagreement between rows is
  // ambiguous, and both stay unknown rather than being guessed.
  useEffect(() => {
    setChatProviderSession(null);
  }, [activeTunnelConnection, effectiveSessionId, token, transportBaseUrl]);
  // A refresh tick re-reads the same pane's rows without first forgetting its provider session, so
  // a rotated session (e.g. after /new) replaces the old one in place.
  useEffect(() => {
    if (!effectiveSessionId || !token) {
      return;
    }
    let cancelled = false;
    const controller = new AbortController();
    void fetchChatSessionRows({
      baseUrl: transportBaseUrl,
      token,
      tunnel: activeTunnelConnection,
      signal: controller.signal,
    })
      .then((rows) => {
        if (cancelled) return;
        const epoch = chatDaemonEpochFromRows(rows, effectiveSessionId);
        if (epoch !== null) {
          sessionEpochsRef.current.set(effectiveSessionId, epoch);
          setSessionEpochs((prev) =>
            prev[effectiveSessionId] === epoch ? prev : { ...prev, [effectiveSessionId]: epoch },
          );
        }
        setChatProviderSession(chatProviderSessionId(rows, effectiveSessionId));
      })
      .catch(() => {
        /* the terminal path resolves the epoch for itself; this lane simply stays idle */
      });
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [activeTunnelConnection, chatSessionRowsTick, effectiveSessionId, token, transportBaseUrl]);

  /**
   * Release files staged for a target and never sent, on the host `route` reaches. The route is
   * the owning pane's own, so this never depends on which pane is current by the time it runs.
   * Resolves to the ids the host confirmed it let go of.
   */
  const releaseStagedChatFiles = useCallback(
    async (
      target: ReferenceTargetRef,
      files: readonly ReferenceFileReceipt[],
      route: ChatLaneRoute,
    ): Promise<ReadonlySet<string>> => {
      const bearer = route.token;
      if (!bearer || files.length === 0) return new Set();
      const query = referenceChatReadQuery({ target: target.target }, chatAgentTypeRef.current ?? "", undefined, null);
      const answers = await Promise.all(
        files.map(async (file) => {
          const id = file.receipt.attachmentId;
          // Both departure and a late definite submit refusal reach this path. Read the owning
          // target's live queue, not the displayed rows (which a pane switch already cleared).
          if (heldChatFileIsReferenced(target, file)) return null;
          try {
            const response = await remoteFetch(
              route.baseUrl,
              `${referenceChatRoute(target.target.backendSessionId, `files/${encodeURIComponent(id)}`)}?${query}`,
              { method: "DELETE", headers: { Authorization: `Bearer ${bearer}` } },
            );
            return response.ok ? id : null;
          } catch {
            /* the host prunes what it was not told about on its own schedule */
            return null;
          }
        }),
      );
      return new Set(answers.filter((id): id is string => id !== null));
    },
    [],
  );

  /** The transcript side of the lane: what the pane's history and prompt said. */
  const clearChatTranscript = useCallback(() => {
    setChatMessages([]);
    setChatOlderMessages([]);
    setChatOlderLoading(false);
    setChatOlderFailed(false);
    setChatLoadedOlder(false);
    setChatPage(null);
    setChatOlderCursor(null);
    setChatHasOlder(false);
    chatLoadedOlderRef.current = false;
    chatNewestStartRef.current = null;
    chatNewestTurnsRef.current = [];
    chatOlderGenerationRef.current += 1;
    chatPromptGenerationRef.current += 1;
    setChatPrompt(null);
    chatPromptIdRef.current = null;
    chatPromptRevisionRef.current = null;
    setChatScreenRevision(null);
    setChatTypedAnswer(null);
    setChatPromptError(null);
    chatFenceRef.current = null;
    chatRetentionTruncatedRef.current = false;
  }, []);

  /** Everything that belongs to ONE pane: another target starts from a clean lane. */
  const resetChatLane = useCallback(() => {
    chatLaneAbortRef.current.abort();
    chatLaneAbortRef.current = new AbortController();
    const previous = chatLaneTargetRef.current;
    if (previous !== null) {
      const released = chatStagedFilesRef.current.filter(
        (file) => !chatSubmittedFilesRef.current.has(submittedFileKey(previous, file.receipt.attachmentId)),
      );
      // A released file no longer exists on the host: its mention leaves the pane's saved draft
      // with it, as removing the chip does, so returning to the pane never sends a dead mention.
      // A file the host did not confirm releasing is still staged there, and keeps its mention.
      void releaseStagedChatFiles(previous, released, chatLaneRouteRef.current).then((confirmed) => {
        const draft = referenceDrafts.read(previous).text;
        let stripped = draft;
        for (const file of released) {
          if (!confirmed.has(file.receipt.attachmentId)) continue;
          const path = referenceMentionPathOf(file.mentionText);
          if (path !== null) stripped = stripped.split(referenceMentionFor(path)).join("");
        }
        if (stripped === draft) return;
        referenceDrafts.set(previous, stripped);
        if (chatTargetIsCurrent(previous)) setChatDraftText(stripped);
      });
    }
    chatStagedFilesRef.current = [];
    clearChatTranscript();
    setChatWarnings([]);
    setChatComposerNotice(null);
    setChatDraftText("");
    setChatStagedFiles([]);
    setChatAttaching(false);
    setChatAttachError(null);
    setChatHeld([]);
    // A held row's send stays in flight across a pane switch, and its id is unique to the row: the
    // set is kept, so returning to the pane before it settles cannot send the same row twice.
  }, [chatTargetIsCurrent, clearChatTranscript, releaseStagedChatFiles]);

  /* The draft and the held rows belong to the TARGET, not to this component instance: switching
     panes reads that target's own rows back, and never carries them across. */
  useEffect(() => {
    // A null target is a gap, not a departure: a new token or transport has no gateway answer
    // until the host re-answers, and the same pane usually comes right back. Leaving the lane
    // target in place means the files staged for it stay staged and its draft keeps their
    // mentions; only a DIFFERENT target replacing it releases them.
    if (chatReferenceTarget === null) return;
    const previous = chatLaneTargetRef.current;
    if (previous !== null && sameReferenceTarget(previous, chatReferenceTarget)) return;
    resetChatLane();
    chatLaneTargetRef.current = chatReferenceTarget;
    chatLaneRouteRef.current = { baseUrl: chatBaseUrlRef.current, token: chatTokenRef.current };
    referenceDrafts.refresh(chatReferenceTarget);
    referenceQueues.refresh(chatReferenceTarget);
    setChatDraftText(referenceDrafts.read(chatReferenceTarget).text);
    setChatHeld([...referenceQueues.read(chatReferenceTarget)]);
    // Files the owning host still holds staged for this pane - an upload whose response was lost
    // to an abort, or a reload - come back into the lane, so they can be seen and released
    // instead of holding the per-turn slots until the host's TTL.
    const rehydrateTarget = chatReferenceTarget;
    const bearer = chatTokenRef.current;
    if (!bearer) return;
    const signal = chatLaneAbortRef.current.signal;
    const query = referenceChatReadQuery(
      { target: rehydrateTarget.target },
      chatAgentTypeRef.current ?? "",
      undefined,
      null,
    );
    void remoteFetch(
      chatBaseUrlRef.current,
      `${referenceChatRoute(rehydrateTarget.target.backendSessionId, "files")}?${query}`,
      { headers: { Authorization: `Bearer ${bearer}` }, signal },
    )
      .then(async (response) => (response.ok ? response.json() : null))
      .then((raw: unknown) => {
        if (signal.aborted || !chatTargetIsCurrent(rehydrateTarget)) return;
        const listed = parseReferenceStagedFileList(raw, rehydrateTarget);
        if (listed.length === 0) return;
        const known = new Set(chatStagedFilesRef.current.map((file) => file.receipt.attachmentId));
        const fresh = listed.filter((file) =>
          !known.has(file.receipt.attachmentId) &&
          !chatSubmittedFilesRef.current.has(submittedFileKey(rehydrateTarget, file.receipt.attachmentId)),
        );
        if (fresh.length === 0) return;
        chatStagedFilesRef.current = [...chatStagedFilesRef.current, ...fresh];
        setChatStagedFiles((prev) => [
          ...prev,
          ...fresh.filter((file) => !prev.some((entry) => entry.receipt.attachmentId === file.receipt.attachmentId)),
        ]);
      })
      .catch(() => {
        /* the host prunes what the lane could not list on its own schedule */
      });
  }, [chatTargetKey, chatTargetIsCurrent, resetChatLane]);

  /* The newest page. A late answer for another generation is dropped, and an identity refusal
     clears the lane rather than browsing another transcript. The provider session the owning
     host published for this pane is part of the binding tuple, so learning it re-reads the page
     under the identity the route compares against instead of waiting for the next poll. */
  useEffect(() => {
    if (viewMode !== "chat") return;
    const target = chatReferenceTarget;
    const sessionId = effectiveSessionId;
    if (!target || !sessionId || !token) return;
    // The history route requires the pane's registry id; a pane with no agent has none, so the
    // read would only ever be refused. Say so once instead of polling into a refresh failure.
    if (!chatAgentType) {
      clearChatTranscript();
      setChatWarnings((prev) =>
        withChatWarning(
          prev.filter((warning) => warning !== CHAT_REFRESH_WARNING && warning !== CHAT_UNAVAILABLE_WARNING),
          CHAT_NO_READER_WARNING,
        ),
      );
      return;
    }
    const registryId = chatAgentType;
    let cancelled = false;
    let fetching = false;
    const controller = new AbortController();

    const readNewest = async () => {
      if (cancelled || fetching) return;
      if (typeof document !== "undefined" && document.hidden) return;
      fetching = true;
      try {
        const read = await readChatHistory({
          baseUrl: transportBaseUrl,
          sessionId,
          token,
          target,
          registryId,
          limit: CHAT_HISTORY_PAGE_LIMIT,
          signal: controller.signal,
          transport: activeTunnelConnection?.transport,
        });
        if (cancelled) return;
        // The pane is the fence: an answer for a target that is no longer current is dropped,
        // and the generation it came from is recorded with it for the cursor check below.
        const liveTarget = chatTargetRef.current;
        if (liveTarget === null || !sameReferenceTarget(liveTarget, read.fence.target)) return;
        chatFenceRef.current = read.fence;
        // An unchanged page keeps its object, so the disclosure derived from it is not recomputed.
        setChatPage((prev) =>
          prev !== null &&
          prev.generation === read.page.generation &&
          prev.source === read.page.source &&
          prev.availability === read.page.availability &&
          prev.hasMore === read.page.hasMore &&
          (prev.unavailableReason ?? null) === (read.page.unavailableReason ?? null) &&
          prev.turns.length === read.page.turns.length &&
          (prev.cursor?.streamId ?? null) === (read.page.cursor?.streamId ?? null) &&
          (prev.cursor?.offset ?? null) === (read.page.cursor?.offset ?? null)
            ? prev
            : read.page,
        );
        const start = read.page.cursor ?? null;
        const anchor = chatNewestStartRef.current;
        const mapped = mapReferenceTurns(read.page);
        const previousTurns = chatNewestTurnsRef.current;
        chatNewestTurnsRef.current = mapped;
        // The window slid forward on the same stream: the turns it slid past join the end of the
        // older block, so the older pages stay and nothing goes missing between them. That holds
        // only while the new window still starts inside the old one; past that the gap is unknown.
        let slidOut: MobileChatMessageProps[] | null = null;
        if (
          chatLoadedOlderRef.current &&
          start !== null &&
          anchor !== null &&
          start.streamId === anchor.streamId &&
          start.offset > anchor.offset &&
          mapped.length > 0
        ) {
          const joinAt = chatSlideJoin(previousTurns, mapped);
          if (joinAt >= 0) slidOut = previousTurns.slice(0, joinAt);
        }
        if (slidOut !== null) {
          chatNewestStartRef.current = start;
          const absorbed = slidOut;
          if (absorbed.length > 0) setChatOlderMessages((prev) => [...prev, ...absorbed]);
        }
        const moved =
          slidOut === null &&
          (start === null || anchor === null || start.streamId !== anchor.streamId || start.offset !== anchor.offset);
        if (moved) {
          // an older read still out started from the old anchor: its page no longer joins this one
          chatOlderGenerationRef.current += 1;
          setChatOlderLoading(false);
        }
        if (chatLoadedOlderRef.current && moved) {
          chatLoadedOlderRef.current = false;
          setChatLoadedOlder(false);
          setChatOlderMessages([]);
          setChatOlderFailed(false);
        }
        if (!chatLoadedOlderRef.current) {
          chatNewestStartRef.current = start;
          setChatOlderCursor(read.page.cursor ?? null);
          setChatHasOlder(read.page.hasMore && read.page.cursor != null);
        }
        setChatMessages((prev) => {
          const pending = prev.filter((message) => message.id.startsWith("pending-"));
          // An echo retires only against a turn that is new since it was sent: an earlier turn with
          // the same words is not its delivery. A turn is new when its id was not on screen at send
          // time (the window may have slid the earlier one out) or when the count grew past the
          // baseline. Each new matching turn retires at most one echo.
          const spent = new Map<string, number>();
          const kept = pending.filter((echo) => {
            const key = `${echo.role}\u0000${echo.content}`;
            const matching = mapped.filter((turn) => turn.role === echo.role && turn.content === echo.content);
            const baseline = chatEchoBaselineRef.current.get(echo.id) ?? { ids: new Set<string>(), count: 0 };
            const fresh = Math.max(
              matching.filter((turn) => !baseline.ids.has(turn.id)).length,
              matching.length - baseline.count,
            );
            const used = spent.get(key) ?? 0;
            if (fresh > used) {
              spent.set(key, used + 1);
              chatEchoBaselineRef.current.delete(echo.id);
              return false;
            }
            return true;
          });
          // A turn that reads the same as the one on screen keeps its object, so it is not redrawn.
          const previousById = new Map<string, MobileChatMessageProps>();
          for (const message of prev) previousById.set(message.id, message);
          const reused = mapped.map((turn) => {
            const previous = previousById.get(turn.id);
            return previous !== undefined && chatTurnSignature(previous) === chatTurnSignature(turn)
              ? previous
              : turn;
          });
          const { messages: capped, truncated } = capRetainedMessages([...reused, ...kept]);
          if (truncated) chatRetentionTruncatedRef.current = true;
          return capped.length === prev.length && capped.every((message, index) => message === prev[index])
            ? prev
            : capped;
        });
        setChatWarnings((prev) => {
          // The page's own disclosure is drawn by the page banner from `chatPage`, which follows
          // every read; repeating it here would draw it twice and outlive the page that said it.
          const cleared = prev.filter(
            (warning) =>
              warning !== CHAT_REFRESH_WARNING &&
              warning !== CHAT_UNAVAILABLE_WARNING &&
              warning !== CHAT_NO_READER_WARNING &&
              warning !== CHAT_RETENTION_WARNING,
          );
          const next = chatRetentionTruncatedRef.current
            ? withChatWarning(cleared, CHAT_RETENTION_WARNING)
            : cleared;
          return next.length === prev.length && next.every((warning, index) => warning === prev[index]) ? prev : next;
        });
      } catch (error) {
        if (cancelled) return;
        if (error instanceof ReferenceHistoryFetchError && referenceHistoryErrorIsIdentity(error.code)) {
          // A refusal is an answer, not permission to look somewhere else. A stale incarnation or
          // a superseded owner authority is the one refusal that can be repaired: re-read the
          // host's identity and let the next poll speak to the incarnation that is live now.
          // Everything else clears the lane.
          if (error.code === "TARGET_EXPIRED" || error.code === "FORBIDDEN") {
            await refreshChatIdentity();
            // the user may have moved to another pane while the identity was being re-read
            if (cancelled) return;
            // The pane's provider session may have rotated (e.g. /new): re-read its session rows
            // once per refusal; the poll cadence bounds how often that can happen.
            if (error.code === "TARGET_EXPIRED") setChatSessionRowsTick((tick) => tick + 1);
          }
          // Only what the pane's transcript said is withdrawn: the draft, held rows and staged
          // files are the user's own and stay. A changed target resets those through its key.
          clearChatTranscript();
          setChatWarnings([CHAT_UNAVAILABLE_WARNING]);
          return;
        }
        setChatWarnings((prev) => withChatWarning(prev, CHAT_REFRESH_WARNING));
      } finally {
        fetching = false;
      }
    };

    void readNewest();
    const timer = setInterval(() => {
      void readNewest();
    }, 3000);
    const onVisible = () => {
      if (typeof document !== "undefined" && document.visibilityState === "visible") void readNewest();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      controller.abort();
      clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [chatAgentType, chatProviderSession, chatTargetKey, clearChatTranscript, effectiveSessionId, refreshChatIdentity, token, transportBaseUrl, viewMode]);

  /* The prompt the original pane waits on. A pane whose reader the reference does not have keeps
     no card: an unknown menu is answered in the terminal, never guessed from here. */
  useEffect(() => {
    if (viewMode !== "chat") return;
    const target = chatReferenceTarget;
    const sessionId = effectiveSessionId;
    if (!target || !sessionId || !token) return;
    if (!chatAgentType || !chatAgentIsNative) {
      chatPromptGenerationRef.current += 1;
      chatPromptIdRef.current = null;
      chatPromptRevisionRef.current = null;
      setChatPrompt(null);
      setChatScreenRevision(null);
      return;
    }
    let cancelled = false;
    let fetching = false;
    const controller = new AbortController();

    const readPrompt = async () => {
      if (cancelled || fetching) return;
      if (typeof document !== "undefined" && document.hidden) return;
      fetching = true;
      try {
        const path = `${referenceChatRoute(sessionId, "prompt")}?${referenceChatReadQuery(target, chatAgentType)}`;
        const response = await remoteFetch(transportBaseUrl, path, {
          headers: { Authorization: `Bearer ${token}` },
          signal: controller.signal,
        });
        if (cancelled) return;
        if (!response.ok) {
          const code = await readChatErrorCode(response);
          if (code === "REQUEST_CONFLICT" || code === "UNSUPPORTED") {
            // The screen could not be reconstructed, so nothing may be answered from it: the
            // message box is held instead of being typed into a menu nobody read.
            setChatWarnings((prev) => withChatWarning(prev, CHAT_HELD_WARNING));
          }
          return;
        }
        const body: unknown = await response.json();
        const prompt = parseChatPrompt(body);
        const promptId = prompt?.id ?? null;
        const revision = prompt === null ? null : parseChatScreenRevision(body);
        // The same occurrence on the same screen is the card already drawn; it is not replaced.
        const promptUnchanged =
          promptId === chatPromptIdRef.current &&
          revision === chatPromptRevisionRef.current &&
          (prompt === null || revision !== null);
        // Each occurrence owns its typed answer and error, even if the provider reuses an id.
        if (promptId !== chatPromptIdRef.current || revision !== chatPromptRevisionRef.current) {
          chatPromptGenerationRef.current += 1;
          setChatTypedAnswer(null);
          setChatPromptError(null);
        }
        chatPromptIdRef.current = promptId;
        chatPromptRevisionRef.current = revision;
        if (!promptUnchanged) setChatPrompt(prompt);
        setChatScreenRevision(revision);
        setChatWarnings((prev) => {
          const next = prev.filter((warning) => warning !== CHAT_HELD_WARNING);
          return next.length === prev.length ? prev : next;
        });
      } catch {
        /* a transient prompt read keeps the card the user is looking at */
      } finally {
        fetching = false;
      }
    };

    void readPrompt();
    const timer = setInterval(() => {
      void readPrompt();
    }, 3000);
    return () => {
      cancelled = true;
      controller.abort();
      clearInterval(timer);
    };
  }, [chatAgentIsNative, chatAgentType, chatPromptRefresh, chatTargetKey, effectiveSessionId, token, transportBaseUrl, viewMode]);

  /** One older page, on an explicit request only: nothing fetches backwards on its own. */
  const loadChatOlderPage = useCallback(async () => {
    const target = chatTargetRef.current;
    const sessionId = chatSessionIdRef.current;
    const bearer = chatTokenRef.current;
    const cursor = chatOlderCursor;
    if (!target || !sessionId || !bearer || cursor === null) return;
    const signal = chatLaneAbortRef.current.signal;
    const generation = chatOlderGenerationRef.current;
    const superseded = () => generation !== chatOlderGenerationRef.current;
    setChatOlderLoading(true);
    setChatOlderFailed(false);
    try {
      const read = await readChatHistory({
        baseUrl: chatBaseUrlRef.current,
        sessionId,
        token: bearer,
        target,
        registryId: chatAgentTypeRef.current ?? "",
        limit: CHAT_HISTORY_PAGE_LIMIT,
        cursor,
        signal,
      });
      // A page for a pane that is no longer current belongs to nobody on screen.
      if (signal.aborted || !chatTargetIsCurrent(target)) return;
      // The newest page re-anchored while this read was out: its turns end before the old anchor
      // and would join the new page with a silent gap, so the chain restarts from the new anchor.
      if (superseded()) return;
      // A cursor only names a position while its stream is still the live one: a foreign one is
      // dropped rather than re-anchored, and the control then says there is no more to reach.
      // The stream is the one the newest page's cursor named, which this request carried.
      const nextCursor =
        read.page.cursor == null
          ? null
          : referenceHistoryCursorForStream(read.page.cursor, cursor.streamId);
      chatLoadedOlderRef.current = true;
      setChatLoadedOlder(true);
      setChatOlderCursor(nextCursor);
      setChatHasOlder(read.page.hasMore && nextCursor !== null);
      const mapped = mapReferenceTurns(read.page);
      setChatOlderMessages((prev) => [...mapped, ...prev]);
      // The oldest page a capped tail window holds is not the conversation's start: say so.
      const disclosed = referenceHistoryNotice(read.page);
      setChatWarnings((prev) => {
        const cleared = prev.filter((warning) => warning !== CHAT_OLDER_FAILED_WARNING);
        return disclosed === null ? cleared : withChatWarning(cleared, disclosed);
      });
    } catch {
      if (signal.aborted || !chatTargetIsCurrent(target) || superseded()) return;
      setChatOlderFailed(true);
      setChatWarnings((prev) => withChatWarning(prev, CHAT_OLDER_FAILED_WARNING));
    } finally {
      // A target change aborts the lane and a re-anchor bumps the generation: both clear the flag
      // themselves. A target that is only briefly null does neither, so this read clears it.
      if (!signal.aborted && !superseded()) setChatOlderLoading(false);
    }
  }, [chatOlderCursor, chatTargetIsCurrent]);

  /**
   * One mutation, as the frozen envelope carries it: target, request id, payload.
   *
   * The target echoes the gateway's published owner and epoch rather than anything this client
   * decided, so a mutation whose authority moved is refused TARGET_EXPIRED on the same path an
   * epoch change is - never re-attributed to another owner.
   */
  const chatMutationBody = useCallback(
    (target: ReferenceTargetRef, params: unknown) => ({
      requestId: newChatRequestId(),
      target: target.target,
      ...(chatAgentTypeRef.current ? { registryId: chatAgentTypeRef.current } : {}),
      params,
    }),
    [],
  );

  /**
   * Send one message into the original pane. The echo is the user's own words — never an
   * assistant turn — and a send that was not accepted puts the text back in the box.
   */
  const sendChatMessage = useCallback(
    async (rawText: string, heldId?: string): Promise<boolean> => {
      const target = chatTargetRef.current;
      const sessionId = chatSessionIdRef.current;
      const bearer = chatTokenRef.current;
      const text = rawText.trim();
      if (!text) return false;
      if (!target || !sessionId || !bearer) {
        setChatWarnings((prev) => withChatWarning(prev, CHAT_NO_SESSION_WARNING));
        return false;
      }
      if (text.length > REFERENCE_SUBMIT_MAX_CHARS) {
        setChatWarnings((prev) => withChatWarning(prev, CHAT_SUBMIT_TOO_LONG_WARNING));
        return false;
      }
      if (heldId === undefined) referenceDrafts.begin(target, text);
      else {
        chatHeldSendingRef.current.add(heldId);
        setChatHeldSendingIds([...chatHeldSendingRef.current]);
      }
      const endHeldSend = (): void => {
        if (heldId === undefined || !chatHeldSendingRef.current.delete(heldId)) return;
        setChatHeldSendingIds([...chatHeldSendingRef.current]);
      };
      const pending: MobileChatMessageProps = {
        id: `pending-${newChatRequestId()}`,
        role: "user",
        content: text,
        timestamp: Date.now(),
      };
      setChatMessages((prev) => {
        const matching = prev.filter(
          (message) => !message.id.startsWith("pending-") && message.role === "user" && message.content === text,
        );
        chatEchoBaselineRef.current.set(pending.id, {
          ids: new Set(matching.map((message) => message.id)),
          count: matching.length,
        });
        return [...prev, pending];
      });
      const payload: ReferenceSubmitPayload = { text, attachmentIds: [], origin: "chat" };
      const route: ChatLaneRoute = { baseUrl: chatBaseUrlRef.current, token: bearer };
      const mentioned = (entry: ReferenceFileReceipt): boolean => {
        const path = referenceMentionPathOf(entry.mentionText);
        return path !== null && text.includes(referenceMentionFor(path).trimEnd());
      };
      const submitted = new Map<string, ReferenceFileReceipt>();
      for (const file of chatStagedFilesRef.current) {
        if (mentioned(file)) submitted.set(submittedFileKey(target, file.receipt.attachmentId), file);
      }
      for (const [key, entry] of chatSubmittedFilesRef.current) {
        if (key === submittedFileKey(target, entry.file.receipt.attachmentId) && mentioned(entry.file)) {
          submitted.set(key, entry.file);
        }
      }
      for (const [key, file] of submitted) {
        const entry = chatSubmittedFilesRef.current.get(key) ?? { file, pending: 0, retained: false };
        entry.pending += 1;
        chatSubmittedFilesRef.current.set(key, entry);
      }
      const outcome = await postChatMutation(
        route.baseUrl,
        sessionId,
        bearer,
        "submit",
        chatMutationBody(target, payload),
      );
      const stillCurrent = chatTargetIsCurrent(target);
      const unsent: ReferenceFileReceipt[] = [];
      for (const [key] of submitted) {
        const entry = chatSubmittedFilesRef.current.get(key);
        if (!entry) continue;
        entry.pending -= 1;
        entry.retained ||= outcome.ok || referenceIsOutcomeUnknown(outcome.code);
        if (entry.pending === 0 && !entry.retained) {
          chatSubmittedFilesRef.current.delete(key);
          unsent.push(entry.file);
        }
      }
      if (!stillCurrent) void releaseStagedChatFiles(target, unsent, route).then((confirmed) => {
        let draft = referenceDrafts.read(target).text;
        for (const file of unsent) {
          if (!confirmed.has(file.receipt.attachmentId)) continue;
          const path = referenceMentionPathOf(file.mentionText);
          if (path !== null) draft = draft.split(referenceMentionFor(path)).join("");
        }
        referenceDrafts.set(target, draft);
        if (chatTargetIsCurrent(target)) setChatDraftText(draft);
      });
      else if (unsent.length > 0) {
        const known = new Set(chatStagedFilesRef.current.map((file) => file.receipt.attachmentId));
        const restored = unsent.filter((file) => !known.has(file.receipt.attachmentId));
        chatStagedFilesRef.current = [...chatStagedFilesRef.current, ...restored];
        setChatStagedFiles((prev) => [...prev, ...restored]);
      }
      if (outcome.ok) {
        if (stillCurrent) {
          // The mentions this text carried were delivered: those files are the agent's now, not
          // staged ones the user may still remove or that count against the next turn.
          const remaining = chatStagedFilesRef.current.filter((entry) => !mentioned(entry));
          chatStagedFilesRef.current = remaining;
          setChatStagedFiles((prev) => prev.filter((entry) => !mentioned(entry)));
          setChatWarnings((prev) =>
            prev.filter(
              (warning) => warning !== CHAT_SEND_FAILED_WARNING && warning !== CHAT_OUTCOME_UNKNOWN_WARNING,
            ),
          );
        }
        if (heldId === undefined) {
          // the acknowledgement settles the SENT PREFIX only: an edit made while it was in
          // flight survives, and so does everything typed after it
          const settled = referenceDrafts.settle(target, text);
          referenceDrafts.end(target, text);
          if (stillCurrent) setChatDraftText(settled.text);
        } else {
          referenceQueues.remove(target, heldId);
          endHeldSend();
          if (!stillCurrent) return true;
          setChatHeld([...referenceQueues.read(target)]);
        }
        if (stillCurrent) setChatComposerNotice(null);
        return true;
      }
      if (!stillCurrent) {
        endHeldSend();
        if (heldId === undefined) {
          const current = referenceDrafts.read(target).text;
          if (current.length === 0) referenceDrafts.set(target, text);
          else if (!current.startsWith(text)) referenceDrafts.set(target, `${text}\n${current}`);
          referenceDrafts.end(target, text);
        }
        return false;
      }
      // A refusal or an uncertain outcome removes the unconfirmed echo and restores the draft.
      setChatComposerNotice(
        referenceIsOutcomeUnknown(outcome.code)
          ? CHAT_OUTCOME_UNKNOWN_WARNING
          : CHAT_SEND_FAILED_WARNING,
      );
      setChatMessages((prev) => prev.filter((message) => message.id !== pending.id));
      if (heldId === undefined) {
        const current = referenceDrafts.read(target).text;
        // the failed text comes back in front of whatever the user typed while it was in flight
        if (current.length === 0) referenceDrafts.set(target, text);
        else if (!current.startsWith(text)) referenceDrafts.set(target, `${text}\n${current}`);
        referenceDrafts.end(target, text);
        setChatDraftText(referenceDrafts.read(target).text);
      } else {
        endHeldSend();
      }
      setChatWarnings((prev) =>
        withChatWarning(
          prev,
          referenceIsOutcomeUnknown(outcome.code) ? CHAT_OUTCOME_UNKNOWN_WARNING : CHAT_SEND_FAILED_WARNING,
        ),
      );
      return false;
    },
    [chatMutationBody, chatTargetIsCurrent, releaseStagedChatFiles],
  );

  /**
   * Stop the pane's own turn. `refused` is an answer, not a reason to send a killing signal: an
   * unknown capability sends nothing and says so, and the explicit terminal keeps Ctrl-C.
   */
  const chatStopInFlightRef = useRef(new Set<string>());
  const stopChatExecution = useCallback(async () => {
    const target = chatTargetRef.current;
    const sessionId = chatSessionIdRef.current;
    const bearer = chatTokenRef.current;
    if (!target || !sessionId || !bearer) {
      setChatWarnings((prev) => withChatWarning(prev, CHAT_NO_SESSION_WARNING));
      return;
    }
    const capability = chatStopCapabilityFor(chatAgentTypeRef.current);
    if (capability === "refused") {
      setChatComposerNotice(CHAT_STOP_REFUSED_WARNING);
      setChatWarnings((prev) => withChatWarning(prev, CHAT_STOP_REFUSED_WARNING));
      return;
    }
    // Each request carries a fresh requestId, so the host would send a second interrupt for a
    // second tap; in Claude Code that second Escape opens the rewind picker on the idle prompt.
    const stopKey = referenceTargetKey(target);
    if (chatStopInFlightRef.current.has(stopKey)) return;
    chatStopInFlightRef.current.add(stopKey);
    const payload: ReferenceStopPayload = { capability };
    let outcome: Awaited<ReturnType<typeof postChatMutation>>;
    try {
      outcome = await postChatMutation(
        chatBaseUrlRef.current,
        sessionId,
        bearer,
        "stop",
        chatMutationBody(target, payload),
      );
    } finally {
      chatStopInFlightRef.current.delete(stopKey);
    }
    if (!outcome.ok && chatTargetIsCurrent(target)) {
      setChatWarnings((prev) => withChatWarning(prev, CHAT_STOP_FAILED_WARNING));
    }
  }, [chatMutationBody, chatTargetIsCurrent]);

  /**
   * Answer the prompt the user is looking at. The card owns the press/focus rules; this only
   * carries the frozen payload and reports a moved screen back to the card as a typed refusal.
   */
  const answerChatPrompt = useCallback(
    async (payload: ReferencePromptAnswerPayload): Promise<boolean> => {
      const target = chatTargetRef.current;
      const sessionId = chatSessionIdRef.current;
      const bearer = chatTokenRef.current;
      if (!target || !sessionId || !bearer) {
        throw new ReferencePromptRefused("UNAUTHORIZED", CHAT_NO_SESSION_WARNING);
      }
      const promptGeneration = chatPromptGenerationRef.current;
      const outcome = await postChatMutation(
        chatBaseUrlRef.current,
        sessionId,
        bearer,
        "answer",
        chatMutationBody(target, payload),
      );
      // A later prompt in the same pane also owns its own input and errors. Settle the old
      // request truthfully without touching the current occurrence or interrupting its poll.
      if (
        !chatTargetIsCurrent(target) ||
        chatPromptGenerationRef.current !== promptGeneration ||
        chatPromptIdRef.current !== payload.promptId ||
        chatPromptRevisionRef.current !== payload.screenRevision
      ) {
        if (!outcome.ok) throw new ReferencePromptRefused(outcome.code, outcome.message);
        return true;
      }
      if (!outcome.ok) {
        setChatPromptError(outcome.message);
        throw new ReferencePromptRefused(outcome.code, outcome.message);
      }
      setChatTypedAnswer(null);
      setChatPromptError(null);
      chatPromptGenerationRef.current += 1;
      setChatPrompt(null);
      chatPromptIdRef.current = null;
      chatPromptRevisionRef.current = null;
      setChatScreenRevision(null);
      setChatPromptRefresh((generation) => generation + 1);
      return true;
    },
    [chatMutationBody, chatTargetIsCurrent],
  );

  /** The owning host's own routes, as the file lane's transport. Nothing is staged locally. */
  const chatFileTransport = useMemo<ReferenceFileStagingTransport>(
    () => ({
      stage: async (target, payload, signal) => {
        const sessionId = chatSessionIdRef.current;
        const bearer = chatTokenRef.current;
        if (!sessionId || !bearer) {
          throw new ReferenceFileError("UNAUTHORIZED", "no session is attached to stage a file");
        }
        const outcome = await postChatMutation(
          chatBaseUrlRef.current,
          sessionId,
          bearer,
          "files",
          chatMutationBody({ target }, payload),
          signal,
        );
        if (!outcome.ok) {
          throw new ReferenceFileError(outcome.code as never, outcome.message, outcome.retryable);
        }
        return outcome.data;
      },
      cancel: async (target, attachmentId) => {
        const sessionId = chatSessionIdRef.current;
        const bearer = chatTokenRef.current;
        if (!sessionId || !bearer) return false;
        const query = referenceChatReadQuery(
          { target },
          chatAgentTypeRef.current ?? "",
          undefined,
          null,
        );
        try {
          const response = await remoteFetch(
            chatBaseUrlRef.current,
            `${referenceChatRoute(sessionId, `files/${encodeURIComponent(attachmentId)}`)}?${query}`,
            { method: "DELETE", headers: { Authorization: `Bearer ${bearer}` } },
          );
          return response.ok;
        } catch {
          return false;
        }
      },
      remove: async (target, attachmentId) => {
        const sessionId = chatSessionIdRef.current;
        const bearer = chatTokenRef.current;
        if (!sessionId || !bearer) return false;
        const query = referenceChatReadQuery({ target }, chatAgentTypeRef.current ?? "", undefined, null);
        try {
          const response = await remoteFetch(
            chatBaseUrlRef.current,
            `${referenceChatRoute(sessionId, `files/${encodeURIComponent(attachmentId)}`)}?${query}`,
            { method: "DELETE", headers: { Authorization: `Bearer ${bearer}` } },
          );
          return response.ok;
        } catch {
          return false;
        }
      },
    }),
    [chatMutationBody],
  );

  /** Stage files on the owning host and put their mentions in the draft, at the caret. */
  const attachChatFiles = useCallback(
    async (files: readonly File[], caret: number) => {
      const target = chatTargetRef.current;
      if (!target) {
        setChatWarnings((prev) => withChatWarning(prev, CHAT_NO_SESSION_WARNING));
        return;
      }
      setChatAttaching(true);
      setChatAttachError(null);
      // The host these upload to: an abandoned upload is released there, not on whatever host
      // the lane has moved to by then.
      const route: ChatLaneRoute = { baseUrl: chatBaseUrlRef.current, token: chatTokenRef.current };
      const staged: ReferenceFileReceipt[] = [];
      let failure: string | null = null;
      const signal = chatLaneAbortRef.current.signal;
      for (const file of files) {
        try {
          const used = [...chatStagedFiles, ...staged].reduce((sum, entry) => sum + entry.receipt.sizeBytes, 0);
          const receipt = await stageReferenceChatFile(
            { transport: chatFileTransport },
            target,
            file,
            signal,
            { fileCount: chatStagedFiles.length + staged.length, turnBytes: used },
          );
          staged.push(receipt);
        } catch (error) {
          failure = error instanceof Error ? error.message : String(error);
          break;
        }
        if (!chatTargetIsCurrent(target)) break;
      }
      if (signal.aborted || !chatTargetIsCurrent(target)) {
        // The pane changed while these uploaded: the lane they were for is gone, so nothing is
        // inserted anywhere and the host is told to let them go.
        void releaseStagedChatFiles(target, staged, route);
        // A target that is only briefly null resets nothing, so this upload clears its own flag;
        // a real target change aborted the lane and already reset it.
        if (!signal.aborted) setChatAttaching(false);
        return;
      }
      setChatAttaching(false);
      if (failure !== null) setChatAttachError(failure);
      if (staged.length === 0) return;
      chatStagedFilesRef.current = [...chatStagedFilesRef.current, ...staged];
      setChatStagedFiles((prev) => [...prev, ...staged]);
      const paths = staged.map((entry) => referenceMentionPathOf(entry.mentionText));
      const usable = paths.filter((path): path is string => path !== null);
      if (usable.length === 0) return;
      let inserted = referenceDrafts.read(target).text;
      let at = caret;
      for (const path of usable) {
        const next = referenceMentionInsertion(inserted, path, at);
        inserted = next.text;
        at = next.caret;
      }
      referenceDrafts.set(target, inserted);
      setChatDraftText(inserted);
    },
    [chatFileTransport, chatStagedFiles, chatTargetIsCurrent, releaseStagedChatFiles],
  );

  /** Delete a staged file. Explicit, and the only path that removes one. */
  const removeChatStagedFile = useCallback(
    async (attachmentId: string) => {
      const target = chatTargetRef.current;
      if (!target) return;
      if (chatSubmittedFilesRef.current.has(submittedFileKey(target, attachmentId))) return;
      const entry = chatStagedFiles.find((file) => file.receipt.attachmentId === attachmentId);
      if (!entry) return;
      if (heldChatFileIsReferenced(target, entry)) {
        setChatAttachError("Edit or remove the held messages referencing this file before removing it.");
        return;
      }
      const cleaned = await cancelReferenceChatFile({ transport: chatFileTransport }, target, entry);
      const stillCurrent = chatTargetIsCurrent(target);
      if (!cleaned.cleaned) {
        if (stillCurrent) setChatAttachError("The host did not confirm that the staged file was removed.");
        return;
      }
      if (stillCurrent) {
        chatStagedFilesRef.current = chatStagedFilesRef.current.filter(
          (file) => file.receipt.attachmentId !== attachmentId,
        );
        setChatStagedFiles((prev) =>
          prev.filter((file) => file.receipt.attachmentId !== attachmentId),
        );
      }
      const path = referenceMentionPathOf(entry.mentionText);
      if (path === null) return;
      const mention = referenceMentionFor(path);
      const current = referenceDrafts.read(target).text;
      const next = current.split(mention).join("");
      if (next !== current) {
        // The draft is the target's own: it is updated there, and shown only while it is current.
        referenceDrafts.set(target, next);
        if (stillCurrent) setChatDraftText(next);
      }
    },
    [chatFileTransport, chatStagedFiles, chatTargetIsCurrent],
  );

  /** Hold a message the prompt's menu cannot take, for the user to send later. */
  const holdChatMessage = useCallback((text: string) => {
    const target = chatTargetRef.current;
    if (!target) return;
    referenceQueues.add(target, text);
    setChatComposerNotice(CHAT_HELD_WARNING);
    setChatHeld([...referenceQueues.read(target)]);
  }, []);

  const editChatHeld = useCallback((id: string, text: string) => {
    const target = chatTargetRef.current;
    if (!target) return;
    referenceQueues.edit(target, id, text);
    setChatHeld([...referenceQueues.read(target)]);
  }, []);

  const removeChatHeld = useCallback((id: string) => {
    const target = chatTargetRef.current;
    if (!target) return;
    referenceQueues.remove(target, id);
    setChatHeld([...referenceQueues.read(target)]);
  }, []);

  /** The composer's own send: a prompt turns the text into an answer, or holds it. */
  const handleChatComposerSend = useCallback(
    (text: string): boolean => {
      if (chatPrompt !== null) {
        const typed = referenceAnswerFromText(chatPrompt, text);
        if (typed === null) {
          // The menu takes its own options only: the text is held for the user, never guessed
          // into a key press, and never silently dropped. `false` keeps it in the message box.
          holdChatMessage(text);
          setChatWarnings((prev) => withChatWarning(prev, CHAT_HELD_WARNING));
          return false;
        }
        // An answer with no live session would be refused before it is sent: keep the text.
        if (!chatTargetRef.current || !chatSessionIdRef.current || !chatTokenRef.current) {
          setChatWarnings((prev) => withChatWarning(prev, CHAT_NO_SESSION_WARNING));
          return false;
        }
        if (referencePromptNeedsConfirmation(chatPrompt, typed)) {
          // A typed pick of an approval, a plan or a menu waits for the card's Confirm.
          setChatTypedAnswer(typed);
          return true;
        }
        if (!referenceAnswerIsSingleChoice(typed)) {
          // An answer naming two options is ambiguous: it is neither sent nor held.
          setChatWarnings((prev) => withChatWarning(prev, CHAT_AMBIGUOUS_ANSWER_WARNING));
          return false;
        }
        const revision = chatScreenRevision;
        if (revision === null) {
          setChatPromptError(REFERENCE_PROMPT_STALE_MESSAGE);
          return false;
        }
        const target = chatTargetRef.current;
        void answerChatPrompt({
          promptId: chatPrompt.id,
          screenRevision: revision,
          answer: typed,
        }).catch(() => {
          // The card reports the refusal it was given; the composer cleared the box on send, so
          // the typed answer comes back in front of anything typed since.
          // The draft is the target's own: a pane left meanwhile gets it back in its store.
          if (target === null) return;
          const current = referenceDrafts.read(target).text;
          if (current.startsWith(text)) return;
          const restored = current.length === 0 ? text : `${text}\n${current}`;
          referenceDrafts.set(target, restored);
          if (chatTargetIsCurrent(target)) setChatDraftText(restored);
        });
        return true;
      }
      // the checks that refuse before anything is sent answer here, so the box keeps its text
      const trimmed = text.trim();
      if (!trimmed) return false;
      if (!chatTargetRef.current || !chatSessionIdRef.current || !chatTokenRef.current) {
        setChatWarnings((prev) => withChatWarning(prev, CHAT_NO_SESSION_WARNING));
        return false;
      }
      if (trimmed.length > REFERENCE_SUBMIT_MAX_CHARS) {
        setChatWarnings((prev) => withChatWarning(prev, CHAT_SUBMIT_TOO_LONG_WARNING));
        return false;
      }
      void sendChatMessage(text);
      return true;
    },
    [answerChatPrompt, chatPrompt, chatScreenRevision, chatTargetIsCurrent, holdChatMessage, sendChatMessage],
  );

  const chatPartContext = useMemo<ReferencePartRenderContext>(
    () => ({
      resolveImageUrl: (_image: ReferenceImageRef) => {
        // An <img src> cannot carry the bearer header, and the server rejects URL-borne device
        // tokens (they leak through logs, history and Referer). The old bare relative path was
        // never authorized either, so render the labelled chip fallback until images are
        // fetched with the bearer header into blob URLs.
        return null;
      },
    }),
    [],
  );

  /** The disclosure for this page's source; null when the page is a native transcript. */
  const chatPageDisclosure = useMemo<string | null>(
    () => (chatPage === null ? null : referenceHistoryNotice(chatPage)),
    [chatPage],
  );

  const chatDraftUnsaved =
    chatReferenceTarget === null ? false : referenceDrafts.isUnsaved(chatReferenceTarget);
  const chatHeldUnsaved =
    chatReferenceTarget === null ? false : referenceQueues.isUnsaved(chatReferenceTarget);

  /** Every draft edit goes to the target's own store, so it survives a mode switch. */
  const handleChatDraftChange = useCallback((text: string) => {
    const target = chatTargetRef.current;
    setChatDraftText(text);
    if (target !== null) referenceDrafts.set(target, text);
  }, []);

  /** The card the original pane is waiting on. Its press and focus rules are the card's own. */
  const chatPromptCard = useMemo(() => {
    if (chatPrompt === null || chatScreenRevision === null) return null;
    return (
      <ReferencePromptCard
        // Returning to identical content after a clear or transition is still a new card.
        key={chatPromptGenerationRef.current}
        prompt={chatPrompt}
        screenRevision={chatScreenRevision}
        typedAnswer={chatTypedAnswer}
        error={chatPromptError}
        onAnswer={answerChatPrompt}
        onPromptChanged={() => {
          // The screen moved under the answer: say so and re-read it before anything else.
          setChatPromptError(REFERENCE_PROMPT_STALE_MESSAGE);
          setChatPromptRefresh((generation) => generation + 1);
        }}
        onAnswered={(toMessageBox) => {
          setChatTypedAnswer(null);
          if (toMessageBox) chatComposerRef.current?.focus();
        }}
        onTypedAnswerDone={() => setChatTypedAnswer(null)}
      />
    );
  }, [answerChatPrompt, chatPrompt, chatPromptError, chatScreenRevision, chatTypedAnswer]);

  /** Older pages first, then the newest page and the user's own pending echoes. */
  const chatTranscript = useMemo(
    () => {
      const all = chatOlderMessages.length === 0 ? chatMessages : [...chatOlderMessages, ...chatMessages];
      // A turn id is its content, so a repeated turn repeats it: each repeat gets its own key.
      const seen = new Map<string, number>();
      return all.map((message) => {
        const repeat = seen.get(message.id) ?? 0;
        seen.set(message.id, repeat + 1);
        return repeat === 0 ? message : { ...message, id: `${message.id}~${repeat}` };
      });
    },
    [chatMessages, chatOlderMessages],
  );

  /** What the message box says while a prompt waits, so the user knows what typing does. */
  const chatComposerPlaceholder =
    chatPrompt !== null ? referenceAnswerHint(chatPrompt) : undefined;

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
      const response = await remoteFetch(transportBaseUrl, "/api/v1/workspace/worktrees", {
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
        onLoginSuccess={(tok, email) => {
          storeAccountEmail(email);
          setAccountEmail(email || null);
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
    if (planLimitSuspendedRef.current) return false;
    // A new pick supersedes a timed-out attempt before the effect issues the new selection.
    lateSelectionRef.current = null;
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
      ...(accountOpt.attachOnly ? { attachOnly: true } : {}),
    };
    setCreationError(null);

    // A relay drops an idle tunnel (about 2 minutes without traffic); a closed connection must be
    // reacquired, never reused, or every request on it fails with STREAM_CLOSED.
    if (activeTunnelConnection && !activeTunnelConnection.isClosed && token && activeMachineId === accountOpt.machineId) {
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
    if (planLimitSuspendedRef.current) {
      conn?.close();
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

  const CONNECTION_SHORT: Record<string, string> = {
    relay: "Relay",
    lan: "LAN",
    tailscale: "Tailscale",
  };

  const terminalTabs =
    effectiveModel.context.terminalTabs ?? model.context.terminalTabs ?? [];
  const activeTerminalTab = terminalTabs.find(
    (tab) => tab.id === model.context.activeTabId,
  );
  const mobileTitle =
    activeTerminalTab?.label ??
    model.context.activeTerminal?.title ??
    model.context.worktreeLabel ??
    model.context.workspaceId ??
    "Ferryx";
  const mobileAgentType = activeTerminalTab?.agentType ?? null;
  const mobileActivityState = (activeTerminalTab?.activityState as MobileTopBarActivityState) ?? null;
  const mobileHostBadge = accountPreselection
    ? undefined
    : {
        label:
          activeTunnelConnection?.machine.displayName ||
          CONNECTION_SHORT[transport.type] ||
          transport.type,
        transport: transport.type,
        ariaLabel: `Connection: ${CONNECTION_BADGE_LABEL[transport.type]}`,
      };
  const mobileAttention = firstWaiting
    ? {
        label: firstWaiting.label,
        count: waitingCount,
        ariaLabel: attentionAriaLabel,
        disabled: pending !== null,
        onPress: () => {
          void selectContext({
            workspaceId: firstWaiting.workspaceId,
            worktreeSlug: firstWaiting.worktreeSlug,
            worktreeLabel: firstWaiting.worktreeLabel,
            tabId: firstWaiting.tabId,
            sessionId: firstWaiting.sessionId,
          });
          document
            .querySelector<HTMLTextAreaElement>(
              'textarea[data-testid="remote-terminal-input-sink"]',
            )
            ?.focus({ preventScroll: true });
        },
      }
    : null;

  /* Phone home wiring. Every destination opens a session through the same account selection
     the worktree picker uses, then pushes the Session screen once the selection is under way. */
  const showPhoneHome = phoneHome && (phoneNav.screen.kind === "home" || (!token && !initialAccountTarget));
  const openPhoneSession = (option: AccountWorktreeOption) => {
    void selectAccountOption(option).then((ok) => {
      if (ok) phoneNav.openSession();
    });
  };
  const phoneNeedsYou = new Set(
    accountDiscovery.accountOptions
      .filter((option) => option.sessionId && option.inventoryActivityState === "waiting")
      .map((option) => `${option.machineId}\u0000${option.sessionId}`),
  );
  const phoneInboxRows: InboxRow[] = phoneHome
    ? buildInboxRows({
        options: accountDiscovery.accountOptions,
        inventories: accountDiscovery.sessionInventories,
        lanes: phoneSupervisor.lanes,
      })
    : [];
  const activeSupervisorSnapshot = phoneHome && activeMachineId
    ? phoneSupervisor.machines.find((machine) => machine.machineId === activeMachineId)?.snapshot ?? null
    : null;
  const sessionSupervision = activeSupervisorSnapshot
    ? paneSupervision(activeSupervisorSnapshot, effectiveSessionId, phoneSupervisor.now, chatActivity === "waiting")
    : null;
  const focusSessionInput = () => {
    if (viewMode === "chat" && chatComposerRef.current) {
      chatComposerRef.current.focus();
      return;
    }
    document
      .querySelector<HTMLTextAreaElement>('textarea[data-testid="remote-terminal-input-sink"]')
      ?.focus({ preventScroll: true });
  };
  const phoneWorktrees = (
    <WorktreesHome onOpenAccount={() => setAccountSheetOpen(true)}>
      <RemoteWorkspaceMirror
        inline
        model={effectiveModel}
        pending={pending}
        selectorOpen
        onSelectorOpenChange={() => {}}
        onSignOut={handleSignOut}
        activeMachineId={activeMachineId}
        sidebarSections={sidebarSections}
        pickerStatus={accountPickerStatus}
        onSelect={(option) => {
          const accountOpt = (option as Partial<AccountWorktreeOption>).machineId
            ? (option as AccountWorktreeOption)
            : null;
          if (accountOpt) {
            openPhoneSession(accountOpt);
            return;
          }
          void selectContext(option).then((ok) => {
            if (ok) phoneNav.openSession();
          });
        }}
        onCreateTerminal={() => {
          if (!model.context.workspaceId) return;
          void selectContext({ workspaceId: model.context.workspaceId, worktreeSlug: model.context.worktreeSlug, worktreeLabel: model.context.worktreeLabel }, true).then((ok) => {
            if (ok) phoneNav.openSession();
          });
        }}
        onCreateWorktree={createWorktree}
        creationError={creationError}
      />
    </WorktreesHome>
  );

  return (
    <div className="flex h-[100dvh] min-h-0 min-w-0 flex-col overflow-hidden remote-app-root bg-background text-foreground" style={viewportHeight ? { height: viewportHeight } : undefined}>
      <Toaster />
      {showPhoneHome ? (
        <PhoneHomeView
          destination={phoneNav.destination}
          onSelectTab={phoneNav.selectTab}
          supervisor={{
            machines: phoneSupervisor.machines,
            now: phoneSupervisor.now,
            needsYou: phoneNeedsYou,
            onSetPaused: phoneSupervisor.setPaused,
            onOpenLane: (machineId, sessionId) => {
              const option = optionForSession(accountDiscovery.accountOptions, machineId, sessionId);
              if (!option) {
                toast.error("This session is not available right now", {
                  description: "Its computer is offline or has not listed it yet.",
                });
                return;
              }
              openPhoneSession(option);
            },
          }}
          inbox={{
            rows: phoneInboxRows,
            loading: accountInventoryLoading,
            onOpen: (row) => openPhoneSession(row.option),
          }}
          worktrees={phoneWorktrees}
          resume={
            token && model.context.workspaceId
              ? { title: mobileTitle, subtitle: contextName(model.context), onOpen: phoneNav.openSession }
              : null
          }
        />
      ) : (
      <>
      {desktopSidebar ? (
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

          <div className="flex items-center gap-1 border-l border-chat-border/40 pl-2">
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
      ) : (
        <MobileTopBar
          title={mobileTitle}
          agentType={mobileAgentType}
          activityState={mobileActivityState}
          contextLabel={contextName(model.context)}
          hostBadge={mobileHostBadge}
          pickerOpen={selectorOpen}
          onTogglePicker={() => setSelectorOpen((open) => !open)}
          attention={mobileAttention}
          viewMode={viewMode as "chat" | "terminal"}
          onViewModeChange={setViewMode}
          sessionCount={terminalTabs.length >= 1 ? terminalTabs.length : undefined}
          switcherOpen={switcherOpen}
          onOpenSwitcher={() => setSwitcherOpen(true)}
          menuOpen={menuOpen}
          onOpenMenu={() => setMenuOpen(true)}
          hideControls={accountPreselection}
          onBack={phoneHome ? phoneNav.back : undefined}
        />
      )}
      {phoneHome && sessionSupervision && activeMachineId ? (
        <SupervisorSessionStrip
          state={sessionSupervision}
          now={phoneSupervisor.now}
          onSetPaused={(laneKey, paused) => phoneSupervisor.setPaused(activeMachineId, laneKey, paused)}
          onFocusInput={focusSessionInput}
        />
      ) : null}

      {showRelayRepairNotice ? (
        <div
          data-testid="relay-repair-notice"
          role={relayRepairError ? "alert" : "status"}
          data-code={REPAIR_REQUIRED}
          className={`mx-3 mt-2 rounded-md border px-3 py-2 text-xs ${relayRepairError ? "border-destructive/50 text-destructive" : "border-border text-muted-foreground"}`}
        >
          <p>{RELAY_REPAIR_MESSAGE}</p>
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              className="rounded border border-border px-2 py-1 font-medium text-foreground"
              onClick={() => {
                setRelayRepairErrorBase(null);
                disconnect();
              }}
            >
              Re-pair through account
            </button>
            {!relayRepairError ? (
              <button
                type="button"
                className="rounded px-2 py-1"
                onClick={() => setRelayRepairDismissedBase(transportBaseUrl)}
              >
                Not now
              </button>
            ) : null}
          </div>
        </div>
      ) : null}
      <RemoteWorkspaceMirror
        model={effectiveModel}
        pending={pending}
        selectorOpen={selectorOpen}
        onSelectorOpenChange={setSelectorOpen}
        // Account mode never exposes a machine chooser; sign-out stays reachable in the picker.
        onOpenHosts={isAccountMode ? undefined : () => setHostDrawerOpen(true)}
        onSignOut={isAccountMode ? handleSignOut : undefined}
        activeMachineId={activeMachineId}
        sidebarSections={sidebarSections}
        supervisor={sidebarSupervisor}
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
        {planLimitNotice ? (
          <PlanLimitNotice
            state={planLimitNotice}
            onRetry={() => {
              // Clearing the notice re-enables discovery, so this retry is a full
              // rediscovery for both the machine-less and the machine-scoped refusal.
              setPlanLimitNotice(null);
              workspaceRefreshVersionRef.current += 1;
            }}
          />
        ) : accountPreselection ? (
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
                showHeader={desktopSidebar}
                headerTitle={
                  model.context.terminalTabs?.find((t) => t.id === model.context.activeTabId)?.label ??
                  model.context.activeTerminal?.title ??
                  model.context.workspaceId ??
                  undefined
                }
                headerSubtitle={model.context.worktreeLabel ?? model.context.workspaceId ?? undefined}
                workspaceLabel={model.context.workspaceId ?? undefined}
                worktreeLabel={model.context.worktreeLabel ?? undefined}
                onBack={() => setSelectorOpen(true)}
                messages={chatTranscript}
                warnings={chatWarnings}
                isRunning={chatIsRunning}
                activityState={chatActivity === "working" ? "thinking" : "idle"}
                onSendMessage={handleChatComposerSend}
                onStopExecution={() => void stopChatExecution()}
                onOpenTerminal={() => setViewMode("terminal")}
                pageDisclosure={chatPageDisclosure}
                hasOlderPage={chatHasOlder}
                loadedOlder={chatLoadedOlder}
                olderState={chatOlderLoading ? "loading" : chatOlderFailed ? "failed" : "idle"}
                onLoadOlder={() => void loadChatOlderPage()}
                draft={chatDraftText}
                onDraftChange={handleChatDraftChange}
                draftUnsaved={chatDraftUnsaved}
                stagedFiles={chatStagedFiles}
                onAttachFiles={(files, caret) => void attachChatFiles(files, caret)}
                onRemoveStagedFile={(attachmentId) => void removeChatStagedFile(attachmentId)}
                attaching={chatAttaching}
                attachError={chatAttachError}
                heldMessages={chatHeld}
                heldSendingIds={chatHeldSendingIds}
                heldUnsaved={chatHeldUnsaved}
                onEditHeld={editChatHeld}
                onSendHeld={(id) => {
                  // A row already on its way is not sent again.
                  if (chatHeldSendingRef.current.has(id)) return;
                  const held = chatHeld.find((message) => message.id === id);
                  if (held) void sendChatMessage(held.text, id);
                }}
                onRemoveHeld={removeChatHeld}
                promptCard={chatPromptCard}
                composerHandleRef={chatComposerRef}
                composerWarning={chatComposerNotice}
                composerPlaceholder={chatComposerPlaceholder}
                referenceContext={chatPartContext}
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
                        if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229 && (e.target as HTMLInputElement).value.trim()) {
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
              showTitleBar={desktopSidebar}
              sessionId={effectiveSessionId}
              token={token}
              title={model.context.worktreeLabel ?? model.context.workspaceId ?? undefined}
              transportUrl={transportBaseUrl}
              onTransportFailure={rollbackTransport}
              activeTabId={model.context.activeTabId}
              onBack={() => setViewMode("chat")}
              embedded
              onSwipePreviousTab={handleSwipePreviousTab}
              onSwipeNextTab={handleSwipeNextTab}
              onSocketLifecycle={handleTerminalSocketLifecycle}
              isAccountSession={Boolean(activeTunnelConnection)}
              daemonEpoch={sessionEpochs[effectiveSessionId] ?? sessionEpochsRef.current.get(effectiveSessionId) ??
                (model.context.activeTerminal?.sessionId === effectiveSessionId ? model.context.daemonEpoch : undefined)}
              onAgentStateFrame={handleMachineAgentStateFrame}
              createWebSocket={terminalCreateWebSocket}
            />
          ) : null}
      </RemoteWorkspaceMirror>
      </>
      )}

      <MobileHostDrawer
        open={hostDrawerOpen}
        onOpenChange={setHostDrawerOpen}
        onDisconnect={disconnect}
        onSignOut={handleSignOut}
        isAccountSession={Boolean(activeTunnelConnection)}
      />

      <MobileSessionSwitcherSheet
        open={switcherOpen}
        onOpenChange={setSwitcherOpen}
        tabs={terminalTabs}
        activeTabId={model.context.activeTabId}
        worktreeLabel={model.context.worktreeLabel}
        pending={pending}
        disabled={pending !== null || !model.context.workspaceId}
        onSelectTab={(tab) => {
          const option = paneSelectionOption(model, tab);
          if (option) {
            void selectContext(option);
          }
          setSwitcherOpen(false);
        }}
        onNewTerminal={() => {
          if (!model.context.workspaceId) return;
          void selectContext(
            {
              workspaceId: model.context.workspaceId,
              worktreeSlug: model.context.worktreeSlug,
              worktreeLabel: model.context.worktreeLabel,
            },
            true,
          );
        }}
      />

      <MobileOverflowSheet
        open={menuOpen}
        onOpenChange={setMenuOpen}
        onNewTerminal={() => {
          if (!model.context.workspaceId) return;
          void selectContext(
            {
              workspaceId: model.context.workspaceId,
              worktreeSlug: model.context.worktreeSlug,
              worktreeLabel: model.context.worktreeLabel,
            },
            true,
          );
        }}
        onNewWorktree={() => {
          void createWorktree();
        }}
        onOpenMachines={isAccountMode ? undefined : () => setHostDrawerOpen(true)}
        onSignOut={isAccountMode ? handleSignOut : undefined}
        connectionLabel={
          accountPreselection
            ? undefined
            : activeTunnelConnection?.machine.displayName ||
              CONNECTION_SHORT[transport.type] ||
              transport.type
        }
        buildStamp={BUILD_STAMP}
        disabled={pending !== null || !model.context.workspaceId}
      />

      {isAccountMode ? (
        <AccountSheet
          open={accountSheetOpen}
          onOpenChange={setAccountSheetOpen}
          email={accountEmail}
          onSignOut={handleSignOut}
          onDelete={(confirmEmail) =>
            deleteAccount(getStoredAccountTokenOrigin() || relayUrl, accountSessionToken ?? "", confirmEmail)
          }
          onDeleted={handleLogout}
        />
      ) : null}
    </div>
  );
};
