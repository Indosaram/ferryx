/**
 * Remote Browser Screencast WebSocket Client (§4.1, §4.2)
 *
 * Implements typed transport for browser screencast protocol.
 * Enforces NO auto-claim of driver lease (driver claims must be explicit).
 * Enforces client->server binary is NEVER sent (binary messages from client are rejected).
 */

import {
  decodeFrame,
  parseServerMessage,
  serializeClientMessage,
  type BrowserDriverClaimedMessage,
  type BrowserDriverChangedMessage,
  type BrowserDriverReleasedMessage,
  type BrowserErrorMessage,
  type BrowserFrame,
  type BrowserHelloMessage,
  type BrowserPongMessage,
  type BrowserStateMessage,
  type BrowserSubscribedMessage,
  type BrowserSubscribeOptions,
  type BrowserUnsubscribedMessage,
  type ClientMessage,
  type ServerMessage,
} from "./browserProtocol";

export interface BrowserCommandRequest {
  browserId: string;
  leaseEpoch: string;
  browserInstanceId: string;
  desktopEpoch: string;
  documentGeneration: string;
  command: string;
  params?: Record<string, unknown>;
}

export class BrowserClient {
  private ws: WebSocket;
  private requestCounter = 0;
  private pendingRequests = new Map<
    string,
    {
      resolve: (value: any) => void;
      reject: (reason: any) => void;
      timeoutId?: ReturnType<typeof setTimeout>;
    }
  >();

  private frameListeners = new Set<(frame: BrowserFrame) => void>();
  private helloListeners = new Set<(hello: BrowserHelloMessage) => void>();
  private stateListeners = new Set<(state: BrowserStateMessage) => void>();
  private driverChangedListeners = new Set<(msg: BrowserDriverChangedMessage) => void>();
  private errorListeners = new Set<(err: Error | BrowserErrorMessage) => void>();
  private closeListeners = new Set<() => void>();

  private cachedHello: BrowserHelloMessage | null = null;
  private currentSubscription: BrowserSubscribedMessage | null = null;
  private isClosed = false;

  constructor(readonly url: string) {
    this.ws = new WebSocket(url);
    this.ws.binaryType = "arraybuffer";

    this.ws.onmessage = (event) => {
      this.handleIncomingMessage(event.data);
    };

    this.ws.onerror = (_err) => {
      const error = new Error(`WebSocket error on ${this.url}`);
      for (const listener of this.errorListeners) {
        listener(error);
      }
    };

    this.ws.onclose = () => {
      this.isClosed = true;
      this.rejectAllPending(new Error("WebSocket closed"));
      for (const listener of this.closeListeners) {
        listener();
      }
    };
  }

  private isSocketOpen(): boolean {
    return (
      this.ws.readyState === 1 ||
      (typeof WebSocket !== "undefined" && this.ws.readyState === WebSocket.OPEN)
    );
  }

  private async ensureOpen(): Promise<void> {
    if (this.isClosed) throw new Error("WebSocket is closed");
    if (this.isSocketOpen()) return;

    return new Promise((resolve, reject) => {
      const check = () => {
        if (this.isSocketOpen()) {
          resolve();
        } else if (this.ws.readyState > 1) {
          reject(new Error("WebSocket closed before opening"));
        }
      };

      const prevOpen = this.ws.onopen;
      this.ws.onopen = (ev) => {
        prevOpen?.call(this.ws, ev as any);
        resolve();
      };

      const prevClose = this.ws.onclose;
      this.ws.onclose = (ev) => {
        prevClose?.call(this.ws, ev as any);
        reject(new Error("WebSocket closed before opening"));
      };

      const prevError = this.ws.onerror;
      this.ws.onerror = (ev) => {
        prevError?.call(this.ws, ev as any);
        reject(new Error("WebSocket error before opening"));
      };

      check();
    });
  }

  private nextRequestId(): string {
    this.requestCounter += 1;
    return `req-${Date.now().toString(36)}-${this.requestCounter}`;
  }

  private handleIncomingMessage(data: string | ArrayBuffer) {
    if (data instanceof ArrayBuffer) {
      try {
        const frame = decodeFrame(data);
        for (const listener of this.frameListeners) {
          listener(frame);
        }
      } catch (err) {
        const error = err instanceof Error ? err : new Error(String(err));
        for (const listener of this.errorListeners) {
          listener(error);
        }
      }
      return;
    }

    if (typeof data === "string") {
      let msg: ServerMessage;
      try {
        msg = parseServerMessage(data);
      } catch (err) {
        const error = err instanceof Error ? err : new Error(String(err));
        for (const listener of this.errorListeners) {
          listener(error);
        }
        return;
      }

      switch (msg.type) {
        case "browserHello": {
          this.cachedHello = msg;
          for (const listener of this.helloListeners) {
            listener(msg);
          }
          break;
        }
        case "browserSubscribed": {
          this.currentSubscription = msg;
          this.resolvePending(msg.requestId, msg);
          break;
        }
        case "browserPong": {
          if (msg.requestId) {
            this.resolvePending(msg.requestId, msg);
          }
          break;
        }
        case "browserDriverClaimed": {
          this.resolvePending(msg.requestId, msg);
          break;
        }
        case "browserDriverChanged": {
          for (const listener of this.driverChangedListeners) {
            listener(msg);
          }
          break;
        }
        case "browserDriverReleased": {
          this.resolvePending(msg.requestId, msg);
          break;
        }
        case "browserResult": {
          this.resolvePending(msg.requestId, msg);
          break;
        }
        case "browserError": {
          if (msg.requestId && this.pendingRequests.has(msg.requestId)) {
            const err = new Error(msg.message) as Error & {
              code: string;
              retryable: boolean;
              retryAfterMs?: number;
            };
            err.code = msg.code;
            err.retryable = msg.retryable;
            err.retryAfterMs = msg.retryAfterMs;
            this.rejectPending(msg.requestId, err);
          } else {
            for (const listener of this.errorListeners) {
              listener(msg);
            }
          }
          break;
        }
        case "browserState": {
          for (const listener of this.stateListeners) {
            listener(msg);
          }
          break;
        }
        case "browserUnsubscribed": {
          this.currentSubscription = null;
          this.resolvePending(msg.requestId, msg);
          break;
        }
      }
    }
  }

  private sendJson(message: ClientMessage) {
    if (this.isClosed || !this.isSocketOpen()) {
      throw new Error("Cannot send message: WebSocket is not open");
    }
    const text = serializeClientMessage(message);
    this.ws.send(text);
  }

  private requestResponse<T>(
    buildMessage: (requestId: string) => ClientMessage,
    timeoutMs = 10000,
  ): Promise<T> {
    const requestId = this.nextRequestId();
    const message = buildMessage(requestId);

    return new Promise<T>((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        this.pendingRequests.delete(requestId);
        reject(new Error(`Request timed out after ${timeoutMs}ms (${requestId})`));
      }, timeoutMs);

      this.pendingRequests.set(requestId, {
        resolve: (val) => {
          clearTimeout(timeoutId);
          resolve(val);
        },
        reject: (err) => {
          clearTimeout(timeoutId);
          reject(err);
        },
        timeoutId,
      });

      const doSend = () => {
        try {
          this.sendJson(message);
        } catch (err) {
          clearTimeout(timeoutId);
          this.pendingRequests.delete(requestId);
          reject(err);
        }
      };

      if (this.isSocketOpen()) {
        doSend();
      } else {
        this.ensureOpen().then(doSend, (err) => {
          clearTimeout(timeoutId);
          this.pendingRequests.delete(requestId);
          reject(err);
        });
      }
    });
  }

  private resolvePending(requestId: string, value: unknown) {
    const pending = this.pendingRequests.get(requestId);
    if (pending) {
      this.pendingRequests.delete(requestId);
      pending.resolve(value);
    }
  }

  private rejectPending(requestId: string, error: unknown) {
    const pending = this.pendingRequests.get(requestId);
    if (pending) {
      this.pendingRequests.delete(requestId);
      pending.reject(error);
    }
  }

  private rejectAllPending(error: Error) {
    for (const [, pending] of this.pendingRequests) {
      if (pending.timeoutId) clearTimeout(pending.timeoutId);
      pending.reject(error);
    }
    this.pendingRequests.clear();
  }

  // -------------------------------------------------------------------------
  // Event Listeners
  // -------------------------------------------------------------------------

  onHello(callback: (hello: BrowserHelloMessage) => void): () => void {
    if (this.cachedHello) {
      callback(this.cachedHello);
    }
    this.helloListeners.add(callback);
    return () => this.helloListeners.delete(callback);
  }

  onFrame(callback: (frame: BrowserFrame) => void): () => void {
    this.frameListeners.add(callback);
    return () => this.frameListeners.delete(callback);
  }

  onState(callback: (state: BrowserStateMessage) => void): () => void {
    this.stateListeners.add(callback);
    return () => this.stateListeners.delete(callback);
  }

  onDriverChanged(callback: (msg: BrowserDriverChangedMessage) => void): () => void {
    this.driverChangedListeners.add(callback);
    return () => this.driverChangedListeners.delete(callback);
  }

  onError(callback: (err: Error | BrowserErrorMessage) => void): () => void {
    this.errorListeners.add(callback);
    return () => this.errorListeners.delete(callback);
  }

  onClose(callback: () => void): () => void {
    this.closeListeners.add(callback);
    return () => this.closeListeners.delete(callback);
  }

  // -------------------------------------------------------------------------
  // Protocol Operations
  // -------------------------------------------------------------------------

  waitForHello(timeoutMs = 10000): Promise<BrowserHelloMessage> {
    if (this.cachedHello) return Promise.resolve(this.cachedHello);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        unsub();
        reject(new Error(`Timed out waiting for browserHello after ${timeoutMs}ms`));
      }, timeoutMs);
      const unsub = this.onHello((hello) => {
        clearTimeout(timer);
        unsub();
        resolve(hello);
      });
    });
  }

  subscribe(
    options: BrowserSubscribeOptions,
    viewerInstanceId = `viewer-${Math.random().toString(36).slice(2)}`,
    timeoutMs = 10000,
  ): Promise<BrowserSubscribedMessage> {
    return this.requestResponse<BrowserSubscribedMessage>(
      (requestId) => ({
        type: "browserSubscribe",
        requestId,
        viewerInstanceId,
        options,
      }),
      timeoutMs,
    );
  }

  unsubscribe(timeoutMs = 10000): Promise<BrowserUnsubscribedMessage> {
    const subscriptionId = this.currentSubscription?.subscriptionId;
    if (!subscriptionId) {
      return Promise.reject(new Error("Cannot unsubscribe: no active subscription"));
    }
    return this.requestResponse<BrowserUnsubscribedMessage>(
      (requestId) => ({
        type: "browserUnsubscribe",
        requestId,
        subscriptionId,
      }),
      timeoutMs,
    );
  }

  ackFrame(streamId: number, seq: number): void {
    if (!this.isSocketOpen() || this.isClosed) return;
    try {
      this.sendJson({
        type: "browserFrameAck",
        streamId,
        seq,
      });
    } catch {
      // Ignored if socket closed during send
    }
  }

  heartbeat(leaseEpoch?: string, timeoutMs = 5000): Promise<BrowserPongMessage> {
    return this.requestResponse<BrowserPongMessage>(
      (requestId) => ({
        type: "browserHeartbeat",
        requestId,
        leaseEpoch,
        subscriptionId: this.currentSubscription?.subscriptionId,
      }),
      timeoutMs,
    );
  }

  claimDriver(browserId: string, subscriptionId: string, timeoutMs = 10000): Promise<BrowserDriverClaimedMessage> {
    return this.requestResponse<BrowserDriverClaimedMessage>(
      (requestId) => ({
        type: "browserDriverClaim",
        requestId,
        subscriptionId,
        browserId,
      }),
      timeoutMs,
    );
  }

  releaseDriver(leaseEpoch: string, timeoutMs = 10000): Promise<BrowserDriverReleasedMessage> {
    return this.requestResponse<BrowserDriverReleasedMessage>(
      (requestId) => ({
        type: "browserDriverRelease",
        requestId,
        leaseEpoch,
      }),
      timeoutMs,
    );
  }

  sendCommand(request: BrowserCommandRequest, timeoutMs = 15000): Promise<any> {
    this.requestCounter += 1;
    const requestSeq = this.requestCounter.toString();

    return this.requestResponse<any>(
      (requestId) => ({
        type: "browserCommand",
        requestId,
        requestSeq,
        browserId: request.browserId,
        leaseEpoch: request.leaseEpoch,
        browserInstanceId: request.browserInstanceId,
        desktopEpoch: request.desktopEpoch,
        documentGeneration: request.documentGeneration,
        command: request.command,
        params: request.params,
      }),
      timeoutMs,
    );
  }

  close() {
    this.isClosed = true;
    this.ws.close();
    this.rejectAllPending(new Error("BrowserClient closed"));
  }
}
