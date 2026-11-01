// No-GUI checks of the browser-agent harness: config/engine validation, isolated env, CLI
// runner and error-code parsing. Run with `bun test scripts/qa/browser-agent/run.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { cliErrorCode, evidenceLabel, isolatedEnv, loadConfig, runCli, UsageError } from './run.mjs';

const root = mkdtempSync(path.join(tmpdir(), 'ferryx-browser-agent-qa-'));

function config(extra = {}) {
  return {
    os: 'macos',
    binaries: { gui: ['/opt/ferryx/Ferryx'], cli: ['/opt/ferryx/ferryx'] },
    isolation: { runtimeRoot: path.join(root, 'run'), dataRoot: path.join(root, 'data'), profileRoot: path.join(root, 'profile') },
    mahord: { host: 'maho-mac', pairingId: '01234567-89ab-cdef-0123-456789abcdef' },
    remoteGateway: { url: 'http://127.0.0.1:1', bootstrapFile: path.join(root, 'bootstrap.json') },
    ...extra,
  };
}

function load(value) {
  const file = path.join(root, `config-${Math.random().toString(16).slice(2)}.json`);
  writeFileSync(file, JSON.stringify(value));
  return loadConfig(file);
}

test('the engine selects evidence dir and the Chromium engine dir', () => {
  const webview = load(config());
  assert.equal(evidenceLabel(webview), 'macos-webview');
  assert.equal(isolatedEnv(webview).FERRYX_CEF_ENGINE_DIR, undefined);
  assert.equal(isolatedEnv(webview).FERRYX_DATA_DIR, path.join(root, 'data'));

  const chromium = load(config({ engine: 'chromium', engineDir: '/opt/ferryx-engine' }));
  assert.equal(evidenceLabel(chromium), 'macos-chromium');
  assert.equal(isolatedEnv(chromium).FERRYX_CEF_ENGINE_DIR, '/opt/ferryx-engine');
});

test('an engine config without its dir, or with a stray dir or unknown engine, is refused', () => {
  assert.throws(() => load(config({ engine: 'chromium' })), UsageError);
  assert.throws(() => load(config({ engineDir: '/opt/ferryx-engine' })), UsageError);
  assert.throws(() => load(config({ engine: 'gecko' })), UsageError);
});

test('runCli reports exit code, output and parsed JSON; a hung child is killed at the bound', async () => {
  const spawnStub = (code, stdout, stderr, never = false) => () => ({
    exited: never ? new Promise(() => {}) : Promise.resolve(code),
    stdout: new Response(stdout).body,
    stderr: new Response(stderr).body,
    kill() {},
  });
  const ok = await runCli('/bin/ferryx', ['list'], { spawn: spawnStub(0, '{"type":"acted"}\n', '') });
  assert.deepEqual([ok.code, ok.json], [0, { type: 'acted' }]);
  const stderr = JSON.stringify({ type: 'error', code: 'BROWSER_CLI_INVALID_OR_UNAVAILABLE', message: 'BROWSER_FORBIDDEN: no grant for this tab' });
  const refused = await runCli('/bin/ferryx', ['list'], { spawn: spawnStub(2, '', `waiting...\n${stderr}\n`) });
  assert.equal(refused.code, 2);
  assert.equal(cliErrorCode(refused), 'BROWSER_FORBIDDEN');
  const local = JSON.stringify({ type: 'error', code: 'BROWSER_CLI_INVALID_OR_UNAVAILABLE', message: 'missing required option --ref' });
  assert.equal(cliErrorCode({ stderr: local }), 'BROWSER_CLI_INVALID_OR_UNAVAILABLE');
  assert.equal(cliErrorCode({ stderr: 'not json' }), null);
  const hung = await runCli('/bin/ferryx', ['list'], { spawn: spawnStub(0, '', '', true), timeoutMs: 20 });
  assert.equal(hung.code, null);
});
