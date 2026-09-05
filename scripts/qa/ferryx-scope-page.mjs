import { createServer } from 'node:http';
import { listen, closeServer, releaseAll } from './ferryx-scope-lifecycle.mjs';

// Deterministic paint fixture, NOT a substitute for product-native capture.
export async function startPage() {
  const frame = createServer((_req, res) => res.end('<!doctype html><title>QA cross origin</title><p id="frame-marker">FERRYX_FRAME</p>'));
  let server;
  try {
  await listen(frame);
  const frameAddress = frame.address();
  if (!frameAddress || typeof frameAddress === 'string') throw new Error('FIXTURE_BIND_FAILED');
  const html = `<!doctype html><meta charset="utf-8"><title>Ferryx isolated pixel fixture</title>
<style>body{margin:0;padding:16px;min-height:1800px} [data-testid=design-element]{width:120px;height:64px;background:rgb(25,120,210)}</style>
<button data-testid="design-element" onclick="document.querySelector('[data-testid=design-click-count]').textContent=++window.clicks">FERRYX_BLUE</button>
<output data-testid="design-click-count">0</output>
<canvas data-testid="design-canvas" width="120" height="64"></canvas>
<canvas data-testid="design-webgl" width="120" height="64"></canvas>
<img data-testid="design-image" width="120" height="64" alt="Green marker" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='120' height='64'%3E%3Cpath fill='%2300aa55' d='M0 0h120v64H0z'/%3E%3C/svg%3E">
<iframe data-testid="design-iframe" src="http://127.0.0.1:${frameAddress.port}"></iframe>
<script>window.clicks=0;const c=document.querySelector('[data-testid=design-canvas]').getContext('2d');c.fillStyle='#ef6633';c.fillRect(0,0,120,64);const g=document.querySelector('[data-testid=design-webgl]').getContext('webgl');if(g){g.clearColor(.5,0,.75,1);g.clear(g.COLOR_BUFFER_BIT)}document.documentElement.dataset.ready='true';</script>`;
  server = createServer((_req, res) => { res.setHeader('Content-Type', 'text/html; charset=utf-8'); res.setHeader('Cache-Control', 'no-store'); res.end(html); });
    await listen(server);
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('FIXTURE_BIND_FAILED');
    return { url: `http://127.0.0.1:${address.port}`, frameUrl: `http://127.0.0.1:${frameAddress.port}`, async close() {
      await releaseAll([() => closeServer(server), () => closeServer(frame)]);
    } };
  } catch (error) {
    try { await releaseAll([() => closeServer(frame), () => server && closeServer(server)]); }
    catch (cleanupError) { throw new AggregateError([error, cleanupError], 'QA_START_AND_CLEANUP_FAILED'); }
    throw error;
  }
}
