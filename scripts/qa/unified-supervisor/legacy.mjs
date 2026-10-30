// Scenario `legacy` (plan todo 16): reversible legacy adoption and cutover against an isolated
// daemon whose hub was upgraded from an omo snapshot.
//
// Fixture: a runner-owned omo database holding historical closed, failed and blocked lanes, one
// safe lane (blocked, no pane), a legacy Slack session with its transcript, and two cron jobs.
// `ferryx-cli hub migrate --from-omo` imports it into the isolated hub database before the
// daemon starts (the upgrade). The hub's mhc engine is a runner-owned wrapper that hides one
// required capability while `drop-capability` names it, so the missing-capability case and the
// happy path run on one daemon; only the hub generation restarts (`hub stop`/`hub start`), never
// the daemon, so every PTY survives.
//
// 1. Upgrade: the legacy intake is open, nothing is admitted, no lane is revived, no worker
//    starts, and a new submit is SUPERVISOR_INTAKE_CLOSED (503).
// 2. Missing capability: `hub cutover` is refused with MHC_CAPABILITY_MISSING and changes
//    nothing; the legacy transcript stays readable over the hub's HTTP API.
// 3. Cutover: the engine is restored; `hub cutover` commits the legacy stop, then the unified
//    intake. One explicit adoption of the safe lane starts one worker; a retry is the same goal.
// 4. Rollback: new scheduling stops (a submit is refused, nothing new is dispatched), the
//    adopted run's receipt and its pane are unchanged, the legacy intake is reopened.
// 5. Recovery: `hub cutover` again; the refused submit is admitted and starts.
// Cron jobs keep their row (payload, schedule, enabled) through every step. The hub database is
// only read, read-only.
import { mkdirSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { HarnessError, POSIX } from './harness.mjs';

export const LEGACY_REQUIRED_ASSERTIONS = Object.freeze([
  'omo-snapshot-imported', 'setup-created-identity-project-and-host', 'workspace-registered', 'engine-degraded-missing-capability',
  'upgrade-keeps-legacy-intake-open', 'upgrade-revived-no-lane', 'upgrade-started-no-worker', 'upgrade-admitted-nothing',
  'new-autonomy-unavailable-before-cutover',
  'cutover-refused-missing-capability', 'refused-cutover-changed-nothing', 'old-conversation-readable-without-capability',
  'engine-ready-after-restore', 'cutover-stopped-legacy-before-enabling-unified',
  'adoption-admitted-one-goal', 'adopted-worker-started-once', 'adoption-retry-is-duplicate', 'adoption-retry-started-nothing',
  'historical-lanes-not-revived', 'old-transcript-not-learned',
  'rollback-closed-unified-then-reopened-legacy', 'rollback-refuses-new-work', 'rollback-scheduled-nothing',
  'rollback-preserved-receipts', 'rollback-preserved-panes',
  'recutover-enabled-unified', 'recovered-request-admitted-and-started',
  'cron-jobs-untouched',
]);

const SAFE_LANE = 'qa-legacy-safe';
const HISTORY = Object.freeze([['qa-legacy-closed', 'closed'], ['qa-legacy-failed', 'failed'], ['qa-legacy-blocked', 'blocked']]);
const SESSION_KEY = 'slack:CQALEGACY:1700000000.000100';
const REQUIREMENTS = Object.freeze([{ id: 'r1', text: 'result.txt contains exactly READY', verifier: 'verify.sh exits 0' }]);

/** The runner-owned omo snapshot: legacy lanes, one Slack session with its transcript, cron. */
async function writeOmoSnapshot(path, { cwd, marker, transcriptMarker }) {
  let Database;
  try { ({ Database } = await import('bun:sqlite')); } catch {
    throw new HarnessError('SQLITE_UNAVAILABLE', 'the legacy scenario writes its omo snapshot with bun:sqlite; run the runner with bun');
  }
  mkdirSync(dirname(path), { recursive: true });
  const db = new Database(path, { create: true });
  try {
    db.exec(`CREATE TABLE lanes (key TEXT, title TEXT, lead_session_key TEXT, cwd TEXT, brief_path TEXT, session_key TEXT, omo_thread_id TEXT, status TEXT, done_signals TEXT, claim_evidence TEXT, claim_at TEXT, turns_run INTEGER, last_activity_at TEXT);
      CREATE TABLE sessions (session_key TEXT PRIMARY KEY, platform TEXT, guild_id TEXT, channel_id TEXT, thread_id TEXT, user_id TEXT, state_json TEXT, created_at TEXT, updated_at TEXT);
      CREATE TABLE messages (id TEXT PRIMARY KEY, session_key TEXT, role TEXT, content TEXT, metadata_json TEXT, created_at TEXT, sequence INTEGER);
      CREATE TABLE cron_jobs (id TEXT, session_key TEXT, expression TEXT, payload_json TEXT, enabled INTEGER, next_run_at TEXT, created_at TEXT);`);
    const lane = db.prepare('INSERT INTO lanes VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, NULL, NULL, 3, ?)');
    for (const [key, status] of HISTORY) lane.run(key, `History ${key}`, 'meta', cwd, '', 'meta', status, 'file marker', '2026-10-01T00:00:00Z');
    lane.run(SAFE_LANE, `Safe fixture ${marker}`, 'meta', cwd, '', 'meta', 'blocked', 'file marker', '2026-10-01T00:00:00Z');
    db.prepare('INSERT INTO sessions VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?)').run(SESSION_KEY, 'slack', 'CQALEGACY', '1700000000.000100', 'UQAOWNER', '{}', '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z');
    const message = db.prepare('INSERT INTO messages VALUES (?, ?, ?, ?, ?, ?, ?)');
    message.run('qa-legacy-m1', SESSION_KEY, 'user', `Keep the ledger reconciled. ${transcriptMarker}`, '{}', '2026-10-01T00:00:00Z', 1);
    message.run('qa-legacy-m2', SESSION_KEY, 'assistant', 'Reconciled; three entries matched.', '{}', '2026-10-01T00:00:01Z', 2);
    const cron = db.prepare('INSERT INTO cron_jobs VALUES (?, NULL, ?, ?, ?, ?, ?)');
    // Never due during the run: the scheduler leaves both rows as imported.
    cron.run('qa-cron-brief', '0 9 * * *', JSON.stringify({ command: 'echo qa-cron-brief', deliver: 'local' }), 1, '2099-01-01T00:00:00Z', '2026-10-01T00:00:00Z');
    cron.run('qa-cron-paused', '0 10 * * *', JSON.stringify({ command: 'echo qa-cron-paused', deliver: 'local' }), 0, '2099-01-01T00:00:00Z', '2026-10-01T00:00:00Z');
  } finally {
    db.close();
  }
}

export async function runLegacy(ctx, deps) {
  if (!POSIX) throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'legacy needs the POSIX fixture worker shell and the capability-filter wrapper');
  const mhc = await import('./mhc.mjs');
  const { config } = ctx;
  const transcriptLines = [];
  const note = (line) => {
    const stamped = `${new Date().toISOString()} ${line}`;
    transcriptLines.push(stamped);
    ctx.evidence.appendText('task-16-cutover.log', `${stamped}\n`);
    ctx.log(line);
  };
  const marker = `QA_LEGACY_SAFE_${ctx.runId}`;
  const transcriptMarker = `QA_LEGACY_TRANSCRIPT_${ctx.runId}`;
  const data = realpathSync(ctx.paths.data);
  const repo = join(data, 'ws', deps.project);
  const snapshot = join(data, 'omo', 'omon_gateway.db');
  await writeOmoSnapshot(snapshot, { cwd: repo, marker, transcriptMarker });
  note(`omo snapshot written at ${snapshot}: lanes ${HISTORY.map(([key, status]) => `${key}=${status}`).join(', ')}, ${SAFE_LANE}=blocked; session ${SESSION_KEY}; 2 cron jobs`);

  // The engine wrapper: the real mhc while drop-capability is empty, the capability filter
  // otherwise. Each engine launch reads the file; a hub restart relaunches the engine.
  const binDir = join(data, 'bin');
  mkdirSync(binDir, { recursive: true });
  const dropFile = join(binDir, 'mhc-drop-capability');
  const frames = join(binDir, 'mhc-filter-frames.jsonl');
  const wrapper = join(binDir, 'mhc-switchable');
  writeFileSync(dropFile, mhc.DROPPED_CAPABILITY);
  writeFileSync(wrapper, [
    '#!/bin/sh',
    `drop=$(cat ${deps.shellQuote(dropFile)} 2>/dev/null)`,
    `[ -z "$drop" ] && exec ${deps.shellQuote(config.mhcBin)} "$@"`,
    `UNIFIED_SUPERVISOR_MHC_REAL=${deps.shellQuote(config.mhcBin)}`,
    'UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY="$drop"',
    `UNIFIED_SUPERVISOR_MHC_FRAMES=${deps.shellQuote(frames)}`,
    'export UNIFIED_SUPERVISOR_MHC_REAL UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY UNIFIED_SUPERVISOR_MHC_FRAMES',
    `exec ${deps.shellQuote(process.execPath)} ${deps.shellQuote(mhc.MHC_FILTER)} "$@"`,
    '',
  ].join('\n'), { mode: 0o755 });

  const stub = await mhc.startProviderStub(ctx);
  try {
    let env = null;
    const h = await deps.supervisedHub(ctx, {
      workspaceId: 'qa-legacy',
      label: 'legacy',
      extraEnv: async (isolation) => {
        env = await mhc.prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: wrapper, label: 'legacy', extra: isolation });
        // The upgrade: the omo snapshot is imported before the supervisor is set up and before
        // the daemon (and with it the hub) starts.
        const imported = await ctx.cli(['hub', 'migrate', '--from-omo', snapshot], { label: 'legacy-import', env, deadlineMs: 60_000 });
        const lanes = /^lanes (\d+)$/m.exec(imported.stdout ?? '')?.[1];
        ctx.assert('omo-snapshot-imported', imported.code === 0 && Number(lanes) === HISTORY.length + 1, { code: imported.code, stdout: imported.stdout, stderr: imported.stderr });
        note(`hub migrate --from-omo exited ${imported.code}; lanes ${lanes}`);
        return env;
      },
    });
    const { hub } = h;
    const hubDir = dirname(h.dbPath);
    const cronRows = () => hub.all('SELECT id, expression, payload_json, enabled, next_run_at FROM scheduled_jobs ORDER BY id');
    const laneRows = () => hub.all("SELECT key, status, omo_thread_id FROM lanes WHERE key NOT LIKE 'sv-%' ORDER BY key");
    const cronBefore = cronRows();
    const lanesBefore = laneRows();
    ctx.evidence.writeJson('legacy/imported.json', { cron: cronBefore, lanes: lanesBefore });
    const cronSnapshots = [{ step: 'imported', rows: cronBefore }];
    const intake = async (step) => {
      const result = await ctx.cli(['hub', 'intake'], { label: `legacy-intake-${step}`, env });
      ctx.evidence.appendJsonl('legacy/intake.jsonl', { step, code: result.code, json: result.json, at: new Date().toISOString() });
      return result.json?.intake ?? null;
    };
    const hubOp = async (op, step) => {
      const result = await ctx.cli(['hub', op], { label: `legacy-${op}-${step}`, env });
      ctx.evidence.appendJsonl('legacy/control.jsonl', { step, op, code: result.code, json: result.json, at: new Date().toISOString() });
      note(`hub ${op} (${step}): exit ${result.code} ${JSON.stringify(result.json?.code ? { code: result.json.code } : { steps: result.json?.steps, intake: result.json?.intake })}`);
      return result;
    };
    let requestSeq = 0;
    const request = async (label, envelope) => {
      requestSeq += 1;
      const file = join(data, 'requests', `${String(requestSeq).padStart(2, '0')}-${label}.json`);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, JSON.stringify(envelope));
      ctx.evidence.appendJsonl('legacy/envelopes.jsonl', { seq: requestSeq, label, envelope, at: new Date().toISOString() });
      const result = await ctx.cli(['supervisor', 'request', '--json', file], { label: `legacy-${label}`, env });
      note(`${label}: exit ${result.code} ${JSON.stringify(result.json?.code ? { code: result.json.code, status: result.json.status } : { outcome: result.json?.receipt?.outcome, goal: result.json?.receipt?.receipt?.goal_id, duplicate: result.json?.receipt?.duplicate })}`);
      return result;
    };
    const projectId = hub.one('SELECT project_id FROM supervisor_projects LIMIT 1')?.project_id ?? null;

    // 1. Upgrade, with the engine missing a capability.
    const degraded = await mhc.awaitEngine(ctx, env, 'legacy-degraded', (json) => json?.engine?.state === 'degraded' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-degraded-missing-capability', degraded?.engine?.faultCode === 'MHC_CAPABILITY_MISSING', degraded?.engine ?? null);
    const upgraded = await intake('upgrade');
    ctx.assert('upgrade-keeps-legacy-intake-open', upgraded?.legacy === 'open' && upgraded?.unifiedOpen === false && upgraded?.legacyHistory === true, upgraded);
    ctx.assert('upgrade-revived-no-lane', JSON.stringify(laneRows()) === JSON.stringify(lanesBefore)
      && lanesBefore.every((row) => row.omo_thread_id === null), { lanes: laneRows() });
    const submitEnvelope = { requestId: `qa-legacy-new-${ctx.runId}`, projectId, action: 'submit', payload: { request: deps.work.instruction, work: deps.work } };
    const early = await request('submit-before-cutover', submitEnvelope);
    ctx.assert('new-autonomy-unavailable-before-cutover', early.code !== 0 && early.json?.code === 'SUPERVISOR_INTAKE_CLOSED' && early.json?.status === 503, early.json);
    ctx.assert('upgrade-admitted-nothing', hub.count('SELECT COUNT(*) AS n FROM supervisor_goals') === 0
      && hub.count('SELECT COUNT(*) AS n FROM supervisor_dispatches') === 0, {});
    ctx.assert('upgrade-started-no-worker', deps.readWorkerLogs(h.workerLogs).length === 0, {});

    // 2. Missing capability: the cutover is refused and nothing changes.
    const refused = await hubOp('cutover', 'missing-capability');
    ctx.assert('cutover-refused-missing-capability', refused.code !== 0 && refused.json?.status === 'failed' && refused.json?.code === 'MHC_CAPABILITY_MISSING', refused.json);
    const afterRefusal = await intake('after-refused-cutover');
    ctx.assert('refused-cutover-changed-nothing', afterRefusal?.legacy === 'open' && afterRefusal?.unifiedOpen === false
      && hub.count('SELECT COUNT(*) AS n FROM supervisor_cutover_log') === 0, afterRefusal);
    // The hub's HTTP listener binds asynchronously after the hub opens.
    const bound = await deps.hubStatusUntil(ctx, env, 'legacy-http', { accept: (result) => result.code === 0 && result.json?.http?.bound === true });
    if (!bound.ok) throw new HarnessError('HUB_HTTP_UNBOUND', 'the hub HTTP listener did not bind', bound.result?.json ?? null);
    const addr = env.FERRYX_HUB_TRACKER_ADDR;
    let transcript = null;
    try {
      const response = await fetch(`http://${addr}/api/sessions/${encodeURIComponent(SESSION_KEY)}/messages`, { signal: ctx.signal });
      transcript = { status: response.status, body: await response.json() };
    } catch (error) { transcript = { error: String(error?.message ?? error) }; }
    ctx.evidence.writeJson('legacy/old-conversation.json', transcript);
    ctx.assert('old-conversation-readable-without-capability', transcript?.status === 200 && transcript.body?.total === 2
      && transcript.body.items?.[0]?.content?.includes(transcriptMarker), transcript);

    // 3. Engine restored (hub generation restart only), then cutover and one adoption.
    writeFileSync(dropFile, '');
    const childrenBefore = await h.children('before-restore');
    if (!await deps.hubControl(ctx, env, 'stop', 'legacy-restore')) throw new HarnessError('HUB_CONTROL_FAILED', 'hub stop failed');
    if (!await deps.hubControl(ctx, env, 'start', 'legacy-restore')) throw new HarnessError('HUB_CONTROL_FAILED', 'hub start failed');
    const ready = await mhc.awaitEngine(ctx, env, 'legacy-ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-ready-after-restore', ready?.engine?.state === 'ready', ready?.engine ?? null);
    const cut = await hubOp('cutover', 'first');
    const firstLog = hub.all('SELECT step FROM supervisor_cutover_log ORDER BY id').map((row) => row.step);
    ctx.assert('cutover-stopped-legacy-before-enabling-unified', cut.code === 0 && JSON.stringify(cut.json?.steps) === JSON.stringify(['legacy_intake_stopped', 'unified_intake_enabled'])
      && JSON.stringify(firstLog) === JSON.stringify(['legacy_intake_stopped', 'unified_intake_enabled'])
      && cut.json?.intake?.legacy === 'stopped' && cut.json?.intake?.unifiedOpen === true, { reply: cut.json, log: firstLog });
    cronSnapshots.push({ step: 'cutover', rows: cronRows() });

    const adoptEnvelope = (requestId) => ({ requestId, projectId, action: 'adopt', payload: { laneKey: SAFE_LANE, requirements: REQUIREMENTS } });
    const adopted = await request('adopt', adoptEnvelope(`qa-legacy-adopt-${ctx.runId}`));
    const goalId = adopted.json?.receipt?.receipt?.goal_id ?? null;
    ctx.assert('adoption-admitted-one-goal', adopted.code === 0 && adopted.json?.receipt?.outcome === 'admitted' && adopted.json?.receipt?.duplicate === false
      && typeof goalId === 'string' && hub.count('SELECT COUNT(*) AS n FROM supervisor_goals') === 1, adopted.json);
    if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the adoption admitted no goal');
    const started = await deps.waitForChange(ctx, 'adopted run started', hubDir, () => {
      const rows = deps.goalRows(hub, goalId);
      return { done: rows.dispatches.some((row) => row.state === 'started') || deps.heldStates.has(rows.goal?.run_state), ...rows };
    }, 120_000);
    ctx.evidence.writeJson('legacy/adopted-goal.json', started);
    const deliveredOnce = deps.inputsWith(h.workerLogs, marker);
    ctx.assert('adopted-worker-started-once', started.goal?.run_state === 'running' && started.dispatches.length === 1
      && deliveredOnce.length === 1, { started, deliveredOnce });
    const retried = await request('adopt-retry', adoptEnvelope(`qa-legacy-adopt-retry-${ctx.runId}`));
    ctx.assert('adoption-retry-is-duplicate', retried.code === 0 && retried.json?.receipt?.duplicate === true
      && retried.json?.receipt?.receipt?.goal_id === goalId, retried.json);
    ctx.assert('adoption-retry-started-nothing', hub.count('SELECT COUNT(*) AS n FROM supervisor_goals') === 1
      && hub.count('SELECT COUNT(*) AS n FROM supervisor_dispatches') === 1 && deps.inputsWith(h.workerLogs, marker).length === 1, {});
    const historical = laneRows().filter((row) => HISTORY.some(([key]) => key === row.key));
    ctx.assert('historical-lanes-not-revived', HISTORY.every(([key, status]) => historical.some((row) => row.key === key && row.status === status && row.omo_thread_id === null)), { historical });
    const learned = hub.count("SELECT COUNT(*) AS n FROM supervisor_memory_sources WHERE body LIKE ?", `%${transcriptMarker}%`);
    ctx.assert('old-transcript-not-learned', learned === 0, { learned });

    // 4. Rollback: new scheduling stops; receipts and panes stay.
    const panesBefore = await h.panes('before-rollback');
    const receiptsBefore = deps.goalRows(hub, goalId).dispatches;
    const childrenRollback = await h.children('before-rollback');
    const rolled = await hubOp('rollback', 'toggle');
    const rollLog = hub.all('SELECT step FROM supervisor_cutover_log ORDER BY id').map((row) => row.step).slice(2);
    ctx.assert('rollback-closed-unified-then-reopened-legacy', rolled.code === 0 && JSON.stringify(rolled.json?.steps) === JSON.stringify(['unified_intake_closed', 'legacy_intake_reopened'])
      && JSON.stringify(rollLog) === JSON.stringify(['unified_intake_closed', 'legacy_intake_reopened'])
      && rolled.json?.intake?.legacy === 'open' && rolled.json?.intake?.unifiedOpen === false, { reply: rolled.json, log: rollLog });
    cronSnapshots.push({ step: 'rollback', rows: cronRows() });
    const blocked = await request('submit-after-rollback', submitEnvelope);
    ctx.assert('rollback-refuses-new-work', blocked.code !== 0 && blocked.json?.code === 'SUPERVISOR_INTAKE_CLOSED' && blocked.json?.status === 503, blocked.json);
    ctx.assert('rollback-scheduled-nothing', hub.count('SELECT COUNT(*) AS n FROM supervisor_goals') === 1
      && hub.count('SELECT COUNT(*) AS n FROM supervisor_dispatches') === 1 && deps.readWorkerLogs(h.workerLogs).length === 1, {});
    const receiptsAfter = deps.goalRows(hub, goalId).dispatches;
    ctx.assert('rollback-preserved-receipts', JSON.stringify(receiptsAfter) === JSON.stringify(receiptsBefore)
      && deps.goalRows(hub, goalId).goal?.run_state === 'running', { receiptsBefore, receiptsAfter });
    const panesAfter = await h.panes('after-rollback');
    const childrenAfter = await h.children('after-rollback');
    ctx.assert('rollback-preserved-panes', panesBefore.length >= 1 && panesBefore.every((id) => panesAfter.includes(id))
      && deps.survived(childrenRollback, childrenAfter) && deps.survived(childrenBefore, childrenAfter), { panesBefore, panesAfter });

    // 5. Recovery: cut over again; the refused request is admitted and starts.
    const again = await hubOp('cutover', 'recovery');
    ctx.assert('recutover-enabled-unified', again.code === 0 && again.json?.intake?.unifiedOpen === true && again.json?.intake?.legacy === 'stopped', again.json);
    cronSnapshots.push({ step: 'recutover', rows: cronRows() });
    const recovered = await request('submit-after-recutover', submitEnvelope);
    const recoveredGoal = recovered.json?.receipt?.receipt?.goal_id ?? null;
    let recoveredRows = null;
    if (recoveredGoal) {
      recoveredRows = await deps.waitForChange(ctx, 'recovered run started', hubDir, () => {
        const rows = deps.goalRows(hub, recoveredGoal);
        return { done: rows.dispatches.some((row) => row.state === 'started') || deps.heldStates.has(rows.goal?.run_state), ...rows };
      }, 120_000);
    }
    ctx.evidence.writeJson('legacy/recovered-goal.json', { reply: recovered.json, rows: recoveredRows });
    ctx.assert('recovered-request-admitted-and-started', recovered.code === 0 && recovered.json?.receipt?.outcome === 'admitted'
      && recoveredRows?.goal?.run_state === 'running', { reply: recovered.json, rows: recoveredRows });

    ctx.evidence.writeJson('legacy/cron.json', cronSnapshots);
    ctx.assert('cron-jobs-untouched', cronBefore.length === 2
      && cronSnapshots.every((snapshot) => JSON.stringify(snapshot.rows) === JSON.stringify(cronBefore)), { cronSnapshots });
  } finally {
    await stub.close();
    ctx.evidence.writeText('legacy/transcript.txt', `${transcriptLines.join('\n')}\n`);
  }
}
