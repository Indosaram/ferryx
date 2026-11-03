// F3 real-surface acceptance contract (supervisor-chat-redesign plan F3, as restated by the
// owner): scenario ids, the clause each proves, the lanes and raw artifact kinds each needs, and
// the one validator that turns lane receipts into a verdict. Pure: no I/O, no clock.
//
// Fail-closed rules the validator enforces:
//  - a scenario passes only when EVERY lane it names has a `pass` step for it, the step has at
//    least one assertion and all of them hold, every required artifact kind is present with a
//    sha256 and a nonzero byte count, and every cross-lane assertion for it holds;
//  - a lane counts only when its candidate head is the run revision, its checkout is clean and
//    its machine id is known;
//  - a blocked lane (host unavailable, device or push permission absent) makes every scenario it
//    belongs to `blocked`: never `pass`, never dropped from the report;
//  - cleanup entries must be session-owned outcomes; anything else fails the lane.

export const RUN_SCHEMA = 'ferryx-supervisor-f3.run/1';
export const LANE_SCHEMA = 'ferryx-supervisor-f3.lane/1';
export const EXIT = Object.freeze({ PASS: 0, FAIL: 1, BLOCKED: 2, USAGE: 5, INTERRUPTED: 130 });
export const LANES = Object.freeze(['macos', 'windows', 'iphone']);
const CLEANUP_ACTIONS = new Set(['killed', 'already-exited', 'removed', 'session-deleted', 'reported-not-killed']);

export const SCENARIOS = Object.freeze([
  { id: 'F3-D1', clause: 'macOS and Windows desktop show the same decision card', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'panel-read', 'gui-receipt'] },
  { id: 'F3-D2', clause: 'macOS and Windows desktop show the same prompt and Reply', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'transcript-read'] },
  { id: 'F3-P1', clause: 'iPhone Safari approves a decision with one tap, the card inside the gesture area', lanes: ['iphone'],
    artifacts: ['screenshot', 'accessibility-source', 'panel-read', 'fixture-log'] },
  { id: 'F3-P2', clause: 'iPhone Safari rejects a decision with one tap, the card inside the gesture area', lanes: ['iphone'],
    artifacts: ['screenshot', 'accessibility-source', 'panel-read', 'fixture-log'] },
  { id: 'F3-P3', clause: 'iPhone Safari stops a session with one tap while a decision card is visible', lanes: ['iphone'],
    artifacts: ['screenshot', 'accessibility-source', 'panel-read', 'fixture-log'] },
  { id: 'F3-P4', clause: 'a decision push arrives while the user is on another card and opens the new card', lanes: ['iphone'],
    artifacts: ['screenshot', 'accessibility-source', 'push-subscription', 'panel-read'] },
  { id: 'F3-D3', clause: 'transcript export downloads a file holding the prompt and the Reply', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'export-file', 'transcript-read'] },
  { id: 'F3-D4', clause: 'the long report opens in Viewer', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'transcript-read'] },
]);

export const scenarioById = (id) => SCENARIOS.find((scenario) => scenario.id === id) ?? null;
export const scenariosForLane = (lane) => SCENARIOS.filter((scenario) => scenario.lanes.includes(lane)).map((scenario) => scenario.id);

const isSha = (value) => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
const isRevision = (value) => typeof value === 'string' && /^[0-9a-f]{40}$/.test(value);

/** Why a lane receipt cannot vouch for anything, or null when it can. */
export function laneDefect(lane, revision) {
  if (!lane || lane.schema !== LANE_SCHEMA) return 'lane receipt missing or wrong schema';
  if (lane.blocked) return null;
  if (lane.candidate?.headRevision !== revision) return `candidate head ${lane.candidate?.headRevision} is not run revision ${revision}`;
  if (lane.candidate?.clean !== true) return 'candidate checkout is not clean';
  if (typeof lane.machine?.id !== 'string' || lane.machine.id.length === 0) return 'machine id unknown';
  const bad = (lane.cleanup ?? []).find((entry) => !CLEANUP_ACTIONS.has(entry.action));
  if (bad) return `cleanup entry ${JSON.stringify(bad)} is not a session-owned outcome`;
  return null;
}

function stepDefect(step, scenario) {
  if (!step) return 'no step recorded';
  if (step.status !== 'pass') return `step ${step.status}${step.code ? ` (${step.code})` : ''}`;
  const assertions = step.assertions ?? [];
  if (assertions.length === 0) return 'no assertions recorded';
  const failed = assertions.find((assertion) => assertion.ok !== true);
  if (failed) return `assertion ${failed.id} did not hold`;
  for (const kind of scenario.artifacts) {
    const ok = (step.artifacts ?? []).some((artifact) => artifact.kind === kind && isSha(artifact.sha256) && artifact.bytes > 0);
    if (!ok) return `required ${kind} artifact missing`;
  }
  if (!(step.durationMs >= 0) || !step.startedAt || !step.endedAt) return 'step timing missing';
  return null;
}

/** The verdict of one run: per scenario, then overall. */
export function validateRun(run) {
  const errors = [];
  if (run?.schema !== RUN_SCHEMA) errors.push('run receipt missing or wrong schema');
  if (!isRevision(run?.revision)) errors.push('run revision is not a 40-hex commit');
  const scenarios = SCENARIOS.map((scenario) => {
    const reasons = [];
    let blocked = false;
    let failed = false;
    for (const name of scenario.lanes) {
      const lane = run?.lanes?.[name];
      if (lane?.blocked) { blocked = true; reasons.push(`${name} blocked: ${lane.blocked.code}`); continue; }
      const defect = laneDefect(lane, run?.revision);
      if (defect) { failed = true; reasons.push(`${name}: ${defect}`); continue; }
      const step = (lane.steps ?? []).find((entry) => entry.scenarioId === scenario.id);
      const problem = stepDefect(step, scenario);
      if (!problem) continue;
      reasons.push(`${name}: ${problem}`);
      if (step?.status === 'blocked') blocked = true; else failed = true;
    }
    // A cross-lane check that could not hold only because a lane was blocked is not a failure;
    // every other unmet cross-lane check is.
    const cross = (run?.cross ?? []).filter((entry) => entry.scenarioId === scenario.id);
    const failedCross = cross.find((entry) => entry.ok !== true);
    if (failedCross) { reasons.push(`cross-lane assertion ${failedCross.id} did not hold`); if (!blocked) failed = true; }
    if (scenario.lanes.length > 1 && cross.length === 0 && reasons.length === 0) { reasons.push('no cross-lane assertion recorded'); failed = true; }
    // A real failure on a lane that ran is never hidden behind another lane's block.
    const status = reasons.length === 0 ? 'pass' : failed ? 'fail' : 'blocked';
    return { id: scenario.id, clause: scenario.clause, status, reasons };
  });
  const verdict = errors.length > 0 ? 'fail'
    : scenarios.every((entry) => entry.status === 'pass') ? 'pass'
      : scenarios.some((entry) => entry.status === 'fail') ? 'fail' : 'blocked';
  return { verdict, exitCode: verdict === 'pass' ? EXIT.PASS : verdict === 'blocked' ? EXIT.BLOCKED : EXIT.FAIL, errors, scenarios };
}
