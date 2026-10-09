import { switchDebug, switchDebugRunId } from "./switchDebug";

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

export type TerminalInputDropReason =
  | "dropped"
  | "outage"
  | "quarantined"
  | "overflow"
  | "stale-generation";

export type TerminalInputDropCallback = (
  reason: TerminalInputDropReason,
  total: number,
) => void;

const dropCounters = new Map<TerminalInputDropReason, number>();
const dropListeners = new Set<TerminalInputDropCallback>();

export function recordTerminalInputDrop(reason: TerminalInputDropReason): void {
  const next = (dropCounters.get(reason) ?? 0) + 1;
  dropCounters.set(reason, next);
  for (const listener of dropListeners) {
    try {
      listener(reason, next);
    } catch {
      // Drop accounting callbacks must not throw across caller boundaries.
    }
  }
}

export function getTerminalInputDropCount(reason?: TerminalInputDropReason): number {
  if (reason) return dropCounters.get(reason) ?? 0;
  let total = 0;
  for (const count of dropCounters.values()) total += count;
  return total;
}

export function getTerminalInputDropTotals(): Readonly<Record<TerminalInputDropReason, number>> {
  return {
    dropped: dropCounters.get("dropped") ?? 0,
    outage: dropCounters.get("outage") ?? 0,
    quarantined: dropCounters.get("quarantined") ?? 0,
    overflow: dropCounters.get("overflow") ?? 0,
    "stale-generation": dropCounters.get("stale-generation") ?? 0,
  };
}

export function subscribeTerminalInputDrop(callback: TerminalInputDropCallback): () => void {
  dropListeners.add(callback);
  return () => {
    dropListeners.delete(callback);
  };
}

export function resetTerminalInputDropCountsForTest(): void {
  dropCounters.clear();
  dropListeners.clear();
}

interface QueuedItem<T = unknown> {
  readonly id: number;
  readonly requestId: string;
  readonly operationName: string;
  readonly queuedAt: number;
  readonly generation: number | null;
  readonly bytes: number;
  readonly execute: (requestId: string) => Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (error: unknown) => void;
  readonly kind?: "input" | "preedit";
  readonly supersededResolvers?: Array<(value: T) => void>;
}

interface SessionQueueState {
  items: QueuedItem[];
  allocatedBytes: number;
  allocatedEntries: number;
  running: boolean;
  preeditRunning: boolean;
  runningSince: number | null;
  runningRequestId: string | null;
  activeGeneration: number | null;
  inFlightTimer?: ReturnType<typeof setTimeout> | null;
  lastHeadBlockedLogAt?: number;
}

export interface NativeTerminalInputQueueOptions {
  readonly maxQueueBytes?: number;
  readonly maxQueueEntries?: number;
}

export const SLOW_INPUT_THRESHOLD_MS = 250;

const DEFAULT_MAX_QUEUE_BYTES = 256 * 1024;
// Mirror the daemon's per-session admission bound (MAX_PENDING_OPERATIONS = 17,
// which counts in-flight and queued ops together). Buffering far more entries
// locally would only turn entries 18+ into explicit Busy rejections downstream.
const DEFAULT_MAX_QUEUE_ENTRIES = 17;

function notifySuperseded<T>(
  resolvers: Array<(value: T) => void> | undefined,
  value: T,
): void {
  if (!resolvers) return;
  for (const resolve of resolvers) {
    try {
      resolve(value);
    } catch {
      // Superseded preedit resolution errors must not disrupt pump.
    }
  }
}

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
    const s = this.sessions.get(sessionId);
    return (s?.running ?? false) || (s?.preeditRunning ?? false);
  }

  public getInFlightRequestId(sessionId: string): string | null {
    return this.sessions.get(sessionId)?.runningRequestId ?? null;
  }

  public getRunningAgeMs(sessionId: string): number | null {
    const since = this.sessions.get(sessionId)?.runningSince;
    return since == null ? null : Math.max(0, Date.now() - since);
  }

  public recordDrop(reason: TerminalInputDropReason): void {
    recordTerminalInputDrop(reason);
  }

  public getDropCount(reason?: TerminalInputDropReason): number {
    return getTerminalInputDropCount(reason);
  }

  public getDropTotals(): Readonly<Record<TerminalInputDropReason, number>> {
    return getTerminalInputDropTotals();
  }

  public subscribeDrop(callback: TerminalInputDropCallback): () => void {
    return subscribeTerminalInputDrop(callback);
  }

  private getOrCreateState(sessionId: string): SessionQueueState {
    let state = this.sessions.get(sessionId);
    if (!state) {
      state = {
        items: [],
        allocatedBytes: 0,
        allocatedEntries: 0,
        running: false,
        preeditRunning: false,
        runningSince: null,
        runningRequestId: null,
        activeGeneration: null,
      };
      this.sessions.set(sessionId, state);
    }
    return state;
  }

  public invalidateOldGenerations(sessionId: string, currentGeneration: number): void {
    const state = this.sessions.get(sessionId);
    if (!state) {
      this.sessions.set(sessionId, {
        items: [],
        allocatedBytes: 0,
        allocatedEntries: 0,
        running: false,
        preeditRunning: false,
        runningSince: null,
        runningRequestId: null,
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
        notifySuperseded(item.supersededResolvers, undefined);
      } else {
        remaining.push(item);
      }
    }
    state.items = remaining;
  }

  public clear(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state) return;

    if (state.inFlightTimer) {
      clearTimeout(state.inFlightTimer);
      state.inFlightTimer = null;
    }

    const items = state.items;
    state.items = [];

    for (const item of items) {
      state.allocatedBytes = Math.max(0, state.allocatedBytes - item.bytes);
      state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
      item.reject(new Error("Terminal input queue cleared"));
      notifySuperseded(item.supersededResolvers, undefined);
    }
  }

  public resetForTest(): void {
    for (const sessionId of Array.from(this.sessions.keys())) {
      this.clear(sessionId);
    }
    this.sessions.clear();
    resetTerminalInputDropCountsForTest();
  }

  public enqueue<T>(
    sessionId: string,
    generation: number | null,
    payloadBytes: number,
    operation: (requestId: string) => Promise<T>,
    customRequestId?: string,
    operationName = "input",
  ): Promise<T> {
    const state = this.getOrCreateState(sessionId);

    if (
      generation !== null &&
      state.activeGeneration !== null &&
      generation < state.activeGeneration
    ) {
      return Promise.reject(new NativeTerminalStaleGenerationError());
    }

    if (state.running && state.runningSince !== null) {
      const now = Date.now();
      const runningAgeMs = now - state.runningSince;
      if (
        runningAgeMs >= SLOW_INPUT_THRESHOLD_MS &&
        (!state.lastHeadBlockedLogAt || now - state.lastHeadBlockedLogAt >= 1000)
      ) {
        state.lastHeadBlockedLogAt = now;
        switchDebug("terminal.surface.input.in_flight_slow", {
          backendSessionId: sessionId,
          inFlightRequestId: state.runningRequestId,
          runningAgeMs,
          queuedEntries: state.allocatedEntries,
          queuedBytes: state.allocatedBytes,
          trigger: "enqueue_head_blocked",
        });
      }
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

    const itemId = this.nextItemId++;
    const runPrefix = switchDebugRunId.slice(0, 8);
    const requestId = customRequestId ?? `req-${runPrefix}-${sessionId}-${itemId}`;

    return new Promise<T>((resolve, reject) => {
      const item: QueuedItem<T> = {
        id: itemId,
        requestId,
        operationName,
        queuedAt: Date.now(),
        generation,
        bytes: boundedBytes,
        execute: operation,
        resolve,
        reject,
        kind: "input",
      };

      state.items.push(item as QueuedItem);
      this.pump(sessionId);
    });
  }

  public enqueuePreedit<T>(
    sessionId: string,
    generation: number | null,
    payloadBytes: number,
    operation: (requestId: string) => Promise<T>,
    customRequestId?: string,
  ): Promise<T> {
    const state = this.getOrCreateState(sessionId);

    if (
      generation !== null &&
      state.activeGeneration !== null &&
      generation < state.activeGeneration
    ) {
      return Promise.reject(new NativeTerminalStaleGenerationError());
    }

    const boundedBytes = Math.max(1, payloadBytes);
    const existingIndex = state.items.findIndex((item) => item.kind === "preedit");
    const existingItem = existingIndex !== -1 ? state.items[existingIndex] : undefined;

    const prospectiveEntries = existingItem
      ? state.allocatedEntries
      : state.allocatedEntries + 1;
    const prospectiveBytes = existingItem
      ? state.allocatedBytes - existingItem.bytes + boundedBytes
      : state.allocatedBytes + boundedBytes;

    if (
      prospectiveEntries > this.maxQueueEntries ||
      prospectiveBytes > this.maxQueueBytes
    ) {
      if (existingIndex !== -1) {
        const displaced = state.items.splice(existingIndex, 1)[0];
        if (displaced) {
          state.allocatedBytes = Math.max(0, state.allocatedBytes - displaced.bytes);
          state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
          const overflowError = new NativeTerminalQueueOverflowError();
          displaced.reject(overflowError);
          notifySuperseded(displaced.supersededResolvers, undefined);
        }
      }
      recordTerminalInputDrop("overflow");
      return Promise.reject(new NativeTerminalQueueOverflowError());
    }

    const supersededResolvers: Array<(value: unknown) => void> = [];

    if (existingIndex !== -1) {
      const removed = state.items.splice(existingIndex, 1)[0];
      if (removed) {
        state.allocatedBytes = Math.max(0, state.allocatedBytes - removed.bytes);
        state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
        supersededResolvers.push(removed.resolve);
        if (removed.supersededResolvers) {
          supersededResolvers.push(...removed.supersededResolvers);
        }
      }
    }

    state.allocatedBytes += boundedBytes;
    state.allocatedEntries += 1;

    const itemId = this.nextItemId++;
    const runPrefix = switchDebugRunId.slice(0, 8);
    const requestId = customRequestId ?? `req-preedit-${runPrefix}-${sessionId}-${itemId}`;

    return new Promise<T>((resolve, reject) => {
      const item: QueuedItem<T> = {
        id: itemId,
        requestId,
        operationName: "preedit",
        queuedAt: Date.now(),
        generation,
        bytes: boundedBytes,
        execute: operation,
        resolve,
        reject,
        kind: "preedit",
        supersededResolvers: supersededResolvers as Array<(value: T) => void>,
      };

      state.items.push(item as QueuedItem);
      this.pump(sessionId);
    });
  }

  private pump(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state || state.items.length === 0) return;

    if (state.running || state.preeditRunning) return;

    const item = state.items.shift();
    if (!item) return;

    const isInput = item.kind !== "preedit";
    if (isInput) {
      state.running = true;
    } else {
      state.preeditRunning = true;
    }
    const startedAt = Date.now();
    state.runningSince = startedAt;
    state.runningRequestId = item.requestId;

    if (state.inFlightTimer) {
      clearTimeout(state.inFlightTimer);
    }
    state.inFlightTimer = setTimeout(() => {
      if (state.runningRequestId === item.requestId && (state.running || state.preeditRunning)) {
        const runningAgeMs = Date.now() - startedAt;
        switchDebug("terminal.surface.input.in_flight_slow", {
          backendSessionId: sessionId,
          inFlightRequestId: item.requestId,
          operation: item.operationName,
          runningAgeMs,
          queuedEntries: state.allocatedEntries,
          queuedBytes: state.allocatedBytes,
          phase: "pending_ipc",
        });
      }
    }, SLOW_INPUT_THRESHOLD_MS);

    void (async () => {
      try {
        if (
          item.generation !== null &&
          state.activeGeneration !== null &&
          item.generation < state.activeGeneration
        ) {
          item.reject(new NativeTerminalStaleGenerationError());
          notifySuperseded(item.supersededResolvers, undefined);
        } else {
          try {
            const result = await item.execute(item.requestId);
            item.resolve(result);
            notifySuperseded(item.supersededResolvers, result);
          } catch (error: unknown) {
            item.reject(error);
            notifySuperseded(item.supersededResolvers, undefined);
          }
        }
      } finally {
        if (state.inFlightTimer) {
          clearTimeout(state.inFlightTimer);
          state.inFlightTimer = null;
        }
        const durationMs = Date.now() - startedAt;
        if (durationMs >= SLOW_INPUT_THRESHOLD_MS) {
          switchDebug("terminal.surface.input.slow", {
            backendSessionId: sessionId,
            requestId: item.requestId,
            operation: item.operationName,
            durationMs,
            queuedEntries: state.allocatedEntries,
            queuedBytes: state.allocatedBytes,
          });
        }
        state.allocatedBytes = Math.max(0, state.allocatedBytes - item.bytes);
        state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
        if (isInput) {
          state.running = false;
        } else {
          state.preeditRunning = false;
        }
        state.runningSince = null;
        state.runningRequestId = null;
        this.pump(sessionId);
      }
    })();
  }
}

export const terminalInputQueue = new NativeTerminalInputQueueManager();
