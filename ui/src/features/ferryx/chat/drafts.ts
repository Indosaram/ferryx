import type { ChatDraft, TargetRef } from "../../../lib/scopedContracts";
export function draftKey(target: TargetRef): string {
 return `ferryx.chatDraft.${encodeURIComponent(JSON.stringify([target.hostId,target.ownerId,target.epoch,target.backendSessionId]))}`;
}
export function saveDraft(storage: Storage, target: TargetRef, draft: ChatDraft): void { storage.setItem(draftKey(target), JSON.stringify(draft)); }
export function loadDraft(storage: Storage, target: TargetRef): ChatDraft {
 try {
  const parsed: unknown = JSON.parse(storage.getItem(draftKey(target)) ?? "null");
  if (parsed && typeof parsed === "object" && typeof (parsed as ChatDraft).text === "string" && Array.isArray((parsed as ChatDraft).attachments)) return parsed as ChatDraft;
 } catch { /* corrupt or unreadable entry falls back to an empty draft */ }
 return { text: "", attachments: [] };
}
export function revokeDrafts(storage: Storage): void {
 const keys = Array.from({length:storage.length}, (_, i) => storage.key(i));
 for (const key of keys) if (key?.startsWith("ferryx.chatDraft.")) storage.removeItem(key);
}
