// Session host (Windows) services of the F3 run and the facts every lane shares: the run-owned
// HTTPS relay with its embedded account service, sign-in links from its file mailer, the
// decision card the fleet holds, the fixture panes and their logs, the push store, and the
// gates that order the lanes (cleanup of the session host waits for the other lanes; the iPhone
// waits until both desktops recorded the same card, prompt and Reply).
import { randomUUID } from 'node:crypto';
import { rpcProcess } from '../fixtures/native-configured-adapters.mjs';
import { LaneBlocked, StepFailed, latch, mutex, until } from './steps.mjs';

export function startSession({ config, run, windows }) {
  const host = config.hosts.windows;
  const helper = rpcProcess(config.sshExecutable ?? 'ssh', ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', host.sshDestination,
    `powershell.exe -NoProfile -NonInteractive -Command "& '${host.bun}' '${host.hostHelper}' --rpc"`], { signal: run.signal });
  const relayRoot = `${host.worktree}\\.native-qa\\${run.runId}-relay`;
  const origin = new URL(config.relayOrigin).origin;
  const port = Number(new URL(config.relayOrigin).port || 443);
  const tokens = new Map(['macos', 'windows'].map((name) => [config.hosts[name].hostId, `f3-${run.runId}-${name}`]));
  const cleanupGate = latch();
  const desktopsSaw = latch();
  const shared = {
    relayOrigin: origin, phoneOrigin: origin, phoneUrl: `${origin}/`,
    decisionTitle: 'f3decision', pushTitle: 'f3push', upstreamTitle: 'f3upstream',
    ownerEmail: config.ownerEmail,
    machineToken: (hostId) => tokens.get(hostId),
    desktops: {}, cross: [], observed: {},
    promptToken: `f3tok${run.runId.replace(/[^a-z0-9]/g, '').slice(-8)}`,
    get prompt() { return `F3 check ${shared.promptToken}: what is waiting on me?`; },
    // The session host's lane holds its cleanup until the runner opens this, after the macOS
    // and iPhone lanes have settled; the other desktop cleans up when it is done.
    cleanupGate: (lane) => (lane === 'windows' ? cleanupGate.promise : Promise.resolve()),
    releaseCleanup: () => cleanupGate.open(),
    // The iPhone answers (which changes the decision card) only after each desktop either
    // recorded F3-D1 and F3-D2 or finished its scenarios without them (then D1/D2 fail or block).
    desktopsSawCard: desktopsSaw.promise,
    desktopDone(lane) { finished.add(lane); releaseIfSeen(); },
    record(scenarioId, lane, value) {
      (shared.observed[scenarioId] ??= {})[lane] = value;
      releaseIfSeen();
    },
  };
  const finished = new Set();
  function releaseIfSeen() {
    const settled = (lane) => finished.has(lane) || (shared.observed['F3-D1']?.[lane] && shared.observed['F3-D2']?.[lane]);
    if (settled('macos') && settled('windows')) desktopsSaw.open();
  }
  let relay = null;
  // Reachability alone decides HOST_UNAVAILABLE: the identity probe over SSH.
  shared.probe = async () => {
    try { return await helper.call('identity', {}); } catch (error) {
      throw new LaneBlocked('HOST_UNAVAILABLE', `session host ${host.sshDestination} (${host.hostId}): ${error.message}`);
    }
  };
  // A reachable host whose relay does not come up is a product/lane failure, not a block.
  shared.start = async () => {
    try {
      relay = await helper.call('spawn-owned', { profile: relayRoot, key: 'relay', executable: host.relay,
        args: ['--port', String(port), '--tls-cert', host.relayTlsCert, '--tls-key', host.relayTlsKey],
        env: { FERRYX_DEPLOYMENT_MODE: 'selfhost', FERRYX_RELAY_MACHINE_TOKENS: [...tokens.values()].join(','), FERRYX_ACCOUNT_DATA_DIR: `${relayRoot}\\account`,
          FERRYX_ACCOUNT_ORIGIN: origin, FERRYX_ACCOUNT_RELAY_ORIGIN: origin, FERRYX_UI_DIST_DIR: host.uiDist, RUST_LOG: 'info' }, ready: 'ferryx-relay listening', deadlineMs: 60_000 });
      windows.write('relay', 'relay-spawn.json', relay);
    } catch (error) {
      throw new StepFailed('RELAY_START', `relay on ${host.hostId} did not become ready: ${error.message}`);
    }
  };
  const mailDir = `${relayRoot}\\account\\mail`;
  const mailNames = async () => new Set((await helper.call('list-files', { profile: relayRoot, dir: mailDir })).map((f) => f.name));
  const nextLink = async (before) => {
    const isNew = (f) => !before.has(f.name) && f.bytes > 0;
    const files = await until('magic link mail', () => helper.call('list-files', { profile: relayRoot, dir: mailDir }), (list) => list.some(isNew), { deadlineMs: 60_000 });
    const fresh = files.filter(isNew);
    if (fresh.length !== 1) throw new StepFailed('MAIL_AMBIGUOUS', `${fresh.length} new mails for one sign-in: ${fresh.map((f) => f.name).join(', ')}`);
    return Buffer.from((await helper.call('read-b64', { profile: relayRoot, path: fresh[0].path })).base64, 'base64').toString('utf8').trim();
  };
  // The FileMailer writes one file per link and ignores the recipient, so a sign-in is one
  // section: snapshot the mail dir, trigger the mail, wait for the single new file. Sections of
  // different lanes never overlap.
  const mail = mutex();
  const signIn = (trigger) => mail(async () => { const before = await mailNames(); await trigger(); return nextLink(before); });
  // A desktop joins through device authorization (`ferryx-cli account login`); the approve link
  // is opened from the control host over the relay's HTTPS origin, as a person's browser would.
  shared.enroll = async ({ helper: hostHelper, profile, env, cli }) => {
    let login;
    const link = await signIn(async () => {
      login = await hostHelper.call('spawn-owned', { profile, key: 'account-login', executable: cli, args: ['account', 'login', '--email', config.ownerEmail, '--origin', origin], env });
    });
    if (new URL(link).origin !== origin) throw new StepFailed('MAIL_ORIGIN', `link ${link} is not on ${origin}`);
    const approved = await fetch(link, { signal: AbortSignal.timeout(30_000) });
    const exit = await hostHelper.call('await-exit', { key: 'account-login' });
    if (approved.status !== 200 || exit.exit?.code !== 0) throw new StepFailed('ACCOUNT_LOGIN', JSON.stringify({ approved: approved.status, exit }));
    return { login, approved: approved.status, exit: exit.exit };
  };
  shared.signInPhone = (trigger) => signIn(trigger);
  const session = () => shared.desktops.windows;
  shared.spawnPanes = async () => {
    const s = session();
    const panes = {};
    for (const mode of ['decision', 'held', 'upstream']) {
      const reply = await s.call(`spawn-${mode}`, [{ type: 'spawn', clientRequestId: randomUUID(), workspaceId: 'f3', worktree: null, cwd: s.setup.worktree, cols: 120, rows: 32, shell: s.wrappers.wrappers[mode].shell }]);
      if (reply?.type !== 'spawnOk') throw new StepFailed('FIXTURE_PANES', `spawn ${mode}: ${JSON.stringify(reply)}`);
      panes[mode] = reply.sessionId;
    }
    shared.panes = panes;
    windows.write('panes', 'panes.json', panes);
    return panes;
  };
  shared.decisionCard = async () => {
    const s = session();
    const mine = (d) => d.sessionRef.endsWith(`:${shared.panes.decision}`);
    const view = await until('the fleet decision card', async () => (await s.call('panel', [{ type: 'supervisorReadV1', request: { view: 'panel' } }]))?.value,
      (v) => (v?.decisions ?? []).some(mine), { deadlineMs: 180_000 });
    const decision = view.decisions.find(mine);
    return { sessionRef: decision.sessionRef, promptId: decision.promptId, commandFragment: `prod.example.com/${shared.decisionTitle}`, decision };
  };
  const logPath = (mode) => `${session().wrappers.logs}\\agent-${session().wrappers.wrappers[mode].title}.jsonl`;
  shared.agentLog = async (mode, after = 0) => {
    const file = await session().helper.call('read-b64', { profile: session().setup.profile, path: logPath(mode) });
    return Buffer.from(file.base64, 'base64').toString('utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line)).slice(after);
  };
  shared.agentLogCursor = async (mode) => (await shared.agentLog(mode)).length;
  // The phone's answers (F3-P1, F3-P2) in order: each names the nonce of the prompt it answered,
  // so a desktop can prove that exact card left its panel (F3-D5). Settles once P1 and P2 ran.
  const answered = latch();
  shared.phoneAnswers = [];
  shared.recordPhoneAnswer = (entry) => { shared.phoneAnswers.push(entry); };
  shared.phoneAnswersDone = answered.promise;
  shared.phoneAnswersSettled = () => answered.open();
  const typeInto = (mode, text) => session().call(`write-${mode}`, [{ type: 'write', sessionId: shared.panes[mode], data: Buffer.from(`${text}\r`).toString('base64') }]);
  shared.releaseHeldDecision = () => typeInto('held', 'F3-DRAW');
  shared.sendLongReport = (token) => typeInto('decision', `F3-REPORT:${token}`);
  // The Windows daemon's own push store: the subscription the iPhone's web app registered, read
  // before the push and again after it was delivered (a gone endpoint is dropped by push.rs).
  shared.pushSubscriptions = async () => {
    const s = session();
    const path = `${s.setup.profile}\\data\\remote\\push-subscriptions.json`;
    const listed = await s.helper.call('list-files', { profile: s.setup.profile, dir: `${s.setup.profile}\\data\\remote` });
    if (!listed.some((f) => f.name === 'push-subscriptions.json')) return [];
    const file = await s.helper.call('read-b64', { profile: s.setup.profile, path });
    return JSON.parse(Buffer.from(file.base64, 'base64').toString('utf8')).subscriptions ?? [];
  };
  shared.servedCandidate = async () => {
    const who = await helper.call('identity', {});
    return { ...who.candidate, stamp: session()?.setup?.candidate ?? null, servedBy: host.hostId, uiDist: host.uiDist };
  };
  // Always runs: ends the children this helper spawned and removes the relay root it created
  // (an absent root is recorded as absent, never assumed).
  shared.stop = async () => {
    const receipts = [];
    for (const [label, call] of [['relay-children', () => helper.call('cleanup', {})], ['relay-root', () => helper.call('remove-owned-dir', { profile: relayRoot })]]) {
      try { receipts.push(...[].concat(await call())); } catch (error) { receipts.push({ resource: label, action: 'reported-not-killed', error: String(error.message ?? error) }); }
    }
    return relay ? receipts : receipts.map((entry) => ({ ...entry, relaySpawned: false }));
  };
  shared.close = () => helper.close();
  return shared;
}
