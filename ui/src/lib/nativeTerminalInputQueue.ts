export class NativeTerminalQueueOverflowError extends Error {
  constructor(message = "Terminal input queue overflow: limit exceeded") {
    super(message);
    this.name = "NativeTerminalQueueOverflowError";
  }
}

export class NativeTerminalStaleGenerationError extends Error {
  constructor(message = "Terminal input belongs to an old connection generation") {
    super(message);
    this.name = "NativeTerminalStaleGenerationError";
  }
}

interface QueuedItem<T = unknown> {
  readonly id: number;
  readonly generation: number | null;
  readonly bytes: number;
  readonly execute: () => Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (error: unknown) => void;
}

interface SessionQueueState {
  items: QueuedItem[];
  allocatedBytes: number;
  allocatedEntries: number;
  running: boolean;
  activeGeneration: number | null;
}

export interface NativeTerminalInputQueueOptions {
  readonly maxQueueBytes?: number;
  readonly maxQueueEntries?: number;
}

const DEFAULT_MAX_QUEUE_BYTES = 256 * 1024;
// Mirror the daemon's per-session admission bound (MAX_PENDING_OPERATIONS = 17,
// which counts in-flight and queued ops together). Buffering far more entries
// locally would only turn entries 18+ into explicit Busy rejections downstream.
const DEFAULT_MAX_QUEUE_ENTRIES = 17;

export class NativeTerminalInputQueueManager {
  private readonly maxQueueBytes: number;
  private readonly maxQueueEntries: number;
  private readonly sessions = new Map<string, SessionQueueState>();
  private nextItemId = 1;

  constructor(options?: NativeTerminalInputQueueOptions) {
    this.maxQueueBytes = options?.maxQueueBytes ?? DEFAULT_MAX_QUEUE_BYTES;
    this.maxQueueEntries = options?.maxQueueEntries ?? DEFAULT_MAX_QUEUE_ENTRIES;
  }

  public getQueuedBytes(sessionId: string): number {
    return this.sessions.get(sessionId)?.allocatedBytes ?? 0;
  }

  public getAllocatedBytes(sessionId: string): number {
    return this.getQueuedBytes(sessionId);
  }

  public getQueuedCount(sessionId: string): number {
    return this.sessions.get(sessionId)?.allocatedEntries ?? 0;
  }

  public isRunning(sessionId: string): boolean {
    return this.sessions.get(sessionId)?.running ?? false;
  }

  public invalidateOldGenerations(sessionId: string, currentGeneration: number): void {
    const state = this.sessions.get(sessionId);
    if (!state) {
      this.sessions.set(sessionId, {
        items: [],
        allocatedBytes: 0,
        allocatedEntries: 0,
        running: false,
        activeGeneration: currentGeneration,
      });
      return;
    }

    state.activeGeneration = currentGeneration;
    if (state.items.length === 0) return;

    const remaining: QueuedItem[] = [];
    for (const item of state.items) {
      if (item.generation !== null && item.generation < currentGeneration) {
        state.allocatedBytes = Math.max(0, state.allocatedBytes - item.bytes);
        state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
        item.reject(new NativeTerminalStaleGenerationError());
      } else {
        remaining.push(item);
      }
    }
    state.items = remaining;
  }

  public clear(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state) return;

    const items = state.items;
    state.items = [];

    for (const item of items) {
      state.allocatedBytes = Math.max(0, state.allocatedBytes - item.bytes);
      state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
      item.reject(new Error("Terminal input queue cleared"));
    }
  }

  public resetForTest(): void {
    for (const sessionId of Array.from(this.sessions.keys())) {
      this.clear(sessionId);
    }
    this.sessions.clear();
  }

  public enqueue<T>(
    sessionId: string,
    generation: number | null,
    payloadBytes: number,
    operation: () => Promise<T>,
  ): Promise<T> {
    let state = this.sessions.get(sessionId);
    if (!state) {
      state = {
        items: [],
        allocatedBytes: 0,
        allocatedEntries: 0,
        running: false,
        activeGeneration: null,
      };
      this.sessions.set(sessionId, state);
    }

    if (
      generation !== null &&
      state.activeGeneration !== null &&
      generation < state.activeGeneration
    ) {
      return Promise.reject(new NativeTerminalStaleGenerationError());
    }

    const boundedBytes = Math.max(1, payloadBytes);
    if (
      state.allocatedEntries >= this.maxQueueEntries ||
      state.allocatedBytes + boundedBytes > this.maxQueueBytes
    ) {
      return Promise.reject(new NativeTerminalQueueOverflowError());
    }

    state.allocatedBytes += boundedBytes;
    state.allocatedEntries += 1;

    return new Promise<T>((resolve, reject) => {
      const item: QueuedItem<T> = {
        id: this.nextItemId++,
        generation,
        bytes: boundedBytes,
        execute: operation,
        resolve,
        reject,
      };

      state.items.push(item as QueuedItem);
      this.pump(sessionId);
    });
  }

  private pump(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state || state.running) return;

    const item = state.items.shift();
    if (!item) return;

    state.running = true;

    void (async () => {
      try {
        if (
          item.generation !== null &&
          state.activeGeneration !== null &&
          item.generation < state.activeGeneration
        ) {
          item.reject(new NativeTerminalStaleGenerationError());
        } else {
          try {
            const result = await item.execute();
            item.resolve(result);
          } catch (error: unknown) {
            item.reject(error);
          }
        }
      } finally {
        state.allocatedBytes = Math.max(0, state.allocatedBytes - item.bytes);
        state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
        state.running = false;
        this.pump(sessionId);
      }
    })();
  }
}

export const terminalInputQueue = new NativeTerminalInputQueueManager();
