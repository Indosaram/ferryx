/**
 * Prompt stash (t3code composer.md "Prompt stash").
 *
 * Cmd/Ctrl+S saves the current prompt with its attachments for later; with an empty composer
 * the same shortcut restores a single stash or opens the stash menu when there are several.
 * Persistence is a `ferryx.*` localStorage key, newest first, bounded, and survives restarts.
 */

export const COMPOSER_STASH_STORAGE_KEY = "ferryx.chat.composerStash:v1";

/** Stashes kept at once; older entries fall off the end. */
export const COMPOSER_STASH_LIMIT = 10;

export interface StashedPrompt {
  readonly id: string;
  readonly text: string;
  /** Attachment display names kept with the prompt; restoring re-attaching is the owner's job. */
  readonly attachmentNames: readonly string[];
  readonly savedAt: number;
}

type StashStorage = Pick<Storage, "getItem" | "setItem">;

function isStashRow(value: unknown): value is StashedPrompt {
  if (typeof value !== "object" || value === null) return false;
  const row = value as Record<string, unknown>;
  return (
    typeof row.id === "string" &&
    typeof row.text === "string" &&
    Array.isArray(row.attachmentNames) &&
    row.attachmentNames.every((name) => typeof name === "string") &&
    typeof row.savedAt === "number"
  );
}

export function readStashes(storage: StashStorage | null): readonly StashedPrompt[] {
  if (!storage) return [];
  const raw = storage.getItem(COMPOSER_STASH_STORAGE_KEY);
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isStashRow);
  } catch {
    return [];
  }
}

/** A draft with text or attachments can be stashed; an empty one cannot. */
export function stashableDraft(text: string, attachmentNames: readonly string[]): StashedPrompt | null {
  if (text.trim().length === 0 && attachmentNames.length === 0) return null;
  return {
    id: `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`,
    text,
    attachmentNames: [...attachmentNames],
    savedAt: Date.now(),
  };
}

export function saveStash(
  storage: StashStorage | null,
  entry: StashedPrompt,
): readonly StashedPrompt[] {
  const next = [entry, ...readStashes(storage)].slice(0, COMPOSER_STASH_LIMIT);
  storage?.setItem(COMPOSER_STASH_STORAGE_KEY, JSON.stringify(next));
  return next;
}

export function removeStash(storage: StashStorage | null, id: string): readonly StashedPrompt[] {
  const next = readStashes(storage).filter((entry) => entry.id !== id);
  storage?.setItem(COMPOSER_STASH_STORAGE_KEY, JSON.stringify(next));
  return next;
}
