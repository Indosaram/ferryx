import { charIndexToGridCol } from "./linkRouting";

/** Right-edge columns a TUI may leave empty when it wraps its own output (margins, padding). */
export const TUI_WRAP_MARGIN = 8;

export interface WrappedRow {
  readonly text: string;
  /** The terminal soft-wrapped this row into the next; absent on receipts from older backends. */
  readonly wrapped?: boolean;
}

const URL_START = /^[a-z][a-z0-9+.-]*:\/\//i;

function gridWidth(text: string): number {
  return charIndexToGridCol(text, text.length);
}

/**
 * Whether the terminal itself wrapped `from` into `to` (one logical line, no newline between).
 * The backend reports this exactly; without the flag, a full row with no blank on either side
 * of the boundary is taken as wrapped.
 */
export function terminalWrapsInto(from: WrappedRow, to: WrappedRow, cols: number): boolean {
  if (from.wrapped !== undefined) return from.wrapped;
  if (cols <= 0 || from.text.length < cols || from.text.length === 0 || to.text.length === 0) {
    return false;
  }
  return !/\s/.test(from.text[from.text.length - 1]) && !/\s/.test(to.text[0]);
}

/**
 * A TUI (an agent's markdown renderer, for example) wraps its own output and ends each row
 * with a real newline, so the terminal sees separate lines and records no wrap. A word wrapper
 * only breaks inside a token when it has to: the token ends in a path separator and the next
 * word did not fit on the row, or the token fills the row and is too long to fit on any row.
 * Returns the count of leading margin characters to drop from `to` when it continues `from`
 * this way, else null. The result is a guess; callers confirm the joined link.
 */
export function tuiWrapIndent(from: WrappedRow, to: WrappedRow, cols: number): number | null {
  if (cols <= 0) return null;
  const head = from.text.trimEnd();
  const tail = to.text.trimStart();
  const token = /\S+$/.exec(head)?.[0];
  const word = /^\S+/.exec(tail)?.[0];
  // A row that starts a new URL never continues the previous one.
  if (!token || !word || URL_START.test(word)) return null;
  const limit = cols - Math.min(TUI_WRAP_MARGIN, Math.floor(cols / 4));
  const headWidth = gridWidth(head);
  const separatorBreak = /[/\\]$/.test(head) && headWidth + gridWidth(word) > limit;
  const overlongToken = headWidth >= limit && gridWidth(token) + gridWidth(word) > limit;
  return separatorBreak || overlongToken ? to.text.length - tail.length : null;
}
