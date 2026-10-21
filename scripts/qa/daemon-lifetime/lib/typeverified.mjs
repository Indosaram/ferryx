// Types text into a GUI terminal through MahoRD one key at a time, confirming each key by its echo
// in the session output before sending the next.
//
// On omarchy MahoRD's synthetic keyboard drops keys: a remote_type_text burst of a 21-char marker
// delivered 0 keys or a prefix ("UNCE") even into a plain GtkEntry with the IM off
// (run-q10-13/q11-probe). Per-key presses mostly arrive but can still be lost, so every key is
// confirmed and a key whose echo never shows is resent a bounded number of times. A late echo of a
// resent key would show up twice, which the caller's exact-content checks then report.

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
 * `waitEcho(prefix, deadlineMs)` resolves `{ timedOut }` once the session output contains
 * `prefix` (conn.waitOutput). Returns per-key attempts; throws TypeVerifiedError if a key is
 * still not echoed after `attempts` sends.
 */
export async function typeVerified({ mahord, waitEcho, text, perKeyMs = 3_000, attempts = 3 }) {
  const sends = [];
  for (let i = 0; i < text.length; i += 1) {
    const prefix = text.slice(0, i + 1);
    let echoed = false;
    let n = 0;
    while (!echoed && n < attempts) {
      n += 1;
      await pressChar(mahord, text[i]);
      echoed = !(await waitEcho(prefix, perKeyMs)).timedOut;
    }
    sends.push(n);
    if (!echoed) throw new TypeVerifiedError({ index: i, char: text[i], attempts: n, sends });
  }
  return { sends, resent: sends.reduce((a, n) => a + n - 1, 0) };
}
