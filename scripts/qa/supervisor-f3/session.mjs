// Session host services of the F3 run and the facts every lane shares. The session host is its
// own machine (`config.sessionHost`, Linux, macOS or Windows; neither desktop): it runs the
// run-owned relay with its embedded account service, fronted either by a run-owned
// `tailscale serve` HTTPS endpoint (public port -> loopback relay port) or by an operator PEM the
// relay serves itself, and an isolated candidate daemon that holds the fixture panes (the agent
// sessions both desktops and the phone act on). It also owns the sign-in links from the relay's
// file mailer, the decision card the fleet holds, the push store, and the gates that order the
// lanes (the iPhone waits until both desktops recorded the same card, prompt and Reply).
//
// Everything here goes through the session host's host-helper.mjs and native-remote-agent.mjs
// (`--rpc`, over SSH or as local children when `sshDestination` is "local"). Cleanup ends the
// daemon by exact identity, the relay and other children by handle, removes only the profile and
// relay root this run created, and turns off only the serve port this run added, proving the
// serve config is back to its snapshot.
import { randomUUID } from 'node:crypto';
import { rpcProcess } from '../fixtures/native-configured-adapters.mjs';
import { SESSION_PROFILE_SUFFIX, acceptedReply, configureGateway, gatewayPortFor, hubEnv, relayListenPort, remotePath, rpcCommand, sameServeConfig, serveAddedProblems } from './seams.mjs';
import { LaneBlocked, StepFailed, latch, mutex, stopExact, until } from './steps.mjs';

export function startSession({ config, run, lane }) {
  const host = config.sessionHost;
  const front = host.relayFront;
  const rpc = (script) => rpcProcess(...rpcCommand(host, script, run.sshExecutable), { signal: run.signal });
  const helper = rpc(host.hostHelper);
  let agent = null;
  const relayRoot = remotePath(host, host.worktree, '.native-qa', `${run.runId}-relay`);
  // Its own profile name, so a session host that is also a desktop of the run (all-local) never
  // collides with that desktop's `.native-qa/<runId>` profile in a shared worktree.
  const sessionRunId = `${run.runId}${SESSION_PROFILE_SUFFIX}`;
  const originUrl = new URL(config.relayOrigin);
  const origin = originUrl.origin;
  const tokens = new Map([...['macos', 'windows'].filter((name) => run.lanes.includes(name)).map((name) => [config.hosts[name].hostId, name]), [host.hostId, 'session']]
    .map(([hostId, name]) => [hostId, `f3-${run.runId}-${name}`]));
  const desktopsSaw = latch();
  const shared = {
    relayOrigin: origin, phoneOrigin: origin, phoneUrl: `${origin}/`,
    decisionTitle: 'f3decision', pushTitle: 'f3push', upstreamTitle: 'f3upstream',
    ownerEmail: config.ownerEmail,
    machineToken: (hostId) => tokens.get(hostId),
    desktops: {}, cross: [], observed: {},
    promptToken: `f3tok${run.runId.replace(/[^a-z0-9]/g, '').slice(-8)}`,
    get prompt() { return `F3 check ${shared.promptToken}: what is waiting on me?`; },
    // The iPhone answers (which changes the decision card) only after each desktop either
    // recorded F3-D1 and F3-D2 or finished its scenarios without them (then D1/D2 fail or block).
    desktopsSawCard: desktopsSaw.promise,
    desktopDone(name) { finished.add(name); releaseIfSeen(); },
    record(scenarioId, name, value) {
      (shared.observed[scenarioId] ??= {})[name] = value;
      releaseIfSeen();
    },
  };
  const finished = new Set();
  function releaseIfSeen() {
    const settled = (name) => finished.has(name) || (shared.observed['F3-D1']?.[name] && shared.observed['F3-D2']?.[name]);
    if (settled('macos') && settled('windows')) desktopsSaw.open();
  }
  let relay = null;
  let serveBefore = null;
  let setup = null;
  let daemon = null;
  let handle = null;
  // Reachability alone decides HOST_UNAVAILABLE: the identity probe.
  shared.probe = async () => {
    try { return await helper.call('identity', {}); } catch (error) {
      throw new LaneBlocked('HOST_UNAVAILABLE', `session host ${host.sshDestination} (${host.hostId}): ${error.message}`);
    }
  };
  // A reachable host whose endpoint, relay or daemon does not come up is a failure, not a block.
  shared.start = async () => {
    if (lane.receipt.candidate?.headRevision !== run.revision) throw new StepFailed('REVISION_MISMATCH', `${host.hostId} checkout is at ${lane.receipt.candidate?.headRevision}, run revision ${run.revision}`);
    if (front.kind === 'tailscale-serve') {
      let served;
      try { served = await helper.call('serve-add', { publicPort: front.publicPort, backendPort: front.backendPort }); } catch (error) {
        throw new StepFailed('SERVE_ENDPOINT', `tailscale serve :${front.publicPort} -> 127.0.0.1:${front.backendPort} on ${host.hostId}: ${error.message}`);
      }
      serveBefore = served.before;
      lane.write('relay', 'relay/serve-before.json', served.before);
      lane.write('relay', 'relay/serve-after-add.json', served.after);
      const problems = serveAddedProblems(served.before, served.after, { hostName: originUrl.hostname, port: front.publicPort, backend: front.backendPort });
      if (problems.length) throw new StepFailed('SERVE_ENDPOINT', problems.join('; '));
    }
    const tls = front.kind === 'tls' ? ['--tls-cert', front.cert, '--tls-key', front.key] : [];
    try {
      relay = await helper.call('spawn-owned', { profile: relayRoot, key: 'relay', executable: host.relay,
        args: ['--port', String(relayListenPort(config)), ...tls],
        env: { FERRYX_DEPLOYMENT_MODE: 'selfhost', FERRYX_RELAY_MACHINE_TOKENS: [...tokens.values()].join(','), FERRYX_ACCOUNT_DATA_DIR: remotePath(host, relayRoot, 'account'),
          FERRYX_ACCOUNT_ORIGIN: origin, FERRYX_ACCOUNT_RELAY_ORIGIN: origin, FERRYX_UI_DIST_DIR: host.uiDist, RUST_LOG: 'info' }, ready: 'ferryx-relay listening', deadlineMs: 60_000 });
      lane.write('relay', 'relay/relay-spawn.json', relay);
    } catch (error) {
      throw new StepFailed('RELAY_START', `relay on ${host.hostId} did not become ready: ${error.message}`);
    }
    // The public origin, as the desktops and the phone reach it, answers before anyone uses it.
    const health = await until('relay health over the public origin', () => fetch(`${origin}/healthz`, { signal: AbortSignal.timeout(10_000) }).then((r) => r.status, (e) => String(e.message ?? e)),
      (status) => status === 200, { deadlineMs: 60_000, gapMs: 2_000 }).catch((error) => { throw new StepFailed('RELAY_ORIGIN', `${origin}/healthz: ${error.detail ?? error.message}`); });
    lane.write('relay', 'relay/origin-health.json', { origin, status: health });

    // The fixture daemon: the session host's own isolated candidate, prepared and launched by the
    // native agent with exact-pid receipts and the build stamp checked against its manifest.
    agent = rpc(host.remoteAgent);
    setup = await agent.call('prepare', { worktreeRoot: host.worktreeRoot, worktree: host.worktree, runId: sessionRunId, operationId: `f3-${run.runId}`,
      binary: host.binary, uiDist: host.uiDist, manifest: host.buildManifest });
    Object.assign(lane.receipt.candidate, { stamp: setup.candidate, binarySha256: setup.binarySha256, uiSha256: setup.uiSha256 });
    if (typeof setup.candidate !== 'string' || !setup.candidate.startsWith(`${run.revision}+`)) throw new StepFailed('CANDIDATE_STAMP', `${host.hostId} candidate stamp ${setup.candidate} is not ${run.revision}+<patch>`);
    const wrappers = await helper.call('write-wrappers', { profile: setup.profile });
    const gatewayPort = gatewayPortFor(host.hostId, await helper.call('free-port', {}));
    const env = { ...setup.env, ...hubEnv(host, setup.profile), FERRYX_RELAY_URL: origin, FERRYX_MACHINE_TOKEN: shared.machineToken(host.hostId), FERRYX_ACCOUNT_ORIGIN: origin, FERRYX_QA_GATEWAY_PORT: String(gatewayPort) };
    setup.env = env;
    daemon = await agent.call('spawn-daemon', { setup, runId: sessionRunId, operationId: `f3-${run.runId}`, hostId: host.hostId, timeoutMs: 120_000 });
    lane.write('daemon', 'daemon-launch.json', daemon);
    const exchange = async (label, frames, deadlineMs) => {
      const replies = await helper.call('daemon', { profile: setup.profile, frames, deadlineMs });
      lane.write('daemon-io', `daemon/${Date.now()}-${label}.json`, { label, frames, replies });
      return replies;
    };
    const call = async (label, frames, deadlineMs) => acceptedReply(label, await exchange(label, frames, deadlineMs));
    lane.write('account', 'account/enroll.json', await shared.enroll({ helper, profile: setup.profile, env, cli: host.cli }));
    acceptedReply('register', await exchange('register', [{ type: 'registerWorkspace', workspaceId: 'f3', repoRoot: setup.worktree }]), 'registerWorkspaceOk');
    acceptedReply('agent-control', await exchange('agent-control', [{ type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] }]), 'agentControlSettingsOk');
    lane.write('relay', 'relay/gateway.json', await configureGateway({ call: exchange, port: gatewayPort, relayOrigin: origin }));
    handle = { call, helper, setup, wrappers };
  };
  const mailDir = remotePath(host, relayRoot, 'account', 'mail');
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
  // A daemon joins through device authorization (`ferryx-cli account login`); the approve link
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
  // All-local (loopback relay): a panel shows decision cards from its own daemon's panes only
  // (the escalations are the local autonomy's, hub_runtime/panel_autonomy.rs observe), so the
  // fixture panes live on the desktop lane's daemon, which hands itself over with hostPanes.
  let paneHost = null;
  shared.panesOnDesktop = front.kind === 'loopback';
  shared.hostPanes = (desktop) => { paneHost = desktop; };
  const session = () => paneHost ?? handle;
  shared.spawnPanes = async () => {
    const s = session();
    const panes = {};
    for (const mode of ['decision', 'held', 'upstream']) {
      const reply = await s.call(`spawn-${mode}`, [{ type: 'spawn', clientRequestId: randomUUID(), workspaceId: 'f3', worktree: null, cwd: s.setup.worktree, cols: 120, rows: 32, shell: s.wrappers.wrappers[mode].shell }]);
      if (reply?.type !== 'spawnOk') throw new StepFailed('FIXTURE_PANES', `spawn ${mode}: ${JSON.stringify(reply)}`);
      panes[mode] = reply.sessionId;
    }
    shared.panes = panes;
    lane.write('panes', 'panes.json', panes);
    return panes;
  };
  shared.decisionCard = async () => {
    const s = session();
    const mine = (d) => d.sessionRef.endsWith(`:${shared.panes.decision}`);
    const view = await until('the fleet decision card', async () => (await s.call('panel', [{ type: 'supervisorReadV1', request: { view: 'panel' } }]))?.value,
      (v) => (v?.decisions ?? []).some(mine), { deadlineMs: 180_000 });
    const decision = view.decisions.find(mine);
    return { sessionRef: decision.sessionRef, promptId: decision.promptId, // The card shows the command on one truncated line (DecisionCard.tsx `truncate`), so the
    // fragment read on screen is its head; the panel read proves the whole command.
    commandFragment: decision.prompt.command.split('\n')[0].split(/\s+/).slice(0, 3).join(' '), decision };
  };
  const logPath = (mode) => remotePath(host, session().wrappers.logs, `agent-${session().wrappers.wrappers[mode].title}.jsonl`);
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
  // The session daemon's own push store: the subscription the iPhone's web app registered, read
  // before the push and again after it was delivered (a gone endpoint is dropped by push.rs).
  shared.pushSubscriptions = async () => {
    const s = session();
    const dir = remotePath(host, s.setup.profile, 'data', 'remote');
    const listed = await s.helper.call('list-files', { profile: s.setup.profile, dir });
    if (!listed.some((f) => f.name === 'push-subscriptions.json')) return [];
    const file = await s.helper.call('read-b64', { profile: s.setup.profile, path: remotePath(host, dir, 'push-subscriptions.json') });
    return JSON.parse(Buffer.from(file.base64, 'base64').toString('utf8')).subscriptions ?? [];
  };
  shared.servedCandidate = async () => {
    const who = await helper.call('identity', {});
    return { ...who.candidate, stamp: setup?.candidate ?? null, servedBy: host.hostId, uiDist: host.uiDist };
  };
  // Always runs, after every lane settled: the daemon by exact identity, then the helper's
  // children (relay, account login), then the profile (only when the daemon is proven gone),
  // the relay root, and the serve port this run added (proven back to its snapshot).
  shared.stop = async () => {
    const receipts = [];
    const attempt = async (resource, body) => {
      try { receipts.push(...[].concat(await body())); } catch (error) { receipts.push({ resource, action: 'reported-not-killed', error: String(error.message ?? error) }); }
    };
    if (agent && daemon) receipts.push({ ...(await stopExact(agent, { resource: 'session-daemon', receipt: daemon })) });
    await attempt('session-children', () => helper.call('cleanup', {}));
    const daemonGone = !daemon || receipts.some((r) => r.resource === 'session-daemon' && ['killed', 'already-exited'].includes(r.action));
    if (agent && setup) {
      if (daemonGone) await attempt('profile', async () => ({ resource: 'profile', path: setup.profile, action: 'removed', detail: await agent.call('remove-profile', { profile: setup.profile }, AbortSignal.timeout(60_000)) }));
      else receipts.push({ resource: 'profile', path: setup.profile, action: 'reported-not-killed', error: 'profile kept: the session daemon is not proven gone' });
    }
    await attempt('relay-root', () => helper.call('remove-owned-dir', { profile: relayRoot }));
    if (serveBefore) {
      await attempt('serve-endpoint', async () => {
        const { after } = await helper.call('serve-remove', { publicPort: front.publicPort });
        lane.write('relay', 'relay/serve-after-remove.json', after);
        return sameServeConfig(serveBefore, after)
          ? { resource: 'serve-endpoint', port: front.publicPort, action: 'removed' }
          : { resource: 'serve-endpoint', port: front.publicPort, action: 'reported-not-killed', error: 'serve config after removal differs from the snapshot taken before this run' };
      });
    }
    return relay ? receipts : receipts.map((entry) => ({ ...entry, relaySpawned: false }));
  };
  shared.close = () => { agent?.close(); helper.close(); };
  return shared;
}
