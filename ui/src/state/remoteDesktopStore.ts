import { useSyncExternalStore } from "react";

import {
  RdInputSender,
  rdConnect,
  rdDisconnect,
  rdInput,
  type RdConnectionState,
  type RdFrameMessage,
  type RdInputEvent,
} from "../lib/remoteDesktop";
import { toIpcError } from "../lib/tauri";

/** Viewer lifecycle of one paired host's remote desktop. */
export type RemoteDesktopStatus = "idle" | "connecting" | "connected" | "error";

export type RemoteDesktopError = { code: string; message: string };

export type RemoteDesktopSession = {
  readonly hostId: string;
  readonly label: string;
  /** The native viewer session; null until `cmd_rd_connect` resolves and after it ends. */
  readonly rdSessionId: string | null;
  readonly status: RemoteDesktopStatus;
  /** Present only while `status` is "error"; matched by structured code, never by message text. */
  readonly error?: RemoteDesktopError;
  /** Last state pushed by the native frame channel; display detail behind `status`. */
  readonly wireState: RdConnectionState | null;
  readonly reason: string | null;
};

export type RemoteDesktopState = {
  readonly sessions: Readonly<Record<string, RemoteDesktopSession>>;
};

export type RemoteDesktopFrameHandler = (message: RdFrameMessage) => void;

export type RemoteDesktopServices = {
  connect: typeof rdConnect;
  disconnect: typeof rdDisconnect;
  input: typeof rdInput;
};

export type RemoteDesktopStore = {
  getState: () => RemoteDesktopState;
  subscribe: (listener: (state: RemoteDesktopState) => void) => () => void;
  /**
   * Opens the host's session unless a connected or connecting one already exists; a second caller
   * (another pane, a remount) attaches to that same session instead of dialing again.
   */
  connect: (hostId: string, label: string) => Promise<void>;
  /** The only path that calls `cmd_rd_disconnect`. */
  disconnect: (hostId: string) => Promise<void>;
  list: () => RemoteDesktopSession[];
  getSession: (hostId: string) => RemoteDesktopSession | null;
  /** Attaches a canvas to the session's frame channel; frames fan out to every subscriber. */
  subscribeFrames: (hostId: string, handler: RemoteDesktopFrameHandler) => () => void;
  sendInput: (hostId: string, event: RdInputEvent) => void;
  reset: () => void;
};

const INITIAL_STATE: RemoteDesktopState = { sessions: {} };

const DEFAULT_SERVICES: RemoteDesktopServices = {
  connect: rdConnect,
  disconnect: rdDisconnect,
  input: rdInput,
};

type SessionRecord = {
  snapshot: RemoteDesktopSession;
  subscribers: Set<RemoteDesktopFrameHandler>;
  sender: RdInputSender | null;
  /** Bumped on every connect/disconnect so a late `cmd_rd_connect` resolution is ignored. */
  attempt: number;
  pending: Promise<void> | null;
};

function statusFromWire(state: RdConnectionState): RemoteDesktopStatus {
  if (state === "connected") return "connected";
  if (state === "ended") return "idle";
  return "connecting";
}

function isSameSession(a: RemoteDesktopSession, b: RemoteDesktopSession): boolean {
  return (
    a.hostId === b.hostId &&
    a.label === b.label &&
    a.rdSessionId === b.rdSessionId &&
    a.status === b.status &&
    a.wireState === b.wireState &&
    a.reason === b.reason &&
    a.error?.code === b.error?.code &&
    a.error?.message === b.error?.message
  );
}

/**
 * Session registry for remote desktop viewers. A session (and the native frame channel behind it)
 * outlives any single pane, so switching worktrees or tabs only detaches a canvas — it never drops
 * the connection. Only `disconnect` tears a session down.
 */
export function createRemoteDesktopStore(
  services: RemoteDesktopServices = DEFAULT_SERVICES,
): RemoteDesktopStore {
  let state: RemoteDesktopState = INITIAL_STATE;
  const listeners = new Set<(state: RemoteDesktopState) => void>();
  const records = new Map<string, SessionRecord>();

  function notify(): void {
    for (const listener of listeners) listener(state);
  }

  function ensureRecord(hostId: string, label = ""): SessionRecord {
    const existing = records.get(hostId);
    if (existing) return existing;
    const record: SessionRecord = {
      snapshot: { hostId, label, rdSessionId: null, status: "idle", wireState: null, reason: null },
      subscribers: new Set(),
      sender: null,
      attempt: 0,
      pending: null,
    };
    records.set(hostId, record);
    state = { sessions: { ...state.sessions, [hostId]: record.snapshot } };
    return record;
  }

  function patchSession(hostId: string, patch: Partial<RemoteDesktopSession>): void {
    const record = records.get(hostId);
    if (!record) return;
    const next: RemoteDesktopSession = { ...record.snapshot, ...patch };
    if (isSameSession(record.snapshot, next)) return;
    record.snapshot = next;
    state = { sessions: { ...state.sessions, [hostId]: next } };
    notify();
  }

  function closeSender(record: SessionRecord): void {
    record.sender?.close();
    record.sender = null;
  }

  function isCurrent(hostId: string, record: SessionRecord, attempt: number): boolean {
    return records.get(hostId) === record && record.attempt === attempt;
  }

  function handleFrame(hostId: string, record: SessionRecord, attempt: number, message: RdFrameMessage): void {
    if (!isCurrent(hostId, record, attempt)) return;
    if (message.kind === "status") {
      if (message.state === "ended") closeSender(record);
      const ready = record.snapshot.rdSessionId !== null;
      patchSession(hostId, {
        // Input needs a live session id, so "connected" on the wire only counts once it resolved.
        status: message.state === "connected" && !ready ? "connecting" : statusFromWire(message.state),
        wireState: message.state,
        reason: message.reason,
        error: undefined,
        ...(message.state === "ended" ? { rdSessionId: null } : {}),
      });
      return;
    }
    for (const subscriber of [...record.subscribers]) subscriber(message);
  }

  function connect(hostId: string, label: string): Promise<void> {
    const record = ensureRecord(hostId, label);
    const { status } = record.snapshot;
    if (status === "connected" || status === "connecting") {
      if (label && record.snapshot.label !== label) patchSession(hostId, { label });
      return record.pending ?? Promise.resolve();
    }
    const stale = record.snapshot.rdSessionId;
    if (stale) {
      void services.disconnect(stale).catch((error: unknown) => {
        console.warn("Remote desktop disconnect failed", error);
      });
    }
    record.attempt += 1;
    const attempt = record.attempt;
    closeSender(record);
    patchSession(hostId, {
      label,
      status: "connecting",
      rdSessionId: null,
      error: undefined,
      wireState: "connecting",
      reason: null,
    });
    const started = services
      .connect(
        hostId,
        (message) => handleFrame(hostId, record, attempt, message),
        (error: unknown) => console.warn("Remote desktop frame rejected", error),
      )
      .then(
        (result) => {
          if (!isCurrent(hostId, record, attempt)) {
            // The session was torn down while dialing: never leak it on the native side.
            void services.disconnect(result.rdSessionId).catch((error: unknown) => {
              console.warn("Remote desktop disconnect failed", error);
            });
            return;
          }
          record.sender = new RdInputSender(result.rdSessionId, services.input, (error) => {
            console.warn("Remote desktop input failed", error);
          });
          patchSession(hostId, {
            rdSessionId: result.rdSessionId,
            // The wire may already have reported connected while the session id was in flight.
            ...(record.snapshot.wireState === "connected" ? { status: "connected" as const } : {}),
          });
        },
        (error: unknown) => {
          if (!isCurrent(hostId, record, attempt)) return;
          const ipc = toIpcError(error);
          patchSession(hostId, {
            status: "error",
            rdSessionId: null,
            wireState: null,
            reason: null,
            error: { code: ipc.code, message: ipc.message },
          });
        },
      )
      .finally(() => {
        if (record.pending === started) record.pending = null;
      });
    record.pending = started;
    return started;
  }

  function disconnect(hostId: string): Promise<void> {
    const record = records.get(hostId);
    if (!record) return Promise.resolve();
    record.attempt += 1;
    record.pending = null;
    closeSender(record);
    const rdSessionId = record.snapshot.rdSessionId;
    patchSession(hostId, {
      status: "idle",
      rdSessionId: null,
      wireState: null,
      reason: null,
      error: undefined,
    });
    if (!rdSessionId) return Promise.resolve();
    return services.disconnect(rdSessionId).catch((error: unknown) => {
      console.warn("Remote desktop disconnect failed", error);
    });
  }

  function subscribeFrames(hostId: string, handler: RemoteDesktopFrameHandler): () => void {
    const record = ensureRecord(hostId);
    record.subscribers.add(handler);
    return () => {
      record.subscribers.delete(handler);
    };
  }

  function sendInput(hostId: string, event: RdInputEvent): void {
    records.get(hostId)?.sender?.push(event);
  }

  function reset(): void {
    for (const record of records.values()) {
      record.attempt += 1;
      record.pending = null;
      closeSender(record);
      record.subscribers.clear();
      const rdSessionId = record.snapshot.rdSessionId;
      if (rdSessionId) {
        void services.disconnect(rdSessionId).catch((error: unknown) => {
          console.warn("Remote desktop disconnect failed", error);
        });
      }
    }
    records.clear();
    state = INITIAL_STATE;
    notify();
  }

  return {
    getState: () => state,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    connect,
    disconnect,
    list: () => Object.values(state.sessions),
    getSession: (hostId) => state.sessions[hostId] ?? null,
    subscribeFrames,
    sendInput,
    reset,
  };
}

export const remoteDesktopStore = createRemoteDesktopStore();

/** Subscribes a canvas to one host's session snapshot. */
export function useRemoteDesktopSession(hostId: string): RemoteDesktopSession | null {
  return useSyncExternalStore(
    remoteDesktopStore.subscribe,
    () => remoteDesktopStore.getSession(hostId),
    () => null,
  );
}
