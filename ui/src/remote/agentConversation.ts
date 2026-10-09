import { remoteApiUrl } from "./remoteClient";
import type { MobileChatMessageProps } from "./chat/MobileChatMessage";
import type { ChatWorkItem, ToolCallCardProps, ToolStatus } from "./chat/MobileChatComponents";

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
  conversationGeneration?: string | null;
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
  let conversationGeneration: string | null | undefined;
  if ("conversationGeneration" in body) {
    if (body.conversationGeneration !== null && typeof body.conversationGeneration !== "string") {
      throw new ConversationFetchError(
        "MALFORMED_RESPONSE",
        "Agent history response conversationGeneration is not a string or null",
      );
    }
    conversationGeneration = body.conversationGeneration as string | null;
  }
  return {
    sessionId: body.sessionId,
    items: body.items.map(toMessage),
    nextCursor: body.nextCursor,
    partial: body.partial,
    warnings: body.warnings,
    ...(conversationGeneration !== undefined ? { conversationGeneration } : {}),
  };
}

export interface TranscriptTargetIdentity {
  sessionId: string;
  targetEpoch: string | null;
  generation: number;
  historyGeneration: string | null | undefined;
}

export function isTranscriptResponseCurrent(
  expected: TranscriptTargetIdentity,
  current: {
    sessionId: string;
    targetEpoch: string | null;
    generation: number;
    historyGeneration: string | null | undefined;
  },
  page?: { sessionId: string },
): boolean {
  if (expected.generation !== current.generation) return false;
  if (expected.sessionId !== current.sessionId) return false;
  if (expected.targetEpoch !== current.targetEpoch) return false;
  if (expected.historyGeneration !== current.historyGeneration) return false;
  if (page && page.sessionId !== expected.sessionId) return false;
  return true;
}

export function hasConversationReset(
  lastGeneration: string | null | undefined,
  nextGeneration: string | null | undefined,
): boolean {
  if (lastGeneration !== undefined && nextGeneration !== undefined) {
    return lastGeneration !== nextGeneration;
  }
  return false;
}

export async function fetchAgentConversation(args: {
  baseUrl: string;
  sessionId: string;
  token: string;
  limit?: number;
  cursor?: number | null;
  signal?: AbortSignal;
}): Promise<ConversationPage> {
  const { baseUrl, sessionId, token, cursor, signal } = args;
  const limit = args.limit ?? 200;
  let path = `/api/v1/agent-history/${encodeURIComponent(sessionId)}?limit=${limit}`;
  if (typeof cursor === "number") {
    path += `&cursor=${cursor}`;
  }
  let response: Response;
  try {
    response = await fetch(remoteApiUrl(baseUrl, path), {
      headers: { Authorization: `Bearer ${token}` },
      signal,
    });
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new ConversationFetchError("NETWORK_ERROR", error instanceof Error ? error.message : String(error));
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
