/**
 * QA-12..QA-16 for the herdr reference-chat acceptance runner.
 *
 * These rows follow the same isolated-gateway contract as QA-01..11: the runner's own
 * throwaway profile, port 43821, --allow-host, and real HTTP/event/DOM receipts. A provider
 * or device lane with no credential is unmeasured N/A. It is never a pass, and it does not
 * stop the other lanes in the same branch. A missing product route is a failure that names
 * the file and function; it is not rewritten into N/A.
 *
 * Assertion functions are pure. The live branches call them on captured receipts.
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync, copyFileSync, readdirSync, realpathSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import http from "node:http";
import net from "node:net";
import {
  connectDaemonControl,
  isolatedPaneCwd,
  readCredentialToken,
  readGatewayCapabilities,
  referenceAuthorityOwner,
  referenceRequest,
  targetFor,
} from "./herdr-reference-fixtures.mjs";
import { fillExitedAgentTab } from "./fixtures/exited-agent-tab.mjs";

export const REV3_QA_IDS = Object.freeze(["QA-12", "QA-13", "QA-14", "QA-15", "QA-16"]);

export const REV3_QA_ROWS = Object.freeze([
  {
    id: "QA-12",
    ideal: "IS-12",
    title: "event, replacement delta, reset, reconnect, and rev2 fallback",
    transport: "local",
    happy: "a referenceHistoryChanged frame triggers one fenced delta or inline reset at 390px coarse and 1280px fine",
    failure: "cursor+after is refused, a foreign anchor resets instead of merging, and a rev2 host is polled with full pages only",
  },
  {
    id: "QA-13",
    ideal: "IS-13",
    title: "composer model and option controls",
    transport: "local",
    happy: "an advertised model/option set is shown and one apply is accepted",
    failure: "rejected and outcome-unknown applies are not retried, and an unsupported provider shows no controls",
  },
  {
    id: "QA-14",
    ideal: "IS-14",
    title: "queue, steer, duplicate send, and probed dictation",
    transport: "local",
    happy: "two Send activations before settle produce one request id and one post; a queued row is delivered once on idle",
    failure: "steer is hidden unless advertised, and dictation is hidden when the probe fails",
  },
  {
    id: "QA-15",
    ideal: "IS-15",
    title: "provider-attributed work rows and lazy payloads",
    transport: "local",
    happy: "recorded native rich rows render, and a bound payload ref loads once",
    failure: "a stale or foreign ref is a typed refusal, restart is unverifiable, and an unsupported provider keeps its current cards",
  },
  {
    id: "QA-16",
    ideal: "IS-16",
    title: "new-pane continuation leaves the source unchanged",
    transport: "local",
    happy: "resume-only on an exited source and fork-native on an idle source create a distinct session",
    failure: "a working source, a duplicate claim, and an unknown create are refused without retry and without touching the source",
  },
]);

export const HISTORY_CHANGED_TYPE = "referenceHistoryChanged";
export const DELTA_CAPABILITY = "referenceHistoryDeltaV1";
export const CONTINUE_CAPABILITY = "terminalContinueV1";
export const PAYLOAD_CAPABILITY = "referencePayloadV1";
export const COMPOSER_CAPABILITY = "referenceComposerControlV1";

export const RESET_REASONS = Object.freeze([
  "generationChanged",
  "anchorOutsideWindow",
  "anchorNotTurnStart",
  "anchorPrevMismatch",
  "tooFarBehind",
  "unalignedTurns",
  "notNative",
  "unsupportedProjection",
]);

export const CONTROL_OUTCOMES = Object.freeze(["applied", "rejected", "unknown"]);
export const WORK_STATUSES = Object.freeze(["running", "completed", "failed", "unverifiable"]);
export const DISCOVERY_PROVIDERS = Object.freeze(["claude", "codex", "grok"]);
export const FORK_NATIVE_PROVIDERS = Object.freeze(["claude", "codex"]);

/**
 * Registry ids with a native reader. Port of
 * `ReferenceNativeHistoryKind::from_registry_id`
 * (src-tauri/src/remote/reference_chat/types.rs:201-220). Anything else is
 * `Unavailable` (types.rs:1496-1502: droid, aider, mimo-code, prime-agent,
 * openclaw). history.rs:411-424 shows that pane as scrollback, not a
 * provider-attributed rich row.
 */
export const RICH_ROW_PROVIDERS = Object.freeze([
  "claude",
  "codex",
  "omp",
  "omo",
  "gjc",
  "pi",
  "copilot",
  "antigravity",
  "agy",
  "kimi",
  "grok",
  "cline",
  "opencode",
  "cursor",
  "cursor-agent",
  "crush",
]);

export function richRowProviderSupported(registryId) {
  return RICH_ROW_PROVIDERS.includes(String(registryId || "").trim().toLowerCase());
}

/** Named product gaps. A live receipt that matches one fails the lane with this text. */
export const PRODUCT_GAPS = Object.freeze({
  historyAfter:
    "reference_chat_history returned a rev2 page for an after query (server.rs ReferenceChatReadQuery.after was not bound, so reference_chat_after_anchor saw none / history_delta.rs reference_history_negotiate returns the rev2 page)",
  controlRoute:
    "POST reference_chat_control returned an untyped 404 (server.rs route table)",
  payloadRoute:
    "GET reference_chat_payload returned an untyped 404 (server.rs payload route /api/v1/reference-chat/{sessionId}/payload, handler reference_chat_payload, is not registered)",
  agentContinue:
    "session_api.rs decode_create refused agentContinue before continue_startup::admit_continue",
});

const FIXTURE_DIR = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "herdr-reference-chat");
const PATH_LEAK = /(?:\/Users\/|\/home\/|\/var\/|\/tmp\/|[A-Za-z]:\\)/;

/**
 * Turn-part display the user is meant to see. `summary` is that line for a tool:
 * claude_tool_summary copies file_path (history_claude.rs). The product rule
 * (.omo/plans/orca-chat-priority-adoption.md:29) is host paths out of event frames
 * and payload URLs, not out of transcript content.
 */
const PART_DISPLAY_KEYS = new Set(["text", "input", "output", "title", "command", "summary"]);

function isPayloadIdentityKey(key) {
  return key === "ref"
    || key === "url"
    || key === "generation"
    || key === "cursor"
    || key === "streamId"
    || key === "outputPayload"
    || key === "outputRef"
    || key === "patch"
    || key.endsWith("Ref")
    || key.endsWith("Url");
}

/** Envelope and metadata, plus every ref/url/generation/cursor/streamId, including refs inside parts. */
function hostPathScanText(value) {
  const pieces = [];
  collectHostPathScan(value, pieces, false);
  return pieces.join("\n");
}

function collectHostPathScan(value, pieces, force) {
  if (value === null || value === undefined) return;
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    pieces.push(String(value));
    return;
  }
  if (typeof value !== "object") return;
  if (Array.isArray(value)) {
    for (const entry of value) collectHostPathScan(entry, pieces, force);
    return;
  }
  for (const [key, entry] of Object.entries(value)) {
    if (isPayloadIdentityKey(key)) {
      collectHostPathScan(entry, pieces, true);
      continue;
    }
    if (!force && PART_DISPLAY_KEYS.has(key)) continue;
    collectHostPathScan(entry, pieces, force);
  }
}

function assertNoEnvelopeHostPath(body, label) {
  if (PATH_LEAK.test(hostPathScanText(body))) fail(label + " leaks a host path");
}

function fail(message) {
  throw new Error(message);
}

function envelopeOf(body) {
  if (body === null || body === undefined || typeof body !== "object") return "none";
  if (typeof body.ok === "boolean") return "scope";
  if (body.error && typeof body.error === "object" && typeof body.error.code === "string") return "machine";
  return "other";
}

function errorCode(body) {
  if (!body || typeof body !== "object") return null;
  if (body.error && typeof body.error.code === "string") return body.error.code;
  return null;
}

function isUnregistered(result) {
  if (!result || result.status !== 404) return false;
  return envelopeOf(result.json) === "none";
}

/**
 * A bodyless 404 on a route this revision adds is the route not being registered, not a
 * typed refusal the lane can accept.
 */
export function explainHostGap(kind, result) {
  if (!isUnregistered(result)) return null;
  const text = PRODUCT_GAPS[kind];
  if (!text) fail("unknown product gap kind " + kind);
  return text;
}

/**
 * A 200 body with `turns` and no `mode` is the rev2 page. That is what
 * `reference_history_negotiate` returns when `ReferenceChatReadQuery` did not bind
 * `after`, so `reference_chat_after_anchor` never saw the query.
 * That is not a delta and not a reset.
 */
export function explainDeltaIgnored(result) {
  if (!result || result.status !== 200 || !result.json || typeof result.json !== "object") return null;
  if (result.json.mode === "delta" || result.json.mode === "reset") return null;
  if (!Array.isArray(result.json.turns)) return null;
  return PRODUCT_GAPS.historyAfter;
}

export function assertHistoryChangedFrame(frame) {
  if (!frame || typeof frame !== "object" || Array.isArray(frame)) fail("referenceHistoryChanged frame is not an object");
  if (frame.type !== HISTORY_CHANGED_TYPE) fail("expected type referenceHistoryChanged, got " + String(frame.type));
  const target = frame.target;
  if (!target || typeof target !== "object") fail("frame carries no target");
  for (const field of ["hostId", "ownerId", "epoch", "backendSessionId"]) {
    if (target[field] === undefined || target[field] === null || String(target[field]).length === 0) {
      fail("frame target has no " + field);
    }
  }
  if (typeof frame.changeSeq !== "string" || frame.changeSeq.length === 0) fail("frame changeSeq is not a non-empty string");
  for (const key of ["path", "stream", "content", "model", "transcript", "turns", "text"]) {
    if (Object.prototype.hasOwnProperty.call(frame, key)) fail("frame leaks " + key);
  }
  if (PATH_LEAK.test(JSON.stringify(frame))) fail("frame leaks a host path");
  return frame;
}

export function assertDeltaResponse(body) {
  if (!body || body.mode !== "delta") fail("expected mode delta, got " + (body && body.mode));
  if (typeof body.source !== "string" || body.source.length === 0) fail("delta has no source");
  if (typeof body.generation !== "string" || body.generation.length === 0) fail("delta has no generation");
  if (typeof body.replaceFromTurnId !== "string" || body.replaceFromTurnId.length === 0) {
    fail("delta has no replaceFromTurnId");
  }
  if (!Array.isArray(body.turns) || body.turns.length === 0) fail("delta does not re-send the anchor turn");
  if (body.turns[0].id !== body.replaceFromTurnId) fail("delta does not start at the anchor turn");
  if (!body.nextAnchor || typeof body.nextAnchor !== "object") fail("delta has no nextAnchor");
  if (typeof body.tailOpen !== "boolean") fail("delta tailOpen is not a boolean");
  assertNoEnvelopeHostPath(body, "delta");
  return body;
}

export function assertResetResponse(body, reason) {
  if (!body || body.mode !== "reset") fail("expected mode reset, got " + (body && body.mode));
  if (!RESET_REASONS.includes(body.reason)) fail("reset reason is not frozen: " + body.reason);
  if (reason !== undefined && body.reason !== reason) fail("expected reset reason " + reason + ", got " + body.reason);
  const page = body.page;
  if (!page || !Array.isArray(page.turns) || typeof page.generation !== "string" || typeof page.source !== "string") {
    fail("reset did not include a complete newest page");
  }
  assertNoEnvelopeHostPath(body, "reset");
  return body;
}

export function assertCursorPlusAfterRefused(result) {
  if (!result || result.status !== 400) fail("cursor+after expected HTTP 400, got " + (result && result.status));
  if (envelopeOf(result.json) !== "machine") fail("cursor+after expected the machine envelope");
  if (errorCode(result.json) !== "INVALID_REQUEST") fail("cursor+after expected INVALID_REQUEST, got " + errorCode(result.json));
  return result.json.error;
}

export function assertRev2Fallback(observation) {
  if (!observation || observation.capabilityAdvertised !== false) {
    fail("rev2 fallback applies only when referenceHistoryDeltaV1 is absent");
  }
  const requests = observation.historyRequests;
  if (!Array.isArray(requests) || requests.length < 1) fail("rev2 fallback issued no full-page poll");
  for (const request of requests) {
    const query = String(request.query || "");
    if (/(^|[?&])after=/.test(query)) fail("rev2 client issued a delta query: " + query);
  }
  return observation;
}

export function assertWidthExercise(observed) {
  if (!Array.isArray(observed)) fail("width exercise is not a list");
  const seen = new Set(observed.map((row) => row.viewport.width + ":" + (row.coarse ? "coarse" : "fine")));
  if (!seen.has("390:coarse")) fail("390px coarse pointer was not exercised");
  if (!seen.has("1280:fine")) fail("1280px fine pointer was not exercised");
  for (const row of observed) {
    if (row.coarse && row.pointer !== "coarse") fail("390px context did not report a coarse pointer");
    if (!row.coarse && row.pointer !== "fine") fail("1280px context did not report a fine pointer");
    if (row.chatVisible !== true) fail("chat stream was not visible at " + row.viewport.width);
    if (typeof row.screenshot !== "string" || row.screenshot.length === 0) fail("width " + row.viewport.width + " has no screenshot");
  }
  return observed;
}

export function assertComposerCapabilities(doc) {
  if (doc === null || doc === undefined) return null;
  if (typeof doc !== "object" || Array.isArray(doc)) fail("composer capabilities are not an object");
  if (doc.models !== null && doc.models !== undefined) {
    if (typeof doc.models !== "object") fail("composer.models is not an object");
    if (doc.models.current !== null && typeof doc.models.current !== "string") fail("composer.models.current is not a string or null");
    if (!Array.isArray(doc.models.choices)) fail("composer.models.choices is not an array");
    if (doc.models.apply !== "command" && doc.models.apply !== "agent-picker") {
      fail("composer.models.apply is not command or agent-picker");
    }
    for (const choice of doc.models.choices) {
      if (typeof choice.id !== "string" || typeof choice.label !== "string" || !Array.isArray(choice.efforts)) {
        fail("a model choice is missing id, label, or efforts");
      }
    }
  }
  if (!Array.isArray(doc.options)) fail("composer.options is not an array");
  for (const option of doc.options) {
    if (typeof option.id !== "string" || typeof option.label !== "string" || !Array.isArray(option.values)) {
      fail("an option is missing id, label, or values");
    }
    if (!["command", "agent-picker", "start-only"].includes(option.apply)) fail("option apply is not a frozen mode");
  }
  if (!Array.isArray(doc.slashCommands)) fail("composer.slashCommands is not an array");
  if (!doc.followUp || typeof doc.followUp.queue !== "boolean" || typeof doc.followUp.steer !== "boolean") {
    fail("composer.followUp does not advertise queue and steer as booleans");
  }
  return doc;
}

export function assertControlOutcome(result, expected) {
  if (!CONTROL_OUTCOMES.includes(expected)) fail("unknown control outcome " + expected);
  if (!result || result.status !== 200) fail("control " + expected + " was HTTP " + (result && result.status));
  if (envelopeOf(result.json) !== "scope" || result.json.ok !== true) fail("control " + expected + " was not a scope success");
  const outcome = result.json.data && result.json.data.outcome;
  if (outcome !== expected) fail("expected control outcome " + expected + ", got " + outcome);
  if (expected === "unknown") {
    if (result.json.data.retryable === true) fail("unknown apply was marked retryable");
    if (result.json.data.code !== "OPERATION_OUTCOME_UNKNOWN") fail("unknown apply did not carry OPERATION_OUTCOME_UNKNOWN");
  }
  return result.json.data;
}

export function assertOutcomeUnknownNotRetried(posts, requestId) {
  if (!Array.isArray(posts)) fail("control posts are not a list");
  const matches = posts.filter((post) => post.requestId === requestId);
  if (matches.length !== 1) fail("outcome-unknown request " + requestId + " was posted " + matches.length + " times");
  const outcome = matches[0].outcome || matches[0].code;
  if (outcome !== "unknown" && outcome !== "OPERATION_OUTCOME_UNKNOWN") {
    fail("the single post was not an unknown outcome");
  }
  return matches[0];
}

export function assertUnsupportedProviderHidesControls(observation) {
  if (!observation || observation.supported !== false) fail("hide-controls applies to an unsupported provider");
  if (observation.composerModelsPresent) fail("unsupported provider published model controls");
  if (observation.modelControlVisible || observation.optionControlVisible) fail("unsupported provider rendered composer controls");
  if (observation.textChatVisible !== true) fail("unsupported provider hid text chat");
  return observation;
}

export function assertDuplicateSendOneRequest(posts) {
  if (!Array.isArray(posts) || posts.length !== 1) fail("duplicate Send produced " + (posts ? posts.length : 0) + " posts");
  if (typeof posts[0].requestId !== "string" || posts[0].requestId.length === 0) fail("duplicate Send has no request id");
  const ids = new Set(posts.map((post) => post.requestId));
  if (ids.size !== 1) fail("duplicate Send used more than one request id");
  return posts[0];
}

export function assertQueuedDeliveredOnce(rows) {
  if (!Array.isArray(rows)) fail("queued rows are not a list");
  const delivered = rows.filter((row) => row.delivered === true);
  if (delivered.length !== 1) fail("queued row was delivered " + delivered.length + " times");
  if (delivered[0].deliveries !== 1) fail("queued row delivery count is not 1");
  if (delivered[0].requestId === undefined || delivered[0].requestId === null) fail("queued delivery has no request id");
  return delivered[0];
}

export function assertSteerHiddenUnlessAdvertised(observation) {
  if (!observation || typeof observation.advertised !== "boolean") fail("steer observation has no advertised flag");
  if (observation.advertised && observation.visible !== true) fail("steer was advertised but hidden");
  if (!observation.advertised && observation.visible === true) fail("steer was visible without an advertisement");
  return observation;
}

export function assertDictationHiddenWhenProbeFails(observation) {
  if (!observation || observation.probeOk !== false) fail("dictation hide applies only when the probe fails");
  if (observation.micVisible === true) fail("dictation mic was visible after a failed probe");
  if (observation.autoSent === true) fail("a failed dictation probe auto-sent");
  return observation;
}

export function assertRichWorkRows(fixture, rendered) {
  if (!fixture || !Array.isArray(fixture.rows) || fixture.rows.length === 0) fail("recorded native fixture has no rich rows");
  if (!Array.isArray(rendered)) fail("rendered work rows are not a list");
  for (const row of fixture.rows) {
    if ((typeof row.callId !== "string" || row.callId.length === 0) && (typeof row.taskId !== "string" || row.taskId.length === 0)) {
      fail("rich row has no provider call or task id");
    }
    if (row.status !== undefined && !WORK_STATUSES.includes(row.status)) fail("rich row status is not frozen: " + row.status);
    const match = rendered.find((item) => (row.callId && item.callId === row.callId) || (row.taskId && item.taskId === row.taskId));
    if (!match) fail("rendered work rows omit recorded id " + (row.callId || row.taskId));
    if (row.usage === undefined && match.usage !== undefined && match.usage !== null) {
      fail("rendered usage that the fixture did not record for " + (row.callId || row.taskId));
    }
    if (row.status && match.status !== row.status) fail("rendered status " + match.status + " does not match recorded " + row.status);
  }
  return rendered;
}

function payloadBody(result) {
  if (result && result.json && typeof result.json === "object") return result.json;
  return result;
}

export function assertPayloadResult(result, kind) {
  if (!result || typeof result !== "object") fail("payload result is not an object");
  const body = payloadBody(result);
  if (PATH_LEAK.test(JSON.stringify(body))) fail("payload result leaked a host path");
  if (kind === "success") {
    if (result.json && result.status !== 200) fail("payload success was HTTP " + result.status);
    if (body.status !== "loaded") fail("payload success status was " + body.status);
    if (body.kind !== "output" && body.kind !== "diff") fail("payload success kind was " + body.kind);
    if (typeof body.byteLength !== "number" || body.byteLength < 0) fail("payload success has no byteLength");
    if (typeof body.digest !== "string" || body.digest.length === 0) fail("payload success has no digest");
    if (body.truncated !== false) fail("payload success was truncated");
    return body;
  }
  if (kind === "truncated") {
    if (body.status !== "truncated" || body.truncated !== true) fail("truncated payload was not marked truncated");
    if (result.json && result.status !== 200) fail("truncated payload was HTTP " + result.status);
    return body;
  }
  if (kind === "expired") {
    const code = (body.error && body.error.code) || body.code;
    if (code !== "TARGET_EXPIRED") fail("expired payload was " + code);
    return body;
  }
  fail("unknown payload kind " + kind);
}

export function assertPayloadRefusal(result) {
  if (!result || typeof result.status !== "number") fail("payload refusal has no status");
  const code = errorCode(result.json);
  if (![400, 404, 410].includes(result.status)) fail("payload refusal was HTTP " + result.status);
  if (!["INVALID_REQUEST", "NOT_FOUND", "TARGET_EXPIRED"].includes(code)) fail("payload refusal code was " + code);
  if (PATH_LEAK.test(JSON.stringify(result.json || {}))) fail("payload refusal leaked a host path");
  return code;
}

export function assertRestartUnverifiable(status) {
  if (status !== "unverifiable") fail("unfinished work after restart was " + status + ", not unverifiable");
  return status;
}

export function assertUnsupportedKeepsCurrentCards(before, after) {
  if (!before || before.supported !== false) fail("card preservation applies to an unsupported provider");
  if (!after) fail("card preservation has no after snapshot");
  if (JSON.stringify(before.cards) !== JSON.stringify(after.cards)) fail("unsupported provider replaced current cards");
  if (after.syntheticRich === true) fail("unsupported provider synthesized rich state");
  return after;
}

export function assertContinuationStartup(body) {
  const startup = body && body.startup;
  if (!startup || startup.kind !== "agentContinue") fail("startup.kind is not agentContinue");
  if (startup.mode !== "fork" && startup.mode !== "resume") fail("startup.mode is not fork or resume");
  if (typeof startup.agentType !== "string" || startup.agentType.length === 0) fail("startup has no agentType");
  const provider = startup.providerSession;
  if (!provider || typeof provider.id !== "string" || provider.id.length === 0 || typeof provider.key !== "string") {
    fail("startup providerSession is not {key, id}");
  }
  if (provider.transcriptPath || startup.argv || startup.args || startup.command) {
    fail("startup carries client argv or a transcript path");
  }
  return startup;
}

export function assertResumeOnlyExited(source) {
  if (!source || source.exited !== true || source.running !== false) fail("resume-only source is not exited");
  if (source.mode !== "resume") fail("resume-only source was not resumed");
  if (source.forkOffered === true) fail("resume-only provider offered fork");
  if (FORK_NATIVE_PROVIDERS.includes(source.agentType) && source.forkHidden !== true) {
    fail("a fork-native provider was treated as resume-only");
  }
  return source;
}

/**
 * Providers with a resume builder. Port of RESUME_BUILDERS
 * (ui/src/lib/agentResume.ts:29-49). Fork stays FORK_NATIVE_PROVIDERS
 * (agentResume.ts:216, verifiedContinuationModes:223-228).
 */
export const RESUME_CAPABLE_PROVIDERS = Object.freeze([
  "claude", "codex", "antigravity", "opencode", "pi", "prime-agent",
  "mimo-code", "droid", "grok", "devin", "omp", "omo", "kimi", "gjc",
  "copilot", "cursor", "cursor-agent", "cline", "crush",
]);

/**
 * Modes continuationReason allows for an exited, non-working, native, capable
 * source with no other claim (agentResumeAffordance.ts:340-360). Resume needs
 * sourceIsExited. Fork needs only a non-working source, so a fork-native
 * provider still offers fork. assertResumeOnlyExited is the non-fork-native case.
 */
export function exitedContinuationModes(agentType) {
  const agent = String(agentType || "").trim().toLowerCase();
  const modes = [];
  if (FORK_NATIVE_PROVIDERS.includes(agent)) modes.push("fork");
  if (RESUME_CAPABLE_PROVIDERS.includes(agent)) modes.push("resume");
  return modes;
}

/** continue-in-new-pane-* test ids, in DOM order. */
export function continuationStripModes(snapshot) {
  const parsed = snapshot && Array.isArray(snapshot.buttons)
    ? { buttons: snapshot.buttons }
    : parseContinuationStripDom(snapshot);
  const modes = [];
  for (const testId of parsed.buttons || []) {
    const mode = String(testId || "").replace(/^continue-in-new-pane-/, "");
    if (mode === "fork" || mode === "resume") modes.push(mode);
  }
  return modes;
}

/**
 * Strip for an exited source. Resume must be offered. Every offered mode must
 * be one continuationReason allows. A non-fork-native provider is resume-only
 * (assertResumeOnlyExited). A fork-native provider may still offer fork.
 */
export function assertExitedResumeStrip(source) {
  if (!source || source.exited !== true || source.running !== false) fail("resume source is not exited");
  const allowed = exitedContinuationModes(source.agentType);
  if (!allowed.includes("resume")) fail("continuationReason does not allow resume for " + source.agentType);
  const offered = Array.isArray(source.offered) ? source.offered.slice() : [];
  if (!offered.includes("resume")) fail("strip did not offer resume");
  for (const mode of offered) {
    if (!allowed.includes(mode)) {
      fail("strip offered " + mode + " which continuationReason hides for an exited " + source.agentType);
    }
  }
  if (!FORK_NATIVE_PROVIDERS.includes(String(source.agentType || "").trim().toLowerCase())) {
    assertResumeOnlyExited({
      exited: true,
      running: false,
      mode: "resume",
      agentType: source.agentType,
      forkOffered: offered.includes("fork"),
      forkHidden: !offered.includes("fork"),
    });
  }
  return offered;
}

/** The page POST continuationInNewPane sends. Mode must be resume. */
export function assertResumeCreatePost(body) {
  const startup = assertContinuationStartup(body);
  if (startup.mode !== "resume") fail("startup.mode is not resume");
  return startup;
}

/**
 * Resume create keeps the source providerSession (session_service_machine_tests.rs:143
 * published_create_provider_session_for_startup). One new pane, host resume argv.
 */
export function assertResumeAcceptance(observation) {
  const added = assertContinueAcceptance({ ...observation, mode: "resume" });
  if (observation.posts !== 1) fail("resume create was posted " + observation.posts + " times");
  if (!added || added.providerSessionId !== observation.providerSessionId) {
    fail("resume create did not keep the source providerSession");
  }
  if (Array.isArray(observation.argv) && observation.argv.includes("--fork-session")) {
    fail("resume argv carried --fork-session");
  }
  return added;
}

/** T19 local desktop happy path. Isolated QA has no desktop GUI. */
export function localDesktopHappyLane() {
  return unmeasured(
    "local-desktop",
    "N/A: T19 local desktop happy path is not run. Isolated QA has no desktop GUI, and production-GUI mutation is forbidden (plan T19, section 9.2). Not a pass.",
  );
}

/**
 * Daemon UDS spawn the fixture uses for a desktop-owned pane
 * (herdr-reference-provision.mjs spawnOriginalPty). This is not POST
 * /api/v1/sessions: that journal row is machine_only, and set_active_selection
 * clears it (state.rs:1329-1339). shell null is the daemon default; the
 * provision wrapper only records PID identity and is not what makes the pane
 * desktop-owned. startup null is a shell, same as a fixture pane with no agent startup.
 */
export function desktopOwnedSpawnRequest(input) {
  const workspaceId = input && typeof input.workspaceId === "string" ? input.workspaceId : "";
  const cwd = input && typeof input.cwd === "string" ? input.cwd : "";
  const requestId = input && typeof input.requestId === "string" ? input.requestId : "";
  if (!workspaceId) fail("desktop spawn has no workspaceId");
  if (!cwd.startsWith("/")) fail("desktop spawn cwd is not absolute");
  if (!requestId) fail("desktop spawn has no requestId");
  const cols = Number(input.cols);
  const rows = Number(input.rows);
  if (!Number.isInteger(cols) || cols < 1 || cols > 1000) fail("desktop spawn cols are not a pane size");
  if (!Number.isInteger(rows) || rows < 1 || rows > 1000) fail("desktop spawn rows are not a pane size");
  const shell = typeof input.shell === "string" && input.shell.length > 0 ? input.shell : null;
  return {
    type: "spawn",
    clientRequestId: requestId,
    workspaceId,
    worktree: input.worktree || null,
    cwd,
    cols,
    rows,
    shell,
    startup: null,
  };
}

/** sessionId and epoch from spawnOk. Anything else is not a created desktop pane. */
export function desktopSpawnIdentity(response) {
  if (!response || response.type !== "spawnOk") return { sessionId: null, epoch: null };
  const sessionId = typeof response.sessionId === "string" && response.sessionId.length > 0 ? response.sessionId : null;
  const epoch = response.epoch == null ? null : String(response.epoch);
  return {
    sessionId,
    epoch: epoch && /^(?:0|[1-9][0-9]*)$/.test(epoch) ? epoch : null,
  };
}

/** Desktop close. DaemonRequest::Close. Idempotent once the PTY is already gone. */
export function desktopPaneCloseRequest(sessionId) {
  if (typeof sessionId !== "string" || sessionId.length === 0) fail("desktop close has no sessionId");
  return { type: "close", sessionId };
}

/**
 * A published desktop pane cannot offer resume until an exit publication exists.
 * Null when the sessions row is retained (running false, providerSession present)
 * or the published desktop tab carries running false and providerSession.
 * Null when this was not a published desktop spawn.
 * The citation is the product gap against plan T19, not a harness miss.
 */
export function exitedDesktopResumeGap(facts) {
  if (!facts || facts.ownership !== "desktop" || facts.published !== true) return null;
  if (facts.tabRunning === false && typeof facts.tabProviderSessionId === "string" && facts.tabProviderSessionId.length > 0) {
    return null;
  }
  const providerSessionId = typeof facts.providerSessionId === "string" && facts.providerSessionId.length > 0
    ? facts.providerSessionId
    : null;
  if (facts.rowFound === true && facts.running === false && providerSessionId) return null;
  const running = facts.running == null ? "absent" : String(facts.running);
  return "PRODUCT: remote resume-on-exited is unreachable, so T18 and T19 are not met. "
    + "A naturally exited local pane leaves the registry (pty.rs:736-738 lifecycle watcher, pty.rs:822-824 finalize_natural_exit). "
    + "Resume needs the desktop tab publication App.tsx sends after exit (running false and providerSession on remoteSetActiveSelection) to survive get_workspace_state. "
    + "This run's published tab did not carry both, so the page cannot offer resume. "
    + "observed rowFound=" + (facts.rowFound === true)
    + " running=" + running
    + " providerSession=" + (providerSessionId || "absent")
    + " tabRunning=" + (facts.tabRunning == null ? "absent" : String(facts.tabRunning))
    + " tabProviderSession=" + (facts.tabProviderSessionId || "absent")
    + ".";
}

/** Workspace-state sessions[].running as published. Absent is not rewritten to true. */
export function publishedSessionRunning(json, sessionId) {
  const sessions = json && Array.isArray(json.sessions) ? json.sessions : [];
  const row = sessions.find((entry) => entry && (entry.sessionId === sessionId || entry.id === sessionId)) || null;
  if (!row || typeof row.running !== "boolean") return { found: false, running: null };
  return { found: true, running: row.running };
}

/**
 * Which product path published running false. sessions-row is
 * project_desktop_gui_session while the PTY is still listed and not
 * Starting|Running. A natural exit drops that row; the desktop tab is the
 * other path. A missing row is not rewritten into a tab running flag.
 */
export function exitPublication(facts) {
  const providerSessionId = facts && typeof facts.providerSessionId === "string" && facts.providerSessionId.length > 0
    ? facts.providerSessionId
    : null;
  const input = {
    ownership: facts && facts.ownership,
    published: facts && facts.published === true,
    rowFound: facts && facts.rowFound === true,
    running: facts ? facts.running : null,
    providerSessionId,
    tabRunning: facts ? facts.tabRunning : null,
    tabProviderSessionId: facts && typeof facts.tabProviderSessionId === "string" ? facts.tabProviderSessionId : null,
  };
  if (input.tabRunning === false && input.tabProviderSessionId) {
    return {
      path: "desktop-tab",
      running: false,
      workspaceRunning: facts && facts.workspaceFound === true ? facts.workspaceRunning === true : null,
      citation: "App.tsx exited tab publication",
      productGap: null,
    };
  }
  if (input.rowFound && input.running === false && providerSessionId) {
    return {
      path: "sessions-row",
      running: false,
      workspaceRunning: facts && facts.workspaceFound === true ? facts.workspaceRunning === true : null,
      citation: "session_service.rs:1164-1167",
      productGap: null,
    };
  }
  return {
    path: "none",
    running: input.rowFound ? input.running === true : null,
    workspaceRunning: facts && facts.workspaceFound === true ? facts.workspaceRunning === true : null,
    citation: "pty.rs:736-738 pty.rs:822-824 machine_owner.rs:239-247 server.rs:1207-1214 RemoteSessionList.tsx:158 RemoteSessionList.tsx:313-316",
    productGap: exitedDesktopResumeGap(input),
  };
}

/** Machine shell the harness creates. cwdRelative null is the workspace root. */
export function throwawayShellCreate(workspaceId, requestId) {
  if (typeof workspaceId !== "string" || workspaceId.length === 0) fail("throwaway shell has no workspaceId");
  if (typeof requestId !== "string" || requestId.length === 0) fail("throwaway shell has no requestId");
  return {
    requestId,
    workspaceId,
    worktree: null,
    cols: 80,
    rows: 24,
    inheritFromSessionId: null,
    cwdRelative: null,
    startup: { kind: "shell" },
  };
}

/** Session id and canonical daemonEpoch from a machine create body. */
export function machineCreateIdentity(json) {
  const sessionId = sessionIdFromCreateBody(json);
  const target = json && json.target && typeof json.target === "object" ? json.target : null;
  const epoch = target && (typeof target.daemonEpoch === "string" ? target.daemonEpoch : null);
  return {
    sessionId,
    daemonEpoch: epoch && /^(?:0|[1-9][0-9]*)$/.test(epoch) ? epoch : null,
  };
}

/**
 * Plantable provider id that is not the fixture source's id.
 * A machine throwaway is admitted only when the store file for this id records the
 * pane cwd (session_metadata_provider.rs admit_agent_report -> verify_machine_resume_cwd).
 * The default is a canonical UUID: claude's own session ids are UUIDs, and a
 * `qa16resume…` token is not the id shape the working fixture plant uses.
 */
export function distinctResumeProviderId(existing, minted) {
  const taken = new Set(Array.isArray(existing) ? existing.filter(Boolean) : []);
  const id = typeof minted === "string" && minted.length > 0 ? minted : randomUUID();
  if (!/^[A-Za-z0-9._-]{8,80}$/.test(id)) fail("resume provider id is not plantable");
  if (taken.has(id)) fail("resume provider id collides with the fixture source");
  return id;
}

/**
 * Claude Code's project directory under ~/.claude/projects.
 * Measured: /private/tmp/parity-claude-59r7 -> -private-tmp-parity-claude-59r7
 * (src-tauri/src/cli/agent_hook.rs CLAUDE_TRANSCRIPT). Separators become '-'.
 */
export function claudeProjectSlug(cwd) {
  if (typeof cwd !== "string" || !cwd.startsWith("/")) return null;
  const slug = cwd.replace(/[\\/]+/g, "-");
  if (slug.length < 2 || slug === "-" || slug.includes("..")) return null;
  return slug;
}

/** Absolute cwd printed by the throwaway shell, or null when the probe did not. */
export function pwdFromProbe(text) {
  if (typeof text !== "string" || !text.includes("QA16_PWD_SENT")) return null;
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line.length > 0);
  const marker = lines.lastIndexOf("QA16_PWD_SENT");
  if (marker < 0) return null;
  for (let index = marker - 1; index >= 0; index -= 1) {
    const line = lines[index];
    if (line.startsWith("/") && !line.includes(" ") && !line.includes("QA16_PWD")) return line;
  }
  return null;
}

/**
 * QA-13 outcome-unknown after the QA-16 unknown-create setup still has no
 * model select. The typed proof stays; the observed history and page are why.
 */
export function composerControlMissingEvidence(sessionId, observed) {
  const extra = observed && typeof observed === "object" ? observed : {};
  const choices = extra.modelChoices == null ? "absent" : String(extra.modelChoices);
  return composerControlOutcomeUnknownProof(sessionId)
    + " Same setup as QA-16 unknown-create (planted transcript, workspace-root retarget, SessionStart idle hook, desktop agentType tab, Connection: close injector) still did not render composer-model-control."
    + " historyStatus=" + (extra.historyStatus == null ? "unread" : String(extra.historyStatus))
    + " availability=" + (extra.availability == null ? "unread" : String(extra.availability))
    + " composerModels=" + choices
    + " providerSession=" + (extra.providerSessionId || "absent")
    + " pageUrl=" + (extra.pageUrl || "unread")
    + ".";
}

export function assertForkNativeIdle(source) {
  if (!source || !FORK_NATIVE_PROVIDERS.includes(source.agentType)) fail("fork-native source is not claude or codex");
  if (source.working === true || source.running !== true) fail("fork source is not an idle live pane");
  if (source.mode !== "fork") fail("idle native source was not forked");
  return source;
}

export function assertWorkingSourceBlocked(result) {
  if (!result || result.created === true) fail("working source created a continuation");
  if (result.code !== "AGENT_CONTINUE_CONFLICT") {
    fail("working or live source was not refused with AGENT_CONTINUE_CONFLICT: " + result.code);
  }
  if (result.sourceStopped === true) fail("working source was stopped to make continuation possible");
  return result;
}

export function assertDuplicateClaimConflict(result) {
  if (!result || result.created === true) fail("duplicate claim created a session");
  if (result.code !== "AGENT_CONTINUE_CONFLICT") {
    fail("duplicate claim was not AGENT_CONTINUE_CONFLICT: " + result.code);
  }
  return result;
}

export function continuationSpawnedPane(before, after) {
  const ids = new Set(((before && before.sessions) || []).map((row) => row && row.id));
  return ((after && after.sessions) || []).some((row) => row && row.id && !ids.has(row.id));
}

/**
 * Machine create reaches `admit_continue` (continue_startup.rs:70-80) only after
 * the transcript cwd proof (session_service.rs:2785, resume_cwd.rs:109-147). A
 * running source is a provider-session claim or an agent-state report, not a bash
 * pane with the same registry label. Binding that claim is
 * `admit_agent_report`, and session_metadata_provider.rs:10 has no HTTP surface,
 * so a live credentialed provider report is required. That state is unmeasured,
 * never a pass. INVALID_REQUEST is a harness-shaped body, not a product gap.
 */
export function classifyContinuationProbe(code, spawned) {
  if (spawned) return { status: "failed", reason: "continuation create spawned a pane" };
  if (code === "AGENT_CONTINUE_CONFLICT") return { status: "pass", code };
  if (code === "INVALID_REQUEST") {
    return { status: "failed", reason: "harness continuation body was refused INVALID_REQUEST" };
  }
  if (code === "AGENT_CONTINUE_UNSUPPORTED") {
    return {
      status: "unmeasured",
      reason: "continue_supported refused this provider before admit_continue (continue_startup.rs:26-54); the live pane was not stopped",
    };
  }
  if (code === "AGENT_TRANSCRIPT_UNVERIFIED" || code === "AGENT_CWD_MISMATCH" || code === "AGENT_RESUME_UNSUPPORTED") {
    return {
      status: "unmeasured",
      reason: "observed " + code + "; admit_continue (continue_startup.rs:70-80) only refuses a running provider-session claim, and session_metadata_provider.rs:10 has no HTTP surface to bind the bash pane. A live credentialed provider report is required. Not a pass.",
    };
  }
  return { status: "failed", reason: "continuation create was " + (code || "unregistered") };
}

export function workspaceRootOf(session) {
  const registration = session && session.workspaceRegistration;
  const root = (registration && registration.repoRoot) || (session && session.repoRoot) || null;
  return typeof root === "string" && root.startsWith("/") ? root : null;
}

/**
 * Machine createSession success is the journal's completed status, 201
 * (machine_operation_journal.rs commit_spawn). Replay returns that same status.
 * 200 is not a create success.
 */
export function continuationCreateAccepted(status) {
  return status === 201;
}

/**
 * The claim hook publishes providerSessionId after the pre-claim inventory.
 * original-preserved must start from the post-claim snapshot, once that snapshot
 * still contains the source. A later change is a product rebind, not the hook.
 */
export function preservedBaseline(preClaim, postClaim, sourceId) {
  const sessions = postClaim && Array.isArray(postClaim.sessions) ? postClaim.sessions : null;
  if (sessions && sessions.some((row) => row && row.id === sourceId)) return postClaim;
  return preClaim;
}

/** The pane's own cwd when the provisioner recorded one, else the workspace root. */
export function plantCwdOf(session) {
  const cwd = session && typeof session.cwd === "string" && session.cwd.startsWith("/") ? session.cwd : null;
  return cwd || workspaceRootOf(session);
}

/**
 * Continuation create uses the workspace root unless cwdRelative names a child.
 * resume_cwd.rs matching_cwd compares the transcript cwd to that request cwd.
 */
export function continuationCwdRelative(session) {
  const root = workspaceRootOf(session);
  const cwd = session && typeof session.cwd === "string" ? session.cwd : null;
  if (!root || !cwd) return null;
  const prefix = root.endsWith("/") ? root : root + "/";
  if (!cwd.startsWith(prefix)) return null;
  const relative = cwd.slice(prefix.length);
  if (!relative || relative.split("/").some((part) => part.length === 0 || part === "." || part === "..")) return null;
  return relative;
}

/**
 * Cwd the unknown-create click requests. RemoteApp.tsx:5481-5490 sends
 * cwdRelative null and a worktree only when the page context has a slug.
 * This fixture's workspace state publishes worktreeSlug null, so
 * session_service.rs requests the registered workspace root
 * (resolve_terminal_target with no identity). Happy fork is different: the
 * harness posts continuationCwdRelative, which is the isolated pane directory.
 */
export function unknownCreateTranscriptCwd(session) {
  return workspaceRootOf(session);
}

/** resume_cwd.rs matching_cwd compares canonicalize(recorded) to canonicalize(requested). */
export function canonicalWorkspaceCwd(cwd) {
  if (typeof cwd !== "string" || !cwd.startsWith("/")) return null;
  try {
    const resolved = realpathSync(cwd);
    return resolved.startsWith("/") ? resolved : null;
  } catch {
    return null;
  }
}

/** Rewrite every cwd in a planted claude jsonl to the workspace root.
 *  history/mod.rs session_cwd refuses disagreeing cwds (InvalidIdentity), and
 *  resume_cwd.rs matching_cwd canonicalizes the recorded cwd against the workspace.
 */
export function retargetPlantedTranscriptCwd(body, cwd) {
  if (typeof cwd !== "string" || !cwd.startsWith("/")) fail("planted cwd is not an absolute workspace path");
  if (typeof body !== "string" || !body.includes('"cwd":')) fail("planted transcript has no cwd field");
  const next = body.replace(/"cwd":"(?:\\.|[^"\\])*"/g, '"cwd":' + JSON.stringify(cwd));
  if (next.includes('"/Users/dev/project"') || next.includes('"/work/qa15"')) fail("planted cwd was not fully retargeted");
  if (!next.includes(JSON.stringify(cwd))) fail("planted cwd was not written");
  return next;
}

export function shellQuote(value) {
  return "'" + String(value).replace(/'/g, "'\\''") + "'";
}

/**
 * One line a pane shell can run. `ferryx agent-hook` (src-tauri/src/main.rs:13)
 * reads FERRYX_SESSION_ID and FERRYX_AGENT_STATE_SOCKET (pty.rs:531-541) and
 * writes an agentState line (agent_hook_core.rs prepare). claude PreToolUse
 * reports working (agent_hook_core.rs:142). The marker is assembled by printf
 * so the typed echo is not the marker itself.
 */
function paneAgentHookLine(binary, providerSessionId, event, marker) {
  if (typeof binary !== "string" || binary.length === 0) return null;
  if (typeof providerSessionId !== "string" || providerSessionId.length === 0) return null;
  if (typeof event !== "string" || event.length === 0) return null;
  const payload = JSON.stringify({ session_id: providerSessionId });
  return "printf %s " + shellQuote(payload) + " | " + shellQuote(binary) + " agent-hook claude " + event + "; printf '\\n" + marker + "_%s\\n' SENT";
}

export function paneClaimHookLine(binary, providerSessionId) {
  return paneAgentHookLine(binary, providerSessionId, "PreToolUse", "QA16_CLAIM");
}

/**
 * Claude SessionStart reports idle, not working (agent_hook_core.rs state_for:
 * SessionStart | Stop | SessionEnd => Idle) and prepare() publishes providerSession
 * from session_id. The marker is not QA16_CLAIM_SENT, so the later PreToolUse wait
 * is not satisfied by this print.
 */
export function paneIdleHookLine(binary, providerSessionId) {
  return paneAgentHookLine(binary, providerSessionId, "SessionStart", "QA16_IDLE");
}

/** SessionEnd is idle, not working (agent_hook_core.rs state_for). It does not by itself exit the pane. */
export function paneEndHookLine(binary, providerSessionId) {
  return paneAgentHookLine(binary, providerSessionId, "SessionEnd", "QA16_END");
}

/** A hook-bound source must be AGENT_CONTINUE_CONFLICT and must not spawn. */
export function claimedContinueLane(code, spawned) {
  if (spawned) return { status: "failed", reason: "continuation create spawned a pane" };
  if (code === "AGENT_CONTINUE_CONFLICT") return { status: "pass", code };
  return {
    status: "failed",
    reason: "hook-bound source was not refused with AGENT_CONTINUE_CONFLICT: " + (code || "none"),
  };
}

export function assertUnknownCreateNotRetried(creates) {
  if (!Array.isArray(creates) || creates.length !== 1) {
    fail("unknown create was issued " + (creates ? creates.length : 0) + " times");
  }
  const outcome = creates[0].code || creates[0].outcome;
  if (outcome !== "OPERATION_OUTCOME_UNKNOWN" && outcome !== "outcome-unknown") {
    fail("the single create was not outcome-unknown");
  }
  return creates[0];
}

export function assertOriginalSessionPreserved(before, after) {
  if (!before || !after || !Array.isArray(before.sessions) || !Array.isArray(after.sessions)) {
    fail("session inventory is not a before/after pair");
  }
  const source = before.sessions.find((row) => row.id === before.sourceId);
  const later = after.sessions.find((row) => row.id === before.sourceId);
  if (!source) fail("inventory before has no source session");
  if (!later) fail("source session disappeared");
  for (const field of ["id", "running", "pid", "cwd", "workspaceId", "providerSessionId", "cols", "rows"]) {
    if (JSON.stringify(source[field]) !== JSON.stringify(later[field])) {
      fail("source " + field + " changed");
    }
  }
  const beforeIds = new Set(before.sessions.map((row) => row.id));
  for (const id of beforeIds) {
    if (!after.sessions.some((row) => row.id === id)) fail("inventory lost session " + id);
  }
  return later;
}

/**
 * A natural exit removes the throwaway from the registry. Resume must not
 * find that id in either inventory: it stays exited, and the create must
 * not resurrect or re-register it.
 */
export function assertExitedSourceStaysAbsent(before, after, exitedId) {
  if (typeof exitedId !== "string" || exitedId.length === 0) fail("exited source has no id");
  if (!before || !after || !Array.isArray(before.sessions) || !Array.isArray(after.sessions)) {
    fail("session inventory is not a before/after pair");
  }
  if (before.sessions.some((row) => row && row.id === exitedId)) {
    fail("exited source is still in the before inventory");
  }
  if (after.sessions.some((row) => row && row.id === exitedId)) {
    fail("exited source was re-registered");
  }
  return exitedId;
}

/**
 * Unmeasured never becomes pass. A failed lane wins over an unmeasured one so a missing
 * credential cannot hide a product failure in the same branch.
 */
export function classifyLane(lanes) {
  if (!Array.isArray(lanes) || lanes.length === 0) return "failed";
  if (lanes.some((lane) => lane.status === "failed")) return "failed";
  if (lanes.some((lane) => lane.status === "blocked")) return "blocked";
  if (lanes.some((lane) => lane.status === "unmeasured")) return "unmeasured";
  if (lanes.every((lane) => lane.status === "pass")) return "pass";
  return "failed";
}

function loadRecorded(name) {
  const path = join(FIXTURE_DIR, name);
  if (!existsSync(path)) fail("recorded fixture missing: " + path);
  return JSON.parse(readFileSync(path, "utf8"));
}

function registryOf(session) {
  return String(session.registryId || session.provider || "").trim().toLowerCase();
}

export function providerCredentialLane(ctx, session) {
  const registry = registryOf(session);
  const supported = DISCOVERY_PROVIDERS.includes(registry);
  const declared = session.providerCredentialFile
    || (ctx.fixtureRaw.providerCredentials && ctx.fixtureRaw.providerCredentials[registry])
    || (ctx.fixtureRaw.providerCredentials && ctx.fixtureRaw.providerCredentials[session.backendSessionId]);
  if (!supported) {
    return { registry, supported: false, available: false, reason: "unsupported-provider" };
  }
  if (typeof declared !== "string" || declared.length === 0 || !existsSync(declared)) {
    return {
      registry,
      supported: true,
      available: false,
      reason: "provider-credential-absent: no credential file for " + registry,
    };
  }
  return { registry, supported: true, available: true, reason: null };
}

function deviceLane(ctx) {
  const devices = (ctx.fixture && ctx.fixture.devices) || ctx.fixtureRaw.devices || [];
  if (!Array.isArray(devices) || devices.length === 0) {
    return { available: false, reason: "no provisioned device in the fixture manifest" };
  }
  return { available: true, reason: null };
}

function redact(value, depth) {
  const level = depth || 0;
  if (level > 6) return "[depth-limit]";
  if (typeof value === "string") return value.length > 200 ? value.slice(0, 200) + "…(" + value.length + " chars)" : value;
  if (Array.isArray(value)) return value.slice(0, 20).map((entry) => redact(entry, level + 1));
  if (value && typeof value === "object") {
    const out = {};
    for (const [key, entry] of Object.entries(value)) {
      out[key] = /token|secret|authorization|credential|password|ticket|bearer/i.test(key) ? "[redacted]" : redact(entry, level + 1);
    }
    return out;
  }
  return value;
}

function openEvidence(ctx, scenarioId, caseName) {
  const nested = join(ctx.evidenceDir, scenarioId, caseName);
  const shots = join(nested, "screenshots");
  mkdirSync(shots, { recursive: true });
  const httpPath = join(nested, "http.jsonl");
  const eventsPath = join(nested, "events.jsonl");
  writeFileSync(httpPath, "");
  writeFileSync(eventsPath, "");
  const state = {
    schema: "herdr-reference-chat/scenario-result/1",
    scenarioId,
    caseName,
    status: "running",
    startedAt: new Date().toISOString(),
    lanes: [],
  };
  return {
    dir: nested,
    lane(entry) {
      state.lanes.push(entry);
      return entry;
    },
    http(entry) {
      appendFileSync(httpPath, JSON.stringify(redact(entry)) + "\n");
    },
    event(entry) {
      appendFileSync(eventsPath, JSON.stringify(redact(entry)) + "\n");
    },
    screenshot(name, bytes) {
      const path = join(shots, name);
      writeFileSync(path, bytes);
      return path;
    },
    finish(status, observed) {
      state.status = status;
      state.finishedAt = new Date().toISOString();
      state.observed = redact(observed);
      writeFileSync(join(nested, "result.json"), JSON.stringify(state, null, 2));
      if (ctx.args.scenario === scenarioId && (ctx.args.caseName === caseName || ctx.args.caseName === "all")) {
        for (const file of ["result.json", "http.jsonl", "events.jsonl"]) {
          copyFileSync(join(nested, file), join(ctx.evidenceDir, file));
        }
        const rootShots = join(ctx.evidenceDir, "screenshots");
        mkdirSync(rootShots, { recursive: true });
        for (const name of readdirSync(shots)) {
          copyFileSync(join(shots, name), join(rootShots, scenarioId + "-" + caseName + "-" + name));
        }
      }
      ctx.record(scenarioId.toLowerCase() + "-" + caseName + ".json", state);
      return state;
    },
  };
}

function historyQuery(session, extra) {
  const params = new URLSearchParams();
  const fields = {
    ownerId: referenceAuthorityOwner(session),
    epoch: session.epoch,
    backendSessionId: session.backendSessionId,
    providerSessionId: session.providerSessionId,
    registryId: session.registryId,
    limit: extra && extra.limit,
    cursor: extra && extra.cursor,
    cursorStream: extra && extra.cursorStream,
  };
  for (const [key, value] of Object.entries(fields)) {
    if (value === undefined || value === null || value === "") continue;
    params.set(key, String(value));
  }
  if (extra && extra.after) params.set("after", JSON.stringify(extra.after));
  if (extra && extra.ref) params.set("ref", String(extra.ref));
  const query = params.toString();
  return query.length === 0 ? "" : "?" + query;
}

async function traced(evidence, host, options) {
  const started = Date.now();
  let result;
  try {
    result = await referenceRequest(host, options);
  } catch (error) {
    evidence.http({
      at: new Date().toISOString(),
      ms: Date.now() - started,
      method: options.method,
      suffix: options.suffix,
      query: options.query || "",
      error: String(error && error.message ? error.message : error),
    });
    throw error;
  }
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - started,
    method: options.method,
    suffix: options.suffix,
    query: options.query || "",
    status: result.status,
    envelope: envelopeOf(result.json),
    body: result.json,
  });
  return result;
}

async function readHistory(evidence, host, session, extra) {
  const query = historyQuery(session, extra || { limit: 50 });
  return traced(evidence, host, {
    method: "GET",
    sessionId: session.backendSessionId,
    suffix: "history",
    query,
  });
}

function capabilityOn(document, name) {
  return Boolean(document && Array.isArray(document.capabilities) && document.capabilities.includes(name));
}

/**
 * The list parseChatGatewayIdentity reads (RemoteApp.tsx:712-733). A missing or
 * non-canonical daemonEpoch is not an identity: the page then has no chatGateway
 * and will not post. terminalContinueV1 is machine-scope only
 * (server.rs gateway_capability_names); a mirror device token omits it.
 */
export function chatGatewayCapabilityAdvertised(document, name) {
  if (!document || typeof document !== "object") return false;
  const epoch = document.daemonEpoch;
  if (typeof epoch !== "string" || !/^(?:0|[1-9][0-9]*)$/.test(epoch)) return false;
  return Array.isArray(document.capabilities) && document.capabilities.includes(name);
}

/** The page's own GET /api/v1/capabilities, as the proxy relayed it. */
export function capabilityAdvertisedFromEntries(entries, name) {
  const hits = (Array.isArray(entries) ? entries : []).filter((entry) => {
    if (!entry || entry.method !== "GET" || entry.forwarded !== true) return false;
    return String(entry.path || "").split("?")[0] === "/api/v1/capabilities";
  });
  if (hits.length === 0 || hits[hits.length - 1].upstreamBody === undefined) {
    return { known: false, advertised: false };
  }
  let document = null;
  try {
    document = JSON.parse(hits[hits.length - 1].upstreamBody);
  } catch {
    document = null;
  }
  return { known: true, advertised: chatGatewayCapabilityAdvertised(document, name) };
}

async function readCapabilities(evidence, host, options) {
  const started = Date.now();
  const bearer = options && typeof options.bearer === "string" ? options.bearer : null;
  if (bearer) {
    const base = String((options && options.baseUrl) || host.url).replace(/\/+$/, "");
    const response = await fetch(base + "/api/v1/capabilities", {
      headers: { accept: "application/json", authorization: "Bearer " + bearer },
    });
    const text = await response.text();
    let json = null;
    try {
      json = text.length > 0 ? JSON.parse(text) : null;
    } catch {
      json = null;
    }
    const result = { status: response.status, json, text };
    evidence.http({
      at: new Date().toISOString(),
      ms: Date.now() - started,
      method: "GET",
      suffix: "/api/v1/capabilities",
      status: result.status,
      auth: "page-bearer",
      body: result.json && {
        capabilities: result.json.capabilities || [],
        daemonEpoch: result.json.daemonEpoch || null,
      },
    });
    return result;
  }
  const result = await readGatewayCapabilities(host);
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - started,
    method: "GET",
    suffix: "/api/v1/capabilities",
    status: result.status,
    body: result.json && {
      capabilities: result.json.capabilities || [],
      daemonEpoch: result.json.daemonEpoch || null,
    },
  });
  return result;
}

async function subscribeEvents(ctx, host, evidence) {
  if (typeof WebSocket !== "function") {
    return { ok: false, reason: "this runtime has no WebSocket client" };
  }
  const token = readCredentialToken(host);
  const base = String(host.url).replace(/\/+$/, "");
  const started = Date.now();
  const ticketResponse = await fetch(base + "/api/v1/socket-ticket", {
    method: "POST",
    headers: {
      authorization: "Bearer " + token,
      "content-type": "application/json",
      accept: "application/json",
    },
    body: JSON.stringify({ target: "/api/v1/events" }),
  });
  const ticketText = await ticketResponse.text();
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - started,
    method: "POST",
    suffix: "/api/v1/socket-ticket",
    status: ticketResponse.status,
    body: ticketResponse.status === 200 ? { ticket: "[redacted]" } : ticketText.slice(0, 180),
  });
  if (ticketResponse.status !== 200) return { ok: false, reason: "socket-ticket HTTP " + ticketResponse.status };
  let ticket;
  try {
    ticket = JSON.parse(ticketText).ticket;
  } catch {
    return { ok: false, reason: "socket-ticket body was not JSON" };
  }
  if (typeof ticket !== "string" || ticket.length === 0) return { ok: false, reason: "socket-ticket minted no ticket" };
  const wsUrl = base.replace(/^http/, "ws") + "/api/v1/events?ticket=" + encodeURIComponent(ticket);
  const frames = [];
  const ws = new WebSocket(wsUrl);
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("events socket did not open")), ctx.args.timeoutMs);
      ws.addEventListener("open", () => {
        clearTimeout(timer);
        resolve();
      });
      ws.addEventListener("error", () => {
        clearTimeout(timer);
        reject(new Error("events socket failed"));
      });
    });
  } catch (error) {
    return { ok: false, reason: String(error && error.message ? error.message : error) };
  }
  ws.addEventListener("message", (event) => {
    let parsed = null;
    try {
      parsed = JSON.parse(String(event.data));
    } catch {
      parsed = { type: "unparsed" };
    }
    frames.push(parsed);
    evidence.event({ at: new Date().toISOString(), frame: parsed });
  });
  evidence.event({ at: new Date().toISOString(), frame: { type: "qa-subscribed", target: "/api/v1/events" } });
  return {
    ok: true,
    frames,
    close() {
      try {
        ws.close();
      } catch {
        /* The receipt already records whether a frame arrived. */
      }
    },
    nextMatching(predicate, timeoutMs) {
      const existing = frames.find(predicate);
      if (existing) return Promise.resolve(existing);
      return new Promise((resolve) => {
        const timer = setTimeout(() => {
          ws.removeEventListener("message", onMessage);
          resolve(null);
        }, timeoutMs);
        function onMessage() {
          const found = frames.find(predicate);
          if (!found) return;
          clearTimeout(timer);
          ws.removeEventListener("message", onMessage);
          resolve(found);
        }
        ws.addEventListener("message", onMessage);
      });
    },
  };
}

function anchorFromPage(page) {
  const turns = page && Array.isArray(page.turns) ? page.turns : [];
  const withId = turns.filter((turn) => typeof turn.id === "string" && turn.id.length > 0);
  if (withId.length === 0) return null;
  const open = withId.find((turn) => turn.tailOpen === true || (turn.parts || []).some((part) => part && part.status === "running"));
  const chosen = open || withId[withId.length - 1];
  const previous = withId[withId.indexOf(chosen) - 1];
  return {
    streamId: referenceHistoryStreamId(page),
    generation: page.generation,
    anchorTurnId: chosen.id,
    previousTurnFingerprint: previous && previous.fingerprint ? previous.fingerprint : null,
  };
}

/** Stream id from the page cursor, else from `{streamId}:inc-N` or `{streamId}:{16 hex}`.
 *  Port of ui/src/remote/chat/referenceHistorySync.ts:131-138 (`referenceHistoryStreamId`).
 */
export function referenceHistoryStreamId(page) {
  if (!page || typeof page !== "object") return null;
  const cursorId = page.cursor && page.cursor.streamId;
  if (typeof cursorId === "string" && cursorId.length > 0) return cursorId;
  const inc = /^(.*):inc-[0-9]+$/.exec(page.generation);
  if (inc && inc[1].length > 0) return inc[1];
  const genesis = /^(.*):[0-9a-f]{16}$/.exec(page.generation);
  if (genesis && genesis[1].length > 0) return genesis[1];
  return null;
}

async function captureWidths(ctx, host, session, evidence, openChatPage) {
  const specs = [
    { width: 390, height: 844, coarse: true, name: "390-coarse.png" },
    { width: 1280, height: 800, coarse: false, name: "1280-fine.png" },
  ];
  const observed = [];
  for (const spec of specs) {
    const opened = await openChatPage(ctx, host, session, {
      viewport: { width: spec.width, height: spec.height },
      coarse: spec.coarse,
    });
    try {
      const pointer = await opened.page.evaluate(() => (
        window.matchMedia("(pointer: coarse)").matches ? "coarse" : "fine"
      ));
      const chatVisible = await opened.page.locator('[data-testid="chat-message-stream"]').count().then((count) => count > 0);
      const screenshot = evidence.screenshot(spec.name, await opened.page.screenshot());
      observed.push({
        viewport: { width: spec.width, height: spec.height },
        coarse: spec.coarse,
        pointer,
        chatVisible,
        screenshot,
      });
    } finally {
      await opened.close();
    }
  }
  return observed;
}

async function countTestId(page, id) {
  return page.locator('[data-testid="' + id + '"]').count();
}

async function sessionInventory(evidence, host, bearer) {
  const token = bearer || readCredentialToken(host);
  const base = String(host.url).replace(/\/+$/, "");
  const started = Date.now();
  const response = await fetch(base + "/api/v1/sessions", {
    headers: { accept: "application/json", authorization: "Bearer " + token },
  });
  const text = await response.text();
  let json = null;
  try {
    json = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - started,
    method: "GET",
    suffix: "/api/v1/sessions",
    status: response.status,
    body: json && { completeness: json.completeness, count: Array.isArray(json.sessions) ? json.sessions.length : null },
  });
  const sessions = json && Array.isArray(json.sessions) ? json.sessions.map(inventoryRow) : [];
  return { status: response.status, completeness: json && json.completeness, sessions };
}

function inventoryRow(session) {
  const provider = session.providerSession || null;
  return {
    id: session.target && session.target.sessionId,
    running: session.running === true,
    pid: numericPid(session.pid),
    cwd: session.cwd || null,
    workspaceId: session.workspaceId || null,
    worktree: session.worktree || null,
    providerSessionId: provider && provider.id ? provider.id : null,
    cols: session.cols,
    rows: session.rows,
  };
}

async function resourceSessionPid(host, bearer, sessionId) {
  const base = String(host.url).replace(/\/+$/, "");
  const response = await fetch(base + "/api/v1/resources", {
    headers: { accept: "application/json", authorization: "Bearer " + bearer },
  });
  const text = await response.text();
  let json = null;
  try {
    json = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  const sessions = json && Array.isArray(json.sessions) ? json.sessions : [];
  const row = sessions.find((entry) => entry && entry.sessionId === sessionId);
  return { status: response.status, pid: row ? numericPid(row.pid) : null, count: sessions.length };
}

/**
 * machine_detail (session_service.rs:1350-1360) returns the journal row stored with
 * pid: None (session_service.rs:3032), so GET /api/v1/sessions has no pid while the
 * PTY is live. GET /api/v1/resources reads pty.pid() (resources_api.rs:44-53).
 * The pane process is /bin/sh; the agent is its child (shell.rs:688-720).
 */
async function observeContinueArgv(evidence, host, bearer, sessionId, inventoryPid, expected) {
  const deadline = Date.now() + 2000;
  let resource = { status: null, pid: null, count: null };
  let argvs = [];
  let pid = numericPid(inventoryPid);
  while (Date.now() <= deadline) {
    resource = await resourceSessionPid(host, bearer, sessionId);
    pid = resource.pid || numericPid(inventoryPid);
    if (pid) {
      argvs = readProcessArgvTree(pid);
      const argv = matchingContinueArgv(argvs, expected);
      if (argv) {
        evidence.http({
          at: new Date().toISOString(),
          method: "GET",
          suffix: "/api/v1/resources",
          status: resource.status,
          body: { sessionId, pid, source: resource.pid ? "resources" : "inventory", argv0: argv[0] },
        });
        return { argv, pid, source: resource.pid ? "resources" : "inventory", resourceStatus: resource.status, argvs };
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 40));
  }
  evidence.http({
    at: new Date().toISOString(),
    method: "GET",
    suffix: "/api/v1/resources",
    status: resource.status,
    body: { sessionId, pid, resourcePid: resource.pid, inventoryPid: numericPid(inventoryPid), argvCount: argvs.length },
  });
  return { argv: null, pid, source: resource.pid ? "resources" : (numericPid(inventoryPid) ? "inventory" : "none"), resourceStatus: resource.status, argvs };
}

function continuationBody(session, mode, requestId) {
  const body = {
    requestId,
    workspaceId: session.workspaceId,
    worktree: session.worktree || null,
    cols: session.cols || 80,
    rows: session.rows || 24,
    inheritFromSessionId: null,
    cwdRelative: continuationCwdRelative(session),
    startup: {
      kind: "agentContinue",
      agentType: registryOf(session) || "claude",
      providerSession: {
        key: "session_id",
        id: session.providerSessionId || "missing-provider-session",
      },
      mode,
    },
  };
  assertContinuationStartup(body);
  return body;
}

async function postJson(evidence, host, path, body, bearer) {
  const token = bearer || readCredentialToken(host);
  const base = String(host.url).replace(/\/+$/, "");
  const started = Date.now();
  const response = await fetch(base + path, {
    method: "POST",
    headers: {
      accept: "application/json",
      authorization: "Bearer " + token,
      "content-type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  let json = null;
  try {
    json = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - started,
    method: "POST",
    suffix: path,
    status: response.status,
    envelope: envelopeOf(json),
    body: json,
  });
  return { status: response.status, json, text };
}

function admissionCode(result) {
  return errorCode(result.json);
}

function gapOrCode(kind, result) {
  return explainHostGap(kind, result) || admissionCode(result) || ("HTTP " + result.status);
}

/**
 * Continuation create is POST /api/v1/sessions, which admits only a Machine-scope
 * Control device. The shared reference-chat token is the Mirror phone
 * (remoteCreatePairingCode). The real continuation client pairs with
 * remoteCreateMachinePairingCode. The bearer stays in memory.
 */
async function machineScopeBearer(ctx) {
  const gateway = (ctx.gateways || []).find((entry) => entry && entry.controlTransport && entry.url);
  if (!gateway) {
    return {
      ok: false,
      reason: "continuation create requires a Machine-scope device; this run has no daemon control transport for remoteCreateMachinePairingCode",
    };
  }
  let control = null;
  try {
    control = await connectDaemonControl(gateway.controlTransport, {
      timeoutMs: ctx.args && ctx.args.timeoutMs ? ctx.args.timeoutMs : 15000,
    });
    const pinResponse = await control.call({ type: "remoteCreateMachinePairingCode" });
    if (!pinResponse || pinResponse.type !== "remotePairingCodeOk" || !/^\d{6}$/.test(String(pinResponse.code || ""))) {
      return { ok: false, reason: "remoteCreateMachinePairingCode did not mint a machine pairing code" };
    }
    const response = await fetch(String(gateway.url).replace(/\/+$/, "") + "/api/v1/pair/exchange", {
      method: "POST",
      headers: { accept: "application/json", "content-type": "application/json" },
      body: JSON.stringify({
        code: String(pinResponse.code),
        deviceName: "herdr-qa16-machine",
        installationId: "herdr-qa16-" + randomUUID(),
      }),
    });
    const json = await response.json().catch(() => null);
    const scope = json && json.device && json.device.accessScope;
    if (response.status !== 200 || !json || typeof json.token !== "string" || json.token.length === 0 || scope !== "machine") {
      return { ok: false, reason: "machine pairing exchange was HTTP " + response.status + (scope ? " scope=" + scope : "") };
    }
    return { ok: true, bearer: json.token };
  } catch (error) {
    return { ok: false, reason: "machine pairing failed: " + (error && error.message ? error.message : String(error)) };
  } finally {
    if (control) {
      try { control.close(); } catch { /* the socket is already gone */ }
    }
  }
}

function pass(name, detail) {
  return { name, status: "pass", detail: detail || null };
}

function failedLane(name, reason) {
  return { name, status: "failed", reason };
}

function unmeasured(name, reason) {
  return { name, status: "unmeasured", reason };
}

function settle(deps, evidence, lanes) {
  const status = classifyLane(lanes);
  evidence.finish(status, { lanes });
  if (status === "failed") {
    const lane = lanes.find((entry) => entry.status === "failed");
    throw deps.failed(lane.name, lane.reason);
  }
  if (status === "blocked") {
    const lane = lanes.find((entry) => entry.status === "blocked");
    throw deps.blocked(lane.reason, lane.detail || "");
  }
  if (status === "unmeasured") {
    return {
      status: "unmeasured",
      reason: lanes.filter((entry) => entry.status === "unmeasured").map((entry) => entry.name + ": " + entry.reason).join("; "),
      observed: { lanes },
    };
  }
  return { status: "pass", observed: { lanes } };
}


/**
 * Runner-side fault injection. Listens on 127.0.0.1:ephemeral and forwards to the isolated
 * gateway. A targeted POST is forwarded, the host is allowed to answer, then the client
 * socket is destroyed without relaying that answer. That is a transport loss: applyReferenceControl
 * maps a thrown post to status "unknown" (ui/src/remote/chat/referenceControl.ts), and
 * continueInNewPane's rejection path sets the composer notice to data-status="unknown"
 * (ui/src/remote/RemoteApp.tsx). Other requests are relayed with Connection: close and
 * keep-alive off, so the targeted POST arrives on a fresh socket. Chromium resends a POST
 * when a reused keep-alive socket closes with zero bytes; a fresh socket is not that resend.
 * Other scenarios never start this.
 */
export function outcomeUnknownTarget(kind, method, url, body) {
  if (String(method || "").toUpperCase() !== "POST") return false;
  const path = String(url || "").split("?")[0];
  if (kind === "control") return path.endsWith("/control");
  if (kind === "agentContinue") {
    if (path !== "/api/v1/sessions") return false;
    const text = Buffer.isBuffer(body) ? body.toString("utf8") : String(body || "");
    return text.includes('"agentContinue"');
  }
  return false;
}

export function assertProxyDroppedOnce(entries, label) {
  const name = label || "targeted request";
  if (!Array.isArray(entries) || entries.length !== 1) {
    fail(name + " was forwarded " + (entries ? entries.length : 0) + " times");
  }
  if (entries[0].forwarded !== true || entries[0].dropped !== true) {
    fail(name + " was not forwarded and then dropped");
  }
  if (entries[0].upstreamStatus === undefined) {
    fail(name + " was dropped before the host answered");
  }
  return entries[0];
}

/** requestId from a create/control body. Same field the daemon caches on. */
export function proxyBodyRequestId(body) {
  const text = Buffer.isBuffer(body) ? body.toString("utf8") : String(body || "");
  if (!text) return null;
  try {
    const parsed = JSON.parse(text);
    if (parsed && typeof parsed.requestId === "string" && parsed.requestId.length > 0) return parsed.requestId;
  } catch {
    return null;
  }
  return null;
}

/**
 * Session id a 2xx create names. Same fields createdSessionIdentity reads
 * (RemoteApp.tsx:838-851): sessionId, data.sessionId, target.sessionId.
 * A truncated body still yields the first sessionId string.
 */
export function proxyCreatedSessionId(text) {
  const raw = String(text || "");
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    const id = sessionIdFromCreateBody(parsed);
    if (id) return id;
  } catch {
    /* a captured prefix may not be whole JSON */
  }
  const match = raw.match(/"sessionId"\s*:\s*"([^"]+)"/);
  return match ? match[1] : null;
}

function sessionIdFromCreateBody(record) {
  if (!record || typeof record !== "object") return null;
  const data = record.data && typeof record.data === "object" ? record.data : null;
  const target = record.target && typeof record.target === "object"
    ? record.target
    : (data && data.target && typeof data.target === "object" ? data.target : null);
  const candidates = [record.sessionId, data && data.sessionId, target && target.sessionId];
  for (const value of candidates) {
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}

/** Every forwarded targeted entry must carry the request body's requestId. */
export function assertTargetedForwardIdentity(entries) {
  if (!Array.isArray(entries)) fail("forwarded targeted entries are missing");
  const forwarded = entries.filter((entry) => entry && entry.forwarded === true && entry.targeted === true);
  if (forwarded.length === 0) fail("no forwarded targeted entry carried a requestId");
  for (const entry of forwarded) {
    if (typeof entry.requestId !== "string" || entry.requestId.length === 0) {
      fail("forwarded targeted entry has no requestId");
    }
  }
  return { requestIds: [...new Set(forwarded.map((entry) => entry.requestId))], count: forwarded.length };
}

/** Upstream create must be 2xx and name one session. A refusal stays a failure. */
export function assertUnknownCreateUpstream(entry) {
  const status = Number(entry && entry.upstreamStatus);
  const body = entry && typeof entry.upstreamBody === "string" ? entry.upstreamBody : "";
  if (!(status >= 200 && status < 300)) {
    fail("agentContinue create upstream was " + status + " " + body.slice(0, 180));
  }
  const created = proxyCreatedSessionId(body);
  if (!created) fail("agentContinue create upstream " + status + " named no session id");
  return created;
}

/** The dropped create holds the startup: unknown notice, and the button stays disabled. */
export function assertUnknownCreateHeld(observation) {
  if (!observation || observation.status !== "unknown") {
    fail("unknown notice data-status was " + (observation && observation.status));
  }
  if (typeof observation.text !== "string" || !observation.text.includes("will not be started again")) {
    fail("unknown notice did not say the startup will not be started again");
  }
  if (observation.disabled !== true) fail("continuation button was not disabled after unknown");
  if (observation.retried === true) fail("the client retried the create");
  return observation;
}

/**
 * One new pane, and it is the session the host created for this request id.
 * A browser resend of the same requestId must not grow the inventory again
 * (session_service.rs cache_key by request id).
 */
export function assertUnknownCreateReconciled(before, after, createdId, sourceId) {
  assertOriginalSessionPreserved({ sourceId, sessions: before }, { sessions: after });
  const beforeIds = new Set((before || []).map((row) => row && row.id));
  const added = (after || []).filter((row) => row && !beforeIds.has(row.id));
  if (added.length !== 1) fail("unknown create added " + added.length + " panes");
  if (added[0].id !== createdId) fail("inventory pane " + added[0].id + " is not the created session " + createdId);
  return added[0];
}

export function assertPayloadMatchesRef(result, ref) {
  const body = payloadBody(result);
  if (!ref || typeof ref.ref !== "string" || ref.ref.length === 0) fail("payload ref is missing");
  if (typeof ref.byteLength === "number" && body.byteLength !== ref.byteLength) {
    fail("payload byteLength " + body.byteLength + " does not match the page ref " + ref.byteLength);
  }
  if (typeof ref.digest === "string" && ref.digest.length > 0 && body.digest !== ref.digest) {
    fail("payload digest does not match the page ref");
  }
  if (ref.kind && body.kind !== ref.kind) fail("payload kind " + body.kind + " does not match " + ref.kind);
  return body;
}

export function payloadRefs(page) {
  const refs = [];
  const turns = page && Array.isArray(page.turns) ? page.turns : [];
  for (const turn of turns) {
    for (const part of turn.parts || []) {
      if (!part || typeof part !== "object") continue;
      if (part.outputPayload && typeof part.outputPayload.ref === "string") {
        refs.push({
          kind: "output",
          ref: part.outputPayload.ref,
          byteLength: part.outputPayload.byteLength,
          digest: part.outputPayload.digest,
        });
      }
      if (part.kind === "diff" && part.patch && typeof part.patch.ref === "string") {
        refs.push({
          kind: "diff",
          ref: part.patch.ref,
          byteLength: part.patch.byteLength,
          digest: part.patch.digest,
        });
      }
    }
  }
  return refs;
}

function rewriteUpstreamLocation(location, upstream, proxyOrigin) {
  if (!location || !proxyOrigin) return location;
  if (location.startsWith(upstream.origin)) return proxyOrigin + location.slice(upstream.origin.length);
  return location;
}

export const PROXY_BODY_LIMIT = 512;
/** Targeted creates and the page capabilities document need more than the error prefix. */
export const PROXY_CAPTURE_LIMIT = 4096;

/**
 * The proxied page must open the claude fixture pane. The first discovery
 * provider, or the local fallback, can be an omo pane, and that pane never
 * publishes composer.models.choices or a claude continuation affordance.
 */
export function claudeFixturePane(sessions, fallback) {
  const rows = Array.isArray(sessions) ? sessions : [];
  const claude = rows.filter((row) => registryOf(row) === "claude" && row && row.backendSessionId);
  return claude.find((row) => row.providerSessionId) || claude[0]
    || (fallback && registryOf(fallback) === "claude" && fallback.backendSessionId ? fallback : null);
}

/**
 * QA-16 unknown-create source. Fork is offered only for a non-working source
 * (agentResumeAffordance.ts:295-296). A pane the claim hook already marked
 * working is not eligible.
 */
export function unknownCreatePane(sessions, fallback, claimedWorkingIds) {
  const claimed = claimedWorkingIds instanceof Set
    ? claimedWorkingIds
    : new Set(Array.isArray(claimedWorkingIds) ? claimedWorkingIds : []);
  const rows = Array.isArray(sessions) ? sessions : [];
  const idle = rows.find((row) => (
    registryOf(row) === "claude"
    && row
    && row.backendSessionId
    && row.providerSessionId
    && !claimed.has(row.backendSessionId)
  ));
  if (idle) return idle;
  if (
    fallback
    && registryOf(fallback) === "claude"
    && fallback.backendSessionId
    && fallback.providerSessionId
    && !claimed.has(fallback.backendSessionId)
  ) {
    return fallback;
  }
  return null;
}

/** Agent-control states that are not sourceIsWorking. */
export function agentControlNonWorking(state) {
  return state === "idle" || state === "blocked" || state === "unknown";
}

/** The sessions-list row for this pane. The strip reads target.sessionId, not a history query. */
export function sessionRowForPane(sessionsJson, sessionId) {
  const sessions = sessionsJson && Array.isArray(sessionsJson.sessions) ? sessionsJson.sessions : [];
  return sessions.find((entry) => {
    if (!entry || typeof entry !== "object") return false;
    if (entry.sessionId === sessionId) return true;
    return Boolean(entry.target && entry.target.sessionId === sessionId);
  }) || null;
}

/** providerSession.id as chatProviderSessionFromRows reads it. Snake_case is not that field. */
export function publishedProviderSessionId(row) {
  if (!row || typeof row !== "object") return null;
  const provider = row.providerSession;
  if (!provider || typeof provider !== "object") return null;
  return typeof provider.id === "string" && provider.id.length > 0 ? provider.id : null;
}

/**
 * Selection tab the desktop publishes for an agent pane (App.tsx:716-717).
 * agentType comes from activity.agentType. Idle omits activityState: the desktop
 * sends that field only when state is working or the activity is unseen.
 * After a real exit App.tsx also sends running false and the normalized providerSession.
 */
export function desktopContinuationTab(pane) {
  const rawSessionId = pane && typeof pane.backendSessionId === "string" && pane.backendSessionId.length > 0
    ? pane.backendSessionId
    : null;
  const sessionId = rawSessionId && !rawSessionId.startsWith("standby:") ? rawSessionId : null;
  const agentType = pane ? registryOf(pane) : "";
  if (pane && pane.exited === true) {
    const provider = pane.providerSession && typeof pane.providerSession === "object" ? pane.providerSession : null;
    const id = provider && typeof provider.id === "string" && provider.id.length > 0
      ? provider.id
      : (pane.providerSessionId && typeof pane.providerSessionId === "string" ? pane.providerSessionId : "");
    const transcriptPath = provider && typeof provider.transcriptPath === "string" ? provider.transcriptPath : "";
    return fillExitedAgentTab({
      tabId: typeof pane.tabId === "string" && pane.tabId.length > 0 ? pane.tabId : (sessionId || ""),
      label: typeof pane.label === "string" && pane.label.length > 0 ? pane.label : "Terminal",
      agentType,
      worktreeLabel: typeof pane.worktreeLabel === "string" ? pane.worktreeLabel : "",
      sessionId: sessionId || "",
      providerKey: provider && (provider.key === "session_id" || provider.key === "conversation_id") ? provider.key : "session_id",
      providerSessionId: id,
      transcriptPath,
      launcher: provider && provider.launcher === "mhc" ? "mhc" : "",
    });
  }
  const tab = {
    id: sessionId,
    label: "Terminal",
    sessionId,
  };
  if (agentType) tab.agentType = agentType;
  return tab;
}

/** Workspace-state tab the page reads. Sessions-row agentType is not this field. */
export function publishedWorkspaceTab(stateJson, sessionId) {
  const empty = { found: false, agentType: null, activityState: null, title: null, running: null, providerSessionId: null };
  if (!stateJson || typeof stateJson !== "object" || !sessionId) return empty;
  const contexts = [stateJson.activeContext, stateJson.context, stateJson].filter((value) => value && typeof value === "object");
  for (const context of contexts) {
    const rows = context.terminalTabs || context.tabs;
    if (!Array.isArray(rows)) continue;
    for (const tab of rows) {
      if (!tab || typeof tab !== "object") continue;
      if (tab.sessionId !== sessionId && tab.id !== sessionId) continue;
      const title = typeof tab.label === "string" && tab.label.length > 0
        ? tab.label
        : (typeof tab.title === "string" && tab.title.length > 0 ? tab.title : null);
      const provider = tab.providerSession && typeof tab.providerSession === "object" ? tab.providerSession : null;
      return {
        found: true,
        agentType: typeof tab.agentType === "string" && tab.agentType.length > 0 ? tab.agentType : null,
        activityState: typeof tab.activityState === "string" && tab.activityState.length > 0 ? tab.activityState : null,
        title,
        running: tab.running === false ? false : tab.running === true ? true : null,
        providerSessionId: provider && typeof provider.id === "string" && provider.id.length > 0 ? provider.id : null,
      };
    }
  }
  return empty;
}

/** Bound for either a continue button or a typed unavailable reason. Not the scenario timeout. */
export const CONTINUE_STRIP_WAIT_MS = 10000;

/** Proxied-page strip snapshot. testId/reason are the DOM attributes, not a guess. */
export function parseContinuationStripDom(snapshot) {
  const nodes = snapshot && Array.isArray(snapshot.nodes) ? snapshot.nodes : [];
  const buttons = [];
  const unavailable = [];
  for (const node of nodes) {
    if (!node || typeof node !== "object") continue;
    const testId = typeof node.testId === "string" ? node.testId : "";
    if (testId.startsWith("continue-in-new-pane-")) buttons.push(testId);
    if (testId.startsWith("continue-unavailable-")) {
      unavailable.push({
        testId,
        reason: typeof node.reason === "string" && node.reason.length > 0 ? node.reason : null,
      });
    }
  }
  const notice = snapshot && typeof snapshot.notice === "string" ? snapshot.notice : "";
  return {
    buttons,
    unavailable,
    noAgentNotice: notice.includes("No agent is running in this terminal"),
    workspacePresent: Boolean(snapshot && snapshot.workspacePresent),
  };
}

/**
 * A typed data-reason that cannot be what continuationReason returns for these facts.
 * A reason that matches the facts explains a missing button and is not a product fail.
 */
export function continuationReasonContradiction(facts) {
  const reasons = facts && Array.isArray(facts.reasons) ? facts.reasons.filter(Boolean) : [];
  const availability = facts && facts.availability != null ? String(facts.availability) : "";
  const providerSessionId = facts && facts.providerSessionId ? String(facts.providerSessionId) : "";
  const agentState = facts && facts.agentState != null ? String(facts.agentState) : "";
  const activityState = facts && facts.activityState != null ? String(facts.activityState) : "";
  const tabAgentType = facts && facts.tabAgentType ? String(facts.tabAgentType) : "";
  if (facts && facts.capabilityAdvertised === false && reasons.some((reason) => reason !== "remoteCapabilityAbsent")) {
    return "typed reason " + reasons.join(",")
      + " contradicts a missing terminalContinueV1 advertisement; continuationReason returns remoteCapabilityAbsent before any other reason (agentResumeAffordance.ts:345)";
  }
  if (reasons.includes("remoteCapabilityAbsent") && facts && facts.capabilityAdvertised === true) {
    return "typed reason remoteCapabilityAbsent contradicts advertised terminalContinueV1; continuationReason returns remoteCapabilityAbsent only when the remote owner lacks the capability (agentResumeAffordance.ts:345)";
  }
  if (reasons.includes("missingNativeHistory") && availability === "native" && providerSessionId && tabAgentType) {
    return "typed reason missingNativeHistory contradicts native history, a published providerSession, and tab agentType; continuationReason returns missingNativeHistory without extractNormalizedProviderSession or when agentType is absent (agentResumeAffordance.ts:352-353)";
  }
  if (reasons.includes("sourceWorking") && agentState !== "working" && activityState !== "working") {
    return "typed reason sourceWorking contradicts a non-working source; continuationReason returns sourceWorking only from sourceIsWorking (agentResumeAffordance.ts:355, 295-296)";
  }
  if (reasons.includes("providerUnsupported") && tabAgentType === "claude") {
    return "typed reason providerUnsupported contradicts a claude tab; verifiedContinuationModes includes fork and resume for claude (agentResume.ts:223, continuationReason agentResumeAffordance.ts:346)";
  }
  return null;
}

/** Tab activityState RemoteApp feeds getContinuationAffordance. Absent is not working. */
export function publishedPaneActivity(stateJson, sessionId) {
  if (!stateJson || typeof stateJson !== "object" || !sessionId) return null;
  const contexts = [stateJson.activeContext, stateJson.context, stateJson].filter((value) => value && typeof value === "object");
  for (const context of contexts) {
    const rows = context.terminalTabs || context.tabs;
    if (!Array.isArray(rows)) continue;
    for (const tab of rows) {
      if (!tab || typeof tab !== "object") continue;
      if (tab.sessionId !== sessionId && tab.id !== sessionId) continue;
      if (typeof tab.activityState === "string" && tab.activityState.length > 0) return tab.activityState;
    }
  }
  return null;
}

/**
 * The row RemoteApp continuationAffordance feeds getContinuationAffordance.
 * A sessions-row providerSession wins. The published tab's providerSession is
 * used only when that row is absent. A running sessions row wins over the tab
 * running flag. activityState still comes from the workspace tab.
 */
export function continuationPageRow(sessionsJson, stateJson, sessionId) {
  const row = sessionRowForPane(sessionsJson, sessionId);
  const tab = publishedWorkspaceTab(stateJson, sessionId);
  const rowProvider = publishedProviderSessionId(row);
  if (row) {
    return {
      found: true,
      sessionId: sessionId || null,
      agentType: typeof row.agentType === "string" ? row.agentType : tab.agentType,
      running: row.running === true,
      providerSessionId: rowProvider,
      activityState: tab.activityState,
    };
  }
  return {
    found: tab.found,
    sessionId: sessionId || null,
    agentType: tab.agentType,
    running: tab.running,
    providerSessionId: tab.providerSessionId,
    activityState: tab.activityState,
  };
}

/**
 * A missing continue-in-new-pane button is a product failure only when this
 * source is verified non-working and native history was published.
 * sourceWorking is a harness selection defect.
 */
function productUnknownCreate(sessionId, detail, extras) {
  return {
    status: "failed",
    reason: "PRODUCT: " + detail
      + " on non-working claude pane " + sessionId
      + " with verified native history and published providerSession " + extras.providerSessionId
      + " (agent-control state=" + extras.agentState + ", activityState=" + (extras.activityState || "absent") + ", availability=native)"
      + extras.reasonSuffix
      + "; RemoteApp.tsx:5367-5371 offers a mode only when getContinuationAffordance does (agentResumeAffordance.ts:344-360)",
  };
}

export function classifyUnknownCreateAbsence(input) {
  const reasons = input && Array.isArray(input.reasons) ? input.reasons.filter(Boolean) : [];
  const agentState = input && input.agentState != null ? String(input.agentState) : "";
  const availability = input && input.historyAvailability != null ? String(input.historyAvailability) : "";
  const sessionId = input && input.sessionId ? String(input.sessionId) : "unknown";
  const pageRow = input && input.pageRow && typeof input.pageRow === "object" ? input.pageRow : null;
  const providerSessionId = pageRow && typeof pageRow.providerSessionId === "string" ? pageRow.providerSessionId : "";
  const activityState = pageRow && pageRow.activityState != null ? String(pageRow.activityState) : "";
  const tab = input && input.tab && typeof input.tab === "object" ? input.tab : null;
  const tabActivity = tab && tab.activityState != null ? String(tab.activityState) : "";
  const reasonSuffix = reasons.length ? "; data-reason=" + reasons.join(",") : "";
  if (agentState === "working" || activityState === "working" || tabActivity === "working") {
    return {
      status: "unmeasured",
      reason: "continuation strip hid continue-in-new-pane on claude pane " + sessionId
        + " because the source is working" + reasonSuffix
        + (agentState ? "; agent-control state=" + agentState : "")
        + (activityState ? "; activityState=" + activityState : "")
        + "; getContinuationAffordance offers fork only for a non-working source (agentResumeAffordance.ts:295-296, 344-360). Measure unknown-create before the claim hook. Not a product fail.",
    };
  }
  if (availability !== "native") {
    return {
      status: "unmeasured",
      reason: "native history was not verified for claude pane " + sessionId
        + " (availability=" + (availability || "absent") + ")" + reasonSuffix
        + "; RemoteApp.tsx:5371 sets nativeHistory from chatPage.availability, and a missing button is not a product fail until that read is native",
    };
  }
  if (!agentControlNonWorking(agentState)) {
    return {
      status: "unmeasured",
      reason: "agent-control did not confirm a non-working source on claude pane " + sessionId
        + " (state=" + (agentState || "unread") + ")" + reasonSuffix
        + "; refusing to call a hidden fork button a product fail without that observe",
    };
  }
  if (!providerSessionId) {
    return {
      status: "unmeasured",
      reason: "page session row published no providerSession for claude pane " + sessionId
        + reasonSuffix
        + (agentState ? "; agent-control state=" + agentState : "")
        + "; continuationReason returns missingNativeHistory without extractNormalizedProviderSession (agentResumeAffordance.ts:352-353, RemoteApp.tsx:747-776). Reference-chat history is not that row. Not a product fail.",
    };
  }
  const tabAgentType = tab && typeof tab.agentType === "string" ? tab.agentType : "";
  const effectiveActivity = tabActivity || activityState;
  const publishedTabAgentType = input && typeof input.publishedTabAgentType === "string" ? input.publishedTabAgentType : "";
  const capabilityKnown = Boolean(input) && Object.prototype.hasOwnProperty.call(input, "capabilityAdvertised");
  const capabilityAdvertised = Boolean(input && input.capabilityAdvertised);
  const dom = input && input.dom && typeof input.dom === "object" ? input.dom : null;
  const buttons = dom && Array.isArray(dom.buttons) ? dom.buttons.filter(Boolean) : [];
  const extras = { providerSessionId, agentState, activityState: effectiveActivity, reasonSuffix };
  const notice = dom && dom.noAgentNotice === true
    ? "; proxied page showed the no-agent notice, which RemoteApp.tsx:4166 sets only when chatAgentType is missing"
    : "";
  if (!tab || tab.found === false || !tabAgentType) {
    if (publishedTabAgentType) {
      return productUnknownCreate(
        sessionId,
        "remoteSetActiveSelection published agentType " + publishedTabAgentType
          + " but the workspace-state tab has no agentType" + notice
          + "; chatAgentType is activeObservedTab.agentType (RemoteApp.tsx:3594-3596) and continuationAffordance is null without it (RemoteApp.tsx:5353-5354). set_active_selection stores that tab (daemon/server.rs:8153-8158); get_workspace_state fills activity_state only (server.rs:1080-1088)",
        extras,
      );
    }
    return {
      status: "unmeasured",
      reason: "workspace-state tab for pane " + sessionId + " has no agentType" + reasonSuffix + notice
        + "; chatAgentType is activeObservedTab.agentType (RemoteApp.tsx:3594-3596), so continuationAffordance is null (RemoteApp.tsx:5353-5354) and the strip renders nothing. "
        + "A desktop claude pane carries agentType because App.tsx:717 copies activity.agentType onto the selection tab; idle omits activityState (App.tsx:716). "
        + "get_workspace_state does not invent agent_type (server.rs:1080-1088). The sessions row is not that tab. Not a product fail.",
    };
  }
  if (tabAgentType !== "claude") {
    return {
      status: "unmeasured",
      reason: "workspace-state tab agentType is " + tabAgentType + " not claude on pane " + sessionId
        + reasonSuffix + "; getContinuationAffordance is not the claude fork offer. Not a product fail.",
    };
  }
  if (!capabilityKnown || !capabilityAdvertised) {
    return {
      status: "unmeasured",
      reason: "terminalContinueV1 was not advertised for pane " + sessionId
        + reasonSuffix
        + "; continuationReason returns remoteCapabilityAbsent when the remote owner lacks it (agentResumeAffordance.ts:345, 340-342). Not a product fail.",
    };
  }
  const contradiction = continuationReasonContradiction({
    reasons,
    availability,
    providerSessionId,
    agentState,
    activityState: effectiveActivity,
    tabAgentType,
    capabilityAdvertised,
  });
  if (contradiction) return productUnknownCreate(sessionId, contradiction + notice, extras);
  if (buttons.length > 0) {
    return {
      status: "unmeasured",
      reason: "continue-in-new-pane was present (" + buttons.join(",") + ") on pane " + sessionId + ". Not an absence.",
    };
  }
  if (reasons.length > 0) {
    return {
      status: "unmeasured",
      reason: "continuation strip published a typed unavailable reason on pane " + sessionId
        + reasonSuffix
        + "; that reason is not contradicted by the tab, native history, providerSession, or advertised terminalContinueV1. Not a product fail.",
    };
  }
  return productUnknownCreate(
    sessionId,
    "continuation strip published no continue-in-new-pane button" + notice,
    extras,
  );
}

/**
 * QA-13 outcome-unknown is a composer control apply (plan T12). No rendered
 * control POSTs /control without the list_models probe, so the lane stays
 * unmeasured. Stop and slash are not that apply.
 */
export function composerControlOutcomeUnknownProof(sessionId) {
  const id = sessionId || "unknown";
  return "composer-model-control was not rendered on claude pane " + id
    + "; every composer control that POSTs /control requires the list_models probe. "
    + "MobileChatComposer.tsx:535-537 shows model only when composer.models.choices is non-empty, effort only when that model has efforts (tsx:536, 827-841), and option selects only from composer.options (tsx:532-534, 853-870). "
    + "server.rs:7381-7407 reference_chat_attach_composer omits the document unless probed_kind is set and attach_composer succeeds; control.rs:514-540 and 543-577 fill page.composer only after discover() (control.rs:1255-1261 composer_from_models from parse_claude_list_models, model_discovery.rs:32-52). "
    + "Stop is not that apply: MobileChatComposer.tsx:977-984 renders stop-button when isRunning, and RemoteApp.tsx:4848-4856 posts suffix stop, not /control, and does not set composer-control-notice data-status=unknown. "
    + "referenceControl.ts:15-16 names only model and effort; there is no mode control. "
    + "Slash inserts text (MobileChatComposer.tsx:905-910) and does not POST /control. "
    + "Plan T12 unknown control path is the control apply, so stop and slash do not satisfy it. A planted transcript does not publish choices.";
}

function rewriteReferer(referer, upstream) {
  try {
    const url = new URL(String(referer));
    return upstream.origin + url.pathname + url.search + url.hash;
  } catch {
    return upstream.origin + "/";
  }
}

/**
 * Host is the gateway. Origin and Referer are rewritten to that same origin
 * for every relayed request: cors_origin_allowed (server.rs) admits Origin
 * only when it equals http(s)://{Host}, the relay, or a saved paired host.
 */
export function proxyUpstreamHeaders(headers, upstream) {
  const next = { ...(headers || {}), host: upstream.host };
  delete next["transfer-encoding"];
  if (next.origin) next.origin = upstream.origin;
  if (next.referer) next.referer = rewriteReferer(next.referer, upstream);
  return next;
}

/**
 * Relayed responses must not leave a reusable keep-alive socket. Chromium
 * resends a POST when a reused socket closes with zero response bytes. A
 * fresh connection that is then destroyed is a real failure, not a stale
 * resend. assertProxyDroppedOnce stays exactly one forward.
 */
export function relayedProxyResponseHeaders(headers) {
  const next = { ...(headers || {}) };
  delete next.connection;
  delete next["keep-alive"];
  next.connection = "close";
  return next;
}

export function truncatedUpstreamBody(status, text, totalLength) {
  if (!(Number(status) >= 400)) return undefined;
  const body = String(text || "");
  const total = typeof totalLength === "number" ? totalLength : body.length;
  if (total <= PROXY_BODY_LIMIT) return body;
  return body.slice(0, PROXY_BODY_LIMIT) + "…(" + total + " chars)";
}

/**
 * Which proxy records must reach http.jsonl. Targeted posts were the only ones
 * mirrored, so a non-2xx sidebar or history answer never appeared in the receipt.
 */
export function proxyEntryMirrored(entry) {
  if (!entry) return false;
  if (entry.method === "LISTEN") return true;
  if (entry.targeted) return true;
  if (Number(entry.upstreamStatus) >= 400 || Number(entry.status) >= 400) return true;
  const path = String(entry.path || "");
  return path.includes("/api/v1/capabilities")
    || path.includes("/api/v1/workspace/sidebar")
    || path.includes("/api/v1/workspace/state")
    || path.includes("/api/v1/sessions")
    || path.includes("/api/v1/agent-control/");
}

export function expectedContinueArgv(agent, id, mode) {
  const kind = String(agent || "").trim().toLowerCase();
  const sessionId = String(id || "");
  if (!sessionId) return null;
  if (kind === "claude" && mode === "fork") return ["claude", "--resume", sessionId, "--fork-session"];
  if (kind === "claude" && mode === "resume") return ["claude", "--resume", sessionId];
  if (kind === "codex" && mode === "fork") return ["codex", "fork", sessionId];
  if (kind === "codex" && mode === "resume") return ["codex", "resume", sessionId];
  return null;
}

export function argvMatchesContinue(argv, expected) {
  if (!Array.isArray(argv) || !Array.isArray(expected) || expected.length < 2) return false;
  const program = String(argv[0] || "").split(/[/\\]/).pop();
  if (program !== expected[0]) return false;
  const rest = argv.slice(1).map(String);
  const want = expected.slice(1);
  for (let index = 0; index <= rest.length - want.length; index += 1) {
    if (want.every((arg, offset) => rest[index + offset] === arg)) return true;
  }
  return false;
}

export function assertContinueAcceptance(observation) {
  if (!observation || typeof observation !== "object") fail("continuation acceptance has no observation");
  const created = observation.createdSessionId;
  if (typeof created !== "string" || created.length === 0) fail("continuation created no session id");
  if (created === observation.sourceId) fail("continuation reused the source pane");
  assertOriginalSessionPreserved(
    { sourceId: observation.sourceId, sessions: observation.before },
    { sessions: observation.after },
  );
  const added = (observation.after || []).find((row) => row.id === created);
  if (!added) fail("inventory has no new pane " + created);
  if (added.workspaceId !== observation.workspaceId) fail("new pane workspace changed");
  if (JSON.stringify(added.worktree || null) !== JSON.stringify(observation.worktree || null)) {
    fail("new pane worktree changed");
  }
  const expected = expectedContinueArgv(observation.agentType, observation.providerSessionId, observation.mode);
  if (!expected) fail("no host argv is defined for " + observation.agentType + " " + observation.mode);
  if (!argvMatchesContinue(observation.argv, expected)) {
    fail("new pane argv did not match the host plan " + expected.join(" "));
  }
  const beforeIds = new Set((observation.before || []).map((row) => row.id));
  const spawned = (observation.after || []).filter((row) => !beforeIds.has(row.id));
  if (spawned.length !== 1) fail("continuation created " + spawned.length + " panes");
  return added;
}

export function numericPid(value) {
  if (typeof value === "number" && Number.isInteger(value) && value > 0) return value;
  if (typeof value === "string" && /^[0-9]+$/.test(value)) {
    const parsed = Number(value);
    return parsed > 0 ? parsed : null;
  }
  if (value && typeof value === "object") return numericPid(value.pid);
  return null;
}

function localProcessArgv(pid) {
  const number = numericPid(pid);
  if (!number) return null;
  const path = "/proc/" + number + "/cmdline";
  if (!existsSync(path)) return null;
  try {
    return readFileSync(path).toString("utf8").split("\0").filter((part) => part.length > 0);
  } catch {
    return null;
  }
}

function ppidOf(pid) {
  try {
    const stat = readFileSync("/proc/" + pid + "/stat", "utf8");
    const end = stat.lastIndexOf(")");
    if (end < 0) return null;
    const ppid = Number(stat.slice(end + 2).split(" ")[1]);
    return Number.isInteger(ppid) && ppid > 0 ? ppid : null;
  } catch {
    return null;
  }
}

function directChildPids(pid) {
  const found = new Set();
  const childrenPath = "/proc/" + pid + "/task/" + pid + "/children";
  if (existsSync(childrenPath)) {
    try {
      for (const part of readFileSync(childrenPath, "utf8").trim().split(/\s+/)) {
        const child = numericPid(part);
        if (child) found.add(child);
      }
    } catch { /* the children file is optional */ }
  }
  let names = [];
  try {
    names = readdirSync("/proc");
  } catch {
    names = [];
  }
  for (const name of names) {
    if (!/^[0-9]+$/.test(name)) continue;
    if (ppidOf(name) === pid) found.add(Number(name));
  }
  return [...found];
}

/** Pane pid plus one child level. shell.rs:688 runs the agent as a child of /bin/sh, not as argv[0]. */
export function readProcessArgvTree(pid) {
  const number = numericPid(pid);
  const argvs = [];
  if (!number) return argvs;
  const root = localProcessArgv(number);
  if (root) argvs.push(root);
  for (const child of directChildPids(number)) {
    const argv = localProcessArgv(child);
    if (argv) argvs.push(argv);
  }
  return argvs;
}

export function matchingContinueArgv(argvs, expected) {
  if (!Array.isArray(argvs)) return null;
  for (const argv of argvs) {
    if (argvMatchesContinue(argv, expected)) return argv;
  }
  return null;
}

function captureUpstreamPrefix(stream, status, finish, limit) {
  const cap = Number(limit) > 0 ? Number(limit) : PROXY_BODY_LIMIT;
  if (!(Number(status) >= 400) && !(Number(limit) > 0)) {
    finish();
    return;
  }
  const chunks = [];
  let kept = 0;
  let total = 0;
  let settled = false;
  stream.on("data", (chunk) => {
    total += chunk.length;
    if (kept >= cap) return;
    const room = cap - kept;
    chunks.push(chunk.subarray(0, room));
    kept += Math.min(chunk.length, room);
  });
  const done = () => {
    if (settled) return;
    settled = true;
    const text = Buffer.concat(chunks).toString("utf8");
    if (Number(status) >= 400) finish(truncatedUpstreamBody(status, text, total));
    else finish(total <= cap ? text : text.slice(0, cap));
  };
  stream.on("end", done);
  stream.on("error", done);
}

export function startOutcomeUnknownProxy(options) {
  const upstream = new URL(options.upstream);
  const evidenceDir = options.evidenceDir;
  if (!evidenceDir) fail("outcome-unknown proxy has no evidence dir");
  mkdirSync(evidenceDir, { recursive: true });
  const logPath = join(evidenceDir, "outcome-unknown-proxy.jsonl");
  writeFileSync(logPath, "");
  const log = [];
  let proxyOrigin = "";
  function record(entry) {
    log.push(entry);
    appendFileSync(logPath, JSON.stringify(entry) + "\n");
    if (options.onHttp && proxyEntryMirrored(entry)) options.onHttp(entry);
  }
  const server = http.createServer((req, res) => {
    // No HTTP keep-alive toward the browser. The targeted drop destroys the
    // client socket; that is only a single forward when the POST is not a
    // reused keep-alive socket Chromium will resend.
    res.shouldKeepAlive = false;
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      const body = Buffer.concat(chunks);
      let targeted = false;
      try {
        targeted = options.match(req.method, req.url || "", body) === true;
      } catch {
        targeted = false;
      }
      const requestId = proxyBodyRequestId(body);
      const pathOnly = String(req.url || "").split("?")[0];
      const headers = { ...req.headers, host: upstream.host };
      delete headers["transfer-encoding"];
      headers["content-length"] = String(body.length);
      const forwarded = proxyUpstreamHeaders(headers, upstream);
      forwarded["content-length"] = String(body.length);
      const upstreamReq = http.request({
        protocol: upstream.protocol,
        hostname: upstream.hostname,
        port: upstream.port,
        method: req.method,
        path: req.url,
        headers: forwarded,
      }, (upstreamRes) => {
        const status = upstreamRes.statusCode;
        const finish = (upstreamBody) => {
          record({
            at: new Date().toISOString(),
            method: req.method,
            path: req.url,
            proxy: true,
            targeted,
            forwarded: true,
            dropped: targeted,
            upstreamStatus: status,
            requestId,
            ...(upstreamBody !== undefined ? { upstreamBody } : {}),
          });
        };
        const captureLimit = targeted || pathOnly === "/api/v1/capabilities" ? PROXY_CAPTURE_LIMIT : 0;
        if (targeted) {
          captureUpstreamPrefix(upstreamRes, status, finish, PROXY_CAPTURE_LIMIT);
          upstreamRes.resume();
          upstreamRes.on("error", () => {});
          try { req.socket.destroy(); } catch { /* the client is already gone */ }
          return;
        }
        if (captureLimit > 0 || Number(status) >= 400) {
          captureUpstreamPrefix(upstreamRes, status, finish, captureLimit || PROXY_BODY_LIMIT);
        } else finish();
        const responseHeaders = { ...upstreamRes.headers };
        if (responseHeaders.location) {
          responseHeaders.location = rewriteUpstreamLocation(responseHeaders.location, upstream, proxyOrigin);
        }
        res.writeHead(status || 502, relayedProxyResponseHeaders(responseHeaders));
        upstreamRes.pipe(res);
      });
      upstreamReq.on("error", (error) => {
        record({
          at: new Date().toISOString(),
          method: req.method,
          path: req.url,
          proxy: true,
          targeted,
          forwarded: false,
          dropped: false,
          requestId,
          error: error.message,
        });
        if (!res.headersSent && !res.writableEnded) {
          try {
            res.writeHead(502, relayedProxyResponseHeaders({ "content-type": "text/plain" }));
            res.end();
          } catch { /* client already gone */ }
        }
      });
      upstreamReq.end(body);
    });
  });
  server.keepAliveTimeout = 1;
  server.on("upgrade", (req, socket, head) => {
    const port = Number(upstream.port) || 80;
    const upstreamSocket = net.connect(port, upstream.hostname, () => {
      const lines = [req.method + " " + req.url + " HTTP/1.1"];
      for (const [key, value] of Object.entries(req.headers)) {
        const items = Array.isArray(value) ? value : [value];
        for (const item of items) {
          if (item === undefined) continue;
          const name = key.toLowerCase();
          if (name === "host") lines.push("host: " + upstream.host);
          else if (name === "origin") lines.push("origin: " + upstream.origin);
          else if (name === "referer") lines.push("referer: " + rewriteReferer(item, upstream));
          else lines.push(key + ": " + item);
        }
      }
      upstreamSocket.write(lines.join("\r\n") + "\r\n\r\n");
      if (head && head.length > 0) upstreamSocket.write(head);
      upstreamSocket.pipe(socket);
      socket.pipe(upstreamSocket);
    });
    upstreamSocket.on("error", () => socket.destroy());
    socket.on("error", () => upstreamSocket.destroy());
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const bound = server.address();
      if (!bound || typeof bound === "string") {
        reject(new Error("outcome-unknown proxy did not bind an ephemeral port"));
        return;
      }
      proxyOrigin = "http://127.0.0.1:" + bound.port;
      record({
        at: new Date().toISOString(),
        method: "LISTEN",
        path: proxyOrigin,
        proxy: true,
        targeted: false,
        forwarded: false,
        upstreamStatus: 0,
        body: "bound",
      });
      resolve({
        url: proxyOrigin,
        logPath,
        log,
        close() {
          return new Promise((done) => {
            if (typeof server.closeAllConnections === "function") server.closeAllConnections();
            server.close(() => done());
          });
        },
      });
    });
  });
}

function discoverySession(ctx, fallback) {
  return claudeFixturePane((ctx.fixture && ctx.fixture.sessions) || [], fallback);
}

function plantRetargetedClaude(ctx, session) {
  const planted = plantClaudeTranscript(ctx, session);
  if (!planted.ok) return planted;
  const root = plantCwdOf(session);
  if (!root) {
    return { ok: false, file: planted.file, reason: "fixture session has no workspaceRegistration.repoRoot; the planted cwd cannot be retargeted" };
  }
  try {
    writeFileSync(planted.file, retargetPlantedTranscriptCwd(readFileSync(planted.file, "utf8"), root));
  } catch (error) {
    return { ok: false, file: planted.file, reason: error.message };
  }
  return planted;
}

export function chatLaneBound(entry, sessionId) {
  if (!entry || !sessionId) return false;
  const path = String(entry.path || "");
  return path.includes("/api/v1/reference-chat/" + sessionId + "/")
    || path.includes("/api/v1/terminal/" + sessionId);
}

/**
 * Chat mode never renders data-testid="remote-terminal" (RemoteTerminal.tsx:2152 is
 * remote-terminal-grid, and viewMode chat mounts MobileChatWorkspace). A reference-chat
 * or terminal request for this id is the bind RemoteApp.tsx:3875-3907 makes once
 * effectiveSessionId is that pane.
 */
async function pageShowsPane(page, sessionId, timeoutMs, proxy) {
  const deadline = Date.now() + Number(timeoutMs || 0);
  const terminal = page.locator('[data-testid="remote-terminal"][data-session-id="' + sessionId + '"]');
  const tab = page.locator('[role="tab"][data-session-id="' + sessionId + '"]');
  const chat = page.locator('[data-testid="mobile-chat-workspace"]');
  let pageUrl = "unread";
  while (Date.now() <= deadline) {
    try { pageUrl = page.url(); } catch { pageUrl = "unread"; }
    if (await terminal.count() > 0) return { ok: true, via: "remote-terminal", pageUrl };
    if (await tab.count() > 0) return { ok: true, via: "tab", pageUrl };
    const bound = proxy && proxy.log.some((entry) => chatLaneBound(entry, sessionId));
    if (bound && await chat.count() > 0) return { ok: true, via: "reference-chat", pageUrl };
    await new Promise((resolve) => setTimeout(resolve, 40));
  }
  try { pageUrl = page.url(); } catch { pageUrl = "unread"; }
  return { ok: false, via: null, pageUrl };
}

function proxiedEntry(proxy, sessionId) {
  return proxy.url + "/#session=" + encodeURIComponent(sessionId);
}

async function dispatchModelChoice(page, valueId) {
  await page.evaluate((value) => {
    const node = document.querySelector('[data-testid="composer-model-control"]');
    if (!node) throw new Error("composer-model-control missing");
    node.value = value;
    node.dispatchEvent(new Event("change", { bubbles: true }));
  }, valueId);
}

/**
 * RemoteApp.tsx:5719-5743 opens #session= only when that id is already in
 * model.options or terminalTabs, and only when token is set and pending is null.
 * model starts as EMPTY_MODEL (RemoteApp.tsx:1587). model.options are worktree
 * rows from workspace/state, not GET /api/v1/sessions. A null sidebar model
 * (remoteSidebarSource.ts:237-245) falls back to the inventory; it does not
 * clear token or the hash. set_active_selection clears a machine-owned session
 * (state.rs:1329-1339) and still answers Ok, after which server.rs:1090-1163
 * publishes one live tab. The outcome is recorded either way.
 */
async function clearPublishedSelection(ctx, evidence) {
  const finish = (outcome) => {
    if (evidence) {
      evidence.event({
        at: new Date().toISOString(),
        frame: {
          type: "remoteSetActiveSelection",
          ok: outcome.ok,
          reason: outcome.ok ? null : outcome.reason,
          sessionId: null,
          cleared: true,
        },
      });
    }
    return outcome;
  };
  const gateway = (ctx.gateways || []).find((entry) => entry && entry.controlTransport);
  if (!gateway) return finish({ ok: false, reason: "no daemon control transport to clear remoteSetActiveSelection" });
  let control = null;
  try {
    control = await connectDaemonControl(gateway.controlTransport, {
      timeoutMs: (ctx.args && ctx.args.timeoutMs) || 15000,
    });
    const response = await control.call({ type: "remoteSetActiveSelection", selection: null });
    if (!response || response.type !== "remoteSetActiveSelectionOk") {
      return finish({ ok: false, reason: "clear remoteSetActiveSelection was " + JSON.stringify(response && response.type ? response.type : response) });
    }
    return finish({ ok: true });
  } catch (error) {
    return finish({ ok: false, reason: error && error.message ? error.message : String(error) });
  } finally {
    if (control) {
      try { control.close(); } catch { /* already closed */ }
    }
  }
}

async function publishFixturePane(ctx, pane, evidence) {
  const finish = (outcome) => {
    if (evidence) {
      evidence.event({
        at: new Date().toISOString(),
        frame: {
          type: "remoteSetActiveSelection",
          ok: outcome.ok,
          reason: outcome.ok ? null : outcome.reason,
          sessionId: pane.backendSessionId,
          workspaceId: pane.workspaceId || null,
          publishedAgentType: desktopContinuationTab(pane).agentType || null,
        },
      });
    }
    return outcome;
  };
  const gateway = (ctx.gateways || []).find((entry) => entry && entry.controlTransport);
  if (!gateway) return finish({ ok: false, reason: "no daemon control transport to publish remoteSetActiveSelection" });
  let control = null;
  try {
    control = await connectDaemonControl(gateway.controlTransport, {
      timeoutMs: (ctx.args && ctx.args.timeoutMs) || 15000,
    });
    const response = await control.call({
      type: "remoteSetActiveSelection",
      selection: {
        workspaceId: pane.workspaceId || null,
        worktreeSlug: null,
        worktreeLabel: null,
        sessionId: pane && pane.exited === true ? null : pane.backendSessionId,
        tabId: desktopContinuationTab(pane).id,
        terminalTabs: [desktopContinuationTab(pane)],
      },
    });
    if (!response || response.type !== "remoteSetActiveSelectionOk") {
      return finish({ ok: false, reason: "remoteSetActiveSelection was " + JSON.stringify(response && response.type ? response.type : response) });
    }
    return finish({ ok: true, responseType: response.type });
  } catch (error) {
    return finish({ ok: false, reason: error && error.message ? error.message : String(error) });
  } finally {
    if (control) {
      try { control.close(); } catch { /* already closed */ }
    }
  }
}

export function listingContainsPane(json, sessionId) {
  if (!json || typeof json !== "object" || !sessionId) return false;
  const contexts = [json.activeContext, json.context, json].filter((value) => value && typeof value === "object");
  for (const context of contexts) {
    const tabs = context.terminalTabs || context.tabs;
    if (Array.isArray(tabs) && tabs.some((tab) => tab && (tab.sessionId === sessionId || tab.id === sessionId))) return true;
  }
  const sessions = Array.isArray(json.sessions) ? json.sessions : [];
  return sessions.some((row) => row && (row.sessionId === sessionId || row.id === sessionId || (row.target && row.target.sessionId === sessionId)));
}

/** Fields RemoteApp.tsx:5720-5737 actually reads. Sessions-list membership is not one of them. */
export function workspaceStateSummary(json, sessionId) {
  const contexts = [json && json.activeContext, json && json.context, json].filter((value) => value && typeof value === "object");
  let activeSessionId = null;
  const tabs = [];
  for (const context of contexts) {
    if (!activeSessionId && typeof context.sessionId === "string") activeSessionId = context.sessionId;
    const rows = context.terminalTabs || context.tabs;
    if (!Array.isArray(rows)) continue;
    for (const tab of rows) {
      if (!tab || typeof tab !== "object") continue;
      tabs.push({ id: tab.id || null, sessionId: tab.sessionId || null });
    }
  }
  return {
    activeSessionId,
    tabs,
    inTabs: tabs.some((tab) => tab.sessionId === sessionId),
    activeMatches: activeSessionId === sessionId,
    inSessions: listingContainsPane({ sessions: json && json.sessions }, sessionId),
  };
}

export function statePublishesPane(json, sessionId) {
  const summary = workspaceStateSummary(json, sessionId);
  return summary.inTabs || summary.activeMatches;
}

async function recordProxiedGet(evidence, url, token, label) {
  const started = Date.now();
  const response = await fetch(url, {
    headers: { accept: "application/json", authorization: "Bearer " + token },
  });
  const text = await response.text();
  const bodyLimit = label === "/api/v1/workspace/state" ? text.length : 512;
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - started,
    method: "GET",
    suffix: label,
    path: label,
    auth: "machine-bearer",
    status: response.status,
    body: text.slice(0, bodyLimit),
  });
  let json = null;
  try {
    json = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  return { status: response.status, json, text };
}

async function waitForListedPane(ctx, proxyUrl, token, pane, evidence) {
  const timeoutMs = (ctx.args && ctx.args.timeoutMs) || 15000;
  const deadline = Date.now() + timeoutMs;
  const base = String(proxyUrl).replace(/\/+$/, "");
  const id = pane.backendSessionId;
  let last = "listing did not contain " + id;
  while (Date.now() <= deadline) {
    const state = await recordProxiedGet(evidence, base + "/api/v1/workspace/state", token, "/api/v1/workspace/state");
    const sidebar = await recordProxiedGet(evidence, base + "/api/v1/workspace/sidebar", token, "/api/v1/workspace/sidebar");
    const sessions = await recordProxiedGet(evidence, base + "/api/v1/sessions", token, "/api/v1/sessions");
    const summary = workspaceStateSummary(state.json, id);
    const inSessions = listingContainsPane(sessions.json, id);
    const sidebarModel = sidebar.json && Object.prototype.hasOwnProperty.call(sidebar.json, "model") ? sidebar.json.model : undefined;
    if (statePublishesPane(state.json, id) || inSessions) {
      return {
        ok: true,
        inState: statePublishesPane(state.json, id),
        inSessions,
        summary,
        sidebarModel,
        sidebar: sidebar.status,
        state: state.status,
        sessions: sessions.status,
      };
    }
    last = "state HTTP " + state.status + " sidebar HTTP " + sidebar.status + " sessions HTTP " + sessions.status;
    if (sidebar.status >= 400) last += " sidebarBody=" + textSlice(sidebar.text);
    else if (sidebarModel === null) last += " sidebarModel=null";
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return { ok: false, reason: last };
}

function textSlice(value) {
  return String(value || "").slice(0, 180);
}

async function prepareProxiedPane(ctx, proxy, pane, evidence, machine) {
  const bearer = machine && machine.ok ? machine : await machineScopeBearer(ctx);
  if (!bearer.ok) return { ok: false, reason: bearer.reason };
  const published = await publishFixturePane(ctx, pane, evidence);
  if (!published.ok) return { ok: false, reason: published.reason };
  const listed = await waitForListedPane(ctx, proxy.url, bearer.bearer, pane, evidence);
  if (!listed.ok) {
    return {
      ok: false,
      reason: "claude pane " + pane.backendSessionId + " was not in GET /api/v1/workspace/state or /api/v1/sessions before #session= (" + listed.reason + "); RemoteApp.tsx:5719-5743 opens #session= only from model.options or terminalTabs, and server.rs:1090-1163 publishes one live tab until remoteSetActiveSelection names it",
    };
  }
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "pane-listed",
      sessionId: pane.backendSessionId,
      inState: listed.inState,
      inSessions: listed.inSessions,
      summary: listed.summary,
      sidebarModel: listed.sidebarModel === undefined ? null : listed.sidebarModel,
    },
  });
  return { ok: true, bearer: bearer.bearer, listed, published };
}

function paneSelectFailure(pane, prepared, selected, ownership) {
  const id = pane.backendSessionId;
  const listed = prepared.listed || {};
  const pageUrl = selected.pageUrl || "unread";
  const summary = JSON.stringify(listed.summary || null);
  if (listed.inState) {
    return "PRODUCT: workspace/state published pane " + id
      + " in activeContext.sessionId or terminalTabs and the proxied page held a machine bearer, but it did not bind that pane (no reference-chat/" + id
      + " request and no [role=tab][data-session-id]). RemoteApp.tsx:5719-5743. page=" + pageUrl + " summary=" + summary;
  }
  if (ownership === "desktop") {
    return "PRODUCT: desktop-owned claude pane " + id
      + " was not kept in workspace/state terminalTabs after remoteSetActiveSelectionOk, so the page did not bind (page=" + pageUrl
      + "). set_active_selection clears only a machine_only session (state.rs:1329-1339). summary=" + summary;
  }
  return "proxied page did not bind claude pane " + id + " (page=" + pageUrl
    + "). remoteSetActiveSelectionOk does not keep a machine-owned session: set_active_selection clears it (state.rs:1329-1339) and get_workspace_state then publishes one live tab (server.rs:1090-1163). RemoteApp.tsx:5719-5743 opens #session= only from model.options or terminalTabs; GET /api/v1/sessions membership is not that list. summary=" + summary;
}

async function measureControlOutcomeUnknown(ctx, host, session, evidence, deps) {
  const machine = await machineScopeBearer(ctx);
  if (!machine.ok) return failedLane("outcome-unknown", machine.reason);
  const pane = unknownCreatePane((ctx.fixture && ctx.fixture.sessions) || [], session, []);
  if (!pane) {
    return unmeasured(
      "outcome-unknown",
      "the fixture has no idle claude pane with a providerSession; an omo page cannot publish composer.models.choices (server.rs reference_chat_attach_composer)",
    );
  }
  const planted = plantRetargetedClaude(ctx, pane);
  if (!planted.ok) {
    return unmeasured("outcome-unknown", planted.reason + "; " + composerControlOutcomeUnknownProof(pane.backendSessionId));
  }
  const observed = {
    historyStatus: null,
    availability: null,
    modelChoices: null,
    providerSessionId: pane.providerSessionId || null,
    pageUrl: "unread",
  };
  const snapshotHistory = async () => {
    const history = await readHistory(evidence, host, pane, { limit: 20 });
    observed.historyStatus = history.status;
    observed.availability = history.json && history.json.availability;
    const choices = history.json && history.json.composer && history.json.composer.models && history.json.composer.models.choices;
    observed.modelChoices = Array.isArray(choices)
      ? choices.map((choice) => choice && (choice.id || choice.valueId)).filter(Boolean).join(",") || String(choices.length)
      : null;
  };
  try {
    await snapshotHistory();
  } catch (error) {
    return failedLane("outcome-unknown", error && error.message ? error.message : String(error));
  }
  const binary = ctx.candidateRaw && ctx.candidateRaw.binary && ctx.candidateRaw.binary.path;
  const idleHook = paneIdleHookLine(binary, pane.providerSessionId);
  if (!idleHook) {
    return unmeasured("outcome-unknown", "candidate binary or fixture providerSessionId is absent; " + composerControlMissingEvidence(pane.backendSessionId, observed));
  }
  const typedIdle = await typePaneClaim(ctx, host, pane, machine.bearer, evidence, idleHook + "\r", "QA16_IDLE_SENT");
  if (!typedIdle.ok) {
    return unmeasured("outcome-unknown", typedIdle.reason + "; " + composerControlMissingEvidence(pane.backendSessionId, observed));
  }
  const published = await waitForPublishedProviderSession(
    ctx,
    host,
    pane.backendSessionId,
    pane.providerSessionId,
    machine.bearer,
    evidence,
  );
  if (!published.ok) {
    return unmeasured("outcome-unknown", published.reason + "; " + composerControlMissingEvidence(pane.backendSessionId, observed));
  }
  const createCwd = canonicalWorkspaceCwd(unknownCreateTranscriptCwd(pane));
  if (!createCwd) {
    return failedLane(
      "outcome-unknown",
      "workspace root is not a canonical directory; the unknown-create transcript cwd is that root",
    );
  }
  try {
    writeFileSync(planted.file, retargetPlantedTranscriptCwd(readFileSync(planted.file, "utf8"), createCwd));
  } catch (error) {
    return failedLane("outcome-unknown", error && error.message ? error.message : String(error));
  }
  try { await snapshotHistory(); } catch { /* the pre-retarget read is the evidence if this one fails */ }
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "outcome-unknown-source",
      sessionId: pane.backendSessionId,
      cwd: createCwd,
      providerSessionId: pane.providerSessionId,
      historyStatus: observed.historyStatus,
      availability: observed.availability || null,
      modelChoices: observed.modelChoices,
    },
  });
  let proxy = null;
  let opened = null;
  try {
    proxy = await startOutcomeUnknownProxy({
      upstream: host.url,
      evidenceDir: evidence.dir,
      match: (method, url, body) => outcomeUnknownTarget("control", method, url, body),
      onHttp: (entry) => evidence.http(entry),
    });
    const prepared = await prepareProxiedPane(ctx, proxy, pane, evidence, machine);
    if (!prepared.ok) return failedLane("outcome-unknown", prepared.reason);
    opened = await deps.openChatPage(ctx, host, pane, {
      viewport: { width: 1280, height: 800 },
      coarse: false,
      deviceToken: prepared.bearer,
      apiBase: proxy.url,
      entryUrl: proxiedEntry(proxy, pane.backendSessionId),
    });
    const onPane = await pageShowsPane(opened.page, pane.backendSessionId, ctx.args.timeoutMs, proxy);
    observed.pageUrl = onPane.pageUrl || observed.pageUrl;
    evidence.event({ at: new Date().toISOString(), frame: { type: "pane-select", ok: onPane.ok, via: onPane.via, pageUrl: onPane.pageUrl, sessionId: pane.backendSessionId } });
    if (!onPane.ok) return failedLane("outcome-unknown", paneSelectFailure(pane, prepared, onPane));
    const select = opened.page.locator('[data-testid="composer-model-control"]');
    try {
      await select.waitFor({ state: "visible", timeout: ctx.args.timeoutMs });
    } catch {
      try { observed.pageUrl = opened.page.url(); } catch { /* keep the bind url */ }
      return unmeasured("outcome-unknown", composerControlMissingEvidence(pane.backendSessionId, observed));
    }
    const choice = await opened.page.evaluate(() => {
      const node = document.querySelector('[data-testid="composer-model-control"]');
      if (!node) return null;
      const values = [...node.options].map((option) => option.value).filter((value) => value.length > 0);
      return values.find((value) => value !== node.value) || values[0] || null;
    });
    if (!choice) return unmeasured("outcome-unknown", "composer-model-control has no value to apply");
    await dispatchModelChoice(opened.page, choice);
    const notice = opened.page.locator('[data-testid="composer-control-notice"][data-status="unknown"]');
    await notice.waitFor({ state: "visible", timeout: ctx.args.timeoutMs });
    const status = await notice.getAttribute("data-status");
    if (status !== "unknown") return failedLane("outcome-unknown", "composer control notice data-status was " + status);
    const retry = opened.page.waitForRequest(
      (request) => request.method() === "POST" && request.url().includes("/control"),
      { timeout: Math.min(2000, ctx.args.timeoutMs) },
    ).then(() => true).catch(() => false);
    await dispatchModelChoice(opened.page, choice);
    if (await retry) return failedLane("outcome-unknown", "a second identical selection posted control again");
    const posts = proxy.log.filter((entry) => entry.targeted && entry.forwarded);
    assertProxyDroppedOnce(posts, "control");
    assertTargetedForwardIdentity(posts);
    try { evidence.screenshot("outcome-unknown.png", await opened.page.screenshot()); } catch { /* shot is not the receipt */ }
    return pass("outcome-unknown", { posts: posts.length, status, requestId: posts[0].requestId, proxyLog: proxy.logPath });
  } catch (error) {
    if (error && error.code === deps.blockedCode) {
      return { name: "outcome-unknown", status: "blocked", reason: error.reason, detail: error.detail };
    }
    return failedLane("outcome-unknown", error && error.message ? error.message : String(error));
  } finally {
    if (opened) {
      try { await opened.close(); } catch { /* already closed */ }
    }
    if (proxy) {
      try { await proxy.close(); } catch { /* already closed */ }
    }
    try { await clearPublishedSelection(ctx, evidence); } catch { /* a later lane must not inherit this tab */ }
  }
}

async function readAgentControlState(evidence, host, sessionId, bearer) {
  const base = String(host.url).replace(/\/+$/, "");
  const path = "/api/v1/agent-control/sessions/" + sessionId;
  const started = Date.now();
  const response = await fetch(base + path, {
    headers: { accept: "application/json", authorization: "Bearer " + bearer },
  });
  const text = await response.text();
  let json = null;
  try {
    json = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  const state = json && json.session && typeof json.session.state === "string" ? json.session.state : null;
  if (evidence && evidence.http) {
    evidence.http({
      at: new Date().toISOString(),
      ms: Date.now() - started,
      method: "GET",
      suffix: path,
      path,
      auth: "machine-bearer",
      status: response.status,
      body: text.slice(0, 512),
    });
  }
  return { status: response.status, state };
}

function unknownCreateAbsenceLane(input) {
  const classified = classifyUnknownCreateAbsence(input);
  if (classified.status === "failed") return failedLane("unknown-create", classified.reason);
  return unmeasured("unknown-create", classified.reason);
}

async function readContinuationStrip(page, timeoutMs) {
  const budget = Number(timeoutMs) > 0 ? Number(timeoutMs) : CONTINUE_STRIP_WAIT_MS;
  const started = Date.now();
  const deadline = started + budget;
  let raw = null;
  while (Date.now() <= deadline) {
    try {
      raw = await page.evaluate(() => {
        const workspace = document.querySelector('[data-testid="mobile-chat-workspace"]');
        const warning = document.querySelector('[data-testid="chat-history-warning"]');
        const nodes = [...document.querySelectorAll('[data-testid^="continue-in-new-pane-"], [data-testid^="continue-unavailable-"]')].map((node) => ({
          testId: node.getAttribute("data-testid"),
          reason: node.getAttribute("data-reason"),
        }));
        return {
          workspacePresent: Boolean(workspace),
          notice: warning ? warning.textContent || "" : "",
          nodes,
        };
      });
    } catch {
      raw = null;
    }
    const parsed = parseContinuationStripDom(raw);
    if (parsed.buttons.length > 0 || parsed.unavailable.length > 0) {
      return { raw, parsed, waitedMs: Date.now() - started };
    }
    if (Date.now() >= deadline) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return { raw, parsed: parseContinuationStripDom(raw), waitedMs: Date.now() - started };
}

async function measureUnknownCreate(ctx, host, session, evidence, deps, machine, sourceId, claimedWorkingIds) {
  const before = await sessionInventory(evidence, host, machine.bearer);
  const pane = unknownCreatePane((ctx.fixture && ctx.fixture.sessions) || [], session, claimedWorkingIds);
  if (!pane) {
    const claimed = Array.isArray(claimedWorkingIds) && claimedWorkingIds.length > 0;
    return unmeasured(
      "unknown-create",
      claimed
        ? "every claude fixture pane is already a working claim; getContinuationAffordance offers fork only for a non-working source (agentResumeAffordance.ts:295-296). Not a product fail."
        : "the fixture has no claude pane; getContinuationAffordance is not offered for an omo page",
    );
  }
  const planted = plantRetargetedClaude(ctx, pane);
  if (!planted.ok) {
    return unmeasured("unknown-create", planted.reason + "; native history is not verified, so a hidden fork button is not a product fail");
  }
  let availability = null;
  let historyStatus = null;
  let observed = { status: 0, state: null };
  try {
    const history = await readHistory(evidence, host, pane, { limit: 20 });
    availability = history.json && history.json.availability;
    historyStatus = history.status;
    observed = await readAgentControlState(evidence, host, pane.backendSessionId, machine.bearer);
  } catch (error) {
    return failedLane("unknown-create", error && error.message ? error.message : String(error));
  }
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "unknown-create-source",
      sessionId: pane.backendSessionId,
      fixtureSourceId: sourceId,
      agentState: observed.state,
      historyAvailability: availability || null,
      historyStatus,
      inventoryBefore: before.sessions.length,
    },
  });
  if (observed.state === "working" || availability !== "native") {
    return unknownCreateAbsenceLane({
      sessionId: pane.backendSessionId,
      reasons: observed.state === "working" ? ["sourceWorking"] : [],
      agentState: observed.state,
      historyAvailability: availability,
    });
  }
  const binary = ctx.candidateRaw && ctx.candidateRaw.binary && ctx.candidateRaw.binary.path;
  const idleHook = paneIdleHookLine(binary, pane.providerSessionId);
  if (!idleHook) {
    return unmeasured(
      "unknown-create",
      "candidate binary or fixture providerSessionId is absent; claude SessionStart cannot publish providerSession on the page row (agent_hook_core.rs:139 Idle). Not a product fail.",
    );
  }
  const typedIdle = await typePaneClaim(ctx, host, pane, machine.bearer, evidence, idleHook + "\r", "QA16_IDLE_SENT");
  if (!typedIdle.ok) {
    return unmeasured(
      "unknown-create",
      typedIdle.reason + "; SessionStart did not publish providerSession, so a hidden fork button is not a product fail",
    );
  }
  const published = await waitForPublishedProviderSession(
    ctx,
    host,
    pane.backendSessionId,
    pane.providerSessionId,
    machine.bearer,
    evidence,
  );
  if (!published.ok) return unmeasured("unknown-create", published.reason);
  // SessionStart publishes providerSession. Preserve and added-pane checks start after that, not before.
  // admit_agent_report compared the store cwd to this pane's process cwd (the isolated
  // .ferryx-qa-panes directory). The click does not: RemoteApp.tsx sends cwdRelative null,
  // so verify_machine_resume_cwd compares the store to the workspace root. Retarget after
  // the hook, the way a pane started in that root would have recorded it. Happy fork posts
  // continuationCwdRelative instead, so its plant stays on the pane directory.
  const createCwd = canonicalWorkspaceCwd(unknownCreateTranscriptCwd(pane));
  if (!createCwd) {
    return failedLane(
      "unknown-create",
      "workspace root is not a canonical directory; the client sends cwdRelative null so resume_cwd.rs matching_cwd compares the transcript to that root",
    );
  }
  try {
    writeFileSync(planted.file, retargetPlantedTranscriptCwd(readFileSync(planted.file, "utf8"), createCwd));
  } catch (error) {
    return failedLane("unknown-create", error && error.message ? error.message : String(error));
  }
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "unknown-create-transcript-cwd",
      cwd: createCwd,
      providerSessionId: pane.providerSessionId,
    },
  });
  const sourceBaseline = await sessionInventory(evidence, host, machine.bearer);
  let proxy = null;
  let opened = null;
  try {
    proxy = await startOutcomeUnknownProxy({
      upstream: host.url,
      evidenceDir: evidence.dir,
      match: (method, url, body) => outcomeUnknownTarget("agentContinue", method, url, body),
      onHttp: (entry) => evidence.http(entry),
    });
    const prepared = await prepareProxiedPane(ctx, proxy, pane, evidence, machine);
    if (!prepared.ok) return failedLane("unknown-create", prepared.reason);
    opened = await deps.openChatPage(ctx, host, pane, {
      viewport: { width: 1280, height: 800 },
      coarse: false,
      deviceToken: prepared.bearer,
      apiBase: proxy.url,
      entryUrl: proxiedEntry(proxy, pane.backendSessionId),
    });
    const onPane = await pageShowsPane(opened.page, pane.backendSessionId, ctx.args.timeoutMs, proxy);
    evidence.event({ at: new Date().toISOString(), frame: { type: "pane-select", ok: onPane.ok, via: onPane.via, pageUrl: onPane.pageUrl, sessionId: pane.backendSessionId } });
    if (!onPane.ok) return failedLane("unknown-create", paneSelectFailure(pane, prepared, onPane));
    const fresh = await readPublishedContinuationRow(host, pane.backendSessionId, machine.bearer, evidence);
    const controlNow = await readAgentControlState(evidence, host, pane.backendSessionId, machine.bearer);
    const pageCapability = capabilityAdvertisedFromEntries(proxy.log, CONTINUE_CAPABILITY);
    let capabilityAdvertised = pageCapability.advertised;
    if (!pageCapability.known) {
      const capabilities = await readCapabilities(evidence, host, { bearer: machine.bearer, baseUrl: proxy.url });
      capabilityAdvertised = chatGatewayCapabilityAdvertised(capabilities.json, CONTINUE_CAPABILITY);
    }
    const publishedTab = desktopContinuationTab(pane);
    evidence.event({
      at: new Date().toISOString(),
      frame: {
        type: "unknown-create-page-row",
        when: "before-button",
        ...fresh.pageRow,
        tab: fresh.tab,
        capabilityAdvertised,
        publishedAgentType: publishedTab.agentType || null,
        agentState: controlNow.state,
        historyAvailability: availability || null,
      },
    });
    const stripWaitMs = Math.min(CONTINUE_STRIP_WAIT_MS, (ctx.args && ctx.args.timeoutMs) || CONTINUE_STRIP_WAIT_MS);
    const strip = await readContinuationStrip(opened.page, stripWaitMs);
    let proxiedShot = null;
    try {
      proxiedShot = evidence.screenshot("unknown-create-proxied.png", await opened.page.screenshot());
    } catch { /* shot is not the receipt */ }
    const reasons = strip.parsed.unavailable.map((row) => row.reason).filter(Boolean);
    evidence.event({
      at: new Date().toISOString(),
      frame: {
        type: "unknown-create-strip",
        sessionId: pane.backendSessionId,
        waitedMs: strip.waitedMs,
        waitBudgetMs: stripWaitMs,
        buttons: strip.parsed.buttons,
        unavailable: strip.parsed.unavailable,
        noAgentNotice: strip.parsed.noAgentNotice,
        workspacePresent: strip.parsed.workspacePresent,
        tab: fresh.tab,
        capabilityAdvertised,
        screenshot: proxiedShot,
      },
    });
    const fork = opened.page.locator('[data-testid="continue-in-new-pane-fork"]');
    const resume = opened.page.locator('[data-testid="continue-in-new-pane-resume"]');
    const offered = opened.page.locator('[data-testid="continue-in-new-pane-fork"], [data-testid="continue-in-new-pane-resume"]');
    if (strip.parsed.buttons.length === 0) {
      return unknownCreateAbsenceLane({
        sessionId: pane.backendSessionId,
        reasons,
        agentState: controlNow.state,
        historyAvailability: availability,
        pageRow: fresh.pageRow,
        tab: fresh.tab,
        publishedTabAgentType: publishedTab.agentType || null,
        capabilityAdvertised,
        dom: strip.parsed,
      });
    }
    try {
      await offered.first().waitFor({ state: "visible", timeout: Math.min(2000, ctx.args.timeoutMs) });
    } catch (error) {
      return failedLane("unknown-create", "continue-in-new-pane was in the proxied DOM but not visible: " + (error && error.message ? error.message : String(error)));
    }
    if (!capabilityAdvertised) {
      return failedLane(
        "unknown-create",
        "continue-in-new-pane rendered but the page GET /api/v1/capabilities did not list terminalContinueV1 (parseChatGatewayIdentity reads body.capabilities; a mirror device token omits it, server.rs gateway_capability_names)",
      );
    }
    const which = (await fork.count()) > 0 ? fork : resume;
    await which.click();
    const notice = opened.page.locator('[data-testid="composer-control-notice"][data-status="unknown"]');
    await notice.waitFor({ state: "visible", timeout: ctx.args.timeoutMs });
    const status = await notice.getAttribute("data-status");
    const noticeText = await notice.innerText();
    try {
      await opened.page.waitForFunction(() => {
        const node = document.querySelector('[data-testid="continue-in-new-pane-fork"], [data-testid="continue-in-new-pane-resume"]');
        return Boolean(node && node.disabled);
      }, { timeout: ctx.args.timeoutMs });
    } catch (error) {
      return failedLane("unknown-create", "continuation button was not disabled after the unknown notice: " + (error && error.message ? error.message : String(error)));
    }
    const retry = opened.page.waitForRequest(
      (request) => request.method() === "POST" && request.url().includes("/api/v1/sessions"),
      { timeout: Math.min(2000, ctx.args.timeoutMs) },
    ).then(() => true).catch(() => false);
    await which.click({ force: true });
    if (await retry) return failedLane("unknown-create", "the client retried the create");
    const posts = proxy.log.filter((entry) => entry.targeted && entry.forwarded);
    assertProxyDroppedOnce(posts, "agentContinue create");
    assertTargetedForwardIdentity(posts);
    const created = assertUnknownCreateUpstream(posts[0]);
    assertUnknownCreateHeld({ status, text: noticeText, disabled: true, retried: false });
    const deadline = Date.now() + Math.min(5000, ctx.args.timeoutMs || 5000);
    let after = await sessionInventory(evidence, host, machine.bearer);
    while (!after.sessions.some((row) => row.id === created) && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 50));
      after = await sessionInventory(evidence, host, machine.bearer);
    }
    assertUnknownCreateReconciled(sourceBaseline.sessions, after.sessions, created, pane.backendSessionId);
    try { evidence.screenshot("unknown-create.png", await opened.page.screenshot()); } catch { /* shot is not the receipt */ }
    return pass("unknown-create", {
      posts: posts.length,
      requestId: posts[0].requestId,
      createdSessionId: created,
      added: 1,
      proxyLog: proxy.logPath,
    });
  } catch (error) {
    if (error && error.code === deps.blockedCode) {
      return { name: "unknown-create", status: "blocked", reason: error.reason, detail: error.detail };
    }
    return failedLane("unknown-create", error && error.message ? error.message : String(error));
  } finally {
    if (opened) {
      try { await opened.close(); } catch { /* already closed */ }
    }
    if (proxy) {
      try { await proxy.close(); } catch { /* already closed */ }
    }
    try { await clearPublishedSelection(ctx, evidence); } catch { /* a later lane must not inherit this tab */ }
  }
}

async function socketMessageText(data) {
  if (typeof data === "string") return data;
  if (data instanceof Uint8Array) return new TextDecoder().decode(data);
  if (data && typeof data.arrayBuffer === "function") {
    return new TextDecoder().decode(new Uint8Array(await data.arrayBuffer()));
  }
  return String(data);
}

/**
 * Type one line into the fixture pane. Local terminal sockets take raw bytes
 * (server.rs Message::Binary -> write_input) from a Control device. The
 * agent-state socket then admits the hook line (server.rs serve_agent_state_connection).
 * A local pane publishes the reported provider session (ingress_publish_identity Local).
 *
 * A Machine-scope bearer routes ws_terminal_handler (server.rs:2077-2094) into
 * machine_terminal_upgrade, which requires daemonEpoch (server.rs:2259-2261,
 * AuthQuery.daemon_epoch) and rejects this ticket-only dial with STALE_EPOCH
 * before the socket opens. It also rejects a host-scoped id (server.rs:2251-2252).
 * This pane is a local fixture session, not an account tunnel. The shipped local
 * client mints the ticket with the device token and dials
 * `/api/v1/terminal/${sessionId}?ticket=` (ui/src/lib/terminalTransport/remoteTransport.ts
 * attach). RemoteTerminal.tsx adds daemonEpoch only when isAccountSession.
 * The raw backendSessionId is the id that local branch looks up after an optional
 * host-scope unwrap (server.rs:2104-2107); the ticket audience is that same path
 * (server.rs:2057-2068). The machine bearer stays on agent-control observe:
 * authorize_agent_control refuses a mirror device (agent_control/policy.rs).
 */
async function typePaneClaim(ctx, host, session, bearer, evidence, line, sentinel, dial) {
  const marker = typeof sentinel === "string" && sentinel.length > 0 ? sentinel : "QA16_CLAIM_SENT";
  const hookKind = marker === "QA16_IDLE_SENT" ? "qa16-idle-hook" : marker === "QA16_END_SENT" ? "qa16-end-hook" : marker === "QA16_PWD_SENT" ? "qa16-pwd-probe" : "qa16-claim-hook";
  if (typeof WebSocket !== "function") return { ok: false, reason: "this runtime has no WebSocket client" };
  const sessionId = session.backendSessionId;
  if (!sessionId) return { ok: false, reason: "fixture session has no backendSessionId to type into" };
  const machineDial = Boolean(dial && dial.daemonEpoch);
  let token;
  try {
    token = machineDial ? dial.bearer : readCredentialToken(host);
  } catch (error) {
    return { ok: false, reason: "local terminal credential unreadable: " + (error && error.message ? error.message : String(error)) };
  }
  if (!token) return { ok: false, reason: "terminal socket has no bearer" };
  const base = String(host.url).replace(/\/+$/, "");
  const target = "/api/v1/terminal/" + sessionId;
  const timeoutMs = (ctx.args && ctx.args.timeoutMs) || 15000;
  const ticketStarted = Date.now();
  const ticketResponse = await fetch(base + "/api/v1/socket-ticket", {
    method: "POST",
    headers: {
      authorization: "Bearer " + token,
      "content-type": "application/json",
      accept: "application/json",
    },
    body: JSON.stringify({ target }),
  });
  const ticketText = await ticketResponse.text();
  let ticketJson = null;
  try {
    ticketJson = ticketText.length > 0 ? JSON.parse(ticketText) : null;
  } catch {
    ticketJson = null;
  }
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - ticketStarted,
    method: "POST",
    suffix: "/api/v1/socket-ticket",
    status: ticketResponse.status,
    body: ticketResponse.status === 200 ? { ticket: "[redacted]", target } : ticketText.slice(0, 180),
  });
  const ticket = ticketJson && ticketJson.ticket;
  if (ticketResponse.status !== 200 || typeof ticket !== "string" || ticket.length === 0) {
    evidence.event({
      at: new Date().toISOString(),
      frame: {
        type: "qa16-terminal-socket",
        ticketStatus: ticketResponse.status,
        closeCode: null,
        closeReason: "",
      },
    });
    return {
      ok: false,
      reason: ticketResponse.status !== 200
        ? "terminal socket-ticket HTTP " + ticketResponse.status
        : "terminal socket-ticket minted no ticket",
    };
  }
  const epochQuery = machineDial ? "&daemonEpoch=" + encodeURIComponent(String(dial.daemonEpoch)) : "";
  const ws = new WebSocket(base.replace(/^http/, "ws") + target + "?ticket=" + encodeURIComponent(ticket) + epochQuery);
  const chunks = [];
  let closeCode = null;
  let closeReason = "";
  ws.addEventListener("close", (event) => {
    closeCode = event.code;
    closeReason = String(event.reason || "");
  });
  function recordTerminalClose(kind) {
    evidence.event({
      at: new Date().toISOString(),
      frame: {
        type: "qa16-terminal-socket",
        ticketStatus: ticketResponse.status,
        closeCode,
        closeReason,
        kind,
      },
    });
    const reasonText = closeReason ? " " + closeReason : "";
    return "terminal socket failed (ticket HTTP " + ticketResponse.status + ", close " + String(closeCode) + reasonText + ")";
  }
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("terminal socket did not open")), timeoutMs);
      ws.addEventListener("open", () => {
        clearTimeout(timer);
        resolve();
      });
      ws.addEventListener("close", () => {
        if (ws.readyState === WebSocket.OPEN) return;
        clearTimeout(timer);
        reject(new Error("terminal socket failed"));
      });
    });
    const marked = new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), timeoutMs);
      ws.addEventListener("message", (event) => {
        Promise.resolve(socketMessageText(event.data)).then((text) => {
          chunks.push(text);
          if (chunks.join("").includes(marker)) {
            clearTimeout(timer);
            resolve(true);
          }
        }).catch(() => {});
      });
    });
    ws.send(new TextEncoder().encode(line));
    const saw = await marked;
    evidence.event({ at: new Date().toISOString(), frame: { type: hookKind, saw, marker } });
    if (!saw) return { ok: false, reason: "pane did not print " + marker + (dial && dial.skipClaim ? "" : " after ferryx agent-hook; the hook was not published") };
    if (dial && dial.skipClaim) return { ok: true, text: chunks.join("") };
    const published = await waitForPublishedClaim(base, sessionId, bearer, timeoutMs, evidence);
    if (!published.ok) return published;
    return { ok: true };
  } catch (error) {
    const message = String(error && error.message ? error.message : error);
    const diagnosed = recordTerminalClose(message);
    return { ok: false, reason: message === "terminal socket failed" ? diagnosed : message + "; " + diagnosed };
  } finally {
    try { ws.close(); } catch { /* already closed */ }
  }
}

/** Agent-control observe reads the hub (agent_control_service.rs provider_report). */
async function waitForPublishedClaim(base, sessionId, bearer, timeoutMs, evidence) {
  const deadline = Date.now() + timeoutMs;
  const path = "/api/v1/agent-control/sessions/" + sessionId;
  let last = "agent-control session was not read";
  while (Date.now() <= deadline) {
    const started = Date.now();
    const response = await fetch(base + path, {
      headers: { accept: "application/json", authorization: "Bearer " + bearer },
    });
    const text = await response.text();
    let json = null;
    try {
      json = text.length > 0 ? JSON.parse(text) : null;
    } catch {
      json = null;
    }
    if (evidence && evidence.http) {
      evidence.http({
        at: new Date().toISOString(),
        ms: Date.now() - started,
        method: "GET",
        suffix: path,
        path,
        auth: "machine-bearer",
        status: response.status,
        body: text.slice(0, 512),
      });
    }
    if (response.status === 403 || response.status === 401) {
      return { ok: false, reason: "agent-control observe was HTTP " + response.status + " " + text.slice(0, 180) + "; refusing to post continue before the claim is visible" };
    }
    const agent = json && json.session && json.session.agent;
    if (response.status === 200 && agent === "claude") return { ok: true };
    last = "agent-control session was HTTP " + response.status + " agent=" + String(agent);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return { ok: false, reason: "hook report was not visible on the agent-state hub before timeout (" + last + ")" };
}

async function readHostJson(evidence, host, bearer, path) {
  const base = String(host.url).replace(/\/+$/, "");
  const started = Date.now();
  const response = await fetch(base + path, {
    headers: { accept: "application/json", authorization: "Bearer " + bearer },
  });
  const text = await response.text();
  let json = null;
  try {
    json = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  if (evidence && evidence.http) {
    evidence.http({
      at: new Date().toISOString(),
      ms: Date.now() - started,
      method: "GET",
      suffix: path,
      path,
      auth: "machine-bearer",
      status: response.status,
      body: { bytes: text.length, parsed: json !== null },
    });
  }
  return { status: response.status, json };
}

/** Sessions-list row plus workspace tab, the two inputs the continuation strip actually uses. */
async function readPublishedContinuationRow(host, sessionId, bearer, evidence) {
  const sessions = await readHostJson(evidence, host, bearer, "/api/v1/sessions");
  const state = await readHostJson(evidence, host, bearer, "/api/v1/workspace/state");
  const pageRow = continuationPageRow(sessions.json, state.json, sessionId);
  const sessionRow = sessionRowForPane(sessions.json, sessionId);
  const tab = publishedWorkspaceTab(state.json, sessionId);
  if (evidence && evidence.http) {
    evidence.http({
      at: new Date().toISOString(),
      method: "GET",
      suffix: "/api/v1/sessions#source-pane",
      path: "/api/v1/sessions",
      status: sessions.status,
      body: {
        sessionId,
        sessionsStatus: sessions.status,
        stateStatus: state.status,
        pageRow,
        providerSession: sessionRow && sessionRow.providerSession ? sessionRow.providerSession : null,
        agentType: pageRow.agentType,
        running: pageRow.running,
        activityState: pageRow.activityState,
        tab,
      },
    });
  }
  if (evidence && evidence.event) {
    evidence.event({
      at: new Date().toISOString(),
      frame: {
        type: "unknown-create-page-row",
        sessionId,
        sessionsStatus: sessions.status,
        stateStatus: state.status,
        pageRow,
        tab,
        providerSession: sessionRow && sessionRow.providerSession ? sessionRow.providerSession : null,
      },
    });
  }
  return { sessionsStatus: sessions.status, stateStatus: state.status, pageRow, tab };
}

/**
 * SessionStart must land on the row the strip reads before the page opens.
 * agent_hook_core.rs:139 maps claude SessionStart to idle. unknown is not that publish.
 */
async function waitForPublishedProviderSession(ctx, host, sessionId, expectedId, bearer, evidence) {
  const timeoutMs = (ctx.args && ctx.args.timeoutMs) || 15000;
  const deadline = Date.now() + timeoutMs;
  let last = "session row was not read";
  let pageRow = continuationPageRow(null, null, sessionId);
  let agentState = null;
  while (Date.now() <= deadline) {
    const read = await readPublishedContinuationRow(host, sessionId, bearer, evidence);
    const control = await readAgentControlState(evidence, host, sessionId, bearer);
    pageRow = read.pageRow;
    agentState = control.state;
    const published = pageRow.providerSessionId === expectedId;
    const idle = agentState === "idle" || agentState === "blocked";
    const pageWorking = pageRow.activityState === "working";
    if (published && idle && !pageWorking) {
      return { ok: true, pageRow, agentState };
    }
    last = "providerSession=" + (pageRow.providerSessionId || "absent")
      + " expected=" + expectedId
      + " agent-control state=" + (agentState || "unread")
      + " activityState=" + (pageRow.activityState || "absent")
      + " sessions HTTP " + read.sessionsStatus
      + " state HTTP " + read.stateStatus;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return {
    ok: false,
    pageRow,
    agentState,
    reason: "claude SessionStart did not publish a non-working providerSession on the page row before timeout ("
      + last
      + "); agent_hook_core.rs:139 maps SessionStart to idle and prepare() inserts providerSession from session_id. A hidden fork button is not a product fail until that row is published.",
  };
}

async function deleteMachineSession(evidence, host, bearer, sessionId, daemonEpoch) {
  const body = { requestId: randomUUID(), daemonEpoch };
  const base = String(host.url).replace(/\/+$/, "");
  const started = Date.now();
  const response = await fetch(base + "/api/v1/sessions/" + encodeURIComponent(sessionId), {
    method: "DELETE",
    headers: {
      accept: "application/json",
      authorization: "Bearer " + bearer,
      "content-type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  let json = null;
  try { json = text.length > 0 ? JSON.parse(text) : null; } catch { json = null; }
  evidence.http({
    at: new Date().toISOString(),
    ms: Date.now() - started,
    method: "DELETE",
    suffix: "/api/v1/sessions/" + sessionId,
    status: response.status,
    envelope: envelopeOf(json),
    body: json || text.slice(0, 180),
  });
  return { status: response.status, json, text };
}

async function waitForExitedResumeRow(ctx, evidence, host, bearer, sessionId, providerSessionId) {
  const timeoutMs = (ctx.args && ctx.args.timeoutMs) || 15000;
  const deadline = Date.now() + timeoutMs;
  let last = "exit publication was not read";
  let row = null;
  let inventory = null;
  let tab = { found: false, running: null, providerSessionId: null };
  while (Date.now() <= deadline) {
    inventory = await sessionInventory(evidence, host, bearer);
    row = inventory.sessions.find((entry) => entry.id === sessionId) || null;
    const stateRead = await readHostJson(evidence, host, bearer, "/api/v1/workspace/state");
    tab = publishedWorkspaceTab(stateRead.json, sessionId);
    if (row && row.running === false && row.providerSessionId === providerSessionId) {
      return { ok: true, row, inventory, tab, path: "sessions-row" };
    }
    if (tab.found === true && tab.running === false && tab.providerSessionId === providerSessionId) {
      return { ok: true, row, inventory, tab, path: "desktop-tab" };
    }
    last = (row
      ? "sessions running=" + row.running + " providerSession=" + (row.providerSessionId || "absent")
      : "pane " + sessionId + " is not in GET /api/v1/sessions")
      + "; tab running=" + (tab.running == null ? "absent" : String(tab.running))
      + " tabProviderSession=" + (tab.providerSessionId || "absent");
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return { ok: false, row, inventory, tab, reason: last };
}

async function callDesktopDaemon(ctx, request) {
  const gateway = (ctx.gateways || []).find((entry) => entry && entry.controlTransport);
  if (!gateway) return { ok: false, reason: "no daemon control transport for a desktop-owned spawn" };
  let control = null;
  try {
    control = await connectDaemonControl(gateway.controlTransport, {
      timeoutMs: (ctx.args && ctx.args.timeoutMs) || 15000,
    });
    const response = await control.call(request);
    return { ok: true, response };
  } catch (error) {
    return { ok: false, reason: error && error.message ? error.message : String(error) };
  } finally {
    if (control) {
      try { control.close(); } catch { /* already closed */ }
    }
  }
}

/**
 * QA-16 resume lane. The fixture source is not stopped. The exited source is a
 * desktop-owned claude pane, spawned the way the fixture spawns (daemon UDS
 * type:spawn, not POST /api/v1/sessions). A UUID provider id is planted under
 * the cwd-encoded claude project dir. admit_agent_report compares that store
 * cwd to the pane process cwd, so the plant follows a pwd probe and a
 * SessionStart idle hook. The page click requests the workspace root, so the
 * plant is retargeted to that root only after the hook is admitted. The pane
 * is published as a desktop tab with agentType claude, then SessionEnd plus
 * process exit and daemon close. A desktop close removes the PTY
 * (pty.rs:822-824). The exited tab is published before the publication read.
 * A missing sessions row is not a wait: GET /api/v1/workspace/state carrying
 * running false and providerSession is the resume path. The page is still
 * opened so a missing publication is the T19 gap, not an unmeasured bind.
 */
async function measureResumeExited(ctx, host, session, evidence, deps, machine) {
  if (!machine || !machine.ok) return failedLane("resume", machine && machine.reason ? machine.reason : "no machine bearer");
  const root = canonicalWorkspaceCwd(unknownCreateTranscriptCwd(session));
  if (!root || !session.workspaceId) {
    return unmeasured("resume", "fixture session has no canonical workspace root; the resume click sends cwdRelative null (RemoteApp.tsx:5481-5490)");
  }
  const providerSessionId = distinctResumeProviderId([session.providerSessionId]);
  const requestId = randomUUID();
  const spawnCwd = isolatedPaneCwd(root, requestId);
  try {
    mkdirSync(spawnCwd, { recursive: true });
  } catch (error) {
    return unmeasured("resume", "desktop pane cwd was not created: " + (error && error.message ? error.message : String(error)));
  }
  const spawnRequest = desktopOwnedSpawnRequest({
    workspaceId: session.workspaceId,
    cwd: spawnCwd,
    requestId,
    cols: session.cols || 80,
    rows: session.rows || 24,
    worktree: null,
    shell: null,
  });
  const spawned = await callDesktopDaemon(ctx, spawnRequest);
  const identity = desktopSpawnIdentity(spawned.response);
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "desktop-spawn",
      ok: Boolean(identity.sessionId && identity.epoch),
      responseType: spawned.response && spawned.response.type ? spawned.response.type : null,
      sessionId: identity.sessionId,
      epoch: identity.epoch,
      cwd: spawnCwd,
      reason: spawned.ok ? null : spawned.reason,
    },
  });
  if (!spawned.ok || !identity.sessionId || !identity.epoch) {
    return unmeasured(
      "resume",
      "desktop spawn was " + (spawned.response && spawned.response.type ? spawned.response.type : spawned.reason || "unanswered")
        + " sessionId=" + (identity.sessionId || "absent")
        + " epoch=" + (identity.epoch || "absent")
        + "; the fixture path is daemon UDS type:spawn (herdr-reference-provision.mjs spawnOriginalPty). Not a pass.",
    );
  }
  const probed = await typePaneClaim(
    ctx,
    host,
    { backendSessionId: identity.sessionId },
    machine.bearer,
    evidence,
    "pwd; printf '\\nQA16_PWD_%s\\n' SENT\r",
    "QA16_PWD_SENT",
    { skipClaim: true },
  );
  const observedCwd = probed.ok ? pwdFromProbe(probed.text) : null;
  const plantedCwd = canonicalWorkspaceCwd(observedCwd) || observedCwd || spawnCwd;
  const pane = {
    registryId: "claude",
    provider: "claude",
    providerSessionId,
    backendSessionId: identity.sessionId,
    workspaceId: session.workspaceId,
    workspaceRegistration: session.workspaceRegistration || null,
    repoRoot: root,
    cwd: plantedCwd,
    cols: session.cols || 80,
    rows: session.rows || 24,
  };
  const planted = plantClaudeTranscript(ctx, pane);
  if (!planted.ok) return unmeasured("resume", planted.reason + "; native history is not planted");
  try {
    writeFileSync(planted.file, retargetPlantedTranscriptCwd(readFileSync(planted.file, "utf8"), plantedCwd));
  } catch (error) {
    return unmeasured("resume", error && error.message ? error.message : String(error));
  }
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "resume-source",
      sessionId: identity.sessionId,
      providerSessionId,
      cwd: plantedCwd,
      probedCwd: observedCwd,
      plantFile: planted.file,
      fixtureSourceId: session.backendSessionId,
      ownership: "desktop",
    },
  });
  const binary = ctx.candidateRaw && ctx.candidateRaw.binary && ctx.candidateRaw.binary.path;
  const idleHook = paneIdleHookLine(binary, providerSessionId);
  if (!idleHook) {
    return unmeasured("resume", "candidate binary is absent; claude SessionStart cannot publish providerSession");
  }
  const typedIdle = await typePaneClaim(
    ctx, host, pane, machine.bearer, evidence, idleHook + "\r", "QA16_IDLE_SENT",
  );
  if (!typedIdle.ok) {
    return unmeasured("resume", typedIdle.reason + "; SessionStart did not publish providerSession on the desktop pane");
  }
  const published = await waitForPublishedProviderSession(
    ctx, host, identity.sessionId, providerSessionId, machine.bearer, evidence,
  );
  if (!published.ok) return unmeasured("resume", published.reason);
  const capturedSessions = await readHostJson(evidence, host, machine.bearer, "/api/v1/sessions");
  const capturedRow = sessionRowForPane(capturedSessions.json, identity.sessionId);
  pane.providerSession = capturedRow && capturedRow.providerSession && typeof capturedRow.providerSession === "object"
    ? capturedRow.providerSession
    : { key: "session_id", id: providerSessionId };
  const createCwd = canonicalWorkspaceCwd(root);
  if (!createCwd) {
    return failedLane("resume", "workspace root is not a canonical directory; the resume click requests that root");
  }
  try {
    writeFileSync(planted.file, retargetPlantedTranscriptCwd(readFileSync(planted.file, "utf8"), createCwd));
  } catch (error) {
    return failedLane("resume", error && error.message ? error.message : String(error));
  }
  const endHook = paneEndHookLine(binary, providerSessionId);
  if (endHook) {
    const typedEnd = await typePaneClaim(
      ctx, host, pane, machine.bearer, evidence, endHook + "\r", "QA16_END_SENT",
    );
    evidence.event({
      at: new Date().toISOString(),
      frame: { type: "resume-session-end", ok: typedEnd.ok, reason: typedEnd.ok ? null : typedEnd.reason },
    });
  }
  const typedExit = await typePaneClaim(
    ctx,
    host,
    pane,
    machine.bearer,
    evidence,
    "printf '\\nQA16_EXIT_%s\\n' SENT; exit\r",
    "QA16_EXIT_SENT",
    { skipClaim: true },
  );
  const closeRequest = desktopPaneCloseRequest(identity.sessionId);
  const closed = await callDesktopDaemon(ctx, closeRequest);
  const closeType = closed.response && closed.response.type ? closed.response.type : null;
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "desktop-close",
      exitOk: typedExit.ok,
      exitReason: typedExit.ok ? null : typedExit.reason,
      closeType,
      closeOk: closeType === "closeOk",
      reason: closed.ok ? null : closed.reason,
    },
  });
  if (!typedExit.ok && closeType !== "closeOk") {
    return unmeasured(
      "resume",
      "desktop pane did not exit (" + (typedExit.reason || "no marker") + ") and daemon close was " + (closeType || closed.reason || "unanswered")
        + "; SessionEnd does not by itself exit the pane. Not a pass.",
    );
  }
  pane.exited = true;
  const publishedTab = await publishFixturePane(ctx, pane, evidence);
  const exited = await waitForExitedResumeRow(ctx, evidence, host, machine.bearer, identity.sessionId, providerSessionId);
  const afterExit = exited.inventory || await sessionInventory(evidence, host, machine.bearer);
  const stateRead = await readHostJson(evidence, host, machine.bearer, "/api/v1/workspace/state");
  const workspaceSession = publishedSessionRunning(stateRead.json, identity.sessionId);
  const publishedExitTab = publishedWorkspaceTab(stateRead.json, identity.sessionId);
  const publication = exitPublication({
    ownership: "desktop",
    published: publishedTab.ok === true,
    rowFound: Boolean(exited.row),
    running: exited.row ? exited.row.running : null,
    providerSessionId: exited.row ? exited.row.providerSessionId : null,
    workspaceFound: workspaceSession.found,
    workspaceRunning: workspaceSession.running,
    tabRunning: publishedExitTab.running,
    tabProviderSessionId: publishedExitTab.providerSessionId,
  });
  evidence.event({
    at: new Date().toISOString(),
    frame: {
      type: "resume-exit-publication",
      path: publication.path,
      running: publication.running,
      workspaceRunning: publication.workspaceRunning,
      citation: publication.citation,
      productGap: Boolean(publication.productGap),
      rowFound: Boolean(exited.row),
      rowRunning: exited.row ? exited.row.running : null,
      providerSessionId: exited.row ? exited.row.providerSessionId : null,
      listed: Boolean(exited.row),
      waitOk: exited.ok,
      waitReason: exited.ok ? null : exited.reason,
    },
  });
  let proxy = null;
  let opened = null;
  const posts = [];
  try {
    proxy = await startOutcomeUnknownProxy({
      upstream: host.url,
      evidenceDir: evidence.dir,
      match: () => false,
      onHttp: (entry) => evidence.http(entry),
    });
    const prepared = await prepareProxiedPane(ctx, proxy, pane, evidence, machine);
    if (!prepared.ok) {
      return failedLane("resume", (publication.productGap ? publication.productGap + " " : "") + prepared.reason);
    }
    opened = await deps.openChatPage(ctx, host, pane, {
      viewport: { width: 1280, height: 800 },
      coarse: false,
      deviceToken: prepared.bearer,
      apiBase: proxy.url,
      entryUrl: proxiedEntry(proxy, pane.backendSessionId),
    });
    opened.page.on("request", (request) => {
      if (request.method() !== "POST" || !request.url().includes("/api/v1/sessions")) return;
      let body = null;
      try { body = request.postDataJSON(); } catch { body = null; }
      posts.push({ url: request.url(), body });
    });
    const onPane = await pageShowsPane(opened.page, pane.backendSessionId, ctx.args.timeoutMs, proxy);
    evidence.event({ at: new Date().toISOString(), frame: { type: "pane-select", ok: onPane.ok, via: onPane.via, pageUrl: onPane.pageUrl, sessionId: pane.backendSessionId } });
    if (!onPane.ok) {
      const reason = paneSelectFailure(pane, prepared, onPane, "desktop");
      const gap = publication.productGap || exitedDesktopResumeGap({
        ownership: "desktop",
        published: publishedTab.ok === true,
        rowFound: Boolean(exited.row),
        running: exited.row ? exited.row.running : null,
        providerSessionId: exited.row ? exited.row.providerSessionId : null,
        tabRunning: publishedExitTab.running,
        tabProviderSessionId: publishedExitTab.providerSessionId,
      });
      return failedLane("resume", (gap ? gap + " " : "") + reason);
    }
    const stripWaitMs = Math.min(CONTINUE_STRIP_WAIT_MS, (ctx.args && ctx.args.timeoutMs) || CONTINUE_STRIP_WAIT_MS);
    const strip = await readContinuationStrip(opened.page, stripWaitMs);
    const offered = continuationStripModes(strip.parsed);
    const fresh = await readPublishedContinuationRow(host, pane.backendSessionId, machine.bearer, evidence);
    try { evidence.screenshot("resume-strip.png", await opened.page.screenshot()); } catch { /* shot is not the receipt */ }
    evidence.event({
      at: new Date().toISOString(),
      frame: {
        type: "resume-strip",
        offered,
        unavailable: strip.parsed.unavailable,
        running: fresh.pageRow.running,
        providerSessionId: fresh.pageRow.providerSessionId,
        exitPath: publication.path,
        exitCitation: publication.citation,
      },
    });
    const reasons = strip.parsed.unavailable.map((row) => row.reason).filter(Boolean);
    const sourceExited = fresh.pageRow.found === true && fresh.pageRow.running === false;
    const gap = exitedDesktopResumeGap({
      ownership: "desktop",
      published: prepared.ok === true,
      rowFound: fresh.pageRow.found === true,
      running: fresh.pageRow.running,
      providerSessionId: fresh.pageRow.providerSessionId,
    });
    try {
      assertExitedResumeStrip({
        exited: sourceExited,
        running: fresh.pageRow.running === true,
        agentType: "claude",
        offered,
      });
    } catch (error) {
      const detail = error.message
        + (reasons.length ? "; data-reason=" + reasons.join(",") : "")
        + "; offered=" + (offered.join(",") || "none")
        + "; providerSession=" + (fresh.pageRow.providerSessionId || "absent")
        + "; rowFound=" + fresh.pageRow.found
        + "; running=" + fresh.pageRow.running;
      if (gap || publication.productGap) return failedLane("resume", (gap || publication.productGap) + " " + detail);
      return failedLane("resume", detail);
    }
    const resumeButton = opened.page.locator('[data-testid="continue-in-new-pane-resume"]');
    await resumeButton.click();
    const response = await opened.page.waitForResponse(
      (entry) => entry.request().method() === "POST" && entry.url().includes("/api/v1/sessions"),
      { timeout: ctx.args.timeoutMs },
    );
    const responseBody = await response.json().catch(() => null);
    evidence.http({
      at: new Date().toISOString(),
      method: "POST",
      suffix: "/api/v1/sessions",
      status: response.status(),
      body: responseBody,
      phase: "browser-resume",
    });
    const retry = opened.page.waitForRequest(
      (request) => request.method() === "POST" && request.url().includes("/api/v1/sessions"),
      { timeout: Math.min(2000, ctx.args.timeoutMs) },
    ).then(() => true).catch(() => false);
    if (await retry) return failedLane("resume", "the client posted create again");
    const continuePosts = posts.filter((entry) => entry.body && entry.body.startup && entry.body.startup.kind === "agentContinue");
    const createPost = continuePosts[0];
    if (!createPost) return failedLane("resume", "resume click did not POST agentContinue");
    const resumeStartup = assertResumeCreatePost(createPost.body);
    if (resumeStartup.providerSession.id !== providerSessionId) {
      fail("resume POST providerSession was " + resumeStartup.providerSession.id + " not the planted id");
    }
    if (continuePosts.length !== 1) {
      return failedLane("resume", "resume create was posted more than once");
    }
    if (posts.length !== 1) {
      fail("resume click posted /api/v1/sessions " + posts.length + " times");
    }
    const code = admissionCode({ json: responseBody });
    const hostRefused = code === "AGENT_CONTINUE_UNSUPPORTED"
      || code === "AGENT_RESUME_UNSUPPORTED"
      || code === "AGENT_TRANSCRIPT_UNVERIFIED"
      || code === "AGENT_CWD_MISMATCH"
      || code === "AGENT_RESUME_INVALID";
    if (hostRefused) {
      return failedLane(
        "resume",
        "host refused " + code + " after the strip offered resume; resume_cwd.rs compares the planted workspace-root cwd.",
      );
    }
    if (!continuationCreateAccepted(response.status())) {
      return failedLane("resume", "resume create was HTTP " + response.status() + " " + (code || ""));
    }
    const createdId = sessionIdFromCreateBody(responseBody);
    if (!createdId || createdId === identity.sessionId || createdId === session.backendSessionId) {
      return failedLane("resume", "resume create did not name a new session");
    }
    const before = afterExit;
    const deadline = Date.now() + Math.min(5000, ctx.args.timeoutMs || 5000);
    let after = await sessionInventory(evidence, host, machine.bearer);
    let added = after.sessions.find((row) => row.id === createdId) || null;
    while ((!added || added.providerSessionId !== providerSessionId) && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 50));
      after = await sessionInventory(evidence, host, machine.bearer);
      added = after.sessions.find((row) => row.id === createdId) || null;
    }
    const expected = expectedContinueArgv("claude", providerSessionId, "resume");
    const observedArgv = added
      ? await observeContinueArgv(evidence, host, machine.bearer, createdId, added.pid, expected)
      : { argv: null, pid: null, source: "none", resourceStatus: null };
    if (added && added.providerSessionId !== providerSessionId) {
      return failedLane("resume", "resume create row providerSession was " + (added.providerSessionId || "absent") + " not " + providerSessionId);
    }
    if (!added || !observedArgv.argv) {
      return failedLane(
        "resume",
        "new pane " + createdId + " was created (inventoryListed=" + Boolean(added)
          + ", providerSession=" + (added ? added.providerSessionId : null)
          + ", resourcePidSource=" + observedArgv.source + "). argv of the claude resume form was not observed.",
      );
    }
    assertExitedSourceStaysAbsent(
      { sessions: before.sessions },
      { sessions: after.sessions },
      identity.sessionId,
    );
    assertOriginalSessionPreserved(
      { sourceId: session.backendSessionId, sessions: before.sessions },
      { sessions: after.sessions },
    );
    assertResumeAcceptance({
      mode: "resume",
      agentType: "claude",
      providerSessionId,
      sourceId: session.backendSessionId,
      createdSessionId: createdId,
      workspaceId: (exited.row && exited.row.workspaceId) || pane.workspaceId,
      worktree: (exited.row && exited.row.worktree) || null,
      before: before.sessions,
      after: after.sessions,
      argv: observedArgv.argv,
      posts: continuePosts.length,
    });
    try { evidence.screenshot("resume.png", await opened.page.screenshot()); } catch { /* shot is not the receipt */ }
    return pass("resume", { newSessionId: createdId, providerSessionId, pid: observedArgv.pid, argvSource: observedArgv.source });
  } catch (error) {
    if (error && error.code === deps.blockedCode) {
      return { name: "resume", status: "blocked", reason: error.reason, detail: error.detail };
    }
    return failedLane("resume", error && error.message ? error.message : String(error));
  } finally {
    if (opened) {
      try { await opened.close(); } catch { /* already closed */ }
    }
    if (proxy) {
      try { await proxy.close(); } catch { /* already closed */ }
    }
    try { await clearPublishedSelection(ctx, evidence); } catch { /* a later lane must not inherit this tab */ }
  }
}

export function createRev3Scenarios(deps) {
  return {
    "QA-12": {
      async happy(ctx) {
        const evidence = openEvidence(ctx, "QA-12", "happy");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        const capabilities = await readCapabilities(evidence, host);
        const advertised = capabilityOn(capabilities.json, DELTA_CAPABILITY);
        if (!advertised) lanes.push(failedLane("delta-capability", "get_capabilities did not advertise " + DELTA_CAPABILITY));
        const subscription = await subscribeEvents(ctx, host, evidence);
        if (!subscription.ok) lanes.push(failedLane("event-subscribe", subscription.reason));
        const planted = plantClaudeTranscript(ctx, session);
        const page = await readHistory(evidence, host, session, { limit: 50 });
        if (page.status !== 200 || !page.json || !Array.isArray(page.json.turns)) {
          lanes.push(failedLane("history-control", "history control was HTTP " + page.status));
        } else if (!advertised) {
          lanes.push(failedLane("event-delta", "delta query was not sent because " + DELTA_CAPABILITY + " is absent"));
        } else {
          const anchor = anchorFromPage(page.json);
          if (!anchor || !anchor.streamId) {
            lanes.push(failedLane("event-delta", "no stream id could be derived from the page cursor or generation (referenceHistorySync.ts:131-138)"));
          } else {
            // watch.rs Registry::refresh (228-262) re-baselines on note_read (387-405):
            // a change already covered by that read is not pending. The poller is
            // REFERENCE_HISTORY_WATCH_POLL (1s). Touch after the page read, wait for
            // the frame, then delta — a delta read first would absorb the touch.
            if (planted.ok) appendPlantedRecord(planted.file);
            if (subscription.ok) {
              const frame = planted.ok
                ? await subscription.nextMatching((entry) => entry && entry.type === HISTORY_CHANGED_TYPE, ctx.args.timeoutMs)
                : null;
              if (!frame) {
                lanes.push(planted.ok
                  ? failedLane("history-changed", "watch.rs did not emit referenceHistoryChanged after the isolated store was touched")
                  : unmeasured("history-changed", planted.reason));
              } else {
                try {
                  assertHistoryChangedFrame(frame);
                  lanes.push(pass("history-changed", {
                    changeSeq: frame.changeSeq,
                    frames: framesAfter(subscription),
                  }));
                } catch (error) {
                  lanes.push(failedLane("history-changed", error.message));
                }
              }
            }
            const delta = await readHistory(evidence, host, session, { limit: 50, after: anchor });
            const ignored = explainDeltaIgnored(delta);
            if (ignored) lanes.push(failedLane("event-delta", ignored));
            else if (delta.json && delta.json.mode === "delta") {
              try {
                assertDeltaResponse(delta.json);
                lanes.push(pass("event-delta", { mode: "delta", generation: delta.json.generation }));
              } catch (error) {
                lanes.push(failedLane("event-delta", error.message));
              }
            } else if (delta.json && delta.json.mode === "reset") {
              try {
                assertResetResponse(delta.json);
                lanes.push(pass("event-delta", { mode: "reset", reason: delta.json.reason }));
              } catch (error) {
                lanes.push(failedLane("event-delta", error.message));
              }
            } else {
              lanes.push(failedLane("event-delta", "delta query was neither delta nor reset: HTTP " + delta.status));
            }
          }
        }
        if (subscription.ok) {
          subscription.close();
          evidence.event({ at: new Date().toISOString(), frame: { type: "qa-reconnect" } });
          const reopened = await subscribeEvents(ctx, host, evidence);
          const reread = await readHistory(evidence, host, session, { limit: 50 });
          if (!reopened.ok || reread.status !== 200) lanes.push(failedLane("reconnect", "reconnect did not force one history read"));
          else lanes.push(pass("reconnect"));
          if (reopened.ok) reopened.close();
        }
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths", widths.map((row) => row.viewport.width + ":" + row.pointer)));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        return settle(deps, evidence, lanes);
      },
      async failure(ctx) {
        const evidence = openEvidence(ctx, "QA-12", "failure");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        const capabilities = await readCapabilities(evidence, host);
        const advertised = capabilityOn(capabilities.json, DELTA_CAPABILITY);
        const combined = await readHistory(evidence, host, session, {
          limit: 20,
          cursor: 0,
          cursorStream: "qa12-not-a-stream",
          after: {
            streamId: "qa12-not-a-stream",
            generation: "qa12-generation",
            anchorTurnId: "qa12-anchor",
            previousTurnFingerprint: null,
          },
        });
        const ignored = explainDeltaIgnored(combined);
        if (ignored) lanes.push(failedLane("cursor-plus-after", ignored));
        else {
          try {
            assertCursorPlusAfterRefused(combined);
            lanes.push(pass("cursor-plus-after"));
          } catch (error) {
            lanes.push(failedLane("cursor-plus-after", error.message + (explainHostGap("historyAfter", combined) ? "; " + PRODUCT_GAPS.historyAfter : "")));
          }
        }
        const foreign = await readHistory(evidence, host, session, {
          limit: 20,
          after: {
            streamId: "foreign-stream-not-this-session",
            generation: "foreign-generation",
            anchorTurnId: "foreign-anchor",
            previousTurnFingerprint: "foreign-fingerprint",
          },
        });
        if (explainDeltaIgnored(foreign)) lanes.push(failedLane("foreign-anchor", PRODUCT_GAPS.historyAfter));
        else if (foreign.json && foreign.json.mode === "delta") lanes.push(failedLane("foreign-anchor", "a foreign anchor was merged as a delta"));
        else if (foreign.json && foreign.json.mode === "reset") {
          try {
            assertResetResponse(foreign.json);
            lanes.push(pass("foreign-anchor", { reason: foreign.json.reason }));
          } catch (error) {
            lanes.push(failedLane("foreign-anchor", error.message));
          }
        } else if (foreign.status === 410 && errorCode(foreign.json) === "TARGET_EXPIRED") {
          lanes.push(pass("foreign-anchor", { code: "TARGET_EXPIRED" }));
        } else {
          lanes.push(failedLane("foreign-anchor", "foreign anchor was HTTP " + foreign.status + " " + (errorCode(foreign.json) || "")));
        }
        const rev2Requests = [];
        const rev2 = await readHistory(evidence, host, session, { limit: 20 });
        rev2Requests.push({ query: historyQuery(session, { limit: 20 }) });
        if (advertised) {
          if (rev2.status === 200 && rev2.json && Array.isArray(rev2.json.turns)) lanes.push(pass("rev2-shaped-read"));
          else lanes.push(failedLane("rev2-shaped-read", "a no-after read did not return a page"));
          lanes.push(pass("rev2-fallback-not-required", { capabilityAdvertised: true }));
        } else {
          try {
            assertRev2Fallback({ capabilityAdvertised: false, historyRequests: rev2Requests });
            if (rev2.status !== 200) lanes.push(failedLane("rev2-fallback", "polling read was HTTP " + rev2.status));
            else lanes.push(pass("rev2-fallback"));
          } catch (error) {
            lanes.push(failedLane("rev2-fallback", error.message));
          }
        }
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-failure-no-event-required" } });
        return settle(deps, evidence, lanes);
      },
    },
    "QA-13": {
      async happy(ctx) {
        const evidence = openEvidence(ctx, "QA-13", "happy");
        const { host, session } = deps.localContext(ctx);
        const lanes = await composerHappy(ctx, evidence, host, session, deps);
        return settle(deps, evidence, lanes);
      },
      async failure(ctx) {
        const evidence = openEvidence(ctx, "QA-13", "failure");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        const requestId = randomUUID();
        const rejected = await postControl(evidence, host, session, "qa-unknown-control", "nope", requestId);
        const gap = explainHostGap("controlRoute", rejected);
        if (gap) lanes.push(failedLane("control-rejected", gap));
        else if (rejected.json && rejected.json.data && rejected.json.data.outcome === "rejected") {
          try {
            assertControlOutcome(rejected, "rejected");
            lanes.push(pass("control-rejected"));
          } catch (error) {
            lanes.push(failedLane("control-rejected", error.message));
          }
        } else if (errorCode(rejected.json) === "INVALID_REQUEST" || errorCode(rejected.json) === "UNSUPPORTED") {
          lanes.push(pass("control-rejected", { code: errorCode(rejected.json) }));
        } else if (rejected.json && rejected.json.data && rejected.json.data.outcome === "unknown") {
          try {
            assertOutcomeUnknownNotRetried([{ requestId, outcome: "unknown", code: rejected.json.data.code }], requestId);
            lanes.push(pass("outcome-unknown"));
          } catch (error) {
            lanes.push(failedLane("outcome-unknown", error.message));
          }
        } else {
          lanes.push(failedLane("control-rejected", "unknown control was HTTP " + rejected.status + " " + (errorCode(rejected.json) || "")));
        }
        if (!lanes.some((lane) => lane.name === "outcome-unknown")) {
          lanes.push(await measureControlOutcomeUnknown(ctx, host, session, evidence, deps));
        }
        lanes.push(await unsupportedControls(ctx, host, session, evidence, deps));
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-13-failure" } });
        return settle(deps, evidence, lanes);
      },
    },
    "QA-14": {
      async happy(ctx) {
        const evidence = openEvidence(ctx, "QA-14", "happy");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        lanes.push(await duplicateSend(ctx, host, session, evidence, deps));
        lanes.push(await queueOnce(ctx, host, session, evidence, deps));
        const history = await readHistory(evidence, host, session, { limit: 20 });
        lanes.push(await steerHidden(ctx, host, session, evidence, deps));
        const devices = deviceLane(ctx);
        if (!devices.available) lanes.push(unmeasured("dictation-probe-success", devices.reason + "; headless probe success is not a device receipt"));
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-14-happy" } });
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        return settle(deps, evidence, lanes);
      },
      async failure(ctx) {
        const evidence = openEvidence(ctx, "QA-14", "failure");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        lanes.push(await steerHidden(ctx, host, session, evidence, deps));
        lanes.push(await dictationHidden(ctx, host, session, evidence, deps));
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-14-failure" } });
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        return settle(deps, evidence, lanes);
      },
    },
    "QA-15": {
      async happy(ctx) {
        const evidence = openEvidence(ctx, "QA-15", "happy");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        const planted = plantClaudeTranscript(ctx, session);
        if (planted.ok) lanes.push(pass("planted-transcript", { home: "isolated" }));
        else lanes.push(unmeasured("planted-transcript", planted.reason));
        const fixture = loadRecorded("qa15-rich-rows.json");
        const history = await readHistory(evidence, host, session, { limit: 50 });
        const rendered = renderedRows(history.json);
        const ids = new Set(fixture.rows.map((row) => row.callId || row.taskId));
        const overlapped = rendered.filter((row) => ids.has(row.callId) || ids.has(row.taskId));
        if (overlapped.length === 0) {
          const credential = providerCredentialLane(ctx, session);
          lanes.push(unmeasured(
            "rich-rows",
            "isolated native history has none of the recorded call ids; history.rs resolve_reference_history_stream only reads the pane's own store"
              + (credential.available ? "" : "; " + credential.reason),
          ));
        } else {
          try {
            assertRichWorkRows(fixture, overlapped);
            lanes.push(pass("rich-rows"));
          } catch (error) {
            lanes.push(failedLane("rich-rows", error.message));
          }
        }
        const refs = payloadRefs(history.json);
        const outputRef = refs.find((entry) => entry.kind === "output");
        const diffRef = refs.find((entry) => entry.kind === "diff");
        if (!outputRef || !diffRef) {
          lanes.push(unmeasured(
            "payload-success",
            "the live page published no " + (!outputRef ? "outputPayload.ref" : "diff ref")
              + "; history_claude.rs REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS and history_parse.rs REFERENCE_NATIVE_DIFF_LIMIT externalize those refs",
          ));
        } else {
          const loaded = {};
          let payloadFailed = null;
          for (const ref of [outputRef, diffRef]) {
            const payload = await traced(evidence, host, {
              method: "GET",
              sessionId: session.backendSessionId,
              suffix: "payload",
              query: historyQuery(session, { ref: ref.ref }),
            });
            const payloadGap = explainHostGap("payloadRoute", payload);
            if (payloadGap) {
              payloadFailed = payloadGap;
              break;
            }
            try {
              assertPayloadResult(payload, "success");
              assertPayloadMatchesRef(payload, ref);
              loaded[ref.kind] = payload.json.byteLength;
            } catch (error) {
              payloadFailed = error.message;
              break;
            }
          }
          if (payloadFailed) lanes.push(failedLane("payload-success", payloadFailed));
          else lanes.push(pass("payload-success", loaded));
        }
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-15-happy" } });
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        return settle(deps, evidence, lanes);
      },
      async failure(ctx) {
        const evidence = openEvidence(ctx, "QA-15", "failure");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        const expired = await traced(evidence, host, {
          method: "GET",
          sessionId: session.backendSessionId,
          suffix: "payload",
          query: historyQuery(session, { ref: "qa15-foreign-ref" }),
        });
        const gap = explainHostGap("payloadRoute", expired);
        if (gap) lanes.push(failedLane("payload-expired", gap));
        else {
          try {
            const code = assertPayloadRefusal(expired);
            lanes.push(pass("payload-expired", { code }));
          } catch (error) {
            lanes.push(failedLane("payload-expired", error.message));
          }
        }
        const history = await readHistory(evidence, host, session, { limit: 50 });
        const unverifiable = findStatus(history.json, "unverifiable");
        const assumedFailed = findRestartAssumption(history.json);
        if (assumedFailed) lanes.push(failedLane("restart-unverifiable", "unfinished work after restart was marked " + assumedFailed));
        else if (unverifiable) {
          try {
            assertRestartUnverifiable(unverifiable);
            lanes.push(pass("restart-unverifiable"));
          } catch (error) {
            lanes.push(failedLane("restart-unverifiable", error.message));
          }
        } else {
          lanes.push(unmeasured("restart-unverifiable", "the live transcript has no unfinished work from a restart"));
        }
        lanes.push(await currentCards(ctx, host, session, evidence, deps));
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-15-failure" } });
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        return settle(deps, evidence, lanes);
      },
    },
    "QA-16": {
      async happy(ctx) {
        const evidence = openEvidence(ctx, "QA-16", "happy");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        const machine = await machineScopeBearer(ctx);
        const before = machine.ok ? await sessionInventory(evidence, host, machine.bearer) : { sessions: [] };
        const sourceId = session.backendSessionId;
        const sourceRow = before.sessions.find((row) => row.id === sourceId);
        const registry = registryOf(session);
        if (!machine.ok) {
          lanes.push(failedLane("resume-or-fork", machine.reason));
          lanes.push(failedLane("original-preserved", machine.reason));
        } else if (!session.providerSessionId) {
          lanes.push(unmeasured("resume-or-fork", "the fixture session has no providerSessionId; the host must not invent one"));
        } else if (!sourceRow) {
          lanes.push(failedLane("inventory-before", "GET /api/v1/sessions did not list the source pane"));
        } else if (!sourceRow.running) {
          lanes.push(unmeasured(
            "fork",
            "fixture source is not a live idle pane (running=false); fork-native idle is not forced, and resume is the throwaway lane so this source is not resumed",
          ));
        } else if (!FORK_NATIVE_PROVIDERS.includes(registry)) {
          lanes.push(unmeasured("fork", "source is running and " + registry + " is not fork-native; resume is the throwaway lane, and this runner does not kill the fixture source"));
        } else {
          const mode = "fork";
          const planted = registry === "claude" ? plantRetargetedClaude(ctx, session) : { ok: true };
          if (registry === "claude" && !planted.ok) {
            lanes.push(unmeasured("fork", planted.reason + "; resume_cwd.rs:100-116 requires the store cwd before spawn. Not a pass."));
          } else {
            const requestId = randomUUID();
            const body = continuationBody(session, mode, requestId);
            const created = await postJson(evidence, host, "/api/v1/sessions", body, machine.bearer);
            const code = admissionCode(created);
            const gap = explainHostGap("agentContinue", created);
            const hostRefused = code === "AGENT_CONTINUE_UNSUPPORTED"
              || code === "AGENT_RESUME_UNSUPPORTED"
              || code === "AGENT_TRANSCRIPT_UNVERIFIED"
              || code === "AGENT_CWD_MISMATCH"
              || code === "AGENT_RESUME_INVALID";
            if (code === "AGENT_CONTINUE_CONFLICT") {
              lanes.push(unmeasured(mode, "owning host says the source is working; happy fork is not forced by stopping it"));
            } else if (hostRefused) {
              lanes.push(unmeasured(
                "fork",
                "host refused " + code + "; session_service.rs:2767-2777 refuses when resolve_binary finds no provider CLI, and resume_cwd.rs:100-116 refuses an unverified store cwd. Authentication is not required to measure argv. Not a pass.",
              ));
            } else if (gap) {
              lanes.push(failedLane(mode, gap));
            } else if (continuationCreateAccepted(created.status) && created.json && created.json.target && created.json.target.sessionId !== sourceId) {
              const after = await sessionInventory(evidence, host, machine.bearer);
              const createdId = created.json.target.sessionId;
              const added = after.sessions.find((row) => row.id === createdId);
              const expected = expectedContinueArgv(registry, session.providerSessionId, mode);
              const observed = added
                ? await observeContinueArgv(evidence, host, machine.bearer, createdId, added.pid, expected)
                : { argv: null, pid: null, source: "none", resourceStatus: null, argvs: [] };
              if (!added || !observed.argv) {
                lanes.push(unmeasured(
                  "fork",
                  "new pane " + createdId + " was created (inventoryListed=" + Boolean(added) + ", running=" + (added ? added.running : null)
                    + ", inventoryPid=" + (added ? added.pid : null) + ", resourcePidSource=" + observed.source
                    + ", resourceStatus=" + observed.resourceStatus + "). machine_detail (session_service.rs:1350-1360) returns the journal session stored with pid: None (session_service.rs:3032), so GET /api/v1/sessions does not name the process. GET /api/v1/resources (resources_api.rs:44-53) and /proc/<pid>/cmdline plus its child (shell.rs:688-720 runs the agent under /bin/sh, not as the pane argv) did not show resolve_agent_continue_plan's argv. A provider that exits before that read leaves no argv. Not a pass.",
                ));
              } else {
                try {
                  assertContinueAcceptance({
                    mode,
                    agentType: registry,
                    providerSessionId: session.providerSessionId,
                    sourceId,
                    createdSessionId: createdId,
                    workspaceId: sourceRow.workspaceId,
                    worktree: sourceRow.worktree || null,
                    before: before.sessions,
                    after: after.sessions,
                    argv: observed.argv,
                  });
                  lanes.push(pass(mode, { newSessionId: createdId, pid: observed.pid, argvSource: observed.source }));
                } catch (error) {
                  lanes.push(failedLane(mode, error.message));
                }
              }
            } else {
              lanes.push(failedLane(mode, "continuation create was HTTP " + created.status + " " + (code || "")));
            }
          }
        }
        lanes.push(await measureResumeExited(ctx, host, session, evidence, deps, machine));
        lanes.push(localDesktopHappyLane());
        if (machine.ok && before.sessions.length > 0 && sourceRow) {
          const after = await sessionInventory(evidence, host, machine.bearer);
          try {
            assertOriginalSessionPreserved({ sourceId, sessions: before.sessions }, { sessions: after.sessions });
            lanes.push(pass("original-preserved"));
          } catch (error) {
            lanes.push(failedLane("original-preserved", error.message));
          }
        }
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-16-happy", inventoryBefore: before.sessions.length } });
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        return settle(deps, evidence, lanes);
      },
      async failure(ctx) {
        const evidence = openEvidence(ctx, "QA-16", "failure");
        const { host, session } = deps.localContext(ctx);
        const lanes = [];
        const machine = await machineScopeBearer(ctx);
        const before = machine.ok ? await sessionInventory(evidence, host, machine.bearer) : { sessions: [] };
        const sourceId = session.backendSessionId;
        // The claim hook publishes providerSessionId after `before`. original-preserved
        // and the spawn check use the post-claim inventory once the hook is confirmed.
        let claimBaseline = before;
        const requestId = randomUUID();
        let body;
        try {
          body = continuationBody(session, "resume", requestId);
        } catch (error) {
          lanes.push(failedLane("startup-shape", error.message));
          body = null;
        }
        if (!machine.ok) {
          lanes.push(failedLane("working-source", machine.reason));
          lanes.push(failedLane("arbitrary-argv-refused", machine.reason));
          lanes.push(failedLane("original-preserved", machine.reason));
        } else if (body) {
          // Before the claim hook. PreToolUse marks this pane working, and
          // getContinuationAffordance then hides fork (agentResumeAffordance.ts:295-296).
          lanes.push(await measureUnknownCreate(ctx, host, session, evidence, deps, machine, sourceId, []));
          const planted = plantClaudeTranscript(ctx, session);
          let code = null;
          if (!planted.ok) {
            lanes.push(unmeasured("working-source", planted.reason));
            lanes.push(unmeasured("duplicate-claim", planted.reason));
          } else {
            const root = plantCwdOf(session);
            const binary = ctx.candidateRaw && ctx.candidateRaw.binary && ctx.candidateRaw.binary.path;
            const hook = paneClaimHookLine(binary, session.providerSessionId);
            if (!root) {
              lanes.push(unmeasured("working-source", "fixture session has no workspaceRegistration.repoRoot; the planted cwd cannot be retargeted"));
              lanes.push(unmeasured("duplicate-claim", "fixture session has no workspaceRegistration.repoRoot"));
            } else if (!hook) {
              lanes.push(unmeasured("working-source", "candidate binary path is absent; ferryx agent-hook cannot be typed into the pane"));
              lanes.push(unmeasured("duplicate-claim", "candidate binary path is absent; ferryx agent-hook cannot be typed into the pane"));
            } else {
              let retargeted = false;
              try {
                writeFileSync(planted.file, retargetPlantedTranscriptCwd(readFileSync(planted.file, "utf8"), root));
                retargeted = true;
              } catch (error) {
                lanes.push(failedLane("working-source", error.message));
                lanes.push(failedLane("duplicate-claim", error.message));
              }
              const typed = retargeted
                ? await typePaneClaim(ctx, host, session, machine.bearer, evidence, hook + "\r")
                : { ok: false, reason: null };
              if (retargeted && !typed.ok) {
                lanes.push(failedLane("working-source", typed.reason));
                lanes.push(failedLane("duplicate-claim", typed.reason));
              } else if (retargeted) {
                claimBaseline = await sessionInventory(evidence, host, machine.bearer);
                const refused = await postJson(evidence, host, "/api/v1/sessions", body, machine.bearer);
                code = admissionCode(refused);
                const afterProbe = await sessionInventory(evidence, host, machine.bearer);
                const spawned = continuationSpawnedPane({ sessions: claimBaseline.sessions }, afterProbe);
                const probe = isUnregistered(refused)
                  ? { status: "failed", reason: explainHostGap("agentContinue", refused) }
                  : claimedContinueLane(code, spawned);
                if (probe.status === "pass") {
                  try {
                    assertWorkingSourceBlocked({ created: false, code, sourceStopped: false });
                    lanes.push(pass("working-source"));
                  } catch (error) {
                    lanes.push(failedLane("working-source", error.message));
                  }
                } else {
                  lanes.push(failedLane("working-source", probe.reason));
                }
                if (probe.status === "pass") {
                  const claimBody = continuationBody(session, "resume", randomUUID());
                  const again = await postJson(evidence, host, "/api/v1/sessions", claimBody, machine.bearer);
                  const againCode = admissionCode(again);
                  const againAfter = await sessionInventory(evidence, host, machine.bearer);
                  const againSpawned = continuationSpawnedPane({ sessions: claimBaseline.sessions }, againAfter);
                  const claim = isUnregistered(again)
                    ? { status: "failed", reason: explainHostGap("agentContinue", again) }
                    : claimedContinueLane(againCode, againSpawned);
                  if (claim.status === "pass") {
                    try {
                      assertDuplicateClaimConflict({ created: false, code: againCode });
                      lanes.push(pass("duplicate-claim"));
                    } catch (error) {
                      lanes.push(failedLane("duplicate-claim", error.message));
                    }
                  } else {
                    lanes.push(failedLane("duplicate-claim", claim.reason));
                  }
                } else {
                  lanes.push(failedLane("duplicate-claim", "working-source was not AGENT_CONTINUE_CONFLICT, so no duplicate claim was posted"));
                }
              }
            }
          }
        }
        if (machine.ok) {
        const argv = await postJson(evidence, host, "/api/v1/sessions", {
          requestId: randomUUID(),
          workspaceId: session.workspaceId,
          worktree: null,
          cols: session.cols || 80,
          rows: session.rows || 24,
          inheritFromSessionId: null,
          cwdRelative: null,
          startup: {
            kind: "agentContinue",
            agentType: "claude",
            providerSession: { key: "session_id", id: "qa16" },
            mode: "fork",
            argv: ["--resume", "client-supplied"],
          },
        }, machine.bearer);
        if (argv.status === 400 && admissionCode(argv) === "INVALID_REQUEST") lanes.push(pass("arbitrary-argv-refused"));
        else lanes.push(failedLane("arbitrary-argv-refused", "client argv was HTTP " + argv.status + " " + (admissionCode(argv) || "")));
        const after = await sessionInventory(evidence, host, machine.bearer);
        const preservedFrom = preservedBaseline(before, claimBaseline, sourceId);
        if (preservedFrom.sessions.some((row) => row.id === sourceId)) {
          try {
            assertOriginalSessionPreserved({ sourceId, sessions: preservedFrom.sessions }, { sessions: after.sessions });
            lanes.push(pass("original-preserved"));
          } catch (error) {
            lanes.push(failedLane("original-preserved", error.message));
          }
        } else {
          lanes.push(failedLane("original-preserved", "source session was not in the inventory before the probe"));
        }
        }
        lanes.push(localDesktopHappyLane());
        evidence.event({ at: new Date().toISOString(), frame: { type: "qa-16-failure" } });
        try {
          const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
          assertWidthExercise(widths);
          lanes.push(pass("widths"));
        } catch (error) {
          if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
          else lanes.push(failedLane("widths", error.message));
        }
        return settle(deps, evidence, lanes);
      },
    },
  };
}

function framesAfter(subscription) {
  return subscription.frames.filter((frame) => frame && frame.type === HISTORY_CHANGED_TYPE).length;
}

async function composerHappy(ctx, evidence, host, session, deps) {
  const lanes = [];
  const credential = providerCredentialLane(ctx, session);
  const history = await readHistory(evidence, host, session, { limit: 20 });
  const composer = history.json && history.json.composer;
  if (!credential.supported) {
    lanes.push(await unsupportedControls(ctx, host, session, evidence, deps));
    lanes.push(unmeasured("model-apply", "this session is not claude, codex, or grok"));
  } else if (!credential.available) {
    lanes.push(unmeasured(
      "model-apply",
      credential.reason + "; control.rs mapped_command types /model into the pane and confirm_delta (control.rs:842-873) marks applied only from the screen delta. A shell echo of that command would look applied, so this lane stays unmeasured without a live provider. Not a pass.",
    ));
  } else if (!composer) {
    lanes.push(failedLane("model-apply", "composer document omitted"));
  } else {
    try {
      assertComposerCapabilities(composer);
    } catch (error) {
      lanes.push(failedLane("composer-shape", error.message));
    }
    if (!composer.models || !composer.models.choices || composer.models.choices.length === 0) {
      lanes.push(unmeasured("model-apply", "discovery published no model choices"));
    } else {
      const choice = composer.models.choices[0];
      const requestId = randomUUID();
      const applied = await postControl(evidence, host, session, "model", choice.id, requestId);
      const gap = explainHostGap("controlRoute", applied);
      if (gap) lanes.push(failedLane("model-apply", gap));
      else {
        try {
          assertControlOutcome(applied, "applied");
          lanes.push(pass("model-apply", { valueId: choice.id }));
        } catch (error) {
          lanes.push(failedLane("model-apply", error.message));
        }
      }
    }
  }
  try {
    const widths = await captureWidths(ctx, host, session, evidence, deps.openChatPage);
    assertWidthExercise(widths);
    lanes.push(pass("widths"));
  } catch (error) {
    if (error && error.code === deps.blockedCode) lanes.push({ name: "widths", status: "blocked", reason: error.reason, detail: error.detail });
    else lanes.push(failedLane("widths", error.message));
  }
  evidence.event({ at: new Date().toISOString(), frame: { type: "qa-13-happy" } });
  return lanes;
}

async function unsupportedControls(ctx, host, session, evidence, deps) {
  const credential = providerCredentialLane(ctx, session);
  if (credential.supported) {
    const other = (ctx.fixture.sessions || []).find((candidate) => !DISCOVERY_PROVIDERS.includes(registryOf(candidate)));
    if (!other) return unmeasured("unsupported-provider", "the fixture has no unsupported-provider session");
    return unsupportedOn(ctx, host, other, evidence, deps);
  }
  return unsupportedOn(ctx, host, session, evidence, deps);
}

async function unsupportedOn(ctx, host, session, evidence, deps) {
  const history = await readHistory(evidence, host, session, { limit: 10 });
  const composer = history.json && history.json.composer;
  let modelControlVisible = false;
  let optionControlVisible = false;
  let textChatVisible = false;
  try {
    await clearPublishedSelection(ctx, evidence);
    const published = await publishFixturePane(ctx, session, evidence);
    const entryUrl = String(host.url).replace(/\/+$/, "") + "/#session=" + encodeURIComponent(session.backendSessionId);
    const opened = await deps.openChatPage(ctx, host, session, {
      viewport: { width: 390, height: 844 },
      coarse: true,
      entryUrl,
    });
    try {
      const bound = await opened.page.waitForRequest(
        (request) => request.url().includes("/api/v1/reference-chat/" + session.backendSessionId + "/"),
        { timeout: ctx.args.timeoutMs },
      ).then(() => true).catch(() => false);
      evidence.event({
        at: new Date().toISOString(),
        frame: {
          type: "unsupported-provider-bind",
          sessionId: session.backendSessionId,
          registryId: registryOf(session),
          published: published.ok,
          bound,
        },
      });
      if (!bound) {
        try { evidence.screenshot("unsupported-provider.png", await opened.page.screenshot()); } catch { /* shot is not the receipt */ }
        return unmeasured(
          "unsupported-provider",
          "page did not request reference-chat/" + session.backendSessionId
            + " after clearing the prior desktop selection (publish=" + (published.ok ? "ok" : published.reason)
            + "). QA-13 outcome-unknown publishes a claude tab; measuring that tab is not this session. Not a product fail.",
        );
      }
      modelControlVisible = await countTestId(opened.page, "composer-model-control") > 0;
      optionControlVisible = await countTestId(opened.page, "composer-effort-control") > 0;
      textChatVisible = await countTestId(opened.page, "chat-composer-textarea") > 0;
      evidence.screenshot("unsupported-provider.png", await opened.page.screenshot());
    } finally {
      await opened.close();
    }
  } catch (error) {
    if (error && error.code === deps.blockedCode) return { name: "unsupported-provider", status: "blocked", reason: error.reason, detail: error.detail };
    return failedLane("unsupported-provider", error.message);
  } finally {
    try { await clearPublishedSelection(ctx, evidence); } catch { /* widths must not inherit this tab */ }
  }
  try {
    assertUnsupportedProviderHidesControls({
      supported: false,
      composerModelsPresent: Boolean(composer && composer.models),
      modelControlVisible,
      optionControlVisible,
      textChatVisible,
    });
    return pass("unsupported-provider");
  } catch (error) {
    return failedLane("unsupported-provider", error.message);
  }
}

async function duplicateSend(ctx, host, session, evidence, deps) {
  let opened;
  try {
    opened = await deps.openChatPage(ctx, host, session, { viewport: { width: 1280, height: 800 }, coarse: false });
  } catch (error) {
    if (error && error.code === deps.blockedCode) return { name: "duplicate-send", status: "blocked", reason: error.reason, detail: error.detail };
    return failedLane("duplicate-send", error.message);
  }
  const posts = [];
  try {
    opened.page.on("request", (request) => {
      if (request.method() !== "POST" || !request.url().includes("/submit")) return;
      let body = null;
      try {
        body = JSON.parse(request.postData() || "null");
      } catch {
        body = null;
      }
      const entry = { requestId: body && body.requestId, url: request.url() };
      posts.push(entry);
      evidence.http({ at: new Date().toISOString(), method: "POST", suffix: "submit", phase: "browser", requestId: entry.requestId });
    });
    await opened.page.fill('[data-testid="chat-composer-textarea"]', "HERDR_QA14_DUP");
    const pendingSubmit = opened.page.waitForRequest(
      (request) => request.method() === "POST" && request.url().includes("/submit"),
      { timeout: ctx.args.timeoutMs },
    );
    await opened.page.evaluate(() => {
      const button = document.querySelector('[data-testid="send-button"]');
      if (!button) throw new Error("send-button missing");
      button.click();
      button.click();
    });
    try {
      await pendingSubmit;
    } catch {
      if (posts.length === 0) {
        const noAgent = await opened.page.locator("text=No agent is running in this terminal").count();
        if (noAgent > 0) {
          return {
            name: "duplicate-send",
            status: "unmeasured",
            reason: "the page shows no agent reader, so Send was not posted; this lane needs an agent-published pane and is not a pass",
          };
        }
        return failedLane("duplicate-send", "no submit post was observed");
      }
    }
    evidence.screenshot("duplicate-send.png", await opened.page.screenshot());
    try {
      assertDuplicateSendOneRequest(posts);
      return pass("duplicate-send", { requestId: posts[0].requestId });
    } catch (error) {
      return failedLane("duplicate-send", error.message + (posts.length > 1 ? "; " + "follow-up not advertised on this page" : ""));
    }
  } catch (error) {
    return failedLane("duplicate-send", error.message);
  } finally {
    await opened.close();
  }
}

async function queueOnce(ctx, host, session, evidence, deps) {
  const pane = claudeFixturePane(ctx.fixture && ctx.fixture.sessions, session) || session;
  if (registryOf(pane) === "claude") plantRetargetedClaude(ctx, pane);
  const history = await readHistory(evidence, host, pane, { limit: 20 });
  const queue = Boolean(history.json && history.json.composer && history.json.composer.followUp && history.json.composer.followUp.queue);
  const credential = providerCredentialLane(ctx, session);
  if (!queue) {
    return unmeasured(
      "queue-once",
      "followUp.queue is not advertised on " + registryOf(pane) + "; control.rs follow_up_for sets queue for claude only inside composer_from_models, and attach_composer omits the document when the live probe fails (control.rs:334-347, server.rs:7381-7407). A planted transcript does not advertise queue. Not a pass.",
    );
  }
  if (!credential.available) return unmeasured("queue-once", credential.reason + "; a queued row still dispatches into a live provider turn");
  const screen = await traced(evidence, host, {
    method: "GET",
    sessionId: pane.backendSessionId,
    suffix: "screen",
    query: historyQuery(pane, {}),
  });
  const working = Boolean(history.json && history.json.tailOpen) || Boolean(screen.json && screen.json.gap === false && /running|esc to interrupt/i.test(String(screen.json.text || "")));
  if (!working) return unmeasured("queue-once", "the owning target was not observed working, so a queued follow-up cannot be admitted");
  let opened;
  try {
    opened = await deps.openChatPage(ctx, host, pane, { viewport: { width: 390, height: 844 }, coarse: true });
  } catch (error) {
    if (error && error.code === deps.blockedCode) return { name: "queue-once", status: "blocked", reason: error.reason, detail: error.detail };
    return failedLane("queue-once", error.message);
  }
  try {
    const queued = await countTestId(opened.page, "queue-follow-up-button");
    evidence.screenshot("queue.png", await opened.page.screenshot());
    if (queued < 1) return unmeasured("queue-once", "followUp.queue is advertised but the composer has no queue row; " + "follow-up not advertised on this page");
    return failedLane("queue-once", "a queue row is visible but the runner has no delivery receipt proving it was sent once");
  } finally {
    await opened.close();
  }
}

async function steerVisibility(ctx, host, session, evidence, deps, advertisedExpected) {
  const history = await readHistory(evidence, host, session, { limit: 10 });
  const advertised = Boolean(history.json && history.json.composer && history.json.composer.followUp && history.json.composer.followUp.steer);
  if (advertisedExpected && !advertised) return unmeasured("steer", "steer is not advertised");
  let opened;
  try {
    opened = await deps.openChatPage(ctx, host, session, { viewport: { width: 390, height: 844 }, coarse: true });
  } catch (error) {
    if (error && error.code === deps.blockedCode) return { name: "steer", status: "blocked", reason: error.reason, detail: error.detail };
    return failedLane("steer", error.message);
  }
  try {
    const visible = await countTestId(opened.page, "steer-button") > 0;
    evidence.screenshot("steer.png", await opened.page.screenshot());
    assertSteerHiddenUnlessAdvertised({ advertised, visible });
    return pass("steer", { advertised, visible });
  } catch (error) {
    return failedLane("steer", error.message);
  } finally {
    await opened.close();
  }
}

async function dictationHidden(ctx, host, session, evidence, deps) {
  let opened;
  try {
    opened = await deps.openChatPage(ctx, host, session, { viewport: { width: 390, height: 844 }, coarse: true });
  } catch (error) {
    if (error && error.code === deps.blockedCode) return { name: "dictation", status: "blocked", reason: error.reason, detail: error.detail };
    return failedLane("dictation", error.message);
  }
  try {
    const probe = await opened.page.evaluate(() => {
      const Ctor = window.SpeechRecognition || window.webkitSpeechRecognition;
      return { probeOk: typeof Ctor === "function" };
    });
    if (probe.probeOk) return unmeasured("dictation", "the shell probe succeeded; the hidden-mic lane needs a failed probe");
    const micVisible = await countTestId(opened.page, "dictation-mic-button") > 0;
    evidence.screenshot("dictation-hidden.png", await opened.page.screenshot());
    assertDictationHiddenWhenProbeFails({ probeOk: false, micVisible, autoSent: false });
    return pass("dictation", { probeOk: false, micVisible });
  } catch (error) {
    return failedLane("dictation", error.message + "; " + "dictation probe");
  } finally {
    await opened.close();
  }
}


const PLANT_SESSION = "4f0a1c2e-9d3b-4a7c-8e11-2b6f7c0d9a10";

function samePath(left, right) {
  try {
    return realpathSync(left) === realpathSync(right);
  } catch {
    return resolve(left) === resolve(right);
  }
}

function isolatedHome(ctx) {
  const real = process.env.HOME || process.env.USERPROFILE || "";
  const candidates = [];
  for (const gateway of ctx.gateways || []) {
    if (gateway.profile && gateway.profile.paths && gateway.profile.paths.home) {
      candidates.push(gateway.profile.paths.home);
    }
    if (gateway.ownership && gateway.ownership.profileRoot) {
      candidates.push(join(gateway.ownership.profileRoot, "home"));
    }
  }
  for (const host of ctx.fixtureRaw.hosts || []) {
    if (host.ownedGateway && host.ownedGateway.profileRoot) {
      candidates.push(join(host.ownedGateway.profileRoot, "home"));
    }
  }
  for (const home of candidates) {
    if (!home || !existsSync(home)) continue;
    if (real && samePath(home, real)) continue;
    const root = resolve(home, "..");
    if (samePath(home, root)) continue;
    if (resolve(home) !== join(root, "home") && resolve(home) !== resolve(join(root, "home"))) continue;
    return home;
  }
  return null;
}

export function plantClaudeTranscript(ctx, session) {
  const home = isolatedHome(ctx);
  if (!home) return { ok: false, reason: "no isolated profile home; refusing to write outside that profile" };
  if (registryOf(session) !== "claude") {
    return { ok: false, reason: "plant writes only the claude exact store; this pane is " + registryOf(session) };
  }
  const providerId = String(session.providerSessionId || "");
  if (!/^[A-Za-z0-9._-]{8,80}$/.test(providerId)) {
    return { ok: false, reason: "session has no plantable providerSessionId" };
  }
  const templatePath = join(FIXTURE_DIR, "qa15-claude.jsonl");
  if (!existsSync(templatePath)) return { ok: false, reason: "qa15-claude.jsonl is missing" };
  const slug = claudeProjectSlug(plantCwdOf(session)) || "-qa15";
  const dir = join(home, ".claude", "projects", slug);
  const file = join(dir, providerId + ".jsonl");
  if (!file.startsWith(resolve(home) + "/") && !file.startsWith(home + "/")) {
    return { ok: false, reason: "plant path escaped the isolated home" };
  }
  mkdirSync(dir, { recursive: true });
  const body = readFileSync(templatePath, "utf8").split(PLANT_SESSION).join(providerId);
  writeFileSync(file, body.endsWith("\n") ? body : body + "\n");
  return { ok: true, file, reason: null };
}

/**
 * One new JSONL record copied from the planted fixture, with a new uuid.
 * A bare newline is not a turn, so the fenced delta after the frame would have
 * nothing to return. watch.rs emits only after FileStamp len/mtime moves.
 */
function appendPlantedRecord(file) {
  if (!file || !existsSync(file)) return;
  const lines = readFileSync(file, "utf8").split("\n").filter((line) => line.trim().length > 0);
  if (lines.length === 0) return;
  const record = JSON.parse(lines[lines.length - 1]);
  if (!record || typeof record !== "object" || Array.isArray(record)) return;
  record.uuid = randomUUID();
  if (typeof record.timestamp === "string") record.timestamp = new Date().toISOString();
  if (record.message && typeof record.message === "object" && typeof record.message.id === "string") {
    record.message.id = "msg_" + String(record.uuid).replace(/-/g, "").slice(0, 12);
  }
  appendFileSync(file, JSON.stringify(record) + "\n");
}

async function postControl(evidence, host, session, controlId, valueId, requestId) {
  const body = {
    requestId,
    target: targetFor(session),
    registryId: session.registryId || registryOf(session),
    controlId,
    valueId,
  };
  if (session.providerSessionId) body.providerSessionId = session.providerSessionId;
  return traced(evidence, host, {
    method: "POST",
    sessionId: session.backendSessionId,
    suffix: "control",
    body,
  });
}

async function steerHidden(ctx, host, session, evidence, deps) {
  const history = await readHistory(evidence, host, session, { limit: 10 });
  const advertised = Boolean(history.json && history.json.composer && history.json.composer.followUp && history.json.composer.followUp.steer);
  if (advertised) {
    return failedLane("steer", "control.rs follow_up_for advertised steer; it is false for every provider");
  }
  let opened;
  try {
    opened = await deps.openChatPage(ctx, host, session, { viewport: { width: 390, height: 844 }, coarse: true });
  } catch (error) {
    if (error && error.code === deps.blockedCode) return { name: "steer", status: "blocked", reason: error.reason, detail: error.detail };
    return failedLane("steer", error.message);
  }
  try {
    const visible = await countTestId(opened.page, "steer-button") > 0;
    evidence.screenshot("steer.png", await opened.page.screenshot());
    assertSteerHiddenUnlessAdvertised({ advertised: false, visible });
    return pass("steer", { advertised: false, visible });
  } catch (error) {
    return failedLane("steer", error.message);
  } finally {
    await opened.close();
  }
}

function renderedRows(page) {
  const rows = [];
  const turns = page && Array.isArray(page.turns) ? page.turns : [];
  for (const turn of turns) {
    for (const part of turn.parts || []) {
      if (!part || typeof part !== "object") continue;
      if (part.callId || part.taskId) {
        rows.push({
          callId: part.callId || null,
          taskId: part.taskId || null,
          status: part.status || null,
          usage: part.usage,
        });
      }
    }
  }
  return rows;
}

function firstRef(page) {
  const turns = page && Array.isArray(page.turns) ? page.turns : [];
  for (const turn of turns) {
    for (const part of turn.parts || []) {
      if (!part || typeof part !== "object") continue;
      if (part.outputPayload && typeof part.outputPayload.ref === "string") return part.outputPayload.ref;
      if (part.patch && typeof part.patch.ref === "string") return part.patch.ref;
      if (typeof part.outputRef === "string") return part.outputRef;
    }
  }
  return null;
}

function findStatus(page, status) {
  const rows = renderedRows(page);
  const found = rows.find((row) => row.status === status);
  return found ? found.status : null;
}

function findRestartAssumption(page) {
  const rows = renderedRows(page);
  const found = rows.find((row) => row.status === "failed" && row.taskId && String(row.taskId).includes("restart"));
  return found ? found.status : null;
}

/** A fixture pane whose registry id has no native reader. Claude, omo, and codex are supported (types.rs:203-206). */
export function unsupportedRichSession(ctx, session) {
  const sessions = (ctx && ctx.fixture && ctx.fixture.sessions) || [];
  const found = sessions.find((candidate) => !richRowProviderSupported(registryOf(candidate)));
  if (found) return found;
  if (session && !richRowProviderSupported(registryOf(session))) return session;
  return null;
}

async function currentCards(ctx, host, session, evidence, deps) {
  const target = unsupportedRichSession(ctx, session);
  if (!target) {
    return failedLane("current-cards", "fixture has no pane outside the native-reader set (types.rs:201-220); claude, omo, and codex are supported");
  }
  let opened;
  try {
    opened = await deps.openChatPage(ctx, host, target, { viewport: { width: 1280, height: 800 }, coarse: false });
  } catch (error) {
    if (error && error.code === deps.blockedCode) return { name: "current-cards", status: "blocked", reason: error.reason, detail: error.detail };
    return failedLane("current-cards", error.message);
  }
  try {
    const readCards = async () => opened.page.evaluate(() => (
      [...document.querySelectorAll('[data-testid="reference-tool-output"], [data-testid="reference-task-result"], [data-testid="work-row"]')]
        .map((node) => node.getAttribute("data-testid") + ":" + (node.textContent || "").slice(0, 80))
    ));
    const before = { supported: false, cards: await readCards() };
    evidence.screenshot("cards-before.png", await opened.page.screenshot());
    await readHistory(evidence, host, target, { limit: 20 });
    await opened.page.reload({ waitUntil: "domcontentloaded" });
    const after = { cards: await readCards(), syntheticRich: false };
    evidence.screenshot("cards-after.png", await opened.page.screenshot());
    assertUnsupportedKeepsCurrentCards(before, after);
    return pass("current-cards");
  } catch (error) {
    return failedLane("current-cards", error.message);
  } finally {
    await opened.close();
  }
}

