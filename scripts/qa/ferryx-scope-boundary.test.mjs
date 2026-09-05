import { test, expect } from '../../ui/node_modules/vitest/dist/index.js';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { prepare, cleanup, validateRoot, validateEvidence } from './ferryx-scope-fixtures.mjs';
import { probeApi } from './ferryx-scope-api.mjs';
import { createServer } from 'node:http';
import { listen, closeServer } from './ferryx-scope-lifecycle.mjs';

async function serveFixture(fetch) {
  const server = createServer(async (request, response) => {
    const result = fetch(new Request(`http://127.0.0.1${request.url}`, { headers: request.headers }));
    response.writeHead(result.status, Object.fromEntries(result.headers));
    response.end(await result.text());
  });
  await listen(server);
  return { url: new URL(`http://127.0.0.1:${server.address().port}`), stop: () => closeServer(server) };
}

test('unsafe roots and authored pass evidence cannot certify a scenario', () => {
  expect(() => validateRoot('/Users/indo')).toThrow();
  expect(() => validateEvidence('foundation', { status: 'passed' })).toThrow();
  expect(() => validateEvidence('push', { device: { physical: true, backgroundReceipt: 'invented' } })).toThrow();
});

test('wire-level missing routes fail after authenticated real requests', async () => {
  const fixture = prepare();
  const requests = [];
  const token = 'fixture-token-123456789';
  writeFileSync(join(fixture.root, 'token'), token, { mode: 0o600 });
  const server = await serveFixture(request => {
    const route = new URL(request.url).pathname;
    requests.push({ route, auth: request.headers.get('authorization') });
    return new Response('{}', { status: route === '/api/v1/agents' ? 404 : 200, headers: { 'X-Ferryx-QA-Fixture': fixture.id } });
  });
  const report = { scenario: 'mobile', commands: [], http: [], actions: [] };
  try {
    await expect(probeApi({ ...fixture, integration: { base: server.url.href } }, report)).rejects.toThrow('QA_API_ROUTE_UNAVAILABLE');
    expect(requests.length).toBe(5);
    expect(requests[0].auth).toBeNull();
    expect(requests[1].auth).toBe(`Bearer ${token}`);
    expect(report.http.find(item => item.url.endsWith('/agents')).status).toBe(404);
    expect(JSON.stringify(report)).not.toContain(token);
  } finally { await server.stop(true); cleanup(fixture.root); }
});

test('identity mismatch stops before token access or authenticated routes', async () => {
  const fixture = prepare();
  let requests = 0;
  const server = await serveFixture(() => { requests++; return new Response('{}'); });
  try {
    await expect(probeApi({ ...fixture, integration: { base: server.url.href } }, { commands: [], http: [], actions: [] })).rejects.toThrow('QA_API_IDENTITY_MISMATCH');
    expect(requests).toBe(1);
  } finally { await server.stop(true); cleanup(fixture.root); }
});
