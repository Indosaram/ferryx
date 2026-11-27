import { invoke } from "@tauri-apps/api/core";
import {
  desktopChatTransportFrom,
  type DesktopChatIdentity,
  type DesktopChatRequest,
  type DesktopChatResult,
} from "./desktopChat";
import {
  ReferenceFileError,
  type ReferenceFileStagingTransport,
} from "../remote/chat/referenceFiles";
import type {
  ReferenceFileReceipt,
  ReferenceFileStagePayload,
} from "../remote/chat/referenceTypes";
import type { ScopeErrorCode, TargetRef } from "./scopedContracts";

interface WireError { readonly error: { readonly code: string; readonly message: string } }
type WireBody<T> = T | WireError | { readonly ok: true; readonly data: T };

async function request<T>(input: DesktopChatRequest, signal: AbortSignal): Promise<DesktopChatResult<T>> {
  signal.throwIfAborted();
  const reply = await invoke<{ status: number; body: WireBody<T> }>("cmd_desktop_reference_chat", { request: input });
  signal.throwIfAborted();
  const body = reply.body;
  if (typeof body === "object" && body !== null && "error" in body) {
    return { ok: false, code: body.error.code, message: body.error.message };
  }
  if (reply.status < 200 || reply.status >= 300) {
    return { ok: false, code: input.kind === "history" || input.kind === "prompt" ? "INTERNAL" : "OPERATION_OUTCOME_UNKNOWN", message: `Chat request failed (${reply.status})` };
  }
  if (typeof body === "object" && body !== null && "ok" in body && "data" in body) {
    return { ok: true, data: body.data };
  }
  return { ok: true, data: body };
}

export const desktopChatIpcTransport = desktopChatTransportFrom({
  history: (input, signal) => request(input, signal),
  prompt: (input, signal) => request(input, signal),
  submit: (input, signal) => request(input, signal),
  stop: (input, signal) => request(input, signal),
  answer: (input, signal) => request(input, signal),
  resolvePrompt: (input, signal) => request(input, signal),
});

export function desktopChatIdentity(): Promise<DesktopChatIdentity> {
  return invoke("cmd_desktop_reference_chat_identity");
}

// --- Desktop file staging: the reference-chat files routes over the desktop bridge ----------

/**
 * The reply shape both desktop staging commands answer: the HTTP status and body the reference
 * chat's own file routes would have produced, so one mapping serves both transports.
 */
interface FileReply { readonly status: number; readonly body: unknown }

function fileChatError(reply: FileReply, fallback: string): ReferenceFileError {
  const body = reply.body as { error?: { code?: string; message?: string } } | null;
  const code = typeof body?.error?.code === "string" ? body.error.code : "INTERNAL";
  return new ReferenceFileError(code as ScopeErrorCode, body?.error?.message || fallback);
}

/**
 * The same POST/DELETE `/api/v1/reference-chat/{sessionId}/files` behaviour the remote web client
 * uses, carried by the desktop bridge instead of an HTTP bearer session. Nothing is staged in
 * the browser: the bytes go to the pane's own host and come back as a receipt whose mention is
 * the path the pane's agent can open.
 *
 * The bridge resolves the staging directory from the pane the daemon owns, so a session whose
 * working directory lives on a paired host is refused exactly as the route refuses it.
 */
export function desktopChatFileStagingTransport(sessionId: string): ReferenceFileStagingTransport {
  return {
    stage: async (target: TargetRef, payload: ReferenceFileStagePayload, signal) => {
      signal.throwIfAborted();
      const reply = await invoke<FileReply>("cmd_desktop_reference_chat_file_stage", {
        sessionId: target.backendSessionId || sessionId,
        params: payload,
      });
      signal.throwIfAborted();
      if (reply.status < 200 || reply.status >= 300) {
        throw fileChatError(reply, "Staging this file failed on the owning host.");
      }
      return reply.body as ReferenceFileReceipt;
    },
    // Cancellation and removal are best-effort, host-confirmed calls, so neither carries the
    // (already finished) staging call's signal, and a refusal is a plain `false`.
    cancel: async (target: TargetRef, attachmentId: string) => {
      const reply = await invoke<FileReply>("cmd_desktop_reference_chat_file_remove", {
        sessionId: target.backendSessionId || sessionId,
        attachmentId,
      });
      return reply.status >= 200 && reply.status < 300;
    },
    remove: async (target: TargetRef, attachmentId: string) => {
      const reply = await invoke<FileReply>("cmd_desktop_reference_chat_file_remove", {
        sessionId: target.backendSessionId || sessionId,
        attachmentId,
      });
      return reply.status >= 200 && reply.status < 300;
    },
  };
}
