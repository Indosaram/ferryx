/*
 * Regression coverage for the reference-chat runner's refusal assertions.
 *
 * The defect: every negative branch asserted a STATUS ALLOW-LIST (or "not 200"), so an unrelated
 * earlier fence satisfied it. In the owner-null run the refusals were `INVALID_REQUEST` "a
 * reference-chat target names no owner id" and an unreadable-envelope parse error, yet QA-03,
 * QA-04, QA-05, QA-06 and QA-10 all reported `pass` - the guarded paths were never reached. One
 * probe also answered `400` with a NULL body, which proves nothing at all.
 *
 * These tests are pure and deterministic: they feed the assertion helpers the exact response
 * shapes the receipts recorded, and require the helpers to reject the false positives and accept
 * the genuine refusals. Importing the runner must not start a run (it is guarded), which is what
 * makes this file possible.
 */

import assert from 'node:assert/strict';
import test from 'node:test';
import {
  REFERENCE_WIRE_STATUS,
  assertReadControl,
  assertTypedRefusal,
  isTypedRefusal,
  redactForEvidence,
  referenceEnvelopeOf,
} from './herdr-reference-chat.mjs';

/** A response as `referenceRequest` returns it: a status, the raw text, and the parsed body. */
function response(status, body, text) {
  return {
    status,
    text: text === undefined ? (body === null ? '' : JSON.stringify(body)) : text,
    json: body,
  };
}

/** The machine error envelope every read refusal answers with (server.rs machine_error_with_details). */
function machine(code, message) {
  return { error: { code, message, retryable: false, requestId: 'req-1', details: {} } };
}

/** The frozen ScopeResult failure a mutation answers with (server.rs reference_chat_result_failure). */
function scope(code, message) {
  return { ok: false, error: { code, message, retryable: false, details: null }, requestId: 'req-1' };
}

test('the wire status table matches the frozen mapping', () => {
  // Read from server.rs `reference_chat_status` / `reference_chat_wire_code`. A hand-typed status
  // is the failure this pins: the wire codes are UPPERCASE, the Rust variants are CamelCase.
  assert.deepEqual(REFERENCE_WIRE_STATUS, {
    INVALID_REQUEST: 400,
    UNAUTHORIZED: 401,
    FORBIDDEN: 403,
    NOT_FOUND: 404,
    REQUEST_CONFLICT: 409,
    CONTROL_CONFLICT: 409,
    OPERATION_OUTCOME_UNKNOWN: 409,
    TARGET_EXPIRED: 410,
    PAYLOAD_TOO_LARGE: 413,
    UNSUPPORTED: 422,
    CAPTURE_UNSUPPORTED: 422,
    INVENTORY_INCOMPLETE: 503,
    TIMEOUT: 504,
  });
});

test('an envelope is told apart by where requestId sits', () => {
  assert.equal(referenceEnvelopeOf(machine('NOT_FOUND', 'no live session has that id on this host')), 'machine');
  assert.equal(referenceEnvelopeOf(scope('UNSUPPORTED', 'unknown interrupt capability')), 'scope');
  assert.equal(referenceEnvelopeOf(null), 'none');
  assert.equal(referenceEnvelopeOf(''), 'none');
  assert.equal(referenceEnvelopeOf({ unexpected: true }), 'other');
});

test('a bodyless 400 is NOT a typed refusal', () => {
  // The recorded shape: {"status":400,"body":null} for a non-numeric cursor. It proves nothing.
  assert.throws(
    () => assertTypedRefusal(response(400, null, ''), {
      label: 'QA-03 non-numeric cursor',
      status: 'INVALID_REQUEST',
      code: 'INVALID_REQUEST',
      envelope: 'machine',
    }),
    /expected the machine envelope, got none/,
  );
});

test('an unrelated refusal does not satisfy a guard that never ran', () => {
  // The owner-null run: the session guard was expected, the owner guard answered.
  assert.throws(
    () => assertTypedRefusal(
      response(400, machine('INVALID_REQUEST', 'a reference-chat target names no owner id')),
      { label: 'QA-10 unknown session', status: 'NOT_FOUND', code: 'NOT_FOUND', envelope: 'machine' },
    ),
    /expected HTTP 404 NOT_FOUND, got 400/,
  );
});

test('a mixed-case variant name is not a wire code', () => {
  // The correction from the parent: TARGET_EXPIRED on the wire, TargetExpired only in Rust.
  assert.throws(
    () => assertTypedRefusal(
      response(410, machine('TARGET_EXPIRED', 'the cursor belongs to stream a, not b')),
      { label: 'QA-03 foreign cursor', status: 'TargetExpired', code: 'TargetExpired', envelope: 'machine' },
    ),
    /expectation names no known status for TargetExpired/,
  );
  assert.throws(
    () => assertTypedRefusal(
      response(410, machine('TARGET_EXPIRED', 'the cursor belongs to stream a, not b')),
      { label: 'QA-03 foreign cursor', status: 410, code: 'TargetExpired', envelope: 'machine' },
    ),
    /expected wire code TargetExpired, got TARGET_EXPIRED/,
  );
});

test('the envelope kind is asserted, not assumed', () => {
  // A mutation refused by the extractor answers the MACHINE shape; a route refusal answers SCOPE.
  assert.throws(
    () => assertTypedRefusal(
      response(400, machine('INVALID_REQUEST', 'the mutation envelope is not readable: missing field params')),
      { label: 'QA-04 malformed envelope', status: 400, code: 'INVALID_REQUEST', envelope: 'scope' },
    ),
    /expected the scope envelope, got machine/,
  );
  assert.throws(
    () => assertTypedRefusal(
      response(413, scope('PAYLOAD_TOO_LARGE', 'the message is larger than one submit may carry')),
      { label: 'QA-04 oversized composer', status: 413, code: 'PAYLOAD_TOO_LARGE', envelope: 'machine' },
    ),
    /expected the machine envelope, got scope/,
  );
});

test('a genuine typed refusal passes, in both envelopes', () => {
  const machineError = assertTypedRefusal(
    response(404, machine('NOT_FOUND', 'no live session has that id on this host')),
    { label: 'QA-10 unknown session', status: 'NOT_FOUND', code: 'NOT_FOUND', envelope: 'machine' },
  );
  assert.equal(machineError.code, 'NOT_FOUND');

  const scopeError = assertTypedRefusal(
    response(422, scope('UNSUPPORTED', 'this target interrupt capability is unknown; nothing was typed')),
    { label: 'QA-06 refused capability', status: 'UNSUPPORTED', code: 'UNSUPPORTED', envelope: 'scope' },
  );
  assert.equal(scopeError.code, 'UNSUPPORTED');

  // The two fences QA-05 must keep apart: 409 stale screen vs 400 unknown card.
  assertTypedRefusal(
    response(409, scope('REQUEST_CONFLICT', 'the screen changed since the card was read; nothing was sent')),
    { label: 'QA-05 stale screen', status: 'REQUEST_CONFLICT', code: 'REQUEST_CONFLICT', envelope: 'scope' },
  );
  assertTypedRefusal(
    response(400, scope('INVALID_REQUEST', 'The prompt was not produced by parseInteractivePrompt.')),
    { label: 'QA-05 unknown card', status: 'INVALID_REQUEST', code: 'INVALID_REQUEST', envelope: 'scope' },
  );
});

test('a missing baseline is not a control', () => {
  // QA-03 run 3: the baseline itself was 404, so every cursor probe was refused by that earlier
  // fence. A control must be a page, not merely a status the branch happens to accept.
  assert.throws(
    () => assertReadControl(response(404, machine('NOT_FOUND', 'nothing identifies a transcript for this session')), 'QA-03 baseline'),
    /the control read did not answer 200/,
  );
  assert.throws(
    () => assertReadControl(response(200, null, ''), 'QA-10 live-session control'),
    /the control read carries no JSON body/,
  );
  assert.deepEqual(
    assertReadControl(response(200, { turns: [], generation: 'g1' }), 'QA-03 baseline'),
    { turns: [], generation: 'g1' },
  );
});

test('evidence redaction removes credential-shaped keys and truncates long strings', () => {
  const redacted = redactForEvidence({
    token: 'tunnel-redeemed-device-bearer',
    deviceToken: 'saved-device-token-abc',
    authorization: 'Bearer secret',
    contentBase64: 'aGVsbG8=',
    apiKey: 'sk-not-a-real-key',
    private_key: '-----BEGIN PRIVATE KEY-----',
    requestId: 'req-1',
    code: 'INVALID_REQUEST',
    nested: { machineAttachPublicKey: 'machine-noise-pub-key', ok: true },
  });
  assert.equal(redacted.token, '[redacted]');
  assert.equal(redacted.deviceToken, '[redacted]');
  assert.equal(redacted.authorization, '[redacted]');
  assert.equal(redacted.contentBase64, '[redacted]');
  // The two credential names the fixture layer redacts with are redacted here as well.
  assert.equal(redacted.apiKey, '[redacted]');
  assert.equal(redacted.private_key, '[redacted]');
  // A key that names no credential is evidence, not a secret.
  assert.equal(redacted.requestId, 'req-1');
  assert.equal(redacted.code, 'INVALID_REQUEST');
  // A PUBLIC key is not a credential: it is handed to the client by design, and replacing it would
  // delete the evidence of which machine key a request bound.
  assert.equal(redacted.nested.machineAttachPublicKey, 'machine-noise-pub-key');
  assert.equal(redacted.nested.ok, true);
  const long = redactForEvidence('x'.repeat(500));
  assert.ok(long.length < 240);
  assert.match(long, /\(500 chars\)$/);
  assert.equal(redactForEvidence(null), null);
});

test("a refusal is about the caller's request id", () => {
  // A scope refusal echoes the caller's id, and that echo is what shows the refusal came from the
  // route this caller bound rather than from an unrelated fence that answered first.
  const conflict = scope('REQUEST_CONFLICT', 'this request id was already used with a different payload');
  assertTypedRefusal(response(409, conflict), {
    label: 'QA-04 conflicting payload',
    status: 'REQUEST_CONFLICT',
    code: 'REQUEST_CONFLICT',
    envelope: 'scope',
    requestId: 'req-1',
  });
  assert.throws(
    () => assertTypedRefusal(response(409, conflict), {
      label: 'QA-04 conflicting payload',
      status: 'REQUEST_CONFLICT',
      code: 'REQUEST_CONFLICT',
      envelope: 'scope',
      requestId: 'req-2',
    }),
    /the refusal is about request id req-1, not the caller's req-2/,
  );
  // The machine envelope mints its own id, so only its presence is assertable there.
  assertTypedRefusal(
    response(400, machine('INVALID_REQUEST', 'a reference-chat target names no owner id')),
    { label: 'QA-03 baseline', status: 'INVALID_REQUEST', code: 'INVALID_REQUEST', envelope: 'machine', requestId: 'req-1' },
  );
  assert.throws(
    () => assertTypedRefusal(
      response(400, { error: { code: 'INVALID_REQUEST', message: 'a reference-chat target names no owner id' } }),
      { label: 'QA-03 baseline', status: 'INVALID_REQUEST', code: 'INVALID_REQUEST', envelope: 'machine', requestId: 'req-1' },
    ),
    /the refusal carries no request id/,
  );
});

test('only a missing native transcript blocks the QA-03 baseline', () => {
  // The classification the branch performs: 404 NOT_FOUND on the history read is the frozen
  // "nothing identifies a store for this pane" refusal - a missing fixture prerequisite.
  assert.equal(
    isTypedRefusal(
      response(404, machine('NOT_FOUND', 'nothing identifies a store for this pane')),
      { status: 'NOT_FOUND', code: 'NOT_FOUND', envelope: 'machine' },
    ),
    true,
  );
  // An auth failure, a protocol error, a bodyless 5xx or a page that is not a page is NOT that: the
  // branch must report each as a failure rather than laundering it into a blocker.
  for (const other of [
    response(401, machine('UNAUTHORIZED', 'the machine token was refused')),
    response(403, machine('FORBIDDEN', 'a view-only device cannot read this pane')),
    response(500, null, 'internal error'),
    response(200, { turns: [], generation: 'g1' }),
    response(404, { ok: false, error: { code: 'NOT_FOUND', message: 'x' }, requestId: 'req-1' }),
  ]) {
    assert.equal(
      isTypedRefusal(other, { status: 'NOT_FOUND', code: 'NOT_FOUND', envelope: 'machine' }),
      false,
    );
  }
});

