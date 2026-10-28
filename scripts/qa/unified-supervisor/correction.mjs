// Scenario `correction` (plan todo 10): an owner correction lands before the first dispatch is
// sent, and a replay of it at the stale revision is refused.
//
// The first dispatch is held without any product hook: the hub generation is stopped (`ferryx-cli
// hub stop`, as recovery's before-send boundary does), so the admitted goal's ready intent has no
// dispatcher to claim it. The owner then corrects the goal ("Use DONE instead of READY") through
// `ferryx-cli supervisor request` (the daemon's SupervisorRequestV1, the desktop surface), which
// bumps the revision, supersedes the never-sent READY dispatch and queues the corrected one. The
// same correction replayed at the old expectedRevision is REVISION_CONFLICT (409) and writes
// nothing. Only then does the hub start again: the dispatcher sends the corrected instruction
// once, the fixture worker (--multi-turn --obey-correction) writes what the correction names, the
// hub runs verify.sh (which accepts exactly DONE) and grades on the retained mhc engine (a
// loopback provider stub), and the goal completes. The hub database is only read, read-only.
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { HarnessError, isInside, POSIX } from './harness.mjs';

export const CORRECTION_REQUIRED_ASSERTIONS = Object.freeze([
  'engine-ready', 'workspace-registered', 'hub-running-before-submit', 'hub-stopped-before-submit',
  'submit-admitted-ready-unsent', 'correction-landed-before-dispatch', 'old-dispatch-superseded-before-send',
  'stale-revision-replay-409', 'stale-replay-changed-nothing', 'hub-restarted', 'engine-ready-after-restart',
  'goal-completed', 'old-dispatch-never-sent', 'corrected-dispatch-ran-once', 'worker-received-only-the-correction',
  'final-bytes-done-and-verifier-passes', 'verified-names-corrected-run-and-revision', 'fixture-workers-gone',
]);

const WORKSPACE_ID = 'qa-correction';
const PROJECT = 'qa-correction';
const BODY = 'Create result.txt containing READY. Run the verifier. Correct failures until it passes.';
const WORK = Object.freeze({
  title: 'Write result.txt',
  instruction: 'Create result.txt containing READY. Run verify.sh. Correct failures until it passes.',
  requirements: [{ id: 'r1', text: 'result.txt contains exactly READY', verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test'],
});
const CORRECTION = 'Use DONE instead of READY';
const CORRECTED_REQUIREMENTS = Object.freeze([{ id: 'r1', text: 'result.txt contains exactly DONE', verifier: 'verify.sh exits 0' }]);
// The fixture repository's verifier accepts only the corrected bytes.
const VERIFY = 'test "$(cat result.txt)" = DONE\n';

export async function runCorrection(ctx, deps) {
  if (!POSIX) {
    throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'correction runs a POSIX fixture worker pane (the Windows pane shell ignores SHELL); this scenario runs on Linux or macOS');
  }
  const { awaitEngine, prepareEngineEnv, startProviderStub } = await import('./mhc.mjs');
  const { config } = ctx;
  const git = deps.onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the correction fixture project is a git repository');
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `hub database ${dbPath} is outside the run root`);
  const hubDir = dirname(dbPath);
  const log = [];
  const note = (line) => { log.push(`${new Date().toISOString()} ${line}`); ctx.log(line); };

  // Fixture: a committed verify.sh (every lane worktree has it) in a workspace checkout.
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, PROJECT);
  mkdirSync(repo, { recursive: true });
  writeFileSync(join(repo, 'verify.sh'), VERIFY, { mode: 0o755 });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [
    ['git-init', [...gitArgs, 'init', '-q', repo]],
    ['git-add', [...gitArgs, '-C', repo, 'add', 'verify.sh']],
    ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '-m', 'qa fixture verifier']],
  ]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const gitCommonDir = realpathSync(join(repo, '.git'));
  const workerLogs = join(data, 'workers');
  const requestsDir = join(data, 'requests');
  for (const dir of [workerLogs, requestsDir]) mkdirSync(dir, { recursive: true });
  const shell = join(data, 'bin', 'qa-worker-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${deps.shellQuote(process.execPath)} ${deps.shellQuote(deps.worker)} ${deps.shellQuote(workerLogs)} --multi-turn --obey-correction\n`, { mode: 0o755 });

  const stub = await startProviderStub(ctx, { answer: (json) => (deps.promptTextOf(json).includes(deps.convergenceMarker) ? deps.converged : null) });
  try {
    const env = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: config.mhcBin, label: 'correction', extra: {
      SHELL: shell,
      FERRYX_HUB_WORKSPACE: WORKSPACE_ID,
      OMON_LANE_AUTO_WORKTREE: '1',
      OMON_INBOX_ENABLED: '0',
    } });
    ctx.announce({ correction: 'fixture', workspaceRoot, repo, gitCommonDir, hubHttp: env.FERRYX_HUB_TRACKER_ADDR, dbPath, provider: stub.baseUrl });

    const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', PROJECT, '--repo-identity', `qa:${PROJECT}`,
      '--host', 'local', '--git-common-dir', gitCommonDir, '--json'], { label: 'supervisor-setup', env });
    if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${JSON.stringify(setup.json)} ${setup.stderr}`);
    const projectId = setup.json.projectId;
    const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
    const engine = await awaitEngine(ctx, env, 'correction-ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-ready', engine?.engine?.state === 'ready', { engine });
    const registered = await deps.registerWorkspace(ctx, join(data, 'runtime', 'daemon.sock'), WORKSPACE_ID, realpathSync(workspaceRoot));
    ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });

    // The owner's channel: one envelope file per request through `ferryx-cli supervisor request`.
    let requestSeq = 0;
    const request = async (label, envelope) => {
      requestSeq += 1;
      const file = join(requestsDir, `${String(requestSeq).padStart(2, '0')}-${label}.json`);
      writeFileSync(file, JSON.stringify(envelope));
      ctx.evidence.appendJsonl('envelopes.jsonl', { seq: requestSeq, label, envelope, at: new Date().toISOString() });
      const result = await ctx.cli(['supervisor', 'request', '--json', file], { label, env });
      note(`${label}: exit ${result.code} ${JSON.stringify(result.json)}`);
      return result;
    };
    const hub = await deps.openHubReader(dbPath);
    const rows = (goalId) => hub.read((query) => ({
      goal: query('SELECT run_state, state_detail, revision, lane_key FROM supervisor_goals WHERE goal_id = ?', goalId)[0] ?? null,
      dispatches: query('SELECT request_id, goal_revision, state, run_id, provider_session_id, instruction, settled_at FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId),
    }));
    const brief = (snapshot) => ({ ...snapshot, dispatches: snapshot.dispatches.map(({ instruction, ...row }) => row) });

    // 1. Hold the first dispatch: stop the hub generation (dispatcher included); the daemon and
    //    its request surface stay up.
    const up = await deps.hubStatusUntil(ctx, env, 'correction-up', { accept: deps.hubRunning });
    ctx.assert('hub-running-before-submit', up.ok, { attempt: up.attempt, code: up.result.code, json: up.result.json, stderr: up.result.stderr });
    const stopped = await deps.hubControl(ctx, env, 'stop', 'correction-hold');
    const down = await ctx.cli(['hub', 'status'], { label: 'hub-status-correction-down', env });
    ctx.assert('hub-stopped-before-submit', stopped && down.code === 0 && down.json?.running === false && down.json?.stopped_flag === true,
      { stopped, status: down.json, stderr: down.stderr });

    // 2. The READY request is admitted; its dispatch stays an unsent ready intent.
    const sourceId = `qa-correction-${ctx.runId}`;
    const submitted = await request('submit', { requestId: sourceId, projectId, action: 'submit', payload: { request: BODY, work: WORK } });
    const goalId = submitted.json?.receipt?.receipt?.goal_id ?? null;
    const admitted = goalId ? rows(goalId) : null;
    ctx.assert('submit-admitted-ready-unsent', submitted.code === 0 && submitted.json?.receipt?.outcome === 'admitted' && typeof goalId === 'string'
      && admitted?.goal?.run_state === 'ready' && admitted.dispatches.length === 1 && admitted.dispatches[0].state === 'ready'
      && admitted.dispatches[0].instruction.includes(WORK.instruction) && deps.readWorkerLogs(workerLogs).length === 0,
    { code: submitted.code, json: submitted.json, rows: admitted && brief(admitted) });
    if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the submit returned no goal; nothing can be corrected');
    const admittedRevision = admitted.goal.revision;
    const oldDispatch = admitted.dispatches[0];

    // 3. The owner correction, at the admitted revision, before anything was sent.
    const correctEnvelope = { requestId: `qa-correction-correct-${ctx.runId}`, projectId, goalId, expectedRevision: admittedRevision,
      action: 'correct', payload: { text: CORRECTION, requirements: CORRECTED_REQUIREMENTS } };
    const corrected = await request('correct', correctEnvelope);
    const afterCorrect = rows(goalId);
    const newRequestId = corrected.json?.receipt?.dispatchRequestId ?? null;
    const newDispatch = afterCorrect.dispatches.find((row) => row.request_id === newRequestId) ?? null;
    ctx.assert('correction-landed-before-dispatch', corrected.code === 0 && corrected.json?.ok === true && corrected.json?.receipt?.revision === admittedRevision + 1
      && corrected.json?.receipt?.carriedOriginal === true && afterCorrect.goal?.revision === admittedRevision + 1 && afterCorrect.goal?.run_state === 'ready'
      && newDispatch?.state === 'ready' && newDispatch.goal_revision === admittedRevision + 1 && newDispatch.instruction.includes(CORRECTION)
      && deps.readWorkerLogs(workerLogs).length === 0,
    { code: corrected.code, json: corrected.json, rows: brief(afterCorrect), newInstruction: newDispatch?.instruction ?? null });
    ctx.assert('old-dispatch-superseded-before-send', afterCorrect.dispatches.length === 2
      && afterCorrect.dispatches[0].request_id === oldDispatch.request_id && afterCorrect.dispatches[0].state === 'superseded'
      && afterCorrect.dispatches[0].goal_revision === admittedRevision && afterCorrect.dispatches[0].run_id === null, { rows: brief(afterCorrect) });

    // 4. The same correction replayed at the stale revision: 409, nothing written.
    const before = JSON.stringify(rows(goalId));
    const stale = await request('correct-stale-replay', correctEnvelope);
    const after = JSON.stringify(rows(goalId));
    ctx.assert('stale-revision-replay-409', stale.code !== 0 && stale.json?.ok === false && stale.json?.code === 'REVISION_CONFLICT' && stale.json?.status === 409,
      { code: stale.code, json: stale.json });
    ctx.assert('stale-replay-changed-nothing', before === after && deps.readWorkerLogs(workerLogs).length === 0, { before: JSON.parse(before), after: JSON.parse(after) });

    // 5. Release: the hub starts again; its dispatcher drains the only ready intent.
    ctx.assert('hub-restarted', await deps.hubControl(ctx, env, 'start', 'correction-release'), {});
    const restarted = await awaitEngine(ctx, env, 'correction-restarted', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-ready-after-restart', restarted?.engine?.state === 'ready', { engine: restarted });
    const ended = await deps.waitForChange(ctx, 'corrected goal to complete or hold', hubDir, () => {
      const snapshot = rows(goalId);
      return { done: deps.heldStates.has(snapshot.goal?.run_state), goal: snapshot.goal };
    }, 6 * 60_000);
    const final = rows(goalId);
    ctx.assert('goal-completed', final.goal?.run_state === 'completed', { goal: final.goal, observed: ended.goal });
    const [first, second] = final.dispatches;
    ctx.assert('old-dispatch-never-sent', first?.request_id === oldDispatch.request_id && first.state === 'superseded'
      && first.run_id === null && first.settled_at === null, { rows: brief(final) });
    ctx.assert('corrected-dispatch-ran-once', final.dispatches.length === 2 && second?.request_id === newRequestId && second.state === 'settled'
      && second.goal_revision === admittedRevision + 1 && typeof second.run_id === 'string', { rows: brief(final) });

    // What the worker actually received: one instruction, the corrected one; it wrote DONE.
    const workers = deps.readWorkerLogs(workerLogs);
    const instructions = workers.flatMap((entry) => entry.entries.filter((line) => line.kind === 'instruction'));
    const turns = workers.flatMap((entry) => entry.entries.filter((line) => line.kind === 'turn'));
    ctx.assert('worker-received-only-the-correction', workers.length === 1 && instructions.length === 1
      && instructions[0].text.includes(CORRECTION) && instructions[0].text.includes(`Owner correction at goal revision ${admittedRevision + 1}`)
      && turns.length === 1 && turns[0].wrote === 'DONE' && turns[0].verifyExit === 0, { instructions, turns });

    // The runner checks the final bytes itself, in the lane's checkout.
    const lane = hub.one('SELECT cwd FROM lanes WHERE key = ?', final.goal?.lane_key ?? '');
    const cwd = lane?.cwd ?? null;
    const check = cwd ? await ctx.run('verify-final', '/bin/sh', ['verify.sh'], { cwd, deadlineMs: 20_000 }) : null;
    const bytes = cwd && existsSync(join(cwd, 'result.txt')) ? readFileSync(join(cwd, 'result.txt'), 'utf8') : null;
    ctx.assert('final-bytes-done-and-verifier-passes', check?.code === 0 && bytes === 'DONE', { cwd, code: check?.code, bytes });
    let verified = null;
    try { verified = JSON.parse((final.goal?.state_detail ?? '').replace(/^VERIFIED: /, '')); } catch { /* asserted below */ }
    const requirement = hub.one("SELECT text, status FROM lane_goal_requirements WHERE lane_key = ? AND req_id = 'r1'", final.goal?.lane_key ?? '');
    ctx.assert('verified-names-corrected-run-and-revision', final.goal?.state_detail?.startsWith('VERIFIED: ') && verified?.runId === second?.run_id
      && verified?.requestId === newRequestId && verified?.goalRevision === final.goal?.revision && verified?.evidenceRefs?.includes('sig:req:r1')
      && requirement?.status === 'met' && requirement?.text === CORRECTED_REQUIREMENTS[0].text, { detail: final.goal?.state_detail, verified, requirement });

    // Cleanup: stop the daemon; the fixture worker must exit by itself.
    const workerPids = workers.map((entry) => Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])).filter(Number.isInteger);
    const cleanExit = new Set(['stdin-closed', 'hangup', 'terminated']);
    await ctx.terminate(daemon, 'worker-cleanup');
    const closed = await deps.waitForChange(ctx, 'fixture worker exit', workerLogs, () => {
      const open = deps.readWorkerLogs(workerLogs).filter((entry) => !entry.entries.some((line) => cleanExit.has(line.kind)));
      return { done: open.length === 0, open: open.map((entry) => entry.name) };
    }, 30_000).then(() => true, (error) => { if (ctx.signal.aborted) throw error; return false; });
    const alive = await deps.stillAlive(workerPids);
    ctx.assert('fixture-workers-gone', workerPids.length === 1 && closed && alive.length === 0, { workerPids, closed, alive });

    ctx.evidence.writeJson('correction.json', {
      worker: { kind: 'fixture', note: 'fixture-worker.mjs --multi-turn --obey-correction: writes the word an owner "Use X instead of Y" line names, runs verify.sh, reports working then idle. Not a real provider worker.' },
      model: { kind: 'loopback stub', note: 'The stub always answers complete and cites sig:req:r1; the check Ferryx ran decides.' },
      hold: 'the first dispatch is held by stopping the hub generation (ferryx-cli hub stop) before the submit; no product hook',
      sourceId, goalId, admittedRevision, oldDispatch: { requestId: oldDispatch.request_id }, newRequestId,
      correct: corrected.json, stale: stale.json, final: brief(final), bytes,
    });
  } finally {
    for (const entry of deps.readWorkerLogs(workerLogs)) ctx.evidence.writeText(`workers/${entry.name}`, entry.text);
    ctx.evidence.writeText('task-10-correction.log', `${log.join('\n')}\n`);
    await stub.close();
  }
}
