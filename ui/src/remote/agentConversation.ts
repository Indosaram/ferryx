import { remoteApiUrl } from "./remoteClient";
import type { MobileChatMessageProps } from "./chat/MobileChatMessage";
import type { ChatWorkItem, ToolCallCardProps, ToolStatus } from "./chat/MobileChatComponents";
import {
  referenceChatRoute,
  referenceCursorMatches,
  referenceHistoryDisclosure,
  referenceHistoryIsNative,
  referenceTargetKey,
  sameReferenceTarget,
  type ReferenceHistoryAvailability,
  type ReferenceHistoryCursor,
  type ReferenceHistoryPage,
  type ReferenceHistorySource,
  type ReferencePart,
  type ReferencePartKind,
  type ReferenceTargetRef,
  type ReferenceTextPhase,
  type ReferenceTurn,
  type ReferenceTurnRole,
  type ReferenceTurnSource,
} from "./chat/referenceTypes";
import type { TunnelResponse, TunnelTransport } from "./attachTunnel";

/** A tool call made by an assistant record; its result arrives as a later `toolResult` record. */
export interface ConversationToolCall {
  id?: string | null;
  name: string;
  summary?: string | null;
  input?: string | null;
}

export interface ConversationMessage {
  ordinal: number;
  role: string;
  text: string;
  id?: string | null;
  timestamp?: number | string | null;
  toolName?: string | null;
  command?: string | null;
  status?: string | null;
  durationMs?: number | null;
  thinking?: string | null;
  toolCalls?: ConversationToolCall[];
  toolCallId?: string | null;
  isError?: boolean | null;
}

function isToolCard(item: ChatWorkItem): item is ToolCallCardProps {
  return item.kind !== "thinking";
}

export function parseLegacyToolMarkers(rawText: string): { cleanedProse: string; toolNames: string[] } {
  const normalized = rawText.replace(/\r\n/g, "\n");
  const lines = normalized.split("\n");
  let inFence = false;
  const keptLines: string[] = [];
  const toolNames: string[] = [];
  const markerRegex = /^→ ([A-Za-z0-9_.:-]+)[ \t]*$/;

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith("```")) {
      if (trimmed.length > 3 && trimmed.slice(3).includes("```")) {
        keptLines.push(line);
        continue;
      }
      inFence = !inFence;
      keptLines.push(line);
      continue;
    }
    if (inFence) {
      keptLines.push(line);
      continue;
    }
    const match = markerRegex.exec(line);
    if (match) {
      toolNames.push(match[1]);
    } else {
      keptLines.push(line);
    }
  }

  return {
    cleanedProse: keptLines.join("\n").trim(),
    toolNames,
  };
}

function cleanAssistantProse(item: ConversationMessage): string {
  return parseLegacyToolMarkers(item.text ?? "").cleanedProse;
}

function parseToolResultOutput(text: string): string | undefined {
  // Emitted by the daemon for empty tool output: src-tauri/src/agent_transcript.rs format!("← {tool} result").
  if (/^←\s+[A-Za-z0-9_.:-]+\s+result$/.test(text.trim())) {
    return undefined;
  }
  return text;
}

export function formatWorkedDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) return seconds === 0 ? `${minutes}m` : `${minutes}m ${seconds}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function isToolRole(role: string): boolean {
  const normalized = role.toLowerCase().replace(/[-_]/g, "");
  return (
    normalized === "toolresult" ||
    normalized === "tooluse" ||
    normalized === "toolcall" ||
    normalized === "tool"
  );
}

export function extractToolName(item: ConversationMessage): string {
  if (item.toolName) return item.toolName;
  if (item.id) {
    const stripped = item.id.replace(/^(?:call_|tool_|tool-)/, "");
    const match = /^([a-zA-Z0-9_-]+?)(?:[-_]\d+)?$/.exec(stripped);
    if (match && match[1] && !/^\d+$/.test(match[1])) {
      return match[1];
    }
  }
  return "tool";
}

export function parseTimestamp(ts: unknown): number | null {
  if (typeof ts === "number") {
    return ts < 1e11 ? ts * 1000 : ts;
  }
  if (typeof ts === "string") {
    const parsed = Date.parse(ts);
    if (!Number.isNaN(parsed)) return parsed;
    const num = Number(ts);
    if (!Number.isNaN(num)) return num < 1e11 ? num * 1000 : num;
  }
  return null;
}

export interface MapConversationOptions {
  activeTurnStartedAt?: number | null;
  previousMessages?: MobileChatMessageProps[];
  turnDurationsMap?: Map<string, string>;
}

export function mapAgentConversation(
  items: ConversationMessage[],
  options?: MapConversationOptions,
): MobileChatMessageProps[] {
  const sorted = [...items].sort((a, b) => a.ordinal - b.ordinal);
  const mapped: MobileChatMessageProps[] = [];

  const prevById = new Map<string, MobileChatMessageProps>();
  if (options?.previousMessages) {
    for (const msg of options.previousMessages) {
      prevById.set(msg.id, msg);
    }
  }

  function resolveDuration(
    assistantId: string,
    toolCalls: ChatWorkItem[],
    timestamps: number[],
    isLastSpan: boolean,
    promptTimestamp?: number | null,
  ): string {
    const turnActive = isLastSpan && options?.activeTurnStartedAt != null;
    if (!turnActive) {
      if (options?.turnDurationsMap?.has(assistantId)) {
        return options.turnDurationsMap.get(assistantId)!;
      }
      if (!isLastSpan) {
        const prevMsg = prevById.get(assistantId);
        if (prevMsg?.durationLabel) {
          return prevMsg.durationLabel;
        }
      }
    }
    if (isLastSpan && options?.activeTurnStartedAt != null) {
      const promptMs =
        promptTimestamp !== null && promptTimestamp !== undefined
          ? promptTimestamp
          : null;
      const activeMs =
        parseTimestamp(options.activeTurnStartedAt) ?? options.activeTurnStartedAt;
      const startMs = promptMs !== null ? promptMs : activeMs;
      return formatWorkedDuration(Math.max(0, Date.now() - startMs));
    }
    let label: string;
    const sumMs = toolCalls.filter(isToolCard).reduce((acc, tc) => acc + (tc.durationMs ?? 0), 0);
    if (timestamps.length >= 2) {
      const min = Math.min(...timestamps);
      const max = Math.max(...timestamps);
      const diff = Math.max(0, max - min);
      label = formatWorkedDuration(diff);
    } else if (sumMs > 0) {
      label = formatWorkedDuration(sumMs);
    } else {
      label = formatWorkedDuration(0);
    }
    // Idle last span: honor the label finalize settled, but never cache a computed one (more records may still arrive).
    if (!isLastSpan) {
      options?.turnDurationsMap?.set(assistantId, label);
    }
    return label;
  }

  type SpanGroup = {
    span: ConversationMessage[];
    isLastSpan: boolean;
    userItem?: ConversationMessage;
  };

  const groups: SpanGroup[] = [];
  let currentSpan: ConversationMessage[] = [];

  for (const item of sorted) {
    if (item.role === "user") {
      groups.push({
        span: currentSpan,
        isLastSpan: false,
        userItem: item,
      });
      currentSpan = [];
    } else {
      currentSpan.push(item);
    }
  }
  groups.push({
    span: currentSpan,
    isLastSpan: true,
  });

  function processSpan(
    span: ConversationMessage[],
    isLastSpan: boolean,
    promptTimestamp?: number | null,
  ) {
    if (span.length === 0) return;
    const turnActive = isLastSpan && options?.activeTurnStartedAt != null;

    const spanTimestamps: number[] = [];
    if (promptTimestamp !== null && promptTimestamp !== undefined) {
      spanTimestamps.push(promptTimestamp);
    }
    for (const item of span) {
      const ts = parseTimestamp(item.timestamp);
      if (ts !== null) spanTimestamps.push(ts);
    }

    let lastProseIndex = -1;
    for (let i = span.length - 1; i >= 0; i--) {
      const item = span[i];
      if (item.role === "assistant" && cleanAssistantProse(item).length > 0) {
        lastProseIndex = i;
        break;
      }
    }

    const workList: ChatWorkItem[] = [];
    const openCalls = new Map<string, ToolCallCardProps>();
    const openCardsWithoutId: ToolCallCardProps[] = [];
    const nonTurnItems: { ordinal: number; message: MobileChatMessageProps }[] = [];

    for (let i = 0; i < span.length; i++) {
      const item = span[i];

      if (item.role !== "assistant" && !isToolRole(item.role)) {
        nonTurnItems.push({
          ordinal: item.ordinal,
          message: {
            id: `system-${item.ordinal}`,
            role: "system",
            content: item.text,
            timestamp: parseTimestamp(item.timestamp) ?? Date.now(),
          },
        });
        continue;
      }

      if (isToolRole(item.role)) {
        let matched: ToolCallCardProps | undefined;
        if (item.toolCallId) {
          if (openCalls.has(item.toolCallId)) {
            matched = openCalls.get(item.toolCallId);
            openCalls.delete(item.toolCallId);
          } else if (openCalls.size > 0) {
            const firstKey = openCalls.keys().next().value;
            if (firstKey !== undefined) {
              matched = openCalls.get(firstKey);
              openCalls.delete(firstKey);
            }
          } else if (openCardsWithoutId.length > 0) {
            matched = openCardsWithoutId.shift();
          }
        } else {
          if (openCardsWithoutId.length > 0) {
            matched = openCardsWithoutId.shift();
          } else if (openCalls.size > 0) {
            const firstKey = openCalls.keys().next().value;
            if (firstKey !== undefined) {
              matched = openCalls.get(firstKey);
              openCalls.delete(firstKey);
            }
          }
        }

        if (matched) {
          matched.output = parseToolResultOutput(item.text);
          matched.status = item.isError ? "error" : ((item.status as ToolStatus) || "success");
          matched.durationMs = item.durationMs ?? matched.durationMs;
          continue;
        }

        workList.push({
          workKey: `result-${item.ordinal}`,
          toolName: item.toolName || extractToolName(item),
          command: item.command || undefined,
          output: parseToolResultOutput(item.text),
          status: item.isError ? "error" : (item.status as ToolStatus) || "success",
          durationMs: item.durationMs ?? undefined,
        });
        continue;
      }

      if (item.role === "assistant") {
        if (item.thinking && item.thinking.trim().length > 0) {
          workList.push({ workKey: `thinking-${item.ordinal}`, kind: "thinking", text: item.thinking });
        }

        const cleanProse = cleanAssistantProse(item);
        if (i !== lastProseIndex && cleanProse.length > 0) {
          workList.push({ workKey: `prose-${item.ordinal}`, kind: "thinking", text: cleanProse, source: "prose" });
        }

        if (!item.toolCalls?.length) {
          const { toolNames } = parseLegacyToolMarkers(item.text ?? "");
          for (let k = 0; k < toolNames.length; k++) {
            const toolName = toolNames[k];
            const card: ToolCallCardProps = {
              workKey: `tool-${item.ordinal}-${k}`,
              toolName,
              status: "running",
            };
            openCardsWithoutId.push(card);
            workList.push(card);
          }
        } else {
          for (let k = 0; k < item.toolCalls.length; k++) {
            const call = item.toolCalls[k];
            const card: ToolCallCardProps = {
              workKey: call.id ? `tool-${call.id}` : `tool-${item.ordinal}-${k}`,
              toolName: call.name,
              summary: call.summary || undefined,
              command: call.input || undefined,
              status: "running",
            };
            if (call.id) {
              openCalls.set(call.id, card);
            } else {
              openCardsWithoutId.push(card);
            }
            workList.push(card);
          }
        }
        continue;
      }


    }

    if (!turnActive) {
      for (const card of openCardsWithoutId) {
        if (card.status === "running") {
          card.status = "success";
        }
      }
      for (const card of openCalls.values()) {
        if (card.status === "running") {
          card.status = "success";
        }
      }
    }

    let turnMessage: MobileChatMessageProps | undefined;

    if (lastProseIndex !== -1) {
      const proseRecord = span[lastProseIndex];
      const assistantId = `assistant-${span[0].ordinal}`;
      const content = cleanAssistantProse(proseRecord);
      const hasTools = workList.length > 0;
      const durationLabel = hasTools
        ? resolveDuration(assistantId, workList, spanTimestamps, isLastSpan, promptTimestamp)
        : undefined;

      turnMessage = {
        id: assistantId,
        role: "assistant",
        content,
        ...(hasTools ? { toolCalls: workList, durationLabel } : {}),
        timestamp: parseTimestamp(proseRecord.timestamp) ?? spanTimestamps[spanTimestamps.length - 1] ?? Date.now(),
      };
    } else if (workList.length > 0) {
      const assistantId = `assistant-${span[0].ordinal}`;
      const durationLabel = resolveDuration(assistantId, workList, spanTimestamps, isLastSpan, promptTimestamp);
      turnMessage = {
        id: assistantId,
        role: "assistant",
        content: "",
        toolCalls: workList,
        durationLabel,
        timestamp: spanTimestamps[spanTimestamps.length - 1] ?? parseTimestamp(span[0].timestamp) ?? Date.now(),
      };
    }

    if (turnMessage) {
      const turnOrderOrdinal =
        lastProseIndex !== -1 ? span[lastProseIndex].ordinal : span[0].ordinal;
      let inserted = false;
      for (const nonTurn of nonTurnItems) {
        if (!inserted && nonTurn.ordinal > turnOrderOrdinal) {
          mapped.push(turnMessage);
          inserted = true;
        }
        mapped.push(nonTurn.message);
      }
      if (!inserted) {
        mapped.push(turnMessage);
      }
    } else {
      for (const nonTurn of nonTurnItems) {
        mapped.push(nonTurn.message);
      }
    }
  }

  for (let g = 0; g < groups.length; g++) {
    const group = groups[g];
    const prevUserItem = g > 0 ? groups[g - 1].userItem : undefined;
    const promptTimestamp = prevUserItem ? parseTimestamp(prevUserItem.timestamp) : null;
    if (group.span.length > 0) {
      processSpan(group.span, group.isLastSpan, promptTimestamp);
    }
    if (group.userItem) {
      mapped.push({
        id: `user-${group.userItem.ordinal}`,
        role: "user",
        content: group.userItem.text,
        timestamp: parseTimestamp(group.userItem.timestamp) ?? Date.now(),
      });
    }
  }

  return mapped;
}

export interface ConversationPage {
  sessionId: string;
  items: ConversationMessage[];
  nextCursor: number | null;
  partial: boolean;
  warnings: string[];
}

export class ConversationFetchError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "ConversationFetchError";
    this.code = code;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function toMessage(raw: unknown): ConversationMessage {
  const entry = isRecord(raw) ? raw : {};
  const rawId = entry.id;
  let id: string | null | undefined;
  if (rawId === undefined) {
    id = undefined;
  } else if (typeof rawId === "string") {
    id = rawId;
  } else {
    id = null;
  }
  const timestamp =
    typeof entry.timestamp === "number" || typeof entry.timestamp === "string"
      ? entry.timestamp
      : undefined;
  return {
    ordinal: Number(entry.ordinal),
    role: String(entry.role),
    text: String(entry.text),
    ...(id !== undefined ? { id } : {}),
    ...(timestamp !== undefined ? { timestamp } : {}),
    ...(typeof entry.toolName === "string" ? { toolName: entry.toolName } : {}),
    ...(typeof entry.command === "string" ? { command: entry.command } : {}),
    ...(typeof entry.status === "string" ? { status: entry.status } : {}),
    ...(typeof entry.durationMs === "number" ? { durationMs: entry.durationMs } : {}),
    ...(typeof entry.thinking === "string" ? { thinking: entry.thinking } : {}),
    ...(Array.isArray(entry.toolCalls) ? { toolCalls: entry.toolCalls.filter(isRecord).map(toToolCall) } : {}),
    ...(typeof entry.toolCallId === "string" ? { toolCallId: entry.toolCallId } : {}),
    ...(typeof entry.isError === "boolean" ? { isError: entry.isError } : {}),
  };
}

function toToolCall(entry: Record<string, unknown>): ConversationToolCall {
  return {
    name: typeof entry.name === "string" ? entry.name : "tool",
    ...(typeof entry.id === "string" ? { id: entry.id } : {}),
    ...(typeof entry.summary === "string" ? { summary: entry.summary } : {}),
    ...(typeof entry.input === "string" ? { input: entry.input } : {}),
  };
}

function toPage(body: unknown): ConversationPage {
  if (!isRecord(body)) {
    throw new ConversationFetchError("MALFORMED_RESPONSE", "Agent history response body is not an object");
  }
  if (!Array.isArray(body.items)) {
    throw new ConversationFetchError("MALFORMED_RESPONSE", "Agent history response items is not an array");
  }
  if (typeof body.sessionId !== "string") {
    throw new ConversationFetchError("MALFORMED_RESPONSE", "Agent history response sessionId is not a string");
  }
  if (body.nextCursor !== null && typeof body.nextCursor !== "number") {
    throw new ConversationFetchError("MALFORMED_RESPONSE", "Agent history response nextCursor is not a number or null");
  }
  if (typeof body.partial !== "boolean") {
    throw new ConversationFetchError("MALFORMED_RESPONSE", "Agent history response partial is not a boolean");
  }
  if (!Array.isArray(body.warnings) || !body.warnings.every((warning) => typeof warning === "string")) {
    throw new ConversationFetchError("MALFORMED_RESPONSE", "Agent history response warnings is not a string array");
  }
  return {
    sessionId: body.sessionId,
    items: body.items.map(toMessage),
    nextCursor: body.nextCursor,
    partial: body.partial,
    warnings: body.warnings,
  };
}

export interface FetchAgentConversationArgs {
  baseUrl: string;
  sessionId: string;
  token: string;
  limit?: number;
  cursor?: number | null;
  signal?: AbortSignal;
  transport?: Pick<TunnelTransport, "fetchLike"> | null;
}

export async function fetchAgentConversation(args: FetchAgentConversationArgs): Promise<ConversationPage> {
  const { baseUrl, sessionId, token, cursor, signal, transport } = args;
  const limit = args.limit ?? 200;
  let path = `/api/v1/agent-history/${encodeURIComponent(sessionId)}?limit=${limit}`;
  if (typeof cursor === "number") {
    path += `&cursor=${cursor}`;
  }

  let response: Response;
  if (transport) {
    signal?.throwIfAborted();
    let raw: TunnelResponse;
    try {
      raw = await transport.fetchLike(path, {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      });
    } catch (error) {
      if (signal?.aborted) throw error;
      throw new ConversationFetchError("NETWORK_ERROR", error instanceof Error ? error.message : String(error));
    }
    signal?.throwIfAborted();
    response = new Response(raw.body.slice().buffer, {
      status: raw.status,
      headers: raw.headers,
    });
  } else {
    try {
      response = await fetch(remoteApiUrl(baseUrl, path), {
        headers: { Authorization: `Bearer ${token}` },
        signal,
      });
    } catch (error) {
      if (signal?.aborted) throw error;
      throw new ConversationFetchError("NETWORK_ERROR", error instanceof Error ? error.message : String(error));
    }
  }
  if (response.ok) {
    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new ConversationFetchError("MALFORMED_RESPONSE", "Agent history response body is not valid JSON");
    }
    return toPage(body);
  }
  let errorCode: string | null = null;
  try {
    const text = await response.text();
    try {
      const parsed: unknown = JSON.parse(text);
      if (isRecord(parsed) && typeof parsed.error === "string") {
        errorCode = parsed.error;
      }
    } catch {}
  } catch {}
  if (errorCode !== null) {
    throw new ConversationFetchError(errorCode, `Agent history request failed with status ${response.status}`);
  }
  if (response.status === 404) {
    throw new ConversationFetchError("TRANSCRIPT_NOT_FOUND", `Agent history transcript not found for session ${sessionId}`);
  }
  if (response.status === 400) {
    throw new ConversationFetchError("INVALID_SESSION_ID", `Invalid agent history session id ${sessionId}`);
  }
  throw new ConversationFetchError("REQUEST_FAILED", `Agent history request failed with status ${response.status}`);
}

export const MAX_RETAINED_CHAT_MESSAGES = 400;

export function capRetainedMessages(
  messages: readonly MobileChatMessageProps[],
  max: number = MAX_RETAINED_CHAT_MESSAGES,
): { messages: MobileChatMessageProps[]; truncated: boolean } {
  if (messages.length <= max) {
    return { messages: [...messages], truncated: false };
  }
  return {
    messages: messages.slice(messages.length - max),
    truncated: true,
  };
}

// ---------------------------------------------------------------------------
// Reference native history (plan task 4)
// ---------------------------------------------------------------------------
//
// The legacy `/api/v1/agent-history` path above keeps its existing callers and wire shape
// untouched. Everything below is the explicit reference-chat path
// (`docs/chat/herdr-port-contract.md` sections 2 and 4): it carries the frozen rich page —
// source, availability, rich parts, cursor and generation — and fences every response
// against the pane the caller actually asked for.
//
// Two outcomes must never be conflated:
//   * an *unavailable* source (`scrollback`, `notStarted`) is a legitimate page the UI
//     discloses; it is not an error and never a reason to read another transcript;
//   * an auth, identity-mismatch or ambiguous-owner failure is a typed error.

/** Default page size for a reference history read, matching the legacy client. */
export const REFERENCE_HISTORY_PAGE_LIMIT = 200;

/**
 * A reference history read failed. `code` is the wire `ScopeErrorCode` where the server
 * answered with one, or a transport/parse code from this module.
 */
export class ReferenceHistoryFetchError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "ReferenceHistoryFetchError";
    this.code = code;
  }
}

/**
 * Codes that mean *you may not read this transcript*. They are errors, never a fallback: an
 * unavailable source arrives as a page with a disclosure instead.
 */
const REFERENCE_HISTORY_IDENTITY_ERRORS: readonly string[] = [
  "UNAUTHORIZED",
  "FORBIDDEN",
  "NOT_FOUND",
  "TARGET_EXPIRED",
  "REQUEST_CONFLICT",
  "INVENTORY_INCOMPLETE",
];

/** Is this failure an auth / identity / ambiguity refusal rather than an unavailable source? */
export function referenceHistoryErrorIsIdentity(code: string): boolean {
  return REFERENCE_HISTORY_IDENTITY_ERRORS.includes(code);
}

/**
 * What the caller asked for: the session, the pane identity, and the generation the answer
 * must carry. A response failing this fence is discarded, never painted.
 */
export interface ReferenceHistoryFence {
  readonly sessionId: string;
  readonly target: ReferenceTargetRef;
  readonly generation: string;
}

export function referenceHistoryFence(
  sessionId: string,
  target: ReferenceTargetRef,
  generation: string,
): ReferenceHistoryFence {
  return { sessionId, target, generation };
}

/** A stable string form of the fence, for logs and request bookkeeping. */
export function referenceHistoryFenceKey(fence: ReferenceHistoryFence): string {
  return `${fence.sessionId}#${referenceTargetKey(fence.target)}#${fence.generation}`;
}

/**
 * Is the response still the answer to the request? Session, pane identity (host, owner,
 * epoch, backend session) and generation must all still match.
 */
export function isReferenceHistoryCurrent(
  expected: ReferenceHistoryFence,
  current: ReferenceHistoryFence,
): boolean {
  if (expected.sessionId !== current.sessionId) return false;
  if (expected.generation !== current.generation) return false;
  return sameReferenceTarget(expected.target, current.target);
}

export type ReferenceHistoryOutcome =
  | { readonly kind: "current"; readonly page: ReferenceHistoryPage }
  | { readonly kind: "stale" };

/** Fence a completed read against the request: a late foreign generation is dropped. */
export function fenceReferenceHistory(
  expected: ReferenceHistoryFence,
  response: { readonly page: ReferenceHistoryPage; readonly fence: ReferenceHistoryFence },
): ReferenceHistoryOutcome {
  return isReferenceHistoryCurrent(expected, response.fence)
    ? { kind: "current", page: response.page }
    : { kind: "stale" };
}

/**
 * A cursor is only usable while it still names the live stream. A cursor minted against
 * another stream is dropped here rather than re-anchored by the client.
 */
export function referenceHistoryCursorForStream(
  cursor: ReferenceHistoryCursor,
  liveStreamId: string,
): ReferenceHistoryCursor | null {
  return referenceCursorMatches(cursor, liveStreamId) ? cursor : null;
}

/** How the UI treats a page: paint it as native, or paint it with a disclosure. */
export type ReferenceHistoryDisposition = "render" | "disclose";

export function referenceHistoryDisposition(
  page: ReferenceHistoryPage,
): ReferenceHistoryDisposition {
  return referenceHistoryIsNative(page) ? "render" : "disclose";
}

/** The disclosure text for a non-native page; `null` when nothing needs disclosing. */
export function referenceHistoryNotice(page: ReferenceHistoryPage): string | null {
  return referenceHistoryDisclosure(page);
}

export interface ReferenceHistoryQueryArgs {
  readonly target: ReferenceTargetRef;
  readonly limit?: number;
  readonly cursor?: ReferenceHistoryCursor | null;
}

/**
 * The query string binding a read to its target. `epoch` and `providerSessionId` travel with
 * the request; the provider session is sent only where a reader identified one.
 */
export function referenceHistoryQuery(args: ReferenceHistoryQueryArgs): string {
  const { hostId, ownerId, epoch, backendSessionId } = args.target.target;
  const params = new URLSearchParams();
  params.set("hostId", hostId);
  params.set("ownerId", ownerId);
  params.set("epoch", epoch);
  params.set("backendSessionId", backendSessionId);
  const providerSessionId = args.target.providerSessionId;
  if (typeof providerSessionId === "string" && providerSessionId.trim().length > 0) {
    params.set("providerSessionId", providerSessionId);
  }
  params.set("limit", String(args.limit ?? REFERENCE_HISTORY_PAGE_LIMIT));
  if (args.cursor) {
    params.set("cursor", String(args.cursor.offset));
    params.set("cursorStream", args.cursor.streamId);
  }
  return params.toString();
}

function malformed(detail: string): never {
  throw new ReferenceHistoryFetchError("MALFORMED_RESPONSE", detail);
}

function requireRecord(value: unknown, detail: string): Record<string, unknown> {
  if (!isRecord(value)) malformed(`${detail} is not an object`);
  return value;
}

function requireString(entry: Record<string, unknown>, key: string, detail: string): string {
  const value = entry[key];
  if (typeof value !== "string") malformed(`${detail}.${key} is not a string`);
  return value;
}

function requireNumber(entry: Record<string, unknown>, key: string, detail: string): number {
  const value = entry[key];
  if (typeof value !== "number" || !Number.isFinite(value)) malformed(`${detail}.${key} is not a number`);
  return value;
}

function requireBoolean(entry: Record<string, unknown>, key: string, detail: string): boolean {
  const value = entry[key];
  if (typeof value !== "boolean") malformed(`${detail}.${key} is not a boolean`);
  return value;
}

function requireEnum<T extends string>(
  entry: Record<string, unknown>,
  key: string,
  allowed: readonly T[],
  detail: string,
): T {
  const value = entry[key];
  if (typeof value !== "string" || !(allowed as readonly string[]).includes(value)) {
    malformed(`${detail}.${key} is not one of ${allowed.join(", ")}`);
  }
  return value as T;
}

function optionalString(
  entry: Record<string, unknown>,
  key: string,
  detail: string,
): string | null | undefined {
  const value = entry[key];
  if (value === undefined) return undefined;
  if (value === null) return null;
  if (typeof value !== "string") malformed(`${detail}.${key} is not a string or null`);
  return value;
}

function optionalNumber(
  entry: Record<string, unknown>,
  key: string,
  detail: string,
): number | null | undefined {
  const value = entry[key];
  if (value === undefined) return undefined;
  if (value === null) return null;
  if (typeof value !== "number" || !Number.isFinite(value)) {
    malformed(`${detail}.${key} is not a number or null`);
  }
  return value;
}

function optionalBoolean(
  entry: Record<string, unknown>,
  key: string,
  detail: string,
): boolean | null | undefined {
  const value = entry[key];
  if (value === undefined) return undefined;
  if (value === null) return null;
  if (typeof value !== "boolean") malformed(`${detail}.${key} is not a boolean or null`);
  return value;
}

function optionalEnum<T extends string>(
  entry: Record<string, unknown>,
  key: string,
  allowed: readonly T[],
  detail: string,
): T | null | undefined {
  const value = entry[key];
  if (value === undefined) return undefined;
  if (value === null) return null;
  if (typeof value !== "string" || !(allowed as readonly string[]).includes(value)) {
    malformed(`${detail}.${key} is not one of ${allowed.join(", ")}`);
  }
  return value as T;
}

const REFERENCE_HISTORY_SOURCES: readonly ReferenceHistorySource[] = [
  "claude-transcript",
  "codex-transcript",
  "omp-transcript",
  "omo-transcript",
  "gjc-transcript",
  "pi-transcript",
  "scrollback",
];

const REFERENCE_HISTORY_AVAILABILITIES: readonly ReferenceHistoryAvailability[] = [
  "native",
  "scrollback",
  "notStarted",
];

const REFERENCE_TURN_ROLES: readonly ReferenceTurnRole[] = ["user", "assistant"];
const REFERENCE_TURN_SOURCES: readonly ReferenceTurnSource[] = ["typed", "runtime"];
const REFERENCE_PART_KINDS: readonly ReferencePartKind[] = [
  "text",
  "thinking",
  "skill",
  "tool",
  "image",
  "compact",
  "notice",
  "taskResult",
];
const REFERENCE_TEXT_PHASES: readonly ReferenceTextPhase[] = ["commentary", "finalAnswer"];
const REFERENCE_SKILL_EVIDENCES = ["invocation", "instructions"] as const;
const REFERENCE_SKILL_STATUSES = ["requested", "loaded", "failed"] as const;
const REFERENCE_TASK_STATUSES = ["completed", "failed", "cancelled"] as const;

function toReferenceImageRef(raw: unknown, detail: string): { mediaType: string; ref: string } {
  const entry = requireRecord(raw, detail);
  return {
    mediaType: requireString(entry, "mediaType", detail),
    ref: requireString(entry, "ref", detail),
  };
}

function toReferenceSkillActivity(raw: unknown, detail: string) {
  const entry = requireRecord(raw, detail);
  const path = optionalString(entry, "path", detail);
  return {
    name: requireString(entry, "name", detail),
    evidence: requireEnum(entry, "evidence", REFERENCE_SKILL_EVIDENCES, detail),
    status: requireEnum(entry, "status", REFERENCE_SKILL_STATUSES, detail),
    ...(path !== undefined ? { path } : {}),
  };
}

function toReferenceTaskResult(raw: unknown, detail: string) {
  const entry = requireRecord(raw, detail);
  const agent = optionalString(entry, "agent", detail);
  const model = optionalString(entry, "model", detail);
  const durationMs = optionalNumber(entry, "durationMs", detail);
  const turns = optionalNumber(entry, "turns", detail);
  const toolCalls = optionalNumber(entry, "toolCalls", detail);
  const tokens = optionalNumber(entry, "tokens", detail);
  const resultCut = optionalBoolean(entry, "resultCut", detail);
  return {
    id: requireString(entry, "id", detail),
    title: requireString(entry, "title", detail),
    status: requireEnum(entry, "status", REFERENCE_TASK_STATUSES, detail),
    result: requireString(entry, "result", detail),
    ...(agent !== undefined ? { agent } : {}),
    ...(model !== undefined ? { model } : {}),
    ...(durationMs !== undefined ? { durationMs } : {}),
    ...(turns !== undefined ? { turns } : {}),
    ...(toolCalls !== undefined ? { toolCalls } : {}),
    ...(tokens !== undefined ? { tokens } : {}),
    ...(resultCut !== undefined ? { resultCut } : {}),
  };
}

function toReferencePart(raw: unknown, detail: string): ReferencePart {
  const entry = requireRecord(raw, detail);
  const kind = requireEnum(entry, "kind", REFERENCE_PART_KINDS, detail);
  switch (kind) {
    case "text": {
      const phase = optionalEnum(entry, "phase", REFERENCE_TEXT_PHASES, detail);
      return {
        kind: "text",
        text: requireString(entry, "text", detail),
        ...(phase !== undefined ? { phase } : {}),
      };
    }
    case "thinking":
      return { kind: "thinking", text: requireString(entry, "text", detail) };
    case "skill":
      return {
        kind: "skill",
        skill: toReferenceSkillActivity(entry.skill, `${detail}.skill`),
      };
    case "tool": {
      const error = optionalBoolean(entry, "error", detail);
      const outputRef = optionalString(entry, "outputRef", detail);
      const outputSize = optionalNumber(entry, "outputSize", detail);
      const skill =
        entry.skill === undefined || entry.skill === null
          ? (entry.skill as null | undefined)
          : toReferenceSkillActivity(entry.skill, `${detail}.skill`);
      const images = Array.isArray(entry.images)
        ? entry.images.map((image, index) =>
            toReferenceImageRef(image, `${detail}.images[${index}]`),
          )
        : undefined;
      return {
        kind: "tool",
        name: requireString(entry, "name", detail),
        summary: requireString(entry, "summary", detail),
        input: requireString(entry, "input", detail),
        output: requireString(entry, "output", detail),
        ...(error !== undefined ? { error } : {}),
        ...(skill !== undefined ? { skill } : {}),
        ...(outputRef !== undefined ? { outputRef } : {}),
        ...(outputSize !== undefined ? { outputSize } : {}),
        ...(images !== undefined ? { images } : {}),
      };
    }
    case "image":
      return {
        kind: "image",
        mediaType: requireString(entry, "mediaType", detail),
        ref: requireString(entry, "ref", detail),
      };
    case "compact":
      return { kind: "compact", text: requireString(entry, "text", detail) };
    case "notice": {
      const source = optionalString(entry, "source", detail);
      return {
        kind: "notice",
        text: requireString(entry, "text", detail),
        ...(source !== undefined ? { source } : {}),
      };
    }
    case "taskResult": {
      if (!Array.isArray(entry.tasks)) malformed(`${detail}.tasks is not an array`);
      return {
        kind: "taskResult",
        tasks: entry.tasks.map((task, index) =>
          toReferenceTaskResult(task, `${detail}.tasks[${index}]`),
        ),
      };
    }
  }
}

function toReferenceTurn(raw: unknown, detail: string): ReferenceTurn {
  const entry = requireRecord(raw, detail);
  const startedAt = optionalString(entry, "startedAt", detail);
  const endedAt = optionalString(entry, "endedAt", detail);
  const source = optionalEnum(entry, "source", REFERENCE_TURN_SOURCES, detail);
  if (!Array.isArray(entry.parts)) malformed(`${detail}.parts is not an array`);
  let abandoned: ReferenceTurn["abandoned"];
  if (entry.abandoned === undefined || entry.abandoned === null) {
    abandoned = entry.abandoned as null | undefined;
  } else {
    const branch = requireRecord(entry.abandoned, `${detail}.abandoned`);
    const summary = optionalString(branch, "summary", `${detail}.abandoned`);
    abandoned = {
      count: requireNumber(branch, "count", `${detail}.abandoned`),
      branches: requireNumber(branch, "branches", `${detail}.abandoned`),
      ...(summary !== undefined ? { summary } : {}),
    };
  }
  return {
    role: requireEnum(entry, "role", REFERENCE_TURN_ROLES, detail),
    parts: entry.parts.map((part, index) => toReferencePart(part, `${detail}.parts[${index}]`)),
    ...(startedAt !== undefined ? { startedAt } : {}),
    ...(endedAt !== undefined ? { endedAt } : {}),
    ...(source !== undefined ? { source } : {}),
    ...(abandoned !== undefined ? { abandoned } : {}),
  };
}

/**
 * Parse an untrusted reference history body into the frozen page DTO. Anything the contract
 * does not allow is `MALFORMED_RESPONSE`; nothing is guessed or defaulted.
 */
export function parseReferenceHistoryPage(body: unknown): ReferenceHistoryPage {
  const entry = requireRecord(body, "Reference history response body");
  const source = requireEnum(entry, "source", REFERENCE_HISTORY_SOURCES, "Reference history response");
  const availability = requireEnum(
    entry,
    "availability",
    REFERENCE_HISTORY_AVAILABILITIES,
    "Reference history response",
  );
  if (!Array.isArray(entry.turns)) malformed("Reference history response.turns is not an array");
  const generation = requireString(entry, "generation", "Reference history response");
  const hasMore = requireBoolean(entry, "hasMore", "Reference history response");
  const unavailableReason = optionalString(entry, "unavailableReason", "Reference history response");
  let cursor: ReferenceHistoryCursor | null | undefined;
  if (entry.cursor === undefined) {
    cursor = undefined;
  } else if (entry.cursor === null) {
    cursor = null;
  } else {
    const raw = requireRecord(entry.cursor, "Reference history response.cursor");
    cursor = {
      streamId: requireString(raw, "streamId", "Reference history response.cursor"),
      offset: requireNumber(raw, "offset", "Reference history response.cursor"),
    };
  }
  if (availability !== "native" && unavailableReason === undefined) {
    malformed("Reference history response.unavailableReason is required when availability is not native");
  }
  return {
    source,
    availability,
    turns: entry.turns.map((turn, index) =>
      toReferenceTurn(turn, `Reference history response.turns[${index}]`),
    ),
    hasMore,
    generation,
    ...(cursor !== undefined ? { cursor } : {}),
    ...(unavailableReason !== undefined ? { unavailableReason } : {}),
  };
}

/** A completed read: the page plus the fence it must be checked against. */
export interface ReferenceHistoryRead {
  readonly page: ReferenceHistoryPage;
  readonly fence: ReferenceHistoryFence;
}

function referenceHistoryStatusErrorCode(status: number): string {
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
    default:
      return "REQUEST_FAILED";
  }
}

/**
 * Read one page of reference native history for a target.
 *
 * A non-native page is a successful read carrying its disclosure; an auth or identity
 * refusal throws `ReferenceHistoryFetchError` and is never converted into a page.
 */
export async function fetchReferenceHistory(args: {
  baseUrl: string;
  sessionId: string;
  token: string;
  target: ReferenceTargetRef;
  limit?: number;
  cursor?: ReferenceHistoryCursor | null;
  signal?: AbortSignal;
}): Promise<ReferenceHistoryRead> {
  const { baseUrl, sessionId, token, signal } = args;
  const query = referenceHistoryQuery({
    target: args.target,
    limit: args.limit,
    cursor: args.cursor,
  });
  const path = `${referenceChatRoute(sessionId, "history")}?${query}`;
  let response: Response;
  try {
    response = await fetch(remoteApiUrl(baseUrl, path), {
      headers: { Authorization: `Bearer ${token}` },
      signal,
    });
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new ReferenceHistoryFetchError(
      "NETWORK_ERROR",
      error instanceof Error ? error.message : String(error),
    );
  }
  if (response.ok) {
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
    return { page, fence: referenceHistoryFence(sessionId, args.target, page.generation) };
  }
  let errorCode: string | null = null;
  try {
    const text = await response.text();
    try {
      const parsed: unknown = JSON.parse(text);
      if (isRecord(parsed)) {
        if (isRecord(parsed.error) && typeof parsed.error.code === "string") {
          errorCode = parsed.error.code;
        } else if (typeof parsed.error === "string") {
          errorCode = parsed.error;
        }
      }
    } catch {}
  } catch {}
  if (errorCode !== null) {
    throw new ReferenceHistoryFetchError(
      errorCode,
      `Reference history request failed with status ${response.status}`,
    );
  }
  throw new ReferenceHistoryFetchError(
    referenceHistoryStatusErrorCode(response.status),
    `Reference history request failed with status ${response.status}`,
  );
}
