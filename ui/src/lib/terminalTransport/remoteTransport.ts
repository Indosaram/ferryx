import type { TerminalAttachment, TerminalTransport, Unsubscribe } from "./types";

export class WebSocketTerminalTransport implements TerminalTransport {
  private baseUrl: string;
  private token: string;
  private sockets: Map<string, WebSocket> = new Map();
  private outputListeners: Map<string, Set<(data: Uint8Array) => void>> = new Map();

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
    return data.map((s: { sessionId: string }) => ({ sessionId: s.sessionId }));
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

  write(sessionId: string, data: string | Uint8Array) {
    const ws = this.sockets.get(sessionId);
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
    ws.send(bytes);
  }

  resize(sessionId: string, cols: number, rows: number) {
    const ws = this.sockets.get(sessionId);
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: "resize", cols, rows }));
  }

  signal(sessionId: string, signal: "interrupt" | "terminate" | "kill") {
    const ws = this.sockets.get(sessionId);
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: "signal", signal }));
  }

  async close(sessionId: string) {
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
