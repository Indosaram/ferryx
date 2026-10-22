// Types text into a GUI terminal through MahoRD one key at a time, confirming each key by its echo
// in the session output before sending the next.
//
// On omarchy MahoRD's synthetic keyboard drops keys: a remote_type_text burst of a 21-char marker
// delivered 0 keys or a prefix ("UNCE") even into a plain GtkEntry with the IM off
// (run-q10-13/q11-probe). Per-key presses mostly arrive but can still be lost, and a key can also
// arrive several times (run-int2-all Q11: one Shift+r echoed "RRRRR"), or its echo can land after
// the wait for it ended. So the typed line is read back as the tty renders it (renderEcho: the
// canonical-mode erase echo "\b \b" removes a character) and driven to the expected text: a
// missing key is sent (at most `attempts` times per position), and anything beyond the longest
// correct prefix, a repeated key or a late echo of a resent one, is erased with Backspace. A late
// echo is re-read after every timed-out wait before a key is resent.

export class TypeVerifiedError extends Error {
  constructor(detail) {
    super(`TYPE_NOT_ECHOED: ${JSON.stringify(detail)}`);
    this.code = 'TYPE_NOT_ECHOED';
    this.detail = detail;
  }
}

/** One printable ASCII key: an uppercase letter is Shift+letter, anything else a plain press. */
export async function pressChar(mahord, ch) {
  if (/^[A-Z]$/.test(ch)) return mahord.hotkey(['Shift', ch.toLowerCase()]);
  return mahord.key(ch);
}

/**
 * The input line a canonical-mode echo shows: escape sequences dropped, a backspace moves the
 * cursor left, a printable character overwrites at the cursor, trailing blanks (from "\b \b") cut.
 */
export function renderEcho(raw) {
  const text = String(raw ?? '')
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '')
    .replace(/\x1b[@-Z\\-_]/g, '');
  const line = [];
  let cursor = 0;
  for (const ch of text) {
    if (ch === '\b') { cursor = Math.max(0, cursor - 1); continue; }
    if (ch < ' ' || ch === '\x7f') continue;
    line[cursor] = ch;
    cursor += 1;
  }
  return line.join('').trimEnd();
}

function commonPrefix(a, b) {
  let n = 0;
  while (n < a.length && n < b.length && a[n] === b[n]) n += 1;
  return n;
}

/**
 * `echo()` resolves the session output typed so far (everything after the baseline offset);
 * `waitMore(length, deadlineMs)` resolves `{ timedOut }` once that output is longer than
 * `length` (conn.waitOutput since baseline + length). Returns per-position sends, the number of
 * resends and of erased characters, and the rendered line; throws TypeVerifiedError when a
 * position is still missing after `attempts` sends or the erasing does not converge.
 */
export async function typeVerified({ mahord, echo, waitMore, text, perKeyMs = 3_000, attempts = 3 }) {
  const sends = [];
  let erased = 0;
  let erases = 0;
  const maxErases = attempts * (text.length + 1);
  for (;;) {
    const raw = await echo();
    const line = renderEcho(raw);
    if (line === text) return { sends, resent: sends.reduce((a, n) => a + n - 1, 0), erased, line };
    const good = commonPrefix(line, text);
    if (line.length > good) {
      // A repeated key, a late echo of a resent key, or a wrong character: erase back to `good`.
      if (erases >= maxErases) throw new TypeVerifiedError({ index: good, char: text[good] ?? null, attempts: sends[good] ?? 0, sends, erased, line });
      erases += 1;
      const extra = line.length - good;
      for (let k = 0; k < extra; k += 1) await mahord.key('Backspace');
      erased += extra;
    } else {
      const n = sends[good] ?? 0;
      if (n >= attempts) throw new TypeVerifiedError({ index: good, char: text[good], attempts: n, sends, line });
      sends[good] = n + 1;
      for (let k = 0; k < good; k += 1) sends[k] = sends[k] ?? 1;
      await pressChar(mahord, text[good]);
    }
    // Wait for the echo of what was just sent; a timed-out wait is followed by a fresh read above,
    // so an echo that lands late is seen before anything is resent.
    await waitMore(raw.length, perKeyMs);
  }
}

