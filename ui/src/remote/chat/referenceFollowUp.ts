/**
 * Target-scoped follow-ups admitted while an agent turn is still running (plan T10).
 *
 * Persistence uses the durable draft key (host + backend session). Owner and epoch are stored
 * on each row and are a fence, not an identity: a live target whose owner or epoch differs
 * drops the rows. Nothing here posts. A caller delivers one row, in order, through the same
 * single-begin path, and an unknown outcome stays unknown. A reload of a delivering row is
 * unknown too, so a restart cannot invent a second request.
 */
import { referenceDraftKey, type ReferenceTargetRef } from "./referenceTypes";

export const REFERENCE_FOLLOW_UP_STORAGE_PREFIX = "ferryx.referenceChat.followUp:";

export const FOLLOW_UP_DROPPED_NOTICE =
  "Queued messages were dropped because this session changed owner.";

export type FollowUpPhase = "queued" | "delivering" | "unknown";

export interface FollowUpMessage {
  readonly id: string;
  readonly text: string;
  readonly ownerId: string;
  readonly epoch: string;
  readonly phase: FollowUpPhase;
  readonly requestId: string | null;
}

type FollowUpStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

const newId = () =>
  globalThis.crypto?.randomUUID?.() ??
  `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

function isPhase(value: unknown): value is FollowUpPhase {
  return value === "queued" || value === "delivering" || value === "unknown";
}

export class ReferenceFollowUpStore {
  private saved = new Map<string, string | null>();
  private unsaved = new Set<string>();
  private foreign = new Set<string>();
  private rows = new Map<string, FollowUpMessage[]>();
  private listeners = new Set<() => void>();

  constructor(private storage: () => FollowUpStorage = () => window.localStorage) {}

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private notify(): void {
    for (const listener of this.listeners) listener();
  }

  storageKey(target: ReferenceTargetRef): string {
    return REFERENCE_FOLLOW_UP_STORAGE_PREFIX + referenceDraftKey(target);
  }

  read(target: ReferenceTargetRef): readonly FollowUpMessage[] {
    return this.readKey(referenceDraftKey(target));
  }

  heldUnknown(target: ReferenceTargetRef): boolean {
    return this.read(target).some((row) => row.phase === "unknown");
  }

  refreshStorageKey(storageKey: string): void {
    if (!storageKey.startsWith(REFERENCE_FOLLOW_UP_STORAGE_PREFIX)) return;
    const key = storageKey.slice(REFERENCE_FOLLOW_UP_STORAGE_PREFIX.length);
    if (this.unsaved.has(key)) return;
    this.rows.delete(key);
    this.readKey(key);
    this.notify();
  }

  /** Re-read storage when another tab, or a remount, changed the durable row. */
  refresh(target: ReferenceTargetRef): void {
    const key = referenceDraftKey(target);
    if (this.unsaved.has(key)) return;
    let raw: string | null = null;
    try {
      raw = this.storage().getItem(this.storageKey(target));
    } catch {
      return;
    }
    if (this.rows.has(key) && this.saved.get(key) === raw) return;
    this.rows.delete(key);
    this.readKey(key);
    this.notify();
  }

  /** Explicit admission. One new row; nothing is posted. */
  admit(target: ReferenceTargetRef, text: string): FollowUpMessage {
    const key = referenceDraftKey(target);
    const message: FollowUpMessage = {
      id: newId(),
      text,
      ownerId: target.target.ownerId,
      epoch: target.target.epoch,
      phase: "queued",
      requestId: null,
    };
    this.write(key, [...this.read(target), message]);
    return message;
  }

  /** Cancel only before delivery. A delivering or unknown row stays. */
  cancel(target: ReferenceTargetRef, id: string): boolean {
    const key = referenceDraftKey(target);
    const current = this.read(target);
    const row = current.find((message) => message.id === id);
    if (!row || row.phase !== "queued") return false;
    this.write(
      key,
      current.filter((message) => message.id !== id),
    );
    return true;
  }

  /**
   * Begin the oldest queued row. Null when one is already delivering, any row is unknown,
   * or the queue is empty. The request id is allocated once, inside this begin.
   */
  beginDelivery(target: ReferenceTargetRef, allocateRequestId: () => string): FollowUpMessage | null {
    const key = referenceDraftKey(target);
    const current = this.read(target);
    if (current.some((row) => row.phase === "delivering" || row.phase === "unknown")) return null;
    const next = current.find((row) => row.phase === "queued");
    if (!next) return null;
    const started: FollowUpMessage = {
      ...next,
      phase: "delivering",
      requestId: next.requestId ?? allocateRequestId(),
    };
    this.write(
      key,
      current.map((row) => (row.id === next.id ? started : row)),
    );
    return started;
  }

  complete(target: ReferenceTargetRef, id: string): void {
    const key = referenceDraftKey(target);
    const current = this.read(target);
    if (!current.some((row) => row.id === id)) return;
    this.write(
      key,
      current.filter((row) => row.id !== id),
    );
  }

  holdUnknown(target: ReferenceTargetRef, id: string): void {
    const key = referenceDraftKey(target);
    const current = this.read(target);
    if (!current.some((row) => row.id === id)) return;
    this.write(
      key,
      current.map((row) => (row.id === id ? { ...row, phase: "unknown" } : row)),
    );
  }

  /**
   * Drop rows admitted under a different owner or epoch. Same-fence rows stay, including
   * an unknown one, which must not be delivered again.
   */
  dropStale(target: ReferenceTargetRef): readonly FollowUpMessage[] {
    const key = referenceDraftKey(target);
    const current = this.read(target);
    const dropped = current.filter(
      (row) => row.ownerId !== target.target.ownerId || row.epoch !== target.target.epoch,
    );
    if (dropped.length === 0) return dropped;
    this.write(
      key,
      current.filter((row) => !dropped.some((item) => item.id === row.id)),
    );
    return dropped;
  }

  private readKey(key: string): FollowUpMessage[] {
    const cached = this.rows.get(key);
    if (cached) return cached;
    let raw: string | null = null;
    try {
      raw = this.storage().getItem(REFERENCE_FOLLOW_UP_STORAGE_PREFIX + key);
    } catch {
      raw = null;
    }
    const parsed = parseFollowUps(raw);
    if (parsed === null) {
      this.foreign.add(key);
      this.saved.set(key, raw);
      this.rows.set(key, []);
      return [];
    }
    this.foreign.delete(key);
    const revived = parsed.map((row) =>
      row.phase === "delivering" ? { ...row, phase: "unknown" as const } : row,
    );
    this.saved.set(key, raw);
    this.rows.set(key, revived);
    if (revived.some((row, index) => row.phase !== parsed[index]?.phase)) {
      this.write(key, revived);
    }
    return this.rows.get(key) ?? revived;
  }

  private write(key: string, messages: FollowUpMessage[]): void {
    this.rows.set(key, messages);
    if (this.foreign.has(key)) {
      this.unsaved.add(key);
      this.notify();
      return;
    }
    try {
      const storageKey = REFERENCE_FOLLOW_UP_STORAGE_PREFIX + key;
      const raw = messages.length === 0 ? null : JSON.stringify({ version: 1, messages });
      if (raw === null) this.storage().removeItem(storageKey);
      else this.storage().setItem(storageKey, raw);
      this.saved.set(key, raw);
      this.unsaved.delete(key);
    } catch {
      this.unsaved.add(key);
    }
    this.notify();
  }
}

function parseFollowUps(raw: string | null): FollowUpMessage[] | null {
  if (raw === null) return [];
  try {
    const data = JSON.parse(raw) as unknown;
    if (data === null || typeof data !== "object") return null;
    const record = data as { version?: unknown; messages?: unknown };
    if (record.version !== 1 || !Array.isArray(record.messages)) return null;
    const ids = new Set<string>();
    const messages: FollowUpMessage[] = [];
    for (const item of record.messages) {
      if (!item || typeof item !== "object") continue;
      const row = item as FollowUpMessage;
      if (typeof row.id !== "string" || typeof row.text !== "string" || ids.has(row.id)) continue;
      if (typeof row.ownerId !== "string" || typeof row.epoch !== "string" || !isPhase(row.phase)) continue;
      if (row.requestId !== null && typeof row.requestId !== "string") continue;
      ids.add(row.id);
      messages.push({
        id: row.id,
        text: row.text,
        ownerId: row.ownerId,
        epoch: row.epoch,
        phase: row.phase,
        requestId: row.requestId,
      });
    }
    return messages;
  } catch {
    return null;
  }
}

export const referenceFollowUps = new ReferenceFollowUpStore();

if (typeof window !== "undefined") {
  const onStorage = (event: StorageEvent) => {
    if (event.key) referenceFollowUps.refreshStorageKey(event.key);
  };
  window.addEventListener("storage", onStorage);
  import.meta.hot?.dispose(() => window.removeEventListener("storage", onStorage));
}
