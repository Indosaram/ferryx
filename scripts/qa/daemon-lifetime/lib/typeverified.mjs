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

/**
 * The input line of a Windows ConPTY session. ConPTY repaints the full screen (ESC[H, rows
 * separated by CRLF) and then parks the cursor on the input row with a CUP; renderEcho flattens
 * that screen into one line, so "C:\Users ..." satisfied a dropped "U" (run-int2-all Q11). The
 * input row is the row after the anchor line in the last repaint holding the anchor; later output
 * (CUP back to that row, characters, ESC[K, backspace) edits it. Without a repaint: renderEcho of
 * the text after the anchor's line (the anchor line itself is never input), or of everything when
 * the anchor never appeared.
 */
export function renderConptyInput(raw, anchor) {
  const s = String(raw ?? '');
  let frame = -1;
  for (let at = s.lastIndexOf('\x1b[H'); at >= 0; at = at > 0 ? s.lastIndexOf('\x1b[H', at - 1) : -1) {
    if (s.indexOf(anchor, at) >= 0) { frame = at; break; }
  }
  if (frame < 0) {
    const at = s.lastIndexOf(anchor);
    if (at < 0) return renderEcho(s);
    const end = s.indexOf('\n', at);
    return end < 0 ? '' : renderEcho(s.slice(end + 1));
  }
  const tail = s.slice(frame);
  const lineEnd = tail.indexOf('\n', tail.lastIndexOf(anchor));
  if (lineEnd < 0) return '';
  const rest = tail.slice(lineEnd + 1);
  const line = [];
  let row = 0;
  let col = 0;
  for (let i = 0; i < rest.length;) {
    const ch = rest[i];
    if (ch === '\x1b') {
      const csi = /^\x1b\[([0-?]*)[ -/]*([@-~])/.exec(rest.slice(i, i + 32));
      if (csi) {
        // After a repaint the cursor is parked on the input row; a CUP moves back to it.
        if (csi[2] === 'H' || csi[2] === 'f') { row = 0; col = Math.max(0, (Number(csi[1].split(';')[1]) || 1) - 1); }
        else if (csi[2] === 'K' && row === 0) line.length = Math.min(line.length, col);
        i += csi[0].length;
        continue;
      }
      const osc = /^\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/.exec(rest.slice(i));
      i += osc ? osc[0].length : 2;
      continue;
    }
    if (ch === '\r') col = 0;
    else if (ch === '\n') row += 1;
    else if (ch === '\b') col = Math.max(0, col - 1);
    else if (ch >= ' ' && ch !== '\x7f') {
      if (row === 0) line[col] = ch;
      col += 1;
    }
    i += 1;
  }
  return Array.from(line, (c) => c ?? ' ').join('').trimEnd();
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
export async function typeVerified({ mahord, echo, waitMore, text, render = renderEcho, perKeyMs = 3_000, attempts = 3 }) {
  const sends = [];
  let erased = 0;
  let erases = 0;
  const maxErases = attempts * (text.length + 1);
  for (;;) {
    const raw = await echo();
    const line = render(raw);
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

