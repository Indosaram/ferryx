/**
 * Turns any error the remote client can catch into one sentence a phone user can act on.
 *
 * Raw transport messages, provider payloads, error codes and session ids never reach the
 * screen: callers branch on the typed `code` for logic and render only this text.
 * The original error stays available to diagnostics through `console`/telemetry at the
 * call site; this helper is presentation only.
 */

export interface UserFacingErrorOptions {
  /** What the user was trying to do, e.g. "open this worktree". Lowercase, no trailing period. */
  action?: string;
}

type CodeCopy = { message: string; hint?: string };

const CODE_COPY: Record<string, CodeCopy> = {
  STREAM_CLOSED: { message: "The connection to your computer dropped.", hint: "Reconnecting usually fixes this." },
  SOCKET_CLOSED: { message: "The connection to your computer dropped.", hint: "Reconnecting usually fixes this." },
  TUNNEL_CLOSED: { message: "The connection to your computer dropped.", hint: "Reconnecting usually fixes this." },
  NETWORK: { message: "You appear to be offline.", hint: "Check your connection and try again." },
  TIMEOUT: { message: "Your computer took too long to answer.", hint: "Try again in a moment." },
  UNAUTHORIZED: { message: "You were signed out.", hint: "Sign in again to continue." },
  FORBIDDEN: { message: "This account can't open that.", hint: "Check you're signed in with the right account." },
  MAIL_FAILED: { message: "We couldn't send the sign-in email.", hint: "Check the address and try again." },
  INVALID_EMAIL: { message: "That email address doesn't look right." },
  RATE_LIMITED: { message: "Too many attempts.", hint: "Wait a minute and try again." },
  MACHINE_OFFLINE: { message: "That computer is offline.", hint: "Open Ferryx on it, then try again." },
  MACHINE_KEY_CHANGED: { message: "That computer's identity changed.", hint: "Re-link it from the desktop app." },
  REPAIR_REQUIRED: { message: "This phone needs to be linked again.", hint: "Re-link it from the desktop app." },
  E2E_REQUIRED: { message: "This phone needs to be linked again.", hint: "Re-link it from the desktop app." },
  SESSION_GONE: { message: "That terminal has closed.", hint: "Pick another session or start a new one." },
  EPOCH_CHANGED: { message: "That terminal was restarted on your computer.", hint: "Open it again to continue." },
  NOT_FOUND: { message: "That item no longer exists." },
  PLAN_LIMIT: { message: "Your plan's limit has been reached." },
  HUB_UNAVAILABLE: { message: "The supervisor isn't running on that computer." },
  UNSUPPORTED: { message: "That computer's Ferryx version doesn't support this yet.", hint: "Update Ferryx on it." },
  BROWSER_INVALID_REFERENCE: { message: "That page element is no longer there.", hint: "Refresh the page view and try again." },
};

const HTTP_COPY: Array<[number, number, string]> = [
  [401, 401, "UNAUTHORIZED"],
  [403, 403, "FORBIDDEN"],
  [404, 404, "NOT_FOUND"],
  [408, 408, "TIMEOUT"],
  [429, 429, "RATE_LIMITED"],
  [500, 599, "SERVER"],
];

function readCode(error: unknown): string | null {
  if (!error || typeof error !== "object") return null;
  const code = (error as { code?: unknown }).code;
  return typeof code === "string" && code.length > 0 ? code : null;
}

function readStatus(error: unknown): number | null {
  if (!error || typeof error !== "object") return null;
  const status = (error as { status?: unknown }).status;
  return typeof status === "number" ? status : null;
}

function isNetworkFailure(error: unknown): boolean {
  if (typeof navigator !== "undefined" && navigator.onLine === false) return true;
  // fetch() rejects with a TypeError on network failure in every engine.
  return error instanceof TypeError;
}

export function userFacingCode(error: unknown): string | null {
  const code = readCode(error);
  if (code) {
    if (CODE_COPY[code]) return code;
    if (code.endsWith("_CLOSED") || code.includes("STREAM")) return "STREAM_CLOSED";
    if (code.includes("TIMEOUT")) return "TIMEOUT";
    if (code.includes("OFFLINE")) return "MACHINE_OFFLINE";
    if (code.includes("EPOCH")) return "EPOCH_CHANGED";
    if (code.startsWith("PLAN_LIMIT") || code.endsWith("_LIMIT")) return "PLAN_LIMIT";
    if (code.includes("UNSUPPORTED")) return "UNSUPPORTED";
  }
  const status = readStatus(error);
  if (status !== null) {
    for (const [lo, hi, mapped] of HTTP_COPY) if (status >= lo && status <= hi) return mapped;
  }
  if (isNetworkFailure(error)) return "NETWORK";
  return code;
}

export function userFacingError(error: unknown, options: UserFacingErrorOptions = {}): string {
  const lead = options.action ? `Couldn't ${options.action}.` : "";
  const code = userFacingCode(error);
  let body: CodeCopy;
  if (code && CODE_COPY[code]) {
    body = CODE_COPY[code];
  } else if (code === "SERVER") {
    body = { message: "Something went wrong on our side.", hint: "Try again in a moment." };
  } else {
    body = { message: "Something went wrong.", hint: "Try again." };
  }
  return [lead, body.message, body.hint].filter(Boolean).join(" ");
}
