import { readFileSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { collectChild, identityHeader, spawnChild } from './ferryx-scope-lifecycle.mjs';

// Read-only probes: no live resource adoption, no mutating an active selection.
export async function probeApi(fixture, report) {
  const binding = fixture.integration;
  if (!binding) throw new Error('QA_API_BINDING_REQUIRED');
  const base = new URL(binding.base);
  if (base.protocol !== 'http:' || base.hostname !== '127.0.0.1' || !base.port || ['5173', '43821'].includes(base.port) || base.pathname !== '/' || base.search || base.hash || base.username || base.password) throw new Error('UNSAFE_API_BINDING');
  const routes = ['/api/v1/health', '/api/v1/sessions', '/api/v1/workspace/state', '/api/v1/agents', '/api/v1/hosts'];
  if (report.scenario === 'push') routes.push('/api/v1/push/vapid-public-key');
  let token;
  for (const route of routes) {
    const url = new URL(route, base).href;
    // Token goes through stdin, never argv. Disable curlrc and redirects/proxies.
    const argv = ['curl', '--disable', '--noproxy', '*', '--silent', '--show-error', '--max-time', '5', '--dump-header', '-', '--config', '-', url];
    const child = spawnChild(argv, { env: { PATH: process.env.PATH, HOME: join(fixture.root, 'home') } });
    if (token) child.stdin.write(`header = "Authorization: Bearer ${token}"\n`);
    child.stdin.end();
    const [raw, stderr, exitCode] = await collectChild(child);
    const safe = text => token ? text.replaceAll(token, '[REDACTED]') : text;
    report.commands.push({ argv, stdin: token ? '[PRIVATE AUTH HEADER]' : '', exitCode, stderr: safe(stderr) });
    const split = raw.indexOf('\r\n\r\n');
    const headers = raw.slice(0, split);
    const body = raw.slice(split + 4);
    const status = Number(/^HTTP\/\S+ (\d+)/.exec(headers)?.[1] ?? 0);
    // Bodies remain private to the fixture; public evidence contains only safe
    // protocol status, not session transcripts or personal paths from a router.
    report.http.push({ url, status, headers: safe(headers.replace(/^set-cookie:.*$/gim, 'set-cookie: [REDACTED]')), bodyBytes: Buffer.byteLength(body), body: '[PRIVATE RESPONSE OMITTED]', surface: 'explicit-qa-api' });
    if (exitCode || split < 0) throw new Error('QA_API_TRANSPORT_FAILED');
    if (route === '/api/v1/health') {
      if (status !== 200 || !identityHeader(headers, fixture.id)) throw new Error('QA_API_IDENTITY_MISMATCH');
      const path = join(fixture.root, 'token');
      const stat = lstatSync(path);
      if (!stat.isFile() || stat.isSymbolicLink() || (stat.mode & 0o077)) throw new Error('UNSAFE_TOKEN');
      token = readFileSync(path, 'utf8').trim();
      if (!/^[A-Za-z0-9._~-]{16,4096}$/.test(token)) throw new Error('INVALID_TOKEN');
    } else {
      report.actions.push({ action: 'api.get', route, status, passed: status === 200 });
    }
  }
  if (report.actions.some(action => action.action === 'api.get' && !action.passed)) throw new Error('QA_API_ROUTE_UNAVAILABLE');
}
