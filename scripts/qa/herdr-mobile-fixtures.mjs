import { spawn } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, existsSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { createHash, randomUUID } from "node:crypto";
import { createInterface } from "node:readline";
import { EventEmitter } from "node:events";

export const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export function sha256(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }
export function deadline(promise, label, ms = 20000) {
  let timer;
  return Promise.race([promise, new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(label + " timed out")), ms);
  })]).finally(() => clearTimeout(timer));
}
export function assert(condition, message) { if (!condition) throw new Error(message); }
export function setupIsolatedProfile() {
  const root = mkdtempSync(join(tmpdir(), "ferryx-herdr-"));
  const paths = {};
  for (const name of ["data", "runtime", "session", "home"]) {
    paths[name] = join(root, name); mkdirSync(paths[name]);
  }
  return { root, paths, runId: randomUUID(), env: {
    ...process.env, FERRYX_QA_ISOLATED: "1", FERRYX_DATA_DIR: paths.data,
    FERRYX_RUNTIME_DIR: paths.runtime, FERRYX_SESSION_DIR: paths.session,
    HOME: paths.home, USERPROFILE: paths.home,
  }, cleanup() { rmSync(root, { recursive: true }); } };
}
export async function spawnRustGatewayFixture({ fixtureBinary, profile, allowHost = false }) {
  assert(process.platform === "win32" && allowHost === true && profile.env.FERRYX_QA_ISOLATED === "1", "Gateway requires an explicitly authorized Windows host and isolated profile");
  assert(existsSync(fixtureBinary), "Build the composed fixture on maho-win before acceptance");
  const events = new EventEmitter();
  const child = spawn(fixtureBinary, [], { cwd: repoRoot, env: profile.env, stdio: ["pipe", "pipe", "pipe"] });
  let stderr = "";
  child.stderr.on("data", data => { stderr = (stderr + data).slice(-16000); });
  const exit = new Promise((resolveExit, reject) => {
    child.once("error", reject); child.once("exit", (code, signal) => resolveExit({code, signal}));
  });
  async function terminateOwnedTree() {
    if (child.exitCode !== null || child.signalCode !== null || !child.pid) return;
    const killer = spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"], {stdio:"ignore"});
    await deadline(new Promise((resolveKill,reject) => {
      killer.once("error",reject); killer.once("exit",code=>code===0 ? resolveKill() : reject(new Error("Owned PID tree cleanup failed")));
    }), "owned process tree cleanup");
    await deadline(exit,"owned fixture exit");
  }
  const lines = createInterface({input: child.stdout});
  lines.on("line", line => {
    try { const value = JSON.parse(line); events.emit(value.event, value); }
    catch { /* Non-JSON library logs are not protocol events. */ }
  });
  function next(event) {
    let listener;
    const eventPromise = new Promise(resolveEvent => { listener = resolveEvent; events.once(event, listener); });
    return deadline(Promise.race([eventPromise, exit.then(result => { throw new Error("Fixture exited: " + JSON.stringify(result) + " " + stderr); })]), event)
      .finally(() => events.off(event, listener));
  }
  let readyInfo;
  try { readyInfo = await next("ready"); }
  catch (error) { await terminateOwnedTree(); throw error; }
  return {readyInfo, next, async command(command, event) {
    const observed = next(event); child.stdin.write(JSON.stringify(command) + "\n"); return observed;
  }, async close() {
    if (child.exitCode !== null) throw new Error("Fixture exited before teardown receipt");
    const stopped = next("stopped");
    child.stdin.end(JSON.stringify({type:"shutdown"}) + "\n");
    try { const receipt = await stopped; const result = await deadline(exit, "fixture shutdown");
      assert(result.code === 0 && receipt.cleanupErrors.length === 0, "Fixture cleanup failed"); return receipt;
    } finally { await terminateOwnedTree(); lines.close(); }
  }};
}
export function validateConsumerReceipt(receiptPath, candidate) {
  if (!receiptPath) return {status:"UNMET_PREREQUISITE", reason:"Same-candidate machine_two_consumer_harness receipt required"};
  const receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
  const logPath = resolve(dirname(receiptPath), receipt.logPath);
  const log = readFileSync(logPath, "utf8");
  const names = ["two_consumer_gateway_healthy_controller_progresses_while_viewer_stalls_and_drops", "two_consumer_controller_exclusivity_second_controller_conflicts"];
  const sameCandidate = receipt.candidateId === candidate.candidateId && receipt.sourceManifestSha256 === candidate.sourceManifestSha256;
  const actualRun = receipt.host?.toLowerCase() === "maho-win" && receipt.exitCode === 0 &&
    receipt.command?.includes("machine_two_consumer_harness") && sha256(logPath) === receipt.logSha256 &&
    names.every(name => log.split(/\r?\n/).some(line => line.startsWith("test ") && line.endsWith(name + " ... ok"))) && /test result: ok\./.test(log);
  return {status: sameCandidate && actualRun ? "PASS" : "FAIL", sameCandidate, actualRun, receiptSha256:sha256(receiptPath), logSha256:sha256(logPath)};
}
export function validateAndroidDeviceEvidence(directory, candidate) {
  if (!directory || !existsSync(join(directory, "result.json"))) return {status:"UNMET_PREREQUISITE", reason:"Native runner result.json absent"};
  const result = JSON.parse(readFileSync(join(directory,"result.json"),"utf8"));
  const provenancePath = join(directory,"prereq","provenance.json");
  const provenance = existsSync(provenancePath) ? JSON.parse(readFileSync(provenancePath,"utf8")) : null;
  // A separate verifier binds native captures to the served candidate, not this script's HEAD.
  const bindingPath = join(directory,"candidate-binding.json");
  const binding = existsSync(bindingPath) ? JSON.parse(readFileSync(bindingPath,"utf8")) : null;
  const bound = binding?.candidateId === candidate.candidateId && binding?.sourceManifestSha256 === candidate.sourceManifestSha256 && binding?.resultSha256 === sha256(join(directory,"result.json"));
  const artifacts = Array.isArray(binding?.artifacts) && binding.artifacts.length > 0 && binding.artifacts.every(artifact =>
    typeof artifact.path === "string" && sha256(resolve(directory,artifact.path)) === artifact.sha256);
  const required = ["direct-once","mode-switch","composition-enter","target-switch"];
  const scenarios = Array.isArray(result.scenarios) ? result.scenarios : [];
  const requested = Array.isArray(result.scenariosRequested) ? result.scenariosRequested : [];
  const count = (text, token) => text.split(token).length - 1;
  const captureChecks = Object.fromEntries(required.map(name => {
    const rows = scenarios.filter(s => s.name === name);
    const s = rows[0], c = s?.captured, a = s?.assertions;
    if (rows.length !== 1 || s.status !== "pass" || !c || !a ||
      typeof c.baselineGridText !== "string" || typeof c.finalGridText !== "string") return [name,false];
    if (name === "target-switch") return [name,
      c.chatDraftTyped === "한" && c.chatDraftRetained === c.chatDraftTyped &&
      count(c.finalGridText,"한") === count(c.baselineGridText,"한") && a.sendPressed === false];
    const token = {"direct-once":"모바일","mode-switch":"한글","composition-enter":"가다"}[name];
    const once = count(c.finalGridText,token)-count(c.baselineGridText,token) === 1 && a.hangulToken === token;
    if (name === "direct-once") return [name,once && typeof c.preEnterGridText === "string" && c.preeditAfterEnter === ""];
    if (name === "mode-switch") return [name,once && c.draftAtSwitch === token && c.draftRestored === token && c.finalDraft === "" &&
      [c.typedGridText,c.directSwitchGridText,c.backToLineGridText].every(text=>text===c.baselineGridText)];
    return [name,once && c.draftBefore === token && c.typedGridText === c.baselineGridText && c.finalDraft === "" &&
      typeof c.afterFirstEnterGridText === "string" && typeof c.draftAfterFirstEnter === "string" &&
      ["committed-and-submitted-in-one","absorbed-by-ime-no-send"].includes(a.firstEnterFinding)];
  }));
  const cleanup = result.cleanup;
  const cleanupVerified = Array.isArray(cleanup?.createdForwards) && Array.isArray(cleanup?.createdReverses) && Array.isArray(cleanup?.removals) &&
    cleanup.removals.every(r=>r.exit===0) &&
    [["adb-forward",cleanup.createdForwards],["adb-reverse",cleanup.createdReverses]].every(([type,ids])=>
      ids.every(id=>cleanup.removals.filter(r=>r.type===type && r.id===id && r.exit===0).length===1));
  const provenanceVerified = result.runner?.host?.platform === "darwin" &&
    result.runner?.scriptSha256 === "04239f4e7250d328c3f34026468cae0da9bcd69ac32cb3ae049e5aa7710af04d" &&
    result.device?.serial === "R3CN8126R4Y" && result.device?.model === "SM-N981N" && result.device?.androidRelease === "13" &&
    result.device?.adbState === "device" && result.chrome?.versionName?.startsWith("154.") &&
    result.ime?.defaultIme?.includes("honeyboard") && result.ime?.enabled?.includes(result.ime.defaultIme) &&
    typeof result.page?.urlRedacted === "string" && result.page?.cdp &&
    provenance?.device?.serial === result.device.serial && provenance?.ime?.defaultIme === result.ime.defaultIme;
  const complete = result.schema === "ferryx-herdr-native-ime.result/1" && result.verdict === "pass" && result.exitCode === 0 &&
    required.every(name=>requested.includes(name)) && requested.length === scenarios.length && new Set(requested).size === requested.length &&
    scenarios.every(s=>requested.includes(s.name) && s.status === "pass") && Object.values(captureChecks).every(Boolean) &&
    provenanceVerified && cleanupVerified && bound && artifacts;
  return {status:complete ? "PASS" : "UNMET_PREREQUISITE", provenance, bound, artifacts, captureChecks,cleanupVerified,provenanceVerified:Boolean(provenanceVerified),
    resultSha256:sha256(join(directory,"result.json")), reason:complete ? "Captured native payloads and provenance validated" : "Native captures, cleanup, provenance, or candidate binding incomplete"};
}
export function validateSpeechEvidence(directory, candidate) {
  void directory;
  void candidate;
  return {status:"DEFERRED",reason:"Voice is deferred by user decision; no device evidence was collected"};
}
export function writeReport(evidenceDir, value) {
  mkdirSync(evidenceDir, {recursive:true});
  writeFileSync(join(evidenceDir,"report.json"), JSON.stringify(value,null,2));
}
