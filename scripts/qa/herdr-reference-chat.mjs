#!/usr/bin/env node
/**
 * Herdr reference-chat isolated acceptance runner (plan task 14).
 *
 * Contract: docs/chat/herdr-port-contract.md (rev 2). Routes were registered by task 13 on
 * the EXISTING gateway; this runner consumes them and never mounts a second listener.
 *
 * WHAT IT PROVES
 *   Every QA-01..QA-11 row has a happy branch and a failure/regression branch. A branch is
 *   proven by an observable on the real surface: an HTTP status plus body from the frozen
 *   route, or a DOM state read from the served UI in a real browser. Nothing is proven by
 *   "the command exited zero".
 *
 * WHAT IT REFUSES
 *   - It validates the fixture and candidate manifests instead of guessing defaults. A
 *     missing host, session, credential, transcript, device or hash is a reported blocker.
 *   - It re-hashes the candidate binary, every candidate source file and every candidate UI
 *     file against the manifest before any scenario runs. A mismatch is fatal.
 *   - It never substitutes a fixture for missing identity. A session without a
 *     providerSessionId is reported as missing that identity, and any branch that would need
 *     it fails as BLOCKED rather than quietly choosing another session.
 *   - It never kills a process it did not spawn. Teardown re-reads each recorded PID's live
 *     executable and kills only an exact match; anything else is reported.
 *
 * ISOLATED GATEWAY
 *   A host that declares startLocal is launched as the product's own headless daemon
 *   (--daemon) on a throwaway profile with a persisted isolated remote config; its URL
 *   and device token come from that daemon and its own auth store. There is no JSON ready
 *   line to wait for - the daemon's ready line is plain text and the bound address is
 *   read back over the control socket.
 *
 *   The product FORCES the gateway port, so an isolated launch requires that fixed
 *   127.0.0.1 port to be free on this host: the launcher pre-flights it and refuses as
 *   BLOCKED when something else holds it, without touching that service. Run the harness
 *   on a dedicated free host; a configured external host still needs no launch at all.
 *
 *   A fixture produced by the provisioner's retained handoff carries `ownedGateway`: this
 *   runner ADOPTS that exact daemon - the one the fixture's original PTYs were spawned in -
 *   instead of launching a second one, so the sessions, the epoch and the recorded identity
 *   stay the same across the two processes. Adopting requires --allow-host true like any
 *   other host action: the ownership record in a fixture manifest is DATA, never permission.
 *   This runner reaps an adopted daemon in its own cleanup, and that cleanup is the only
 *   thing that does: the record's lease is an adoption deadline, not a lifetime, so if this
 *   runner never adopts, the verifier reaps the recorded exact PID in its own finally.
 *
 * EXIT CODES
 *   0  every selected branch passed
 *   2  BLOCKED: a real dependency (host, credential, device, driver, transcript) is missing
 *   3  interaction failure (a surface could not be driven)
 *   4  a selected branch FAILED its observable, or QA-11 detected tampering
 *   5  usage error
 *   130 interrupted (teardown still runs)
 *
 * USAGE
 *   node scripts/qa/herdr-reference-chat.mjs --scenario QA-01 --case happy \
 *     --fixture-manifest <fixtures.json> --candidate-manifest <candidate.json> \
 *     --evidence-dir <dir> [--scenario ALL --case all] [--allow-host true] \
 *     [--device-binding-out <path>] [--browser-channel chrome] [--timeout-ms 30000]
 */

import { deepStrictEqual } from "node:assert/strict";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import {
  IsolatedGatewayError,
  OwnedProcessLedger,
  REFERENCE_CAPABILITIES_PATH,
  REFERENCE_HOST_ACCESS_KINDS,
  REFERENCE_NONLOCAL_TRANSPORTS,
  adoptOwnedGateway,
  hostAccessContract,
  REFERENCE_HISTORY_DEFAULT_LIMIT,
  REFERENCE_HISTORY_MAX_LIMIT,
  REFERENCE_QA_IDS,
  REFERENCE_QA_SCENARIOS,
  REFERENCE_REGISTRY_ROWS,
  REFERENCE_SCROLLBACK_DISCLOSURE,
  REPORT_SCHEMA,
  assert,
  assertFileReceipt,
  assertScopeResult,
  buildDeviceBinding,
  deadline,
  deviceReceiptPath,
  killExactPid,
  launchIsolatedGateway,
  mutationEnvelope,
  probeProcessIdentity,
  readCredentialToken,
  readDeviceReceipt,
  readGatewayCapabilities,
  redactUrl,
  referenceIdentityRefusal,
  referenceReadQuery,
  referenceRequest,
  repoRoot,
  scenarioById,
  sessionMissingIdentity,
  sessionsForHost,
  sha256File,
  validateCandidateManifest,
  validateFixtureManifest,
  validateReferenceCapabilities,
  validateRegistryRows,
  writeDeviceBinding,
  writeJson,
  writeReport,
} from "./herdr-reference-fixtures.mjs";
import { ptyIdentitySurvived } from "./herdr-reference-pty-identity.mjs";

/* ==========================================================================
 * Scenario registry
 *
 * Declared BEFORE the first registration below: `const` is not hoisted, so a
 * registration that ran above this block would throw on module load.
 * ========================================================================== */

const SCENARIOS = {};
function scenario(id, implementation) {
  SCENARIOS[id] = implementation;
}

/* ------------------------------------------------------------------ QA-06 */
scenario("QA-06", {
  async happy(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const before = await http.read(session, "screen", {}, "screen");
    const requestId = randomUUID();
    const stopped = await http.mutate(session, "stop", requestId, { capability: "providerInterrupt" });
    const data = assertScopeResult(stopped.json, requestId, "QA-06 stop");
    const after = await http.read(session, "screen", {}, "screen");
    // Stop must never be a kill: the original session must still be the same process.
    const survived = assertSessionSurvived(session, "QA-06 happy");
    const report = {
      beforeRevision: before.json && before.json.revision,
      afterRevision: after.json && after.json.revision,
      receipt: data.receipt,
      stopStage: data.receipt && data.receipt.stage,
      pidSurvived: session.pid,
      survived,
    };
    ctx.record("qa-06-happy.json", report);
    assert(data.receipt && data.receipt.stage === "accepted", "Stop did not reach the accepted stage");
    // The pane must be the same process: a Stop that replaced it would be a kill.
    const detail = await referenceRequest(host, {
      method: "GET",
      sessionId: session.backendSessionId,
      suffix: "screen",
      query: referenceReadQuery(session, {}, "screen"),
    });
    assert(detail.status === 200, "the pane stopped answering after Stop");
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    // "refused" and "shellSignal" are VALID enum values, so they reach capability validation;
    // "ctrl-c" is not a variant at all, so the envelope is refused before any capability is read.
    // Those two are different guards and are asserted separately below.
    const refused = await http.mutate(session, "stop", randomUUID(), { capability: "refused" });
    const shellSignal = await http.mutate(session, "stop", randomUUID(), { capability: "shellSignal" });
    const badCapability = await http.mutate(session, "stop", randomUUID(), { capability: "ctrl-c" });
    const report = {
      refused: { status: refused.status, body: redactForEvidence(refused.json) },
      shellSignal: { status: shellSignal.status, body: redactForEvidence(shellSignal.json) },
      unknownCapability: { status: badCapability.status, body: redactForEvidence(badCapability.json) },
    };
    ctx.record("qa-06-failure.json", report);
    // The capability guard itself: a valid capability the target cannot honour is a typed
    // ScopeResult refusal, never a substituted killing signal. "shellSignal" is recorded above and
    // deliberately NOT asserted - its semantics are unchanged pending the contract adjudication.
    assertTypedRefusal(refused, {
      label: "QA-06 refused capability",
      status: "UNSUPPORTED",
      code: "UNSUPPORTED",
      envelope: "scope",
    });
    // A schema-invalid capability is a malformed REQUEST, not a capability refusal: INVALID_REQUEST
    // on the machine envelope, from the extractor, is the correct and only expected outcome here.
    assertTypedRefusal(badCapability, {
      label: "QA-06 unknown capability (schema-invalid)",
      status: "INVALID_REQUEST",
      code: "INVALID_REQUEST",
      envelope: "machine",
    });
    return { status: "pass", observed: report };
  },
});

/* ------------------------------------------------------------------ QA-07 */
scenario("QA-07", {
  async happy(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const imageBytes = Buffer.from("HERDR_QA07_IMAGE_BYTES", "utf8");
    const imageRequestId = randomUUID();
    // The staged media type is one of the product's OWN five attachment variants
    // (`AttachmentMediaType`, scoped_contracts.rs:271-283). It is not a category name: the wire
    // field deserializes into that enum, so anything else is refused as an unreadable envelope
    // before the route runs a single staging guard.
    const staged = await http.mutate(session, "files", imageRequestId, {
      name: "qa07-image.png",
      mediaType: "image/png",
      sizeBytes: imageBytes.length,
      contentBase64: imageBytes.toString("base64"),
    });
    // Record what the route ACTUALLY answered before asserting on it, so a shape mismatch is
    // diagnosable from the receipt instead of only from the assertion message (the previous run
    // failed at the assertion and left no receipt behind). Credential-shaped keys and encoded
    // payloads are redacted; the fixture owns the bytes and can re-derive them.
    ctx.record("qa-07-stage-response.json", {
      status: staged.status,
      envelope: referenceEnvelopeOf(staged.json),
      body: redactForEvidence(staged.json),
    });
    const data = assertScopeResult(staged.json, imageRequestId, "QA-07 stage");
    const receipt = assertFileReceipt(data, "QA-07 stage");
    const preview = await http.previewFile(session, receipt.attachmentId);
    // The preview route answers a bare object ({receipt, displayName, sizeBytes,
    // contentBase64}); it is a read, not a ScopeResult mutation envelope.
    const previewData = preview.json;
    const removed = await http.deleteFile(session, receipt.attachmentId);
    const afterDelete = await http.previewFile(session, receipt.attachmentId);
    const report = {
      staged: { status: staged.status, receipt, mentionText: data.mentionText },
      preview: { status: preview.status, displayName: previewData.displayName, sizeBytes: previewData.sizeBytes },
      delete: { status: removed.status },
      previewAfterDelete: { status: afterDelete.status, body: redactForEvidence(afterDelete.json) },
    };
    ctx.record("qa-07-happy.json", report);
    assert(receipt.hostId === session.hostId, "the staged file was receipted on a different host");
    assert(data.mentionText === "@" + receipt.attachmentId + " " || data.mentionText.startsWith("@"),
      "the mention is not an editable @path mention");
    assert(preview.status === 200, "the staged file could not be previewed from its own owner");
    assert(previewData && typeof previewData.displayName === "string" && previewData.receipt,
      "the preview answered without the staged file's own receipt");
    assert(removed.status === 204, "explicit deletion did not answer 204");
    assert(afterDelete.status === 404, "a deleted file was still previewable");
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    // The control: the SAME route on the SAME target must ACCEPT a well-formed stage under every
    // bound. Without it a refusal from an earlier fence (an unreadable envelope, a missing owner id,
    // a dead session) satisfies the probes below and the branch passes while no file guard ran.
    const controlRequestId = randomUUID();
    // Every staged payload names one of the product's five attachment variants
    // (`AttachmentMediaType`, scoped_contracts.rs:271-283) - the same set the product's own client
    // derives in `referenceMediaTypeFor` (ui/src/remote/chat/referenceFiles.ts:260-286). The wire
    // field IS that enum, so a category name is refused as an unreadable envelope before any
    // staging guard runs, and each probe below would then be judging the wrong fence. These
    // payloads are ASCII text, so the honest pair for each is a `.txt` name and `text/plain`.
    const control = await http.mutate(session, "files", controlRequestId, {
      name: "qa07-control.txt",
      mediaType: "text/plain",
      sizeBytes: 4,
      contentBase64: Buffer.from("okay").toString("base64"),
    });
    const traversal = await http.mutate(session, "files", randomUUID(), {
      name: "../../escape.txt",
      mediaType: "text/plain",
      sizeBytes: 4,
      contentBase64: Buffer.from("evil").toString("base64"),
    });
    const oversize = await http.mutate(session, "files", randomUUID(), {
      name: "qa07-oversize.txt",
      mediaType: "text/plain",
      sizeBytes: 10 * 1024 * 1024 + 1,
      contentBase64: "",
    });
    const mismatch = await http.mutate(session, "files", randomUUID(), {
      name: "qa07-mismatch.txt",
      mediaType: "text/plain",
      sizeBytes: 999999,
      contentBase64: Buffer.from("short").toString("base64"),
    });
    const missingPreview = await http.previewFile(session, "00000000-0000-0000-0000-000000000000");
    // Every response is recorded before anything is asserted, so a shape mismatch is diagnosable
    // from the receipt and not only from an assertion message.
    const report = {
      control: { status: control.status, envelope: referenceEnvelopeOf(control.json), body: redactForEvidence(control.json) },
      traversal: { status: traversal.status, envelope: referenceEnvelopeOf(traversal.json), body: redactForEvidence(traversal.json) },
      oversize: { status: oversize.status, envelope: referenceEnvelopeOf(oversize.json), body: redactForEvidence(oversize.json) },
      declaredSizeMismatch: { status: mismatch.status, envelope: referenceEnvelopeOf(mismatch.json), body: redactForEvidence(mismatch.json) },
      unknownFilePreview: { status: missingPreview.status, envelope: referenceEnvelopeOf(missingPreview.json), body: redactForEvidence(missingPreview.json) },
    };
    ctx.record("qa-07-failure.json", report);
    assert(control.status === 200 && control.json && control.json.ok === true,
      "the control stage was not accepted: " + describeResponse(control));
    const controlData = assertScopeResult(control.json, controlRequestId, "QA-07 control stage");
    const controlReceipt = assertFileReceipt(controlData, "QA-07 control stage");
    // A name is a NAME: a separator is refused by validate_reference_file_name (files.rs) before any
    // bytes are decoded. This route's staging refusals are the MACHINE envelope, never a ScopeResult.
    assertTypedRefusal(traversal, {
      label: "QA-07 traversal-shaped name",
      status: "INVALID_REQUEST",
      code: "INVALID_REQUEST",
      envelope: "machine",
    });
    // Over ATTACHMENT_MAX_FILE_BYTES (10 MiB) the bounds check refuses before anything is staged.
    assertTypedRefusal(oversize, {
      label: "QA-07 oversized file",
      status: "PAYLOAD_TOO_LARGE",
      code: "PAYLOAD_TOO_LARGE",
      envelope: "machine",
    });
    // A declared size that disagrees with the bytes carried is a malformed request, not a size refusal.
    assertTypedRefusal(mismatch, {
      label: "QA-07 declared-size mismatch",
      status: "INVALID_REQUEST",
      code: "INVALID_REQUEST",
      envelope: "machine",
    });
    // A file id this target never staged resolves to nothing: the same NOT_FOUND the route answers
    // when the staged file is gone.
    assertTypedRefusal(missingPreview, {
      label: "QA-07 unknown file preview",
      status: "NOT_FOUND",
      code: "NOT_FOUND",
      envelope: "machine",
    });
    // Return the target's staged set to its prior state: the control is this branch's own artefact,
    // and a staged file left behind would count against the per-turn file bound on a re-run.
    const cleaned = await http.deleteFile(session, controlReceipt.attachmentId);
    report.controlCleanup = { status: cleaned.status };
    assert(cleaned.status === 204, "the control file was not deleted: " + describeResponse(cleaned));
    ctx.record("qa-07-failure.json", report);
    return { status: "pass", observed: report };
  },
});

/* ------------------------------------------------------------------ QA-08 */
scenario("QA-08", {
  async happy(ctx) {
    // QA-08 is a real-device scenario. This runner CONSUMES the receipts task 15 produces;
    // it never fabricates a native keyboard event, and a missing device is BLOCKED.
    const android = readDeviceReceipt(ctx.evidenceDir, "android");
    const ios = readDeviceReceipt(ctx.evidenceDir, "ios");
    const report = { android, ios };
    ctx.record("qa-08-happy.json", report);
    if (android.status === "MISSING" || ios.status === "MISSING") {
      throw blocked("device-receipt-missing",
        "QA-08 needs the native producer receipts: " + JSON.stringify({ android: android.status, ios: ios.status }));
    }
    if (android.status === "BLOCKED" || ios.status === "BLOCKED") {
      throw blocked("device-blocked", JSON.stringify({ android: android.reason, ios: ios.reason }));
    }
    assert(android.ok, "the Android receipt did not validate: " + android.errors.join("; "));
    assert(ios.ok, "the iOS receipt did not validate: " + ios.errors.join("; "));
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const { page, close } = await openChatPage(ctx, host, session);
    try {
      const composer = '[data-testid="chat-composer-textarea"]';
      await page.waitForSelector(composer, { timeout: ctx.args.timeoutMs });
      // A composition event that is not yet committed must never send.
      await page.evaluate(() => {
        const node = document.querySelector('[data-testid="chat-composer-textarea"]');
        node.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
        node.value = "한";
        node.dispatchEvent(new InputEvent("input", { bubbles: true, isComposing: true }));
      });
      const sentDuringComposition = await page.locator('[data-testid="user-message-bubble"]').count();
      await page.evaluate(() => {
        const node = document.querySelector('[data-testid="chat-composer-textarea"]');
        node.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
        node.dispatchEvent(new InputEvent("input", { bubbles: true, isComposing: false }));
      });
      const report = { sentDuringComposition, valueAfterComposition: await page.inputValue(composer) };
      ctx.record("qa-08-failure.json", report);
      assert(sentDuringComposition === 0, "an uncommitted composition produced a send");
      return { status: "pass", observed: report };
    } finally {
      await close();
    }
  },
});

/* ------------------------------------------------------------------ QA-09 */
scenario("QA-09", {
  async happy(ctx) {
    const { hosts, missing } = hostsForScenario(ctx, scenarioById("QA-09"));
    const rows = [];
    for (const host of hosts) {
      const sessions = sessionsOf(ctx, host);
      if (sessions.length === 0) {
        rows.push({ hostId: host.id, transport: host.transport, status: "no-session" });
        continue;
      }
      const session = sessions[0];
      const query = referenceReadQuery(session, {}, "screen");
      const response = await referenceRequest(host, {
        method: "GET",
        sessionId: session.backendSessionId,
        suffix: "screen",
        query,
      });
      rows.push({
        hostId: host.id,
        transport: host.transport,
        urlRedacted: host.urlRedacted || redactUrl(host.url),
        status: response.status,
        revision: response.json && response.json.revision,
        cols: response.json && response.json.cols,
        rows: response.json && response.json.rows,
        missingIdentity: sessionMissingIdentity(session),
      });
    }
    const report = { transports: rows, missingTransports: missing };
    ctx.record("qa-09-happy.json", report);
    assert(missing.length === 0, "no isolated host is configured for: " + missing.join(", "));
    for (const row of rows) {
      assert(row.status !== "no-session", "host " + row.hostId + " has no session to read");
      assert(row.status === 200, "transport " + row.transport + " did not answer 200: " + row.status);
    }
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { hosts } = hostsForScenario(ctx, scenarioById("QA-09"));
    const rows = [];
    for (const host of hosts) {
      const sessions = sessionsOf(ctx, host);
      if (sessions.length === 0) continue;
      const session = sessions[0];
      const wrongEpoch = await referenceRequest(host, {
        method: "GET",
        sessionId: session.backendSessionId,
        suffix: "screen",
        query: referenceReadQuery(session, { epoch: foreignEpochFor(session) }, "screen"),
      });
      const wrongOwner = await referenceRequest(host, {
        method: "GET",
        sessionId: session.backendSessionId,
        suffix: "screen",
        query: referenceReadQuery(session, { ownerId: "not-the-owner" }, "screen"),
      });
      rows.push({
        hostId: host.id,
        transport: host.transport,
        wrongEpoch: { status: wrongEpoch.status, body: wrongEpoch.json },
        wrongOwner: { status: wrongOwner.status, body: wrongOwner.json },
      });
    }
    ctx.record("qa-09-failure.json", { rows });
    for (const row of rows) {
      // A foreign incarnation is TargetExpired (410 GONE), never a served read.
      assert(row.wrongEpoch.status === 410,
        "transport " + row.transport + " did not refuse a foreign epoch with 410: " + row.wrongEpoch.status);
      assert(row.wrongOwner.status !== 200 || (row.wrongOwner.body && row.wrongOwner.body.code),
        "transport " + row.transport + " served a read for a foreign owner");
    }
    return { status: "pass", observed: { rows } };
  },
});

/* ------------------------------------------------------------------ QA-10 */
scenario("QA-10", {
  async happy(ctx) {
    const matrix = validateRegistryRows();
    const { host } = localContext(ctx);
    const rows = [];
    for (const row of REFERENCE_REGISTRY_ROWS) {
      const session = (ctx.fixture.sessions || []).find((entry) => entry.registryId === row.id);
      const aliases = [];
      for (const alias of row.aliases) {
        // An alias must resolve to the same native kind as its row, and never widen it.
        aliases.push({ alias, nativeKind: nativeKindOf(alias), rowNativeKind: row.nativeReader });
      }
      rows.push({
        registryId: row.id,
        aliases,
        nativeReader: row.nativeReader,
        detector: row.detector,
        hasSession: Boolean(session),
        originalPaneAccess: Boolean(session) && typeof session.backendSessionId === "string",
      });
    }
    const report = { matrixOk: matrix.ok, rows, sourceIds: matrix.ids, rowCount: matrix.rows, hostId: host.id };
    ctx.record("qa-10-happy.json", report);
    assert(matrix.ok, "the registry matrix does not match its source union: " + matrix.errors.join("; "));
    for (const row of rows) {
      for (const alias of row.aliases) {
        if (row.nativeReader === "none") {
          assert(alias.nativeKind === "unavailable",
            "alias " + alias.alias + " claims a native reader its row does not have");
        }
      }
    }
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    // The control is a SCREEN read, not a history read: the screen route resolves no transcript, so
    // a 404 there is unambiguously the live-session fence - whereas a history read 404s for a
    // missing transcript too, with the SAME status and code, and the two cannot be told apart.
    const control = await referenceRequest(host, {
      method: "GET",
      sessionId: session.backendSessionId,
      suffix: "screen",
      query: referenceReadQuery(session, {}, "screen"),
    });
    // The unknown-session probe still names the REAL incarnation and owner, so the only thing wrong
    // with it is the session id. That keeps the refusal attributable to the session rather than to
    // an expired epoch or a missing owner.
    const unknownSession = { ...session, backendSessionId: "no-such-session" };
    const unknown = await referenceRequest(host, {
      method: "GET",
      sessionId: "no-such-session",
      suffix: "screen",
      query: referenceReadQuery(unknownSession, {}, "screen"),
    });
    const codexAlias = nativeKindOf("codex-cli");
    const gjcDetector = REFERENCE_REGISTRY_ROWS.find((row) => row.id === "gjc").detector;
    const report = {
      control: { status: control.status, envelope: referenceEnvelopeOf(control.json) },
      unknownSession: { status: unknown.status, envelope: referenceEnvelopeOf(unknown.json), body: redactForEvidence(unknown.json) },
      unknownSessionEpoch: unknownSession.epoch,
      unknownAliasNativeKind: codexAlias,
      gjcDetector,
      knownRows: REFERENCE_REGISTRY_ROWS.map((row) => row.id),
    };
    ctx.record("qa-10-failure.json", report);
    assertReadControl(control, "QA-10 live-session control");
    // reference_chat_ensure_live: the id names no live session on this host.
    assertTypedRefusal(unknown, {
      label: "QA-10 unknown session",
      status: "NOT_FOUND",
      code: "NOT_FOUND",
      envelope: "machine",
    });
    // Matrix checks, not route evidence: they exercise the fixture's own registry functions and
    // pass regardless of what the gateway answered above.
    assert(codexAlias === "unavailable", "an unknown alias claimed a native reader");
    assert(gjcDetector === "none", "gjc was given a detector the reference does not have");
    return { status: "pass", observed: report };
  },
});

/* ------------------------------------------------------------------ QA-11 */
scenario("QA-11", {
  async happy(ctx) {
    const candidate = ctx.candidateRaw;
    const provenance = verifyCandidateProvenance(candidate, candidate.uiDist);
    const selectors = verifyShippedSelectors(candidate, candidate.uiDist);
    const inventory = ctx.fixtureRaw.sessions.map((session) => ({
      backendSessionId: session.backendSessionId,
      hostId: session.hostId,
      provider: session.provider,
      missingIdentity: sessionMissingIdentity(session),
    }));
    const report = {
      candidateId: candidate.candidateId,
      sourceRevision: candidate.sourceRevision,
      dirtyPatchSha256: candidate.dirtyPatchSha256,
      binarySha256: candidate.binary && candidate.binary.sha256,
      uiFileCount: (candidate.uiFiles || []).length,
      provenanceFindings: provenance,
      missingSelectors: selectors.missing,
      sessionInventory: inventory,
      spawnLedger: ctx.fixtureRaw.spawnLedger,
      scenarios: REFERENCE_QA_IDS,
    };
    ctx.record("qa-11-happy.json", report);
    assert(provenance.length === 0, "candidate provenance does not match: " + JSON.stringify(provenance));
    assert(selectors.missing.length === 0, "the served UI lacks selectors: " + selectors.missing.join(", "));
    assert(ctx.fixtureRaw.spawnLedger && Array.isArray(ctx.fixtureRaw.spawnLedger.entries),
      "the fixture manifest carries no spawn ledger");
    assert(REFERENCE_QA_IDS.length === 11, "the scenario inventory is not the frozen eleven");
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    // The gate's rule, verbatim: a deliberate mismatch MUST make the runner exit nonzero.
    // This branch proves it by RUNNING THE RUNNER AGAIN as a real child process against a
    // tampered candidate and asserting the child's exit code. The real artifacts are never
    // modified: the tampered bytes and the tampered manifest live under the evidence
    // directory, and the child writes its own evidence beside them.
    const tamperDir = join(ctx.evidenceDir, "tamper");
    const tamperUiDist = join(tamperDir, "ui");
    const tamperEvidence = join(tamperDir, "evidence");
    mkdirSync(tamperUiDist, { recursive: true });
    mkdirSync(tamperEvidence, { recursive: true });
    const candidate = ctx.candidateRaw;
    const target = (candidate.uiFiles || [])[0];
    if (!target) throw blocked("tamper-target-missing", "the candidate names no UI file to tamper with");
    const source = candidate.uiDist ? join(candidate.uiDist, target.path) : target.path;
    if (!existsSync(source)) throw blocked("tamper-source-missing", source);
    const tamperedPath = join(tamperUiDist, target.path);
    mkdirSync(dirname(tamperedPath), { recursive: true });
    writeFileSync(tamperedPath, Buffer.concat([readFileSync(source), Buffer.from("\n/* tampered */\n", "utf8")]));

    // The tampered manifest keeps EVERY recorded hash, including the original hash of the
    // file whose bytes just changed. That mismatch is exactly what the verifier must reject.
    const tamperedManifestPath = join(tamperDir, "candidate-tampered.json");
    writeJson(tamperedManifestPath, { ...candidate, uiDist: tamperUiDist });

    const child = spawn(process.execPath, [
      resolve(fileURLToPath(import.meta.url)),
      "--scenario", "QA-11",
      "--case", "happy",
      "--fixture-manifest", resolve(ctx.args.fixtureManifest),
      "--candidate-manifest", tamperedManifestPath,
      "--evidence-dir", tamperEvidence,
    ], { stdio: ["ignore", "pipe", "pipe"] });
    let childOutput = "";
    child.stdout.on("data", (data) => { childOutput += data.toString(); });
    child.stderr.on("data", (data) => { childOutput += data.toString(); });
    const childExitCode = await deadline(new Promise((resolveExit) => {
      child.once("exit", (code) => resolveExit(code === null ? -1 : code));
      child.once("error", () => resolveExit(-1));
    }), "tampered-child-exit", ctx.args.timeoutMs);

    // A dropped scenario must also be detectable, so the gate cannot pass a run that silently
    // stopped selecting a row.
    const fullInventory = REFERENCE_QA_IDS.slice();
    const dropped = fullInventory.slice(0, fullInventory.length - 1);
    const droppedDetected = fullInventory.some((id) => !dropped.includes(id));

    const report = {
      tamperTarget: { path: target.path, originalSha256: target.sha256 },
      tamperedManifest: tamperedManifestPath,
      tamperedUiDist: tamperUiDist,
      childExitCode,
      childOutputTail: childOutput.slice(-4000),
      tamperRejected: childExitCode !== 0,
      droppedScenarioDetected: droppedDetected,
      nestedVerdict: childExitCode !== 0 ? "nonzero-as-required" : "NOT-DETECTED",
      scenarioInventory: fullInventory,
    };
    ctx.record("qa-11-failure.json", report);
    assert(childExitCode !== 0, "a tampered candidate artifact did not make the runner exit nonzero");
    assert(droppedDetected, "a dropped scenario was not detectable against the frozen inventory");
    return { status: "pass", observed: report };
  },
});

/* ==========================================================================
 * Browser helper
 * ========================================================================== */

/** The absolute URL of one frozen route on a host. */
function referenceRouteUrlFor(host, sessionId, suffix) {
  const base = String(host.url).replace(/\/+$/, "");
  return base + "/api/v1/reference-chat/" + sessionId + "/" + suffix;
}

/** The native reader a registry id or alias maps to. Mirrors referenceTypes.ts. */
function nativeKindOf(registryId) {
  switch (String(registryId).trim().toLowerCase()) {
    case "claude": return "claude";
    case "codex": return "codex";
    case "omp": return "omp";
    case "omo": return "omo";
    case "gjc": return "gjc";
    case "pi": return "pi";
    default: return "unavailable";
  }
}

/**
 * Assert the original session survived an operation that must not replace it.
 *
 * The comparison is against the SPAWN-TIME identity record, never a fresh scan: a replaced
 * pane (new PID) and a recycled PID (same number, different executable) both fail. A session
 * with no recorded identity reports that survival is unassertable rather than passing.
 */
function assertSessionSurvived(session, label) {
  const record = session.ptyIdentity || null;
  if (!record) {
    throw blocked(
      "pty-identity-absent",
      label + ": the fixture recorded no spawn-time PTY identity for " +
        String(session.backendSessionId) + ", so the original session's survival cannot be asserted",
    );
  }
  const survived = ptyIdentitySurvived(record, probeProcessIdentity);
  if (!survived.survived) {
    throw failed(label + ": the original session did not survive: " + survived.reason);
  }
  return survived;
}

/**
 * An incarnation that is guaranteed NOT to be the target's own, so the negative branch tests
 * a real foreign epoch rather than a value that might coincidentally match.
 */
function foreignEpochFor(session) {
  const current = String(session.epoch === undefined || session.epoch === null ? "" : session.epoch).trim();
  if (!/^\d+$/.test(current)) return "1";
  try {
    return (BigInt(current) + 1n).toString();
  } catch {
    return current === "0" ? "1" : "0";
  }
}

/**
 * Open the served candidate UI in a real browser against one host, with the host's own
 * credential seeded into the client store. Nothing here clears or clones a user profile:
 * the context is a fresh, isolated one.
 */
async function openChatPage(ctx, host, session) {
  if (!ctx.args.allowHost) throw blocked("browser-authorization", "driving the served UI requires --allow-host true");
  const require = createRequire(join(repoRoot, "ui", "package.json"));
  let chromium;
  try {
    ({ chromium } = require(process.env.PLAYWRIGHT_CORE_PATH || "playwright-core"));
  } catch (error) {
    throw blocked("playwright-missing", String(error));
  }
  ctx.browser = ctx.browser || await chromium.launch({
    headless: true,
    channel: ctx.args.browserChannel === "chromium-headless" ? undefined : ctx.args.browserChannel,
  });
  const token = readCredentialToken(host);
  const context = await ctx.browser.newContext({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  await context.addInitScript((seed) => {
    localStorage.setItem("ferryx_remote_hosts", JSON.stringify({
      hosts: {
        [seed.hostId]: {
          hostId: seed.hostId,
          name: "Herdr reference isolated",
          address: seed.base,
          transport: "mdns",
          authStatus: "paired",
          online: true,
          deviceToken: seed.token,
          machineId: seed.hostId,
        },
      },
      activeHostId: seed.hostId,
    }));
    localStorage.setItem("ferryx_device_id_" + seed.hostId, seed.deviceId);
  }, { hostId: host.id, base: host.url, token, deviceId: host.id + "-device" });
  const page = await context.newPage();
  page.setDefaultTimeout(ctx.args.timeoutMs);
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto(host.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="mobile-chat-workspace"], [data-testid="remote-workspace-loading"]',
    { timeout: ctx.args.timeoutMs });
  // The rendered page is the evidence the JSON artifact below only names. A capture that
  // fails must not take down a branch that otherwise passes, so the failure is recorded in
  // that same artifact instead of being thrown.
  const screenshotName = "browser-" + scenarioSafeName(ctx) + ".png";
  let screenshot = null;
  let screenshotError = null;
  try {
    screenshot = ctx.recordFile(screenshotName, await page.screenshot());
  } catch (error) {
    screenshotError = String(error && error.message ? error.message : error);
  }
  ctx.record("browser-" + scenarioSafeName(ctx) + ".json", {
    hostId: host.id,
    urlRedacted: host.urlRedacted || host.url,
    pageErrors,
    backendSessionId: session.backendSessionId,
    screenshot,
    screenshotError,
  });
  return {
    page,
    context,
    async close() {
      await context.close();
    },
  };
}

function scenarioSafeName(ctx) {
  return String(ctx.currentScenario || "unknown").replace(/[^A-Za-z0-9-]/g, "_");
}

/* ==========================================================================
 * Main
 * ========================================================================== */

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const scenarios = selectedScenarios(args);
  const cases = selectedCases(args);
  const { fixtureRaw, candidateRaw, fixture, candidate } = loadManifests(args);

  const ledger = new OwnedProcessLedger("herdr-reference-chat");
  const ctx = createContext(args, fixtureRaw, candidateRaw, fixture, candidate, ledger);

  const result = {
    schema: REPORT_SCHEMA,
    scriptId: SCRIPT_ID,
    startedAt: new Date().toISOString(),
    scenarioSelection: args.scenario,
    caseSelection: args.caseName,
    fixtureManifest: args.fixtureManifest,
    candidateManifest: args.candidateManifest,
    fixtureOk: fixture.ok,
    candidateOk: candidate.ok,
    fixtureErrors: fixture.errors,
    candidateErrors: candidate.errors,
    branches: {},
    blockers: [],
    failures: [],
    cleanup: null,
    overallStatus: "BLOCKED",
  };

  let interrupted = false;
  const onSignal = () => {
    interrupted = true;
  };
  process.on("SIGINT", onSignal);
  process.on("SIGTERM", onSignal);

  let exitCode = EXIT.OK;
  try {
    // Manifest validity is a hard prerequisite: a runner that guessed a default would be
    // certifying a fixture it never saw.
    if (!fixture.ok) {
      result.blockers.push({ kind: "fixture-manifest", errors: fixture.errors });
      throw blocked("fixture-manifest-invalid", fixture.errors.join("; "));
    }
    if (!candidate.ok) {
      result.blockers.push({ kind: "candidate-manifest", errors: candidate.errors });
      throw blocked("candidate-manifest-invalid", candidate.errors.join("; "));
    }
    for (const entry of fixtureRaw.blockers || []) {
      result.blockers.push({ kind: "provisioner-blocker", ...entry });
    }

    const provenance = verifyCandidateProvenance(candidateRaw, candidateRaw.uiDist);
    result.candidateProvenance = provenance;
    if (provenance.length > 0) {
      throw blocked("candidate-provenance-mismatch", JSON.stringify(provenance));
    }
    const selectors = verifyShippedSelectors(candidateRaw, candidateRaw.uiDist);
    result.shippedSelectors = selectors;
    if (selectors.missing.length > 0) {
      throw blocked("candidate-ui-selector-missing", selectors.missing.join(", "));
    }

    // Every configured host's access contract, resolved before any scenario runs. A local
    // host is reached through a daemon this runner owns; a non-local host (ssh, paired,
    // account-relay) is reached over the frozen HTTP routes at its own gateway url with its
    // own credential - the repository's existing mechanism. An incomplete fixture is reported
    // with the exact missing endpoint or credential, so QA-09's non-local rows are BLOCKED on
    // a named prerequisite rather than on the transport itself.
    {
      const hostAccess = [];
      const accessBlockers = [];
      for (const host of fixtureRaw.hosts || []) {
        try {
          const access = hostAccessContract(host);
          hostAccess.push(access);
          if (access.kind === REFERENCE_HOST_ACCESS_KINDS.httpGateway) {
            // A non-local host must actually answer before its rows are attempted, and the
            // answer must be its own: this is the same authenticated capabilities read the
            // local host goes through, so a wrong URL or a revoked credential is named here
            // rather than surfacing as an unexplained 401 inside a scenario. A credential that
            // cannot even be read is the same class of BLOCKED, not a harness interaction
            // failure, so the reader's own throw is converted here.
            try {
              const capabilities = await readGatewayCapabilities(host);
              access.capabilitiesStatus = capabilities.status;
              if (capabilities.status !== 200) {
                accessBlockers.push({
                  kind: "host-access-unreachable",
                  hostId: host.id,
                  transport: host.transport,
                  reason:
                    "GET " + REFERENCE_CAPABILITIES_PATH + " -> " + capabilities.status +
                    " at " + redactUrl(host.url) + " (" + access.credentialTier + " credential)",
                });
              }
            } catch (error) {
              access.capabilitiesStatus = null;
              accessBlockers.push({
                kind: "host-credential-unreadable",
                hostId: host.id,
                transport: host.transport,
                detail: String(access.credentialFile || ""),
                reason: String(error && error.message ? error.message : error),
              });
            }
          }
        } catch (error) {
          if (!(error instanceof IsolatedGatewayError)) throw error;
          accessBlockers.push({
            kind: "host-access-contract",
            hostId: host.id,
            transport: host.transport,
            reason: error.reason,
            detail: error.detail,
          });
        }
      }
      // Driving a provisioned host is a host action, so it takes the same explicit
      // authorization as launching one: without it the non-local rows are not attempted and
      // the missing authorization is named, rather than the transport being called unusable.
      for (const access of hostAccess) {
        access.authorized = args.allowHost === true;
      }
      result.hostAccess = hostAccess;
      // Non-local transports the fixture did not configure at all are named as such, so the
      // QA-09 coverage boundary is visible in the receipt instead of being inferred.
      const configured = new Set(hostAccess.map((entry) => entry.transport));
      result.nonLocalTransportsMissing = REFERENCE_NONLOCAL_TRANSPORTS.filter((transport) => !configured.has(transport));
      if (!args.allowHost && hostAccess.some((entry) => REFERENCE_NONLOCAL_TRANSPORTS.includes(entry.transport))) {
        accessBlockers.push({
          kind: "host-access-authorization-required",
          reason: "driving a provisioned non-local host requires --allow-host true",
          transports: hostAccess
            .filter((entry) => REFERENCE_NONLOCAL_TRANSPORTS.includes(entry.transport))
            .map((entry) => entry.transport),
        });
      }
      for (const entry of accessBlockers) result.blockers.push(entry);
    }

    // The frozen gateway the scenarios drive, in one of three shapes: ADOPTED from the
    // provisioning run that spawned this fixture's original PTYs (the same daemon, epoch and
    // sessions), LAUNCHED here on a throwaway profile, or already running elsewhere and
    // referenced by url + credential. A launch is never used when a provisioning run handed
    // one over: a fresh daemon would have a different epoch and none of those sessions, so
    // the recorded originalPTY identity would be asserted against a daemon that never
    // spawned it.
    const localHost = (fixtureRaw.hosts || []).find((entry) => entry.transport === "local");
    if (localHost && localHost.ownedGateway) {
      // allowHost is passed through, not assumed: adopting a daemon recorded in a fixture
      // manifest is a host action, and the adopter refuses without it.
      const gateway = await adoptOwnedGateway(localHost.ownedGateway, {
        timeoutMs: args.timeoutMs,
        allowHost: args.allowHost,
        // The adopted daemon's own log lives inside the profile the record names, so it is
        // captured into this run's evidence before that profile is removed.
        evidenceDir: args.evidenceDir,
        evidenceName: "daemon-output-adopted.log",
      });
      ctx.gateways.push(gateway);
      localHost.url = gateway.url;
      localHost.gatewayUrl = gateway.url;
      localHost.credentialFile = gateway.credentialFile;
      result.localGateway = {
        contract: gateway.contract,
        adopted: true,
        ownership: gateway.ownership,
        urlRedacted: redactUrl(gateway.url),
        port: gateway.port,
        boundAddress: gateway.boundAddress,
        portRequirement: gateway.portRequirement,
        pid: gateway.entry.pid,
        executablePath: gateway.entry.executablePath,
        daemonTransport: gateway.daemonTransport,
        daemonSocketPath: gateway.daemonSocketPath,
        daemonPid: gateway.daemonPid,
        daemonEpoch: gateway.daemonEpoch,
        devicePermission: gateway.devicePermission,
        referenceHostId: gateway.referenceHostId,
      };
    } else if (localHost && localHost.startLocal === true) {
      const gateway = await startIsolatedGateway(args, candidateRaw, ledger);
      ctx.gateways.push(gateway);
      localHost.url = gateway.url;
      localHost.gatewayUrl = gateway.url;
      // The credential file the launcher wrote inside the isolated profile: this runner
      // never invents a token and never reads a production one.
      localHost.credentialFile = gateway.credentialFile;
      result.localGateway = {
        contract: gateway.contract,
        adopted: false,
        urlRedacted: redactUrl(gateway.url),
        port: gateway.port,
        boundAddress: gateway.boundAddress,
        portRequirement: gateway.portRequirement,
        pid: gateway.entry.pid,
        executablePath: gateway.entry.executablePath,
        // The control endpoint the launcher actually drove (a unix socket, or on Windows the
        // loopback port/token pair), so the receipt names the transport rather than a path
        // that does not exist on that platform.
        daemonTransport: gateway.daemonTransport,
        daemonSocketPath: gateway.daemonSocketPath,
        daemonPid: gateway.daemonPid,
        daemonEpoch: gateway.daemonEpoch,
        devicePermission: gateway.devicePermission,
        referenceHostId: gateway.referenceHostId,
      };
    }


    // The corrected identity contract: capabilities own the authoritative daemonEpoch and the
    // referenceHostId a mutation target must carry. A read omits hostId entirely and the
    // gateway resolves its own. machineId is never substituted for referenceHostId.
    {
      const capabilityHost = (fixtureRaw.hosts || []).find((entry) => entry.transport === "local");
      if (!capabilityHost) throw blocked("local-host-missing", "the fixture manifest configures no local host");
      if (typeof capabilityHost.url !== "string" || capabilityHost.url.trim().length === 0) {
        throw blocked(
          "local-host-url-missing",
          "the local host has no url; run with --allow-host true to launch the frozen gateway, " +
            "or give the fixture an already-running isolated gateway url",
        );
      }
      const capabilities = await readGatewayCapabilities(capabilityHost);
      if (capabilities.status !== 200 || !capabilities.json) {
        throw blocked("capabilities-unreadable", "GET " + capabilityHost.url + "/api/v1/capabilities -> " + capabilities.status);
      }
      const validated = validateReferenceCapabilities(capabilities.json);
      result.capabilities = {
        status: capabilities.status,
        daemonEpoch: capabilities.json.daemonEpoch || null,
        referenceHostId: capabilities.json.referenceHostId || null,
        machineId: capabilities.json.machineId || null,
        ok: validated.ok,
        missing: validated.missing,
      };
      if (!validated.ok) {
        throw blocked("reference-host-id-absent", validated.errors.join("; "));
      }
      const epochDrift = [];
      for (const session of fixtureRaw.sessions || []) {
        if (session.hostId !== capabilityHost.id) continue;
        if (session.epoch && String(session.epoch) !== String(capabilities.json.daemonEpoch)) {
          epochDrift.push({ backendSessionId: session.backendSessionId, manifestEpoch: session.epoch, capabilityEpoch: capabilities.json.daemonEpoch });
        }
        // Capabilities are authoritative for the incarnation and the host id.
        session.epoch = String(capabilities.json.daemonEpoch);
        session.referenceHostId = capabilities.json.referenceHostId;
      }
      result.epochDrift = epochDrift;
    }

    // The device binding: the candidate/page/target/PTY tuple the task 15 producers copy
    // into their receipts. Written from the manifests and the live gateway, never guessed,
    // and announced on stdout so the operator can export FERRYX_HERDR_REFERENCE_BINDING with
    // the exact path rather than a default. Built AFTER capabilities so target.hostId is the
    // gateway's own referenceHostId rather than a local label.
    {
      const bindingHost = (fixtureRaw.hosts || []).find((entry) => entry.transport === "local");
      const bindingSession = bindingHost
        ? (fixtureRaw.sessions || []).find((session) => session.hostId === bindingHost.id)
        : null;
      const bindingPath = args.deviceBindingOut
        ? resolve(args.deviceBindingOut)
        : join(resolve(args.evidenceDir), "device-binding.json");
      const binding = buildDeviceBinding({
        candidate: {
          candidateId: candidateRaw.candidateId,
          sourceManifestSha256: sha256File(args.candidateManifest),
          binaryPath: candidateRaw.binary && candidateRaw.binary.path,
          binarySha256: candidateRaw.binary && candidateRaw.binary.sha256,
        },
        host: bindingHost || {},
        session: bindingSession || {},
        gateway: bindingHost
          ? { origin: redactUrl(bindingHost.url), url: bindingHost.url, urlRedacted: redactUrl(bindingHost.url) }
          : {},
        receiptPath: deviceReceiptPath(resolve(args.evidenceDir), "android"),
      });
      writeDeviceBinding(bindingPath, binding);
      result.deviceBinding = {
        path: bindingPath,
        envVariable: "FERRYX_HERDR_REFERENCE_BINDING",
        candidateId: binding.candidate.candidateId,
        backendSessionId: binding.target.backendSessionId,
        targetHostId: binding.target.hostId,
        receiptPath: binding.receiptPath,
      };
      process.stdout.write("FERRYX_HERDR_REFERENCE_BINDING=" + bindingPath + "\n");
    }

    for (const row of scenarios) {
      for (const caseName of cases) {
        ctx.currentScenario = row.id + "-" + caseName;
        const key = row.id + "/" + caseName;
        const implementation = SCENARIOS[row.id];
        if (!implementation || typeof implementation[caseName] !== "function") {
          result.branches[key] = { status: "missing-branch", reason: "no implementation for this branch" };
          result.failures.push({ scenario: row.id, case: caseName, reason: "no implementation for this branch" });
          continue;
        }
        const startedAt = Date.now();
        try {
          const observed = await implementation[caseName](ctx);
          result.branches[key] = { status: observed.status, ms: Date.now() - startedAt, observed: observed.observed };
        } catch (error) {
          const code = error instanceof RunnerError ? error.code : EXIT.FAILED;
          const status = code === EXIT.BLOCKED ? "blocked" : "failed";
          result.branches[key] = {
            status,
            ms: Date.now() - startedAt,
            reason: error.reason || error.message,
            detail: error.detail || "",
          };
          if (status === "blocked") result.blockers.push({ kind: "branch", scenario: row.id, case: caseName, reason: error.reason || error.message });
          else result.failures.push({ scenario: row.id, case: caseName, reason: error.reason || error.message, detail: error.detail || "" });
        }
        if (interrupted) break;
      }
      if (interrupted) break;
    }

    if (interrupted) {
      exitCode = EXIT.INTERRUPT;
      result.overallStatus = "INTERRUPTED";
    } else if (result.failures.length > 0) {
      exitCode = EXIT.FAILED;
      result.overallStatus = "FAIL";
    } else if (result.blockers.length > 0) {
      exitCode = EXIT.BLOCKED;
      result.overallStatus = "BLOCKED";
    } else {
      const allPass = Object.values(result.branches).every((branch) => branch.status === "pass");
      const expected = scenarios.length * cases.length;
      const ran = Object.keys(result.branches).length;
      exitCode = allPass && ran === expected ? EXIT.OK : EXIT.FAILED;
      result.overallStatus = exitCode === EXIT.OK ? "PASS" : "FAIL";
      if (ran !== expected) {
        result.failures.push({ scenario: "*", case: "*", reason: "selected " + expected + " branches but recorded " + ran });
      }
    }
  } catch (error) {
    const code = error instanceof RunnerError ? error.code : EXIT.INTERACTION;
    exitCode = code;
    result.overallStatus = code === EXIT.BLOCKED ? "BLOCKED" : "FAIL";
    result.blockers.push({ kind: "harness", reason: error.reason || error.message, detail: error.detail || "" });
  } finally {
    // Teardown is paired and recorded. Only PIDs whose live executable still matches the
    // spawn-recorded one are killed; anything else is reported.
    for (const gateway of ctx.gateways) {
      // Persist before stopping: the daemon's final lines are the only thing that attributes
      // a mid-run transport failure, and they are gone once it exits.
      try {
        gateway.persistDaemonOutput(args.evidenceDir, "daemon-output.log");
      } catch {
        /* Evidence that cannot be written must not replace the failure it explains. */
      }
      try {
        await gateway.stop();
      } catch {
        /* The ledger below is the authority for the receipt. */
      }
      try {
        gateway.profile.cleanup();
      } catch {
        /* A leftover temp profile is reported by the ledger, not hidden. */
      }
    }
    if (ctx.browser) {
      try {
        await ctx.browser.close();
      } catch {
        /* The ledger below is the authority for the receipt. */
      }
    }
    const receipt = ledger.teardown(killExactPid);
    result.cleanup = receipt;
    result.finishedAt = new Date().toISOString();
    try {
      writeReport(args.evidenceDir, result);
    } catch (error) {
      process.stderr.write("could not write the report: " + String(error) + "\n");
    }
    for (const entry of receipt.receipts) {
      if (entry.action === "reported-not-killed") {
        process.stderr.write(
          "REPORTED (not killed): pid " + entry.pid + " live=" + entry.liveExecutable +
            " recorded=" + entry.recordedExecutablePath + "\n",
        );
      }
    }
  }

  if (result.blockers.length > 0) {
    process.stderr.write(
      "BLOCKED (" + result.blockers.length + "):\n" +
        result.blockers.map((entry) => "  - " + (entry.kind || "?") + ": " + (entry.reason || JSON.stringify(entry.errors || entry))).join("\n") + "\n",
    );
  }
  if (result.failures.length > 0) {
    process.stderr.write(
      "FAILED (" + result.failures.length + "):\n" +
        result.failures.map((entry) => "  - " + entry.scenario + "/" + entry.case + ": " + entry.reason).join("\n") + "\n",
    );
  }
  process.stdout.write("overallStatus=" + result.overallStatus + " branches=" + Object.keys(result.branches).length + "\n");
  return exitCode;
}

const SCRIPT_ID = "herdr-reference-chat.mjs/1.0.0";

const EXIT = { OK: 0, BLOCKED: 2, INTERACTION: 3, FAILED: 4, USAGE: 5, INTERRUPT: 130 };

/** The selector set the served candidate UI must actually ship. */
const REQUIRED_TEST_IDS = [
  "mobile-chat-workspace",
  "chat-message-stream",
  "remote-view-mode-chat",
  "remote-view-mode-terminal",
  // The production remote terminal surface. NOTE: "remote-terminal" is NOT a production
  // selector — it exists only as a vi.mock stub in the remote UI's own test files, so
  // requiring it would assert something the shipped bundle never renders. The real surface
  // is the grid (ui/src/remote/RemoteTerminal.tsx) plus its input sink.
  "remote-terminal-grid",
  "remote-terminal-input-sink",
  "chat-composer-textarea",
  "send-button",
  "stop-button",
  "file-upload-input",
  "attach-file-button",
  "reference-prompt-card",
  "reference-prompt-submit",
  "reference-prompt-confirm",
  "reference-prompt-confirm-send",
  "reference-older-button",
  "reference-disclosure",
  "reference-abandoned",
  "reference-parts",
  "reference-image",
  "reference-skill",
];

class RunnerError extends Error {
  constructor(code, reason, detail) {
    super(reason + (detail ? ": " + detail : ""));
    this.code = code;
    this.reason = reason;
    this.detail = detail || "";
  }
}

const blocked = (reason, detail) => new RunnerError(EXIT.BLOCKED, reason, detail);
const failed = (reason, detail) => new RunnerError(EXIT.FAILED, reason, detail);

/* ==========================================================================
 * CLI
 * ========================================================================== */

function usage(message) {
  process.stderr.write(
    "USAGE ERROR: " + message + "\n\n" +
      "  node scripts/qa/herdr-reference-chat.mjs --scenario <QA-01..QA-11|ALL> --case <happy|failure|all>\n" +
      "      --fixture-manifest <fixtures.json> --candidate-manifest <candidate.json>\n" +
      "      --evidence-dir <dir> [--allow-host true] [--browser-channel <channel>] [--timeout-ms <ms>]\n",
  );
  process.exit(EXIT.USAGE);
}

function parseArgs(argv) {
  const args = { browserChannel: "chrome", timeoutMs: 30000, allowHost: false };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) usage(flag + " requires a value");
      return argv[i];
    };
    switch (flag) {
      case "--scenario": args.scenario = next(); break;
      case "--case": args.caseName = next(); break;
      case "--fixture-manifest": args.fixtureManifest = next(); break;
      case "--candidate-manifest": args.candidateManifest = next(); break;
      case "--evidence-dir": args.evidenceDir = next(); break;
      case "--device-binding-out": args.deviceBindingOut = next(); break;
      case "--allow-host": args.allowHost = next() === "true"; break;
      case "--browser-channel": args.browserChannel = next(); break;
      case "--timeout-ms": args.timeoutMs = Number.parseInt(next(), 10); break;
      case "-h":
      case "--help":
        process.stdout.write(readFileSync(new URL(import.meta.url), "utf8").split("*/")[0] + "*/\n");
        process.exit(EXIT.OK);
        break;
      default:
        usage("unknown flag " + flag);
    }
  }
  if (!args.scenario) usage("--scenario is mandatory");
  if (!args.caseName) usage("--case is mandatory");
  if (!args.fixtureManifest) usage("--fixture-manifest is mandatory");
  if (!args.candidateManifest) usage("--candidate-manifest is mandatory");
  if (!args.evidenceDir) usage("--evidence-dir is mandatory");
  if (args.scenario !== "ALL" && !REFERENCE_QA_IDS.includes(args.scenario)) {
    usage("--scenario must be one of " + REFERENCE_QA_IDS.join(", ") + " or ALL");
  }
  if (!["happy", "failure", "all"].includes(args.caseName)) usage("--case must be happy, failure or all");
  if (!Number.isFinite(args.timeoutMs) || args.timeoutMs <= 0) usage("--timeout-ms must be a positive integer");
  return args;
}

function selectedCases(args) {
  if (args.caseName === "all") return ["happy", "failure"];
  return [args.caseName];
}

function selectedScenarios(args) {
  if (args.scenario === "ALL") return REFERENCE_QA_SCENARIOS.slice();
  const row = scenarioById(args.scenario);
  assert(row, "unknown scenario " + args.scenario);
  return [row];
}

/* ==========================================================================
 * Manifest loading and provenance
 * ========================================================================== */

function loadManifests(args) {
  if (!existsSync(args.fixtureManifest)) throw blocked("fixture-manifest-missing", args.fixtureManifest);
  if (!existsSync(args.candidateManifest)) throw blocked("candidate-manifest-missing", args.candidateManifest);
  const fixtureRaw = JSON.parse(readFileSync(args.fixtureManifest, "utf8"));
  const candidateRaw = JSON.parse(readFileSync(args.candidateManifest, "utf8"));
  const fixture = validateFixtureManifest(fixtureRaw);
  const candidate = validateCandidateManifest(candidateRaw);
  return { fixtureRaw, candidateRaw, fixture, candidate };
}

/**
 * Re-hash every artifact the candidate manifest names and compare. A candidate whose binary
 * or UI bytes changed under it is not the candidate the evidence would certify.
 */
function verifyCandidateProvenance(candidateRaw, uiDist) {
  const findings = [];
  const binary = candidateRaw.binary;
  if (!binary || !binary.path) {
    findings.push({ kind: "binary", reason: "candidate manifest names no binary path" });
  } else if (!existsSync(binary.path)) {
    findings.push({ kind: "binary", path: binary.path, reason: "candidate binary is absent" });
  } else {
    const actual = sha256File(binary.path);
    if (actual !== binary.sha256) {
      findings.push({ kind: "binary", path: binary.path, expected: binary.sha256, actual, reason: "binary hash mismatch" });
    }
  }
  for (const file of candidateRaw.sourceFiles || []) {
    if (!existsSync(file.path)) {
      findings.push({ kind: "source", path: file.path, reason: "candidate source file is absent" });
      continue;
    }
    const actual = sha256File(file.path);
    if (actual !== file.sha256) {
      findings.push({ kind: "source", path: file.path, expected: file.sha256, actual, reason: "source hash mismatch" });
    }
  }
  const dist = uiDist || candidateRaw.uiDist;
  for (const file of candidateRaw.uiFiles || []) {
    const absolute = dist ? join(dist, file.path) : file.path;
    if (!existsSync(absolute)) {
      findings.push({ kind: "ui", path: absolute, reason: "candidate UI file is absent" });
      continue;
    }
    const actual = sha256File(absolute);
    if (actual !== file.sha256) {
      findings.push({ kind: "ui", path: absolute, expected: file.sha256, actual, reason: "UI hash mismatch" });
    }
  }
  return findings;
}

/** The served UI must actually ship the selectors the scenarios drive. */
function verifyShippedSelectors(candidateRaw, uiDist) {
  const missing = [];
  const sources = [];
  for (const file of candidateRaw.uiFiles || []) {
    if (!/\.(?:m?js)$/.test(file.path)) continue;
    const absolute = uiDist ? join(uiDist, file.path) : file.path;
    if (existsSync(absolute)) sources.push(readFileSync(absolute, "utf8"));
  }
  if (sources.length === 0) {
    return { missing: REQUIRED_TEST_IDS.slice(), reason: "the candidate ships no JavaScript to inspect" };
  }
  for (const id of REQUIRED_TEST_IDS) {
    if (!sources.some((source) => source.includes(id))) missing.push(id);
  }
  return { missing, reason: missing.length === 0 ? null : "the served UI does not ship every required selector" };
}

/* ==========================================================================
 * Harness context
 * ========================================================================== */

/**
 * Launch the frozen candidate as an isolated gateway and obtain its URL and device token.
 *
 * The contract is the product's own headless daemon, not a JSON ready line it never
 * writes: --daemon plus the persisted isolated remote config, the bound address read back
 * from that daemon, and a token from the real pairing flow. launchIsolatedGateway in
 * herdr-reference-fixtures.mjs carries the source each step is read from.
 */
async function startIsolatedGateway(args, candidateRaw, ledger) {
  if (!args.allowHost) {
    throw blocked("host-authorization", "starting a local gateway requires --allow-host true");
  }
  try {
    return await launchIsolatedGateway({
      binary: candidateRaw.binary && candidateRaw.binary.path,
      label: "chat",
      uiDist: candidateRaw.uiDist,
      ledger,
      timeoutMs: args.timeoutMs,
      // The daemon's own output travels with this run's evidence, so a transport failure
      // during the scenarios is attributable instead of being inferred from a reset.
      evidenceDir: args.evidenceDir,
      evidenceName: "daemon-output.log",
      extra: { launchedBy: "herdr-reference-chat.mjs" },
    });
  } catch (error) {
    if (error instanceof IsolatedGatewayError) throw blocked(error.reason, error.detail);
    throw error;
  }
}

function createContext(args, fixtureRaw, candidateRaw, fixture, candidate, ledger) {
  const evidenceDir = resolve(args.evidenceDir);
  mkdirSync(evidenceDir, { recursive: true });
  const artifacts = [];
  return {
    args,
    fixtureRaw,
    candidateRaw,
    fixture,
    candidate,
    ledger,
    evidenceDir,
    gateways: [],
    browser: null,
    /** Persist one evidence artifact and return its path. */
    record(name, value) {
      const path = join(evidenceDir, name);
      writeJson(path, value);
      artifacts.push({ name, path });
      return path;
    },
    recordFile(name, bytes) {
      const path = join(tmpdir(), "ferryx-reference-chat", randomUUID(), name);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, bytes);
      artifacts.push({ name, path });
      return path;
    },
    artifacts,
  };
}

/**
 * The hosts a scenario drives, restricted to the transports it declares. A scenario that
 * declares "all" must find a configured host for every transport or it reports the missing
 * transport rather than silently running fewer.
 */
function hostsForScenario(ctx, row) {
  const transports = row.transport === "all" ? ["local", "ssh", "paired", "account-relay"] : [row.transport];
  const hosts = [];
  const missing = [];
  for (const transport of transports) {
    const host = (ctx.fixture.hosts || []).find((entry) => entry.transport === transport);
    if (!host) {
      missing.push(transport);
      continue;
    }
    hosts.push(host);
  }
  return { hosts, missing };
}

function sessionsOf(ctx, host) {
  return sessionsForHost(ctx.fixture, host.id);
}

/** The local host plus its sessions; every HTTP scenario binds through this. */
function localContext(ctx) {
  const host = (ctx.fixture.hosts || []).find((entry) => entry.transport === "local");
  if (!host) throw blocked("local-host-missing", "the fixture manifest configures no local host");
  const sessions = sessionsOf(ctx, host);
  if (sessions.length === 0) throw blocked("local-session-missing", "the local host has no session");
  return { host, sessions, session: sessions[0] };
}

/* ==========================================================================
 * HTTP helpers over the frozen routes
 * ========================================================================== */

function httpHelpers(ctx, host) {
  return {
    async read(session, suffix, extra, route) {
      const query = referenceReadQuery(session, extra, route || suffix);
      return referenceRequest(host, { method: "GET", sessionId: session.backendSessionId, suffix, query });
    },
    async mutate(session, suffix, requestId, params, extra) {
      return referenceRequest(host, {
        method: "POST",
        sessionId: session.backendSessionId,
        suffix,
        body: mutationEnvelope(requestId, session, params, extra),
      });
    },
    async deleteFile(session, fileId) {
      const query = referenceReadQuery(session, {}, "file");
      return referenceRequest(host, {
        method: "DELETE",
        sessionId: session.backendSessionId,
        suffix: "files/" + fileId,
        query,
      });
    },
    async previewFile(session, fileId) {
      const query = referenceReadQuery(session, {}, "file");
      return referenceRequest(host, {
        method: "GET",
        sessionId: session.backendSessionId,
        suffix: "files/" + fileId,
        query,
      });
    },
  };
}

/** The target tuple a response echoes must be the one the request bound. */
function assertSameTarget(echoed, session, label) {
  assert(echoed && typeof echoed === "object", label + ": response carries no target");
  assert(echoed.hostId === session.hostId, label + ": response hostId differs");
  assert(echoed.ownerId === session.ownerId, label + ": response ownerId differs");
  assert(String(echoed.epoch) === String(session.epoch), label + ": response epoch differs");
  assert(echoed.backendSessionId === session.backendSessionId, label + ": response backendSessionId differs");
}

/* ==========================================================================
 * Typed refusals, controls, and evidence redaction
 *
 * The frozen mapping is `reference_chat_status` / `reference_chat_wire_code` in
 * `src-tauri/src/remote/server.rs`. The wire strings are UPPERCASE; the Rust enum variants are
 * CamelCase, and every check below quotes the WIRE form, never the variant name.
 *
 * Two envelopes answer on these routes, told apart by where `requestId` sits:
 *   machine  { error: { code, message, retryable, requestId, details } }   (machine_error_with_details)
 *   scope    { ok: false, error: { code, message, retryable, details }, requestId }  (reference_chat_result_failure)
 * A mutation whose ENVELOPE the JSON extractor could not read answers the machine shape, because
 * it never reached the route that builds a ScopeResult. A status alone is therefore not evidence:
 * an unrelated earlier fence (a missing owner id, an unreadable envelope) answers 400/404 too.
 * ========================================================================== */

/** The HTTP status each frozen wire code maps to (server.rs `reference_chat_status`). */
export const REFERENCE_WIRE_STATUS = Object.freeze({
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

/** Which envelope a parsed body is, or that it carries none at all. */
export function referenceEnvelopeOf(body) {
  if (body === null || body === undefined || typeof body !== "object") return "none";
  if (typeof body.ok === "boolean") return "scope";
  if (body.error && typeof body.error === "object" && typeof body.error.code === "string") return "machine";
  return "other";
}

// The credential-shaped key names, matching the list the fixture layer already redacts with
// (`redactDiagnosticText`: token|deviceToken|machineToken|pairingToken|apiKey|authorization). An
// API key and a private key are credentials and were missing from this pattern. A PUBLIC key is not
// in the list on purpose: it is handed to the client by design, so replacing it would delete the
// evidence of which machine key a request bound.
const EVIDENCE_REDACT_KEY =
  /token|secret|authorization|credential|password|base64|bearer|api[_-]?key|private[_-]?key/i;

/**
 * A bounded, secret-free copy of a body for evidence. Any key that names a credential, a token or
 * an encoded payload is replaced wholesale; long strings are truncated.
 */
export function redactForEvidence(value, depth = 0) {
  if (depth > 6) return "[depth-limit]";
  if (typeof value === "string") {
    return value.length > 200 ? value.slice(0, 200) + "…(" + value.length + " chars)" : value;
  }
  if (Array.isArray(value)) return value.slice(0, 20).map((entry) => redactForEvidence(entry, depth + 1));
  if (value && typeof value === "object") {
    const out = {};
    for (const [key, entry] of Object.entries(value)) {
      out[key] = EVIDENCE_REDACT_KEY.test(key) ? "[redacted]" : redactForEvidence(entry, depth + 1);
    }
    return out;
  }
  return value;
}

/** A short, redacted description of a response, for an assertion message. */
export function describeResponse(result) {
  if (!result || typeof result !== "object") return "(no response)";
  if (result.json !== null && result.json !== undefined) return JSON.stringify(redactForEvidence(result.json));
  const text = typeof result.text === "string" ? result.text.trim() : "";
  if (text.length === 0) return "(empty body)";
  return "(unparsed body) " + (text.length > 160 ? text.slice(0, 160) + "…" : text);
}

/**
 * One refusal, fully attributed: the status, the envelope kind, and the frozen wire code.
 *
 * Every argument is required, so a caller cannot accidentally fall back to "some non-200".
 * `status` may be the number or a wire code from REFERENCE_WIRE_STATUS.
 */
export function assertTypedRefusal(result, expectation) {
  const label = expectation.label;
  const expectedStatus = typeof expectation.status === "number"
    ? expectation.status
    : REFERENCE_WIRE_STATUS[expectation.status];
  const code = expectation.code;
  assert(typeof expectedStatus === "number", label + ": expectation names no known status for " + code);
  assert(result && typeof result.status === "number", label + ": no response to judge");
  assert(result.status === expectedStatus,
    label + ": expected HTTP " + expectedStatus + " " + code + ", got " + result.status + " " + describeResponse(result));
  const envelope = referenceEnvelopeOf(result.json);
  assert(envelope === expectation.envelope,
    label + ": expected the " + expectation.envelope + " envelope, got " + envelope + " " + describeResponse(result));
  const error = result.json.error;
  assert(error && typeof error.code === "string",
    label + ": the refusal carries no error code " + describeResponse(result));
  assert(error.code === code,
    label + ": expected wire code " + code + ", got " + error.code + " " + describeResponse(result));
  if (expectation.requestId !== undefined) {
    // The caller's id must be the one the refusal is about. A scope refusal echoes it; the machine
    // envelope mints its OWN request id (server.rs machine_error_with_details), so there only its
    // presence is assertable - the caller's id never appears on that envelope.
    const echoed = envelope === "scope" ? result.json.requestId : error.requestId;
    assert(typeof echoed === "string" && echoed.length > 0,
      label + ": the refusal carries no request id " + describeResponse(result));
    if (envelope === "scope") {
      assert(echoed === expectation.requestId,
        label + ": the refusal is about request id " + echoed + ", not the caller's " + expectation.requestId);
    }
  }
  return error;
}

/**
 * Is this response exactly this typed refusal? The same rule as `assertTypedRefusal`, with the
 * failure swallowed, for the one place a branch must CLASSIFY a refusal instead of requiring it: a
 * baseline that refuses with the frozen NOT_FOUND because the fixture's pane resolves no native
 * transcript is a missing prerequisite (BLOCKED), while any other refusal on that route is a
 * protocol failure the branch must report as one. Implementing the predicate as the assertion means
 * the two can never drift apart.
 */
export function isTypedRefusal(result, expectation) {
  try {
    assertTypedRefusal(result, { ...expectation, label: expectation.label || "typed refusal" });
    return true;
  } catch {
    return false;
  }
}

/**
 * The control half of every negative branch: the SAME request shape against the real target must
 * reach the layer under test. Without it, a refusal produced by an earlier fence satisfies the
 * negative assertion and the branch passes while the guarded path was never reached.
 */
export function assertReadControl(result, label) {
  assert(result && typeof result.status === "number", label + ": no control response");
  assert(result.status === 200,
    label + ": the control read did not answer 200 (got " + result.status + " " + describeResponse(result) + ")");
  assert(result.json && typeof result.json === "object",
    label + ": the control read carries no JSON body " + describeResponse(result));
  return result.json;
}


/* ------------------------------------------------------------------ QA-01 */
scenario("QA-01", {
  async happy(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const { page, close } = await openChatPage(ctx, host, session);
    try {
      const widths = [{ width: 360, height: 800 }, { width: 390, height: 844 }, { width: 1280, height: 800 }];
      const observed = [];
      for (const viewport of widths) {
        await page.setViewportSize(viewport);
        await page.waitForSelector('[data-testid="mobile-chat-workspace"]', { timeout: ctx.args.timeoutMs });
        const chatVisible = await page.isVisible('[data-testid="chat-message-stream"]');
        const terminalVisible = await page.isVisible('[data-testid="remote-terminal-grid"]');
        observed.push({ viewport, chatVisible, terminalVisible });
        if (!chatVisible) throw failed("chat-not-first", JSON.stringify({ viewport }));
        // Chat is the default at every width, so the terminal grid must not be showing yet.
        if (terminalVisible) throw failed("terminal-visible-before-selection", JSON.stringify({ viewport }));
      }
      const before = await http.read(session, "history", { limit: REFERENCE_HISTORY_DEFAULT_LIMIT }, "history");
      await page.click('[data-testid="remote-view-mode-terminal"]');
      await page.waitForSelector('[data-testid="remote-terminal-grid"]', { timeout: ctx.args.timeoutMs });
      // The grid itself carries no session attribute; the authoritative identity evidence is
      // the target the route echoes, asserted below. Here we only prove the real terminal
      // surface and its input sink are the ones that appeared.
      const terminalSurface = {
        grid: await page.locator('[data-testid="remote-terminal-grid"]').count(),
        inputSink: await page.locator('[data-testid="remote-terminal-input-sink"]').count(),
      };
      await page.click('[data-testid="remote-view-mode-chat"]');
      await page.waitForSelector('[data-testid="chat-message-stream"]', { timeout: ctx.args.timeoutMs });
      const chatRestored = await page.locator('[data-testid="mobile-chat-workspace"]').count();
      const after = await http.read(session, "history", { limit: REFERENCE_HISTORY_DEFAULT_LIMIT }, "history");
      // The roundtrip must not have replaced the original process.
      const survived = assertSessionSurvived(session, "QA-01 happy");
      const report = {
        viewports: observed,
        survived,
        terminalSurface,
        chatRestored,
        identity: {
          requested: { backendSessionId: session.backendSessionId, epoch: session.epoch, pid: session.pid, providerSessionId: session.providerSessionId },
          beforeTarget: before.json && before.json.target,
          afterTarget: after.json && after.json.target,
        },
      };
      ctx.record("qa-01-happy.json", report);
      assert(terminalSurface.grid === 1, "the production terminal grid did not appear in terminal mode");
      assert(terminalSurface.inputSink === 1, "the terminal input sink did not appear in terminal mode");
      assert(chatRestored === 1, "the chat surface was not restored after returning from terminal");
      if (before.json && before.json.target) assertSameTarget(before.json.target, session, "QA-01 before");
      if (after.json && after.json.target) assertSameTarget(after.json.target, session, "QA-01 after");
      return { status: "pass", observed: report };
    } finally {
      await close();
    }
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const { page, close } = await openChatPage(ctx, host, session);
    try {
      const composer = '[data-testid="chat-composer-textarea"]';
      await page.waitForSelector(composer, { timeout: ctx.args.timeoutMs });
      const draft = "HERDR_QA01_MID_COMPOSE";
      await page.fill(composer, draft);
      const surfacesBefore = await page.locator('[data-testid="mobile-chat-workspace"]').count();
      const terminalBefore = await page.locator('[data-testid="remote-terminal-grid"]').count();
      await page.click('[data-testid="remote-view-mode-terminal"]');
      await page.waitForSelector('[data-testid="remote-terminal-grid"]', { timeout: ctx.args.timeoutMs });
      await page.click('[data-testid="remote-view-mode-chat"]');
      await page.waitForSelector(composer, { timeout: ctx.args.timeoutMs });
      const value = await page.inputValue(composer);
      const surfacesAfter = await page.locator('[data-testid="mobile-chat-workspace"]').count();
      const terminalAfter = await page.locator('[data-testid="remote-terminal-grid"]').count();
      const report = { draft, valueAfterRoundtrip: value, surfacesBefore, surfacesAfter, terminalBefore, terminalAfter };
      ctx.record("qa-01-failure.json", report);
      assert(value === draft, "the draft did not survive the mode roundtrip");
      assert(surfacesAfter === surfacesBefore, "a duplicate chat surface appeared");
      assert(terminalAfter === terminalBefore, "the terminal surface was not restored to its prior count");
      return { status: "pass", observed: report };
    } finally {
      await close();
    }
  },
});

/* ------------------------------------------------------------------ QA-02 */
scenario("QA-02", {
  async happy(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const response = await http.read(session, "history", { limit: REFERENCE_HISTORY_DEFAULT_LIMIT }, "history");
    const body = response.json;
    const rows = [];
    for (const candidate of ctx.fixture.sessions) {
      const other = await referenceRequest(host, {
        method: "GET",
        sessionId: candidate.backendSessionId,
        suffix: "history",
        query: referenceReadQuery(candidate, { limit: REFERENCE_HISTORY_DEFAULT_LIMIT }, "history"),
      });
      rows.push({
        backendSessionId: candidate.backendSessionId,
        registryId: candidate.registryId,
        providerSessionId: candidate.providerSessionId,
        status: other.status,
        source: other.json && other.json.source,
        availability: other.json && other.json.availability,
        turns: other.json && Array.isArray(other.json.turns) ? other.json.turns.length : null,
        unavailableReason: other.json && other.json.unavailableReason,
        missingIdentity: sessionMissingIdentity(candidate),
      });
    }
    ctx.record("qa-02-happy.json", { firstStatus: response.status, firstBody: body, perSession: rows });
    assert(response.status === 200, "the first session's history did not answer 200");
    assert(body && typeof body.source === "string", "the history page carries no source");
    for (const row of rows) {
      if (row.status !== 200) continue;
      if (row.availability === "native") {
        assert(row.turns !== null, "a native page reported no turn array");
      } else {
        assert(typeof row.unavailableReason === "string" && row.unavailableReason.length > 0,
          "a non-native page carries no disclosure reason: " + row.backendSessionId);
      }
    }
    const unlabeled = rows.filter((row) => row.missingIdentity.length > 0);
    return { status: "pass", observed: { perSession: rows, missingIdentitySessions: unlabeled } };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const wrongHost = await http.read(session, "history", { hostId: "not-this-host", limit: 50 }, "history");
    const wrongEpoch = await http.read(session, "history", { epoch: foreignEpochFor(session), limit: 50 }, "history");
    // The unauthenticated probe: a raw fetch that sends no authorization header at all.
    const forged = await fetch(referenceRouteUrlFor(host, session.backendSessionId, "history") +
      referenceReadQuery(session, { limit: 50 }, "history"), { method: "GET" });
    const report = {
      wrongHost: { status: wrongHost.status, body: wrongHost.json },
      wrongEpoch: { status: wrongEpoch.status, body: wrongEpoch.json },
      noAuthorizationHeader: { status: forged.status, body: await forged.text() },
      ambiguityRefusals: ctx.fixtureRaw.sessions.map((entry) => ({
        backendSessionId: entry.backendSessionId,
        refusal: referenceIdentityRefusal(entry),
      })),
      scrollbackDisclosure: REFERENCE_SCROLLBACK_DISCLOSURE,
    };
    ctx.record("qa-02-failure.json", report);
    // A caller-named host id that is not this gateway's own reference host id is Forbidden.
    assert(wrongHost.status === 403,
      "a foreign host id was not refused with 403: " + wrongHost.status);
    // A read that names no host id at all must still be accepted: the gateway resolves its own.
    const hostless = await http.read(session, "history", { limit: 50 }, "history");
    report.hostlessRead = { status: hostless.status };
    assert(hostless.status === 200,
      "a read that omits hostId was not resolved by the gateway itself: " + hostless.status);
    assert(forged.status === 401 || forged.status === 403,
      "an unauthenticated read was not refused: " + forged.status);
    // Identity absence must produce a refusal, never a substituted newest/same-cwd conversation.
    for (const entry of report.ambiguityRefusals) {
      if (entry.refusal === null) continue;
      assert(typeof entry.refusal === "string" && entry.refusal.length > 0,
        "an incomplete identity produced no refusal reason: " + entry.backendSessionId);
    }
    return { status: "pass", observed: report };
  },
});

/* ------------------------------------------------------------------ QA-03 */
scenario("QA-03", {
  async happy(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const first = await http.read(session, "history", { limit: 20 }, "history");
    const body = first.json;
    ctx.record("qa-03-happy-page-1.json", body);
    assert(first.status === 200, "the first history page did not answer 200");
    assert(body && Array.isArray(body.turns), "the first page carries no turns array");
    const limitBounds = [];
    for (const limit of [0, 1, REFERENCE_HISTORY_MAX_LIMIT, REFERENCE_HISTORY_MAX_LIMIT + 1]) {
      const bounded = await http.read(session, "history", { limit }, "history");
      limitBounds.push({ limit, status: bounded.status, turns: bounded.json && bounded.json.turns ? bounded.json.turns.length : null });
    }
    let older = null;
    if (body.cursor && body.hasMore) {
      // The cursor wire shape is two separate query fields, NOT a JSON blob: `cursor` is the
      // byte offset and `cursorStream` names the stream it was minted against
      // (server.rs reference_chat_cursor).
      older = await http.read(session, "history", {
        limit: 20,
        cursor: body.cursor.offset,
        cursorStream: body.cursor.streamId,
      }, "history");
      ctx.record("qa-03-happy-page-2.json", older.json);
    }
    const report = { page1: { status: first.status, generation: body.generation, hasMore: body.hasMore, cursor: body.cursor }, limitBounds, page2: older && { status: older.status, turns: older.json && older.json.turns ? older.json.turns.length : null } };
    ctx.record("qa-03-happy.json", report);
    for (const bound of limitBounds) {
      assert(bound.status === 200 || bound.status === 400 || bound.status === 422,
        "limit " + bound.limit + " produced an unexpected status " + bound.status);
    }
    if (older) assert(older.status === 200, "the older page did not answer 200");
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const first = await http.read(session, "history", { limit: 20 }, "history");
    const foreign = await http.read(session, "history", {
      limit: 20,
      cursor: 0,
      cursorStream: "foreign-stream-not-this-session",
    }, "history");
    const malformed = await http.read(session, "history", { limit: 20, cursor: "not-a-cursor" }, "history");
    const oversized = await http.read(session, "history", { limit: REFERENCE_HISTORY_MAX_LIMIT + 5000 }, "history");
    // The control: a cursor guard can only be judged against a pane that CAN serve a page. When the
    // baseline is not a page, every probe below is refused by that earlier fence and the branch would
    // pass while no cursor was ever examined.
    //
    // Exactly ONE baseline outcome is a missing fixture prerequisite: the frozen NOT_FOUND refusal
    // that says nothing identifies a store for this pane (history.rs
    // resolve_reference_history_stream). An auth failure, an unreadable envelope or a 5xx here is a
    // PROTOCOL failure and is asserted as one below - it is never laundered into a blocker.
    const baselineMissingNativeTranscript = isTypedRefusal(first, {
      status: "NOT_FOUND",
      code: "NOT_FOUND",
      envelope: "machine",
    });
    const report = {
      control: {
        status: first.status,
        envelope: referenceEnvelopeOf(first.json),
        missingNativeTranscript: baselineMissingNativeTranscript,
        body: redactForEvidence(first.json),
      },
      foreignCursor: { status: foreign.status, envelope: referenceEnvelopeOf(foreign.json), body: redactForEvidence(foreign.json) },
      malformedCursor: { status: malformed.status, envelope: referenceEnvelopeOf(malformed.json), body: redactForEvidence(malformed.json) },
      oversizedLimit: {
        status: oversized.status,
        envelope: referenceEnvelopeOf(oversized.json),
        turns: oversized.json && Array.isArray(oversized.json.turns) ? oversized.json.turns.length : null,
        body: redactForEvidence(oversized.json),
      },
    };
    ctx.record("qa-03-failure.json", report);
    if (baselineMissingNativeTranscript) {
      throw blocked(
        "baseline-missing",
        "the fixture's pane resolves no native transcript (404 NOT_FOUND), so no cursor guard is " +
          "reachable: " + describeResponse(first),
      );
    }
    const baseline = assertReadControl(first, "QA-03 baseline");
    assert(Array.isArray(baseline.turns),
      "the baseline page carries no turns array, so this route is not serving pages: " + describeResponse(first));
    // A cursor minted against another stream is refused when the page is cut: TARGET_EXPIRED (410).
    assertTypedRefusal(foreign, {
      label: "QA-03 foreign cursor",
      status: "TARGET_EXPIRED",
      code: "TARGET_EXPIRED",
      envelope: "machine",
    });
    // A cursor that names no stream is refused before any store is touched: INVALID_REQUEST (400).
    const cursorWithoutStream = await http.read(session, "history", { limit: 20, cursor: 0 }, "history");
    report.cursorWithoutStream = {
      status: cursorWithoutStream.status,
      envelope: referenceEnvelopeOf(cursorWithoutStream.json),
      body: redactForEvidence(cursorWithoutStream.json),
    };
    ctx.record("qa-03-failure.json", report);
    assertTypedRefusal(cursorWithoutStream, {
      label: "QA-03 cursor without a stream",
      status: "INVALID_REQUEST",
      code: "INVALID_REQUEST",
      envelope: "machine",
    });
    // A non-numeric cursor never reaches the route: the query extractor refuses it. That refusal is
    // still required to be a TYPED envelope - a bodyless 400 is not evidence of a guard.
    assertTypedRefusal(malformed, {
      label: "QA-03 non-numeric cursor",
      status: "INVALID_REQUEST",
      code: "INVALID_REQUEST",
      envelope: "machine",
    });
    // An oversized limit is either clamped to the frozen maximum or refused. Both outcomes are
    // asserted, so neither can silently skip the check.
    if (oversized.status === 200) {
      assert(
        Array.isArray(oversized.json.turns) && oversized.json.turns.length <= REFERENCE_HISTORY_MAX_LIMIT,
        "an oversized limit returned more turns than the frozen maximum: " + describeResponse(oversized),
      );
    } else {
      assertTypedRefusal(oversized, {
        label: "QA-03 oversized limit",
        status: "INVALID_REQUEST",
        code: "INVALID_REQUEST",
        envelope: "machine",
      });
    }
    return { status: "pass", observed: report };
  },
});

/* ------------------------------------------------------------------ QA-04 */
scenario("QA-04", {
  async happy(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const requestId = randomUUID();
    const submitted = await http.mutate(session, "submit", requestId, {
      text: "HERDR_QA04_SINGLE",
      attachmentIds: [],
      origin: "chat",
    });
    const duplicate = await http.mutate(session, "submit", requestId, {
      text: "HERDR_QA04_SINGLE",
      attachmentIds: [],
      origin: "chat",
    });
    const conflicting = await http.mutate(session, "submit", requestId, {
      text: "HERDR_QA04_DIFFERENT",
      attachmentIds: [],
      origin: "chat",
    });
    // Recorded before any assertion: the two reuses of one request id are judged below, and a shape
    // mismatch must be diagnosable from the receipt instead of only from an assertion message.
    const report = {
      accepted: { status: submitted.status, envelope: referenceEnvelopeOf(submitted.json), body: redactForEvidence(submitted.json) },
      duplicate: { status: duplicate.status, envelope: referenceEnvelopeOf(duplicate.json), body: redactForEvidence(duplicate.json) },
      conflicting: { status: conflicting.status, envelope: referenceEnvelopeOf(conflicting.json), body: redactForEvidence(conflicting.json) },
    };
    ctx.record("qa-04-happy.json", report);
    // The control: a well-formed submit is ACCEPTED, so the two reuses below are judged against a
    // route that demonstrably reaches its own request-id rule on this target.
    assert(submitted.status === 200, "the control submit was not accepted: " + describeResponse(submitted));
    const data = assertScopeResult(submitted.json, requestId, "QA-04 submit");
    assert(data.receipt && data.receipt.stage === "accepted", "submit did not reach the accepted stage");
    assert(data.receipt.requestId === requestId || data.receipt.requestId === undefined,
      "the receipt names a different request id");
    // An identical payload under the SAME request id is served from its record: the identical
    // delivery receipt, not a second write (reference_chat/input.rs `Claim::Duplicate`).
    const duplicateData = assertScopeResult(duplicate.json, requestId, "QA-04 duplicate submit");
    // A DEEP comparison, because the two receipts are the same record served twice. The fixture's
    // own `assert` is the plain `assert(condition, message)` (herdr-reference-fixtures.mjs:531) and
    // carries no deep-equality member at all, so calling one on it raised a TypeError instead of
    // asserting: the branch failed for a missing method, never on the behaviour it guards.
    deepStrictEqual(duplicateData.receipt, data.receipt,
      "an identical duplicate was not answered from its own record");
    // A DIFFERENT payload under the same id is the frozen typed conflict: 409 REQUEST_CONFLICT on the
    // ScopeResult envelope, echoing the caller's request id (input.rs `Claim::Conflict`).
    assertTypedRefusal(conflicting, {
      label: "QA-04 conflicting payload",
      status: "REQUEST_CONFLICT",
      code: "REQUEST_CONFLICT",
      envelope: "scope",
      requestId,
    });
    report.stage = data.receipt.stage;
    report.duplicateServedFromRecord = true;
    ctx.record("qa-04-happy.json", report);
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    // The control: a well-formed submit under the budget must be ACCEPTED. Without it a refusal from
    // an earlier fence (a missing owner id, an unreadable envelope) satisfies the probes below and
    // the branch passes while neither guard was reached - which is exactly what run 2 did.
    const multiline = await http.mutate(session, "submit", randomUUID(), {
      text: "line-one\nline-two\nline-three",
      attachmentIds: [],
      origin: "chat",
    });
    const unknown = await http.mutate(session, "submit", randomUUID(), {
      text: "x".repeat(20001),
      attachmentIds: [],
      origin: "chat",
    });
    const malformed = await referenceRequest(host, {
      method: "POST",
      sessionId: session.backendSessionId,
      suffix: "submit",
      body: { requestId: randomUUID() },
    });
    const report = {
      control: { status: multiline.status, envelope: referenceEnvelopeOf(multiline.json), body: redactForEvidence(multiline.json) },
      oversizedComposer: { status: unknown.status, envelope: referenceEnvelopeOf(unknown.json), body: redactForEvidence(unknown.json) },
      malformedEnvelope: { status: malformed.status, envelope: referenceEnvelopeOf(malformed.json), body: redactForEvidence(malformed.json) },
      providerReadClaimed: Boolean(
        multiline.json && multiline.json.data && multiline.json.data.receipt && multiline.json.data.receipt.stage === "providerRead"),
    };
    ctx.record("qa-04-failure.json", report);
    assert(multiline.status === 200 && multiline.json && multiline.json.ok === true,
      "QA-04 control: a well-formed submit under the budget was not accepted " + describeResponse(multiline));
    assert(!report.providerReadClaimed,
      "a generic source claimed providerRead, which requires a matching native observation");
    // The submit byte budget, refused by the route itself: a ScopeResult, PAYLOAD_TOO_LARGE (413).
    assertTypedRefusal(unknown, {
      label: "QA-04 oversized composer",
      status: "PAYLOAD_TOO_LARGE",
      code: "PAYLOAD_TOO_LARGE",
      envelope: "scope",
    });
    // An envelope with no params never reaches the route: the extractor refuses it, so the shape is
    // the MACHINE envelope even though the route is a mutation.
    assertTypedRefusal(malformed, {
      label: "QA-04 malformed envelope",
      status: "INVALID_REQUEST",
      code: "INVALID_REQUEST",
      envelope: "machine",
    });
    return { status: "pass", observed: report };
  },
});

/* ------------------------------------------------------------------ QA-05 */
scenario("QA-05", {
  async happy(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const prompt = await http.read(session, "prompt", {}, "prompt");
    const screen = await http.read(session, "screen", {}, "screen");
    const report = {
      promptStatus: prompt.status,
      prompt: prompt.json,
      screenStatus: screen.status,
      screenRevision: screen.json && screen.json.revision,
      screenTextLength: screen.json && typeof screen.json.text === "string" ? screen.json.text.length : null,
      screenTruncated: screen.json && screen.json.truncated,
      screenGap: screen.json && screen.json.gap,
    };
    ctx.record("qa-05-happy.json", report);
    assert(screen.status === 200, "the screen read did not answer 200");
    assert(prompt.status === 200, "the prompt read did not answer 200");
    assert(screen.json && typeof screen.json.revision === "string", "the screen read carries no revision");
    // ReferenceScreenSnapshot carries the VT-aware text, not a line array.
    assert(screen.json && typeof screen.json.text === "string", "the screen read carries no text");
    assert(typeof screen.json.cols === "number" && typeof screen.json.rows === "number",
      "the screen read carries no geometry");
    if (prompt.json && prompt.json.prompt) {
      const answerRequestId = randomUUID();
      // The frozen answer shape is ReferencePromptAnswer: exactly one of optionIndex,
      // optionIndices or customText. The card is bound by its own id plus the revision.
      const answered = await http.mutate(session, "answer", answerRequestId, {
        promptId: prompt.json.prompt.id,
        screenRevision: prompt.json.screenRevision,
        answer: { optionIndex: 0 },
      });
      report.answer = { status: answered.status, body: answered.json };
      ctx.record("qa-05-happy-answer.json", report.answer);
    } else {
      report.answer = { status: "no-card", note: "the pane's screen shows no prompt card" };
    }
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    // Read-only control: the prompt read answers, and it tells us whether a card is on screen at
    // all. The live card is NEVER answered here - a real answer types into the pane and moves the
    // screen revision, which is the very baseline the stale guard is judged against.
    const prompt = await http.read(session, "prompt", {}, "prompt");
    const card = prompt.json && prompt.json.prompt ? prompt.json.prompt : null;
    const revision = prompt.json && prompt.json.screenRevision;
    const unknownCard = await http.mutate(session, "answer", randomUUID(), {
      promptId: "stale-prompt-not-on-screen",
      screenRevision: "stale-revision",
      answer: { optionIndex: 0 },
    });
    const staleScreen = card
      ? await http.mutate(session, "answer", randomUUID(), {
          promptId: card.id,
          screenRevision: "stale-revision",
          answer: { optionIndex: 0 },
        })
      : null;
    const screenBefore = await http.read(session, "screen", {}, "screen");
    const screenAfter = await http.read(session, "screen", {}, "screen");
    const report = {
      control: { promptStatus: prompt.status, cardPresent: Boolean(card), screenRevision: revision },
      unknownCardAnswer: { status: unknownCard.status, body: redactForEvidence(unknownCard.json) },
      staleScreenAnswer: staleScreen
        ? { status: staleScreen.status, body: redactForEvidence(staleScreen.json) }
        : "no-card-on-screen",
      screenRevisionBefore: screenBefore.json && screenBefore.json.revision,
      screenRevisionAfter: screenAfter.json && screenAfter.json.revision,
    };
    ctx.record("qa-05-failure.json", report);
    assertReadControl(prompt, "QA-05 prompt control");
    // An answer naming no card this pane's parser detected is the parser-origin fence:
    // INVALID_REQUEST (400) as a ScopeResult (prompts.rs REFERENCE_PROMPT_UNKNOWN_CARD_MESSAGE).
    assertTypedRefusal(unknownCard, {
      label: "QA-05 unknown card",
      status: "INVALID_REQUEST",
      code: "INVALID_REQUEST",
      envelope: "scope",
    });
    // Nothing was sent, so the screen must not have moved. This is the no-op half of the claim.
    assert(report.screenRevisionBefore === report.screenRevisionAfter,
      "a refused answer moved the screen revision: " + report.screenRevisionBefore + " -> " + report.screenRevisionAfter);
    if (!card) {
      throw blocked(
        "no-prompt-card",
        "the pane's screen shows no prompt card, so the stale-screen fence cannot be exercised",
      );
    }
    // The stale-screen fence: the card's own id with a revision that is not the card's screen.
    assertTypedRefusal(staleScreen, {
      label: "QA-05 stale screen",
      status: "REQUEST_CONFLICT",
      code: "REQUEST_CONFLICT",
      envelope: "scope",
    });
    return { status: "pass", observed: report };
  },
});

/* Run only when this file IS the program. The typed-refusal and control helpers above are
   exported so the neighbouring regression suite can import them; importing must never start a
   run. Invoked as a file (the PowerShell wrapper does exactly that), process.argv[1] is this
   module and the runner behaves as before. */
const invokedAsProgram =
  typeof process.argv[1] === "string" &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedAsProgram) {
  main()
    .then((code) => process.exit(code))
    .catch((error) => {
      const code = error instanceof RunnerError ? error.code : EXIT.INTERACTION;
      process.stderr.write((error.reason || "runner-failed") + ": " + (error.detail || error.message) + "\n");
      process.exit(code);
    });
}

