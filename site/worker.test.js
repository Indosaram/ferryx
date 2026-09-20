import { expect, test } from 'bun:test';
import worker from './worker.js';

test.each(['http://ferryx.dev', 'http://www.ferryx.dev', 'https://www.ferryx.dev'])(
  'redirects %s to canonical HTTPS preserving path and query', async (origin) => {
    const request = new Request(`${origin}/compare/conductor/?utm_source=test`);
    const response = await worker.fetch(request, {
      ASSETS: { fetch: async () => new Response('asset') },
    });
    expect(response.status).toBe(301);
    expect(response.headers.get('location')).toBe('https://ferryx.dev/compare/conductor/?utm_source=test');
  },
);

test('serves HTTPS apex assets without redirecting', async () => {
  const request = new Request('https://ferryx.dev/robots.txt');
  const response = await worker.fetch(request, {
    ASSETS: { fetch: async (received) => {
      expect(received).toBe(request);
      return new Response('robots', { status: 200 });
    } },
  });
  expect(await response.text()).toBe('robots');
});
