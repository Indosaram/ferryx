// Remote harness provenance is enforced at the run-receipt boundary, not on iPhone.
import { describe, expect, test } from 'bun:test';
import { LANES, LANE_SCHEMA, RUN_SCHEMA, SCENARIOS, validateRun } from './contract.mjs';

const revision = 'a'.repeat(40);
const harnessRevision = 'b'.repeat(40);
const sha256 = 'c'.repeat(64);
const receipt = (headRevision = revision) => {
  const lane = (name) => ({
    schema: LANE_SCHEMA, machine: { id: name },
    candidate: { headRevision: revision, clean: true, stamp: `${revision}+e3b0` },
    ...(name === 'iphone' ? {} : { harness: { headRevision, clean: true, status: '' } }),
    cleanup: [],
    steps: SCENARIOS.filter((scenario) => scenario.lanes.includes(name)).map((scenario) => ({
      scenarioId: scenario.id, status: 'pass', durationMs: 0,
      startedAt: '2026-11-05T00:00:00Z', endedAt: '2026-11-05T00:00:00Z',
      assertions: scenario.assertions.map((id) => ({ id, ok: true })),
      artifacts: scenario.artifacts.map((kind) => ({ kind, sha256, bytes: 1 })),
    })),
  });
  return {
    schema: RUN_SCHEMA, revision,
    control: { hostname: 'control', head: headRevision, clean: true, scripts: [{ sha256 }] },
    session: lane('session'), lanes: Object.fromEntries(LANES.map((name) => [name, lane(name)])),
    cross: SCENARIOS.flatMap((scenario) => scenario.cross.map((id) => ({ scenarioId: scenario.id, id, ok: true }))),
  };
};

describe('remote harness provenance', () => {
  test('the run revision is the default harness pin and iPhone needs no harness', () => {
    expect(validateRun(receipt()).verdict).toBe('pass');
  });

  test('a separate harness revision pins remote harnesses without changing candidate provenance', () => {
    const run = { ...receipt(harnessRevision), harnessRevision };
    expect(validateRun(run).verdict).toBe('pass');
  });

  for (const [evidence, replacement] of [
    ['head/tail and Viewer marker only', []],
    ['report row label instead of Viewer middle', [{ id: 'report-row-label-shown', ok: true }]],
  ]) {
    test(`macos D4 rejects ${evidence}`, () => {
      const run = receipt();
      const step = run.lanes.macos.steps.find((entry) => entry.scenarioId === 'F3-D4');
      step.assertions = step.assertions.filter((assertion) => assertion.id !== 'viewer-shows-report-middle').concat(replacement);
      const verdict = validateRun(run);
      const scenario = verdict.scenarios.find((entry) => entry.id === 'F3-D4');
      expect(verdict.verdict).toBe('fail');
      expect(verdict.errors).toEqual([]);
      expect(scenario.status).toBe('fail');
      expect(scenario.reasons).toHaveLength(1);
      expect(scenario.reasons[0]).toContain('macos');
      expect(scenario.reasons[0]).toContain('viewer-shows-report-middle');
    });
  }

  for (const name of ['macos', 'windows', 'session']) {
    for (const [defect, harness] of [
      ['missing', undefined],
      ['wrong revision', { headRevision: harnessRevision, clean: true }],
      ['dirty', { headRevision: revision, clean: false, status: '?? scripts/qa/supervisor-f3/untracked.mjs' }],
      ['missing cleanliness', { headRevision: revision }],
    ]) {
      test(`${name} rejects a ${defect} harness even when all scenario evidence passes`, () => {
        const run = receipt();
        const lane = name === 'session' ? run.session : run.lanes[name];
        lane.harness = harness;
        const verdict = validateRun(run);
        expect(verdict.verdict).toBe('fail');
        expect(verdict.errors.some((error) => error.startsWith(`${name === 'session' ? 'session host' : name}: harness`))).toBe(true);
      });
    }

    test(`${name} cannot use the candidate revision instead of an explicit harness pin`, () => {
      const run = { ...receipt(harnessRevision), harnessRevision };
      const lane = name === 'session' ? run.session : run.lanes[name];
      lane.harness.headRevision = revision;
      expect(validateRun(run).verdict).toBe('fail');
    });
  }

  test('unavailable hosts without harness provenance remain blocked', () => {
    const run = receipt();
    const blocked = { schema: LANE_SCHEMA, blocked: { code: 'HOST_UNAVAILABLE' }, cleanup: [] };
    run.session = structuredClone(blocked);
    for (const name of LANES) run.lanes[name] = structuredClone(blocked);
    run.cross = [];
    const verdict = validateRun(run);
    expect(verdict.errors).toEqual([]);
    expect(verdict.verdict).toBe('blocked');
    expect(verdict.scenarios.every((scenario) => scenario.status === 'blocked')).toBe(true);
  });
});
