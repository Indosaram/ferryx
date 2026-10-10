/**
 * Held messages for the Herdr reference chat (task 8).
 *
 * Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
 * (`src/lib/messageQueue.ts`, MIT — see `docs/chat/HERDR_LICENSE`).
 *
 * The upstream queue is keyed by an opaque owner string. Ferryx keys it by the frozen
 * `referenceDraftKey(target)` from `referenceTypes.ts`, so a held row is scoped to the
 * owning host + backend session + daemon incarnation.
 *
 * Port obligations (task 8 acceptance):
 *  - held rows are *explicitly* sent: nothing here dispatches on reconnect, on a status
 *    change or on a remount — only a caller naming one row moves it;
 *  - an acknowledgement removes exactly its own row, in its own target, after the user has
 *    switched panes;
 *  - a storage failure is *visible* (`isUnsaved`) and never discards the in-memory rows.
 *
 * Pure state plus a `Storage` shim: no fetch, no timer, no DOM beyond the optional
 * `storage` listener that reconciles another tab's write before a mutation.
 */
import { referenceDraftKey, type ReferenceTargetRef } from "./referenceTypes";

/** Canonical `ferryx.*` prefix; the frozen target key is appended verbatim. */
export const REFERENCE_QUEUE_STORAGE_PREFIX = "ferryx.referenceChat.queue:";

export interface HeldMessage {
  readonly id: string;
  readonly text: string;
}

type QueueStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

const newId = () =>
  globalThis.crypto?.randomUUID?.() ??
  `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

/** Target-scoped cache also lets an ACK remove its own item after the user switches panes. */
export class ReferenceQueueStore {
  private saved = new Map<string, string | null>();
  private unsaved = new Set<string>();
  private queues = new Map<string, HeldMessage[]>();
  private listeners = new Set<() => void>();
  private pending = new Set<string>();

  constructor(private storage: () => QueueStorage = () => window.localStorage) {}

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private notify(): void {
    for (const listener of this.listeners) listener();
  }

  ownerKey(owner: ReferenceTargetRef): string {
    return referenceDraftKey(owner);
  }

  storageKey(owner: ReferenceTargetRef): string {
    return REFERENCE_QUEUE_STORAGE_PREFIX + referenceDraftKey(owner);
  }

  isUnsaved(owner: ReferenceTargetRef): boolean {
    return this.unsaved.has(referenceDraftKey(owner));
  }

  isSending(id: string): boolean {
    return this.pending.has(id);
  }

  beginSend(owner: ReferenceTargetRef, id: string): boolean {
    const key = referenceDraftKey(owner);
    if (this.pending.has(id)) return false;
    this.pending.add(id);
    this.queues.set(key, [...this.read(owner)]);
    this.notify();
    return true;
  }

  endSend(owner: ReferenceTargetRef, id: string): void {
    const key = referenceDraftKey(owner);
    this.pending.delete(id);
    this.queues.set(key, [...this.read(owner)]);
    this.notify();
  }

  /** Reconcile storage before mutation; another tab may have written since our last render. */
  refresh(owner: ReferenceTargetRef): void {
    const key = referenceDraftKey(owner);
    if (this.unsaved.has(key)) return;
    try {
      const raw = this.storage().getItem(REFERENCE_QUEUE_STORAGE_PREFIX + key);
      if (this.saved.has(key) && raw === this.saved.get(key)) return;
      this.queues.delete(key);
      this.read(owner);
      this.notify();
    } catch {
      /* keep in-memory messages when storage cannot be read */
    }
  }

  refreshStorageKey(storageKey: string): void {
    if (!storageKey.startsWith(REFERENCE_QUEUE_STORAGE_PREFIX)) return;
    const key = storageKey.slice(REFERENCE_QUEUE_STORAGE_PREFIX.length);
    if (this.unsaved.has(key)) return;
    try {
      const raw = this.storage().getItem(storageKey);
      if (this.saved.has(key) && raw === this.saved.get(key)) return;
      this.queues.delete(key);
      this.readKey(key);
      this.notify();
    } catch {
      /* keep in-memory messages when storage cannot be read */
    }
  }

  read(owner: ReferenceTargetRef): HeldMessage[] {
    return this.readKey(referenceDraftKey(owner));
  }

  add(owner: ReferenceTargetRef, text: string): HeldMessage {
    const key = referenceDraftKey(owner);
    this.refresh(owner);
    const message: HeldMessage = { id: newId(), text };
    this.write(key, [...this.read(owner), message]);
    return message;
  }

  edit(owner: ReferenceTargetRef, id: string, text: string): void {
    const key = referenceDraftKey(owner);
    this.refresh(owner);
    this.write(
      key,
      this.read(owner).map((message) => (message.id === id ? { ...message, text } : message)),
    );
  }

  remove(owner: ReferenceTargetRef, id: string): void {
    const key = referenceDraftKey(owner);
    this.refresh(owner);
    this.write(
      key,
      this.read(owner).filter((message) => message.id !== id),
    );
  }

  private readKey(key: string): HeldMessage[] {
    const cached = this.queues.get(key);
    if (cached) return cached;
    let raw: string | null = null;
    try {
      raw = this.storage().getItem(REFERENCE_QUEUE_STORAGE_PREFIX + key);
    } catch {
      /* private mode */
    }
    let messages: HeldMessage[] = raw ? [{ id: newId(), text: raw }] : [];
    try {
      const data = JSON.parse(raw ?? "null");
      if (data?.version === 1 && Array.isArray(data.messages)) {
        const ids = new Set<string>();
        messages = data.messages.filter((item: unknown): item is HeldMessage => {
          if (!item || typeof item !== "object") return false;
          const value = item as HeldMessage;
          if (typeof value.id !== "string" || typeof value.text !== "string" || ids.has(value.id)) {
            return false;
          }
          ids.add(value.id);
          return true;
        });
      }
    } catch {
      /* previous versions stored a single plain-text message */
    }
    this.saved.set(key, raw);
    this.queues.set(key, messages);
    return messages;
  }

  private write(key: string, messages: HeldMessage[]): void {
    this.queues.set(key, messages);
    try {
      const storageKey = REFERENCE_QUEUE_STORAGE_PREFIX + key;
      const raw = messages.length ? JSON.stringify({ version: 1, messages }) : null;
      if (raw !== null) this.storage().setItem(storageKey, raw);
      else this.storage().removeItem(storageKey);
      this.saved.set(key, raw);
      this.unsaved.delete(key);
    } catch {
      // storage refused the write: the rows are still readable and sendable in memory,
      // and the failure stays observable instead of being reported as persisted.
      this.unsaved.add(key);
    }
    this.notify();
  }
}

// Machine switches remount the terminal; outstanding sends must share the same owner cache.
export const referenceQueues = new ReferenceQueueStore();

if (typeof window !== "undefined") {
  window.addEventListener("storage", (event) => {
    if (event.key) referenceQueues.refreshStorageKey(event.key);
  });
}
