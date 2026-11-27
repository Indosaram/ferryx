/**
 * Recall of previously sent prompts (t3code composer.md "Recall a sent prompt").
 *
 * ArrowUp in an empty composer brings back the last prompt sent in this thread; again walks
 * further back, ArrowDown comes forward and moving past the newest clears the composer. Once a
 * prompt is recalled, recall takes over only while the text is still that unedited prompt and
 * the caret sits on the first visual line (ArrowUp) or the last visual line (ArrowDown);
 * counting wrapped lines needs layout, so the pure rule here counts source lines and the
 * component adds its caret-boundary checks. Editing a recalled prompt ends recall.
 */

export interface RecallState {
  /** The prompt currently shown, or "" when the composer is a plain empty draft. */
  readonly recalled: string;
  /** Index into `prompts` of `recalled`; -1 when nothing is recalled. */
  readonly index: number;
}

export const INITIAL_RECALL: RecallState = { recalled: "", index: -1 };

/** The first visual (source) line holds the caret: no newline before it. */
export function isOnFirstVisualLine(text: string, caret: number): boolean {
  return !text.slice(0, caret).includes("\n");
}

/** The last visual (source) line holds the caret: no newline after it. */
export function isOnLastVisualLine(text: string, caret: number): boolean {
  return !text.slice(caret).includes("\n");
}

export interface RecallStepInput {
  readonly state: RecallState;
  /** Sent prompts of this thread, oldest first, as the owner supplies them. */
  readonly prompts: readonly string[];
  readonly direction: "up" | "down";
  /** Whether the composer draft is empty (no text, no attachment, no picked element). */
  readonly composerEmpty: boolean;
  /** The composer text right now, when it is not empty. */
  readonly text: string;
  readonly caret: number;
}

/**
 * The next recall state, or null when the key press must move the caret as usual instead.
 * Attachments and other extras from the original message are never restored, only the text.
 */
export function stepRecall(input: RecallStepInput): RecallState | null {
  const { state, prompts, direction, composerEmpty } = input;
  if (prompts.length === 0) return null;
  if (composerEmpty) {
    if (direction === "up") {
      const index = prompts.length - 1;
      return { recalled: prompts[index], index };
    }
    return null; // ArrowDown in an empty composer is a no-op for recall.
  }
  if (state.index < 0) return null;
  // Recall takes over only while the text is still the unedited recalled prompt.
  if (input.text !== state.recalled) return null;
  if (direction === "up") {
    if (!isOnFirstVisualLine(input.text, input.caret)) return null;
    if (state.index <= 0) return null;
    return { recalled: prompts[state.index - 1], index: state.index - 1 };
  }
  if (!isOnLastVisualLine(input.text, input.caret)) return null;
  if (state.index + 1 >= prompts.length) {
    // Moving forward past the newest prompt clears the composer.
    return INITIAL_RECALL;
  }
  return { recalled: prompts[state.index + 1], index: state.index + 1 };
}
