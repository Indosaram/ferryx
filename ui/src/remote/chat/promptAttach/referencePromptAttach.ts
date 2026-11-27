/**
 * Attachments staged on a prompt's answer field, for the reference chat (lane G6).
 *
 * Parity target: t3code `docs/user/question-attachments.md` and
 * `questionAttachments.ts` — a question that accepts a custom answer takes files and
 * pasted images; each question keeps its own attachments while the user moves between
 * questions; the main draft is never touched. t3code keys its drafts per
 * environment/thread/request/question (`questionAttachmentDraftId`); this lane's card
 * renders one prompt occurrence, so the store is keyed by the pane's own identity (the
 * `answerScope` the owner passes: the target key of the session the pane talks to) and
 * the prompt's request/question id, and survives the card's unmount while the prompt
 * still waits. The prompt id alone is a content hash: the same question on another host
 * or in another session hashes the same, so it may not be the whole key.
 *
 * Uploads go through the card's `onAttachAnswerFiles` callback (the same staged-file
 * receipt type the composer uses, `ReferenceFileReceipt` in `../referenceTypes`).
 * The store holds only state and the refusal rules; the card drives the upload calls.
 *
 * Limits: as many files per answer set as the server stages in one turn
 * (`ATTACHMENT_MAX_FILES_PER_TURN` in `src-tauri/src/scoped_contracts.rs`, enforced per
 * target by `reference_file_bounds_check`), per-file and answer-set byte budgets identical
 * to the composer's staged files (`REFERENCE_FILE_LIMITS`), and only media types the
 * composer accepts (`referenceMediaTypeFor`).
 */
import { ATTACHMENT_MAX_FILES_PER_TURN } from "../../../lib/scopedContracts";
import {
  REFERENCE_FILE_LIMITS,
  referenceMediaTypeFor,
  type ReferenceFileLimits,
} from "../referenceFiles";
import type { ReferenceFileReceipt } from "../referenceTypes";

/**
 * How many files one answer set may carry: the server's per-turn cap, read from the same
 * frozen constant the composer's staged files use (`ATTACHMENT_MAX_FILES_PER_TURN`,
 * `src-tauri/src/scoped_contracts.rs`), which `reference_file_bounds_check` enforces when
 * the files are staged. A larger number here would accept files the server then refuses,
 * so it is that constant and not a copy of its value.
 */
export const PROMPT_ATTACH_MAX_FILES = ATTACHMENT_MAX_FILES_PER_TURN;

/** One file on a prompt's answer field, from pick to upload. */
export interface ReferenceAnswerAttachment {
  readonly id: string;
  readonly displayName: string;
  readonly sizeBytes: number;
  readonly isImage: boolean;
  readonly status: "uploading" | "ready" | "failed";
  readonly receipt: ReferenceFileReceipt | null;
  readonly error: string | null;
  /** The file itself, kept so a failed upload can retry. */
  readonly file: File;
}

/** A file the store refused, with the reason in plain language. */
export interface ReferenceAnswerAttachmentRefusal {
  readonly name: string;
  readonly reason: string;
}

/**
 * The per-answer-set key: the prompt's own id under the pane's identity.
 *
 * `scope` is the owner's stable name for the pane the answer belongs to (the session, or
 * `referenceTargetKey` of the remote target). It is not decoration: prompt ids are content
 * hashes, so the same question asked in another session or on another host hashes the same
 * and would otherwise share one answer set. A card with no scope falls back to the prompt id
 * alone, which is only right for a card that cannot be confused with another pane's.
 */
export function promptAttachmentDraftKey(promptId: string, scope?: string | null): string {
  return scope != null && scope.length > 0 ? `${scope}\u0000${promptId}` : promptId;
}

/**
 * A stable name for an upload callback, for a card whose owner named no scope.
 *
 * The callback is bound to one target in practice (`onAttachAnswerFiles` stages on the pane's
 * own host), and two different callbacks are two different panes. It is a fallback only: an
 * owner that knows its pane identity passes `answerScope`, which does not depend on a
 * callback's identity staying the same across renders.
 */
const uploadScopes = new WeakMap<object, string>();
let uploadScopeSerial = 0;

export function promptAttachUploadScope(upload: object | undefined): string | null {
  if (upload === undefined) return null;
  const known = uploadScopes.get(upload);
  if (known !== undefined) return known;
  uploadScopeSerial += 1;
  const scope = `attach-${uploadScopeSerial}`;
  uploadScopes.set(upload, scope);
  return scope;
}

function answerAttachmentId(): string {
  return `pa-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Why a file cannot join the answer set, or `null` when it can. */
function answerAttachmentRefusal(
  file: File,
  existing: readonly ReferenceAnswerAttachment[],
  limits: ReferenceFileLimits,
): string | null {
  if (existing.length + 1 > PROMPT_ATTACH_MAX_FILES) {
    return "An answer can carry at most " + PROMPT_ATTACH_MAX_FILES + " files";
  }
  let bytes = existing.reduce((total, each) => total + each.sizeBytes, 0);
  if (file.size > limits.maxFileBytes) {
    return (
      file.name + " is over the " + limits.maxFileBytes + " byte limit for a staged file"
    );
  }
  bytes += file.size;
  if (bytes > limits.maxTurnBytes) {
    return "Attached files for one answer must stay within " + limits.maxTurnBytes + " bytes";
  }
  try {
    referenceMediaTypeFor(file.type, file.name);
  } catch {
    return file.name + " is a file type the composer does not accept";
  }
  return null;
}

/**
 * The attachments for one answer set, shared by every card mounted for the same prompt.
 * Plain pub/sub, so a `useSyncExternalStore` card re-renders on change and a remounted
 * card finds the same answer set (kept while moving between questions).
 */
export class ReferenceAnswerAttachmentStore {
  private sets: Record<string, readonly ReferenceAnswerAttachment[]> = {};
  private readonly listeners = new Set<() => void>();
  private static readonly EMPTY: readonly ReferenceAnswerAttachment[] = [];

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** The answer set itself; a stable reference until it changes. */
  get = (key: string): readonly ReferenceAnswerAttachment[] =>
    this.sets[key] ?? ReferenceAnswerAttachmentStore.EMPTY;

  private set(key: string, next: readonly ReferenceAnswerAttachment[]): void {
    if (next.length === 0) delete this.sets[key];
    else this.sets[key] = next;
    for (const listener of this.listeners) listener();
  }

  /**
   * Add picked files to an answer set. Accepts what the composer accepts and refuses the
   * rest per file, so one bad drop does not lose the good files.
   */
  add(
    key: string,
    files: readonly File[],
    limits: ReferenceFileLimits = REFERENCE_FILE_LIMITS,
  ): { readonly added: readonly ReferenceAnswerAttachment[]; readonly refusals: readonly ReferenceAnswerAttachmentRefusal[] } {
    const existing = this.get(key);
    const added: ReferenceAnswerAttachment[] = [];
    const refusals: ReferenceAnswerAttachmentRefusal[] = [];
    for (const file of files) {
      const current = [...existing, ...added];
      const reason = answerAttachmentRefusal(file, current, limits);
      if (reason !== null) {
        refusals.push({ name: file.name, reason });
        continue;
      }
      added.push({
        id: answerAttachmentId(),
        displayName: file.name,
        sizeBytes: file.size,
        isImage: file.type.startsWith("image/"),
        status: "uploading",
        receipt: null,
        error: null,
        file,
      });
    }
    if (added.length > 0) this.set(key, [...existing, ...added]);
    return { added, refusals };
  }

  /** Uploads came back: bind each receipt to the file that asked for it. */
  markReady(key: string, ids: readonly string[], receipts: readonly ReferenceFileReceipt[]): void {
    if (ids.length !== receipts.length) {
      this.markFailed(key, ids, "The upload did not return every file's saved path");
      return;
    }
    this.set(
      key,
      this.get(key).map((each) => {
        const index = ids.indexOf(each.id);
        return index < 0
          ? each
          : { ...each, status: "ready", receipt: receipts[index] ?? null, error: null };
      }),
    );
  }

  markFailed(key: string, ids: readonly string[], error: string): void {
    this.set(
      key,
      this.get(key).map((each) =>
        ids.includes(each.id) ? { ...each, status: "failed", error } : each,
      ),
    );
  }

  /** A failed upload tries again: back to uploading, error cleared, file kept. */
  markRetrying(key: string, id: string): void {
    this.set(
      key,
      this.get(key).map((each) =>
        each.id === id ? { ...each, status: "uploading", error: null } : each,
      ),
    );
  }

  remove(key: string, id: string): void {
    this.set(
      key,
      this.get(key).filter((each) => each.id !== id),
    );
  }

  /** The answer went out: the set's staged files belong to it now. */
  clear(key: string): void {
    this.set(key, []);
  }

  /**
   * The pane's requests resolved: every answer set staged under it is gone. t3code prunes
   * the same way on `onThreadRequestsResolved`, so a question answered elsewhere does not
   * keep its files (or its place in the store) once the thread moves on.
   */
  clearScope(scope: string): void {
    const prefix = scope + "\u0000";
    let changed = false;
    for (const key of Object.keys(this.sets)) {
      if (!key.startsWith(prefix)) continue;
      delete this.sets[key];
      changed = true;
    }
    if (changed) for (const listener of this.listeners) listener();
  }

  /** Receipts of the uploads that finished, in the order they were added. */
  readyReceipts(key: string): readonly ReferenceFileReceipt[] {
    return this.get(key)
      .filter((each) => each.status === "ready" && each.receipt !== null)
      .map((each) => each.receipt as ReferenceFileReceipt);
  }

  /** An upload is still on its way: no submit may leave before it. */
  hasUploading(key: string): boolean {
    return this.get(key).some((each) => each.status === "uploading");
  }

  /** A failed upload must be retried or removed before the answer goes out. */
  hasFailed(key: string): boolean {
    return this.get(key).some((each) => each.status === "failed");
  }
}

/** The store the prompt cards share. Keyed per answer set, so prompts stay separate. */
export const referenceAnswerAttachmentStore = new ReferenceAnswerAttachmentStore();
