/**
 * Answering a prompt from the chat, for the Herdr reference chat (task 10).
 *
 * Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
 * (`src/lib/promptAnswer.ts`, MIT — see `docs/chat/HERDR_LICENSE`). The pinned module is
 * the normative behavior; this port keeps its rules and renames its entry points with the
 * `reference` prefix the other Ferryx lanes use.
 *
 * A message typed in the chat while an agent's prompt waits answers that prompt, the way
 * the agent's own menu reads keys: an option's number, its label, or the letter it binds
 * ("Yes, proceed (y)"); several numbers for a multiple choice. Anything else is the
 * prompt's own "type something" answer when it has one.
 *
 * Frozen contract consumed, never re-declared: `ReferencePrompt`, `ReferencePromptAnswer`,
 * `referenceSelectableIndices` and `referencePromptNeedsConfirmation` from
 * `./referenceTypes`. Two pinned field names are the frozen ones here:
 * `custom_option_index` → `customOptionIndex`, `multi_select` → `multiSelect`.
 *
 * Pure: no fetch, no DOM beyond the signals the card passes in.
 */
import {
  referencePromptNeedsConfirmation,
  referenceSelectableIndices,
  type ReferencePrompt,
  type ReferencePromptAnswer,
} from "./referenceTypes";

/** The three mutually exclusive answer shapes, as the pinned module names them. */
export type ReferenceTypedAnswer = ReferencePromptAnswer;

/**
 * The pinned module's `needsConfirmation`, under the pinned name.
 *
 * The rule itself lives in the frozen contract (`referencePromptNeedsConfirmation`): a
 * typed pick of an option on an approval, a plan or a menu waits for an explicit Confirm,
 * because a stray "yes" or "1" could act. The frozen rule treats an explicit `null` shape
 * as absent where the pinned one tested `!== undefined`; the frozen rule wins.
 */
export const referenceNeedsConfirmation = referencePromptNeedsConfirmation;

/** "Yes, proceed (y)" binds y; "No, and tell Codex what to do differently (esc)" binds nothing typeable. */
function boundLetter(label: string): string | null {
  return label.match(/\(([a-z])\)$/i)?.[1]?.toLowerCase() ?? null;
}

function bareLabel(label: string): string {
  return label.replace(/\s*\((?:[a-z]|esc|recommended)\)$/i, "").trim().toLowerCase();
}

/**
 * The answer a typed message carries, or `null` when the prompt takes only its options and
 * the text names none of them.
 */
export function referenceAnswerFromText(
  prompt: ReferencePrompt,
  text: string,
): ReferenceTypedAnswer | null {
  const value = text.trim();
  if (!value) return null;
  const valid = referenceSelectableIndices(prompt);
  const byNumber = (token: string): number | null => {
    if (!/^\d+$/.test(token)) return null;
    const index = Number(token) - 1;
    return valid.includes(index) ? index : null;
  };
  if (prompt.multiSelect) {
    const indices = value
      .split(/[\s,]+/)
      .filter(Boolean)
      .map(byNumber);
    return indices.every((index) => index !== null)
      ? { optionIndices: [...new Set(indices as number[])] }
      : null;
  }
  const numbered = byNumber(value);
  if (numbered !== null) return { optionIndex: numbered };
  const lower = value.toLowerCase();
  const named = valid.find((index) => {
    const label = prompt.options[index]?.label ?? "";
    return label.toLowerCase() === lower || bareLabel(label) === lower || boundLetter(label) === lower;
  });
  if (named !== undefined) return { optionIndex: named };
  return prompt.customOptionIndex != null ? { customText: value } : null;
}

function range(prompt: ReferencePrompt): string {
  const numbers = referenceSelectableIndices(prompt).map((index) => index + 1);
  const first = numbers[0] ?? 1;
  const last = numbers[numbers.length - 1] ?? first;
  return numbers.length > 1 ? `${first}–${last}` : String(first);
}

/** How a typed message answers this prompt: the composer's placeholder while it waits. */
export function referenceAnswerHint(prompt: ReferencePrompt): string {
  if (prompt.multiSelect) return "Type the numbers you choose, e.g. 1 3";
  // a free-form question (Codex's queue) has no options to number
  if (referenceSelectableIndices(prompt).length === 0) return "Type your reply…";
  return prompt.customOptionIndex != null
    ? `Type ${range(prompt)} or your own reply…`
    : `Type ${range(prompt)} to choose…`;
}

/** Why a message was not sent: the prompt takes only its options (answerFromText gave null). */
export function referenceAnswerRefusal(prompt: ReferencePrompt): string {
  return prompt.multiSelect
    ? "Choose with the option numbers above, e.g. 1 3."
    : `Choose one of the options above: type ${range(prompt)}.`;
}

/** What pressed an answer in the card; `unknown` when the browser's click does not say. */
export type ReferencePressOrigin = "keyboard" | "mouse" | "touch" | "pen" | "unknown";

/** What the card saw of the press that answered: the click, the pointerdown before it, its keydown. */
export interface ReferencePressSignal {
  readonly pointerType?: string;
  readonly downType?: string;
  readonly detail?: number;
  readonly keyed: boolean;
}

/**
 * What made the click that answered. Asked of the press itself and not of the device: a laptop
 * with a touch screen reports a fine pointer and is still tapped, and a tablet with a keyboard
 * reports a coarse one. A click names its pointer in `pointerType`, and so does the pointerdown on
 * that button before it (`downType`): a finger or a pen in either one wins, since iOS Safari has
 * called a tap's click a mouse's (WebKit bug 282988). A key's click (Enter, Space) has no pointer
 * and a `detail` of 0, and so has a script's `click()` or an assistive technology's activation,
 * which is why a key also needs its keydown on that button (`keyed`). No `press` at all is Enter
 * in the card's own field.
 */
export function referencePressOrigin(press: ReferencePressSignal | undefined): ReferencePressOrigin {
  if (press === undefined) return "keyboard";
  for (const pointer of ["touch", "pen", "mouse"] as const) {
    if (press.downType === pointer || press.pointerType === pointer) return pointer;
  }
  return press.detail === 0 && press.keyed ? "keyboard" : "unknown";
}

/** Where the focus was when the answer came back, and what the device's own pointer is. */
export interface ReferenceFocusSignal {
  readonly fromCard: boolean;
  readonly origin: ReferencePressOrigin;
  readonly coarse: boolean;
  readonly cardMounted: boolean;
  readonly inCard: boolean;
  readonly onPage: boolean;
}

/**
 * After an answer pressed in the card went out: may the keyboard's focus go on to the message box?
 * Only if the card is still there and nothing else took the focus while the answer was on its way
 * (a palette, a held message being edited, another pane): `inCard` and `onPage` say where it is now.
 * After a key or a mouse press, on any device: the card goes, and the focus would fall to the page.
 * Never after a tap or a pen: focus in the message box would raise the on-screen keyboard. A press
 * of unknown origin is a tap where the device's pointer is `coarse`.
 */
export function referenceFocusFollowsAnswer({
  fromCard,
  origin,
  coarse,
  cardMounted,
  inCard,
  onPage,
}: ReferenceFocusSignal): boolean {
  const pressed = origin === "keyboard" || origin === "mouse" || (origin === "unknown" && !coarse);
  return fromCard && pressed && cardMounted && (inCard || onPage);
}

/**
 * The structured code a changed screen answers with. The frozen contract binds an answer to
 * `promptId` + `screenRevision` and refuses one whose screen moved, or a replayed answer, with
 * `REQUEST_CONFLICT` (`staleScreen`). No regex on a message: the code is the contract.
 */
export const REFERENCE_PROMPT_STALE_CODE = "REQUEST_CONFLICT";

/** The pinned module's own name for the same refusal. */
export const REFERENCE_PROMPT_CHANGED_CODE = "prompt_changed";

/** What the card tells the user when the screen moved under its answer. */
export const REFERENCE_PROMPT_STALE_MESSAGE = "the prompt changed — re-read";

/**
 * Was this failure the screen moving under the answer? A stale screen and a replayed answer are
 * the same handling: the card says so and asks its owner to re-read the prompt.
 */
export function referencePromptErrorIsStale(error: unknown): boolean {
  if (error === null || typeof error !== "object") return false;
  const code = (error as { readonly code?: unknown }).code;
  return code === REFERENCE_PROMPT_STALE_CODE || code === REFERENCE_PROMPT_CHANGED_CODE;
}
