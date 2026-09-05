#!/usr/bin/env bun
import { writeFileSync, mkdirSync, realpathSync, lstatSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import { platform, release, arch } from 'node:os';
import { parseArgs } from 'node:util';
import { prepare, validateRoot, cleanup, validateEvidence, scenarios, gates } from './ferryx-scope-fixtures.mjs';
import { startPage } from './ferryx-scope-page.mjs';
import { probeApi } from './ferryx-scope-api.mjs';
import { pageCurl, collectChild, releaseAll, spawnChild } from './ferryx-scope-lifecycle.mjs';

export async function main(argv) {
  process.umask(0o077);
  const report = { schema: 1, status: 'blocked', invocation: [process.execPath, resolve(import.meta.filename), ...argv], versions: { bun: globalThis.Bun?.version ?? null, os: platform(), release: release(), arch: arch() }, commands: [], http: [], actions: [], screenshots: [], cleanup: [] };
  let owned;
  let page;
  let evidenceDir;
  try {
    const { values } = parseArgs({ args: argv, options: { scenario: { type: 'string' }, root: { type: 'string' }, 'evidence-dir': { type: 'string' }, prepare: { type: 'boolean' }, 'prepare-only': { type: 'boolean' }, cleanup: { type: 'boolean' }, 'self-test': { type: 'boolean' } } });
    const scenario = values.scenario ?? (values['self-test'] || values.cleanup ? 'foundation' : undefined);
    if (!scenarios.includes(scenario)) throw new Error('INVALID_SCENARIO');
    report.scenario = scenario;
    if (values.prepare && values['prepare-only']) throw new Error('DUPLICATE_PREPARE');
    const preparing = values.prepare || values['prepare-only'];
    if ([preparing, values.cleanup, values['self-test']].filter(Boolean).length > 1) throw new Error('CONFLICTING_MODES');
    if ((preparing || values['self-test']) && values.root) throw new Error('PREPARE_REQUIRES_NEW_ROOT');
    const fixture = preparing || values['self-test'] ? (owned = prepare()) : validateRoot(values.root);
    report.fixture = { root: fixture.root, id: fixture.id, repositories: fixture.repositories };
    if (values['evidence-dir']) {
      const base = realpathSync('docs/evidence/ferryx-scope');
      const requested = resolve(values['evidence-dir']);
      if (!requested.startsWith(`${base}/`) || requested === base) throw new Error('UNSAFE_EVIDENCE_DIR');
      // Walk each component before creation: never follow an existing symlink.
      let cursor = base;
      for (const part of requested.slice(base.length + 1).split('/')) {
        cursor = join(cursor, part);
        try { mkdirSync(cursor, { mode: 0o700 }); } catch (error) { if (error.code !== 'EEXIST') throw error; }
        if (lstatSync(cursor).isSymbolicLink() || realpathSync(cursor) !== cursor) throw new Error('UNSAFE_EVIDENCE_DIR');
      }
      evidenceDir = requested;
    }
    if (values.cleanup) {
      report.cleanup.push(cleanup(fixture.root));
      report.status = 'cleaned';
    } else if (preparing) {
      report.status = 'prepared';
      report.readiness = { event: 'fixture-files-ready', resourcesStarted: [] };
      report.cleanup.push({ event: 'cleanup', root: fixture.root, removed: false, retainedExplicitly: true, command: [process.execPath, resolve(import.meta.filename), '--cleanup', '--root', fixture.root] });
      owned = undefined;
    } else {
      page = await startPage();
      report.actions.push({ action: 'fixture.listen', host: '127.0.0.1', port: 0, actualUrl: page.url, frameUrl: page.frameUrl });
      const command = pageCurl(page.url);
      const child = spawnChild(command, { env: { PATH: process.env.PATH, HOME: join(fixture.root, 'home') } });
      child.stdin.end();
      const [stdout, stderr, exitCode] = await collectChild(child);
      report.commands.push({ argv: command, exitCode, stdout, stderr });
      const split = stdout.indexOf('\r\n\r\n');
      report.http.push({ url: page.url, headers: stdout.slice(0, split), body: stdout.slice(split + 4), bodySha256: createHash('sha256').update(stdout.slice(split + 4)).digest('hex'), surface: 'fixture-only' });
      if (exitCode !== 0 || !stdout.includes('data-testid="design-element"')) throw new Error('FIXTURE_HTTP_FAILED');
      report.browser = { default: 'Bun.WebView', available: typeof globalThis.Bun?.WebView === 'function', executed: false, reason: 'Private browser data-path isolation and product routes are not integrated', pushRequired: 'Playwright with real Chrome and physical phone', desktopRequired: 'computer-use bound to QA PID/window' };
      if (values['self-test']) {
        const argv = [process.execPath, new URL('../../ui/node_modules/vitest/vitest.mjs', import.meta.url).pathname, 'run', '--config', new URL('./ferryx-scope-vitest.config.mjs', import.meta.url).pathname, 'scripts/qa/ferryx-scope-boundary.test.mjs'];
        const tests = spawnChild(argv);
        tests.stdin.end();
        const [stdout, stderr, exitCode] = await collectChild(tests);
        report.commands.push({ argv, stdout, stderr, exitCode });
        if (exitCode !== 0) throw new Error('SELF_TEST_FAILED');
        report.status = 'self-test-passed';
        report.productVerified = false;
      } else {
        report.missingObservables = gates[scenario];
        await probeApi(fixture, report);
        validateEvidence(scenario, {});
      }
    }
  } catch (error) {
    report.status = 'blocked';
    report.error = error instanceof Error ? error.message : String(error);
  } finally {
    try {
      await releaseAll([
        async () => { if (page) { await page.close(); report.cleanup.unshift({ event: 'cleanup', loopbackServersClosed: 2, processesSignalled: [] }); } },
        async () => { if (owned) report.cleanup.push(cleanup(owned.root)); },
      ]);
    } catch (error) { report.status = 'blocked'; report.cleanupError = String(error); }
  }
  if (evidenceDir) {
    try { writeFileSync(join(evidenceDir, `run-${crypto.randomUUID()}.json`), JSON.stringify(report, null, 2), { mode: 0o600, flag: 'wx' }); }
    catch (error) { report.status = 'blocked'; report.evidenceError = String(error); }
  }
  console.log(JSON.stringify(report, null, 2));
  return report.status === 'blocked' ? 2 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === import.meta.filename) process.exitCode = await main(process.argv.slice(2));
