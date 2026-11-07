import { invoke } from "@tauri-apps/api/core";
import {
  desktopChatTransportFrom,
  type DesktopChatIdentity,
  type DesktopChatRequest,
  type DesktopChatResult,
} from "./desktopChat";

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
});

export function desktopChatIdentity(): Promise<DesktopChatIdentity> {
  return invoke("cmd_desktop_reference_chat_identity");
}
