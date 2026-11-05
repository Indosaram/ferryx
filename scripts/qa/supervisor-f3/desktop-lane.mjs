// Desktop lane (macOS or Windows) of the F3 acceptance run.
//
// Setup goes over SSH: the existing scripts/qa/fixtures/native-remote-agent.mjs (--rpc) checks
// the built candidate against its manifest, makes the isolated `.native-qa/<runId>` profile and
// launches the candidate daemon with exact-PID receipts; host-helper.mjs (--rpc) reports the
// machine and checkout identity and speaks the daemon protocol over that profile's endpoint.
// Every GUI action and every screenshot goes through MahoRD (connectNativeMahord). Windows
// launches the GUI through the Run dialog typed by MahoRD, as native-configured-adapters.mjs
// does; macOS opens a run-owned launcher bundle through LaunchServices (host-helper
// `open-gui-bundle`), because an SSH session has no window server and macOS has no typeable run
// prompt. Either way the native agent's `--launch-gui` spawns the candidate with the isolated env
// and writes the exact-pid receipts. Clicks land on points located by OCR of the real frame, and
// every assertion that names on-screen text is checked against tesseract output of that frame.
//
// Unavailable host = LaneBlocked HOST_UNAVAILABLE (the SSH identity probe failed) or
// MAHORD_UNAVAILABLE (MahoRD connect failed), never a skip. Anything after a reachable host
// answered (wrong revision or stamp, relay, daemon, GUI, product) is a lane failure. Cleanup
// ends the GUI and daemon by exact pid identity, then removes only the profile this run made.
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connectNativeMahord, rpcProcess } from '../fixtures/native-configured-adapters.mjs';
import { FRONTEND_DEV_URL } from '../../lib/qa-scenarios/frontend-server.mjs';
import { connectRemoteAgent } from './remote-agent.mjs';
import { LaneBlocked, StepFailed, recordIdentity, markNotRun, runStep, stopExact, until } from './steps.mjs';
import { scenariosForLane } from './contract.mjs';
import { acceptedReply, configureGateway, framePoint, panelReadRequest, compactOcrShows, workspaceProblem, ocrShows, reportGoalRequest, decisionsNamingNonce, gatewayPortFor, hubEnv, isolationProblems, globalRetryRule, namesNonce, rowFocusVerdict, rpcCommand, rulesLineText, typedInto, uniqueSession, windowRect } from './seams.mjs';
import { terminalTokenShown, singleLineOcrShows, scrollDelta, startupDsrObserved } from './seams.mjs';
import { decodePng, encodePng, ocrImage } from './png.mjs';

/**
 * OCR words with boxes (tesseract TSV) of one frame on the control host. With `rect` (the
 * run-owned window in frame pixels) tesseract reads that window cut out, gray, inverted and
 * doubled (png.mjs; the dark panel is unreadable otherwise), written next to the frame as
 * `<frame>.ocr-input.png`; word boxes are mapped back to frame pixels for clicks.
 */
async function ocr(tesseract, pngPath, rect = null, singleLine = false) {
  let input = pngPath; let map = (x, y) => ({ x, y }); let scale = 1;
  if (rect) {
    const image = ocrImage(readFileSync(pngPath), rect, singleLine ? 3 : 2);
    if (singleLine) {
      const gray = decodePng(image.png);
      for (let i = 0; i < gray.px.length; i += 1) gray.px[i] = Math.max(0, Math.min(255, (gray.px[i] - 20) * 255 / 190));
      image.png = encodePng(gray.width, gray.height, 1, gray.px);
    }
    input = pngPath.replace(/\.png$/, singleLine ? '.ocr-line-input.png' : '.ocr-input.png');
    writeFileSync(input, image.png);
    scale = image.scale;
    map = (x, y) => ({ x: image.origin.x + x / image.scale, y: image.origin.y + y / image.scale });
  }
  return ocrFile(tesseract, input, map, scale, singleLine);
}

async function ocrFile(tesseract, pngPath, map, scale, singleLine = false) {
  const tsv = await new Promise((resolveOcr, reject) => {
    const child = spawn(tesseract, [pngPath, 'stdout', ...(singleLine ? [...(process.env.F3_TERMINAL_TESSDATA ? ['--tessdata-dir', process.env.F3_TERMINAL_TESSDATA] : []), '--psm', '7', '-c', 'load_system_dawg=0', '-c', 'load_freq_dawg=0'] : []), '-c', 'tessedit_create_tsv=1'], { stdio: ['ignore', 'pipe', 'pipe'] });
    let out = ''; let err = '';
    child.stdout.on('data', (d) => { out += d; });
    child.stderr.on('data', (d) => { err += d; });
    child.once('error', reject);
    child.once('close', (code) => (code === 0 ? resolveOcr(out) : reject(new StepFailed('OCR_FAILED', err))));
  });
  const words = tsv.split('\n').slice(1).map((line) => line.split('\t')).filter((f) => f.length >= 12 && f[11].trim())
    .map((f) => ({ text: f[11].trim(), ...map(Number(f[6]), Number(f[7])), w: Number(f[8]) / scale, h: Number(f[9]) / scale, line: `${f[2]}.${f[3]}.${f[4]}` }));
  const lines = new Map();
  for (const word of words) lines.set(word.line, [...(lines.get(word.line) ?? []), word]);
  return { tsv, words, text: [...lines.values()].map((ws) => ws.map((w) => w.text).join(' ')).join('\n') };
}

/** Centre of the first OCR line containing `phrase` (case-insensitive), or null. */
function locate(read, phrase, { below = -1 } = {}) {
  const wanted = phrase.toLowerCase().split(/\s+/);
  const lines = new Map();
  for (const word of read.words) lines.set(word.line, [...(lines.get(word.line) ?? []), word]);
  for (const words of lines.values()) {
    const texts = words.map((w) => w.text.toLowerCase());
    for (let i = 0; i + wanted.length <= texts.length; i += 1) {
      if (wanted.every((part, k) => texts[i + k].includes(part))) {
        const span = words.slice(i, i + wanted.length);
        const x1 = Math.min(...span.map((w) => w.x)); const x2 = Math.max(...span.map((w) => w.x + w.w));
        const y1 = Math.min(...span.map((w) => w.y)); const y2 = Math.max(...span.map((w) => w.y + w.h));
        if (y1 > below) return { x: Math.round((x1 + x2) / 2), y: Math.round((y1 + y2) / 2), box: { x1, y1, x2, y2 } };
      }
    }
  }
  return null;
}

/** The isolated-profile keys a live GUI process must carry with exactly the run's values. */
const ISOLATED_ENV = ['FERRYX_QA_ISOLATED', 'FERRYX_QA_RUN_ID', 'FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_SESSION_DIR', 'HOME'];

export async function runDesktopLane(lane, { host, run, shared, signal }) {
  const ids = scenariosForLane(lane.name);
  let agent; let helper; let mahord; let setup; let daemon; let gui; let wrappers;
  const ssh = (script, onEvent) => rpcProcess(...rpcCommand(host, script, run.sshExecutable), { signal, onEvent });
  try {
    // 0. Host reachability: only an unanswered identity probe makes the host unavailable.
    try {
      helper = ssh(host.hostHelper);
      const who = await helper.call('identity', { checkout: host.worktree });
      recordIdentity(lane, who);
    } catch (error) {
      throw new LaneBlocked('HOST_UNAVAILABLE', `ssh ${host.sshDestination} (${host.hostId}): ${error.message}`);
    }
    const harnessRevision = run.harnessRevision ?? run.revision;
    if (lane.receipt.harness?.headRevision !== harnessRevision) throw new StepFailed('HARNESS_REVISION_MISMATCH', `${host.hostId} harness is at ${lane.receipt.harness?.headRevision}, expected ${harnessRevision}`);
    if (lane.receipt.harness.clean !== true) throw new StepFailed('HARNESS_DIRTY', `${host.hostId} harness checkout is not clean: ${lane.receipt.harness.status}`);
    try {
      mahord = await connectNativeMahord({ executable: run.mahordExecutable, address: host.mahordAddress, pairingId: host.pairingId, nudgeMs: host.mahordNudgeMs, display: host.mahordDisplay, codec: host.mahordCodec, signal });
    } catch (error) {
      throw new LaneBlocked('MAHORD_UNAVAILABLE', `MahoRD ${host.mahordAddress} (${host.hostId}): ${error.message}`);
    }

    if (lane.receipt.candidate.headRevision !== run.revision) throw new StepFailed('REVISION_MISMATCH', `${host.hostId} checkout is at ${lane.receipt.candidate.headRevision}, run revision ${run.revision}`);

    // 1. Isolated profile + candidate daemon through the existing native agent. The build
    // stamp the agent checked against the manifest must be this revision plus its patch hash.
    agent = await connectRemoteAgent({ helper, host, run, lane, connect: ssh });
    setup = await agent.call('prepare', { worktreeRoot: host.worktreeRoot, worktree: host.worktree, runId: run.runId, operationId: `f3-${run.runId}`,
      binary: host.binary, uiDist: host.uiDist, manifest: host.buildManifest });
    lane.receipt.candidate.stamp = setup.candidate;
    lane.receipt.candidate.binarySha256 = setup.binarySha256;
    lane.receipt.candidate.uiSha256 = setup.uiSha256;
    if (typeof setup.candidate !== 'string' || !setup.candidate.startsWith(`${run.revision}+`)) throw new StepFailed('CANDIDATE_STAMP', `${host.hostId} candidate stamp ${setup.candidate} is not ${run.revision}+<patch>`);
    const project = await helper.call('prepare-project', { profile: setup.profile, binary: setup.binary, uiDist: host.uiDist });
    setup.repo = project.repo;
    setup.guiBinary = project.binary;
    setup.frontendDist = project.uiDist;
    lane.write('diagnostic', 'project-setup.json', project);
    wrappers = await helper.call('write-wrappers', { profile: setup.profile, repo: setup.repo });
    // The isolated daemon's gateway gets a run-owned free port: production on this host may
    // already own 43821, and a refused bind must stop the lane, never be stepped over.
    const gatewayPort = gatewayPortFor(host.hostId, await helper.call('free-port', {}));
    const env = { ...setup.env, ...hubEnv(host, setup.profile), FERRYX_RELAY_URL: shared.relayOrigin, FERRYX_MACHINE_TOKEN: shared.machineToken(host.hostId), FERRYX_ACCOUNT_ORIGIN: shared.relayOrigin,
      FERRYX_QA_GATEWAY_PORT: String(gatewayPort) };
    // The lane typing the owner prompt runs the supervisor engine (mhc) against a loopback stub
    // provider: without an installed engine the composer's message is refused MHC_NOT_RUNNING
    // and nothing is stored (hub_runtime.rs supervisor_converse).
    const typing = lane.name === (run.lanes.includes('windows') ? 'windows' : 'macos');
    if (typing && host.mhcBin) {
      const engine = await helper.call('engine-stub', { profile: setup.profile, mhcBin: host.mhcBin });
      lane.write('engine', 'engine-stub.json', engine);
      Object.assign(env, engine.env);
    }
    setup.env = env;
    daemon = await agent.call('spawn-daemon', { setup, runId: run.runId, operationId: `f3-${run.runId}`, hostId: host.hostId, timeoutMs: 120_000 });
    lane.write('gui-receipt', 'daemon-launch.json', daemon);
    const exchange = async (label, frames, deadlineMs) => {
      const replies = await helper.call('daemon', { profile: setup.profile, frames, deadlineMs });
      lane.write('daemon-io', `daemon/${Date.now()}-${label}.json`, { label, frames, replies });
      return replies;
    };
    // Every daemon call is checked: a structured error reply stops the step or the lane.
    const call = async (label, frames, deadlineMs) => acceptedReply(label, await exchange(label, frames, deadlineMs));
    const enrolled = await shared.enroll({ host, helper, profile: setup.profile, env, cli: host.cli });
    lane.write('account', 'enroll.json', enrolled);
    acceptedReply('register', await exchange('register', [{ type: 'registerWorkspace', workspaceId: 'f3', repoRoot: setup.repo }]), 'registerWorkspaceOk');
    acceptedReply('agent-control', await exchange('agent-control', [{ type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] }]), 'agentControlSettingsOk');
    lane.write('relay', 'gateway.json', await configureGateway({ call: exchange, port: gatewayPort, relayOrigin: shared.relayOrigin }));
    // The supervisor's owner: the app's own setup (the "Turn on the supervisor" action) run on
    // this daemon, which provisions the local account as the one owner and maps the worktree.
    // Without an owner every composer message is refused UNAUTHORIZED.
    lane.write('supervisor', 'setup.json', await call('supervisor-setup', [{ type: 'supervisorSetup', request: { action: 'run', importMemory: false, projects: [{ name: 'f3', path: setup.repo }] } }]));
    if (typing && host.mhcBin) {
      const ready = await until('the supervisor engine ready', async () => (await exchange('engine-status', [{ type: 'supervisorEngine', request: { action: 'status' } }])).at(-1),
        (r) => r?.type === 'supervisorEngineOk' && r.snapshot?.engine?.state === 'ready', { deadlineMs: 180_000, gapMs: 2_000 });
      lane.write('supervisor', 'engine-status.json', ready);
    }
    if (shared.panesOnDesktop && lane.name === (run.lanes.includes('windows') ? 'windows' : 'macos')) {
      shared.hostPanes({ call, helper, setup, wrappers });
      await shared.spawnPanes();
    }

    // 2. GUI through MahoRD only (the same launcher path native-configured-adapters uses).
    gui = await launchGui({ agent, helper, mahord, setup, host, run, lane });

    // Nothing is clicked or typed until the live GUI process is proven to run on this run's
    // isolated profile (never the owner's HOME or data dir), read from the process itself.
    const isolation = { expected: Object.fromEntries(ISOLATED_ENV.map((key) => [key, setup.env[key]])), profile: setup.profile };
    const live = await until('the live GUI on the isolated profile', () => helper.call('process-isolation', { pid: gui.pid }),
      (seen) => isolationProblems(seen, { ...isolation, ownerRoots: seen.ownerRoots }).length === 0, { deadlineMs: 60_000, gapMs: 2_000 })
      .catch(async (error) => { if (error.code === 'DEADLINE') return helper.call('process-isolation', { pid: gui.pid }); throw error; });
    const isolationIssues = isolationProblems(live, { ...isolation, ownerRoots: live.ownerRoots });
    lane.write('gui-receipt', 'gui-isolation.json', { pid: gui.pid, ...isolation, ownerRoots: live.ownerRoots, envRead: Array.isArray(live.env), openPaths: live.openPaths, problems: isolationIssues });
    if (isolationIssues.length) throw new StepFailed('GUI_NOT_ISOLATED', isolationIssues.join('; '));
    if (host.platform === 'win32') {
      const front = await agent.call('setup-target', { profile: setup.profile, gui, boundsOnly: true });
      lane.write('gui-receipt', 'gui-before-maximize.json', front);
      if (!front.bounds || front.frontPid !== gui.pid) throw new StepFailed('GUI_NOT_FRONT', `run-owned GUI pid ${gui.pid} not frontmost before maximize: ${JSON.stringify(front)}`);
      await mahord.hotkey(['Meta', 'ArrowUp'], signal);
    }
    // macOS: the run-owned GUI is started by a background launcher, so it never takes the front
    // by itself and the owner's own windows can cover it. It is raised (by its exact pid) before
    // every frame, so every OCR read and every click lands on the run's own window.
    const raise = async () => {
      if (host.platform !== 'darwin') return;
      const raised = await helper.call('activate-gui', { pid: gui.pid, executable: gui.executable });
      if (raised.activated !== true) throw new StepFailed('GUI_NOT_FRONT', `run-owned GUI pid ${gui.pid} was not raised: ${JSON.stringify(raised)}`);
    };
    // macOS: MahoRD types ASCII as key codes, which an input method (e.g. Korean 2-Set) composes
    // into other characters; ABC is selected while the lane types, then the owner's source again.
    const typeText = async (text, enter = false) => {
      const switched = host.platform === 'darwin' ? await helper.call('input-source', { id: 'com.apple.keylayout.ABC' }) : null;
      try {
        await mahord.type(text, signal);
        if (enter) await mahord.key('Enter', signal);
      } finally {
        if (switched?.previous && switched.previous !== 'com.apple.keylayout.ABC') await helper.call('input-source', { id: switched.previous });
      }
    };
    if (run.smoke) {
      // Transport smoke: one real frame and its OCR, then the lane stops and cleans up.
      await raise();
      const shot = lane.write('screenshot', 'smoke/first-frame.png', await mahord.screenshot(signal));
      const read = await ocr(run.tesseract, join(run.evidenceDir, shot.path));
      lane.write('ocr', 'smoke/first-frame.ocr.tsv', read.tsv);
      lane.receipt.smoke = { guiPid: gui.pid, envRead: Array.isArray(live.env), openPathsUnderProfile: Array.isArray(live.openPaths) ? live.openPaths.filter((p) => p.startsWith(`${setup.profile}/`)).length : null, frame: shot, ocrWords: read.words.length };
      return;
    }
    // A frame: real MahoRD pixels plus their OCR. Attached to a step only when `step` is given,
    // so a polled frame attaches exactly the frame its assertion read (see `keep`).
    // macOS: OCR reads the run-owned window only (its live bounds, read by exact pid).
    const windowOf = async (frameWidth) => {
      const seen = host.platform === 'win32'
        ? await agent.call('setup-target', { profile: setup.profile, gui, boundsOnly: true })
        : await helper.call('gui-bounds', { pid: gui.pid, executable: gui.executable });
      // Read right after the capture: a frame taken while another app was in front is never OCRed.
      if (!seen.bounds || seen.frontPid !== gui.pid) throw new StepFailed('GUI_NOT_FRONT', `run-owned GUI pid ${gui.pid} not frontmost with a window at capture: ${JSON.stringify(seen)}`);
      return windowRect(seen.bounds, seen.screen, frameWidth);
    };
    // MahoRD returns its latest streamed frame, which can still show the windows that were in
    // front before the raise. A frame counts only when the cropped run window shows this run's
    // own text (`marker`: the worktree name its cards and rows carry, or the Viewer marker);
    // otherwise it is taken again, and after the attempts the step fails GUI_NOT_FRONT.
    const runMarker = 'Supervisor';
    // OCR reads a middle dot as '-' and 1 as i: text is compared with those folded (seams ocrShows).
    const shows = ocrShows;
    let frameSize; let windowFrame; let captureSequence = 0;
    const click = (point) => mahord.click(framePoint(point, frameSize.width, frameSize.height), signal);
    const scrollPanel = (direction, pixels) => mahord.scroll({ ...framePoint({ x: windowFrame.x + Math.min(180, windowFrame.width * 0.10), y: windowFrame.y + windowFrame.height * 0.55 }, frameSize.width, frameSize.height), dx: 0, dy: scrollDelta(host.platform, direction, pixels) }, signal);
    const frame = async (name, step, { marker = runMarker } = {}) => {
      for (let attempt = 1; ; attempt += 1) {
        await raise();
        const frameId = await mahord.waitForFrame(signal);
        const bytes = await mahord.screenshot(signal);
        frameSize = { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
        const rect = await windowOf(bytes.readUInt32BE(16));
        windowFrame = rect;
        const captureName = `${name}-${++captureSequence}`;
        const shot = step ? step.artifact('screenshot', `${captureName}.png`, bytes) : lane.write('screenshot', `frames/${captureName}.png`, bytes);
        const observation = { captureName, frameId, screenshot: shot.path };
        if (step) step.artifact('frame-observation', `${captureName}.frame.json`, observation);
        else lane.write('frame-observation', `frames/${captureName}.frame.json`, observation);
        const read = await ocr(run.tesseract, join(run.evidenceDir, shot.path), rect);
        if (name.endsWith('terminal-ready')) {
          const header = await ocr(run.tesseract, join(run.evidenceDir, shot.path),
            { x: rect.x + rect.width * 0.2 - 4, y: rect.y + rect.height * 0.07, width: rect.width * 0.7, height: rect.height * 0.08 }, true);
          read.text += `\n${header.text}`;
          read.words.push(...header.words);
        }
        if (step) step.artifact('ocr', `${captureName}.ocr.tsv`, read.tsv);
        if (workspaceProblem(read.text)) throw new StepFailed('HARNESS_WORKSPACE_UNAVAILABLE', read.text.slice(0, 1800));
        if (host.platform !== 'darwin' || !marker || shows(read.text, marker)) return { shot, read, bytes, observation };
        if (attempt >= 5) throw new StepFailed('GUI_NOT_FRONT', `frame ${name}: the run window text "${marker}" is not in the cropped frame after ${attempt} captures (a stale or covered frame)`);
        await new Promise((resolveWait) => { setTimeout(resolveWait, 1_000); });
      }
    };
    const keep = (step, name, seen) => {
      const captureName = `${name}-${++captureSequence}`;
      const shot = step.artifact('screenshot', `${captureName}.png`, seen.bytes);
      step.artifact('frame-observation', `${captureName}.frame.json`, { ...seen.observation, captureName, sourceCaptureName: seen.observation.captureName, screenshot: shot.path });
      step.artifact('ocr', `${captureName}.ocr.tsv`, seen.read.tsv);
      return seen;
    };
    // First launch discovers the other linked checkouts. Dismiss only the import prompt actually
    // observed in the owned window, through the same real MahoRD click used by the clauses.
    const initial = await until('owned supervisor frontend rendered', () => frame('initial-import', null, { marker: null }),
      ({ read }) => shows(read.text, 'Supervisor') || locate(read, "Don't import") !== null, { deadlineMs: 90_000 });
    const dismissImport = locate(initial.read, "Don't import");
    if (dismissImport) {
      await click(dismissImport);
      await until('initial import dialog dismissed', () => frame('import-dismissed', null, { marker: null }),
        ({ read }) => !locate(read, "Don't import"), { deadlineMs: 30_000 });
    }
    const panel = async (step, name = 'poll') => {
      const reply = await call(`panel-${name}`, [panelReadRequest('panel')]);
      if (step) step.artifact('panel-read', `${name}.panel.json`, reply);
      return reply?.value ?? null;
    };
    const transcript = async (step, name = 'poll') => {
      const reply = await call(`transcript-${name}`, [panelReadRequest('transcript')]);
      if (step) step.artifact('transcript-read', `${name}.transcript.json`, reply);
      return reply?.value ?? null;
    };
    const visible = async (step, name, predicate, { scroll = false } = {}) => {
      let attempt = 0;
      return keep(step, name, await until(name, async () => {
        const seen = await frame(`${name}-${++attempt}`);
        await panel(step, `${name}-${attempt}`);
        if (scroll && !predicate(seen.read)) await scrollPanel('down', 520);
        return seen;
      }, ({ read }) => predicate(read), { deadlineMs: 30_000 }));
    };
    const sendComposer = async (step, text, token) => {
      const seen = await visible(step, 'composer-visible', (r) => locate(r, 'Tell the supervisor') !== null, { scroll: true });
      await click(locate(seen.read, 'Tell the supervisor'));
      await typeText(text);
      await visible(step, 'composer-after-type', (r) => compactOcrShows(r.text, token));
      await mahord.key('Enter', signal);
      await frame('composer-after-enter', step);
    };
    shared.desktops[lane.name] = { lane, frame, panel, transcript, call, mahord, setup, helper, wrappers, gui };

    // F3-D1: the shared decision card is on screen. The card's session lives on the session
    // host's daemon; both desktops show the fleet's card.
    await runStep(lane, 'F3-D1', async (step) => {
      step.artifact('gui-receipt', 'gui-launch.json', gui);
      const card = await shared.decisionCard();
      const view = await until('decision card in this desktop panel', () => panel(null), (v) => (v?.decisions ?? []).some((d) => d.sessionRef === card.sessionRef), { deadlineMs: 120_000 });
      step.artifact('panel-read', 'decision.panel.json', view);
      const shown = keep(step, 'decision', await until('decision card text on screen', () => frame('decision'), ({ read }) => read.text.includes(card.commandFragment), { deadlineMs: 60_000 }));
      const decision = view.decisions.find((d) => d.sessionRef === card.sessionRef);
      shared.record('F3-D1', lane.name, { sessionRef: decision.sessionRef, promptId: decision.promptId, question: decision.question, command: decision.command });
      step.assert('card-is-the-fleet-decision', decision.promptId === card.promptId, { decision, card });
      step.assert('card-command-on-screen', shown.read.text.includes(card.commandFragment), { fragment: card.commandFragment });
      step.assert('recommended-option-on-screen', /\bYes\b/.test(shown.read.text) && /\bNo\b/.test(shown.read.text), { text: shown.read.text.slice(0, 2000) });
      await frame('decision-final', step);
    });

    // F3-D2: one owner prompt, typed into the Windows desktop's composer through MahoRD, and the
    // supervisor's Reply: both on each desktop's screen and in each desktop's transcript read.
    await runStep(lane, 'F3-D2', async (step) => {
      const prompt = shared.prompt;
      const token = shared.promptToken;
      // One desktop types the prompt: Windows when it is in the run, else macOS.
      if (lane.name === (run.lanes.includes('windows') ? 'windows' : 'macos')) {
        await frame('before-prompt', step);
        await sendComposer(step, prompt, token);
      }
      const rows = await until('prompt and reply in the transcript', () => transcript(null), (page) => pairOf(page, prompt) !== null, { deadlineMs: 180_000 });
      step.artifact('transcript-read', 'transcript.json', rows);
      const pair = pairOf(rows, prompt);
      const after = keep(step, 'after-prompt', await until('prompt and reply on screen', () => frame('after-prompt'), ({ read: r }) => shows(r.text, token) && shows(r.text, firstWords(pair.reply.text)), { deadlineMs: 60_000 }));
      shared.record('F3-D2', lane.name, { owner: pair.owner.text, reply: pair.reply.text });
      step.assert('prompt-row-stored', pair.owner.text === prompt, pair);
      step.assert('reply-row-stored', pair.reply.text.length > 0, pair);
      step.assert('prompt-on-screen', shows(after.read.text, token), { token, text: after.read.text.slice(0, 2000) });
      step.assert('reply-on-screen', shows(after.read.text, firstWords(pair.reply.text)), { reply: pair.reply.text, text: after.read.text.slice(0, 2000) });
      await frame('reply-final', step);
    });

    // F3-D3: transcript export from the app downloads a real file into this profile's
    // Downloads directory, and the file holds this desktop's prompt and Reply.
    await runStep(lane, 'F3-D3', async (step) => {
      const downloads = join(setup.profile, 'home', 'Downloads');
      const before = new Set((await helper.call('list-files', { profile: setup.profile, dir: downloads })).map((f) => f.name));
      const { read } = await visible(step, 'before-export', (r) => locate(r, host.exportLabel ?? 'Export transcript') !== null, { scroll: true });
      const button = locate(read, host.exportLabel ?? 'Export transcript');
      if (!button) throw new StepFailed('PRODUCT_GAP_EXPORT_CONTROL', `no "${host.exportLabel ?? 'Export transcript'}" control on the supervisor panel`);
      await click(button);
      const done = (f) => f.file && !before.has(f.name) && f.bytes > 0 && !/\.(crdownload|part|partial|download|tmp)$/i.test(f.name);
      // The panel says where the export went ("Saved <path>") or why it failed, under the
      // Conversation header (TranscriptSection.tsx transcript-export-status); that frame is kept.
      const files = await until('a finished new file in Downloads', () => helper.call('list-files', { profile: setup.profile, dir: downloads }), (list) => list.filter(done).length === 1, { deadlineMs: 60_000 })
        .catch(async (error) => {
          const seen = keep(step, 'export-status', await frame('export-status'));
          const status = seen.read.text.split('\n').find((line) => /\b(Saved|not saved|Export|Downloaded|failed|cancelled)\b/i.test(line) && !/^\s*Conversation\b/.test(line)) ?? null;
          throw new StepFailed(error.code ?? 'DEADLINE', `${error.message}; panel status after the click: ${JSON.stringify(status)}; Downloads ${downloads}`);
        });
      const fresh = files.find(done);
      const file = await helper.call('read-b64', { profile: setup.profile, path: fresh.path });
      const bytes = Buffer.from(file.base64, 'base64');
      step.artifact('export-file', `export/${fresh.name}`, bytes);
      const rows = await transcript(step, 'at-export');
      const pair = pairOf(rows, shared.prompt);
      const text = bytes.toString('utf8');
      step.assert('export-holds-prompt', pair !== null && text.includes(pair.owner.text), { name: fresh.name, bytes: bytes.length });
      step.assert('export-holds-reply', pair !== null && text.includes(pair.reply.text), { name: fresh.name });
      await frame('after-export', step);
    });

    // F3-D4: the session's long report (sent by the fixture agent through the product's
    // supervisorReport path) opens in Viewer: before the click its middle is not on screen; after
    // it the heading, a middle section, and the end with a Viewer-only section are proven.
    await runStep(lane, 'F3-D4', async (step) => {
      const token = `${lane.name.slice(0, 3)}${run.runId.replace(/[^a-z0-9]/g, '').slice(-8)}`;
      const heading = `F3 long report ${token}`;
      const end = `End of report ${token}`;
      // A worker report is stored only for a pane an open goal is bound to: the owner's request
      // binds the reporting pane first (supervisorDispatch submit with that pane).
      const admitted = await call('report-goal', [reportGoalRequest(run.runId, shared.reportPane())]);
      step.artifact('transcript-read', 'report-goal.admission.json', admitted);
      if (admitted.result?.outcome !== 'admitted') throw new StepFailed('REPORT_GOAL_NOT_ADMITTED', JSON.stringify(admitted).slice(0, 1500));
      await shared.sendLongReport(token);
      const replies = await until('the actual daemon report acknowledgement', () => shared.agentLog('upstream'),
        (entries) => entries.some((entry) => entry.kind === 'supervisor-report' && entry.token === token), { deadlineMs: 30_000 });
      step.artifact('fixture-log', 'supervisor-report.agent.jsonl', replies.map((entry) => JSON.stringify(entry)).join('\n'));
      const receipt = replies.find((entry) => entry.kind === 'supervisor-report' && entry.token === token);
      step.assert('daemon-accepted-report', JSON.parse(receipt.reply).ok === true, { reply: receipt.reply });
      const rows = await until('the long report in the transcript', () => transcript(null), (page) => JSON.stringify(page ?? {}).includes(heading), { deadlineMs: 120_000 });
      step.artifact('transcript-read', 'report.transcript.json', rows);
      const { read } = await visible(step, 'before-viewer', (r) => locate(r, host.viewerLabel ?? 'Open in Viewer') !== null && compactOcrShows(r.text, heading), { scroll: true });
      // The panel's report row shows the first lines and the last line (reportExcerpt), so the
      // proof the report is not open yet is a middle section only the Viewer shows, and no Viewer.
      const middle = 'Migration check 9 for';
      // Section 2's body is absent from the row excerpt's first three nonblank lines.
      const headMiddle = 'Migration check 2 for';
      const viewerMarker = 'Viewer · Supervisor report';
      step.assert('report-not-open-before-viewer', !shows(read.text, middle) && !singleLineOcrShows(read.text, viewerMarker), { middle, viewerMarker, text: read.text.slice(0, 1500) });
      const open = locate(read, host.viewerLabel ?? 'Open in Viewer');
      if (!open) throw new StepFailed('PRODUCT_GAP_VIEWER_CONTROL', `no "${host.viewerLabel ?? 'Open in Viewer'}" control next to the report`);
      await click(open);
      const head = keep(step, 'viewer-head', await until('report heading in Viewer', () => frame('viewer-head', undefined, { marker: viewerMarker }), ({ read: r }) => compactOcrShows(r.text, heading) && shows(r.text, headMiddle) && singleLineOcrShows(r.text, viewerMarker), { deadlineMs: 60_000 }));
      step.assert('viewer-shows-report-heading', compactOcrShows(head.read.text, heading) && shows(head.read.text, headMiddle), { heading, headMiddle, text: head.read.text.slice(0, 1500) });
      step.assert('viewer-marker-shown', singleLineOcrShows(head.read.text, viewerMarker), { marker: viewerMarker });
      // The document has focus on open. PageDown navigates the real Viewer without clicking
      // its table of contents (which can itself show section titles but not their body text).
      const body = keep(step, 'viewer-middle', await until('report middle in Viewer', async () => {
        const seen = await frame('viewer-middle', undefined, { marker: viewerMarker });
        if (!shows(seen.read.text, middle)) await mahord.key('PageDown', signal);
        return seen;
      }, ({ read: r }) => shows(r.text, middle) && singleLineOcrShows(r.text, viewerMarker), { deadlineMs: 60_000 }));
      step.assert('viewer-shows-report-middle', shows(body.read.text, middle) && singleLineOcrShows(body.read.text, viewerMarker), { middle, text: body.read.text.slice(0, 1500) });
      await mahord.hotkey(host.platform === 'darwin' ? ['Meta', 'ArrowDown'] : ['Control', 'End'], signal);
      const nearEnd = 'Migration check 40 for';
      const tail = keep(step, 'viewer-tail', await until('report end in Viewer', () => frame('viewer-tail', undefined, { marker: viewerMarker }), ({ read: r }) => compactOcrShows(r.text, end) && shows(r.text, nearEnd) && singleLineOcrShows(r.text, viewerMarker), { deadlineMs: 60_000 }));
      step.assert('viewer-shows-report-end', compactOcrShows(tail.read.text, end) && shows(tail.read.text, nearEnd) && singleLineOcrShows(tail.read.text, viewerMarker), { end, nearEnd, marker: viewerMarker, text: tail.read.text.slice(0, 1500) });
    });
    // The Viewer is a modal over the panel; later steps read the panel, so it is closed (Escape,
    // ReportViewer.tsx onKeyDown) whatever D4's outcome.
    await mahord.key('Escape', signal).catch(() => {});

    // F3-D6: one session row, clicked through MahoRD, focuses that session's terminal. Two of
    // this desktop's own instrumented panes (unique titles on this computer), spawned by this
    // desktop's daemon: `away` and `focus`. Focus is first put on `away` through its row and
    // proven there by a probe that fully arrives in the away pane's log (keys go to one focused
    // target, in order), so the focus pane cannot already hold focus. Then the focus row is
    // clicked and a token typed after it must arrive in the focus pane and show on screen.
    await runStep(lane, 'F3-D6', async (step) => {
      const workspace = await helper.call('project-head', { profile: setup.profile, repo: setup.repo });
      if (!workspace) throw new StepFailed('HARNESS_WORKSPACE_UNAVAILABLE', `No committed QA project at ${setup.repo}`);
      const panes = {};
      for (const key of ['away', 'focus']) {
        const pane = wrappers.wrappers[key];
        const spawned = await call(`spawn-${key}`, [{ type: 'spawn', clientRequestId: randomUUID(), workspaceId: 'f3', worktree: null, cwd: pane.cwd, cols: 100, rows: 24, shell: pane.shell }]);
        if (spawned.type !== 'spawnOk') throw new StepFailed('FIXTURE_PANES', `spawn ${key}: ${JSON.stringify(spawned)}`);
        panes[key] = { title: pane.title, sessionId: spawned.sessionId };
        if (pane.shell.toLowerCase().endsWith('.cmd')) {
          const attached = await until(`ConPTY startup for ${key}`, () => call(`startup-${key}`, [{ type: 'attach', sessionId: spawned.sessionId, afterSequence: 0 }]),
            startupDsrObserved, { deadlineMs: 30_000 });
          step.artifact('fixture-startup', `startup-${key}.json`, attached);
          step.artifact('known-gap', `startup-${key}-gap.json`, { gap: 'Harness answers observed ConPTY DSR; F3-D6 does not prove unaided pane startup.' });
          await call(`startup-answer-${key}`, [{ type: 'write', sessionId: spawned.sessionId, data: Buffer.from('\x1b[1;1R').toString('base64') }]);
        }
      }
      const log = async (key, after = 0) => {
        // The row shows as soon as the pane exists; its fixture agent creates the log a moment
        // later. A log not created yet holds no entries.
        const name = `agent-${panes[key].title}.jsonl`;
        if (!(await helper.call('list-files', { profile: setup.profile, dir: wrappers.logs })).some((f) => f.name === name)) return [];
        const file = await helper.call('read-b64', { profile: setup.profile, path: join(wrappers.logs, name) });
        return Buffer.from(file.base64, 'base64').toString('utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line)).slice(after);
      };
      const both = (v) => ['away', 'focus'].every((key) => uniqueSession(v, panes[key].title) !== null);
      const view = await until('the away and focus rows in the panel', () => panel(null), both, { deadlineMs: 120_000 });
      step.artifact('panel-read', 'focus.panel.json', view);
      step.assert('row-in-panel', ['away', 'focus'].every((key) => uniqueSession(view, panes[key].title)?.sessionRef?.endsWith(`:${panes[key].sessionId}`) === true),
        { away: uniqueSession(view, panes.away.title), focus: uniqueSession(view, panes.focus.title), panes });
      const suffix = run.runId.replace(/[^a-z0-9]/g, '').slice(-6);
      const probe = `f3probe${suffix}`;
      const token = `f3token${suffix}`;
      const clickRow = async (key, name) => {
        const { read } = await visible(step, name, (r) => locate(r, panes[key].title) !== null, { scroll: true });
        const row = locate(read, panes[key].title);
        if (!row) throw new StepFailed('ROW_NOT_ON_SCREEN', `no session row "${panes[key].title}" on screen`);
        await click(row);
      };
      // Focus on `away`, proven by the probe arriving there in full before the focus row is clicked.
      const awayMark = (await log('away')).length;
      await clickRow('away', 'before-away-click');
      keep(step, 'away-terminal-ready', await until('away terminal ready after the row click', () => frame('away-terminal-ready'),
        ({ read }) => compactOcrShows(read.text, `${panes.away.title} is idle`), { deadlineMs: 90_000 }));
      await typeText(probe);
      await frame('after-away-click-and-type', step);
      const awayEntries = await until('the probe in the away pane', async () => {
        const entries = await log('away', awayMark);
        step.artifact('fixture-log', 'away-probe.json', entries);
        return entries;
      }, (list) => typedInto(list, probe), { deadlineMs: 30_000 });
      step.artifact('fixture-log', 'away.agent.jsonl', (await log('away')).map((e) => JSON.stringify(e)).join('\n'));
      const focusBefore = await log('focus');
      const elsewhere = rowFocusVerdict({ away: awayEntries, focus: focusBefore, probe, token }).focusWasElsewhere;
      step.assert('keys-in-away-pane-before-click', elsewhere, { probe });
      if (!elsewhere) throw new StepFailed('FOCUS_NOT_ELSEWHERE', `the probe did not land in ${panes.away.title} alone`);
      // Every probe key was taken by `away`, so these entries were logged before the focus click.
      const focusMark = focusBefore.length;
      await clickRow('focus', 'before-row-click');
      keep(step, 'focus-terminal-ready', await until('focus terminal ready after the row click', () => frame('focus-terminal-ready'),
        ({ read }) => compactOcrShows(read.text, `${panes.focus.title} is idle`), { deadlineMs: 90_000 }));
      await typeText(token);
      const entries = await until('typed token in the focus pane', () => log('focus', focusMark), (list) => typedInto(list, token), { deadlineMs: 30_000 });
      step.artifact('fixture-log', 'focus.agent.jsonl', (await log('focus')).map((e) => JSON.stringify(e)).join('\n'));
      const verdict = rowFocusVerdict({ away: await log('away', awayMark), focus: await log('focus'), probe, token });
      step.assert('row-click-focused-terminal', typedInto(entries, token) && verdict.focusWasElsewhere && verdict.clickFocusedTerminal, { token, probe, verdict });
      const shown = keep(step, 'after-row-click', await until('typed token on screen', async () => {
        const seen = await frame('after-row-click');
        const header = locate(seen.read, 'Waiting for input');
        if (header) {
          const line = await ocr(run.tesseract, join(run.evidenceDir, seen.shot.path),
            { x: windowFrame.x + windowFrame.width * 0.2 - 4, y: header.box.y2 + 2, width: windowFrame.width * 0.65, height: header.box.y2 - header.box.y1 + 14 }, true);
          step.artifact('ocr', 'typed-token-line.ocr.tsv', line.tsv);
          seen.read.text += `\n${line.text}`;
        }
        return seen;
      }, ({ read: r }) => terminalTokenShown(r.text, token), { deadlineMs: 30_000 }));
      step.assert('typed-token-on-screen', terminalTokenShown(shown.read.text, token), { token, text: shown.read.text.slice(0, 1500) });
    });

    // F3-D7: "retry every 5 min", typed into this desktop's composer through MahoRD, becomes the
    // standing retry rule (300 s) and the rules line on screen changes. The line is read from
    // OCR before and after; the daemon's panel read proves the stored rule.
    await runStep(lane, 'F3-D7', async (step) => {
      const instruction = 'retry every 5 min';
      await scrollPanel('down', 900);
      const beforeView = await panel(step, 'rules-before');
      const { read: beforeRead } = await visible(step, 'rules-before', (r) => rulesLineText(r.text) !== null, { scroll: true });
      const beforeLine = rulesLineText(beforeRead.text);
      step.assert('rules-line-before', beforeLine !== null && !/every 5 min/.test(beforeLine), { line: beforeLine, rule: globalRetryRule(beforeView) });
      await sendComposer(step, instruction, '5 min');
      const stored = await until('the 300 s retry rule', () => panel(null), (v) => globalRetryRule(v)?.params?.thenEverySecs === 300, { deadlineMs: 120_000 });
      step.artifact('panel-read', 'rules-after.panel.json', stored);
      step.artifact('transcript-read', 'rules.transcript.json', await transcript(null));
      step.assert('rule-stored-every-300s', globalRetryRule(stored)?.params?.thenEverySecs === 300, { rule: globalRetryRule(stored) });
      const after = await visible(step, 'rules-after', (r) => /every 5 min/.test(rulesLineText(r.text) ?? ''), { scroll: true });
      const afterLine = rulesLineText(after.read.text);
      step.assert('rules-line-changed-on-screen', afterLine !== beforeLine && /every 5 min/.test(afterLine ?? ''), { before: beforeLine, after: afterLine });
    });

    // F3-D5: each decision the phone answered (F3-P1, F3-P2; their nonces recorded by the iPhone
    // lane) is gone from this desktop's panel read and from its screen. Waits for the phone.
    await runStep(lane, 'F3-D5', async (step) => {
      if (!run.lanes.includes('iphone')) throw new LaneBlocked('LANE_NOT_IN_RUN', 'no iPhone lane in this run answers a decision');
      await shared.phoneAnswersDone;
      const answers = shared.phoneAnswers;
      step.assert('phone-answered-card', answers.length > 0 && answers.every((a) => Number.isInteger(a.nonce)), { answers });
      const gone = (v) => answers.every((a) => decisionsNamingNonce(v, shared.decisionTitle, a.nonce).length === 0);
      const view = await until('the answered cards gone from the panel', () => panel(null), gone, { deadlineMs: 60_000 });
      step.artifact('panel-read', 'answered.panel.json', view);
      step.assert('answered-card-gone-from-panel', gone(view), { answers, decisions: view.decisions });
      const offScreen = ({ read: r }) => answers.every((a) => !namesNonce(r.text, shared.decisionTitle, a.nonce));
      const shown = keep(step, 'answered', await until('the answered cards gone from screen', () => frame('answered'), offScreen, { deadlineMs: 60_000 }));
      step.assert('answered-card-gone-from-screen', offScreen(shown), { answers, text: shown.read.text.slice(0, 1500) });
    });
  } catch (error) {
    if (error instanceof LaneBlocked) lane.receipt.blocked = { code: error.code, detail: error.detail, at: new Date().toISOString() };
    else lane.receipt.failure = { code: error.code ?? 'LANE_ERROR', detail: String(error.stack ?? error).slice(0, 4000) };
    if (mahord) await mahord.screenshot(AbortSignal.timeout(10_000)).then((bytes) => lane.write('screenshot', 'frames/failure.png', bytes)).catch(() => {});
  } finally {
    markNotRun(lane, ids, lane.receipt.blocked ? `${lane.receipt.blocked.code}` : 'lane stopped before this step');
    shared.desktopDone(lane.name);
    await cleanupLane({ lane, agent, helper, mahord, setup, gui, daemon, wrappers });
  }
}

/** GUI, then daemon by exact identity; helper children; then the profile; each outcome recorded. */
async function cleanupLane({ lane, agent, helper, mahord, setup, gui, daemon, wrappers }) {
  const record = (entry) => lane.receipt.cleanup.push(entry);
  const attempt = async (resource, body) => {
    try { await body(); } catch (error) { record({ resource, action: 'reported-not-killed', error: String(error?.message ?? error) }); }
  };
  if (helper && setup) await attempt('diagnostic-files', async () => {
    lane.write('diagnostic', 'diagnostic/gui-env.json', { profile: setup.profile, env: setup.env });
    for (const name of ['gui-stderr.log', 'gui-stdout.log', 'f3-provider-requests.jsonl']) {
      const files = await helper.call('list-files', { profile: setup.profile, dir: join(setup.profile, 'evidence') });
      if (files.some((f) => f.name === name)) {
        const file = await helper.call('read-b64', { profile: setup.profile, path: join(setup.profile, 'evidence', name) });
        lane.write('diagnostic', `diagnostic/${name}`, Buffer.from(file.base64, 'base64'));
      }
    }
    lane.write('diagnostic', 'diagnostic/downloads.json', await helper.call('list-files', { profile: setup.profile, dir: join(setup.profile, 'home', 'Downloads') }));
    if (wrappers?.logs) for (const file of await helper.call('list-files', { profile: setup.profile, dir: wrappers.logs })) {
      if (file.file && /^agent-[a-z0-9]+\.jsonl$/.test(file.name)) {
        const raw = await helper.call('read-b64', { profile: setup.profile, path: file.path });
        lane.write('fixture-log', `diagnostic/${file.name}`, Buffer.from(raw.base64, 'base64'));
      }
    }
  });
  if (agent && gui) {
    // darwin: the GUI is not the agent's child and the agent's `exit` is Linux pidfd for
    // non-children, so only the launcher's exit receipt (same pid + processStart) proves it gone.
    const exitProof = lane.name === 'macos'
      ? async () => {
        const file = await helper.call('read-b64', { profile: setup.profile, path: join(setup.profile, 'receipts', 'gui-launch.json.exit.json') }, AbortSignal.timeout(10_000));
        return JSON.parse(Buffer.from(file.base64, 'base64').toString('utf8'));
      }
      : null;
    record(await stopExact(agent, { resource: 'gui', receipt: gui, exitProof }));
  }
  if (agent && daemon) record(await stopExact(agent, { resource: 'daemon', receipt: daemon }));
  if (helper && setup) await attempt('helper-children', async () => { for (const entry of await helper.call('cleanup', {})) record(entry); });
  const alive = lane.receipt.cleanup.some((entry) => ['gui', 'daemon'].includes(entry.resource) && !['killed', 'already-exited'].includes(entry.action));
  if (agent && setup) {
    if (alive) record({ resource: 'profile', path: setup.profile, action: 'reported-not-killed', error: 'profile kept: a run-spawned process is not proven gone' });
    else await attempt('profile', async () => record({ resource: 'profile', path: setup.profile, action: 'removed', detail: await agent.call('remove-profile', { profile: setup.profile }, AbortSignal.timeout(60_000)) }));
  }
  await attempt('mahord', async () => mahord?.close());
  agent?.close(); helper?.close();
}

async function launchGui({ agent, helper, mahord, setup, host, run, lane }) {
  // The candidate is a debug build: its webview loads the frozen devUrl, which this run serves
  // from the checked ui/dist (closed with the profile or with the agent).
  lane.write('gui-receipt', 'frontend.json', await agent.call('start-frontend', { profile: setup.profile, uiDist: setup.frontendDist, frontend: { mode: 'debug', url: FRONTEND_DEV_URL } }));
  const { specPath, launcherShortPath } = await agent.call('provision-gui-launch', { setup: { ...setup, binary: setup.guiBinary }, worktreeRoot: host.worktreeRoot, hostId: host.hostId, runId: run.runId, operationId: `f3-${run.runId}` });
  const receipt = agent.call('await-gui-launch', { profile: setup.profile, timeoutMs: 180_000 });
  // Handled now so a failed launch step cannot leave it rejecting unobserved; awaited below.
  receipt.catch(() => {});
  if (host.platform === 'win32') {
    let task;
    let launchError;
    try {
      task = await agent.call('launch-gui-interactive', { profile: setup.profile });
    } catch (error) {
      task = error.receipt;
      launchError = error;
    }
    lane.write('gui-receipt', 'gui-interactive-task.json', task ?? { unavailable: true, error: String(launchError?.message ?? launchError) });
    if (task?.created && !task.deleted) {
      lane.receipt.cleanup.push({ resource: 'gui-interactive-task', action: 'reported-not-killed', taskName: task.taskName, deleteError: task.deleteError });
    }
    if (launchError) throw launchError;
    if (task?.created && !task.deleted) throw new StepFailed('GUI_TASK_DELETE', JSON.stringify({ taskName: task.taskName, deleteError: task.deleteError }));
  } else {
    const opened = await helper.call('open-gui-bundle', { profile: setup.profile, bundleId: `dev.ferryx.qa.${run.runId}`, agent: lane.receipt.remoteAgent.resolvedPath, specPath });
    lane.write('gui-receipt', 'gui-bundle.json', opened);
  }
  const gui = await receipt;
  lane.write('gui-receipt', 'gui-launch.json', gui);
  if (gui.role !== 'gui' || gui.status !== 'spawned' || gui.executable !== setup.guiBinary || !gui.processStart) throw new StepFailed('GUI_LAUNCH', JSON.stringify(gui));
  return gui;
}

export function pairOf(page, prompt) {
  const rows = page?.rows ?? page?.messages ?? page?.items ?? [];
  const owner = rows.find((row) => row.role === 'owner' && row.text === prompt);
  if (!owner) return null;
  const reply = rows.find((row) => row.role === 'supervisor' && row.requestId === `reply:${owner.requestId}`)
    ?? rows.find((row) => row.role === 'supervisor' && row.id > owner.id);
  return reply ? { owner, reply } : null;
}

const firstWords = (text) => String(text).replace(/\s+/g, ' ').trim().split(' ').slice(0, 3).join(' ');
