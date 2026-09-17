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
  type BrowserSnapshotServerMessage,
  type BrowserStateMessage,
  type BrowserSubscribedMessage,
  type BrowserSubscribeOptions,
  type BrowserUnsubscribedMessage,
  type ClientMessage,
  type ServerBrowserDriverRevoked,
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

export enum MessagePriority {
  CONTROL = 0, // High: heartbeats, driver claims/releases, subscriptions
  COMMAND = 1, // Normal: automation commands, snapshots
  ACK = 2,     // Low: frame acknowledgments
}

interface OutgoingWriterMessage {
  id: string;
  priority: MessagePriority;
  text: string;
  requiresSubscription: boolean;
  streamId?: number;
  seq?: number;
}

const MAX_QUEUE_CAPACITY = 64;

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
  private driverRevokedListeners = new Set<(msg: ServerBrowserDriverRevoked) => void>();
  private errorListeners = new Set<(err: Error | BrowserErrorMessage) => void>();
  private closeListeners = new Set<() => void>();

  private cachedHello: BrowserHelloMessage | null = null;
  private currentSubscription: BrowserSubscribedMessage | null = null;
  private currentDriverState: "viewing" | "driving" = "viewing";
  private isClosed = false;
  private isClosing = false;
  private isSubscribing = false;
  private subscribedBarrier = false;
  private writerQueue: OutgoingWriterMessage[] = [];

  get driverState(): "viewing" | "driving" {
    return this.currentDriverState;
  }

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
      this.subscribedBarrier = false;
      this.writerQueue = [];
      this.currentDriverState = "viewing";
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
        this.pumpWriter();
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
          this.isSubscribing = false;
          this.subscribedBarrier = true;
          this.resolvePending(msg.requestId, msg);
          this.pumpWriter();
          break;
        }
        case "browserPong": {
          if (msg.requestId) {
            this.resolvePending(msg.requestId, msg);
          }
          break;
        }
        case "browserDriverClaimed": {
          this.currentDriverState = "driving";
          this.resolvePending(msg.requestId, msg);
          break;
        }
        case "browserDriverChanged": {
          this.currentDriverState = msg.isDriver ? "driving" : "viewing";
          for (const listener of this.driverChangedListeners) {
            listener(msg);
          }
          break;
        }
        case "browserDriverReleased": {
          this.currentDriverState = "viewing";
          this.resolvePending(msg.requestId, msg);
          break;
        }
        case "browserDriverRevoked": {
          this.currentDriverState = "viewing";
          for (const listener of this.driverRevokedListeners) {
            listener(msg);
          }
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
          this.isSubscribing = false;
          this.subscribedBarrier = false;
          this.resolvePending(msg.requestId, msg);
          break;
        }
        case "browserSnapshot": {
          this.resolvePending(msg.requestId, msg);
          break;
        }
      }
    }
  }

  private isPumping = false;

  private enqueueMessage(msg: OutgoingWriterMessage): void {
    if (this.isClosed || this.isClosing) {
      throw new Error("Cannot send message: WebSocket is closed");
    }

    // Coalesce frame ACKs: latest-only per streamId (R4-14)
    if (msg.priority === MessagePriority.ACK && msg.streamId !== undefined && msg.seq !== undefined) {
      const existingAckIdx = this.writerQueue.findIndex(
        (m) => m.priority === MessagePriority.ACK && m.streamId === msg.streamId
      );
      if (existingAckIdx !== -1) {
        const existing = this.writerQueue[existingAckIdx];
        if (existing.seq === undefined || msg.seq >= existing.seq) {
          this.writerQueue[existingAckIdx] = msg;
          return;
        } else {
          return; // Drop older ACK
        }
      }
    }

    // Check bounded queue capacity & enforce backpressure on non-control messages (R4-14)
    const nonControlCount = this.writerQueue.filter((m) => m.priority !== MessagePriority.CONTROL).length;
    const inFlightCount = this.pendingRequests.size;
    if (
      msg.priority !== MessagePriority.CONTROL &&
      (nonControlCount >= MAX_QUEUE_CAPACITY || inFlightCount >= MAX_QUEUE_CAPACITY)
    ) {
      throw new Error("BrowserClient queue full: backpressure limit exceeded");
    }

    // Priority insertion: maintain queue order (CONTROL -> COMMAND -> ACK) (R4-14)
    let insertIdx = this.writerQueue.length;
    for (let i = 0; i < this.writerQueue.length; i++) {
      if (this.writerQueue[i].priority > msg.priority) {
        insertIdx = i;
        break;
      }
    }
    this.writerQueue.splice(insertIdx, 0, msg);

    this.pumpWriter();
  }

  private pumpWriter(): void {
    if (this.isClosed || !this.isSocketOpen() || this.isPumping) {
      return;
    }

    this.isPumping = true;
    try {
      while (this.writerQueue.length > 0 && this.isSocketOpen() && !this.isClosed) {
        // Find highest-priority message eligible to send:
        // Subscribed barrier holds subscription-dependent messages until browserSubscribed confirms (R4-14)
        let eligibleIdx = -1;
        for (let i = 0; i < this.writerQueue.length; i++) {
          const item = this.writerQueue[i];
          if (item.priority === MessagePriority.CONTROL) {
            eligibleIdx = i;
            break;
          }
          if (item.requiresSubscription && this.isSubscribing && !this.subscribedBarrier) {
            continue;
          }
          eligibleIdx = i;
          break;
        }

        if (eligibleIdx === -1) {
          // Waiting behind subscribed-barrier
          break;
        }

        const [msg] = this.writerQueue.splice(eligibleIdx, 1);
        try {
          this.ws.send(msg.text);
        } catch (err) {
          const error = err instanceof Error ? err : new Error(String(err));
          for (const listener of this.errorListeners) {
            listener(error);
          }
          break;
        }
      }
    } finally {
      this.isPumping = false;
    }
  }

  private sendJson(
    message: ClientMessage,
    priority: MessagePriority = MessagePriority.COMMAND,
    requiresSubscription = false
  ) {
    if (this.isClosed || this.isClosing) {
      throw new Error("Cannot send message: WebSocket is closed");
    }
    const text = serializeClientMessage(message);
    this.enqueueMessage({
      id: `raw-${Date.now()}-${Math.random().toString(36).slice(2)}`,
      priority,
      text,
      requiresSubscription,
    });
  }

  private requestResponse<T>(
    buildMessage: (requestId: string) => ClientMessage,
    priority: MessagePriority = MessagePriority.COMMAND,
    requiresSubscription = false,
    timeoutMs = 10000,
  ): Promise<T> {
    const requestId = this.nextRequestId();
    const message = buildMessage(requestId);
    const text = serializeClientMessage(message);

    return new Promise<T>((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        this.pendingRequests.delete(requestId);
        this.writerQueue = this.writerQueue.filter((m) => m.id !== requestId);
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

      const doEnqueue = () => {
        try {
          this.enqueueMessage({
            id: requestId,
            priority,
            text,
            requiresSubscription,
          });
        } catch (err) {
          clearTimeout(timeoutId);
          this.pendingRequests.delete(requestId);
          reject(err);
        }
      };

      if (this.isSocketOpen()) {
        doEnqueue();
      } else {
        this.ensureOpen().then(doEnqueue, (err) => {
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

  onDriverRevoked(callback: (msg: ServerBrowserDriverRevoked) => void): () => void {
    this.driverRevokedListeners.add(callback);
    return () => this.driverRevokedListeners.delete(callback);
  }

  on(event: "driverRevoked", callback: (msg: ServerBrowserDriverRevoked) => void): () => void;
  on(event: "driverChanged", callback: (msg: BrowserDriverChangedMessage) => void): () => void;
  on(event: "frame", callback: (frame: BrowserFrame) => void): () => void;
  on(event: "state", callback: (state: BrowserStateMessage) => void): () => void;
  on(event: "hello", callback: (hello: BrowserHelloMessage) => void): () => void;
  on(event: "error", callback: (err: Error | BrowserErrorMessage) => void): () => void;
  on(event: "close", callback: () => void): () => void;
  on(event: string, callback: (...args: any[]) => void): () => void {
    switch (event) {
      case "driverRevoked":
        return this.onDriverRevoked(callback as (msg: ServerBrowserDriverRevoked) => void);
      case "driverChanged":
        return this.onDriverChanged(callback as (msg: BrowserDriverChangedMessage) => void);
      case "frame":
        return this.onFrame(callback as (frame: BrowserFrame) => void);
      case "state":
        return this.onState(callback as (state: BrowserStateMessage) => void);
      case "hello":
        return this.onHello(callback as (hello: BrowserHelloMessage) => void);
      case "error":
        return this.onError(callback as (err: Error | BrowserErrorMessage) => void);
      case "close":
        return this.onClose(callback as () => void);
      default:
        return () => {};
    }
  }

  emit(event: "driverRevoked", msg: ServerBrowserDriverRevoked): void;
  emit(event: string, ...args: any[]): void {
    if (event === "driverRevoked") {
      this.currentDriverState = "viewing";
      for (const listener of this.driverRevokedListeners) {
        listener(args[0]);
      }
    }
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
    this.isSubscribing = true;
    return this.requestResponse<BrowserSubscribedMessage>(
      (requestId) => ({
        type: "browserSubscribe",
        requestId,
        viewerInstanceId,
        options,
      }),
      MessagePriority.CONTROL,
      false,
      timeoutMs,
    ).catch((err) => {
      this.isSubscribing = false;
      this.pumpWriter();
      throw err;
    });
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
      MessagePriority.CONTROL,
      false,
      timeoutMs,
    );
  }

  ackFrame(streamId: number, seq: number): void {
    if (this.isClosed || this.isClosing) return;
    try {
      const text = serializeClientMessage({
        type: "browserFrameAck",
        streamId,
        seq,
      });
      this.enqueueMessage({
        id: `ack-${streamId}-${seq}`,
        priority: MessagePriority.ACK,
        text,
        requiresSubscription: true,
        streamId,
        seq,
      });
    } catch {
      // Ignored if socket closed during send or queue full
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
      MessagePriority.CONTROL,
      false,
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
      MessagePriority.CONTROL,
      false,
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
      MessagePriority.CONTROL,
      false,
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
      MessagePriority.COMMAND,
      true,
      timeoutMs,
    );
  }

  takeSnapshot(browserId: string, timeoutMs = 10000): Promise<BrowserSnapshotServerMessage> {
    return this.requestResponse<BrowserSnapshotServerMessage>(
      (requestId) => ({
        type: "browserSnapshot",
        requestId,
        browserId,
      }),
      MessagePriority.COMMAND,
      true,
      timeoutMs,
    );
  }

  pause(browserId: string, streamId: number): void {
    if (this.isClosed || this.isClosing) return;
    try {
      this.sendJson(
        {
          type: "browserPause",
          browserId,
          streamId,
        },
        MessagePriority.CONTROL,
        true,
      );
    } catch {
      // Ignored if socket closed
    }
  }

  resume(browserId: string, streamId: number): void {
    if (this.isClosed || this.isClosing) return;
    try {
      this.sendJson(
        {
          type: "browserResume",
          browserId,
          streamId,
        },
        MessagePriority.CONTROL,
        true,
      );
    } catch {
      // Ignored if socket closed
    }
  }

  async close(timeoutMs = 1000): Promise<void> {
    if (this.isClosed) return;
    this.isClosing = true;

    // Bounded join: drain remaining high-priority control messages within timeoutMs (R4-14)
    if (this.isSocketOpen() && timeoutMs > 0) {
      const deadline = Date.now() + timeoutMs;
      while (
        this.writerQueue.some((m) => m.priority === MessagePriority.CONTROL) &&
        Date.now() < deadline
      ) {
        this.pumpWriter();
        await new Promise((r) => setTimeout(r, 10));
      }
    }

    this.isClosed = true;
    this.subscribedBarrier = false;
    this.writerQueue = [];
    try {
      this.ws.close();
    } catch {
      // Ignored
    }
    this.rejectAllPending(new Error("BrowserClient closed during shutdown"));
  }
}
