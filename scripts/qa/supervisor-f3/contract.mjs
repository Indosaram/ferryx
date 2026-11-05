// F3 real-surface acceptance contract (supervisor-chat-redesign plan F3, as restated by the
// owner): scenario ids, the clause each proves, the lanes, the raw artifact kinds and the named
// assertions each needs, and the validator that turns receipts into a verdict. Pure: no I/O,
// no clock. verify.mjs adds the on-disk checks (artifact bytes, hashes, manifest, scripts).
//
// Fail-closed rules the validator enforces:
//  - a scenario passes only when EVERY lane it names has a `pass` step for it, EVERY named
//    assertion for it was recorded and holds, every required artifact kind is present with a
//    sha256 and a nonzero byte count, and every named cross-lane assertion was recorded and holds;
//  - a lane counts only when its candidate head is the run revision, its checkout is clean, its
//    candidate build stamp is `<revision>+<patch hash>`, its machine id is known, and it did not
//    record a lane failure;
//  - a blocked lane (host unavailable, device or push permission absent) makes every scenario it
//    belongs to `blocked`, never `pass`; a lane that ran and failed is never hidden behind it;
//  - every cleanup entry of every lane, blocked or not, must prove the resource is gone; a
//    run-spawned pid reported-not-killed (or any other outcome) fails the run;
//  - the control host's checkout must be the run revision and clean, with script hashes recorded;
//  - the session host (`run.session`: relay, fixture daemon and panes) must be at the run
//    revision with a matching build stamp unless it was blocked, and its cleanup (daemon, relay,
//    profile, relay root, the run's serve endpoint) must prove every resource gone.

export const RUN_SCHEMA = 'ferryx-supervisor-f3.run/3';
export const LANE_SCHEMA = 'ferryx-supervisor-f3.lane/2';
export const EXIT = Object.freeze({ PASS: 0, FAIL: 1, BLOCKED: 2, USAGE: 5, INTERRUPTED: 130 });
export const LANES = Object.freeze(['macos', 'windows', 'iphone']);
const CLEANUP_ACTIONS = new Set(['killed', 'already-exited', 'removed', 'absent', 'session-deleted']);

const ANSWER = ['card-in-gesture-area', 'button-in-gesture-area', 'touch-target-44pt', 'one-tap-one-answer', 'answered-card-left'];
const PHONE_ARTIFACTS = ['screenshot', 'accessibility-source', 'panel-read', 'fixture-log'];

export const SCENARIOS = Object.freeze([
  { id: 'F3-D1', clause: 'macOS and Windows desktop show the same decision card', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'panel-read', 'gui-receipt'],
    assertions: ['card-is-the-fleet-decision', 'card-command-on-screen', 'recommended-option-on-screen'], cross: ['same-card-both-desktops'] },
  { id: 'F3-D2', clause: 'macOS and Windows desktop show the same prompt and Reply', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'transcript-read'],
    assertions: ['prompt-row-stored', 'reply-row-stored', 'prompt-on-screen', 'reply-on-screen'], cross: ['same-prompt-and-reply-both-desktops'] },
  { id: 'F3-P1', clause: 'iPhone Safari approves a decision with one tap, the card inside the gesture area', lanes: ['iphone'],
    artifacts: PHONE_ARTIFACTS, assertions: ANSWER, cross: [] },
  { id: 'F3-P2', clause: 'iPhone Safari rejects a decision with one tap, the card inside the gesture area', lanes: ['iphone'],
    artifacts: PHONE_ARTIFACTS, assertions: ANSWER, cross: [] },
  { id: 'F3-P3', clause: 'iPhone Safari stops a session with one tap while a decision card is visible', lanes: ['iphone'],
    artifacts: PHONE_ARTIFACTS, assertions: ['card-visible-while-stopping', 'stop-touch-target-44pt', 'one-tap-stopped-session'], cross: [] },
  { id: 'F3-P4', clause: 'a decision push arrives while the user is on another card and opens the new card', lanes: ['iphone'],
    artifacts: ['screenshot', 'accessibility-source', 'push-subscription', 'panel-read'],
    assertions: ['user-on-another-card', 'banner-absent-before-release', 'push-banner-shown', 'push-opened-new-card', 'opened-card-in-gesture-area'], cross: [] },
  { id: 'F3-D3', clause: 'transcript export downloads a file holding the prompt and the Reply', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'export-file', 'transcript-read'],
    assertions: ['export-holds-prompt', 'export-holds-reply'], cross: ['both-desktops-pass'] },
  { id: 'F3-D4', clause: 'the long report opens in Viewer', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'transcript-read'],
    assertions: ['report-not-open-before-viewer', 'viewer-shows-report-heading', 'viewer-marker-shown', 'viewer-shows-report-end'], cross: ['both-desktops-pass'] },
  { id: 'F3-D5', clause: 'a decision answered from the phone leaves the panel on both desktops', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'panel-read'],
    assertions: ['phone-answered-card', 'answered-card-gone-from-panel', 'answered-card-gone-from-screen'], cross: ['both-desktops-pass'] },
  { id: 'F3-D6', clause: 'clicking a session row on the desktop focuses that session\'s terminal', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'panel-read', 'fixture-log'],
    assertions: ['row-in-panel', 'keys-in-away-pane-before-click', 'row-click-focused-terminal', 'typed-token-on-screen'], cross: ['both-desktops-pass'] },
  { id: 'F3-D7', clause: 'telling the supervisor "retry every 5 min" changes the rules line', lanes: ['macos', 'windows'],
    artifacts: ['screenshot', 'ocr', 'panel-read', 'transcript-read'],
    assertions: ['rules-line-before', 'rule-stored-every-300s', 'rules-line-changed-on-screen'], cross: ['both-desktops-pass'] },
]);

export const scenarioById = (id) => SCENARIOS.find((scenario) => scenario.id === id) ?? null;
export const scenariosForLane = (lane) => SCENARIOS.filter((scenario) => scenario.lanes.includes(lane)).map((scenario) => scenario.id);

const isSha = (value) => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
const isRevision = (value) => typeof value === 'string' && /^[0-9a-f]{40}$/.test(value);

/** Why a lane receipt cannot vouch for anything, or null when it can. */
export function laneDefect(lane, revision) {
  if (!lane || lane.schema !== LANE_SCHEMA) return 'lane receipt missing or wrong schema';
  if (lane.blocked) return null;
  if (lane.failure) return `lane failed: ${lane.failure.code}`;
  if (lane.candidate?.headRevision !== revision) return `candidate head ${lane.candidate?.headRevision} is not run revision ${revision}`;
  if (lane.candidate?.clean !== true) return 'candidate checkout is not clean';
  if (typeof lane.candidate?.stamp !== 'string' || !lane.candidate.stamp.startsWith(`${revision}+`)) return `candidate build stamp ${lane.candidate?.stamp} is not ${revision}+<patch>`;
  if (typeof lane.machine?.id !== 'string' || lane.machine.id.length === 0) return 'machine id unknown';
  return null;
}

/** Cleanup entries that do not prove their resource is gone, for any lane state. */
export const cleanupLeaks = (lane) => (lane?.cleanup ?? []).filter((entry) => !CLEANUP_ACTIONS.has(entry.action));

function stepDefect(step, scenario) {
  if (!step) return 'no step recorded';
  if (step.status !== 'pass') return `step ${step.status}${step.code ? ` (${step.code})` : ''}`;
  const assertions = step.assertions ?? [];
  for (const id of scenario.assertions) {
    const found = assertions.filter((assertion) => assertion.id === id);
    if (found.length === 0) return `assertion ${id} not recorded`;
    if (found.some((assertion) => assertion.ok !== true)) return `assertion ${id} did not hold`;
  }
  const failed = assertions.find((assertion) => assertion.ok !== true);
  if (failed) return `assertion ${failed.id} did not hold`;
  for (const kind of scenario.artifacts) {
    const ok = (step.artifacts ?? []).some((artifact) => artifact.kind === kind && isSha(artifact.sha256) && artifact.bytes > 0);
    if (!ok) return `required ${kind} artifact missing`;
  }
  if (!(step.durationMs >= 0) || !step.startedAt || !step.endedAt) return 'step timing missing';
  return null;
}

function controlDefect(control, revision) {
  if (!control || typeof control.hostname !== 'string') return 'control host identity missing';
  if (control.head !== revision) return `control checkout ${control.head} is not run revision ${revision}`;
  if (control.clean !== true) return 'control checkout is not clean';
  if (!Array.isArray(control.scripts) || control.scripts.length === 0 || control.scripts.some((s) => !isSha(s.sha256))) return 'control script hashes missing';
  return null;
}

function scenarioVerdict(run, scenario) {
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
  // every other missing or unmet cross-lane check is.
  for (const id of scenario.cross) {
    const entries = (run?.cross ?? []).filter((entry) => entry.scenarioId === scenario.id && entry.id === id);
    if (entries.length === 0) { reasons.push(`cross-lane assertion ${id} not recorded`); if (!blocked) failed = true; continue; }
    if (entries.some((entry) => entry.ok !== true)) { reasons.push(`cross-lane assertion ${id} did not hold`); if (!blocked) failed = true; }
  }
  const status = reasons.length === 0 ? 'pass' : failed ? 'fail' : 'blocked';
  return { id: scenario.id, clause: scenario.clause, status, reasons };
}

/** The verdict of one run receipt plus any extra (on-disk) errors: per scenario, then overall. */
export function validateRun(run, extraErrors = []) {
  const errors = [...extraErrors];
  if (run?.schema !== RUN_SCHEMA) errors.push('run receipt missing or wrong schema');
  if (!isRevision(run?.revision)) errors.push('run revision is not a 40-hex commit');
  if (run?.harnessRevision !== undefined && !isRevision(run.harnessRevision)) errors.push('harness revision is not a 40-hex commit');
  const control = controlDefect(run?.control, run?.harnessRevision ?? run?.revision);
  if (control) errors.push(control);
  if (run?.error) errors.push(`runner error: ${run.error}`);
  if (run?.smoke === true) errors.push('smoke run: proves the transport only, never F3');
  for (const name of LANES) {
    for (const leak of cleanupLeaks(run?.lanes?.[name])) errors.push(`${name} cleanup leak: ${JSON.stringify(leak).slice(0, 400)}`);
  }
  const session = laneDefect(run?.session, run?.revision);
  if (session) errors.push(`session host: ${session}`);
  for (const leak of cleanupLeaks(run?.session)) errors.push(`session host cleanup leak: ${JSON.stringify(leak).slice(0, 400)}`);
  const scenarios = SCENARIOS.map((scenario) => scenarioVerdict(run, scenario));
  const verdict = errors.length > 0 ? 'fail'
    : scenarios.every((entry) => entry.status === 'pass') ? 'pass'
      : scenarios.some((entry) => entry.status === 'fail') ? 'fail' : 'blocked';
  return { verdict, exitCode: verdict === 'pass' ? EXIT.PASS : verdict === 'blocked' ? EXIT.BLOCKED : EXIT.FAIL, errors, scenarios };
}
