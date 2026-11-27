/**
 * Composer message limits (t3code docs/user/composer.md parity, G3 lane).
 *
 * A message may hold up to 120,000 characters. A longer draft stays in the composer so it can
 * be shortened, and the send control is disabled with a visible count message. A paste of
 * 32 KiB or more, or any paste that would push the draft past the limit, becomes a text-file
 * attachment instead of inline text; the shortcut keeps a large paste editable.
 */

/** Characters a single message may hold. Longer drafts stay, sending is disabled. */
export const MAX_PROMPT_LENGTH = 120_000;

/** A paste at or above this size becomes a text-file attachment (32 KiB). */
export const LARGE_PASTE_BYTES = 32 * 1024;

export function isPromptTooLong(text: string): boolean {
  return text.length > MAX_PROMPT_LENGTH;
}

/** The visible count message for an over-limit draft: how much to cut, never the raw number alone. */
export function promptLengthMessage(text: string): string {
  const over = text.length - MAX_PROMPT_LENGTH;
  return `Message limit reached — ${text.length.toLocaleString()} of 120,000 characters. Remove at least ${over.toLocaleString()} characters.`;
}

export interface PasteDecision {
  /** true: the paste must not be inserted inline; hand the text to onAttachFiles instead. */
  readonly attach: boolean;
}

/** Whether a text paste becomes a text-file attachment: it is 32 KiB or more, or inserting it
 * would exceed the message limit. `hasKeepShortcut` is Cmd/Ctrl+Shift+V, which keeps a large
 * paste editable; it cannot rescue a paste that would exceed the limit. `caret` is the insertion
 * point the caller hands to onAttachFiles, so the decision does not depend on where it lands.
 *
 * `replacedLength` is how much text the paste overwrites: a paste over a selection replaces it
 * rather than adding to it, so the draft it leaves behind is `draftLength - replacedLength +
 * pasted.length`. Ignoring it would turn a paste that shortens the draft into an attachment
 * (composer.md:8-13: the attachment exists for a paste that would not fit inline).
 */
export function decideTextPaste(
  pastedText: string,
  draftLength: number,
  _caret: number,
  hasKeepShortcut: boolean,
  replacedLength = 0,
): PasteDecision {
  const bytes = new TextEncoder().encode(pastedText).length;
  const resultingLength = draftLength - Math.max(0, replacedLength) + pastedText.length;
  const fits = resultingLength <= MAX_PROMPT_LENGTH;
  if (bytes < LARGE_PASTE_BYTES) return { attach: !fits };
  if (!hasKeepShortcut) return { attach: true };
  return { attach: !fits };
}

/** The text file a large paste becomes, named like t3code's pasted-text attachment. */
export function pasteTextFile(text: string): File {
  return new File([text], "pasted text.txt", { type: "text/plain" });
}

/** Only image files take the paste/drop path; other dropped files are the owner's business. */
export function isImageFile(file: File): boolean {
  return file.type.startsWith("image/");
}
