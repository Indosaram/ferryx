/**
 * Owner-scoped drafts for the Herdr reference chat (task 8).
 *
 * Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
 * (`src/lib/composerDraft.ts`, MIT — see `docs/chat/HERDR_LICENSE`).
 *
 * The upstream store keys a draft by an opaque pane key. Ferryx keys it by the frozen
 * `referenceDraftKey(target)` from `referenceTypes.ts`, so a draft belongs to the owning
 * host + backend session + daemon incarnation, and a provider-session difference never
 * moves it to another pane.
 *
 * Port obligations (task 8 acceptance):
 *  - an acknowledgement clears the *sent prefix only*: text the user typed after dispatch,
 *    and an edit inside the sent text, both survive;
 *  - a draft belonging to another target is never touched by this target's traffic;
 *  - a storage failure is *visible* (`isUnsaved`) and never discards the in-memory draft.
 *
 * Pure state plus a `Storage` shim: no fetch, no DOM beyond the optional `storage`
 * listener that lets another tab's write reconcile before an acknowledgement lands.
 */
import { referenceDraftKey, type ReferenceTargetRef } from "./referenceTypes";

/** Canonical `ferryx.*` prefix; the frozen target key is appended verbatim. */
export const REFERENCE_DRAFT_STORAGE_PREFIX = "ferryx.referenceChat.draft:";

type DraftStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

/** The draft for one pane, plus whether a send is currently in flight for it. */
export interface ReferenceDraft {
  readonly text: string;
  readonly sending: boolean;
}

export class ReferenceDraftStore {
  private drafts = new Map<string, ReferenceDraft>();
  private saved = new Map<string, string | null>();
  private unsaved = new Set<string>();
  /** the text each pending send carries, and whether the draft stopped extending it meanwhile */
  private pending = new Map<string, { sent: string; edited: boolean }>();
  private listeners = new Set<() => void>();

  constructor(private storage: () => DraftStorage = () => window.localStorage) {}

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private notify(): void {
    for (const listener of this.listeners) listener();
  }

  targetKey(target: ReferenceTargetRef): string {
    return referenceDraftKey(target);
  }

  storageKey(target: ReferenceTargetRef): string {
    return REFERENCE_DRAFT_STORAGE_PREFIX + referenceDraftKey(target);
  }

  /** Did this target's last edit fail to persist? The draft is still in memory. */
  isUnsaved(target: ReferenceTargetRef): boolean {
    return this.unsaved.has(referenceDraftKey(target));
  }

  read(target: ReferenceTargetRef): ReferenceDraft {
    return this.readKey(referenceDraftKey(target));
  }

  refresh(target: ReferenceTargetRef): void {
    this.refreshKey(referenceDraftKey(target));
  }

  set(target: ReferenceTargetRef, value: string | ((previous: string) => string)): void {
    this.setKey(referenceDraftKey(target), value);
  }

  /** `sent`: the draft text this send carries, settled once it is acknowledged. */
  begin(target: ReferenceTargetRef, sent?: string): boolean {
    return this.beginKey(referenceDraftKey(target), sent);
  }

  end(target: ReferenceTargetRef): void {
    this.endKey(referenceDraftKey(target));
  }

  /** Remove only the acknowledged prefix; edits within the sent text stay unsent. */
  settle(target: ReferenceTargetRef, sent: string): { text: string; edited: boolean } {
    return this.settleKey(referenceDraftKey(target), sent);
  }

  refreshStorageKey(storageKey: string): void {
    if (!storageKey.startsWith(REFERENCE_DRAFT_STORAGE_PREFIX)) return;
    this.refreshKey(storageKey.slice(REFERENCE_DRAFT_STORAGE_PREFIX.length));
  }

  private readKey(key: string): ReferenceDraft {
    let draft = this.drafts.get(key);
    if (!draft) {
      let text: string | null = null;
      try {
        text = this.storage().getItem(REFERENCE_DRAFT_STORAGE_PREFIX + key);
      } catch {
        /* private mode: the draft lives only in memory */
      }
      this.saved.set(key, text);
      draft = { text: text ?? "", sending: false };
      this.drafts.set(key, draft);
    }
    return draft;
  }

  private refreshKey(key: string): void {
    if (this.unsaved.has(key)) return;
    const draft = this.readKey(key);
    try {
      const text = this.storage().getItem(REFERENCE_DRAFT_STORAGE_PREFIX + key);
      if (text === this.saved.get(key)) return;
      // another tab changed it while a send was on its way: the same rule as a local edit
      const pending = this.pending.get(key);
      if (pending && !(text ?? "").startsWith(pending.sent)) pending.edited = true;
      this.saved.set(key, text);
      this.drafts.set(key, { ...draft, text: text ?? "" });
      this.notify();
    } catch {
      /* retain the in-memory draft */
    }
  }

  private setKey(key: string, value: string | ((previous: string) => string)): void {
    const draft = this.readKey(key);
    const text = typeof value === "string" ? value : value(draft.text);
    // cleared and retyped while on its way, a draft can end up starting with the sent text
    // again: once it stopped extending it, the whole of it is the user's own
    const pending = this.pending.get(key);
    if (pending && !text.startsWith(pending.sent)) pending.edited = true;
    this.drafts.set(key, { ...draft, text });
    try {
      if (text) this.storage().setItem(REFERENCE_DRAFT_STORAGE_PREFIX + key, text);
      else this.storage().removeItem(REFERENCE_DRAFT_STORAGE_PREFIX + key);
      this.saved.set(key, text || null);
      this.unsaved.delete(key);
    } catch {
      // storage refused the write: the draft is still editable and readable in memory,
      // and the failure stays observable so the composer can warn instead of pretending.
      this.unsaved.add(key);
    }
    this.notify();
  }

  private beginKey(key: string, sent?: string): boolean {
    const draft = this.readKey(key);
    if (draft.sending) return false;
    if (sent !== undefined) this.pending.set(key, { sent, edited: false });
    this.drafts.set(key, { ...draft, sending: true });
    this.notify();
    return true;
  }

  private endKey(key: string): void {
    this.pending.delete(key);
    this.drafts.set(key, { ...this.readKey(key), sending: false });
    this.notify();
  }

  private settleKey(key: string, sent: string): { text: string; edited: boolean } {
    this.refreshKey(key);
    const current = this.readKey(key).text;
    const editedMeanwhile = this.pending.get(key)?.edited === true;
    this.pending.delete(key);
    const edited = editedMeanwhile || (current !== sent && !current.startsWith(sent));
    const text = edited ? current : current.slice(sent.length);
    this.setKey(key, text);
    return { text, edited };
  }
}

export const referenceDrafts = new ReferenceDraftStore();

if (typeof window !== "undefined") {
  window.addEventListener("storage", (event) => {
    if (event.key) referenceDrafts.refreshStorageKey(event.key);
  });
}
