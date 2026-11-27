/**
 * Desktop adapter for the structured-chat transport (lane G7).
 *
 * The GUI and the gateway share one [`StructuredChatStore`] per process, so on the desktop the
 * same operations ride one Tauri command instead of HTTP: a session started over the gateway
 * survives a GUI restart, and a session the GUI created is visible to remote clients. The
 * command's request shape is `{op, sessionId?, body?}` — the wire type this module builds.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  structuredChatIpcWire,
  structuredChatTransportFrom,
  type StructuredChatTransport,
  type StructuredChatWireRequest,
} from "./structuredChatTransport";

export type StructuredChatInvokeRequest = StructuredChatWireRequest;

/** The Tauri command lane G1 registered in `src-tauri/src/ipc/structured_chat.rs`. */
export const STRUCTURED_CHAT_COMMAND = "cmd_structured_chat";

export function desktopStructuredChatTransport(
  call: (request: StructuredChatWireRequest) => Promise<unknown> = (request) =>
    invoke(STRUCTURED_CHAT_COMMAND, { request }),
): StructuredChatTransport {
  return structuredChatTransportFrom(structuredChatIpcWire(call));
}

/** The one instance the desktop surface uses; a test builds its own with `desktopStructuredChatTransport`. */
export const desktopStructuredChat: StructuredChatTransport = desktopStructuredChatTransport();
