import { safeRandomUUID } from "./uuid";
import { switchDebug } from "./switchDebug";

const queueRunId = safeRandomUUID().replace(/-/g, "").slice(0, 8);

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

export interface InputIdentityMeta {
  readonly paneIdentity?: string | null;
  readonly bindingKey?: string | null;
  readonly attemptGeneration?: number | null;
  readonly daemonEpoch?: string | null;
  readonly incarnation?: string | null;
}

/**
 * Input lanes beside ordinary FIFO input.
 * - "paste": large bracketed-paste writes; bytes count against a separate paste budget.
 * - "control": control keys (Ctrl-C, Esc, ...); own admission budget and a serialized lane that
 *   may dispatch while a paste is in flight, so a running paste can be interrupted.
 */
export type NativeTerminalInputLane = "paste" | "control";

interface QueuedItem<T = unknown> {
  readonly id: number;
  readonly requestId: string;
  readonly operationName: string;
  readonly queuedAt: number;
  readonly generation: number | null;
  bytes: number;
  readonly execute: (requestId: string) => Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
  /** "pointer" items (wheel/mouse) are bounded separately so momentum scrolling cannot evict keystrokes. */
  readonly kind?: "input" | "preedit" | "pointer";
  readonly lane?: NativeTerminalInputLane;
  readonly supersededResolvers?: Array<(value: T) => void>;
  readonly identityMeta?: InputIdentityMeta;
  readonly recover?: (error: unknown) => Promise<boolean>;
  /** Present only for plain printable text; later text may be appended while the item is queued. */
  coalesce?: { text: string; readonly send: (text: string, requestId: string) => Promise<T> };
  dispatchedAt?: number | null;
}

interface LaneExecutionState {
  runningSince: number | null;
  runningRequestId: string | null;
  inFlightTimer?: ReturnType<typeof setTimeout> | null;
}

interface SessionQueueState {
  items: QueuedItem[];
  /** Control-lane items, in admission order. Never part of items. */
  controlItems: QueuedItem[];
  allocatedBytes: number;
  allocatedEntries: number;
  /** Bytes held by paste-lane items (queued or in flight); their entries stay in allocatedEntries. */
  pasteBytes: number;
  controlBytes: number;
  controlEntries: number;
  pointerEntries: number;
  running: boolean;
  /** True while the input lane's in-flight item is a paste. */
  inputRunningPaste: boolean;
  preeditRunning: boolean;
  controlRunning: boolean;
  inputLane: LaneExecutionState;
  preeditLane: LaneExecutionState;
  controlLane: LaneExecutionState;
  activeGeneration: number | null;
  /** Set by clear() while a lane is still running; the entry is dropped once it goes idle. */
  cleared?: boolean;
  lastHeadBlockedLogAt?: number;
}

export interface NativeTerminalInputQueueOptions {
  readonly maxQueueBytes?: number;
  readonly maxQueueEntries?: number;
  /** Byte budget for paste-lane items (one paste may use all of it). Default 512 KiB. */
  readonly maxPasteBytes?: number;
  /** Entry cap for control-lane items. Defaults to maxQueueEntries. */
  readonly maxControlEntries?: number;
}

export interface NativeTerminalEnqueueOptions {
  /** Wheel/mouse items: counted against their own entry cap, never the keystroke cap. */
  readonly pointer?: boolean;
  /**
   * "paste": FIFO with ordinary input, bytes bounded by maxPasteBytes instead of maxQueueBytes.
   * "control": admitted against its own budget (a full paste never blocks it), serialized with
   * other control keys, FIFO with ordinary input except that it may dispatch while a paste is in
   * flight. Ignored together with pointer; coalesceText is ignored when a lane is set.
   */
  readonly lane?: NativeTerminalInputLane;
  /**
   * Called when the operation fails. Resolving true re-executes the same item once while it
   * still holds the session lane, so later items cannot overtake it.
   */
  readonly recover?: (error: unknown) => Promise<boolean>;
  /**
   * Plain printable text whose bytes are identical whether written alone or appended to the
   * previous text write. When the entry cap is reached, it is appended to the queued tail item
   * (same session and generation, also carrying coalesceText) instead of being rejected.
   */
  readonly coalesceText?: {
    readonly text: string;
    readonly send: (text: string, requestId: string) => Promise<unknown>;
  };
}

export const SLOW_INPUT_THRESHOLD_MS = 250;

const DEFAULT_MAX_QUEUE_BYTES = 256 * 1024;
// Mirror the daemon's per-session admission bound (MAX_PENDING_OPERATIONS = 17,
// which counts in-flight and queued ops together). Buffering far more entries
// locally would only turn entries 18+ into explicit Busy rejections downstream.
const DEFAULT_MAX_QUEUE_ENTRIES = 17;
// One bracketed paste may be admitted whole; held outside maxQueueBytes so it never starves typing.
const DEFAULT_MAX_PASTE_BYTES = 512 * 1024;
// One coalesced text write never exceeds the backend's per-input text limit
// (MAX_NATIVE_INPUT_BYTES in src-tauri/src/native_terminal/input.rs).
const MAX_COALESCED_TEXT_BYTES = 8192;

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

function createSessionState(activeGeneration: number | null): SessionQueueState {
  return {
    items: [],
    controlItems: [],
    allocatedBytes: 0,
    allocatedEntries: 0,
    pasteBytes: 0,
    controlBytes: 0,
    controlEntries: 0,
    pointerEntries: 0,
    running: false,
    inputRunningPaste: false,
    preeditRunning: false,
    controlRunning: false,
    inputLane: { runningSince: null, runningRequestId: null },
    preeditLane: { runningSince: null, runningRequestId: null },
    controlLane: { runningSince: null, runningRequestId: null },
    activeGeneration,
  };
}

function releaseItem(state: SessionQueueState, item: QueuedItem): void {
  if (item.kind === "pointer") {
    state.pointerEntries = Math.max(0, state.pointerEntries - 1);
    return;
  }
  if (item.lane === "control") {
    state.controlBytes = Math.max(0, state.controlBytes - item.bytes);
    state.controlEntries = Math.max(0, state.controlEntries - 1);
    return;
  }
  if (item.lane === "paste") {
    state.pasteBytes = Math.max(0, state.pasteBytes - item.bytes);
  } else {
    state.allocatedBytes = Math.max(0, state.allocatedBytes - item.bytes);
  }
  state.allocatedEntries = Math.max(0, state.allocatedEntries - 1);
}

/** Earliest-admitted queued (not running) item across the input and control queues. */
function queuedHead(state: SessionQueueState): QueuedItem | undefined {
  const input = state.items[0];
  const control = state.controlItems[0];
  if (!input) return control;
  if (!control) return input;
  return control.id < input.id ? control : input;
}

export class NativeTerminalInputQueueManager {
  private readonly maxQueueBytes: number;
  private readonly maxQueueEntries: number;
  private readonly maxPasteBytes: number;
  private readonly maxControlEntries: number;
  private readonly sessions = new Map<string, SessionQueueState>();
  private nextItemId = 1;

  constructor(options?: NativeTerminalInputQueueOptions) {
    this.maxQueueBytes = options?.maxQueueBytes ?? DEFAULT_MAX_QUEUE_BYTES;
    this.maxQueueEntries = options?.maxQueueEntries ?? DEFAULT_MAX_QUEUE_ENTRIES;
    this.maxPasteBytes = options?.maxPasteBytes ?? DEFAULT_MAX_PASTE_BYTES;
    this.maxControlEntries = options?.maxControlEntries ?? this.maxQueueEntries;
  }

  /** Bytes held across ordinary, paste and control items (queued or in flight). */
  public getQueuedBytes(sessionId: string): number {
    const s = this.sessions.get(sessionId);
    return s ? s.allocatedBytes + s.pasteBytes + s.controlBytes : 0;
  }

  public getAllocatedBytes(sessionId: string): number {
    return this.getQueuedBytes(sessionId);
  }

  public getQueuedCount(sessionId: string): number {
    const s = this.sessions.get(sessionId);
    return s ? s.allocatedEntries + s.controlEntries : 0;
  }

  public isRunning(sessionId: string): boolean {
    const s = this.sessions.get(sessionId);
    return (s?.running ?? false) || (s?.preeditRunning ?? false) || (s?.controlRunning ?? false);
  }

  public getInFlightRequestId(sessionId: string): string | null {
    const s = this.sessions.get(sessionId);
    if (!s) return null;
    return (
      s.inputLane.runningRequestId ??
      s.controlLane.runningRequestId ??
      s.preeditLane.runningRequestId ??
      null
    );
  }

  public getQueuedHeadAgeMs(sessionId: string): number | null {
    const s = this.sessions.get(sessionId);
    const head = s ? queuedHead(s) : undefined;
    if (!head) return null;
    return Math.max(0, Date.now() - head.queuedAt);
  }

  public getHeadQueuedRequestId(sessionId: string): string | null {
    const s = this.sessions.get(sessionId);
    return (s ? queuedHead(s)?.requestId : undefined) ?? null;
  }

  public getRunningAgeMs(sessionId: string): number | null {
    const s = this.sessions.get(sessionId);
    if (!s) return null;
    const since = [s.inputLane, s.preeditLane, s.controlLane]
      .map((lane) => lane.runningSince)
      .filter((value): value is number => value !== null);
    return since.length === 0 ? null : Math.max(0, Date.now() - Math.min(...since));
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
      state = createSessionState(null);
      this.sessions.set(sessionId, state);
    }
    state.cleared = false;
    return state;
  }

  public invalidateOldGenerations(sessionId: string, currentGeneration: number): void {
    const state = this.sessions.get(sessionId);
    if (!state) {
      this.sessions.set(sessionId, createSessionState(currentGeneration));
      return;
    }

    state.activeGeneration = currentGeneration;
    const keepCurrent = (items: QueuedItem[]): QueuedItem[] => {
      const remaining: QueuedItem[] = [];
      for (const item of items) {
        if (item.generation !== null && item.generation < currentGeneration) {
          releaseItem(state, item);
          item.reject(new NativeTerminalStaleGenerationError());
          notifySuperseded(item.supersededResolvers, undefined);
        } else {
          remaining.push(item);
        }
      }
      return remaining;
    };
    if (state.items.length > 0) state.items = keepCurrent(state.items);
    if (state.controlItems.length > 0) state.controlItems = keepCurrent(state.controlItems);
    // Dropping stale items may unblock a queued item of the other lane.
    this.pump(sessionId);
  }

  public clear(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state) return;

    for (const lane of [state.inputLane, state.preeditLane, state.controlLane]) {
      if (lane.inFlightTimer) clearTimeout(lane.inFlightTimer);
      lane.inFlightTimer = null;
    }

    const items = [...state.items, ...state.controlItems];
    state.items = [];
    state.controlItems = [];

    for (const item of items) {
      releaseItem(state, item);
      item.reject(new Error("Terminal input queue cleared"));
      notifySuperseded(item.supersededResolvers, undefined);
    }

    if (!state.running && !state.preeditRunning && !state.controlRunning) {
      this.sessions.delete(sessionId);
    } else {
      state.cleared = true;
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
    identityMeta?: InputIdentityMeta,
    options?: NativeTerminalEnqueueOptions,
  ): Promise<T> {
    const state = this.getOrCreateState(sessionId);

    if (
      generation !== null &&
      state.activeGeneration !== null &&
      generation < state.activeGeneration
    ) {
      return Promise.reject(new NativeTerminalStaleGenerationError());
    }

    if (state.running && state.inputLane.runningSince !== null) {
      const now = Date.now();
      const runningAgeMs = now - state.inputLane.runningSince;
      if (
        runningAgeMs >= SLOW_INPUT_THRESHOLD_MS &&
        (!state.lastHeadBlockedLogAt || now - state.lastHeadBlockedLogAt >= 1000)
      ) {
        state.lastHeadBlockedLogAt = now;
        switchDebug("terminal.surface.input.in_flight_slow", {
          backendSessionId: sessionId,
          inFlightRequestId: state.inputLane.runningRequestId,
          runningAgeMs,
          queuedEntries: state.allocatedEntries,
          queuedBytes: state.allocatedBytes,
          trigger: "enqueue_head_blocked",
        });
      }
    }

    const boundedBytes = Math.max(1, payloadBytes);
    const pointer = options?.pointer === true;
    const lane = pointer ? undefined : options?.lane;
    const coalesceText = pointer || lane ? undefined : options?.coalesceText;
    if (
      coalesceText &&
      state.allocatedEntries >= this.maxQueueEntries &&
      state.allocatedBytes + boundedBytes <= this.maxQueueBytes
    ) {
      // Only the queued tail is a merge target, so a merge never reorders input. Items in
      // state.items are never running (pump shifts the head out before dispatch).
      const tail = state.items[state.items.length - 1];
      const lastControl = state.controlItems[state.controlItems.length - 1];
      if (
        tail &&
        tail.kind === "input" &&
        tail.coalesce &&
        tail.generation === generation &&
        tail.bytes + boundedBytes <= MAX_COALESCED_TEXT_BYTES &&
        // Merging into a tail admitted before a queued control key would move this text ahead of it.
        !(lastControl && lastControl.id > tail.id)
      ) {
        const target = tail as QueuedItem<T>;
        target.coalesce!.text += coalesceText.text;
        target.bytes += boundedBytes;
        state.allocatedBytes += boundedBytes;
        switchDebug("terminal.surface.input.coalesced", {
          backendSessionId: sessionId,
          operationId: target.requestId,
          payloadBytes: boundedBytes,
          coalescedBytes: target.bytes,
          queuedEntries: state.allocatedEntries,
        });
        return new Promise<T>((resolve, reject) => {
          const previousResolve = target.resolve;
          const previousReject = target.reject;
          target.resolve = (value) => {
            previousResolve(value);
            resolve(value);
          };
          target.reject = (error) => {
            previousReject(error);
            reject(error);
          };
        });
      }
    }
    const overflow = pointer
      ? state.pointerEntries >= this.maxQueueEntries
      : lane === "control"
        ? state.controlEntries >= this.maxControlEntries ||
          state.controlBytes + boundedBytes > this.maxQueueBytes
        : lane === "paste"
          ? state.allocatedEntries >= this.maxQueueEntries ||
            state.pasteBytes + boundedBytes > this.maxPasteBytes
          : state.allocatedEntries >= this.maxQueueEntries ||
            state.allocatedBytes + boundedBytes > this.maxQueueBytes;
    if (overflow) {
      return Promise.reject(new NativeTerminalQueueOverflowError());
    }

    if (pointer) {
      state.pointerEntries += 1;
    } else if (lane === "control") {
      state.controlBytes += boundedBytes;
      state.controlEntries += 1;
    } else if (lane === "paste") {
      state.pasteBytes += boundedBytes;
      state.allocatedEntries += 1;
    } else {
      state.allocatedBytes += boundedBytes;
      state.allocatedEntries += 1;
    }

    const itemId = this.nextItemId++;
    const runPrefix = queueRunId;
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
        kind: pointer ? "pointer" : "input",
        lane,
        identityMeta,
        recover: options?.recover,
        coalesce: coalesceText
          ? {
              text: coalesceText.text,
              send: coalesceText.send as (text: string, requestId: string) => Promise<T>,
            }
          : undefined,
      };

      if (lane === "control") {
        state.controlItems.push(item as QueuedItem);
      } else {
        state.items.push(item as QueuedItem);
      }
      switchDebug("terminal.surface.input.accepted", {
        operationId: requestId,
        backendSessionId: sessionId,
        lane: lane ?? (pointer ? "pointer" : "input"),
        paneIdentity: identityMeta?.paneIdentity ?? null,
        bindingKey: identityMeta?.bindingKey ?? null,
        attemptGeneration: identityMeta?.attemptGeneration ?? generation,
        daemonEpoch: identityMeta?.daemonEpoch ?? null,
        incarnation: null,
        payloadBytes: boundedBytes,
        queuedAt: item.queuedAt,
      });
      this.pump(sessionId);
    });
  }

  public enqueuePreedit<T>(
    sessionId: string,
    generation: number | null,
    payloadBytes: number,
    operation: (requestId: string) => Promise<T>,
    customRequestId?: string,
    identityMeta?: InputIdentityMeta,
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
        releaseItem(state, removed);
        supersededResolvers.push(removed.resolve);
        if (removed.supersededResolvers) {
          supersededResolvers.push(...removed.supersededResolvers);
        }
      }
    }

    state.allocatedBytes += boundedBytes;
    state.allocatedEntries += 1;

    const itemId = this.nextItemId++;
    const runPrefix = queueRunId;
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
        identityMeta,
      };

      state.items.push(item as QueuedItem);
      switchDebug("terminal.surface.input.accepted", {
        operationId: requestId,
        backendSessionId: sessionId,
        paneIdentity: identityMeta?.paneIdentity ?? null,
        bindingKey: identityMeta?.bindingKey ?? null,
        attemptGeneration: identityMeta?.attemptGeneration ?? generation,
        daemonEpoch: identityMeta?.daemonEpoch ?? null,
        incarnation: null,
        payloadBytes: boundedBytes,
        queuedAt: item.queuedAt,
      });
      this.pump(sessionId);
    });
  }

  /**
   * Ordering rules: items (input/preedit/pointer/paste) run one at a time in FIFO order and never
   * pass an earlier control key. Control keys run one at a time; each waits for every earlier
   * queued item and for in-flight non-paste input, but may dispatch while a paste is in flight.
   */
  private pump(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state) return;

    const head = state.items[0];
    if (
      head &&
      !state.running &&
      !state.preeditRunning &&
      !state.controlRunning &&
      !(state.controlItems.length > 0 && state.controlItems[0].id < head.id)
    ) {
      state.items.shift();
      this.dispatch(sessionId, state, head, head.kind === "preedit" ? "preedit" : "input");
    }

    const control = state.controlItems[0];
    if (
      control &&
      !state.controlRunning &&
      !state.preeditRunning &&
      (!state.running || state.inputRunningPaste) &&
      !state.items.some((item) => item.id < control.id)
    ) {
      state.controlItems.shift();
      this.dispatch(sessionId, state, control, "control");
    }
  }

  private setLaneRunning(
    state: SessionQueueState,
    laneKind: "input" | "preedit" | "control",
    running: boolean,
    item: QueuedItem,
  ): void {
    if (laneKind === "input") {
      state.running = running;
      state.inputRunningPaste = running && item.lane === "paste";
    } else if (laneKind === "preedit") {
      state.preeditRunning = running;
    } else {
      state.controlRunning = running;
    }
  }

  private dispatch(
    sessionId: string,
    state: SessionQueueState,
    item: QueuedItem,
    laneKind: "input" | "preedit" | "control",
  ): void {
    const lane =
      laneKind === "input"
        ? state.inputLane
        : laneKind === "preedit"
          ? state.preeditLane
          : state.controlLane;
    this.setLaneRunning(state, laneKind, true, item);
    const startedAt = Date.now();
    lane.runningSince = startedAt;
    lane.runningRequestId = item.requestId;
    item.dispatchedAt = startedAt;
    lane.inFlightTimer = setTimeout(() => {
      if (lane.runningRequestId === item.requestId) {
        switchDebug("terminal.surface.input.in_flight_slow", {
          backendSessionId: sessionId,
          inFlightRequestId: item.requestId,
          operation: item.operationName,
          runningAgeMs: Date.now() - startedAt,
          queuedEntries: state.allocatedEntries,
          queuedBytes: state.allocatedBytes,
          phase: "pending_ipc",
        });
      }
    }, SLOW_INPUT_THRESHOLD_MS);

    switchDebug("terminal.surface.input.dispatch", {
      operationId: item.requestId,
      backendSessionId: sessionId,
      paneIdentity: item.identityMeta?.paneIdentity ?? null,
      bindingKey: item.identityMeta?.bindingKey ?? null,
      attemptGeneration: item.identityMeta?.attemptGeneration ?? item.generation,
      daemonEpoch: item.identityMeta?.daemonEpoch ?? null,
      incarnation: null,
      dispatchedAt: startedAt,
    });

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
            const result = await this.executeWithRecovery(item, state);
            item.resolve(result);
            notifySuperseded(item.supersededResolvers, result);
          } catch (error: unknown) {
            item.reject(error);
            notifySuperseded(item.supersededResolvers, undefined);
          }
        }
      } finally {
        if (lane.inFlightTimer) {
          clearTimeout(lane.inFlightTimer);
          lane.inFlightTimer = null;
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
        releaseItem(state, item);
        this.setLaneRunning(state, laneKind, false, item);
        lane.runningSince = null;
        lane.runningRequestId = null;
        if (
          state.cleared &&
          !state.running &&
          !state.preeditRunning &&
          !state.controlRunning &&
          state.items.length === 0 &&
          state.controlItems.length === 0 &&
          this.sessions.get(sessionId) === state
        ) {
          this.sessions.delete(sessionId);
          return;
        }
        this.pump(sessionId);
      }
    })();
  }

  private async executeWithRecovery(item: QueuedItem, state: SessionQueueState): Promise<unknown> {
    const run = () =>
      item.coalesce ? item.coalesce.send(item.coalesce.text, item.requestId) : item.execute(item.requestId);
    try {
      return await run();
    } catch (error: unknown) {
      if (!item.recover || !(await item.recover(error))) throw error;
      if (
        item.generation !== null &&
        state.activeGeneration !== null &&
        item.generation < state.activeGeneration
      ) {
        throw new NativeTerminalStaleGenerationError();
      }
      return run();
    }
  }
}

export const terminalInputQueue = new NativeTerminalInputQueueManager();
