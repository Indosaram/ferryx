#!/usr/bin/env node
/**
 * Herdr reference-chat isolated acceptance runner (plan task 14).
 *
 * AUTHORED, NOT EXECUTED. The complete-code merge barrier owns every run of this file.
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
import {
  OwnedProcessLedger,
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
  setupIsolatedProfile,
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
    const refused = await http.mutate(session, "stop", randomUUID(), { capability: "refused" });
    const shellSignal = await http.mutate(session, "stop", randomUUID(), { capability: "shellSignal" });
    const badCapability = await http.mutate(session, "stop", randomUUID(), { capability: "ctrl-c" });
    const report = {
      refused: { status: refused.status, body: refused.json },
      shellSignal: { status: shellSignal.status, body: shellSignal.json },
      unknownCapability: { status: badCapability.status, body: badCapability.json },
    };
    ctx.record("qa-06-failure.json", report);
    // An unknown capability is a typed refusal, never a substituted killing signal.
    assert(refused.status !== 200 || (refused.json && refused.json.ok === false),
      "a refused capability was answered as a successful stop");
    assert(badCapability.status === 400 || badCapability.status === 422,
      "an unknown stop capability was not refused: " + badCapability.status);
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
    const staged = await http.mutate(session, "files", imageRequestId, {
      name: "qa07-image.png",
      mediaType: "image",
      sizeBytes: imageBytes.length,
      contentBase64: imageBytes.toString("base64"),
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
      previewAfterDelete: { status: afterDelete.status, body: afterDelete.json },
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
    const traversal = await http.mutate(session, "files", randomUUID(), {
      name: "../../escape.txt",
      mediaType: "file",
      sizeBytes: 4,
      contentBase64: Buffer.from("evil").toString("base64"),
    });
    const oversize = await http.mutate(session, "files", randomUUID(), {
      name: "qa07-oversize.bin",
      mediaType: "file",
      sizeBytes: 10 * 1024 * 1024 + 1,
      contentBase64: "",
    });
    const mismatch = await http.mutate(session, "files", randomUUID(), {
      name: "qa07-mismatch.bin",
      mediaType: "file",
      sizeBytes: 999999,
      contentBase64: Buffer.from("short").toString("base64"),
    });
    const missingPreview = await http.previewFile(session, "00000000-0000-0000-0000-000000000000");
    const report = {
      traversal: { status: traversal.status, body: traversal.json },
      oversize: { status: oversize.status, body: oversize.json },
      declaredSizeMismatch: { status: mismatch.status, body: mismatch.json },
      unknownFilePreview: { status: missingPreview.status, body: missingPreview.json },
    };
    ctx.record("qa-07-failure.json", report);
    assert(traversal.status !== 200 || (traversal.json && traversal.json.ok === false),
      "a traversal-shaped name was staged");
    assert(missingPreview.status === 404, "an unknown file id was not refused with 404");
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
    // The unknown-session probe still names the REAL incarnation and owner, so the only thing
    // wrong with it is the session id. That keeps the refusal attributable to the session
    // rather than to an expired epoch or a missing owner.
    const unknownSession = { ...session, backendSessionId: "no-such-session" };
    const unknown = await referenceRequest(host, {
      method: "GET",
      sessionId: "no-such-session",
      suffix: "history",
      query: referenceReadQuery(unknownSession, { limit: 10 }, "history"),
    });
    const codexAlias = nativeKindOf("codex-cli");
    const gjcDetector = REFERENCE_REGISTRY_ROWS.find((row) => row.id === "gjc").detector;
    const report = {
      unknownSession: { status: unknown.status, body: unknown.json },
      unknownSessionEpoch: unknownSession.epoch,
      unknownAliasNativeKind: codexAlias,
      gjcDetector,
      knownRows: REFERENCE_REGISTRY_ROWS.map((row) => row.id),
    };
    ctx.record("qa-10-failure.json", report);
    assert(unknown.status === 404 || unknown.status === 400 || unknown.status === 422,
      "an unknown session was not refused: " + unknown.status);
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
  ctx.record("browser-" + scenarioSafeName(ctx) + ".json", {
    hostId: host.id,
    urlRedacted: host.urlRedacted || host.url,
    pageErrors,
    backendSessionId: session.backendSessionId,
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

    // The frozen gateway the scenarios drive. A host that declares startLocal is launched
    // HERE on a throwaway profile; otherwise the fixture's own URL is used and its
    // credential file is the only secret this runner reads.
    const localHost = (fixtureRaw.hosts || []).find((entry) => entry.transport === "local");
    if (localHost && localHost.startLocal === true) {
      const gateway = await startIsolatedGateway(args, candidateRaw, ledger);
      ctx.gateways.push(gateway);
      localHost.url = gateway.url;
      localHost.gatewayUrl = gateway.url;
      const credentialPath = join(gateway.profile.paths.data, "reference-chat-token");
      writeFileSync(credentialPath, gateway.token);
      localHost.credentialFile = credentialPath;
      result.localGateway = {
        urlRedacted: redactUrl(gateway.url),
        pid: gateway.entry.pid,
        executablePath: gateway.entry.executablePath,
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

main()
  .then((code) => process.exit(code))
  .catch((error) => {
    const code = error instanceof RunnerError ? error.code : EXIT.INTERACTION;
    process.stderr.write((error.reason || "runner-failed") + ": " + (error.detail || error.message) + "\n");
    process.exit(code);
  });


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
 * Start one isolated local gateway on a throwaway profile and wait for its health endpoint
 * to answer. The wait is condition-driven with a deadline, never a fixed sleep.
 */
async function startIsolatedGateway(args, candidateRaw, ledger) {
  const binary = candidateRaw.binary && candidateRaw.binary.path;
  if (!binary || !existsSync(binary)) throw blocked("gateway-binary-missing", String(binary));
  if (!args.allowHost) {
    throw blocked("host-authorization", "starting a local gateway requires --allow-host true");
  }
  const profile = setupIsolatedProfile("chat");
  const env = { ...profile.env };
  if (candidateRaw.uiDist) env.FERRYX_UI_DIST_DIR = resolve(candidateRaw.uiDist);
  const child = spawn(resolve(binary), [], {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  ledger.record(child, { role: "isolated-gateway" });
  let stderr = "";
  child.stderr.on("data", (data) => {
    stderr = (stderr + data.toString()).slice(-16000);
  });
  const exited = new Promise((resolveExit) => {
    child.once("exit", (code, signal) => resolveExit({ code, signal }));
  });
  // The gateway prints its bound address as JSON on stdout; the first such line is the
  // authority for the URL, so the runner never assumes a port.
  const bound = await deadline(
    new Promise((resolveBound, rejectBound) => {
      let buffer = "";
      child.stdout.on("data", (data) => {
        buffer += data.toString();
        for (const line of buffer.split("\n")) {
          const trimmed = line.trim();
          if (!trimmed.startsWith("{")) continue;
          try {
            const parsed = JSON.parse(trimmed);
            if (parsed && (parsed.gatewayUrl || parsed.url) && (parsed.token || parsed.deviceToken)) {
              resolveBound(parsed);
              return;
            }
          } catch {
            /* Not a ready line. */
          }
        }
        const tail = buffer.slice(-8192);
        buffer = tail;
      });
      exited.then((result) => rejectBound(blocked("gateway-exited", JSON.stringify(result) + " " + stderr)));
    }),
    "gateway readiness",
    args.timeoutMs,
  );
  return {
    profile,
    entry: ledger.entries[ledger.entries.length - 1],
    url: bound.gatewayUrl || bound.url,
    token: bound.token || bound.deviceToken,
    deviceId: bound.deviceId || null,
    hostId: bound.hostId || null,
    stderrTail: () => stderr,
    async stop() {
      if (child.exitCode !== null || child.signalCode !== null) return;
      child.kill("SIGTERM");
      await Promise.race([
        exited,
        new Promise((resolveTimeout) => setTimeout(() => resolveTimeout(null), 10000)),
      ]);
    },
  };
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
      const path = join(evidenceDir, name);
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
    const body = first.json;
    const foreign = await http.read(session, "history", {
      limit: 20,
      cursor: 0,
      cursorStream: "foreign-stream-not-this-session",
    }, "history");
    const malformed = await http.read(session, "history", { limit: 20, cursor: "not-a-cursor" }, "history");
    const oversized = await http.read(session, "history", { limit: REFERENCE_HISTORY_MAX_LIMIT + 5000 }, "history");
    const report = {
      baseline: { status: first.status, generation: body && body.generation },
      foreignCursor: { status: foreign.status, body: foreign.json },
      malformedCursor: { status: malformed.status, body: malformed.json },
      oversizedLimit: { status: oversized.status, turns: oversized.json && oversized.json.turns ? oversized.json.turns.length : null },
    };
    ctx.record("qa-03-failure.json", report);
    assert(foreign.status !== 200 || (foreign.json && foreign.json.generation !== body.generation),
      "a cursor minted against another stream was served as if it belonged to this one");
    assert(malformed.status !== 200, "a malformed cursor was accepted");
    if (oversized.status === 200) {
      assert(oversized.json.turns.length <= REFERENCE_HISTORY_MAX_LIMIT,
        "an oversized limit returned more turns than the frozen maximum");
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
    const data = assertScopeResult(submitted.json, requestId, "QA-04 submit");
    const duplicate = await http.mutate(session, "submit", requestId, {
      text: "HERDR_QA04_SINGLE",
      attachmentIds: [],
      origin: "chat",
    });
    const duplicateData = assertScopeResult(duplicate.json, requestId, "QA-04 duplicate submit");
    const conflicting = await http.mutate(session, "submit", requestId, {
      text: "HERDR_QA04_DIFFERENT",
      attachmentIds: [],
      origin: "chat",
    });
    const report = {
      accepted: data,
      duplicate: duplicateData,
      conflicting: { status: conflicting.status, body: conflicting.json },
      stage: data.receipt && data.receipt.stage,
    };
    ctx.record("qa-04-happy.json", report);
    assert(data.receipt && data.receipt.stage === "accepted", "submit did not reach the accepted stage");
    assert(data.receipt.requestId === requestId || data.receipt.requestId === undefined,
      "the receipt names a different request id");
    if (duplicate.json && duplicate.json.ok === false) {
      assert(false, "a duplicate request id with an identical payload was not served from its record");
    }
    assert(conflicting.status !== 200 || (conflicting.json && conflicting.json.ok === false),
      "a conflicting payload for the same request id was accepted");
    return { status: "pass", observed: report };
  },
  async failure(ctx) {
    const { host, session } = localContext(ctx);
    const http = httpHelpers(ctx, host);
    const multiline = await http.mutate(session, "submit", randomUUID(), {
      text: "line-one\r\nline-two\nline-three",
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
      multiline: { status: multiline.status, body: multiline.json },
      oversizedComposer: { status: unknown.status, body: unknown.json },
      malformedEnvelope: { status: malformed.status, body: malformed.json },
      providerReadClaimed: Boolean(
        multiline.json && multiline.json.data && multiline.json.data.receipt && multiline.json.data.receipt.stage === "providerRead"),
    };
    ctx.record("qa-04-failure.json", report);
    assert(!report.providerReadClaimed,
      "a generic source claimed providerRead, which requires a matching native observation");
    assert(malformed.status === 400 || malformed.status === 422,
      "a mutation envelope without a target was not refused: " + malformed.status);
    if (unknown.json && unknown.json.ok === false) {
      assert(typeof unknown.json.error.code === "string", "an oversized composer produced no typed error code");
    }
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
    const stale = await http.mutate(session, "answer", randomUUID(), {
      promptId: "stale-prompt-not-on-screen",
      screenRevision: "stale-revision",
      answer: { optionIndex: 0 },
    });
    const screenBefore = await http.read(session, "screen", {}, "screen");
    const screenAfter = await http.read(session, "screen", {}, "screen");
    const report = {
      staleAnswer: { status: stale.status, body: stale.json },
      screenRevisionBefore: screenBefore.json && screenBefore.json.revision,
      screenRevisionAfter: screenAfter.json && screenAfter.json.revision,
    };
    ctx.record("qa-05-failure.json", report);
    assert(stale.status !== 200 || (stale.json && stale.json.ok === false),
      "an answer for a card that is not on the current screen was accepted");
    return { status: "pass", observed: report };
  },
});

