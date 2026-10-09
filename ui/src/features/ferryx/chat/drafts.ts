import type { ChatDraft, TargetRef } from "../../../lib/scopedContracts";

/** Immutable record of a send that did not reach a definitive accepted receipt. */
export interface HeldRequest {
 readonly requestId: string;
 readonly payload: ChatDraft;
}

/** What is persisted under one target-owned key: the editable draft plus its optional held send. */
export interface StoredDraft extends ChatDraft {
 readonly held?: HeldRequest;
}

export function draftKey(target: TargetRef): string {
 return `ferryx.chatDraft.${encodeURIComponent(JSON.stringify([target.hostId,target.ownerId,target.epoch,target.backendSessionId]))}`;
}
export function saveDraft(storage: Storage, target: TargetRef, draft: ChatDraft, held?: HeldRequest): void {
 const stored: StoredDraft = held ? { ...draft, held } : { text: draft.text, attachments: draft.attachments };
 storage.setItem(draftKey(target), JSON.stringify(stored));
}
export function loadDraft(storage: Storage, target: TargetRef): StoredDraft {
 return JSON.parse(storage.getItem(draftKey(target)) ?? '{"text":"","attachments":[]}') as StoredDraft;
}
/** Snapshot equality: an accepted receipt may clear the draft only when nothing changed since dispatch. */
export function sameDraft(a: ChatDraft, b: ChatDraft): boolean {
 if (a.text !== b.text || a.attachments.length !== b.attachments.length) return false;
 return a.attachments.every((attachment, index) => {
  const other = b.attachments[index];
  return other !== undefined && other.attachmentId === attachment.attachmentId && other.sha256 === attachment.sha256;
 });
}
export function revokeDrafts(storage: Storage): void {
 const keys = Array.from({length:storage.length}, (_, i) => storage.key(i));
 for (const key of keys) if (key?.startsWith("ferryx.chatDraft.")) storage.removeItem(key);
}
