import { gitEnv } from '../unified-supervisor/git-env.mjs';
// Remote harness provenance is enforced at the run-receipt boundary, not on iPhone.
import { describe, expect, test } from 'bun:test';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { rpcProcess } from '../fixtures/native-configured-adapters.mjs';
import { runLanes } from './run.mjs';
import { recordIdentity } from './steps.mjs';
import { connectRemoteAgent } from './remote-agent.mjs';
import { LANES, LANE_SCHEMA, RUN_SCHEMA, SCENARIOS, validateRun, harnessDefect, remoteAgentDefect } from './contract.mjs';

const revision = 'a'.repeat(40);
const harnessRevision = 'b'.repeat(40);
const sha256 = 'c'.repeat(64);
const receipt = (headRevision = revision) => {
  const lane = (name) => ({
    schema: LANE_SCHEMA, machine: { id: name },
    candidate: { headRevision: revision, clean: true, stamp: `${revision}+e3b0` },
    ...(name === 'iphone' ? {} : { harness: { checkout: '/harness', headRevision, clean: true, status: '' }, remoteAgent: { checkout: '/harness', resolvedPath: '/harness/scripts/qa/fixtures/native-remote-agent.mjs', headRevision, clean: true } }),
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
  test('F6-1 real session writer preserves harness identity', async () => {
    const who = { machine: { id: 'session' }, candidate: { headRevision: revision }, harness: { checkout: '/harness', headRevision: revision, clean: true } };
    const artifacts = [];
    const session = { receipt: {}, write: (...args) => artifacts.push(args) };
    let started = false;
    const shared = { probe: async () => who, start: async () => { started = true; throw new Error('stop after identity'); } };
    await runLanes({ config: {}, run: {}, lanes: {}, session, shared });
    expect(started).toBe(true);
    expect(harnessDefect(session.receipt, revision)).toBeNull();
    expect(artifacts).toEqual([['identity', 'identity.json', who]]);
    delete session.receipt.harness;
    expect(harnessDefect(session.receipt, revision)).toBe(`harness head undefined is not harness revision ${revision}`);
    const desktop = { receipt: {}, write: () => {} };
    recordIdentity(desktop, who);
    expect(desktop.receipt).toEqual(who);
  });

  for (const [label, replacement] of [
    ['matching', {}],
    ['outside', { resolvedPath: '/elsewhere/agent.mjs' }],
    ['sibling prefix', { resolvedPath: '/harness-evil/agent.mjs' }],
    ['wrong head', { headRevision: harnessRevision }],
    ['dirty', { clean: false }],
    ['missing clean', { clean: undefined }],
    ['different checkout', { checkout: '/elsewhere' }],
  ]) {
    test(`F6-2 remote agent ${label} is checked before operations`, async () => {
      const lane = { receipt: { machine: { platform: 'linux' }, harness: { checkout: '/harness' } }, write: () => {} };
      const report = { checkout: '/harness', resolvedPath: '/harness/agent.mjs', headRevision: revision, clean: true, ...replacement };
      let connected = false;
      let closed = false;
      const methods = [];
      const agent = { call: async (method) => { methods.push(method); return report; }, close: () => { closed = true; } };
      const helper = { call: async (method, args) => {
        expect(method).toBe('resolve-agent');
        expect(args.checkouts).toEqual(['/harness']);
        return { checkout: report.checkout, resolvedPath: report.resolvedPath };
      } };
      const connect = (path) => { connected = true; expect(path).toBe(report.resolvedPath); return agent; };
      const connection = connectRemoteAgent({ helper, host: { remoteAgent: '/configured/agent.mjs' }, run: { revision }, lane, connect });
      if (label === 'matching') {
        expect(await connection).toBe(agent);
        expect(lane.receipt.remoteAgent).toEqual(report);
        expect(remoteAgentDefect(lane.receipt, revision)).toBeNull();
      } else {
        await expect(connection).rejects.toMatchObject({ code: 'REMOTE_AGENT_PROVENANCE' });
        if (connected) expect(closed).toBe(true);
      }
      expect(methods).toEqual(connected ? ['identity'] : []);
    });
  }

  test('F6-2 real helper resolves checkout paths and real agent reports its own git identity', async () => {
    const checkout = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
    const path = `${checkout}/scripts/qa/fixtures/native-remote-agent.mjs`;
    const signal = AbortSignal.timeout(10_000);
    const helper = rpcProcess(process.execPath, [`${checkout}/scripts/qa/supervisor-f3/host-helper.mjs`, '--rpc'], { signal });
    const agent = rpcProcess(process.execPath, [path, '--rpc'], { signal });
    try {
      expect(await helper.call('resolve-agent', { path, checkouts: [checkout] })).toEqual({ resolvedPath: path, checkout });
      await expect(helper.call('resolve-agent', { path, checkouts: [`${checkout}/scripts/qa/supervisor-f3`] })).rejects.toMatchObject({ code: 'REMOTE_AGENT_PROVENANCE' });
      const report = await agent.call('identity', {});
      const git = (...args) => execFileSync('git', ['-C', checkout, ...args], { encoding: 'utf8', env: gitEnv() }).trim();
      expect(report).toEqual({ resolvedPath: path, checkout, headRevision: git('rev-parse', 'HEAD'), clean: git('status', '--porcelain', '--untracked-files=all') === '', status: git('status', '--porcelain', '--untracked-files=all') });
    } finally { agent.close(); helper.close(); }
  });

  test('F6-2 validator rejects absent, outside, dirty and wrong-head agent receipts', () => {
    for (const name of ['macos', 'windows', 'session']) {
      for (const change of [undefined, { resolvedPath: '/outside/agent.mjs' }, { headRevision: harnessRevision }, { clean: false }]) {
        const run = receipt();
        const lane = name === 'session' ? run.session : run.lanes[name];
        lane.remoteAgent = change && { ...lane.remoteAgent, ...change };
        const verdict = validateRun(run);
        expect(verdict.verdict).toBe('fail');
        expect(verdict.errors.some((error) => error.includes('REMOTE_AGENT_PROVENANCE'))).toBe(true);
      }
    }
  });
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
