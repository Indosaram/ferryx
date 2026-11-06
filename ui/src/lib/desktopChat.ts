/**
 * Desktop chat lens over one terminal pane: transport contract and pure helpers.
 * Reuses the rev3 reference-chat DTOs; the transport is injected, nothing here fetches.
 */
import { formatReferenceDuration } from "../remote/chat/MobileChatComponents";
import type { MobileChatMessageProps } from "../remote/chat/MobileChatMessage";
import {
  REFERENCE_OUTCOME_UNKNOWN_CODE,
  referenceNativeKindFromRegistryId,
  referenceNativeKindIsNative,
  type ReferenceHistoryCursor,
  type ReferenceHistoryPage,
  type ReferencePrompt,
  type ReferencePromptAnswerPayload,
  type ReferenceStopCapability,
  type ReferenceStopPayload,
  type ReferenceSubmitPayload,
  type ReferenceTargetRef,
  type ReferenceTurn,
} from "../remote/chat/referenceTypes";
import type { TerminalSession } from "./types";

export interface DesktopChatPromptRead {
  readonly prompt: ReferencePrompt | null;
  readonly screenRevision: string | null;
}

export interface DesktopChatRequests {
  readonly history: {
    readonly params: { readonly cursor: ReferenceHistoryCursor | null };
    readonly data: ReferenceHistoryPage;
  };
  readonly prompt: { readonly params: Record<string, never>; readonly data: DesktopChatPromptRead };
  readonly submit: { readonly params: ReferenceSubmitPayload; readonly data: unknown };
  readonly stop: { readonly params: ReferenceStopPayload; readonly data: unknown };
  readonly answer: { readonly params: ReferencePromptAnswerPayload; readonly data: unknown };
}

export type DesktopChatRequestKind = keyof DesktopChatRequests;

export interface DesktopChatRequest<K extends DesktopChatRequestKind = DesktopChatRequestKind> {
  readonly kind: K;
  /** Fresh per request; a mutation's id is its idempotency key on the host. */
  readonly requestId: string;
  readonly target: ReferenceTargetRef;
  readonly registryId: string | null;
  readonly params: DesktopChatRequests[K]["params"];
}

export type DesktopChatResult<T> =
  | { readonly ok: true; readonly data: T }
  | { readonly ok: false; readonly code: string; readonly message: string };

export type DesktopChatResponse<K extends DesktopChatRequestKind> = Promise<DesktopChatResult<DesktopChatRequests[K]["data"]>>;

export type DesktopChatAnyRequest = { readonly [K in DesktopChatRequestKind]: DesktopChatRequest<K> }[DesktopChatRequestKind];

/**
 * One typed async request function, overloaded per kind so each request names its data.
 * A throw on a mutation is treated as
 * `OPERATION_OUTCOME_UNKNOWN`: it may have landed and is never replayed on its own.
 */
export interface DesktopChatTransport {
  (request: DesktopChatRequest<"history">, signal: AbortSignal): DesktopChatResponse<"history">;
  (request: DesktopChatRequest<"prompt">, signal: AbortSignal): DesktopChatResponse<"prompt">;
  (request: DesktopChatRequest<"submit">, signal: AbortSignal): DesktopChatResponse<"submit">;
  (request: DesktopChatRequest<"stop">, signal: AbortSignal): DesktopChatResponse<"stop">;
  (request: DesktopChatRequest<"answer">, signal: AbortSignal): DesktopChatResponse<"answer">;
}

export type DesktopChatHandlers = {
  readonly [K in DesktopChatRequestKind]: (request: DesktopChatRequest<K>, signal: AbortSignal) => DesktopChatResponse<K>;
};

export function desktopChatTransportFrom(handlers: DesktopChatHandlers): DesktopChatTransport {
  function dispatch(request: DesktopChatRequest<"history">, signal: AbortSignal): DesktopChatResponse<"history">;
  function dispatch(request: DesktopChatRequest<"prompt">, signal: AbortSignal): DesktopChatResponse<"prompt">;
  function dispatch(request: DesktopChatRequest<"submit">, signal: AbortSignal): DesktopChatResponse<"submit">;
  function dispatch(request: DesktopChatRequest<"stop">, signal: AbortSignal): DesktopChatResponse<"stop">;
  function dispatch(request: DesktopChatRequest<"answer">, signal: AbortSignal): DesktopChatResponse<"answer">;
  function dispatch(request: DesktopChatAnyRequest, signal: AbortSignal): Promise<DesktopChatResult<unknown>> {
    switch (request.kind) {
      case "history":
        return handlers.history(request, signal);
      case "prompt":
        return handlers.prompt(request, signal);
      case "submit":
        return handlers.submit(request, signal);
      case "stop":
        return handlers.stop(request, signal);
      case "answer":
        return handlers.answer(request, signal);
    }
  }
  return dispatch;
}

/** `ownerId`/`epoch` default to the ones the session itself published. */
export interface DesktopChatIdentity {
  readonly hostId: string;
  readonly ownerId?: string | null;
  readonly epoch?: string | null;
}

/** A typed refusal; `ReferencePromptCard` reads `code` to tell a moved screen apart. */
export class DesktopChatRefusal extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "DesktopChatRefusal";
    this.code = code;
  }
}

export function desktopChatSessionExited(session: TerminalSession): boolean {
  return session.lifecycle === "exited" || session.lifecycle === "failed";
}

/** Null until the pane has a complete identity. An exited pane keeps its last real backend id. */
export function desktopChatTargetFor(
  session: TerminalSession,
  identity: DesktopChatIdentity,
): ReferenceTargetRef | null {
  const backendSessionId =
    session.backendSessionId ?? (desktopChatSessionExited(session) ? session.lastBackendSessionId ?? null : null);
  const ownerId = session.ownerId ?? identity.ownerId ?? null;
  const epoch = session.daemonEpoch ?? identity.epoch ?? null;
  if (!identity.hostId || !backendSessionId || !ownerId || !epoch) return null;
  return {
    target: { hostId: identity.hostId, ownerId, epoch, backendSessionId },
    providerSessionId: session.providerSession?.id ?? session.agentSessionId ?? null,
  };
}

export function desktopChatStopCapability(agentType: string | null | undefined): ReferenceStopCapability {
  if (!agentType) return "refused";
  return referenceNativeKindIsNative(referenceNativeKindFromRegistryId(agentType))
    ? "providerInterrupt"
    : "refused";
}

export function newDesktopChatRequestId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
  );
}

function turnDuration(startedAt?: string | null, endedAt?: string | null): string | undefined {
  if (!startedAt || !endedAt) return undefined;
  const start = Date.parse(startedAt);
  const end = Date.parse(endedAt);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return undefined;
  return formatReferenceDuration(end - start);
}

function turnFallbackId(turn: ReferenceTurn): string {
  const first = turn.parts.find((part) => part.kind === "text");
  const seed = `${turn.role}|${turn.startedAt ?? ""}|${(first?.text ?? "").slice(0, 64)}`;
  let hash = 0;
  for (let index = 0; index < seed.length; index += 1) hash = (hash * 31 + seed.charCodeAt(index)) >>> 0;
  return `turn-${hash.toString(36)}`;
}

function turnText(turn: ReferenceTurn): string {
  return turn.parts
    .map((part) => (part.kind === "text" || part.kind === "thinking" || part.kind === "compact" ? part.text : null))
    .filter((text): text is string => text !== null)
    .join("\n\n");
}

export function desktopChatMessagesFromTurns(turns: readonly ReferenceTurn[]): MobileChatMessageProps[] {
  return turns.map((turn) => ({
    id: typeof turn.id === "string" && turn.id.length > 0 ? turn.id : turnFallbackId(turn),
    role: turn.role,
    content: turnText(turn),
    timestamp: turn.startedAt ?? undefined,
    referenceParts: turn.parts,
    referenceSource: turn.source ?? null,
    referenceAbandoned: turn.abandoned ?? null,
    referenceMeta: turn.meta ?? null,
    durationLabel: turn.role === "assistant" ? turnDuration(turn.startedAt, turn.endedAt) : undefined,
  }));
}

export function desktopChatUniqueIds(messages: readonly MobileChatMessageProps[]): MobileChatMessageProps[] {
  const seen = new Map<string, number>();
  return messages.map((message) => {
    const repeat = seen.get(message.id) ?? 0;
    seen.set(message.id, repeat + 1);
    return repeat === 0 ? message : { ...message, id: `${message.id}~${repeat}` };
  });
}

/** Confirmed user turns with exactly this text; a pending echo retires once the count grows. */
export function desktopChatUserTextCount(turns: readonly ReferenceTurn[], text: string): number {
  return turns.filter((turn) => turn.role === "user" && turnText(turn).trim() === text).length;
}

export function desktopChatRestoreDraft(current: string, text: string): string {
  if (current.length === 0) return text;
  if (current.startsWith(text)) return current;
  return `${text}\n${current}`;
}

export async function desktopChatMutate(run: () => Promise<DesktopChatResult<unknown>>): Promise<DesktopChatResult<unknown>> {
  try {
    return await run();
  } catch (error) {
    return {
      ok: false,
      code: REFERENCE_OUTCOME_UNKNOWN_CODE,
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

export const DESKTOP_CHAT_SEND_FAILED = "That message was not delivered. It is back in the message box.";
export const DESKTOP_CHAT_OUTCOME_UNKNOWN =
  "That message may have reached the session, but it was not confirmed. It is back in the message box and won't be sent again on its own.";
export const DESKTOP_CHAT_VIEW_ONLY = "This session is view-only here, so that message wasn't sent. It is back in the message box.";
export const DESKTOP_CHAT_STOP_REFUSED =
  "Stop isn't available for this agent here, so nothing was sent. Open the terminal and press Ctrl-C to interrupt it.";
export const DESKTOP_CHAT_STOP_FAILED = "The stop request was not delivered.";
export const DESKTOP_CHAT_HELD = "Not sent: answer with one of the options above.";
export const DESKTOP_CHAT_AMBIGUOUS = "That answer did not name exactly one option, so it was not sent.";
export const DESKTOP_CHAT_NO_TARGET = "This session is not attached yet, so nothing was sent.";
export const DESKTOP_CHAT_TOO_LONG = "That message is longer than the composer allows, so it was not sent.";
export const DESKTOP_CHAT_REFRESH_FAILED = "Couldn't refresh the chat. Showing the last messages received.";
export const DESKTOP_CHAT_PROMPT_CHANGED = "The question changed. Check it and answer again.";
export const DESKTOP_CHAT_EXITED = "This session has exited. Previously loaded messages remain visible; nothing can be sent to it.";
