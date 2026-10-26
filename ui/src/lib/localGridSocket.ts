import { Channel, invoke } from "@tauri-apps/api/core";

import type { WebSocketLike } from "../remote/RemoteTerminal";

const pendingWrites = new Map<string, Promise<void>>();

/**
 * A WebSocketLike backed by the local grid stream IPC (`cmd_terminal_grid_*`), so the remote
 * client's DOM grid renderer can draw a local session when the native GPU renderer is
 * unavailable. Grid frames arrive on a Tauri channel; input and resize map to local commands.
 * Closing stops only this subscriber; the PTY session keeps running.
 */
export function createLocalGridSocket(sessionId: string): WebSocketLike {
  let streamId: string | null = null;
  let closed = false;
  function write(data: string): void {
    const pending = (pendingWrites.get(sessionId) ?? Promise.resolve())
      .then(() => invoke<void>("cmd_terminal_write", { sessionId, data }))
      .catch((error: unknown) => {
        console.warn("Compatibility renderer input failed", error);
        socket.onerror?.(error);
      });
    pendingWrites.set(sessionId, pending);
    void pending.then(() => {
      if (pendingWrites.get(sessionId) === pending) pendingWrites.delete(sessionId);
    });
  }
  const socket: WebSocketLike = {
    readyState: 0,
    binaryType: "arraybuffer",
    onopen: null,
    onmessage: null,
    onerror: null,
    onclose: null,
    send(data) {
      if (closed) return;
      const text = typeof data === "string" ? data : new TextDecoder().decode(data);
      if (typeof data === "string" && text.startsWith("{")) {
        let control: { type?: unknown; cols?: unknown; rows?: unknown; signal?: unknown } | null = null;
        try {
          control = JSON.parse(text);
        } catch {
          control = null;
        }
        if (control?.type === "resize" && typeof control.cols === "number" && typeof control.rows === "number") {
          if (streamId) {
            void invoke("cmd_terminal_grid_resize", { streamId, sessionId, cols: control.cols, rows: control.rows })
              .catch((error: unknown) => console.warn("Compatibility renderer resize failed", error));
          }
          return;
        }
        if (control?.type === "signal" && control.signal === "interrupt") {
          write("\u0003");
          return;
        }
        if (control?.type === "scroll") return;
      }
      write(text);
    },
    close() {
      if (closed) return;
      closed = true;
      socket.readyState = 3;
      if (streamId) void invoke("cmd_terminal_grid_detach", { streamId }).catch(() => undefined);
    },
  };
  const channel = new Channel<string>((frame) => {
    if (!closed) socket.onmessage?.({ data: frame });
  });
  void invoke<string>("cmd_terminal_grid_attach", { sessionId, channel }).then((id) => {
    streamId = id;
    if (closed) {
      void invoke("cmd_terminal_grid_detach", { streamId: id }).catch(() => undefined);
      return;
    }
    socket.readyState = 1;
    socket.onopen?.();
  }, (error: unknown) => {
    if (closed) return;
    console.warn("Compatibility renderer attach failed", error);
    socket.onerror?.(error);
  });
  return socket;
}
