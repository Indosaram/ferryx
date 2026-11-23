// Pure helpers for the browser-agent QA harness (plan maho-browser-merge, T8).
//
// Everything here is side-effect free so run.test.mjs can pin the observable contract without a
// GUI, a browser or a network: pipe/CLI frame parsing, ref selection from an observe snapshot,
// consent-card geometry, and the evidence predicates the scenarios assert on. Nothing in this
// module launches a process, writes a file or fabricates a pass.

import { createHash } from 'node:crypto';

/** Largest accepted pipe/MCP frame, matching `browser/agent/pipe.rs::MAX_FRAME_BYTES`. */
export const PIPE_MAX_FRAME_BYTES = 1024 * 1024;

/** Error codes the app returns that a scenario asserts on, by name. */
export const CODES = Object.freeze({
  pairingRequired: 'BROWSER_CONTROLLER_PAIRING_REQUIRED',
  revoked: 'BROWSER_CONTROLLER_REVOKED',
  forbidden: 'BROWSER_FORBIDDEN',
  credentialBlocked: 'BROWSER_CREDENTIAL_BLOCKED',
  invalid: 'BROWSER_CLI_REQUEST_INVALID',
  unauthorized: 'BROWSER_CLI_UNAUTHORIZED',
  unknownCapability: 'BROWSER_UNKNOWN_CAPABILITY',
  unsupported: 'UNSUPPORTED',
  frameMalformed: 'PIPE_FRAME_MALFORMED',
  frameTooLarge: 'PIPE_FRAME_TOO_LARGE',
  unknownOp: 'PIPE_UNKNOWN_OP',
  unavailable: 'BROWSER_CLI_UNAVAILABLE',
});

/**
 * The `error.code` of a pipe/MCP tool result (`{ok:false,error:{code}}`), the CLI's stderr error
 * line (`{type:"error",code}`) or an IPC `{code}` object. Returns null when the value carries no
 * code, so a caller can never read a missing field as success.
 */
export function errorCode(value) {
  if (!value || typeof value !== 'object') return null;
  if (typeof value.code === 'string' && value.code) return value.code;
  if (value.error && typeof value.error.code === 'string' && value.error.code) return value.error.code;
  return null;
}

/** True when a pipe frame is a typed refusal with exactly `code`. */
export function isPipeRefusal(frame, code) {
  return Boolean(frame) && frame.ok === false && (!code || errorCode(frame) === code);
}

/**
 * Every `{"type":"error",...}` object printed on a CLI's stderr, in print order.
 *
 * The CLI can print two error lines for one failure: the app's own typed refusal
 * (`cli.rs:4103` prints `BrowserCliResponse::Error{code,..}`), and then the top-level wrapper in
 * `bin/cli.rs:101` / `main.rs:97`, which re-prints the same message under the generic
 * `BROWSER_CLI_INVALID_OR_UNAVAILABLE` code. Only the first line carries the real cause, so the
 * first match is used and the generic duplicate is ignored.
 */
export function stderrErrorEnvelopes(stderr) {
  const lines = String(stderr ?? '').split('\n').map((line) => line.trim()).filter(Boolean);
  const found = [];
  for (const line of lines) {
    try {
      const value = JSON.parse(line);
      if (value && typeof value === 'object' && value.type === 'error') found.push(value);
    } catch { /* not a JSON line */ }
  }
  return found;
}

/**
 * The last JSON object printed on a CLI's stderr, or null. The CLI prints diagnostics and then one
 * `{"type":"error",...}` line; only that line names a code.
 */
export function lastStderrJson(stderr) {
  const lines = String(stderr ?? '').split('\n').map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    try {
      const value = JSON.parse(lines[i]);
      if (value && typeof value === 'object') return value;
    } catch { /* not the JSON line */ }
  }
  return null;
}

/**
 * Server error code of a failed `ferryx browser ...` run. The CLI prints the app's code and the
 * app's message in one envelope (`print_browser_cli_error`); a local usage failure carries
 * `BROWSER_CLI_INVALID_OR_UNAVAILABLE` with a bare message.
 */
export function cliErrorCode(result) {
  // Prefer the first typed refusal: a later generic wrapper repeats its message under
  // BROWSER_CLI_INVALID_OR_UNAVAILABLE, which would otherwise hide the real cause.
  const envelopes = stderrErrorEnvelopes(result?.stderr);
  const candidates = envelopes.length > 0 ? envelopes : [lastStderrJson(result?.stderr)].filter(Boolean);
  for (const envelope of candidates) {
    if (typeof envelope.message === 'string') {
      const head = envelope.message.split(': ', 1)[0];
      if (/^[A-Z][A-Z0-9_]+$/.test(head)) return head;
    }
    if (typeof envelope.code === 'string' && envelope.code !== 'BROWSER_CLI_INVALID_OR_UNAVAILABLE') return envelope.code;
  }
  const fallback = candidates[0];
  return typeof fallback?.code === 'string' ? fallback.code : null;
}

/** Ref of the first observed element matching `predicate`, or null. */
export function findRef(snapshot, predicate) {
  const elements = Array.isArray(snapshot?.elements) ? snapshot.elements : [];
  const hit = elements.find(predicate);
  return hit ? hit.reference : null;
}

/** Every observed element whose accessible name is exactly `name` (case-insensitive). */
export function refsNamed(snapshot, name, { role = null } = {}) {
  const wanted = String(name).toLowerCase();
  const elements = Array.isArray(snapshot?.elements) ? snapshot.elements : [];
  return elements.filter((element) => {
    if (role && String(element.role ?? '').toLowerCase() !== role.toLowerCase()) return false;
    return String(element.name ?? '').trim().toLowerCase() === wanted;
  });
}

/** The text field an observe reported for a form, by its `name` attribute or label. */
export function fieldRef(snapshot, name) {
  const wanted = String(name).toLowerCase();
  return findRef(snapshot, (element) => {
    const tag = String(element.tagName ?? '').toLowerCase();
    if (!['input', 'textarea', 'select'].includes(tag)) return false;
    return String(element.name ?? '').toLowerCase() === wanted
      || String(element.role ?? '').toLowerCase() === 'textbox';
  });
}

/** True when an observe reports the cross-origin frame as an opaque boundary it never entered. */
export function framesAreOpaque(snapshot, frameSentinel) {
  const frames = Array.isArray(snapshot?.frames) ? snapshot.frames : [];
  const text = JSON.stringify(snapshot ?? {});
  return frames.length > 0 && !text.includes(frameSentinel);
}

/** True when a snapshot's element text and names carry no password-field value. */
export function noPasswordValue(snapshot, passwordSentinel) {
  return !JSON.stringify(snapshot ?? {}).includes(passwordSentinel);
}

/** Generation an act must carry: the one the observe that produced `reference` reported. */
export function actGeneration(snapshot) {
  const generation = snapshot?.generation;
  return Number.isInteger(generation) ? generation : null;
}

/**
 * Where the consent card's Allow/Approve button sits, given the card's on-screen rectangle and the
 * host's button offsets. The card is `fixed left-1/2 top-2 w-[min(440px,100vw-24px)]` with its
 * actions at the bottom right (BrowserAgentApprovalCard.tsx); the offsets are measured per host
 * from a screenshot because DPI and window chrome differ.
 */
export function consentApprovePoint(rect, offsets) {
  if (!rect || !offsets) return null;
  const { left, top, width } = rect;
  const { fromRight, fromBottom } = offsets;
  if (![left, top, width, fromRight, fromBottom].every(Number.isFinite)) return null;
  const x = Math.round(left + width - fromRight);
  const y = Math.round(top + fromBottom);
  return Number.isFinite(x) && Number.isFinite(y) ? { x, y } : null;
}

/** Nonzero PNG dimensions, or null when the bytes are not a PNG. */
export function pngSize(bytes) {
  const buf = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes ?? []);
  if (buf.length < 24 || buf.readUInt32BE(0) !== 0x89504e47) return null;
  const width = buf.readUInt32BE(16);
  const height = buf.readUInt32BE(20);
  return width > 0 && height > 0 ? { width, height } : null;
}

/** SHA-256 of the upload fixture's bytes, so the server's recorded hash can be compared. */
export function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

/** Byte size a pipe frame occupies on the wire (payload plus its newline). */
export function frameBytes(message) {
  return Buffer.byteLength(`${JSON.stringify(message)}\n`, 'utf8');
}

/** True when a captured stdout stream is frames only: every non-blank line parses as JSON. */
export function isFramesOnly(stdout) {
  const lines = String(stdout ?? '').split('\n').filter((line) => line.trim() !== '');
  if (lines.length === 0) return false;
  return lines.every((line) => {
    try { JSON.parse(line); return true; } catch { return false; }
  });
}

/**
 * The MCP tool name for a canonical catalog id: `browser.act` becomes `browser_act`
 * (browser/agent/mcp.rs `tools()`).
 */
export function mcpToolName(toolId) {
  return String(toolId).replace(/\./g, '_');
}

/** True when an MCP `tools/call` result is a tool-level error carrying `code`. */
export function mcpToolError(result, code) {
  const payload = result?.result;
  if (!payload || payload.isError !== true) return null;
  const structured = payload.structuredContent;
  if (structured && typeof structured.code === 'string') return structured.code;
  const text = Array.isArray(payload.content)
    ? payload.content.map((part) => String(part.text ?? '')).join('\n')
    : '';
  const head = text.split(': ', 1)[0];
  return /^[A-Z][A-Z0-9_]+$/.test(head) ? head : null;
}

/** JSON-RPC error code of an MCP response, or null when it carried a result. */
export function rpcErrorCode(response) {
  const code = response?.error?.code;
  return Number.isInteger(code) ? code : null;
}

/**
 * Reads a consent-card approval request out of a controller grant/pair CLI transcript: the CLI
 * reports `waiting for approval in the Ferryx app (request <uuid>)` on stderr while the card is up.
 */
export function approvalRequestId(stderr) {
  const match = /waiting for approval in the Ferryx app \(request ([0-9a-fA-F-]{36})\)/.exec(String(stderr ?? ''));
  return match ? match[1] : null;
}

/**
 * The evidence status a scenario earns. A PASS needs at least one assertion, every assertion
 * true, and no NOT_IMPLEMENTED step; anything else - including a scenario that recorded nothing -
 * is a FAIL, so a scenario that could not run never reads as a pass.
 */
export function scenarioStatus({ assertions, notImplemented, error }) {
  if (error) return 'FAIL';
  if (!Array.isArray(assertions) || assertions.length === 0) return 'FAIL';
  if (Array.isArray(notImplemented) && notImplemented.length > 0) return 'FAIL';
  return assertions.every((entry) => entry.pass === true) ? 'PASS' : 'FAIL';
}

/**
 * True when every port in a teardown receipt was released. The harness exits nonzero on a leaked
 * fixture port, because the next run would refuse to bind it.
 */
export function portsReleased(ports) {
  return Array.isArray(ports) && ports.length > 0 && ports.every((entry) => entry.released === true);
}

/** The caller-scoped controller list must contain exactly its own identity, once. */
export function callerControllerListed(controllers, controllerId) {
  return Array.isArray(controllers) && controllers.length === 1
    && (controllers[0]?.id ?? controllers[0]) === controllerId;
}

/** The fixture must record exactly one accepted dialog, never an empty or dismissed ledger. */
export function dialogResolvedOnce(dialogs) {
  return Array.isArray(dialogs) && dialogs.length === 1 && dialogs.every((dialog) => dialog?.result === true);
}

/** A missing upload ledger is not proof that no upload reached the fixture. */
export function noUploadsRecorded(uploads) {
  return Array.isArray(uploads) && uploads.length === 0;
}

/** Distinguishes the two engines in an evidence label (`<os>-<engine>`). */
export function engineLabel(config) {
  return `${config.os}-${config.engine ?? 'webview'}`;
}

/**
 * The pipe request for one observe/act pair, built from the snapshot that produced the ref. Ids
 * always come from earlier responses; nothing is hardcoded.
 */
export function observeRequest(id, browserId) {
  return { id, op: 'observe', tab_id: browserId };
}

export function actRequest(id, { browserId, generation, action, postcondition }) {
  const frame = { id, op: 'act', tab_id: browserId, generation, action };
  if (postcondition) frame.postcondition = postcondition;
  return frame;
}

/** `act` postcondition shapes the app accepts (`BrowserWaitCondition`). */
export function textPostcondition(text, timeoutMs = null) {
  return timeoutMs ? { condition: 'text', text, timeoutMs } : { condition: 'text', text };
}

/** True when a raw fixture record proves the upload hash matched the local bytes. */
export function uploadMatches(record, expectedSha, expectedSize) {
  return Boolean(record)
    && record.sha256 === expectedSha
    && (expectedSize === undefined || record.size === expectedSize);
}
