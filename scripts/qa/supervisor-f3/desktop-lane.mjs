// Desktop lane (macOS or Windows) of the F3 acceptance run.
//
// Setup goes over SSH: the existing scripts/qa/fixtures/native-remote-agent.mjs (--rpc) checks
// the built candidate against its manifest, makes the isolated `.native-qa/<runId>` profile and
// launches the candidate daemon with exact-PID receipts; host-helper.mjs (--rpc) reports the
// machine and checkout identity and speaks the daemon protocol over that profile's endpoint.
// Every GUI action and every screenshot goes through MahoRD (connectNativeMahord): the GUI is
// launched through the OS launcher by MahoRD exactly as native-configured-adapters.mjs does,
// clicks land on points located by OCR of the real frame, and every assertion that names
// on-screen text is checked against tesseract output of that frame.
//
// Unavailable host = LaneBlocked HOST_UNAVAILABLE (SSH probe or MahoRD connect failed), never a
// skip. Cleanup removes only the profile and children this run created.
import { spawn } from 'node:child_process';
import { join } from 'node:path';
import { connectNativeMahord, rpcProcess } from '../fixtures/native-configured-adapters.mjs';
import { LaneBlocked, StepFailed, markNotRun, runStep, until } from './steps.mjs';
import { scenariosForLane } from './contract.mjs';

const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
const psq = (value) => `'${String(value).replaceAll("'", "''")}'`;

function remoteNode(host, script) {
  return host.platform === 'win32'
    ? `powershell.exe -NoProfile -NonInteractive -Command "& ${psq(host.bun)} ${psq(script)} --rpc"`
    : `${quote(host.bun)} ${quote(script)} --rpc`;
}

/** OCR words with boxes (tesseract TSV) of one PNG on the control host. */
async function ocr(tesseract, pngPath) {
  const tsv = await new Promise((resolveOcr, reject) => {
    const child = spawn(tesseract, [pngPath, 'stdout', 'tsv'], { stdio: ['ignore', 'pipe', 'pipe'] });
    let out = ''; let err = '';
    child.stdout.on('data', (d) => { out += d; });
    child.stderr.on('data', (d) => { err += d; });
    child.once('error', reject);
    child.once('close', (code) => (code === 0 ? resolveOcr(out) : reject(new StepFailed('OCR_FAILED', err))));
  });
  const words = tsv.split('\n').slice(1).map((line) => line.split('\t')).filter((f) => f.length >= 12 && f[11].trim())
    .map((f) => ({ text: f[11].trim(), x: Number(f[6]), y: Number(f[7]), w: Number(f[8]), h: Number(f[9]), line: `${f[2]}.${f[3]}.${f[4]}` }));
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

export async function runDesktopLane(lane, { host, run, shared, signal }) {
  const ids = scenariosForLane(lane.name);
  const cleanups = [];
  let agent; let helper; let mahord; let setup;
  const ssh = (script, onEvent) => rpcProcess(run.sshExecutable ?? 'ssh', ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', host.sshDestination, remoteNode(host, script)], { signal, onEvent });
  try {
    // 0. Host reachability: an unavailable host is a blocked lane with its exact cause.
    try {
      helper = ssh(host.hostHelper);
      const who = await helper.call('identity', {});
      lane.receipt.machine = who.machine;
      lane.receipt.candidate = who.candidate;
    } catch (error) {
      throw new LaneBlocked('HOST_UNAVAILABLE', `ssh ${host.sshDestination} (${host.hostId}): ${error.message}`);
    }
    try {
      mahord = await connectNativeMahord({ executable: run.mahordExecutable, address: host.mahordAddress, pairingId: host.pairingId, nudgeMs: host.mahordNudgeMs, signal });
      cleanups.push(() => mahord.close());
    } catch (error) {
      throw new LaneBlocked('HOST_UNAVAILABLE', `MahoRD ${host.mahordAddress} (${host.hostId}): ${error.message}`);
    }
    lane.write('identity', 'identity.json', { machine: lane.receipt.machine, candidate: lane.receipt.candidate });
    if (lane.receipt.candidate.headRevision !== run.revision) throw new LaneBlocked('REVISION_MISMATCH', `${host.hostId} checkout is at ${lane.receipt.candidate.headRevision}, run revision ${run.revision}`);

    // 1. Isolated profile + candidate daemon through the existing native agent.
    agent = ssh(host.remoteAgent);
    setup = await agent.call('prepare', { worktreeRoot: host.worktreeRoot, worktree: host.worktree, runId: run.runId, operationId: `f3-${run.runId}`,
      binary: host.binary, uiDist: host.uiDist, manifest: host.buildManifest });
    cleanups.push(() => agent.call('remove-profile', { profile: setup.profile }, AbortSignal.timeout(60_000)).then((r) => lane.receipt.cleanup.push({ resource: 'profile', path: setup.profile, action: 'removed', detail: r })));
    cleanups.push(() => helper.call('cleanup', {}).then((receipts) => lane.receipt.cleanup.push(...receipts)));
    const wrappers = await helper.call('write-wrappers', { profile: setup.profile });
    const env = { ...setup.env, FERRYX_RELAY_URL: shared.relayOrigin, FERRYX_ALLOW_INSECURE_RELAY: '1', FERRYX_MACHINE_TOKEN: shared.machineToken(host.hostId), FERRYX_ACCOUNT_ORIGIN: shared.relayOrigin };
    setup.env = env;
    const daemon = await agent.call('spawn-daemon', { setup, runId: run.runId, operationId: `f3-${run.runId}`, hostId: host.hostId, timeoutMs: 120_000 });
    lane.receipt.cleanup.push({ resource: 'daemon', pid: daemon.pid, action: 'reported-not-killed', detail: 'terminated by remove-profile of the owning agent' });
    lane.write('gui-receipt', 'daemon-launch.json', daemon);
    const call = async (label, frames, deadlineMs) => {
      const replies = await helper.call('daemon', { profile: setup.profile, frames, deadlineMs });
      lane.write('daemon-io', `daemon/${Date.now()}-${label}.json`, { label, frames, replies });
      return replies.at(-1);
    };
    const enrolled = await shared.enroll({ host, helper, profile: setup.profile, env, cli: host.cli });
    lane.write('account', 'enroll.json', enrolled);
    await call('register', [{ type: 'registerWorkspace', workspaceId: 'f3', repoRoot: setup.worktree }]);
    await call('agent-control', [{ type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] }]);
    await call('relay', [{ type: 'remoteConfigure', config: { mode: 'relay', port: 43821, allowControl: true, relayUrl: shared.relayOrigin } }]);

    // 2. GUI through MahoRD only (the same launcher path native-configured-adapters uses).
    const gui = await launchGui({ agent, mahord, setup, host, run, lane });
    lane.receipt.cleanup.push({ resource: 'gui', pid: gui.pid, action: 'reported-not-killed', detail: 'terminated by remove-profile of the owning agent' });
    const frame = async (name, step) => {
      const bytes = await mahord.screenshot(signal);
      const shot = step ? step.artifact('screenshot', `${name}.png`, bytes) : lane.write('screenshot', `frames/${name}.png`, bytes);
      const read = await ocr(run.tesseract, join(run.evidenceDir, shot.path));
      if (step) step.artifact('ocr', `${name}.ocr.tsv`, read.tsv);
      return { shot, read };
    };
    const panel = async (step, name) => {
      const reply = await call(`panel-${name}`, [{ type: 'supervisorReadV1', request: { view: 'panel' } }]);
      if (step) step.artifact('panel-read', `${name}.panel.json`, reply);
      return reply?.value ?? null;
    };
    const transcript = async (step, name) => {
      const reply = await call(`transcript-${name}`, [{ type: 'supervisorReadV1', request: { view: 'transcript', cursor: 0 } }]);
      if (step) step.artifact('transcript-read', `${name}.transcript.json`, reply);
      return reply?.value ?? null;
    };
    shared.desktops[lane.name] = { lane, frame, panel, transcript, call, mahord, setup, helper, wrappers, gui };

    // F3-D1: the shared decision card is on screen. The card's session lives on the session
    // host (`run.sessionHost`); both desktops show the fleet's card.
    await runStep(lane, 'F3-D1', async (step) => {
      step.artifact('gui-receipt', 'gui-launch.json', gui);
      const card = await shared.decisionCard();
      const view = await until('decision card in this desktop panel', () => panel(null), (v) => (v?.decisions ?? []).some((d) => d.sessionRef === card.sessionRef), { deadlineMs: 120_000 });
      step.artifact('panel-read', 'decision.panel.json', view);
      const shown = await until('decision card text on screen', () => frame('decision'), ({ read }) => read.text.includes(card.commandFragment), { deadlineMs: 60_000 });
      const decision = view.decisions.find((d) => d.sessionRef === card.sessionRef);
      shared.record('F3-D1', lane.name, { sessionRef: decision.sessionRef, promptId: decision.promptId, question: decision.question, command: decision.command });
      step.assert('card-is-the-fleet-decision', decision.promptId === card.promptId, { decision, card });
      step.assert('card-command-on-screen', shown.read.text.includes(card.commandFragment), { fragment: card.commandFragment });
      step.assert('recommended-option-on-screen', /yes/i.test(shown.read.text) && /no/i.test(shown.read.text), { text: shown.read.text.slice(0, 2000) });
      await frame('decision-final', step);
    });

    // F3-D2: one owner prompt, typed into the Windows desktop's composer through MahoRD, and the
    // supervisor's Reply: both on each desktop's screen and in each desktop's transcript read.
    await runStep(lane, 'F3-D2', async (step) => {
      const prompt = shared.prompt;
      const token = shared.promptToken;
      if (lane.name === 'windows') {
        const { read } = await frame('before-prompt', step);
        const box = locate(read, 'Tell the supervisor');
        if (!box) throw new StepFailed('COMPOSER_NOT_FOUND', 'the composer placeholder is not on screen');
        await mahord.click({ x: box.x, y: box.y }, signal);
        await mahord.type(prompt, signal);
        await mahord.key('Enter', signal);
      }
      const rows = await until('prompt and reply in the transcript', () => transcript(null), (page) => pairOf(page, prompt) !== null, { deadlineMs: 180_000 });
      step.artifact('transcript-read', 'transcript.json', rows);
      const pair = pairOf(rows, prompt);
      const after = await until('prompt and reply on screen', () => frame('after-prompt'), ({ read: r }) => r.text.includes(token) && r.text.includes(firstWords(pair.reply.text)), { deadlineMs: 60_000 });
      shared.record('F3-D2', lane.name, { owner: pair.owner.text, reply: pair.reply.text });
      step.assert('prompt-row-stored', pair.owner.text === prompt, pair);
      step.assert('reply-row-stored', pair.reply.text.length > 0, pair);
      step.assert('prompt-on-screen', after.read.text.includes(token), { token, text: after.read.text.slice(0, 2000) });
      step.assert('reply-on-screen', after.read.text.includes(firstWords(pair.reply.text)), { reply: pair.reply.text, text: after.read.text.slice(0, 2000) });
      await frame('reply-final', step);
    });

    // F3-D3: transcript export from the app downloads a real file into this profile's
    // Downloads directory, and the file holds this desktop's prompt and Reply.
    await runStep(lane, 'F3-D3', async (step) => {
      const downloads = join(setup.profile, 'home', 'Downloads');
      const before = new Set((await helper.call('list-files', { profile: setup.profile, dir: downloads })).map((f) => f.name));
      const { read } = await frame('before-export', step);
      const button = locate(read, host.exportLabel ?? 'Export transcript');
      if (!button) throw new StepFailed('PRODUCT_GAP_EXPORT_CONTROL', `no "${host.exportLabel ?? 'Export transcript'}" control on the supervisor panel`);
      await mahord.click({ x: button.x, y: button.y }, signal);
      const files = await until('a new file in Downloads', () => helper.call('list-files', { profile: setup.profile, dir: downloads }), (list) => list.some((f) => f.file && !before.has(f.name) && f.bytes > 0 && !/\.(crdownload|part|download)$/.test(f.name)), { deadlineMs: 60_000 });
      const fresh = files.find((f) => f.file && !before.has(f.name) && f.bytes > 0);
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
    // supervisorReport path) opens in Viewer, its first and last lines readable there.
    await runStep(lane, 'F3-D4', async (step) => {
      const token = `${lane.name.slice(0, 3)}${run.runId.replace(/[^a-z0-9]/g, '').slice(-8)}`;
      await shared.sendLongReport(token);
      const rows = await until('the long report in the transcript', () => transcript(null), (page) => JSON.stringify(page ?? {}).includes(`F3 long report ${token}`), { deadlineMs: 120_000 });
      step.artifact('transcript-read', 'report.transcript.json', rows);
      const { read } = await frame('before-viewer', step);
      const open = locate(read, host.viewerLabel ?? 'Open in Viewer');
      if (!open) throw new StepFailed('PRODUCT_GAP_VIEWER_CONTROL', `no "${host.viewerLabel ?? 'Open in Viewer'}" control next to the report`);
      await mahord.click({ x: open.x, y: open.y }, signal);
      const head = await until('report heading in Viewer', () => frame('viewer-head'), ({ read: r }) => r.text.includes(token) && /viewer/i.test(r.text), { deadlineMs: 60_000 });
      step.artifact('screenshot', 'viewer-head.png', await mahord.screenshot(signal));
      await mahord.hotkey(host.platform === 'darwin' ? ['Meta', 'ArrowDown'] : ['Control', 'End'], signal);
      const tail = await until('report end in Viewer', () => frame('viewer-tail', step), ({ read: r }) => r.text.includes(`End of report ${token}`), { deadlineMs: 60_000 });
      step.assert('viewer-shows-report-heading', head.read.text.includes(`F3 long report ${token}`) || head.read.text.includes(token), { text: head.read.text.slice(0, 1500) });
      step.assert('viewer-shows-report-end', tail.read.text.includes(`End of report ${token}`), { text: tail.read.text.slice(0, 1500) });
    });
  } catch (error) {
    if (error instanceof LaneBlocked) lane.receipt.blocked = { code: error.code, detail: error.detail, at: new Date().toISOString() };
    else lane.receipt.failure = { code: error.code ?? 'LANE_ERROR', detail: String(error.stack ?? error).slice(0, 4000) };
    if (mahord) await mahord.screenshot(AbortSignal.timeout(10_000)).then((bytes) => lane.write('screenshot', 'frames/failure.png', bytes)).catch(() => {});
  } finally {
    markNotRun(lane, ids, lane.receipt.blocked ? `${lane.receipt.blocked.code}` : 'lane stopped before this step');
    for (const step of cleanups.reverse()) {
      try { await step(); } catch (error) { lane.receipt.cleanup.push({ resource: 'cleanup-step', action: 'reported-not-killed', error: String(error.message ?? error) }); }
    }
    agent?.close(); helper?.close();
  }
}

async function launchGui({ agent, mahord, setup, host, run, lane }) {
  const { specPath, launcherShortPath } = await agent.call('provision-gui-launch', { setup, worktreeRoot: host.worktreeRoot, hostId: host.hostId, runId: run.runId, operationId: `f3-${run.runId}` });
  const receipt = agent.call('await-gui-launch', { profile: setup.profile, timeoutMs: 180_000 });
  const command = host.platform === 'win32' ? launcherShortPath : `${quote(host.bun)} ${quote(host.remoteAgent)} --launch-gui ${quote(specPath)}`;
  await mahord.hotkey(host.platform === 'win32' ? ['Meta', 'r'] : host.launchTerminalHotkey, AbortSignal.timeout(30_000));
  if (host.platform === 'win32') await mahord.hotkey(['Control', 'a'], AbortSignal.timeout(30_000));
  await mahord.type(command, AbortSignal.timeout(60_000));
  await mahord.key('Enter', AbortSignal.timeout(30_000));
  const gui = await receipt;
  lane.write('gui-receipt', 'gui-launch.json', gui);
  if (gui.role !== 'gui' || gui.status !== 'spawned' || gui.executable !== setup.binary || !gui.processStart) throw new StepFailed('GUI_LAUNCH', JSON.stringify(gui));
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
