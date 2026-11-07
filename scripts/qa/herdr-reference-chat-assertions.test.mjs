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
 *
 * Two later defects are covered here as well, both measured by the 003 x 6ad diagnostic run:
 * QA-04/happy failed with `TypeError: assert.deepEqual is not a function` because the runner called
 * a deep-equality member on the FIXTURE's `assert(condition, message)` (which has none), and both
 * QA-07 branches staged files with `mediaType` values that are not one of the product's five
 * `AttachmentMediaType` variants, so the route refused the envelope before running any staging
 * guard. The tests below pin the runner's assertion surface and its staged media types against the
 * product's own contract, without executing a run.
 */

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { assert as fixtureAssert } from './herdr-reference-fixtures.mjs';
import {
  REFERENCE_WIRE_STATUS,
  assertReadControl,
  assertTypedRefusal,
  isTypedRefusal,
  redactForEvidence,
  referenceEnvelopeOf,
} from './herdr-reference-chat.mjs';
import { readFileSync } from 'node:fs';
import { fillExitedAgentTab } from './fixtures/exited-agent-tab.mjs';
import {
  PRODUCT_GAPS,
  assertComposerCapabilities,
  assertContinuationStartup,
  assertControlOutcome,
  assertCursorPlusAfterRefused,
  assertDeltaResponse,
  assertDictationHiddenWhenProbeFails,
  assertDuplicateClaimConflict,
  assertDuplicateSendOneRequest,
  assertForkNativeIdle,
  assertHistoryChangedFrame,
  assertOriginalSessionPreserved,
  assertExitedSourceStaysAbsent,
  assertOutcomeUnknownNotRetried,
  assertPayloadResult,
  assertQueuedDeliveredOnce,
  assertResetResponse,
  assertRestartUnverifiable,
  assertResumeOnlyExited,
  assertExitedResumeStrip,
  assertResumeCreatePost,
  assertResumeAcceptance,
  exitedContinuationModes,
  continuationStripModes,
  localDesktopHappyLane,
  throwawayShellCreate,
  desktopOwnedSpawnRequest,
  desktopSpawnIdentity,
  desktopPaneCloseRequest,
  exitedDesktopResumeGap,
  publishedSessionRunning,
  exitPublication,
  machineCreateIdentity,
  distinctResumeProviderId,
  claudeProjectSlug,
  pwdFromProbe,
  composerControlMissingEvidence,
  paneEndHookLine,
  assertRev2Fallback,
  assertRichWorkRows,
  assertSteerHiddenUnlessAdvertised,
  assertUnsupportedKeepsCurrentCards,
  assertUnsupportedProviderHidesControls,
  assertUnknownCreateNotRetried,
  assertWidthExercise,
  assertWorkingSourceBlocked,
  classifyLane,
  classifyContinuationProbe,
  continuationSpawnedPane,
  explainDeltaIgnored,
  explainHostGap,
  referenceHistoryStreamId,
  richRowProviderSupported,
  unsupportedRichSession,
} from './herdr-reference-chat-rev3.mjs';
import { registryHasNativeReader, withUnsupportedRichPane } from './herdr-reference-provision.mjs';
import {
  assertPayloadMatchesRef,
  assertProxyDroppedOnce,
  claimedContinueLane,
  outcomeUnknownTarget,
  paneClaimHookLine,
  paneIdleHookLine,
  continuationPageRow,
  retargetPlantedTranscriptCwd,
  payloadRefs,
  assertContinueAcceptance,
  argvMatchesContinue,
  claudeFixturePane,
  unknownCreatePane,
  agentControlNonWorking,
  classifyUnknownCreateAbsence,
  continuationReasonContradiction,
  desktopContinuationTab,
  parseContinuationStripDom,
  publishedWorkspaceTab,
  composerControlOutcomeUnknownProof,
  expectedContinueArgv,
  proxyUpstreamHeaders,
  truncatedUpstreamBody,
  continuationCreateAccepted,
  preservedBaseline,
  plantCwdOf,
  continuationCwdRelative,
  listingContainsPane,
  proxyEntryMirrored,
  relayedProxyResponseHeaders,
  proxyBodyRequestId,
  proxyCreatedSessionId,
  assertTargetedForwardIdentity,
  assertUnknownCreateUpstream,
  assertUnknownCreateHeld,
  assertUnknownCreateReconciled,
  unknownCreateTranscriptCwd,
  chatGatewayCapabilityAdvertised,
  capabilityAdvertisedFromEntries,
  numericPid,
  matchingContinueArgv,
  chatLaneBound,
  statePublishesPane,
  workspaceStateSummary,
} from './herdr-reference-chat-rev3.mjs';
import { isolatedPaneCwd } from './herdr-reference-fixtures.mjs';

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

/* --------------------------------------------------------------------------
 * The runner's own assertion surface and its staged media types
 * ------------------------------------------------------------------------ */

const runnerSource = readFile(new URL('./herdr-reference-chat.mjs', import.meta.url), 'utf8');

/**
 * The five attachment variants the product's stage payload deserializes into
 * (`AttachmentMediaType`, src-tauri/src/scoped_contracts.rs:271-283) - the same set the product's own
 * client derives in `referenceMediaTypeFor` (ui/src/remote/chat/referenceFiles.ts:260-286).
 */
const PRODUCT_ATTACHMENT_MEDIA_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'text/plain', 'application/pdf'];

test('the runner never calls a deep-equality member its own assert does not have', async () => {
  // The runner's `assert` is the fixture's `assert(condition, message)`
  // (herdr-reference-fixtures.mjs:531) - NOT node's. It carries no deep-equality member at all, so
  // `assert.deepEqual(a, b)` raised `TypeError: assert.deepEqual is not a function` and QA-04/happy
  // reported a failure for a missing method rather than on the behaviour it guards.
  assert.equal(typeof fixtureAssert, 'function');
  assert.equal(fixtureAssert.deepEqual, undefined, 'the fixture assert has no deepEqual member');
  assert.equal(fixtureAssert.deepStrictEqual, undefined, 'and none under the strict name either');
  const source = await runnerSource;
  assert.doesNotMatch(
    source,
    /\bassert\.deep(?:Strict)?Equal\s*\(/,
    'a deep-equality call on the fixture assert cannot work; it fails as a TypeError',
  );
  assert.match(
    source,
    /import \{ deepStrictEqual \} from "node:assert\/strict";/,
    'the runner must take deep equality from node:assert/strict',
  );
});

test('every staged media type the runner sends is one of the product attachment variants', async () => {
  // The stage payload's `mediaType` IS the product's enum, so a category name ('image', 'file') is
  // refused as an unreadable envelope before the route runs any staging guard - which is how both
  // QA-07 branches came to judge a fence they were not aiming at.
  const sent = [...(await runnerSource).matchAll(/mediaType:\s*"([^"]*)"/g)].map((match) => match[1]);
  assert.ok(sent.length > 0, 'the runner stages at least one file');
  for (const value of sent) {
    assert.ok(
      PRODUCT_ATTACHMENT_MEDIA_TYPES.includes(value),
      'the stage payload deserializes into AttachmentMediaType (scoped_contracts.rs:271-283), so "' +
        value + '" is refused before any staging guard runs',
    );
  }
});


test('the runner no longer references the fixed /tmp/ferryx-reference-chat path', async () => {
  // QA-01..11 screenshots used to land in join(tmpdir(), "ferryx-reference-chat", uuid, name)
  // and teardown never removed that directory. The path must not come back, and the bytes
  // must be written under this run's evidence screenshots directory.
  const source = await runnerSource;
  assert.doesNotMatch(
    source,
    /\/tmp\/ferryx-reference-chat/,
    'the runner must not reference the fixed /tmp/ferryx-reference-chat path',
  );
  assert.doesNotMatch(
    source,
    /ferryx-reference-chat/,
    'the runner must not reference the fixed ferryx-reference-chat temp directory',
  );
  assert.match(
    source,
    /recordFile\(name, bytes\) \{[\s\S]*?join\(evidenceDir, "screenshots", name\)/,
    'screenshots must be written into the run evidence screenshots directory',
  );
});

const recorded = (name) => JSON.parse(readFileSync(new URL('./fixtures/herdr-reference-chat/' + name, import.meta.url), 'utf8'));

const frame = () => ({
  type: 'referenceHistoryChanged',
  target: { hostId: 'host-1', ownerId: 'owner-1', epoch: '7', backendSessionId: 'sess-1' },
  changeSeq: '4',
});

const delta = () => ({
  mode: 'delta',
  source: 'claude',
  generation: 'gen-2',
  replaceFromTurnId: 'turn-1',
  turns: [{ id: 'turn-1', role: 'assistant', parts: [] }],
  nextAnchor: { turnId: 'turn-1' },
  tailOpen: false,
});

const resetBody = () => ({
  mode: 'reset',
  reason: 'anchorOutsideWindow',
  page: { source: 'claude', availability: 'native', turns: [], hasMore: false, generation: 'gen-3' },
});

test('explainHostGap names the unregistered route and ignores a typed refusal', () => {
  const missing = { status: 404, json: null, text: '' };
  assert.match(explainHostGap('controlRoute', missing), /server\.rs/);
  assert.match(explainHostGap('payloadRoute', missing), /payload route/);
  assert.match(explainHostGap('agentContinue', missing), /session_api\.rs decode_create/);
  assert.equal(explainHostGap('controlRoute', response(404, machine('NOT_FOUND', 'no session'))), null);
  assert.throws(() => explainHostGap('not-a-gap', missing), /unknown product gap kind/);
});

test('explainDeltaIgnored catches a rev2 page that swallowed after', () => {
  assert.match(explainDeltaIgnored(response(200, { turns: [], generation: 'g1', source: 'claude' })), /ReferenceChatReadQuery/);
  assert.equal(explainDeltaIgnored(response(200, delta())), null);
  assert.equal(explainDeltaIgnored(response(200, resetBody())), null);
  assert.equal(explainDeltaIgnored(response(400, machine('INVALID_REQUEST', 'cursor+after'))), null);
});

test('assertHistoryChangedFrame accepts an advisory frame and rejects payload or path leakage', () => {
  assert.equal(assertHistoryChangedFrame(frame()).changeSeq, '4');
  assert.deepEqual(assertHistoryChangedFrame(recorded('qa12-frames.json').valid).target.backendSessionId, 'sess-1');
  assert.throws(() => assertHistoryChangedFrame({ ...frame(), type: 'history' }), /referenceHistoryChanged/);
  assert.throws(() => assertHistoryChangedFrame({ ...frame(), path: '/Users/x/.claude/session.jsonl' }), /leaks path/);
  assert.throws(() => assertHistoryChangedFrame({ ...frame(), transcript: 'secret' }), /leaks transcript/);
  assert.throws(() => assertHistoryChangedFrame({ ...frame(), changeSeq: '' }), /changeSeq/);
  const leaked = frame();
  leaked.target = { ...leaked.target, note: '/home/owner/transcript' };
  assert.throws(() => assertHistoryChangedFrame(leaked), /host path/);
});

test('assertDeltaResponse requires the anchor turn and rejects a partial body', () => {
  assert.equal(assertDeltaResponse(delta()).replaceFromTurnId, 'turn-1');
  assert.equal(assertDeltaResponse(recorded('qa12-frames.json').delta).generation, 'gen-2');
  assert.throws(() => assertDeltaResponse({ ...delta(), turns: [] }), /anchor turn/);
  assert.throws(() => assertDeltaResponse({ ...delta(), turns: [{ id: 'other' }] }), /start at the anchor/);
  assert.throws(() => assertDeltaResponse({ ...delta(), tailOpen: 'yes' }), /tailOpen/);
  assert.throws(() => assertDeltaResponse({ mode: 'page' }), /mode delta/);
});

test('assertResetResponse accepts only a frozen reason and an inline page', () => {
  assert.equal(assertResetResponse(resetBody()).reason, 'anchorOutsideWindow');
  assert.equal(assertResetResponse(recorded('qa12-frames.json').reset, 'anchorOutsideWindow').page.generation, 'gen-3');
  assert.throws(() => assertResetResponse(resetBody(), 'generationChanged'), /expected reset reason/);
  assert.throws(() => assertResetResponse({ ...resetBody(), reason: 'guessed' }), /not frozen/);
  assert.throws(() => assertResetResponse({ mode: 'reset', reason: 'notNative' }), /complete newest page/);
});

test('assertResetResponse allows transcript display paths and rejects host paths in cursor, generation, and payload refs', () => {
  const display = {
    kind: 'tool',
    name: 'Read',
    summary: '/Users/dev/project/src/api.rs',
    input: '{\n  "file_path": "/Users/dev/project/src/api.rs",\n  "cwd": "/Users/dev/project"\n}',
    output: 'cwd /Users/dev/project',
    text: '/Users/dev/project/README',
    title: '/Users/dev/project',
    command: 'cat /Users/dev/project/src/api.rs',
  };
  const shown = resetBody();
  shown.page.turns = [{ id: 'turn-1', role: 'assistant', parts: [display] }];
  assert.equal(assertResetResponse(shown).reason, 'anchorOutsideWindow');
  const shownDelta = delta();
  shownDelta.turns = [{ id: 'turn-1', role: 'assistant', parts: [display] }];
  assert.equal(assertDeltaResponse(shownDelta).replaceFromTurnId, 'turn-1');
  const cursorLeak = resetBody();
  cursorLeak.page.cursor = { streamId: '/home/owner/transcript', offset: 1 };
  cursorLeak.page.turns = [{ id: 'turn-1', parts: [{ kind: 'tool', input: '/Users/dev/project/src/api.rs' }] }];
  assert.throws(() => assertResetResponse(cursorLeak), /reset leaks a host path/);
  const generationLeak = resetBody();
  generationLeak.page.generation = '/home/owner/gen';
  assert.throws(() => assertResetResponse(generationLeak), /reset leaks a host path/);
  const payloadLeak = resetBody();
  payloadLeak.page.turns = [{
    id: 'turn-1',
    parts: [{
      kind: 'tool',
      input: '/Users/dev/project/src/api.rs',
      outputPayload: { ref: '/home/owner/payload', byteLength: 1, digest: 'd', truncated: false },
    }],
  }];
  assert.throws(() => assertResetResponse(payloadLeak), /reset leaks a host path/);
  const diffLeak = resetBody();
  diffLeak.page.turns = [{
    id: 'turn-1',
    parts: [{ kind: 'diff', body: 'plain patch text', patch: { ref: '/home/owner/diff', byteLength: 1, digest: 'd', truncated: false } }],
  }];
  assert.throws(() => assertResetResponse(diffLeak), /reset leaks a host path/);
  const urlLeak = delta();
  urlLeak.turns = [{ id: 'turn-1', parts: [{ kind: 'image', ref: 'opaque', url: 'file:///home/owner/shot.png' }] }];
  assert.throws(() => assertDeltaResponse(urlLeak), /delta leaks a host path/);
});

test('assertCursorPlusAfterRefused requires the machine INVALID_REQUEST envelope', () => {
  assert.equal(
    assertCursorPlusAfterRefused(response(400, machine('INVALID_REQUEST', 'cursor and after'))).code,
    'INVALID_REQUEST',
  );
  assert.throws(
    () => assertCursorPlusAfterRefused(response(200, { turns: [], generation: 'g1' })),
    /HTTP 400/,
  );
  assert.throws(
    () => assertCursorPlusAfterRefused(response(400, scope('INVALID_REQUEST', 'cursor and after'))),
    /machine envelope/,
  );
});

test('assertRev2Fallback rejects a delta query when the capability is absent', () => {
  assertRev2Fallback({ capabilityAdvertised: false, historyRequests: [{ query: '?limit=20' }] });
  assert.throws(
    () => assertRev2Fallback({ capabilityAdvertised: true, historyRequests: [{ query: '?limit=20' }] }),
    /only when referenceHistoryDeltaV1 is absent/,
  );
  assert.throws(
    () => assertRev2Fallback({ capabilityAdvertised: false, historyRequests: [{ query: '?after=%7B%7D' }] }),
    /delta query/,
  );
  assert.throws(
    () => assertRev2Fallback({ capabilityAdvertised: false, historyRequests: [] }),
    /no full-page poll/,
  );
});

test('assertWidthExercise requires 390 coarse and 1280 fine with screenshots', () => {
  const observed = [
    { viewport: { width: 390, height: 844 }, coarse: true, pointer: 'coarse', chatVisible: true, screenshot: '390-coarse.png' },
    { viewport: { width: 1280, height: 800 }, coarse: false, pointer: 'fine', chatVisible: true, screenshot: '1280-fine.png' },
  ];
  assert.equal(assertWidthExercise(observed).length, 2);
  assert.throws(() => assertWidthExercise([observed[0]]), /1280px fine/);
  assert.throws(
    () => assertWidthExercise([{ ...observed[0], pointer: 'fine' }, observed[1]]),
    /coarse pointer/,
  );
  assert.throws(
    () => assertWidthExercise([{ ...observed[0], screenshot: '' }, observed[1]]),
    /no screenshot/,
  );
});

test('assertComposerCapabilities accepts a discovered document and rejects a guessed apply mode', () => {
  const doc = recorded('qa13-composer.json').claude;
  assert.equal(assertComposerCapabilities(doc).models.apply, 'command');
  assert.equal(assertComposerCapabilities(null), null);
  assert.throws(() => assertComposerCapabilities({ ...doc, models: { ...doc.models, apply: 'guess' } }), /agent-picker/);
  assert.throws(() => assertComposerCapabilities({ ...doc, followUp: { queue: true } }), /followUp/);
});

test('assertControlOutcome accepts applied, rejected, and non-retryable unknown', () => {
  const body = (outcome, extra) => response(200, { ok: true, requestId: 'req-1', data: { outcome, retryable: false, ...extra } });
  assert.equal(assertControlOutcome(body('applied'), 'applied').outcome, 'applied');
  assert.equal(assertControlOutcome(body('rejected'), 'rejected').outcome, 'rejected');
  assert.equal(
    assertControlOutcome(body('unknown', { code: 'OPERATION_OUTCOME_UNKNOWN' }), 'unknown').outcome,
    'unknown',
  );
  assert.throws(
    () => assertControlOutcome(response(200, { ok: true, data: { outcome: 'unknown', code: 'OPERATION_OUTCOME_UNKNOWN', retryable: true } }), 'unknown'),
    /retryable/,
  );
  assert.throws(() => assertControlOutcome(body('applied'), 'rejected'), /got applied/);
});

test('assertOutcomeUnknownNotRetried requires exactly one post of that request id', () => {
  const posts = [{ requestId: 'req-unknown', outcome: 'unknown' }];
  assert.equal(assertOutcomeUnknownNotRetried(posts, 'req-unknown').requestId, 'req-unknown');
  assert.throws(
    () => assertOutcomeUnknownNotRetried(posts.concat([{ requestId: 'req-unknown', outcome: 'unknown' }]), 'req-unknown'),
    /posted 2 times/,
  );
  assert.throws(
    () => assertOutcomeUnknownNotRetried([{ requestId: 'req-unknown', outcome: 'applied' }], 'req-unknown'),
    /not an unknown outcome/,
  );
});

test('assertUnsupportedProviderHidesControls keeps text chat and hides model controls', () => {
  assertUnsupportedProviderHidesControls({
    supported: false,
    composerModelsPresent: false,
    modelControlVisible: false,
    optionControlVisible: false,
    textChatVisible: true,
  });
  assert.throws(
    () => assertUnsupportedProviderHidesControls({
      supported: false,
      composerModelsPresent: false,
      modelControlVisible: true,
      optionControlVisible: false,
      textChatVisible: true,
    }),
    /rendered composer controls/,
  );
  assert.throws(
    () => assertUnsupportedProviderHidesControls({
      supported: true,
      composerModelsPresent: false,
      modelControlVisible: false,
      optionControlVisible: false,
      textChatVisible: true,
    }),
    /unsupported provider/,
  );
});

test('assertDuplicateSendOneRequest rejects a second post or a second request id', () => {
  assert.equal(assertDuplicateSendOneRequest(recorded('qa14-follow-up.json').duplicatePosts).requestId, 'req-dup');
  assert.throws(() => assertDuplicateSendOneRequest([]), /produced 0 posts/);
  assert.throws(
    () => assertDuplicateSendOneRequest([{ requestId: 'a' }, { requestId: 'b' }]),
    /produced 2 posts/,
  );
});

test('assertQueuedDeliveredOnce accepts one delivery and rejects a second', () => {
  assert.equal(assertQueuedDeliveredOnce(recorded('qa14-follow-up.json').queuedRows).requestId, 'req-queue');
  assert.throws(
    () => assertQueuedDeliveredOnce([{ delivered: true, deliveries: 2, requestId: 'req-queue' }]),
    /delivery count is not 1/,
  );
  assert.throws(() => assertQueuedDeliveredOnce([{ delivered: false, deliveries: 0, requestId: 'req-queue' }]), /delivered 0 times/);
});

test('assertSteerHiddenUnlessAdvertised hides steer when the capability is absent', () => {
  assertSteerHiddenUnlessAdvertised(recorded('qa14-follow-up.json').steerHidden);
  assertSteerHiddenUnlessAdvertised({ advertised: true, visible: true });
  assert.throws(() => assertSteerHiddenUnlessAdvertised({ advertised: false, visible: true }), /without an advertisement/);
  assert.throws(() => assertSteerHiddenUnlessAdvertised({ advertised: true, visible: false }), /advertised but hidden/);
});

test('assertDictationHiddenWhenProbeFails hides the mic and does not auto-send', () => {
  assertDictationHiddenWhenProbeFails(recorded('qa14-follow-up.json').dictationHidden);
  assert.throws(
    () => assertDictationHiddenWhenProbeFails({ probeOk: false, micVisible: true, autoSent: false }),
    /mic was visible/,
  );
  assert.throws(
    () => assertDictationHiddenWhenProbeFails({ probeOk: true, micVisible: false, autoSent: false }),
    /only when the probe fails/,
  );
});

test('assertRichWorkRows matches recorded ids and rejects invented usage', () => {
  const fixture = recorded('qa15-rich-rows.json');
  const rendered = fixture.rows.map((row) => ({ callId: row.callId, taskId: row.taskId, status: row.status }));
  assert.equal(assertRichWorkRows(fixture, rendered).length, 2);
  assert.throws(
    () => assertRichWorkRows(fixture, rendered.map((row) => ({ ...row, usage: { inputTokens: 1 } }))),
    /rendered usage/,
  );
  assert.throws(() => assertRichWorkRows(fixture, [rendered[0]]), /omit recorded id/);
  assert.throws(() => assertRichWorkRows({ rows: [{ status: 'invented' }] }, []), /no provider call or task id/);
});

test('assertPayloadResult accepts loaded, truncated, and stale refs without a path', () => {
  const loaded = {
    status: 200,
    json: { status: 'loaded', kind: 'output', byteLength: 11, digest: 'abc', truncated: false, cap: 2097152, body: 'hello' },
  };
  assert.equal(assertPayloadResult(loaded, 'success').digest, 'abc');
  assertPayloadResult({
    status: 200,
    json: { status: 'truncated', kind: 'output', byteLength: 9, digest: 'def', truncated: true, cap: 8, body: 'hello' },
  }, 'truncated');
  assertPayloadResult({ status: 410, json: { error: { code: 'TARGET_EXPIRED', message: 'the payload ref is stale' } } }, 'expired');
  assert.throws(
    () => assertPayloadResult({ status: 200, json: { status: 'loaded', kind: 'output', byteLength: 1, digest: 'd', truncated: false, body: 'x', path: '/tmp/out' } }, 'success'),
    /leaked a host path/,
  );
  assert.throws(
    () => assertPayloadResult({ status: 404, json: { error: { code: 'NOT_FOUND', message: 'missing' } } }, 'expired'),
    /expired payload was NOT_FOUND/,
  );
});

test('assertRestartUnverifiable rejects an assumed failure', () => {
  assert.equal(assertRestartUnverifiable('unverifiable'), 'unverifiable');
  assert.throws(() => assertRestartUnverifiable('failed'), /not unverifiable/);
  assert.throws(() => assertRestartUnverifiable(undefined), /not unverifiable/);
});

test('assertUnsupportedKeepsCurrentCards rejects a replaced or synthetic card list', () => {
  const before = { supported: false, cards: ['reference-tool-output:read'] };
  assertUnsupportedKeepsCurrentCards(before, { cards: ['reference-tool-output:read'], syntheticRich: false });
  assert.throws(
    () => assertUnsupportedKeepsCurrentCards(before, { cards: ['reference-tool-output:other'], syntheticRich: false }),
    /replaced current cards/,
  );
  assert.throws(
    () => assertUnsupportedKeepsCurrentCards(before, { cards: before.cards, syntheticRich: true }),
    /synthesized rich state/,
  );
});

test('assertContinuationStartup rejects client argv and a transcript path', () => {
  const startup = assertContinuationStartup(recorded('qa16-continuation.json').forkStartup);
  assert.equal(startup.mode, 'fork');
  assertContinuationStartup(recorded('qa16-continuation.json').resumeStartup);
  assert.throws(
    () => assertContinuationStartup({
      startup: {
        kind: 'agentContinue',
        mode: 'fork',
        agentType: 'claude',
        providerSession: { key: 'sessionId', id: 'p1' },
        argv: ['--resume', 'client'],
      },
    }),
    /client argv/,
  );
  assert.throws(() => assertContinuationStartup({ startup: { kind: 'agentResume' } }), /agentContinue/);
});

test('assertResumeOnlyExited requires an exited source and no fork offer', () => {
  assertResumeOnlyExited({ exited: true, running: false, mode: 'resume', agentType: 'omp', forkOffered: false, forkHidden: true });
  assert.throws(
    () => assertResumeOnlyExited({ exited: false, running: true, mode: 'resume', agentType: 'omp', forkOffered: false }),
    /not exited/,
  );
  assert.throws(
    () => assertResumeOnlyExited({ exited: true, running: false, mode: 'resume', agentType: 'claude', forkOffered: false, forkHidden: false }),
    /resume-only/,
  );
});

test('exited claude offers resume and may offer fork; a non-fork provider is resume-only', () => {
  assert.deepEqual(exitedContinuationModes('claude'), ['fork', 'resume']);
  assert.deepEqual(exitedContinuationModes('omp'), ['resume']);
  assert.deepEqual(exitedContinuationModes('bash'), []);
  assert.deepEqual(continuationStripModes({
    buttons: ['continue-in-new-pane-fork', 'continue-in-new-pane-resume'],
    unavailable: [],
  }), ['fork', 'resume']);
  assertExitedResumeStrip({ exited: true, running: false, agentType: 'claude', offered: ['resume', 'fork'] });
  assertExitedResumeStrip({ exited: true, running: false, agentType: 'omp', offered: ['resume'] });
  assert.throws(
    () => assertExitedResumeStrip({ exited: true, running: false, agentType: 'claude', offered: ['resume', 'steer'] }),
    /hides/,
  );
  assert.throws(
    () => assertExitedResumeStrip({ exited: true, running: false, agentType: 'omp', offered: ['resume', 'fork'] }),
    /offered fork/,
  );
  assert.throws(
    () => assertExitedResumeStrip({ exited: false, running: true, agentType: 'claude', offered: ['resume'] }),
    /not exited/,
  );
});

test('resume create keeps the provider session and the shell throwaway is not the fixture source', () => {
  const id = 'qa16resume0123456789abcdef';
  assert.equal(distinctResumeProviderId(['fixture-provider'], id), id);
  assert.throws(() => distinctResumeProviderId(['fixture-provider'], 'fixture-provider'), /collides/);
  assert.match(distinctResumeProviderId(['fixture-provider']), /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
  assert.equal(throwawayShellCreate('ws-1', 'req-1').cwdRelative, null);
  assert.deepEqual(throwawayShellCreate('ws-1', 'req-1').startup, { kind: 'shell' });
  assert.deepEqual(machineCreateIdentity({
    target: { sessionId: 'pane-1', daemonEpoch: '12' },
  }), { sessionId: 'pane-1', daemonEpoch: '12' });
  assert.equal(machineCreateIdentity({ target: { sessionId: 'pane-1', daemonEpoch: '01' } }).daemonEpoch, null);
  const startup = assertResumeCreatePost({
    startup: {
      kind: 'agentContinue',
      mode: 'resume',
      agentType: 'claude',
      providerSession: { key: 'session_id', id },
    },
  });
  assert.equal(startup.mode, 'resume');
  const source = { id: 'source', running: false, pid: null, cwd: '/repo', workspaceId: 'ws', worktree: null, providerSessionId: id, cols: 80, rows: 24 };
  const created = { id: 'new', running: true, pid: 2, cwd: '/repo', workspaceId: 'ws', worktree: null, providerSessionId: id, cols: 80, rows: 24 };
  assertResumeAcceptance({
    mode: 'resume',
    agentType: 'claude',
    providerSessionId: id,
    sourceId: 'source',
    createdSessionId: 'new',
    workspaceId: 'ws',
    worktree: null,
    before: [source],
    after: [source, created],
    argv: ['claude', '--resume', id],
    posts: 1,
  });
  assert.throws(() => assertResumeAcceptance({
    mode: 'resume',
    agentType: 'claude',
    providerSessionId: id,
    sourceId: 'source',
    createdSessionId: 'new',
    workspaceId: 'ws',
    worktree: null,
    before: [source],
    after: [source, { ...created, providerSessionId: 'other' }],
    argv: ['claude', '--resume', id],
    posts: 1,
  }), /providerSession/);
  assert.throws(() => assertResumeAcceptance({
    mode: 'resume',
    agentType: 'claude',
    providerSessionId: id,
    sourceId: 'source',
    createdSessionId: 'new',
    workspaceId: 'ws',
    worktree: null,
    before: [source],
    after: [source, created],
    argv: ['claude', '--resume', id, '--fork-session'],
    posts: 1,
  }), /fork-session/);
  const desktop = localDesktopHappyLane();
  assert.equal(desktop.name, 'local-desktop');
  assert.equal(desktop.status, 'unmeasured');
  assert.match(desktop.reason, /^N\/A:/);
  assert.match(paneEndHookLine('/usr/bin/ferryx', id), /agent-hook claude SessionEnd/);
  const missing = composerControlMissingEvidence('pane-1', {
    historyStatus: 200,
    availability: 'native',
    modelChoices: 'opus,sonnet',
    providerSessionId: id,
    pageUrl: 'http://127.0.0.1/#session=pane-1',
  });
  assert.match(missing, /composer-model-control was not rendered/);
  assert.match(missing, /composerModels=opus,sonnet/);
  assert.match(missing, /SessionStart idle hook/);
});

test('assertForkNativeIdle accepts only an idle claude or codex fork', () => {
  assertForkNativeIdle({ agentType: 'codex', working: false, running: true, mode: 'fork' });
  assert.throws(() => assertForkNativeIdle({ agentType: 'omp', working: false, running: true, mode: 'fork' }), /not claude or codex/);
  assert.throws(() => assertForkNativeIdle({ agentType: 'claude', working: true, running: true, mode: 'fork' }), /not an idle live pane/);
});

test('assertWorkingSourceBlocked refuses creation and a forced stop', () => {
  assertWorkingSourceBlocked(recorded('qa16-continuation.json').workingRefusal);
  assert.throws(
    () => assertWorkingSourceBlocked({ created: true, code: 'AGENT_CONTINUE_CONFLICT', sourceStopped: false }),
    /created a continuation/,
  );
  assert.throws(
    () => assertWorkingSourceBlocked({ created: false, code: 'INVALID_REQUEST', sourceStopped: false }),
    /AGENT_CONTINUE_CONFLICT/,
  );
  assert.throws(
    () => assertWorkingSourceBlocked({ created: false, code: 'AGENT_CONTINUE_CONFLICT', sourceStopped: true }),
    /stopped/,
  );
});

test('assertDuplicateClaimConflict accepts the continuation conflict codes only', () => {
  assertDuplicateClaimConflict(recorded('qa16-continuation.json').claimConflict);
  assert.throws(() => assertDuplicateClaimConflict({ created: false, code: 'INVALID_REQUEST' }), /AGENT_CONTINUE_CONFLICT/);
  assert.throws(() => assertDuplicateClaimConflict({ created: true, code: 'AGENT_CONTINUE_CONFLICT' }), /created a session/);
});

test('assertUnknownCreateNotRetried requires one outcome-unknown create', () => {
  assert.equal(assertUnknownCreateNotRetried(recorded('qa16-continuation.json').unknownCreate).requestId, 'req-unknown');
  assert.throws(
    () => assertUnknownCreateNotRetried([
      { requestId: 'req-unknown', code: 'OPERATION_OUTCOME_UNKNOWN' },
      { requestId: 'req-unknown', code: 'OPERATION_OUTCOME_UNKNOWN' },
    ]),
    /issued 2 times/,
  );
  assert.throws(
    () => assertUnknownCreateNotRetried([{ requestId: 'req-unknown', code: 'INVALID_REQUEST' }]),
    /not outcome-unknown/,
  );
});

test('assertOriginalSessionPreserved keeps the source fields and does not drop a session', () => {
  const fixture = recorded('qa16-continuation.json');
  assert.equal(
    assertOriginalSessionPreserved(fixture.inventoryBefore, fixture.inventoryAfter).id,
    'sess-1',
  );
  const dropped = { sessions: fixture.inventoryAfter.sessions.filter((row) => row.id !== 'sess-1') };
  assert.throws(() => assertOriginalSessionPreserved(fixture.inventoryBefore, dropped), /disappeared/);
  const mutated = {
    sessions: fixture.inventoryAfter.sessions.map((row) => row.id === 'sess-1' ? { ...row, pid: 99 } : row),
  };
  assert.throws(() => assertOriginalSessionPreserved(fixture.inventoryBefore, mutated), /source pid changed/);
});

test('assertExitedSourceStaysAbsent requires the throwaway missing from both inventories', () => {
  const fixture = { id: 'fixture', running: true, pid: 1, cwd: '/repo', workspaceId: 'ws', worktree: null, providerSessionId: 'planted', cols: 80, rows: 24 };
  const created = { id: 'created', running: true, pid: 2, cwd: '/repo', workspaceId: 'ws', worktree: null, providerSessionId: 'planted', cols: 80, rows: 24 };
  const before = { sessions: [fixture] };
  const after = { sessions: [fixture, created] };
  assert.equal(assertExitedSourceStaysAbsent(before, after, 'throwaway'), 'throwaway');
  assert.throws(
    () => assertExitedSourceStaysAbsent({ sessions: [fixture, { id: 'throwaway', running: false }] }, after, 'throwaway'),
    /before inventory/,
  );
  assert.throws(
    () => assertExitedSourceStaysAbsent(before, { sessions: [fixture, created, { id: 'throwaway', running: true }] }, 'throwaway'),
    /re-registered/,
  );
  assert.throws(() => assertExitedSourceStaysAbsent(before, after, ''), /no id/);
  assert.throws(() => assertExitedSourceStaysAbsent(null, after, 'throwaway'), /before\/after pair/);
});

test('classifyLane never promotes an unmeasured lane to pass', () => {
  assert.equal(classifyLane([{ status: 'pass' }, { status: 'pass' }]), 'pass');
  assert.equal(classifyLane([{ status: 'pass' }, { status: 'unmeasured' }]), 'unmeasured');
  assert.equal(classifyLane([{ status: 'failed' }, { status: 'unmeasured' }]), 'failed');
  assert.equal(classifyLane([{ status: 'blocked' }, { status: 'unmeasured' }]), 'blocked');
  assert.equal(classifyLane([]), 'failed');
  assert.notEqual(classifyLane([{ status: 'unmeasured', reason: 'provider-credential-absent' }]), 'pass');
});

test('the runner selects QA-12..16 without counting an unmeasured lane as a pass', async () => {
  const source = await runnerSource;
  for (const id of ['QA-12', 'QA-13', 'QA-14', 'QA-15', 'QA-16']) {
    assert.match(source, new RegExp(id));
  }
  assert.match(source, /REV3_QA_ROWS/);
  assert.match(source, /overallStatus = "UNMEASURED"/);
  assert.match(source, /createRev3Scenarios/);
  // The runner registers createRev3Scenarios from this module. The capability gate and
  // the QA-14 DOM probes live there; a string in the runner file would not be the gate.
  assert.match(source, /from "\.\/herdr-reference-chat-rev3\.mjs"/);
  const rev3 = await readFile(new URL('./herdr-reference-chat-rev3.mjs', import.meta.url), 'utf8');
  assert.match(rev3, /export const DELTA_CAPABILITY = "referenceHistoryDeltaV1"/);
  assert.match(rev3, /if \(!advertised\) lanes\.push\(failedLane\("delta-capability"/);
  assert.match(rev3, /delta query was not sent because " \+ DELTA_CAPABILITY/);
  assert.match(rev3, /steer-button/);
  assert.match(rev3, /dictation-mic-button/);
  assert.equal(PRODUCT_GAPS.historyAfter.includes('reference_chat_after_anchor'), true);
  assert.equal(PRODUCT_GAPS.agentContinue.includes('decode_create'), true);
  assert.doesNotMatch(rev3, /code === "INVALID_REQUEST" \? PRODUCT_GAPS\.agentContinue/);
  assert.doesNotMatch(rev3, /failedLane\("duplicate-claim", PRODUCT_GAPS\.agentContinue\)/);
  assert.doesNotMatch(rev3, /failedLane\("working-source", PRODUCT_GAPS\.agentContinue/);
});

test('referenceHistoryStreamId matches referenceHistorySync.ts:131-138', () => {
  const generation = 'claude-transcript:c4a6b8691c6d1ce6:inc-1';
  assert.equal(referenceHistoryStreamId({ generation, cursor: null }), 'claude-transcript:c4a6b8691c6d1ce6');
  assert.equal(referenceHistoryStreamId({ generation: 'stream-a:abcdef0123456789' }), 'stream-a');
  assert.equal(referenceHistoryStreamId({ generation, cursor: { streamId: 'from-cursor' } }), 'from-cursor');
  assert.equal(referenceHistoryStreamId({ generation: 'not-a-stream' }), null);
  assert.equal(referenceHistoryStreamId({}), null);
});

test('QA-12 event-delta fails when no stream id can be derived', async () => {
  const rev3 = await readFile(new URL('./herdr-reference-chat-rev3.mjs', import.meta.url), 'utf8');
  assert.match(rev3, /streamId: referenceHistoryStreamId\(page\)/);
  assert.match(rev3, /referenceHistorySync\.ts:131-138/);
  assert.match(rev3, /failedLane\("event-delta", "no stream id could be derived/);
  assert.doesNotMatch(rev3, /unmeasured\("event-delta", "the live page has no generation-scoped turn id/);
});

test('current-cards selects a registry id with no native reader', () => {
  assert.equal(richRowProviderSupported('claude'), true);
  assert.equal(richRowProviderSupported('omo'), true);
  assert.equal(richRowProviderSupported('codex'), true);
  assert.equal(richRowProviderSupported('droid'), false);
  assert.equal(registryHasNativeReader('droid'), false);
  const ctx = {
    fixture: {
      sessions: [
        { registryId: 'claude', backendSessionId: 'claude-pane' },
        { registryId: 'omo', backendSessionId: 'omo-pane' },
        { registryId: 'codex', backendSessionId: 'codex-pane' },
        { registryId: 'droid', backendSessionId: 'droid-pane' },
      ],
    },
  };
  assert.equal(unsupportedRichSession(ctx, ctx.fixture.sessions[0]).backendSessionId, 'droid-pane');
  assert.equal(unsupportedRichSession({ fixture: { sessions: ctx.fixture.sessions.slice(0, 3) } }, ctx.fixture.sessions[0]), null);
});

test('provisioner adds a droid pane and copies workspaceId from the registration', async () => {
  const host = {
    id: 'local',
    transport: 'local',
    spawnSessions: true,
    sessions: [
      { registryId: 'claude', provider: 'claude', workspaceId: ' orca-qa-ws ', cols: 100, rows: 30, loginArgs: ['--noprofile'] },
      { registryId: 'omo', provider: 'omo' },
      { registryId: 'codex', provider: 'codex' },
    ],
  };
  const sessions = withUnsupportedRichPane(host);
  assert.equal(sessions.length, 4);
  assert.equal(sessions[3].registryId, 'droid');
  assert.equal(sessions[3].workspaceId, ' orca-qa-ws ');
  assert.equal(withUnsupportedRichPane({ ...host, sessions: [{ registryId: 'omo' }] }).length, 1);
  const provision = await readFile(new URL('./herdr-reference-provision.mjs', import.meta.url), 'utf8');
  assert.match(provision, /row\.workspaceId = spawned\.workspaceRegistration\.workspaceId/);
  assert.match(provision, /for \(const session of withUnsupportedRichPane\(host\)\)/);
});

test('continuation probe names the product refusal and never passes an unclaimed source', () => {
  assert.equal(classifyContinuationProbe('AGENT_CONTINUE_CONFLICT', false).status, 'pass');
  assert.equal(classifyContinuationProbe('AGENT_CONTINUE_CONFLICT', true).status, 'failed');
  assert.match(classifyContinuationProbe('INVALID_REQUEST', false).reason, /INVALID_REQUEST/);
  assert.equal(classifyContinuationProbe('INVALID_REQUEST', false).status, 'failed');
  const unverified = classifyContinuationProbe('AGENT_TRANSCRIPT_UNVERIFIED', false);
  assert.equal(unverified.status, 'unmeasured');
  assert.match(unverified.reason, /continue_startup\.rs:70-80/);
  assert.match(unverified.reason, /session_metadata_provider\.rs:10/);
  assert.notEqual(unverified.status, 'pass');
  assert.equal(classifyContinuationProbe('AGENT_CWD_MISMATCH', false).status, 'unmeasured');
  assert.equal(continuationSpawnedPane(
    { sessions: [{ id: 'sess-1' }] },
    { sessions: [{ id: 'sess-1' }, { id: 'sess-2' }] },
  ), true);
  assert.equal(continuationSpawnedPane(
    { sessions: [{ id: 'sess-1' }] },
    { sessions: [{ id: 'sess-1' }] },
  ), false);
});

test('QA-16 failure plants a transcript and does not call a request-shape refusal a product gap', async () => {
  const rev3 = await readFile(new URL('./herdr-reference-chat-rev3.mjs', import.meta.url), 'utf8');
  assert.match(rev3, /const planted = plantClaudeTranscript\(ctx, session\)/);
  assert.match(rev3, /harness continuation body was refused/);
  assert.match(rev3, /classifyContinuationProbe\(code, spawned\)/);
  assert.match(rev3, /paneClaimHookLine\(binary, session\.providerSessionId\)/);
  assert.match(rev3, /retargetPlantedTranscriptCwd\(/);
  assert.match(rev3, /claimedContinueLane\(code, spawned\)/);
});

test('QA-16 retargets the planted cwd and types the agent-hook claim', () => {
  const body = [
    '{"cwd":"/Users/dev/project","sessionId":"abc"}',
    '{"cwd":"/work/qa15","sessionId":"abc"}',
  ].join('\n') + '\n';
  const next = retargetPlantedTranscriptCwd(body, '/home/qa/workspace');
  assert.equal(next.includes('/Users/dev/project'), false);
  assert.equal(next.includes('/work/qa15'), false);
  assert.equal(next.split('"/home/qa/workspace"').length - 1, 2);
  assert.throws(() => retargetPlantedTranscriptCwd(body, 'relative'), /absolute/);
  const line = paneClaimHookLine('/opt/ferryx', '6b1e0c3a-a12b-4f00-9a00-0ca0aa000001');
  assert.match(line, /agent-hook claude PreToolUse/);
  assert.match(line, /session_id/);
  assert.equal(line.includes('QA16_CLAIM_SENT'), false);
  const idle = paneIdleHookLine('/opt/ferryx', '6b1e0c3a-a12b-4f00-9a00-0ca0aa000001');
  assert.match(idle, /agent-hook claude SessionStart/);
  assert.match(idle, /session_id/);
  assert.equal(idle.includes('QA16_IDLE_SENT'), false);
  assert.equal(idle.includes('PreToolUse'), false);
  assert.equal(paneIdleHookLine('', 'id'), null);
  assert.equal(paneIdleHookLine('/opt/ferryx', ''), null);
  assert.equal(claimedContinueLane('AGENT_CONTINUE_CONFLICT', false).status, 'pass');
  assert.equal(claimedContinueLane('AGENT_CONTINUE_CONFLICT', true).status, 'failed');
  assert.equal(claimedContinueLane('AGENT_TRANSCRIPT_UNVERIFIED', false).status, 'failed');
  assert.equal(paneClaimHookLine('', 'id'), null);
});

test('outcomeUnknownTarget matches only the targeted posts', () => {
  assert.equal(outcomeUnknownTarget('control', 'POST', '/api/v1/reference-chat/sess/control', ''), true);
  assert.equal(outcomeUnknownTarget('control', 'GET', '/api/v1/reference-chat/sess/control', ''), false);
  assert.equal(outcomeUnknownTarget('control', 'POST', '/api/v1/sessions', '{"startup":{"kind":"agentContinue"}}'), false);
  assert.equal(
    outcomeUnknownTarget('agentContinue', 'POST', '/api/v1/sessions', '{"startup":{"kind":"agentContinue"}}'),
    true,
  );
  assert.equal(
    outcomeUnknownTarget('agentContinue', 'POST', '/api/v1/sessions?x=1', '{"startup":{"kind":"agentContinue"}}'),
    true,
  );
  assert.equal(
    outcomeUnknownTarget('agentContinue', 'POST', '/api/v1/sessions', '{"startup":{"kind":"shell"}}'),
    false,
  );
});

test('assertProxyDroppedOnce requires one forward that was dropped after the host answered', () => {
  const dropped = { forwarded: true, dropped: true, upstreamStatus: 200 };
  assert.equal(assertProxyDroppedOnce([dropped], 'control').upstreamStatus, 200);
  assert.throws(() => assertProxyDroppedOnce([dropped, dropped], 'control'), /forwarded 2 times/);
  assert.throws(
    () => assertProxyDroppedOnce([{ forwarded: true, dropped: false, upstreamStatus: 200 }], 'control'),
    /not forwarded and then dropped/,
  );
  assert.throws(
    () => assertProxyDroppedOnce([{ forwarded: true, dropped: true }], 'control'),
    /before the host answered/,
  );
});

test('relayed proxy responses close the socket and targeted forwards keep requestId', () => {
  const closed = relayedProxyResponseHeaders({
    connection: 'keep-alive',
    'keep-alive': 'timeout=5',
    'content-type': 'application/json',
  });
  assert.equal(closed.connection, 'close');
  assert.equal(closed['keep-alive'], undefined);
  assert.equal(closed['content-type'], 'application/json');
  const requestId = 'a8ce5381-5775-426c-97f8-d189bbaa03fc';
  assert.equal(proxyBodyRequestId('{"requestId":"' + requestId + '","startup":{"kind":"agentContinue"}}'), requestId);
  assert.equal(proxyBodyRequestId('not-json'), null);
  assert.equal(proxyCreatedSessionId('{"target":{"sessionId":"sess-new","daemonEpoch":"1"}}'), 'sess-new');
  assert.equal(proxyCreatedSessionId('{"sessionId":"sess-top"}'), 'sess-top');
  assert.equal(assertTargetedForwardIdentity([{ forwarded: true, targeted: true, requestId }]).requestIds[0], requestId);
  assert.throws(() => assertTargetedForwardIdentity([{ forwarded: true, targeted: true }]), /requestId/);
  assert.equal(assertUnknownCreateUpstream({ upstreamStatus: 201, upstreamBody: '{"target":{"sessionId":"sess-new"}}' }), 'sess-new');
  assert.throws(
    () => assertUnknownCreateUpstream({ upstreamStatus: 400, upstreamBody: '{"error":{"code":"AGENT_CWD_MISMATCH"}}' }),
    /AGENT_CWD_MISMATCH/,
  );
  assertUnknownCreateHeld({
    status: 'unknown',
    text: 'Continuation was not confirmed. It will not be started again.',
    disabled: true,
    retried: false,
  });
  assert.throws(() => assertUnknownCreateHeld({ status: 'unknown', text: 'rejected', disabled: true, retried: false }), /will not be started again/);
  const before = [{ id: 'source' }];
  const after = [{ id: 'source' }, { id: 'sess-new' }];
  assert.equal(assertUnknownCreateReconciled(before, after, 'sess-new', 'source').id, 'sess-new');
  assert.throws(() => assertUnknownCreateReconciled(before, before, 'sess-new', 'source'), /added 0/);
  assert.throws(() => assertUnknownCreateReconciled(before, [{ id: 'source' }, { id: 'other' }], 'sess-new', 'source'), /not the created session/);
});

test('unknown-create transcript cwd is the workspace root the client requests', () => {
  const session = {
    workspaceRegistration: { repoRoot: '/work/orca-qa-ws' },
    cwd: '/work/orca-qa-ws/.ferryx-qa-panes/pane',
  };
  assert.equal(unknownCreateTranscriptCwd(session), '/work/orca-qa-ws');
  assert.equal(plantCwdOf(session), '/work/orca-qa-ws/.ferryx-qa-panes/pane');
  assert.equal(continuationCwdRelative(session), '.ferryx-qa-panes/pane');
  assert.equal(unknownCreateTranscriptCwd({ cwd: '/work/orca-qa-ws/.ferryx-qa-panes/pane' }), null);
});

test('capability fact is the page capabilities list, not a mirror token', () => {
  const mirror = {
    daemonEpoch: '1793803011230',
    capabilities: ['referenceHistoryDeltaV1', 'referencePayloadV1', 'referenceComposerControlV1'],
  };
  const machine = { ...mirror, capabilities: [...mirror.capabilities, 'terminalContinueV1'] };
  assert.equal(chatGatewayCapabilityAdvertised(mirror, 'terminalContinueV1'), false);
  assert.equal(chatGatewayCapabilityAdvertised(machine, 'terminalContinueV1'), true);
  assert.equal(chatGatewayCapabilityAdvertised({ capabilities: ['terminalContinueV1'] }, 'terminalContinueV1'), false);
  const fromPage = capabilityAdvertisedFromEntries([
    { method: 'GET', path: '/api/v1/capabilities', forwarded: true, upstreamBody: JSON.stringify(machine) },
  ], 'terminalContinueV1');
  assert.equal(fromPage.known, true);
  assert.equal(fromPage.advertised, true);
  assert.equal(capabilityAdvertisedFromEntries([], 'terminalContinueV1').known, false);
});

test('the outcome-unknown proxy drops the client socket only after forwarding', async () => {
  const rev3 = await readFile(new URL('./herdr-reference-chat-rev3.mjs', import.meta.url), 'utf8');
  const start = rev3.indexOf('export function startOutcomeUnknownProxy');
  const next = rev3.indexOf('function discoverySession', start);
  assert.ok(start > 0 && next > start);
  const fn = rev3.slice(start, next);
  const requestAt = fn.indexOf('http.request');
  const resumeAt = fn.indexOf('upstreamRes.resume()');
  const destroyAt = fn.indexOf('req.socket.destroy()');
  assert.ok(requestAt > 0, 'proxy does not forward');
  assert.ok(resumeAt > requestAt, 'proxy drops before the upstream response callback');
  assert.ok(destroyAt > resumeAt, 'proxy destroys the client before the host response is in hand');
  assert.match(fn, /server\.listen\(0, "127\.0\.0\.1"/);
  assert.match(fn, /outcome-unknown-proxy\.jsonl/);
  assert.match(rev3, /apiBase: proxy\.url/);
  assert.match(rev3, /measureControlOutcomeUnknown/);
  assert.match(rev3, /measureUnknownCreate/);
  assert.match(fn, /proxyUpstreamHeaders/);
  assert.match(fn, /relayedProxyResponseHeaders/);
  assert.match(fn, /res\.shouldKeepAlive = false/);
  assert.match(fn, /server\.keepAliveTimeout = 1/);
  assert.match(fn, /proxyBodyRequestId/);
  assert.match(fn, /captureUpstreamPrefix/);
  assert.match(rev3, /truncatedUpstreamBody/);
  assert.match(fn, /origin: " \+ upstream\.origin/);
});

test('claude fixture pane is never the omo fallback', () => {
  const omo = { registryId: 'omo', backendSessionId: 'omo-1', providerSessionId: 'omo-session' };
  const claude = { registryId: 'claude', backendSessionId: 'claude-1', providerSessionId: 'claude-session' };
  assert.equal(claudeFixturePane([omo, claude], omo).backendSessionId, 'claude-1');
  assert.equal(claudeFixturePane([omo], omo), null);
  assert.equal(claudeFixturePane([], claude).backendSessionId, 'claude-1');
  const bare = { registryId: 'claude', backendSessionId: 'claude-bare' };
  const withId = { registryId: 'claude', backendSessionId: 'claude-id', providerSessionId: 'id' };
  assert.equal(claudeFixturePane([bare, withId], null).backendSessionId, 'claude-id');
});

test('unknown-create skips a pane the claim hook already marked working', () => {
  const working = { registryId: 'claude', backendSessionId: 'f05a62af', providerSessionId: 'claimed' };
  const idle = { registryId: 'claude', backendSessionId: 'idle-pane', providerSessionId: 'planted' };
  const omo = { registryId: 'omo', backendSessionId: 'omo-1', providerSessionId: 'omo' };
  assert.equal(unknownCreatePane([working, idle], working, ['f05a62af']).backendSessionId, 'idle-pane');
  assert.equal(unknownCreatePane([working], working, []).backendSessionId, 'f05a62af');
  assert.equal(unknownCreatePane([working, omo], working, ['f05a62af']), null);
  assert.equal(unknownCreatePane([omo], omo, []), null);
  assert.equal(agentControlNonWorking('working'), false);
  assert.equal(agentControlNonWorking('idle'), true);
  assert.equal(agentControlNonWorking('unknown'), true);
  const hidden = classifyUnknownCreateAbsence({
    sessionId: 'f05a62af',
    reasons: ['sourceWorking'],
    agentState: 'working',
    historyAvailability: 'native',
  });
  assert.equal(hidden.status, 'unmeasured');
  assert.match(hidden.reason, /agentResumeAffordance\.ts:295-296/);
  assert.doesNotMatch(hidden.reason, /^PRODUCT:/);
  const unverified = classifyUnknownCreateAbsence({
    sessionId: 'idle-pane',
    reasons: [],
    agentState: 'idle',
    historyAvailability: 'scrollback',
  });
  assert.equal(unverified.status, 'unmeasured');
  assert.match(unverified.reason, /RemoteApp\.tsx:5371/);
  const product = classifyUnknownCreateAbsence({
    sessionId: 'idle-pane',
    reasons: ['remoteCapabilityAbsent'],
    agentState: 'idle',
    historyAvailability: 'native',
    tab: { found: true, agentType: 'claude', activityState: null, title: 'Terminal' },
    capabilityAdvertised: true,
    pageRow: {
      found: true,
      sessionId: 'idle-pane',
      agentType: 'claude',
      running: true,
      providerSessionId: '6b1e0c3a-a12b-4f00-9a00-0ca0aa000001',
      activityState: null,
    },
  });
  assert.equal(product.status, 'failed');
  assert.match(product.reason, /^PRODUCT:/);
  assert.match(product.reason, /RemoteApp\.tsx:5367-5371/);
  assert.match(product.reason, /agentResumeAffordance\.ts:344-360/);
  assert.match(product.reason, /agentResumeAffordance\.ts:345/);
  assert.match(product.reason, /6b1e0c3a-a12b-4f00-9a00-0ca0aa000001/);
  const unpublished = classifyUnknownCreateAbsence({
    sessionId: '8c76e1bf',
    reasons: [],
    agentState: 'unknown',
    historyAvailability: 'native',
    pageRow: {
      found: true,
      providerSessionId: null,
      activityState: null,
      agentType: 'claude',
      running: true,
    },
  });
  assert.equal(unpublished.status, 'unmeasured');
  assert.match(unpublished.reason, /providerSession/);
  assert.match(unpublished.reason, /agentResumeAffordance\.ts:352-353/);
  assert.match(unpublished.reason, /RemoteApp\.tsx:747-776/);
  assert.doesNotMatch(unpublished.reason, /^PRODUCT:/);
  const pageWorking = classifyUnknownCreateAbsence({
    sessionId: 'idle-pane',
    reasons: [],
    agentState: 'idle',
    historyAvailability: 'native',
    pageRow: { providerSessionId: '6b1e0c3a', activityState: 'working', agentType: 'claude' },
  });
  assert.equal(pageWorking.status, 'unmeasured');
  assert.match(pageWorking.reason, /agentResumeAffordance\.ts:295-296/);
  assert.doesNotMatch(pageWorking.reason, /^PRODUCT:/);
  const unread = classifyUnknownCreateAbsence({
    sessionId: 'idle-pane',
    reasons: [],
    agentState: '',
    historyAvailability: 'native',
  });
  assert.equal(unread.status, 'unmeasured');
});

test('QA-16 unknown-create is measured before the working claim', () => {
  const rev3 = readFileSync(new URL('./herdr-reference-chat-rev3.mjs', import.meta.url), 'utf8');
  const start = rev3.indexOf('const evidence = openEvidence(ctx, "QA-16", "failure");');
  const end = rev3.indexOf('function framesAfter', start);
  assert.ok(start > 0 && end > start);
  const body = rev3.slice(start, end);
  const measureAt = body.indexOf('measureUnknownCreate');
  const claimAt = body.indexOf('typePaneClaim');
  assert.ok(measureAt > 0 && claimAt > measureAt, 'unknown-create must run before the claim hook');
  const measureStart = rev3.indexOf('async function measureUnknownCreate');
  const measureEnd = rev3.indexOf('async function socketMessageText', measureStart);
  assert.ok(measureStart > 0 && measureEnd > measureStart);
  const measureBody = rev3.slice(measureStart, measureEnd);
  const idleAt = measureBody.indexOf('paneIdleHookLine');
  const openAt = measureBody.indexOf('deps.openChatPage');
  assert.ok(idleAt > 0 && openAt > idleAt, 'SessionStart must publish providerSession before the page opens');
  assert.ok(measureBody.indexOf('waitForPublishedProviderSession') > idleAt);
  const retargetAt = measureBody.indexOf('unknownCreateTranscriptCwd');
  assert.ok(retargetAt > measureBody.indexOf('waitForPublishedProviderSession'), 'store cwd stays the pane cwd until SessionStart is admitted');
  assert.ok(openAt > retargetAt, 'the create transcript cwd is the workspace root before the page opens');
  assert.ok(measureBody.indexOf('pageRow: fresh.pageRow') > openAt);
  assert.ok(measureBody.includes('capabilityAdvertisedFromEntries'), 'capability is read from the page capabilities response');
  assert.ok(measureBody.includes('CONTINUE_STRIP_WAIT_MS'), 'the strip wait must be bounded, not the scenario timeout');
  assert.ok(measureBody.includes('unknown-create-proxied.png'), 'the proxied page must be shot at the button check');
  assert.ok(measureBody.includes('readContinuationStrip'));
  const publishStart = rev3.indexOf('async function publishFixturePane');
  const publishEnd = rev3.indexOf('export function listingContainsPane', publishStart);
  assert.ok(publishStart > 0 && publishEnd > publishStart);
  assert.match(rev3.slice(publishStart, publishEnd), /terminalTabs: \[desktopContinuationTab\(pane\)\]/);
});

test('live desktop continuation tab omits running and providerSession; an exited pane publishes both', () => {
  const tab = desktopContinuationTab({ registryId: 'claude', backendSessionId: 'ae768781' });
  assert.deepEqual(tab, {
    id: 'ae768781',
    label: 'Terminal',
    sessionId: 'ae768781',
    agentType: 'claude',
  });
  assert.equal(Object.prototype.hasOwnProperty.call(tab, 'activityState'), false);
  assert.equal(Object.prototype.hasOwnProperty.call(tab, 'running'), false);
  assert.equal(Object.prototype.hasOwnProperty.call(tab, 'providerSession'), false);
  const state = {
    activeContext: {
      terminalTabs: [{ id: 'ae768781', sessionId: 'ae768781', label: 'Terminal', agentType: 'claude' }],
    },
  };
  assert.deepEqual(publishedWorkspaceTab(state, 'ae768781'), {
    found: true,
    agentType: 'claude',
    activityState: null,
    title: 'Terminal',
    running: null,
    providerSessionId: null,
  });
  const exited = desktopContinuationTab({
    registryId: 'claude',
    backendSessionId: 'ae768781',
    exited: true,
    providerSession: { key: 'session_id', id: '6b1e0c3a', transcriptPath: '/tmp/claude.jsonl' },
  });
  const exitedFromFixture = fillExitedAgentTab({
    tabId: 'ae768781',
    label: 'Terminal',
    agentType: 'claude',
    sessionId: 'ae768781',
    providerKey: 'session_id',
    providerSessionId: '6b1e0c3a',
    transcriptPath: '/tmp/claude.jsonl',
  });
  assert.deepEqual(exited, {
    id: 'ae768781',
    label: 'Terminal',
    sessionId: 'ae768781',
    agentType: 'claude',
    running: false,
    providerSession: { key: 'session_id', id: '6b1e0c3a', transcriptPath: '/tmp/claude.jsonl' },
  });
  assert.deepEqual(exited, exitedFromFixture);
  assert.equal(Object.prototype.hasOwnProperty.call(exited, 'activityState'), false);
  const standby = desktopContinuationTab({
    registryId: 'claude',
    backendSessionId: 'standby:pane-1',
    exited: true,
    providerSession: { key: 'session_id', id: '6b1e0c3a' },
  });
  assert.equal(Object.prototype.hasOwnProperty.call(standby, 'sessionId'), false);
  assert.equal(JSON.stringify(standby).includes('standby:'), false);
  assert.deepEqual(publishedWorkspaceTab({ activeContext: { terminalTabs: [exited] } }, 'ae768781'), {
    found: true,
    agentType: 'claude',
    activityState: null,
    title: 'Terminal',
    running: false,
    providerSessionId: '6b1e0c3a',
  });
  assert.equal(publishedWorkspaceTab({
    activeContext: { terminalTabs: [{ sessionId: 'other', agentType: 'omo' }] },
    sessions: [{ agentType: 'claude', target: { sessionId: 'ae768781' } }],
  }, 'ae768781').found, false);
});

test('continuation strip DOM records buttons, typed reasons, and the no-agent notice', () => {
  const parsed = parseContinuationStripDom({
    workspacePresent: true,
    notice: 'No agent is running in this terminal, so there is no chat to show.',
    nodes: [
      { testId: 'continue-unavailable-fork', reason: 'liveClaim' },
      { testId: 'continue-in-new-pane-resume', reason: null },
      { testId: 'continue-unavailable-resume', reason: '' },
    ],
  });
  assert.deepEqual(parsed.buttons, ['continue-in-new-pane-resume']);
  assert.deepEqual(parsed.unavailable, [
    { testId: 'continue-unavailable-fork', reason: 'liveClaim' },
    { testId: 'continue-unavailable-resume', reason: null },
  ]);
  assert.equal(parsed.noAgentNotice, true);
  assert.equal(parsed.workspacePresent, true);
  assert.equal(parseContinuationStripDom(null).noAgentNotice, false);
});

test('unknown-create absence is not PRODUCT when the workspace tab has no agentType', () => {
  const missing = classifyUnknownCreateAbsence({
    sessionId: 'ae768781',
    reasons: [],
    agentState: 'idle',
    historyAvailability: 'native',
    capabilityAdvertised: true,
    tab: { found: true, agentType: null, activityState: null, title: 'Terminal' },
    dom: { buttons: [], noAgentNotice: true },
    pageRow: {
      found: true,
      agentType: 'claude',
      providerSessionId: '6b1e0c3a-a12b-4f00-9a00-0ca0aa000001',
      activityState: null,
    },
  });
  assert.equal(missing.status, 'unmeasured');
  assert.match(missing.reason, /RemoteApp\.tsx:3594-3596/);
  assert.match(missing.reason, /RemoteApp\.tsx:5353-5354/);
  assert.match(missing.reason, /App\.tsx:717/);
  assert.doesNotMatch(missing.reason, /^PRODUCT:/);
  const dropped = classifyUnknownCreateAbsence({
    sessionId: 'ae768781',
    reasons: [],
    agentState: 'idle',
    historyAvailability: 'native',
    capabilityAdvertised: true,
    publishedTabAgentType: 'claude',
    tab: { found: true, agentType: null, activityState: null, title: 'Terminal' },
    pageRow: { providerSessionId: '6b1e0c3a-a12b-4f00-9a00-0ca0aa000001', activityState: null },
  });
  assert.equal(dropped.status, 'failed');
  assert.match(dropped.reason, /^PRODUCT:/);
  assert.match(dropped.reason, /daemon\/server\.rs:8153-8158/);
  assert.match(dropped.reason, /server\.rs:1080-1088/);
  const explained = classifyUnknownCreateAbsence({
    sessionId: 'ae768781',
    reasons: ['remoteCapabilityAbsent'],
    agentState: 'idle',
    historyAvailability: 'native',
    capabilityAdvertised: false,
    tab: { found: true, agentType: 'claude', activityState: null, title: 'Terminal' },
    pageRow: { providerSessionId: '6b1e0c3a', activityState: null },
  });
  assert.equal(explained.status, 'unmeasured');
  assert.match(explained.reason, /agentResumeAffordance\.ts:345/);
  assert.doesNotMatch(explained.reason, /^PRODUCT:/);
  const liveClaim = classifyUnknownCreateAbsence({
    sessionId: 'ae768781',
    reasons: ['liveClaim'],
    agentState: 'idle',
    historyAvailability: 'native',
    capabilityAdvertised: true,
    tab: { found: true, agentType: 'claude', activityState: null, title: 'Terminal' },
    pageRow: { providerSessionId: '6b1e0c3a', activityState: null },
  });
  assert.equal(liveClaim.status, 'unmeasured');
  assert.match(liveClaim.reason, /data-reason=liveClaim/);
  assert.doesNotMatch(liveClaim.reason, /^PRODUCT:/);
  const contradicted = classifyUnknownCreateAbsence({
    sessionId: 'ae768781',
    reasons: ['missingNativeHistory'],
    agentState: 'idle',
    historyAvailability: 'native',
    capabilityAdvertised: true,
    tab: { found: true, agentType: 'claude', activityState: null, title: 'Terminal' },
    pageRow: { providerSessionId: '6b1e0c3a-a12b-4f00-9a00-0ca0aa000001', activityState: null },
  });
  assert.equal(contradicted.status, 'failed');
  assert.match(contradicted.reason, /^PRODUCT:/);
  assert.match(contradicted.reason, /agentResumeAffordance\.ts:352-353/);
  assert.notEqual(continuationReasonContradiction({
    reasons: ['sourceWorking'],
    agentState: 'idle',
    activityState: '',
    tabAgentType: 'claude',
    capabilityAdvertised: true,
  }), null);
  assert.match(continuationReasonContradiction({
    reasons: ['sourceWorking'],
    agentState: 'idle',
    activityState: '',
    tabAgentType: 'claude',
    capabilityAdvertised: true,
  }), /agentResumeAffordance\.ts:355/);
});

test('continuationPageRow prefers the sessions-row providerSession and reads the tab only when that row is absent', () => {
  const sessions = {
    sessions: [{
      agentType: 'claude',
      running: true,
      target: { sessionId: '8c76e1bf' },
      providerSession: { key: 'session_id', id: '6b1e0c3a' },
    }],
  };
  const state = {
    activeContext: {
      terminalTabs: [{
        sessionId: '8c76e1bf',
        activityState: 'done',
        running: false,
        providerSession: { key: 'session_id', id: 'tab-only' },
      }],
    },
  };
  const row = continuationPageRow(sessions, state, '8c76e1bf');
  assert.equal(row.found, true);
  assert.equal(row.providerSessionId, '6b1e0c3a');
  assert.equal(row.agentType, 'claude');
  assert.equal(row.activityState, 'done');
  assert.equal(row.running, true);
  const bare = continuationPageRow({
    sessions: [{ agentType: 'claude', running: true, target: { sessionId: '8c76e1bf' } }],
  }, state, '8c76e1bf');
  assert.equal(bare.found, true);
  assert.equal(bare.providerSessionId, null);
  assert.equal(bare.running, true);
  const fromTab = continuationPageRow({ sessions: [] }, state, '8c76e1bf');
  assert.equal(fromTab.found, true);
  assert.equal(fromTab.providerSessionId, 'tab-only');
  assert.equal(fromTab.running, false);
  assert.equal(fromTab.activityState, 'done');
  assert.equal(continuationPageRow({ sessions: [] }, { activeContext: { terminalTabs: [] } }, '8c76e1bf').found, false);
});

test('QA-13 outcome-unknown stays unmeasured without a probe-free control', () => {
  const proof = composerControlOutcomeUnknownProof('da3994ac');
  assert.match(proof, /MobileChatComposer\.tsx:535-537/);
  assert.match(proof, /server\.rs:7381-7407/);
  assert.match(proof, /control\.rs:514-540 and 543-577/);
  assert.match(proof, /model_discovery\.rs:32-52/);
  assert.match(proof, /MobileChatComposer\.tsx:977-984/);
  assert.match(proof, /RemoteApp\.tsx:4848-4856/);
  assert.match(proof, /referenceControl\.ts:15-16/);
  assert.match(proof, /does not POST \/control/);
  assert.match(proof, /there is no mode control/);
});

test('proxy headers rewrite Origin and Referer to the upstream origin', () => {
  const upstream = new URL('http://127.0.0.1:43821');
  const headers = proxyUpstreamHeaders({
    host: '127.0.0.1:9',
    origin: 'http://127.0.0.1:9',
    referer: 'http://127.0.0.1:9/#session=claude-1',
    authorization: 'Bearer secret',
  }, upstream);
  assert.equal(headers.host, '127.0.0.1:43821');
  assert.equal(headers.origin, 'http://127.0.0.1:43821');
  assert.equal(headers.referer, 'http://127.0.0.1:43821/#session=claude-1');
  assert.equal(headers.authorization, 'Bearer secret');
  assert.equal(truncatedUpstreamBody(200, 'ok'), undefined);
  assert.equal(truncatedUpstreamBody(403, '{"error":{"code":"MACHINE_ACCESS_REQUIRED"}}'), '{"error":{"code":"MACHINE_ACCESS_REQUIRED"}}');
  assert.match(truncatedUpstreamBody(403, 'x'.repeat(600), 600), /…\(600 chars\)$/);
});

test('continuation acceptance requires the host argv and one new pane', () => {
  const id = '6b1e0c3a-a12b-4f00-9a00-0ca0aa000001';
  assert.deepEqual(expectedContinueArgv('claude', id, 'fork'), ['claude', '--resume', id, '--fork-session']);
  assert.equal(argvMatchesContinue(['/usr/bin/claude', '--resume', id, '--fork-session'], expectedContinueArgv('claude', id, 'fork')), true);
  assert.equal(argvMatchesContinue(['claude', '--resume', id], expectedContinueArgv('claude', id, 'fork')), false);
  const fork = expectedContinueArgv('claude', id, 'fork');
  assert.deepEqual(matchingContinueArgv([
    ['/bin/sh', '-c', 'script', 'ferryx-agent', '/bin/bash', '', '/usr/bin/claude', '--resume', id, '--fork-session'],
    ['/usr/bin/claude', '--resume', id, '--fork-session'],
  ], fork), ['/usr/bin/claude', '--resume', id, '--fork-session']);
  assert.equal(matchingContinueArgv([['/bin/sh', '-c', 'script']], fork), null);
  assert.equal(matchingContinueArgv([['claude', '--resume', 'other', '--fork-session']], fork), null);
  assert.equal(numericPid(null), null);
  assert.equal(numericPid(0), null);
  assert.equal(numericPid('42'), 42);
  assert.equal(numericPid({ pid: 7 }), 7);
  const source = { id: 'source', running: true, pid: 1, cwd: '/repo', workspaceId: 'ws', worktree: null, providerSessionId: id, cols: 80, rows: 24 };
  const created = { id: 'new', running: true, pid: 2, cwd: '/repo', workspaceId: 'ws', worktree: null, providerSessionId: null, cols: 80, rows: 24 };
  assertContinueAcceptance({
    mode: 'fork',
    agentType: 'claude',
    providerSessionId: id,
    sourceId: 'source',
    createdSessionId: 'new',
    workspaceId: 'ws',
    worktree: null,
    before: [source],
    after: [source, created],
    argv: ['claude', '--resume', id, '--fork-session'],
  });
  assert.throws(() => assertContinueAcceptance({
    mode: 'fork',
    agentType: 'claude',
    providerSessionId: id,
    sourceId: 'source',
    createdSessionId: 'new',
    workspaceId: 'ws',
    worktree: null,
    before: [source],
    after: [source],
    argv: ['claude', '--resume', id, '--fork-session'],
  }), /one new pane|argv|missing|new pane/);
  assert.throws(() => assertContinueAcceptance({
    mode: 'fork',
    agentType: 'claude',
    providerSessionId: id,
    sourceId: 'source',
    createdSessionId: 'new',
    workspaceId: 'ws',
    worktree: null,
    before: [source],
    after: [source, created],
    argv: ['bash'],
  }), /argv/);
});

test('machine create success is HTTP 201, not 200', () => {
  assert.equal(continuationCreateAccepted(201), true);
  assert.equal(continuationCreateAccepted(200), false);
  assert.equal(continuationCreateAccepted(409), false);
});

test('original-preserved baseline is the post-claim inventory', () => {
  const pre = { sessions: [{ id: 'source', providerSessionId: null }] };
  const post = { sessions: [{ id: 'source', providerSessionId: 'claimed' }] };
  assert.equal(preservedBaseline(pre, post, 'source'), post);
  assert.equal(preservedBaseline(pre, { sessions: [] }, 'source'), pre);
  assert.equal(preservedBaseline(pre, null, 'source'), pre);
});

test('planted cwd is the pane directory and continuation requests that child', () => {
  const session = {
    cwd: '/repo/.ferryx-qa-panes/pane-1',
    workspaceRegistration: { repoRoot: '/repo' },
  };
  assert.equal(plantCwdOf(session), '/repo/.ferryx-qa-panes/pane-1');
  assert.equal(continuationCwdRelative(session), '.ferryx-qa-panes/pane-1');
  assert.equal(continuationCwdRelative({ workspaceRegistration: { repoRoot: '/repo' }, cwd: '/repo' }), null);
  assert.equal(continuationCwdRelative({ workspaceRegistration: { repoRoot: '/repo' }, cwd: '/repo/../escape' }), null);
  assert.equal(isolatedPaneCwd('/repo', 'pane/1'), '/repo/.ferryx-qa-panes/pane_1');
});

test('a listed pane is recognized in workspace state and the session inventory', () => {
  const id = 'claude-1';
  assert.equal(listingContainsPane({ activeContext: { terminalTabs: [{ sessionId: id }] } }, id), true);
  assert.equal(listingContainsPane({ sessions: [{ target: { sessionId: id } }] }, id), true);
  assert.equal(listingContainsPane({ sessions: [{ id: 'other' }] }, id), false);
  assert.equal(statePublishesPane({ activeContext: { sessionId: id, terminalTabs: [{ id, sessionId: id }] } }, id), true);
  assert.equal(statePublishesPane({ sessions: [{ target: { sessionId: id } }] }, id), false);
  assert.equal(statePublishesPane({ activeContext: { terminalTabs: [{ id, sessionId: null }] } }, id), false);
  assert.deepEqual(workspaceStateSummary({ activeContext: { sessionId: id, terminalTabs: [{ id, sessionId: id }] } }, id).tabs, [{ id, sessionId: id }]);
  assert.equal(chatLaneBound({ path: '/api/v1/reference-chat/' + id + '/acknowledge-outcome?ownerId=1' }, id), true);
  assert.equal(chatLaneBound({ path: '/api/v1/reference-chat/other/files' }, id), false);
  assert.equal(chatLaneBound({ path: '/api/v1/terminal/' + id }, id), true);
  assert.equal(proxyEntryMirrored({ method: 'GET', path: '/api/v1/workspace/sidebar', upstreamStatus: 403 }), true);
  assert.equal(proxyEntryMirrored({ method: 'GET', path: '/assets/app.js', upstreamStatus: 200 }), false);
  assert.equal(proxyEntryMirrored({ method: 'LISTEN', path: 'http://127.0.0.1:9', upstreamStatus: 0 }), true);
  assert.equal(proxyEntryMirrored({ method: 'GET', path: '/api/v1/health', upstreamStatus: 409, targeted: false }), true);
});

test('payloadRefs keeps an outputPayload ref and a diff ref', () => {
  const refs = payloadRefs({
    turns: [{
      parts: [
        { kind: 'tool', outputPayload: { ref: 'fp1.out', byteLength: 4001, digest: 'abc' } },
        { kind: 'diff', patch: { ref: 'fp1.diff', byteLength: 9000, digest: 'def' } },
        { kind: 'text', text: 'kept' },
      ],
    }],
  });
  assert.deepEqual(refs.map((ref) => ref.kind), ['output', 'diff']);
  assertPayloadMatchesRef({ status: 200, json: { status: 'loaded', kind: 'output', byteLength: 4001, digest: 'abc', truncated: false } }, refs[0]);
  assert.throws(
    () => assertPayloadMatchesRef({ json: { status: 'loaded', kind: 'output', byteLength: 1, digest: 'abc', truncated: false } }, refs[0]),
    /byteLength/,
  );
});

test('qa15 fixture keeps existing rows and adds a tool output and a diff above the externalization thresholds', async () => {
  const claude = await readFile(new URL('./fixtures/herdr-reference-chat/qa15-claude.jsonl', import.meta.url), 'utf8');
  const history = await readFile(new URL('../../src-tauri/src/remote/reference_chat/history_claude.rs', import.meta.url), 'utf8');
  const parse = await readFile(new URL('../../src-tauri/src/remote/reference_chat/history_parse.rs', import.meta.url), 'utf8');
  const outputLimit = Number(history.match(/REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS: usize = ([\d_]+)/)[1].replaceAll('_', ''));
  const diffExpr = parse.match(/REFERENCE_NATIVE_DIFF_LIMIT: usize = (\d+) \* (\d+)/);
  const diffLimit = Number(diffExpr[1]) * Number(diffExpr[2]);
  assert.equal(outputLimit, 4000);
  assert.equal(diffLimit, 8192);
  assert.match(claude, /"id":"toolu_01"/);
  assert.match(claude, /"id":"toolu_qa15_open"/);
  const lines = claude.split('\n').filter((line) => line.length > 0).map((line) => JSON.parse(line));
  const payload = lines.find((line) => JSON.stringify(line).includes('toolu_qa15_payload') && line.type === 'user');
  const content = payload.message.content[0].content;
  assert.ok(content.length > outputLimit, 'tool output is not above REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS');
  const edit = lines.find((line) => JSON.stringify(line).includes('toolu_qa15_diff') && line.type === 'assistant');
  const input = edit.message.content[0].input;
  const body = JSON.stringify({ old_string: input.old_string, new_string: input.new_string });
  assert.ok(body.length > diffLimit, 'edit body is not above REFERENCE_NATIVE_DIFF_LIMIT');
});

test('desktop resume source is a daemon spawn, not a machine journal create', () => {
  const request = desktopOwnedSpawnRequest({
    workspaceId: 'orca-qa-ws',
    cwd: '/repo/.ferryx-qa-panes/req-1',
    requestId: 'req-1',
    cols: 80,
    rows: 24,
    shell: null,
  });
  assert.equal(request.type, 'spawn');
  assert.equal(request.clientRequestId, 'req-1');
  assert.equal(request.workspaceId, 'orca-qa-ws');
  assert.equal(request.cwd, '/repo/.ferryx-qa-panes/req-1');
  assert.equal(request.shell, null);
  assert.equal(request.startup, null);
  assert.equal(Object.prototype.hasOwnProperty.call(request, 'kind'), false);
  assert.deepEqual(desktopSpawnIdentity({ type: 'spawnOk', sessionId: 'pane-1', epoch: 7 }), {
    sessionId: 'pane-1',
    epoch: '7',
  });
  assert.deepEqual(desktopSpawnIdentity({ type: 'error' }), { sessionId: null, epoch: null });
  assert.deepEqual(desktopPaneCloseRequest('pane-1'), { type: 'close', sessionId: 'pane-1' });
  assert.throws(() => desktopOwnedSpawnRequest({ workspaceId: 'ws', cwd: 'relative', requestId: 'req' }), /cwd/);
});

test('exited desktop publication is the sessions row, the published tab, or a T18/T19 product gap', () => {
  const retained = exitPublication({
    ownership: 'desktop',
    published: true,
    rowFound: true,
    running: false,
    providerSessionId: 'a590811a-b87a-4a97-9c54-ff7648cec0bb',
    workspaceFound: true,
    workspaceRunning: true,
  });
  assert.equal(retained.path, 'sessions-row');
  assert.equal(retained.running, false);
  assert.equal(retained.citation, 'session_service.rs:1164-1167');
  assert.equal(retained.productGap, null);
  const tabPath = exitPublication({
    ownership: 'desktop',
    published: true,
    rowFound: false,
    running: null,
    providerSessionId: null,
    workspaceFound: false,
    workspaceRunning: null,
    tabRunning: false,
    tabProviderSessionId: 'a590811a-b87a-4a97-9c54-ff7648cec0bb',
  });
  assert.equal(tabPath.path, 'desktop-tab');
  assert.equal(tabPath.running, false);
  assert.equal(tabPath.productGap, null);
  assert.equal(tabPath.citation, 'App.tsx exited tab publication');
  const dropped = exitPublication({
    ownership: 'desktop',
    published: true,
    rowFound: false,
    running: null,
    providerSessionId: null,
    workspaceFound: true,
    workspaceRunning: true,
  });
  assert.equal(dropped.path, 'none');
  assert.equal(dropped.running, null);
  assert.equal(dropped.workspaceRunning, true);
  assert.match(dropped.citation, /pty\.rs:736-738/);
  assert.match(dropped.citation, /pty\.rs:822-824/);
  assert.match(dropped.citation, /server\.rs:1207-1214/);
  assert.match(dropped.citation, /RemoteSessionList\.tsx:313-316/);
  assert.match(dropped.productGap, /^PRODUCT:/);
  assert.match(dropped.productGap, /T18/);
  assert.match(dropped.productGap, /T19/);
  assert.match(dropped.productGap, /pty\.rs:736-738/);
  assert.equal(exitedDesktopResumeGap({
    ownership: 'desktop',
    published: true,
    rowFound: true,
    running: false,
    providerSessionId: 'kept',
  }), null);
  assert.equal(exitedDesktopResumeGap({
    ownership: 'desktop',
    published: true,
    rowFound: false,
    running: null,
    providerSessionId: null,
    tabRunning: false,
    tabProviderSessionId: 'kept',
  }), null);
  assert.equal(publishedSessionRunning({
    sessions: [{ sessionId: 'pane-1', running: true }],
  }, 'pane-1').running, true);
  assert.equal(publishedSessionRunning({ sessions: [] }, 'pane-1').found, false);
  const rev3 = readFileSync(new URL('./herdr-reference-chat-rev3.mjs', import.meta.url), 'utf8');
  const start = rev3.indexOf('async function measureResumeExited');
  const end = rev3.indexOf('export function createRev3Scenarios', start);
  assert.ok(start > 0 && end > start);
  const body = rev3.slice(start, end);
  assert.equal(body.includes('throwawayShellCreate'), false);
  assert.equal(body.includes('desktopOwnedSpawnRequest'), true);
  assert.equal(body.includes('desktopPaneCloseRequest'), true);
  assert.equal(body.includes('exitPublication'), true);
  assert.equal(body.includes('resume-exit-publication'), true);
  assert.equal(body.includes('resume-strip.png'), true);
  assert.equal(body.includes('assertExitedSourceStaysAbsent'), true);
  assert.equal(body.includes('sourceId: identity.sessionId'), false);
  assert.equal(body.includes('resumeStartup.providerSession.id !== providerSessionId'), true);
  const absentAt = body.indexOf('assertExitedSourceStaysAbsent(');
  const preservedAt = body.indexOf('assertOriginalSessionPreserved(');
  const acceptanceAt = body.indexOf('assertResumeAcceptance({');
  assert.ok(absentAt > 0 && preservedAt > absentAt && acceptanceAt > preservedAt);
  assert.equal(body.slice(preservedAt, acceptanceAt).includes('sourceId: session.backendSessionId'), true);
  assert.equal(body.slice(acceptanceAt, acceptanceAt + 500).includes('sourceId: session.backendSessionId'), true);
  const exitedAt = body.indexOf('pane.exited = true');
  const publishAt = body.indexOf('publishFixturePane', exitedAt);
  const waitAt = body.indexOf('waitForExitedResumeRow', publishAt);
  assert.ok(exitedAt > 0 && publishAt > exitedAt && waitAt > publishAt, 'exited tab is published before the publication wait');
  const waitStart = rev3.indexOf('async function waitForExitedResumeRow');
  const waitEnd = rev3.indexOf('async function callDesktopDaemon', waitStart);
  assert.ok(waitStart > 0 && waitEnd > waitStart);
  const waitBody = rev3.slice(waitStart, waitEnd);
  assert.match(waitBody, /let inventory = null/);
  assert.match(waitBody, /\/api\/v1\/workspace\/state/);
  assert.match(waitBody, /tab\.running === false && tab\.providerSessionId === providerSessionId/);
  assert.doesNotMatch(waitBody, /const inventory = await sessionInventory/);
});

test('resume plant uses a cwd-derived claude project slug and the shell pwd probe', () => {
  assert.equal(claudeProjectSlug('/private/tmp/parity-claude-59r7'), '-private-tmp-parity-claude-59r7');
  assert.equal(claudeProjectSlug('/home/indo/code/ferryx-wt/orca-chat-qa4-20261104'), '-home-indo-code-ferryx-wt-orca-chat-qa4-20261104');
  assert.equal(claudeProjectSlug('relative'), null);
  assert.equal(claudeProjectSlug('/'), null);
  const probed = "user@host:~$ pwd; printf '\\nQA16_PWD_%s\\n' SENT\r\n/home/indo/code/ferryx-wt/orca-chat-qa4-20261104\r\nQA16_PWD_SENT\r\n";
  assert.equal(pwdFromProbe(probed), '/home/indo/code/ferryx-wt/orca-chat-qa4-20261104');
  assert.equal(pwdFromProbe('QA16_PWD_SENT\n'), null);
  assert.equal(pwdFromProbe('no marker'), null);
});
