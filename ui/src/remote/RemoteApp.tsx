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
  normalizeRemoteWorkspaceState,
  RemoteWorkspaceMirror,
  type RemoteContextOption,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";
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
import { referenceDrafts } from "./chat/referenceDraft";
import { referenceQueues, type HeldMessage } from "./chat/referenceQueue";
import { ReferencePromptCard } from "./chat/ReferencePromptCard";
import {
  ReferenceFileError,
  cancelReferenceChatFile,
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
  remoteApiUrl,
  remoteApiUrl as apiUrl,
  remoteSocketUrl,
} from "./remoteClient";

import { AccountLoginPage } from "./AccountLoginPage";
import {
  useAccountWorktrees,
  type AccountWorktreeOption,
} from "./useAccountWorktrees";

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
  getAccountLastSelectedTarget,
  setAccountLastSelectedTarget,
  clearAccountLastSelectedTarget,
  logoutAccountSession,
  createAccountConnection,
  type AccountConnection,
} from "./accountSession";
import type { TunnelTransport, TunnelWebSocket } from "./attachTunnel";

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

/**
 * The identity the owning host publishes for itself, as `/api/v1/capabilities` answers it.
 *
 * `daemonEpoch` is the value the reference-chat route compares a target against, so it is the
 * authoritative incarnation on EVERY transport (relay, direct and the account tunnel alike).
 * `referenceHostId` is the gateway's own `reference_host_id()` where it publishes one.
 * `machineId` is the host's machine identity: recorded for diagnostics, NEVER substituted for
 * the host id, because the two are different values.
 */
export interface ChatGatewayIdentity {
  readonly daemonEpoch: string;
  readonly referenceHostId: string | null;
  readonly machineId: string | null;
}

/** Field names a gateway may publish its reference host id under. */
const CHAT_HOST_ID_FIELDS: readonly string[] = ["referenceHostId", "hostId"];

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
  return { daemonEpoch, referenceHostId, machineId: chatText(body.machineId) };
}

/** The gateway's own identity, from the one authenticated route that publishes it. */
export async function readChatGatewayIdentity(args: {
  baseUrl: string;
  token: string;
  signal?: AbortSignal;
}): Promise<ChatGatewayIdentity | null> {
  let response: Response;
  try {
    response = await fetch(remoteApiUrl(args.baseUrl, "/api/v1/capabilities"), {
      headers: { Authorization: `Bearer ${args.token}` },
      signal: args.signal,
    });
  } catch {
    return null;
  }
  if (!response.ok) return null;
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
    const response = await fetch(remoteApiUrl(args.baseUrl, "/api/v1/sessions"), {
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

function chatTurnId(seed: string, occurrence: number): string {
  let hash = 0;
  for (let index = 0; index < seed.length; index += 1) {
    hash = (hash * 31 + seed.charCodeAt(index)) >>> 0;
  }
  const base = `turn-${hash.toString(36)}`;
  return occurrence === 0 ? base : `${base}-${occurrence}`;
}

/**
 * One reference page as chat messages: a turn per message, its rich parts intact, its own
 * abandoned disclosure left for the page to compose. No assistant turn is ever invented.
 */
function mapReferenceTurns(page: ReferenceHistoryPage): MobileChatMessageProps[] {
  const occurrences = new Map<string, number>();
  return page.turns.map((turn) => {
    const seed = chatTurnSeed(turn);
    const occurrence = occurrences.get(seed) ?? 0;
    occurrences.set(seed, occurrence + 1);
    const text = turn.parts
      .map((part) =>
        part.kind === "text" || part.kind === "thinking" || part.kind === "compact" ? part.text : null,
      )
      .filter((part): part is string => part !== null)
      .join("\n\n");
    return {
      id: chatTurnId(seed, occurrence),
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
    response = await fetch(remoteApiUrl(baseUrl, referenceChatRoute(sessionId, route)), {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal,
    });
  } catch (error) {
    return {
      ok: false,
      code: "NETWORK_ERROR",
      message: error instanceof Error ? error.message : String(error),
      retryable: true,
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
    code: chatStatusErrorCode(response.status),
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
}): Promise<{ page: ReferenceHistoryPage; fence: ReferenceHistoryFence }> {
  const path = `${referenceChatRoute(args.sessionId, "history")}?${referenceChatReadQuery(
    args.target,
    args.registryId,
    args.limit,
    args.cursor,
  )}`;
  let response: Response;
  try {
    response = await fetch(remoteApiUrl(args.baseUrl, path), {
      headers: { Authorization: `Bearer ${args.token}` },
      signal: args.signal,
    });
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
  // Chat is the default at EVERY width: the terminal is a mode the user asks for, never the
  // landing surface of a phone or a desktop. One owner decides the mode, so no nested drawer
  // can mount the same session twice.
  const [viewMode, setViewMode] = useState<"chat" | "terminal" | "browser">("chat");
  const [chatMessages, setChatMessages] = useState<MobileChatMessageProps[]>([]);
  const [chatWarnings, setChatWarnings] = useState<readonly string[]>([]);
  // The lane the chat speaks to: the owning target of the session in focus.

  const [chatDraftText, setChatDraftText] = useState("");
  const [chatStagedFiles, setChatStagedFiles] = useState<readonly ReferenceFileReceipt[]>([]);
  const [chatAttaching, setChatAttaching] = useState(false);
  const [chatAttachError, setChatAttachError] = useState<string | null>(null);
  const [chatHeld, setChatHeld] = useState<readonly HeldMessage[]>([]);
  const [chatHeldSendingId, setChatHeldSendingId] = useState<string | null>(null);
  const [chatOlderLoading, setChatOlderLoading] = useState(false);
  const [chatOlderFailed, setChatOlderFailed] = useState(false);
  const [chatLoadedOlder, setChatLoadedOlder] = useState(false);
  const [chatPage, setChatPage] = useState<ReferenceHistoryPage | null>(null);
  const [chatOlderCursor, setChatOlderCursor] = useState<ReferenceHistoryCursor | null>(null);
  const [chatHasOlder, setChatHasOlder] = useState(false);
  const chatLoadedOlderRef = useRef(false);
  const [chatPrompt, setChatPrompt] = useState<ReferencePrompt | null>(null);
  const [chatScreenRevision, setChatScreenRevision] = useState<string | null>(null);
  const [chatTypedAnswer, setChatTypedAnswer] = useState<ReferencePromptAnswer | null>(null);
  const [chatPromptError, setChatPromptError] = useState<string | null>(null);
  const [chatPromptRefresh, setChatPromptRefresh] = useState(0);
  /** The owning host's own identity, as its authenticated capabilities answer publishes it. */
  const [chatGateway, setChatGateway] = useState<ChatGatewayIdentity | null>(null);
  /** The provider session id the host published for THIS pane; null means unknown, never guessed. */
  const [chatProviderSession, setChatProviderSession] = useState<string | null>(null);
  const [chatComposerNotice, setChatComposerNotice] = useState<string | null>(null);
  // The fence the newest read was issued under: a response for another one is dropped.
  const chatFenceRef = useRef<ReferenceHistoryFence | null>(null);
  const chatRetentionTruncatedRef = useRef(false);

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


  const [initialAccountTarget, setInitialAccountTarget] = useState<{
    machineId: string;
    workspaceId: string;
    worktreeSlug: string | null;
    worktreeLabel: string | null;
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

    const targetMachine = accountDiscovery.machines.find((m) => m.machineId === storedTarget.machineId);
    if (!targetMachine) {
      if (accountDiscovery.initialized && !accountDiscovery.loading && !accountDiscovery.error) {
        clearAccountLastSelectedTarget(relayUrl);
      }
      return;
    }

    if (targetMachine.online === false) {
      return;
    }

    const machineStatus = accountDiscovery.machineStatuses[storedTarget.machineId];
    if (!machineStatus || machineStatus.status === "tunneling" || machineStatus.status === "idle") {
      return;
    }

    if (machineStatus.status === "error" || machineStatus.status === "offline") {
      return;
    }

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
        const res = await activeTunnelConnection.transport.fetchLike("/api/v1/workspace/state", {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (res.status === 401 || res.status === 403) {
          disconnect();
          return null;
        }
        if (res.status < 200 || res.status >= 300) return null;
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
        } catch {}
        const text = new TextDecoder().decode(res.body);
        return normalizeRemoteWorkspaceState(JSON.parse(text));
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
  }, [activeHost?.machineId, activeTunnelConnection, disconnect, token, transportBaseUrl]);

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
      const change = parseActiveSelectionEvent(text);
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
        const res = await activeTunnelConnection.transport.fetchLike("/api/v1/workspace/select", {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${token}`,
          },
          body: JSON.stringify(selectPayload),
        });
        ok = res.status >= 200 && res.status < 300;
        status = res.status;
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
  }, [activeHost?.machineId, activeTunnelConnection, armConfirmationTimeout, clearPendingSelection, confirmSelection, model, token, transportBaseUrl]);

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
    });
  }, [token, activeTunnelConnection, initialAccountTarget, selectContext]);

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
        worktreeSlug: prevTab.worktreeSlug ?? model.context.worktreeSlug,
        worktreeLabel: prevTab.worktreeLabel ?? model.context.worktreeLabel,
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
        worktreeSlug: nextTab.worktreeSlug ?? model.context.worktreeSlug,
        worktreeLabel: nextTab.worktreeLabel ?? model.context.worktreeLabel,
        tabId: nextTab.id,
        sessionId: nextTab.sessionId,
      });
    }
  }, [currentIndex, model.context.workspaceId, model.context.worktreeLabel, model.context.worktreeSlug, selectContext, tabs]);

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
  const chatAgentType =
    model.context.terminalTabs?.find((tab) => tab.id === model.context.activeTabId)?.agentType ?? null;
  const chatActivity =
    model.context.terminalTabs?.find((tab) => tab.id === model.context.activeTabId)?.activityState ?? null;
  const chatWorkspaceId = model.context.workspaceId ?? null;
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
    if (!effectiveSessionId || !chatWorkspaceId || !chatDaemonEpoch) return null;
    return {
      target: {
        // The id the host publishes for itself, else its documented default. Never the machine
        // identity, which is a different value, and never another host's name.
        hostId: chatGateway?.referenceHostId ?? REFERENCE_CHAT_DEFAULT_HOST_ID,
        ownerId: chatWorkspaceId,
        epoch: chatDaemonEpoch,
        backendSessionId: effectiveSessionId,
      },
      // Only the provider session the owning host published for this pane. Absent means unknown,
      // and the reader then binds the conversation by the host's own owner/source rules.
      ...(chatProviderSession !== null ? { providerSessionId: chatProviderSession } : {}),
    };
  }, [chatDaemonEpoch, chatGateway, chatProviderSession, chatWorkspaceId, effectiveSessionId]);
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




  // The gateway's own identity: the daemon incarnation every reference-chat request must name,
  // and the host id it publishes. Both come from the one authenticated route that answers them on
  // every transport, so the chat lane is not tied to a single connection mode.
  /** Re-read the owning host's identity: a restarted daemon answers a new incarnation. */
  const refreshChatIdentity = useCallback(async () => {
    const bearer = chatTokenRef.current;
    if (!bearer) return;
    const identity = await readChatGatewayIdentity({
      baseUrl: chatBaseUrlRef.current,
      token: bearer,
    });
    if (identity !== null) setChatGateway(identity);
  }, []);

  useEffect(() => {
    if (!token) {
      setChatGateway(null);
      return;
    }
    const controller = new AbortController();
    void readChatGatewayIdentity({ baseUrl: transportBaseUrl, token, signal: controller.signal })
      .then((identity) => {
        if (!controller.signal.aborted) setChatGateway(identity);
      })
      .catch(() => {
        /* a transient capabilities failure keeps the identity this lane already had */
      });
    return () => controller.abort();
  }, [token, transportBaseUrl]);

  // The pane's own facts, from the host's session list: its daemon incarnation and the provider
  // session the host published for it. Absence is unknown; a disagreement between rows is
  // ambiguous, and both stay unknown rather than being guessed.
  useEffect(() => {
    if (!effectiveSessionId || !token) {
      setChatProviderSession(null);
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
  }, [activeTunnelConnection, effectiveSessionId, token, transportBaseUrl]);

  /** Everything that belongs to ONE pane: another target starts from a clean lane. */
  const resetChatLane = useCallback(() => {
    setChatMessages([]);
    setChatWarnings([]);
    setChatDraftText("");
    setChatStagedFiles([]);
    setChatAttachError(null);
    setChatHeld([]);
    setChatHeldSendingId(null);
    setChatOlderLoading(false);
    setChatOlderFailed(false);
    setChatLoadedOlder(false);
    setChatPage(null);
    setChatOlderCursor(null);
    setChatHasOlder(false);
    chatLoadedOlderRef.current = false;
    setChatPrompt(null);
    setChatScreenRevision(null);
    setChatTypedAnswer(null);
    setChatPromptError(null);
    chatFenceRef.current = null;
    chatRetentionTruncatedRef.current = false;

  }, []);

  /* The draft and the held rows belong to the TARGET, not to this component instance: switching
     panes reads that target's own rows back, and never carries them across. */
  useEffect(() => {
    if (chatReferenceTarget === null) {
      setChatDraftText("");
      setChatHeld([]);
      return;
    }
    referenceDrafts.refresh(chatReferenceTarget);
    referenceQueues.refresh(chatReferenceTarget);
    setChatDraftText(referenceDrafts.read(chatReferenceTarget).text);
    setChatHeld([...referenceQueues.read(chatReferenceTarget)]);
  }, [chatTargetKey]);

  /* The newest page. A late answer for another generation is dropped, and an identity refusal
     clears the lane rather than browsing another transcript. */
  useEffect(() => {
    if (viewMode !== "chat") return;
    const target = chatReferenceTarget;
    const sessionId = effectiveSessionId;
    if (!target || !sessionId || !token) return;
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
          registryId: registryId ?? "",
          limit: CHAT_HISTORY_PAGE_LIMIT,
          signal: controller.signal,
        });
        if (cancelled) return;
        // The pane is the fence: an answer for a target that is no longer current is dropped,
        // and the generation it came from is recorded with it for the cursor check below.
        const liveTarget = chatTargetRef.current;
        if (liveTarget === null || !sameReferenceTarget(liveTarget, read.fence.target)) return;
        chatFenceRef.current = read.fence;
        setChatPage(read.page);
        if (!chatLoadedOlderRef.current) {
          setChatOlderCursor(read.page.cursor ?? null);
          setChatHasOlder(read.page.hasMore && read.page.cursor != null);
        }
        const mapped = mapReferenceTurns(read.page);
        setChatMessages((prev) => {
          const pending = prev.filter((message) => message.id.startsWith("pending-"));
          const kept = pending.filter(
            (echo) => !mapped.some((turn) => turn.role === echo.role && turn.content === echo.content),
          );
          const { messages: capped, truncated } = capRetainedMessages([...mapped, ...kept]);
          if (truncated) chatRetentionTruncatedRef.current = true;
          return capped;
        });
        setChatWarnings((prev) => {
          const cleared = prev.filter(
            (warning) =>
              warning !== CHAT_REFRESH_WARNING &&
              warning !== CHAT_UNAVAILABLE_WARNING &&
              warning !== CHAT_RETENTION_WARNING,
          );
          const disclosed = referenceHistoryNotice(read.page);
          const next = disclosed === null ? cleared : withChatWarning(cleared, disclosed);
          return chatRetentionTruncatedRef.current
            ? withChatWarning(next, CHAT_RETENTION_WARNING)
            : next;
        });
      } catch (error) {
        if (cancelled) return;
        if (error instanceof ReferenceHistoryFetchError && referenceHistoryErrorIsIdentity(error.code)) {
          // A refusal is an answer, not permission to look somewhere else. A stale incarnation is
          // the one refusal that can be repaired: re-read the host's identity and let the next
          // poll speak to the incarnation that is live now. Everything else clears the lane.
          if (error.code === "TARGET_EXPIRED" || error.code === "FORBIDDEN") {
            await refreshChatIdentity();
          }
          resetChatLane();
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
    return () => {
      cancelled = true;
      controller.abort();
      clearInterval(timer);
    };
  }, [chatAgentType, chatTargetKey, effectiveSessionId, refreshChatIdentity, resetChatLane, token, transportBaseUrl, viewMode]);

  /* The prompt the original pane waits on. A pane whose reader the reference does not have keeps
     no card: an unknown menu is answered in the terminal, never guessed from here. */
  useEffect(() => {
    if (viewMode !== "chat") return;
    const target = chatReferenceTarget;
    const sessionId = effectiveSessionId;
    if (!target || !sessionId || !token) return;
    if (!chatAgentType || !chatAgentIsNative) {
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
        const response = await fetch(remoteApiUrl(transportBaseUrl, path), {
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
        setChatPrompt(prompt);
        setChatScreenRevision(prompt === null ? null : parseChatScreenRevision(body));
        if (prompt === null) {
          setChatTypedAnswer(null);
          setChatPromptError(null);
        }
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
      });
      // A cursor only names a position while its stream is still the live one: a foreign one is
      // dropped rather than re-anchored, and the control then says there is no more to reach.
      const streamId = chatFenceRef.current?.target.target.backendSessionId;
      const nextCursor =
        read.page.cursor == null || streamId === undefined
          ? null
          : referenceHistoryCursorForStream(read.page.cursor, streamId);
      chatLoadedOlderRef.current = true;
      setChatLoadedOlder(true);
      setChatOlderCursor(nextCursor);
      setChatHasOlder(read.page.hasMore && nextCursor !== null);
      const mapped = mapReferenceTurns(read.page);
      setChatMessages((prev) => [...mapped, ...prev]);
    } catch {
      setChatOlderFailed(true);
      setChatWarnings((prev) => withChatWarning(prev, CHAT_OLDER_FAILED_WARNING));
    } finally {
      setChatOlderLoading(false);
    }
  }, [chatOlderCursor]);

  /** One mutation, as the frozen envelope carries it: target, request id, payload. */
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
      else setChatHeldSendingId(heldId);
      const pending: MobileChatMessageProps = {
        id: `pending-${newChatRequestId()}`,
        role: "user",
        content: text,
        timestamp: Date.now(),
      };
      setChatMessages((prev) => [...prev, pending]);
      const payload: ReferenceSubmitPayload = { text, attachmentIds: [], origin: "chat" };
      const outcome = await postChatMutation(
        chatBaseUrlRef.current,
        sessionId,
        bearer,
        "submit",
        chatMutationBody(target, payload),
      );
      if (outcome.ok) {
        if (heldId === undefined) {
          // the acknowledgement settles the SENT PREFIX only: an edit made while it was in
          // flight survives, and so does everything typed after it
          const settled = referenceDrafts.settle(target, text);
          referenceDrafts.end(target);
          setChatDraftText(settled.text);
        } else {
          referenceQueues.remove(target, heldId);
          setChatHeld([...referenceQueues.read(target)]);
          setChatHeldSendingId(null);
        }
        setChatComposerNotice(null);
        return true;
      }
      // Nothing was written: the echo stops claiming it was sent, and the text comes back.
      setChatComposerNotice(
        referenceIsOutcomeUnknown(outcome.code)
          ? CHAT_OUTCOME_UNKNOWN_WARNING
          : CHAT_SEND_FAILED_WARNING,
      );
      setChatMessages((prev) => prev.filter((message) => message.id !== pending.id));
      if (heldId === undefined) {
        const current = referenceDrafts.read(target).text;
        if (current.length === 0) referenceDrafts.set(target, text);
        setChatDraftText(referenceDrafts.read(target).text);
      } else {
        setChatHeldSendingId(null);
      }
      setChatWarnings((prev) =>
        withChatWarning(
          prev,
          referenceIsOutcomeUnknown(outcome.code) ? CHAT_OUTCOME_UNKNOWN_WARNING : CHAT_SEND_FAILED_WARNING,
        ),
      );
      return false;
    },
    [chatMutationBody],
  );

  /**
   * Stop the pane's own turn. `refused` is an answer, not a reason to send a killing signal: an
   * unknown capability sends nothing and says so, and the explicit terminal keeps Ctrl-C.
   */
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
    const payload: ReferenceStopPayload = { capability };
    const outcome = await postChatMutation(
      chatBaseUrlRef.current,
      sessionId,
      bearer,
      "stop",
      chatMutationBody(target, payload),
    );
    if (!outcome.ok) {
      setChatWarnings((prev) => withChatWarning(prev, CHAT_STOP_FAILED_WARNING));
    }
  }, [chatMutationBody]);

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
      const outcome = await postChatMutation(
        chatBaseUrlRef.current,
        sessionId,
        bearer,
        "answer",
        chatMutationBody(target, payload),
      );
      if (!outcome.ok) {
        setChatPromptError(outcome.message);
        throw new ReferencePromptRefused(outcome.code, outcome.message);
      }
      setChatTypedAnswer(null);
      setChatPromptError(null);
      setChatPrompt(null);
      setChatScreenRevision(null);
      setChatPromptRefresh((generation) => generation + 1);
      return true;
    },
    [chatMutationBody],
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
          const response = await fetch(
            remoteApiUrl(
              chatBaseUrlRef.current,
              `${referenceChatRoute(sessionId, `files/${encodeURIComponent(attachmentId)}`)}?${query}`,
            ),
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
          const response = await fetch(
            remoteApiUrl(
              chatBaseUrlRef.current,
              `${referenceChatRoute(sessionId, `files/${encodeURIComponent(attachmentId)}`)}?${query}`,
            ),
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
      const staged: ReferenceFileReceipt[] = [];
      let failure: string | null = null;
      const controller = new AbortController();
      for (const file of files) {
        try {
          const used = chatStagedFiles.reduce((sum, entry) => sum + entry.receipt.sizeBytes, 0);
          const receipt = await stageReferenceChatFile(
            { transport: chatFileTransport },
            target,
            file,
            controller.signal,
            { fileCount: chatStagedFiles.length + staged.length, turnBytes: used },
          );
          staged.push(receipt);
        } catch (error) {
          failure = error instanceof Error ? error.message : String(error);
          break;
        }
      }
      setChatAttaching(false);
      if (failure !== null) setChatAttachError(failure);
      if (staged.length === 0) return;
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
    [chatFileTransport, chatStagedFiles],
  );

  /** Delete a staged file. Explicit, and the only path that removes one. */
  const removeChatStagedFile = useCallback(
    async (attachmentId: string) => {
      const target = chatTargetRef.current;
      if (!target) return;
      const entry = chatStagedFiles.find((file) => file.receipt.attachmentId === attachmentId);
      if (!entry) return;
      const cleaned = await cancelReferenceChatFile({ transport: chatFileTransport }, target, entry);
      if (!cleaned.cleaned) {
        setChatAttachError("The host did not confirm that the staged file was removed.");
        return;
      }
      setChatStagedFiles((prev) =>
        prev.filter((file) => file.receipt.attachmentId !== attachmentId),
      );
      const path = referenceMentionPathOf(entry.mentionText);
      if (path === null) return;
      const mention = referenceMentionFor(path);
      const current = referenceDrafts.read(target).text;
      const next = current.split(mention).join("");
      if (next !== current) {
        referenceDrafts.set(target, next);
        setChatDraftText(next);
      }
    },
    [chatFileTransport, chatStagedFiles],
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
        void answerChatPrompt({
          promptId: chatPrompt.id,
          screenRevision: revision,
          answer: typed,
        }).catch(() => {
          /* the card reports the refusal it was given */
        });
        return true;
      }
      void sendChatMessage(text);
      return true;
    },
    [answerChatPrompt, chatPrompt, chatScreenRevision, holdChatMessage, sendChatMessage],
  );

  const chatPartContext = useMemo<ReferencePartRenderContext>(
    () => ({
      resolveImageUrl: (image: ReferenceImageRef) =>
        referenceChatRoute(chatSessionIdRef.current ?? "", `files/${encodeURIComponent(image.ref)}`),
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
        onAnswered={() => setChatTypedAnswer(null)}
        onTypedAnswerDone={() => setChatTypedAnswer(null)}
      />
    );
  }, [answerChatPrompt, chatPrompt, chatPromptError, chatScreenRevision, chatTypedAnswer]);

  /** What the message box says while a prompt waits, so the user knows what typing does. */
  const chatComposerPlaceholder =
    chatPrompt !== null ? referenceAnswerHint(chatPrompt) : undefined;



  /* Declared above the auth early return so the hook count is identical on the
     login screen and after pairing; a hook below the guard changes the order. */
  const isAccountMode = Boolean(accountSessionToken);
  const effectiveModel: RemoteWorkspaceModel = isAccountMode && accountDiscovery.accountOptions.length > 0
    ? {
        ...model,
        options: accountDiscovery.accountOptions,
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
  const activeMachineId = activeTunnelConnection?.machine.machineId ?? null;

  const accountPickerStatus = isAccountMode ? (
    <div data-testid="remote-account-inventory-status" className="space-y-0.5 pb-1">
      {accountDiscovery.loading ? (
        <p className="px-2 py-1 text-[11px] text-muted-foreground">Loading machines...</p>
      ) : null}
      {accountDiscovery.error ? (
        <p role="alert" className="px-2 py-1 text-[11px] text-destructive">{accountDiscovery.error}</p>
      ) : null}
      {accountDiscovery.machines.map((machine) => {
        const status = accountDiscovery.machineStatuses[machine.machineId];
        if (!status || status.status === "ready" || status.status === "idle") return null;
        const name = machine.displayName || machine.machineId;
        return (
          <div
            key={machine.machineId}
            data-testid={`remote-account-machine-status-${machine.machineId}`}
            data-status={status.status}
            className="flex min-h-[24px] items-center gap-1.5 px-2 text-[11px] text-muted-foreground"
          >
            <span className="min-w-0 flex-1 truncate">
              {name}
              {status.status === "tunneling"
                ? " - connecting..."
                : status.status === "offline"
                ? " - offline"
                : ` - ${status.error ?? "unavailable"}`}
            </span>
            {status.status === "error" ? (
              <button
                type="button"
                aria-label={`Retry ${name}`}
                onClick={() => void accountDiscovery.retryMachine(machine.machineId)}
                className="shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium text-worktree-sidebar-foreground hover:bg-worktree-sidebar-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                Retry
              </button>
            ) : null}
          </div>
        );
      })}
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
    setCreationError(null);

    if (activeTunnelConnection && token && activeMachineId === accountOpt.machineId) {
      // Same machine: keep the connection, gate the body until the exact target confirms.
      initialAccountSelectionAttemptedRef.current = false;
      setInitialAccountTarget(target);
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
      if (conn) {
        try {
          conn.close();
        } catch {}
      }
      return false;
    }

    if (!conn) {
      setCreationError(`Failed to establish secure tunnel to ${accountOpt.machineDisplayName || accountOpt.machineId}`);
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
    setInitialAccountTarget(target);
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

      <RemoteWorkspaceMirror
        model={effectiveModel}
        pending={pending}
        selectorOpen={selectorOpen}
        onSelectorOpenChange={setSelectorOpen}
        onOpenHosts={() => setHostDrawerOpen(true)}
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
          <div data-testid="remote-account-empty-body" className="flex-1" />
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
                workspaceLabel={model.context.workspaceId ?? undefined}
                worktreeLabel={model.context.worktreeLabel ?? undefined}
                onBack={() => setSelectorOpen(true)}
                messages={chatMessages}
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
                heldSendingId={chatHeldSendingId}
                heldUnsaved={chatHeldUnsaved}
                onEditHeld={editChatHeld}
                onSendHeld={(id) => {
                  const held = chatHeld.find((message) => message.id === id);
                  if (held) void sendChatMessage(held.text, id);
                }}
                onRemoveHeld={removeChatHeld}
                promptCard={chatPromptCard}
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
              createWebSocket={
                activeTunnelConnection
                  ? async (path) => {
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
                    }
                  : undefined
              }
            />
          ) : null}
      </RemoteWorkspaceMirror>

      <MobileHostDrawer
        open={hostDrawerOpen}
        onOpenChange={setHostDrawerOpen}
        onDisconnect={disconnect}
        onSignOut={handleSignOut}
        // Account mode keeps sign-out reachable before a worktree tunnel exists.
        isAccountSession={isAccountMode}
      />
    </div>
  );
};
