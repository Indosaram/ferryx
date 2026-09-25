import type { TerminalAttachment, TerminalTransport, Unsubscribe } from "./types";

export const MAX_OUTBOUND_BUFFER_BYTES = 4096;

type PendingPayload = {
  readonly data: string | Uint8Array;
  readonly byteLength: number;
};

interface RemoteSessionTarget {
  sessionId?: string;
  daemonEpoch?: string | number;
  machineId?: string;
}

interface RemoteSessionRow {
  sessionId?: string;
  session_id?: string;
  target?: RemoteSessionTarget;
  daemonEpoch?: string | number;
  worktreePath?: string | null;
  running?: boolean;
}

export class WebSocketTerminalTransport implements TerminalTransport {
  private baseUrl: string;
  private token: string;
  private sockets: Map<string, WebSocket> = new Map();
  private outputListeners: Map<string, Set<(data: Uint8Array) => void>> = new Map();
  private outboundBuffers: Map<string, PendingPayload[]> = new Map();
  private outboundBufferBytes: Map<string, number> = new Map();
  private outboundDropCounts: Map<string, number> = new Map();
  private onOutboundDropCallback?: (sessionId: string, droppedBytes: number) => void;

  constructor(baseUrl: string, token: string) {
    this.baseUrl = baseUrl.replace(/\/$/, "");
    this.token = token;
  }

  async listSessions() {
    // Credentials belong in the Authorization header, never the query string.
    const res = await fetch(`${this.baseUrl}/api/v1/sessions`, {
      headers: { Authorization: `Bearer ${this.token}` },
    });
    if (!res.ok) throw new Error(`Failed to list sessions: ${res.statusText}`);
    const data = await res.json();
    const rows: RemoteSessionRow[] = Array.isArray(data)
      ? data
      : Array.isArray(data?.sessions)
        ? data.sessions
        : [];
    return rows.map((s: RemoteSessionRow) => ({
      sessionId: s.sessionId ?? s.session_id ?? s.target?.sessionId ?? "",
      target: s.target,
      daemonEpoch: s.daemonEpoch !== undefined && s.daemonEpoch !== null
        ? String(s.daemonEpoch)
        : s.target?.daemonEpoch !== undefined && s.target?.daemonEpoch !== null
          ? String(s.target.daemonEpoch)
          : undefined,
      worktreePath: s.worktreePath,
      running: s.running,
    }));
  }

  /// Trades the device token for a single-use ticket.
  ///
  /// A browser `WebSocket` cannot send an `Authorization` header, so the socket URL
  /// has to carry its credential in the query string. Minting a short-lived ticket
  /// over HTTP keeps the permanent token in a header, so the URL that reaches
  /// browser history and gateway access logs is only redeemable once.
  private async mintSocketTicket(target: string): Promise<string> {
    const res = await fetch(`${this.baseUrl}/api/v1/socket-ticket`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${this.token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ target }),
    });
    if (!res.ok) throw new Error(`Failed to mint socket ticket: ${res.statusText}`);
    const { ticket } = await res.json();
    if (!ticket) throw new Error("Socket ticket response did not contain a ticket");
    return ticket as string;
  }

  async attach(sessionId: string, _afterSequence?: string | null): Promise<TerminalAttachment> {
    const wsProto = this.baseUrl.startsWith("https") ? "wss" : "ws";
    const host = this.baseUrl.replace(/^https?:\/\//, "");
    const ticket = await this.mintSocketTicket(`/api/v1/terminal/${sessionId}`);
    const wsUrl = `${wsProto}://${host}/api/v1/terminal/${sessionId}?ticket=${encodeURIComponent(ticket)}`;

    const ws = new WebSocket(wsUrl);
    ws.binaryType = "arraybuffer";

    const listeners = this.outputListeners.get(sessionId) ?? new Set();
    this.outputListeners.set(sessionId, listeners);

    const flush = () => {
      this.flushOutboundBuffer(sessionId, ws);
    };
    if (typeof ws.addEventListener === "function") {
      ws.addEventListener("open", flush);
    }
    const origOnOpen = ws.onopen;
    ws.onopen = (event: Event) => {
      flush();
      if (typeof origOnOpen === "function") {
        origOnOpen.call(ws, event);
      }
    };
    if (ws.readyState === WebSocket.OPEN) {
      flush();
    }

    ws.onmessage = (event) => {
      let data: Uint8Array;
      if (typeof event.data === "string") {
        data = new TextEncoder().encode(event.data);
      } else {
        data = new Uint8Array(event.data);
      }
      for (const listener of listeners) {
        listener(data);
      }
    };

    this.sockets.set(sessionId, ws);
    return { sessionId };
  }

  private enqueuePayload(sessionId: string, data: string | Uint8Array, byteLength: number): void {
    const currentBytes = this.outboundBufferBytes.get(sessionId) ?? 0;
    if (currentBytes + byteLength > MAX_OUTBOUND_BUFFER_BYTES) {
      const drops = (this.outboundDropCounts.get(sessionId) ?? 0) + 1;
      this.outboundDropCounts.set(sessionId, drops);
      this.onOutboundDropCallback?.(sessionId, byteLength);
      return;
    }
    const queue = this.outboundBuffers.get(sessionId) ?? [];
    queue.push({ data, byteLength });
    this.outboundBuffers.set(sessionId, queue);
    this.outboundBufferBytes.set(sessionId, currentBytes + byteLength);
  }

  private flushOutboundBuffer(sessionId: string, ws: WebSocket): void {
    const queue = this.outboundBuffers.get(sessionId);
    if (!queue || queue.length === 0) return;
    this.outboundBuffers.delete(sessionId);
    this.outboundBufferBytes.delete(sessionId);
    for (const item of queue) {
      ws.send(item.data);
    }
  }

  getOutboundDropCount(sessionId: string): number {
    return this.outboundDropCounts.get(sessionId) ?? 0;
  }

  getOutboundBufferSize(sessionId: string): number {
    return this.outboundBufferBytes.get(sessionId) ?? 0;
  }

  clearOutboundBuffer(sessionId: string): void {
    this.outboundBuffers.delete(sessionId);
    this.outboundBufferBytes.delete(sessionId);
  }

  setOnOutboundDrop(callback?: (sessionId: string, droppedBytes: number) => void): void {
    this.onOutboundDropCallback = callback;
  }

  write(sessionId: string, data: string | Uint8Array) {
    const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
    const ws = this.sockets.get(sessionId);
    if (ws && ws.readyState === WebSocket.OPEN && (this.outboundBufferBytes.get(sessionId) ?? 0) === 0) {
      ws.send(bytes);
      return;
    }
    this.enqueuePayload(sessionId, bytes, bytes.byteLength);
  }

  resize(sessionId: string, cols: number, rows: number) {
    const payload = JSON.stringify({ type: "resize", cols, rows });
    const ws = this.sockets.get(sessionId);
    if (ws && ws.readyState === WebSocket.OPEN && (this.outboundBufferBytes.get(sessionId) ?? 0) === 0) {
      ws.send(payload);
      return;
    }
    const byteLength = new TextEncoder().encode(payload).byteLength;
    this.enqueuePayload(sessionId, payload, byteLength);
  }

  signal(sessionId: string, signal: "interrupt" | "terminate" | "kill") {
    const payload = JSON.stringify({ type: "signal", signal });
    const ws = this.sockets.get(sessionId);
    if (ws && ws.readyState === WebSocket.OPEN && (this.outboundBufferBytes.get(sessionId) ?? 0) === 0) {
      ws.send(payload);
      return;
    }
    const byteLength = new TextEncoder().encode(payload).byteLength;
    this.enqueuePayload(sessionId, payload, byteLength);
  }

  async close(sessionId: string) {
    this.clearOutboundBuffer(sessionId);
    const ws = this.sockets.get(sessionId);
    if (ws) {
      ws.close();
      this.sockets.delete(sessionId);
    }
  }

  onOutput(sessionId: string, listener: (data: string | Uint8Array) => void): Unsubscribe {
    const listeners = this.outputListeners.get(sessionId) ?? new Set();
    const wrapped = (bytes: Uint8Array) => listener(bytes);
    listeners.add(wrapped);
    this.outputListeners.set(sessionId, listeners);

    return () => {
      listeners.delete(wrapped);
    };
  }

  onLifecycle(_listener: (event: { sessionId: string; state: string; exitCode?: number | null }) => void): Unsubscribe {
    return () => {};
  }
}
