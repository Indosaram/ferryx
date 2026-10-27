// In-process unit checks for --self-test: pure guards whose failure modes a
// child-process run cannot reach deterministically (ps failure modes, Windows
// lineage, empty aggregates, redaction samples, symlinked production roots).
import { mkdirSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import {
  aggregateExit, assertNotProduction, EXIT, familyOf, finalExitCode, identityVerdict, interpretDarwinPs, PLATFORM, probeProcess, redact, redactValue,
  settledOk, stampEpochMs,
} from './harness.mjs';

function redactionChecks(check) {
  // [sample, secret core that must not survive]
  const samples = [
    [`token xoxb-${'1'.repeat(12)}-abcdef`, '111111111111'],
    ['xapp-1-A0123-4567890123', '4567890123'],
    ['refresh xoxe.xoxp-1-abcdefghijkl', 'abcdefghijkl'],
    ['rotate xoxe-1-mnopqrstuvwx', 'mnopqrstuvwx'],
    ['Authorization: Bearer abcdefghijklmnop', 'abcdefghijklmnop'],
    ['key sk-ant-abcdefghijklmnopqrstuv', 'abcdefghijklmnopqrstuv'],
    [`ghp_${'a'.repeat(36)}`, 'a'.repeat(36)],
    ['{"botToken":"plain-value-123"}', 'plain-value-123'],
    ['SLACK_BOT_TOKEN=plainvalue123', 'plainvalue123'],
    ['password=hunter2hunter2', 'hunter2hunter2'],
    ['secret=s3cr3tvalue', 's3cr3tvalue'],
    ['https://example.test/cb?token=querysecret1&next=1', 'querysecret1'],
    ['jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signaturepart123', 'signaturepart123'],
    ['https://hooks.slack.com/services/T000/B000/webhooksecret1', 'webhooksecret1'],
    [`AIza${'B'.repeat(35)}`, 'B'.repeat(35)],
    ['mhc login --token flagsecret123', 'flagsecret123'],
    ['mhc login --api-key=flagsecret456', 'flagsecret456'],
    ['token: yamlsecret789', 'yamlsecret789'],
    ['X-Api-Key: headersecret12', 'headersecret12'],
    ["password: 'quoted secret 1'", 'quoted secret 1'],
    ['API_KEY="abc123def"', 'abc123def'],
    ["token='abc12345'", 'abc12345'],
    ['mhc login --token "quoted flag secret"', 'quoted flag secret'],
  ];
  const results = samples.map(([sample, core]) => {
    const out = redact(sample);
    return { out, ok: out.includes('[REDACTED') && !out.includes(core) };
  });
  check('redaction-patterns-mask-secrets', results.every((result) => result.ok), results);
  const ordinary = ['plain status line: 4 workers idle, revision 12', 'the refresh-token expired soon after login'];
  const kept = ordinary.map((text) => ({ text, out: redact(text) }));
  check('redaction-keeps-ordinary-text', kept.every((entry) => entry.out === entry.text), kept);
  const nested = redactValue({ password: 'hunter2-hunter2', nested: { apiKey: 'abc', list: ['Bearer abcdefghijkl'] }, plain: 'kept' });
  check('redaction-of-structured-values',
    nested.password === '[REDACTED]' && nested.nested.apiKey === '[REDACTED]' && nested.nested.list[0].includes('[REDACTED') && nested.plain === 'kept', nested);
  let parsed = null;
  try {
    parsed = JSON.parse(redact(JSON.stringify({
      message: 'auth Bearer abcdefghijklmnop', t: `xoxb-${'2'.repeat(10)}`, s: 'MY_TOKEN=abc\\"def', y: 'token: value12345',
      q: 'API_KEY="quotedenv123"', f: 'run --token "flagquoted456"', e: 'TOKEN=', n: 'next',
    }, null, 2)));
  } catch { parsed = null; }
  const flat = JSON.stringify(parsed);
  check('redaction-keeps-json-valid', parsed !== null && !flat.includes('2222222222') && !flat.includes('value12345') && !flat.includes('quotedenv123')
    && !flat.includes('flagquoted456') && parsed.n === 'next', parsed);
}

export async function unitChecks(check, stateRoot) {
  redactionChecks(check);

  const aggregate = [
    ['empty', [], (r) => r.exitCode !== EXIT.PASS && r.status !== 'PASS'],
    ['pass+fail', ['PASS', 'FAIL'], (r) => r.exitCode === EXIT.FAIL],
    ['pass+not-implemented', ['PASS', 'NOT_IMPLEMENTED'], (r) => r.exitCode === EXIT.NOT_IMPLEMENTED],
    ['all-not-implemented', ['NOT_IMPLEMENTED', 'NOT_IMPLEMENTED'], (r) => r.exitCode === EXIT.NOT_IMPLEMENTED],
    ['pass+skipped', ['PASS', 'SKIPPED'], (r) => r.exitCode !== EXIT.PASS && r.status !== 'PASS'],
    ['all-skipped', ['SKIPPED'], (r) => r.exitCode !== EXIT.PASS && r.status !== 'PASS'],
    ['fail+interrupted', ['FAIL', 'INTERRUPTED'], (r) => r.exitCode === EXIT.INTERRUPTED],
    ['pass+cleanup-failed', ['PASS', 'CLEANUP_FAILED'], (r) => r.exitCode === EXIT.CLEANUP],
    ['all-pass', ['PASS', 'PASS'], (r) => r.exitCode === EXIT.PASS && r.status === 'PASS'],
  ].map(([label, statuses, expect]) => {
    const result = aggregateExit(statuses.map((status) => ({ status })));
    return { label, result, ok: expect(result) };
  });
  check('aggregate-exit-gating', aggregate.every((entry) => entry.ok), aggregate);

  const fault = [{ kind: 'unhandledRejection' }];
  const faults = [
    ['pass+fault', finalExitCode(EXIT.PASS, fault), EXIT.HARNESS],
    ['not-implemented+fault', finalExitCode(EXIT.NOT_IMPLEMENTED, fault), EXIT.HARNESS],
    ['cleanup+fault', finalExitCode(EXIT.CLEANUP, fault), EXIT.HARNESS],
    ['interrupted+fault', finalExitCode(EXIT.INTERRUPTED, fault), EXIT.INTERRUPTED],
    ['pass, no fault', finalExitCode(EXIT.PASS, []), EXIT.PASS],
  ].map(([label, got, expected]) => ({ label, got, expected, ok: got === expected }));
  check('fault-outside-a-scenario-forces-harness-error', faults.every((entry) => entry.ok), faults);

  const alive = (startStamp, argv) => ({ state: 'alive', startStamp, argv });
  const launch = { executable: '/opt/bin/bun', args: ['/qa/sleeper.mjs'], startStamp: 'linux:100' };
  const script = { executable: '/qa/worker.sh', args: [], startStamp: 'linux:200' };
  const verdicts = [
    ['same process, same argv', identityVerdict(launch, alive('linux:100', ['/opt/bin/bun', '/qa/sleeper.mjs'])).verdict, 'match'],
    ['equal stamp, argv changed', identityVerdict(launch, alive('linux:100', ['/bin/sh', '-c', 'other'])).verdict, 'argv-changed'],
    ['unequal stamp', identityVerdict(launch, alive('linux:101', ['/opt/bin/bun', '/qa/sleeper.mjs'])).verdict, 'mismatch'],
    ['no stamp, argv changed', identityVerdict({ ...launch, startStamp: null }, alive('linux:100', ['/bin/sh'])).verdict, 'argv-changed'],
    ['#! script without observed argv', identityVerdict(script, alive('linux:200', ['/bin/sh', '/qa/worker.sh'])).verdict, 'argv-changed'],
    ['#! script with observed argv', identityVerdict({ ...script, observedArgv: ['/bin/sh', '/qa/worker.sh'] }, alive('linux:200', ['/bin/sh', '/qa/worker.sh'])).verdict, 'match'],
  ].map(([label, got, expected]) => ({ label, got, expected, ok: got === expected }));
  check('identity-verdict-argv-change-is-never-mismatch', verdicts.every((entry) => entry.ok), verdicts);

  const lstart = 'Mon Oct 27 11:09:00 2026';
  const ps = [
    ['signalled ps, pid alive', { code: null, signal: 'SIGINT', stdout: '' }, true, 'unknown'],
    ['code 1 empty, pid alive', { code: 1, signal: null, stdout: '' }, true, 'unknown'],
    ['timed out, pid alive', { code: null, signal: 'SIGKILL', stdout: '', timedOut: true }, true, 'unknown'],
    ['spawn error, pid alive', { code: null, signal: null, stdout: '', error: 'EAGAIN' }, true, 'unknown'],
    ['existence unknown', { code: 1, signal: null, stdout: '' }, null, 'unknown'],
    ['pid gone', { code: 1, signal: null, stdout: '' }, false, 'gone'],
    ['alive row', { code: 0, signal: null, stdout: `S    ${lstart}\n` }, true, 'alive'],
    ['zombie row', { code: 0, signal: null, stdout: `Z    ${lstart}\n` }, true, 'zombie'],
  ].map(([label, result, exists, expected]) => {
    const got = interpretDarwinPs(result, exists).state;
    return { label, expected, got, ok: got === expected };
  });
  check('darwin-ps-failure-is-never-gone', ps.every((entry) => entry.ok), ps);

  const complete = { exited: true, descendantsObservable: true, postExitObservable: true, descendants: [{ finalState: 'gone' }] };
  const gating = [
    ['complete', complete, true],
    ['table unobservable before signalling', { ...complete, descendantsObservable: false }, false],
    ['table unobservable after exit', { ...complete, postExitObservable: false }, false],
    ['descendant unknown', { ...complete, descendants: [{ finalState: 'unknown' }] }, false],
    ['descendant alive', { ...complete, descendants: [{ finalState: 'alive' }] }, false],
    ['leader not exited', { ...complete, exited: false }, false],
  ].map(([label, receipt, expected]) => ({ label, expected, ok: settledOk(receipt) === expected }));
  check('termination-receipt-requires-observability', gating.every((entry) => entry.ok), gating);

  const at = (second) => `win32:2026-10-27T10:00:${String(second).padStart(2, '0')}.000Z`;
  const rows = [
    { pid: 100, ppid: 1, pgid: null, startStamp: at(10) },
    { pid: 200, ppid: 100, pgid: null, startStamp: at(11) },
    { pid: 300, ppid: 100, pgid: null, startStamp: at(5) },
    { pid: 400, ppid: 200, pgid: null, startStamp: at(12) },
    { pid: 500, ppid: 300, pgid: null, startStamp: at(13) },
  ];
  const family = familyOf(rows, 100, { platform: 'win32' }).map((row) => row.pid).sort((a, b) => a - b);
  check('win32-lineage-requires-creation-order', JSON.stringify(family) === JSON.stringify([200, 400]), { family });

  const fakeHome = join(stateRoot, 'fake-home');
  const target = join(stateRoot, 'symlink-target');
  mkdirSync(fakeHome, { recursive: true });
  mkdirSync(target, { recursive: true });
  let refused = null;
  try {
    symlinkSync(target, join(fakeHome, '.ferryx'), 'dir');
    assertNotProduction(join(target, 'qa'), 'dataRoot', { home: fakeHome, env: {} });
  } catch (error) { refused = error?.code ?? String(error?.message ?? error); }
  check('symlinked-production-root-refused', refused === 'PRODUCTION_PATH_REFUSED', { refused });

  const xdg = join(stateRoot, 'xdg-data');
  const envRoot = join(stateRoot, 'session-dir');
  const extraRoots = [
    ['xdg-data-home', join(xdg, 'ferryx', 'qa'), { XDG_DATA_HOME: xdg }],
    ['ferryx-session-dir', join(envRoot, 'qa'), { FERRYX_SESSION_DIR: envRoot }],
  ].map(([label, path, env]) => {
    let code = null;
    try { assertNotProduction(path, label, { home: fakeHome, env }); } catch (error) { code = error?.code ?? null; }
    return { label, code, ok: code === 'PRODUCTION_PATH_REFUSED' };
  });
  check('xdg-and-ferryx-env-roots-refused', extraRoots.every((entry) => entry.ok), extraRoots);

  const selfStamp = (await probeProcess(process.pid)).startStamp ?? null;
  const epoch = stampEpochMs(selfStamp);
  const age = epoch === null ? null : Date.now() - epoch;
  check('own-start-stamp-epoch-plausible', selfStamp !== null && (PLATFORM === 'win32' || (age !== null && age > -5_000 && age < 3_600_000)),
    { selfStamp, age, platform: PLATFORM });
}
