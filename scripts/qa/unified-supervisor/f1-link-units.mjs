// --self-test unit checks for the F1 agent link (f1-agent-link.mjs) and the detached host agent
// (f1-host-agent.mjs --serve / --attach). Drops are simulated, never waited out: fake links and
// a fake clock for the channel, and for the real agent a SIGKILLed attach client (a dropped ssh)
// and a SIGKILLed server (an agent that did not survive). Every wait is on a concrete line or
// file change with a bounded deadline.
import { spawn } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync, readFileSync, watch } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { AgentChannel, backoffDelay, readAppends, RECONNECT_BUDGET_MS } from './f1-agent-link.mjs';
import { agentLinkArgs } from './f1-acceptance.mjs';

const AGENT = join(dirname(fileURLToPath(import.meta.url)), 'f1-host-agent.mjs');

function appendChecks(check) {
  const enc = (text) => Buffer.from(text, 'utf8');
  const a = readAppends(enc('{"a":1}\n{"b":'), 0);
  const b = readAppends(enc('{"a":1}\n{"b":2}\n'), a.end);
  const c = readAppends(enc('{"a":1}\n{"b":2}\n'), b.end);
  const multi = readAppends(enc('é1\n\nü2\n'), 0);
  const cases = [
    ['partial-line-held-back', a.lines.length === 1 && a.lines[0].text === '{"a":1}' && a.end === 8],
    ['completed-line-delivered-once', b.lines.length === 1 && b.lines[0].text === '{"b":2}' && b.end === 16],
    ['nothing-new-nothing-delivered', c.lines.length === 0 && c.end === 16],
    ['byte-offsets-with-multibyte-text', multi.lines.map((l) => l.text).join('|') === 'é1|ü2' && multi.end === Buffer.byteLength('é1\n\nü2\n')],
    ['shrunk-file-flagged', readAppends(enc('x\n'), 10).truncated === true],
  ].map(([label, ok]) => ({ label, ok }));
  check('f1-pane-appends-by-byte-offset', cases.every((entry) => entry.ok), cases);
}

// A fake link: lines are pushed by the test, `drop()` resolves `closed`.
function fakeLink() {
  let drop;
  const link = { sent: [], send(line) { link.sent.push(JSON.parse(line)); }, closed: new Promise((resolveClosed) => { drop = resolveClosed; }), close() { drop({ closedByUs: true }); } };
  link.drop = (info = { code: 255 }) => drop(info);
  return link;
}

async function channelChecks(check) {
  const clock = { ms: 1_000_000 };
  const links = [];
  const connects = [];
  const delivered = [];
  const recorded = [];
  let refuse = 0;
  let wake = null;
  const sleeps = [];
  const sleep = (ms) => { sleeps.push(ms); clock.ms += ms; return Promise.resolve(); };
  const channel = new AgentChannel({
    id: 'agent-t', now: () => clock.ms, sleep, record: (entry) => recorded.push(entry), deliver: (json) => delivered.push(json),
    connect: async ({ after, attempt, onLine }) => {
      connects.push({ after, attempt });
      if (refuse > 0) { refuse -= 1; throw new Error('ssh: connect timed out'); }
      const link = fakeLink();
      link.onLine = onLine;
      links.push(link);
      wake?.();
      return { link, attached: { serverAlive: true, after } };
    },
  });
  const nextLink = (count) => new Promise((resolveLink) => { wake = () => { if (links.length >= count) resolveLink(links[count - 1]); }; wake(); });
  await channel.open();
  const first = links[0];
  first.onLine({ oseq: 1, event: 'agent-ready' });
  channel.send(1, JSON.stringify({ id: 1, op: 'init' }));
  first.onLine({ oseq: 2, id: 1, ok: true, result: {} });
  channel.send(2, JSON.stringify({ id: 2, op: 'proxyLog' }));
  // Drop with request 2 unanswered; two refused reconnects, then the agent is reachable again.
  refuse = 2;
  first.drop({ code: 255, signal: null });
  const second = await nextLink(2);
  // The agent replays from oseq 2: the duplicate of oseq 2 is dropped, oseq 3 (a pane line
  // emitted while nothing was attached) and the reply to request 2 are delivered once.
  second.onLine({ oseq: 2, id: 1, ok: true, result: {} });
  second.onLine({ oseq: 3, event: 'pane', record: { kind: 'done' } });
  second.onLine({ oseq: 4, id: 2, ok: true, result: { served: [] } });
  const kinds = recorded.map((entry) => entry.kind);
  const resumed = [
    ['reconnect-asks-for-lines-after-last-delivered', connects[1]?.after === 2 && connects.at(-1)?.after === 2],
    ['pending-request-resent-on-new-link', second.sent.length === 1 && second.sent[0].id === 2],
    ['replay-duplicates-dropped', delivered.map((d) => d.oseq).join(',') === '1,2,3,4'],
    ['drop-attempts-reconnect-recorded-in-order', JSON.stringify(kinds) === JSON.stringify(['dropped', 'reconnect-attempt', 'reconnect-failed', 'reconnect-attempt', 'reconnect-failed', 'reconnect-attempt', 'reconnected'])],
    ['records-carry-timestamps', recorded.every((entry) => Number.isFinite(entry.atMs) && typeof entry.at === 'string')],
    ['backoff-grows', JSON.stringify(sleeps) === JSON.stringify([backoffDelay(0), backoffDelay(1), backoffDelay(2)])],
    ['answered-request-no-longer-pending', channel.pending.size === 0 && channel.state === 'up'],
  ].map(([label, ok]) => ({ label, ok }));
  check('f1-agent-channel-resumes-after-drop', resumed.every((entry) => entry.ok), { resumed, connects, kinds, sleeps });

  // Budget: a target that never comes back is lost after the budget, never retried forever, and
  // the unanswered request is named. A lost channel refuses new requests.
  refuse = Number.MAX_SAFE_INTEGER;
  channel.send(3, JSON.stringify({ id: 3, op: 'proxyLog' }));
  const droppedAt = clock.ms;
  second.drop({ code: 255 });
  const lost = await channel.whenLost();
  const budget = [
    ['lost-within-budget', lost.kind === 'lost' && clock.ms - droppedAt <= RECONNECT_BUDGET_MS && lost.attempts > 3],
    ['lost-names-unanswered', JSON.stringify(lost.unanswered) === '[3]'],
    ['lost-refuses-new-requests', channel.send(4, '{}') === false && channel.state === 'lost'],
  ].map(([label, ok]) => ({ label, ok }));
  check('f1-agent-channel-gives-up-after-budget', budget.every((entry) => entry.ok), { budget, lost });

  // A link closed by us (teardown) is not a drop.
  const quiet = [];
  const closing = new AgentChannel({ id: 'agent-c', now: () => clock.ms, sleep, record: (entry) => quiet.push(entry), deliver: () => {},
    connect: async () => ({ link: fakeLink(), attached: null }) });
  await closing.open();
  closing.close();
  await closing.link?.closed;
  await Promise.resolve();
  check('f1-agent-channel-close-is-not-a-drop', quiet.length === 0, { quiet });
}

// Lines from one attach client process, consumed in order.
function attachClient(args) {
  const child = spawn(process.execPath, [AGENT, '--attach', ...args], { stdio: ['pipe', 'pipe', 'pipe'] });
  const lines = [];
  const waiters = [];
  let buffer = '';
  let stderr = '';
  const settle = () => { for (const waiter of [...waiters]) { const found = lines.find(waiter.predicate); if (found) { waiters.splice(waiters.indexOf(waiter), 1); clearTimeout(waiter.timer); waiter.resolve(found); } } };
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', (chunk) => {
    buffer += chunk;
    for (let index = buffer.indexOf('\n'); index >= 0; index = buffer.indexOf('\n')) {
      const line = buffer.slice(0, index);
      buffer = buffer.slice(index + 1);
      try { lines.push(JSON.parse(line)); } catch { lines.push({ unparsed: line }); }
    }
    settle();
  });
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  const exited = new Promise((resolveExit) => child.on('exit', (code, signal) => resolveExit({ code, signal })));
  return {
    child, lines, exited, stderr: () => stderr,
    send: (value) => child.stdin.write(`${JSON.stringify(value)}\n`),
    waitLine: (label, predicate, deadlineMs = 20_000) => new Promise((resolveLine, rejectLine) => {
      const waiter = { predicate, resolve: resolveLine, timer: setTimeout(() => { waiters.splice(waiters.indexOf(waiter), 1); rejectLine(new Error(`${label} not seen within ${deadlineMs}ms; stderr ${stderr.slice(-400)}`)); }, deadlineMs) };
      waiters.push(waiter);
      settle();
    }),
  };
}

// Watch first, then reconcile; a 200 ms re-check backs up fs.watch. Bounded.
function waitFile(path, predicate, deadlineMs = 20_000) {
  return new Promise((resolveFile, rejectFile) => {
    mkdirSync(dirname(path), { recursive: true });
    let watcher = null;
    let poll = null;
    const done = (fn, value) => { clearTimeout(timer); clearInterval(poll); try { watcher?.close(); } catch { /* closed */ } fn(value); };
    const evaluate = () => { try { const text = existsSync(path) ? readFileSync(path, 'utf8') : ''; if (predicate(text)) done(resolveFile, text); } catch { /* next change */ } };
    const timer = setTimeout(() => done(rejectFile, new Error(`${path} did not reach the expected state within ${deadlineMs}ms`)), deadlineMs);
    watcher = watch(dirname(path), evaluate);
    poll = setInterval(evaluate, 200);
    evaluate();
  });
}

const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (error) { return error?.code === 'EPERM'; } };
const waitGone = (pid, deadlineMs = 10_000) => new Promise((resolveGone, rejectGone) => {
  const timer = setTimeout(() => { clearInterval(poll); rejectGone(new Error(`pid ${pid} still alive after ${deadlineMs}ms`)); }, deadlineMs);
  const poll = setInterval(() => { if (!alive(pid)) { clearTimeout(timer); clearInterval(poll); resolveGone(true); } }, 50);
});

async function liveAgentChecks(check, stateRoot) {
  const dataRoot = join(stateRoot, 'f1l');
  const runId = 'u1';
  const runRoot = join(dataRoot, 'runs', runId);
  const agentDir = join(runRoot, 'agent');
  const common = ['--data-root', dataRoot, '--run-id', runId, '--orphan-ms', '120000'];
  const spawned = [];
  let serverPid = null;
  const steps = {};
  try {
    // 1. Start: the attach client launches the detached server and relays its lines.
    const one = attachClient(['--mode', 'start', ...common, '--after', '0']);
    spawned.push(one);
    const attached1 = await one.waitLine('attach 1', (l) => l.event === 'agent-attached');
    await one.waitLine('ready', (l) => l.event === 'agent-ready');
    serverPid = attached1.pid;
    one.send({ id: 1, op: 'init', runId, dataRoot, bun: process.execPath });
    await one.waitLine('init', (l) => l.id === 1);
    one.send({ id: 2, op: 'proxyStart', quotaResetMs: 180_000 });
    const proxyUrl = (await one.waitLine('proxy', (l) => l.id === 2)).result.url;
    const turn = await fetch(`${proxyUrl}/turn?label=q1&role=quota`);
    steps.start = { attached: attached1, quotaStatus: turn.status };

    // 2. Drop: the ssh session dies (client SIGKILLed). A pane line is appended while nothing
    //    is attached; the server must survive and persist it.
    const lastSeen = one.lines.reduce((max, l) => Math.max(max, Number(l.oseq) || 0), 0);
    one.child.kill('SIGKILL');
    await one.exited;
    const paneFile = join(runRoot, 'panes', 'pane-4242.jsonl');
    appendFileSync(paneFile, `${JSON.stringify({ label: 'quota-1', kind: 'done', atMs: Date.now() })}\n`);
    await waitFile(join(agentDir, 'out.jsonl'), (text) => text.includes('"kind":"done"'));
    steps.drop = { lastSeen, serverAliveAfterDrop: alive(serverPid) };

    // 3. Reattach: same server, only lines after lastSeen, the missed pane line exactly once; an
    //    already-received request id re-sent after the reconnect does not run again.
    const two = attachClient(['--mode', 'reattach', ...common, '--after', String(lastSeen)]);
    spawned.push(two);
    const attached2 = await two.waitLine('attach 2', (l) => l.event === 'agent-attached');
    await two.waitLine('missed pane line', (l) => l.event === 'pane' && l.record?.kind === 'done');
    two.send({ id: 2, op: 'proxyStart', quotaResetMs: 180_000 });
    two.send({ id: 3, op: 'proxyLog' });
    const log3 = await two.waitLine('proxyLog live', (l) => l.id === 3);
    steps.reattach = {
      attached: attached2,
      replayedOseqs: two.lines.filter((l) => Number.isSafeInteger(l.oseq)).map((l) => l.oseq),
      paneDone: two.lines.filter((l) => l.event === 'pane' && l.record?.kind === 'done').length,
      resentId2Replies: two.lines.filter((l) => l.id === 2).length,
      served: log3.result?.served?.length ?? null,
    };

    // 4. The server does not survive (SIGKILL): a reattach serves its on-disk record (orphan).
    two.child.kill('SIGKILL');
    await two.exited;
    process.kill(serverPid, 'SIGKILL');
    await waitGone(serverPid);
    appendFileSync(paneFile, `${JSON.stringify({ label: 'quota-1', kind: 'stdin-closed', atMs: Date.now() })}\n`);
    const lastSeen2 = two.lines.reduce((max, l) => Math.max(max, Number(l.oseq) || 0), 0);
    const three = attachClient(['--mode', 'reattach', ...common, '--after', String(lastSeen2)]);
    spawned.push(three);
    const attached3 = await three.waitLine('attach 3', (l) => l.event === 'agent-attached');
    await three.waitLine('pane line after server death', (l) => l.event === 'pane' && l.record?.kind === 'stdin-closed');
    three.send({ id: 4, op: 'proxyLog' });
    three.send({ id: 5, op: 'repo' });
    three.send({ id: 6, op: 'shutdown' });
    const log4 = await three.waitLine('proxyLog from disk', (l) => l.id === 4);
    const repo5 = await three.waitLine('repo refused', (l) => l.id === 5);
    const shut6 = await three.waitLine('orphan shutdown', (l) => l.id === 6);
    const exit3 = await three.exited;
    steps.orphan = { attached: attached3, log: log4.result, repo: repo5, shutdown: shut6, exit: exit3 };

    const cases = [
      ['start-launches-detached-server', attached1.serverAlive === true && Number.isInteger(serverPid) && serverPid !== one.child.pid && turn.status === 429],
      ['server-survives-client-drop', steps.drop.serverAliveAfterDrop === true],
      ['reattach-reaches-same-server', attached2.serverAlive === true && attached2.pid === serverPid && attached2.after === lastSeen],
      ['replay-only-after-cursor', steps.reattach.replayedOseqs.length > 0 && steps.reattach.replayedOseqs.every((n) => n > lastSeen)],
      ['missed-line-delivered-once', steps.reattach.paneDone === 1],
      ['resent-request-not-rerun', steps.reattach.resentId2Replies === 0],
      ['live-rpc-after-reattach', steps.reattach.served === 1],
      ['orphan-when-server-gone', attached3.serverAlive === false && attached3.server?.pid === serverPid],
      ['orphan-proxy-log-from-disk', log4.ok === true && log4.result?.fromDisk === true && log4.result?.served?.length === 1 && Number.isFinite(log4.result?.quotaUntil?.q1)],
      ['orphan-refuses-live-ops', repo5.ok === false && repo5.error?.code === 'AGENT_ORPHANED'],
      ['orphan-shutdown-by-record', shut6.ok === true && shut6.result?.orphan === true && Array.isArray(shut6.result?.daemons) && exit3.code === 0],
    ].map(([label, ok]) => ({ label, ok }));
    check('f1-host-agent-survives-and-reattaches', cases.every((entry) => entry.ok), { cases, steps });
  } catch (error) {
    check('f1-host-agent-survives-and-reattaches', false, { error: String(error?.message ?? error), steps, stderr: spawned.map((c) => c.stderr().slice(-400)) });
  } finally {
    for (const client of spawned) { try { client.child.kill('SIGKILL'); } catch { /* gone */ } }
    if (serverPid !== null && alive(serverPid)) { try { process.kill(serverPid, 'SIGKILL'); } catch { /* gone */ } }
  }
}

function linkArgChecks(check) {
  const target = { ssh: 'qa@203.0.113.7', sshOptions: ['-F', '/abs/ssh-config'], bun: '/usr/local/bin/bun', repoDir: '/srv/f1/ferryx', dataRoot: '/srv/f1/t' };
  const first = agentLinkArgs(target, 'run-1', { after: 0, attempt: 0 });
  const later = agentLinkArgs(target, 'run-1', { after: 57, attempt: 3 });
  const at = (args, name) => args[args.indexOf(name) + 1];
  const cases = [
    ['first-link-starts', at(first, '--mode') === 'start' && at(first, '--after') === '0'],
    ['later-link-reattaches-after-cursor', at(later, '--mode') === 'reattach' && at(later, '--after') === '57'],
    ['same-run-and-root', at(later, '--run-id') === 'run-1' && at(later, '--data-root') === '/srv/f1/t'],
    ['task-options-before-destination', first.indexOf('/abs/ssh-config') < first.indexOf('qa@203.0.113.7') && first.includes('BatchMode=yes')],
    ['orphan-outlives-budget', Number(at(first, '--orphan-ms')) > RECONNECT_BUDGET_MS],
  ].map(([label, ok]) => ({ label, ok }));
  check('f1-agent-link-args', cases.every((entry) => entry.ok), { cases, first, later });
}

export async function f1LinkUnitChecks(check, stateRoot) {
  appendChecks(check);
  linkArgChecks(check);
  await channelChecks(check);
  await liveAgentChecks(check, stateRoot);
}
