// summary.json schema for the daemon-lifetime QA harness (plan todo 12).

export const SUMMARY_SCHEMA = 'ferryx-daemon-lifetime-qa/v1';
export const STATUSES = Object.freeze(['PASS', 'FAIL', 'BLOCKED', 'NOT_APPLICABLE', 'NOT_RUN']);
export const PLATFORMS = Object.freeze(['mac', 'linux', 'windows']);

/** Every scenario of todo 12 with the platforms it applies to. */
export const SCENARIOS = Object.freeze([
  { id: 'Q00', title: 'daemon owned by the QA OS job (precondition)', platforms: PLATFORMS },
  { id: 'Q01', title: 'normal GUI quit keeps daemon and children', platforms: PLATFORMS },
  { id: 'Q02', title: 'kill of the recorded GUI pid keeps daemon and children', platforms: PLATFORMS },
  { id: 'Q03', title: 'OS-attributed GUI termination keeps daemon', platforms: PLATFORMS },
  { id: 'Q04', title: 'listener stall beyond SUPERVISED_START_WAIT: zero supervisor mutations', platforms: PLATFORMS },
  { id: 'Q04b', title: 'loaded-idle QA job: one plain kickstart', platforms: ['mac'] },
  { id: 'Q05', title: 'probe faults classify Unknown: zero mutations', platforms: PLATFORMS },
  { id: 'Q06', title: 'two cold GUI starts plus forced fallback: one owner', platforms: PLATFORMS },
  { id: 'Q06w', title: 'Windows detach guard: no-breakaway job refuses the detached fallback', platforms: ['windows'] },
  { id: 'Q07', title: 'bundle A replaced by B on disk: A generation intact, B attaches', platforms: PLATFORMS },
  { id: 'Q08', title: 'A->B->C control upgrades with local and SSH sessions', platforms: ['mac', 'linux'] },
  { id: 'Q08b', title: 'cross-revision handover-v1 owner meets HEAD GUI', platforms: PLATFORMS },
  { id: 'Q09', title: 'Windows protocol mismatch pending state, confirm, stale count, RetireWhenIdle', platforms: ['windows'] },
  { id: 'Q10', title: 'autostart opt-out toggle and opted-out launch', platforms: PLATFORMS },
  { id: 'Q10b', title: 'headless: job start plus CLI client, no GUI', platforms: PLATFORMS },
  { id: 'Q11', title: 'SSH session survives GUI quit; uncertain write never replayed', platforms: PLATFORMS },
  { id: 'Q12', title: 'exited and disconnected entries never satisfy live assertions', platforms: PLATFORMS },
  { id: 'Q13', title: 'legacy single job migration without stopping its owner', platforms: PLATFORMS },
]);

export const SCENARIO_IDS = Object.freeze(SCENARIOS.map((s) => s.id));

export function scenarioById(id) {
  const found = SCENARIOS.find((s) => s.id === id);
  if (!found) throw new Error(`unknown scenario ${id}`);
  return found;
}

export function applicable(id, platform) {
  return scenarioById(id).platforms.includes(platform);
}

/** Selects scenario ids from --all / --only Q01,Q02 for a platform, Q00 always first. */
export function selectScenarios({ all = false, only = [] }, platform) {
  const wanted = all ? SCENARIO_IDS : only;
  for (const id of wanted) scenarioById(id);
  const ids = SCENARIO_IDS.filter((id) => wanted.includes(id) || id === 'Q00');
  return ids;
}

export function newScenarioResult(id, platform) {
  return {
    id,
    title: scenarioById(id).title,
    applicable: applicable(id, platform),
    status: applicable(id, platform) ? 'NOT_RUN' : 'NOT_APPLICABLE',
    startedAt: null,
    durationMs: null,
    assertions: [],
    evidence: [],
    identities: {},
    barrier: null,
  };
}

export function newSummary({ runId, platform, host, revisions = {}, startedAt = new Date().toISOString() }) {
  if (!PLATFORMS.includes(platform)) throw new Error(`unknown platform ${platform}`);
  return {
    schema: SUMMARY_SCHEMA,
    runId,
    platform,
    host,
    startedAt,
    finishedAt: null,
    revisions: { head: null, handoverV1Rev: null, baselineRev: null, ...revisions },
    hooks: {},
    artifacts: {},
    releaseDaemon: { path: null, before: null, after: null, unchanged: null, state: null },
    scenarios: SCENARIO_IDS.map((id) => newScenarioResult(id, platform)),
    carryover: [],
    teardown: { killed: [], refused: [], jobsRemoved: [], dirsRemoved: [], leftovers: [] },
    exitCode: null,
  };
}

/** Records one assertion; a failed one turns the scenario FAIL when it finishes. */
export function assertion(result, name, ok, detail = null) {
  result.assertions.push({ name, ok: Boolean(ok), detail });
  return Boolean(ok);
}

/** Final status of a scenario from its assertions; BLOCKED and NOT_APPLICABLE are kept. */
export function settle(result) {
  if (result.status === 'BLOCKED' || result.status === 'NOT_APPLICABLE') return result;
  if (result.assertions.length === 0) {
    result.status = 'FAIL';
    result.assertions.push({ name: 'scenario produced assertions', ok: false, detail: 'no assertion was recorded' });
    return result;
  }
  if (result.evidence.length === 0) {
    result.status = 'FAIL';
    result.assertions.push({ name: 'scenario wrote raw evidence', ok: false, detail: 'a scenario counts only with its raw log' });
    return result;
  }
  result.status = result.assertions.every((a) => a.ok) ? 'PASS' : 'FAIL';
  return result;
}

export function block(result, kind, detail) {
  result.status = 'BLOCKED';
  result.barrier = { kind, detail };
  return result;
}

/** Q00 gates every other scenario: a non-PASS Q00 blocks the rest. */
export function applyQ00Gate(summary) {
  const q00 = summary.scenarios.find((s) => s.id === 'Q00');
  if (q00.status === 'PASS') return false;
  for (const s of summary.scenarios) {
    if (s.id !== 'Q00' && s.applicable && s.status === 'NOT_RUN') block(s, 'Q00_PRECONDITION', `Q00 is ${q00.status}`);
  }
  return true;
}

/** 0 only when every selected applicable scenario passed, teardown is clean and production untouched. */
export function computeExitCode(summary, selected = SCENARIO_IDS) {
  const relevant = summary.scenarios.filter((s) => s.applicable && selected.includes(s.id));
  const allPass = relevant.length > 0 && relevant.every((s) => s.status === 'PASS');
  const clean = summary.teardown.leftovers.length === 0 && summary.teardown.refused.length === 0;
  const prodOk = summary.releaseDaemon.unchanged !== false;
  return allPass && clean && prodOk ? 0 : 1;
}

/** Structural validation; returns a list of problems (empty = valid). */
export function validateSummary(summary) {
  const problems = [];
  if (summary?.schema !== SUMMARY_SCHEMA) problems.push(`schema must be ${SUMMARY_SCHEMA}`);
  if (!/^\d{8}t\d{6}z$/.test(String(summary?.runId))) problems.push('runId is not a harness run id');
  if (!PLATFORMS.includes(summary?.platform)) problems.push('platform is invalid');
  const ids = (summary?.scenarios ?? []).map((s) => s.id);
  for (const id of SCENARIO_IDS) if (!ids.includes(id)) problems.push(`scenario ${id} missing`);
  for (const s of summary?.scenarios ?? []) {
    if (!STATUSES.includes(s.status)) problems.push(`${s.id}: bad status ${s.status}`);
    if (s.applicable !== applicable(s.id, summary.platform)) problems.push(`${s.id}: applicability wrong`);
    if (!s.applicable && s.status !== 'NOT_APPLICABLE') problems.push(`${s.id}: not applicable but ${s.status}`);
    if (s.status === 'PASS' && (!s.evidence.length || !s.assertions.length || !s.assertions.every((a) => a.ok))) {
      problems.push(`${s.id}: PASS without evidence or with a failed assertion`);
    }
    if (s.status === 'BLOCKED' && !(s.barrier?.kind && s.barrier?.detail)) problems.push(`${s.id}: BLOCKED without an exact barrier`);
  }
  for (const key of ['killed', 'refused', 'jobsRemoved', 'dirsRemoved', 'leftovers']) {
    if (!Array.isArray(summary?.teardown?.[key])) problems.push(`teardown.${key} must be an array`);
  }
  if (summary?.exitCode !== null && summary?.exitCode !== undefined && ![0, 1, 2].includes(summary.exitCode)) problems.push('exitCode invalid');
  return problems;
}
