#!/usr/bin/env node
// Job 1 (task-9 root cause 1, established by the decisive control in
// `.omo/evidence/local-pane-liveness-completion-replan/task-9/ACCESSIBILITY-EXPERIMENT.md`):
//
// `src-tauri/tauri.conf.json` declares `devUrl: http://127.0.0.1:5173` and
// `frontendDist: ../ui/dist`. A DEBUG binary boots against `devUrl`; the runner
// never started a frontend, so with nothing listening the webview rendered
// Chromium's ERR_CONNECTION_REFUSED page - exactly the 29-node UIA tree pass 7
// measured and mistook for an accessibility defect.
//
// This module serves the frontend the debug binary expects, BEFORE the app
// boots, so the real UI (and therefore the real pane toolbar) exists.
//
// Route decision - static `ui/dist` (option B), not `bun scripts/dev-frontend.mjs`
// (option A):
//   * A is a dev server whose first action is a full UI build and whose Vite
//     process watches the tree and re-transforms on change (HMR). A QA fixture
//     that rebuilds itself mid-run is not deterministic: the bytes the app loads
//     can change between the pane step and the split step.
//   * B serves the already-built assets, immutably, from this process. The
//     verifier proved the route sufficient (`LISTENING pid=22056 HTTP=200
//     bytes=3536 HAS_ROOT_DIV=True`) and the same probe is this module's
//     readiness gate. The static server is ~15 lines, and its assets are the
//     exact bytes the release binary embeds (`frontendDist`), so a QA run and a
//     release run exercise the same UI.
//   * A missing `ui/dist` is therefore a typed, fail-closed block
//     (`FRONTEND_DIST_MISSING`) rather than a silent rebuild - the runner never
//     builds anything.
//
// Fail-closed rules this module implements, as required:
//   * port 5173 already occupied by a FOREIGN listener => typed
//     `FRONTEND_PORT_OCCUPIED`; the foreign listener is never killed, never
//     signalled, never reused, and the port is never assumed free;
//   * a dist that is absent => `FRONTEND_DIST_MISSING`;
//   * a server that does not answer with the app's own root document =>
//     `FRONTEND_NOT_SERVED`;
//   * a binary whose declared `devUrl` is not the URL served =>
//     `FRONTEND_DEVURL_MISMATCH` (a silent drift would put the run back on the
//     error page this module exists to remove);
//   * teardown rides the runner's existing cleanup path: the server is an
//     in-process listener registered on the ResourceRegistry, closed by
//     `ResourceRegistry.cleanup()` with a receipt, and it dies with the runner
//     process regardless.

import { createServer, request as httpRequest } from 'node:http';
import { createConnection } from 'node:net';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { extname, join, resolve, sep } from 'node:path';
import { BUDGETS, HarnessError } from './common-harness.mjs';

// The frozen `devUrl` of the debug binary (`src-tauri/tauri.conf.json`). Loopback
// only: the app and this server are on the same machine (the Windows lane
// launches the app in the interactive session via a scheduled task, and TCP
// loopback is reachable across Windows sessions).
export const FRONTEND_HOST = '127.0.0.1';
export const FRONTEND_PORT = 5173;
export const FRONTEND_DEV_URL = `http://${FRONTEND_HOST}:${FRONTEND_PORT}`;
export const FRONTEND_DIST_RELATIVE = join('ui', 'dist');
export const FRONTEND_INDEX_FILE = 'index.html';
export const TAURI_CONF_RELATIVE = join('src-tauri', 'tauri.conf.json');
// The app's own root document marker: the verifier's decisive control measured
// `HTTP=200 bytes=3536 HAS_ROOT_DIV=True` against this exact served build, so a
// 200 alone is not readiness - the body must be the app's index.
export const FRONTEND_ROOT_MARKER = 'id="root"';

const CONTENT_TYPES = Object.freeze({
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.wasm': 'application/wasm',
  '.map': 'application/json; charset=utf-8',
});

export function contentTypeFor(path) {
  return CONTENT_TYPES[extname(path).toLowerCase()] ?? 'application/octet-stream';
}

// The built assets the debug binary's `devUrl` must serve. Absent (or not a real
// index document) is a typed block: the runner never builds a frontend.
export function resolveFrontendDist(rootDir, distDir = null) {
  const root = distDir ? resolve(distDir) : join(resolve(rootDir), FRONTEND_DIST_RELATIVE);
  const indexPath = join(root, FRONTEND_INDEX_FILE);
  if (!existsSync(indexPath)) {
    throw new HarnessError('FRONTEND_DIST_MISSING', `the debug binary's devUrl (${FRONTEND_DEV_URL}) needs a frontend, but ${indexPath} does not exist; build ui/dist (or stage the built assets) before a native QA run - this runner never builds one`);
  }
  if (!statSync(indexPath).isFile()) {
    throw new HarnessError('FRONTEND_DIST_MISSING', `${indexPath} exists but is not a regular file`);
  }
  return { distDir: root, indexPath };
}

// The `devUrl` the binary in this tree will really use, read from the config
// rather than assumed. `null` when the config is absent (nothing to compare).
export function readTauriDevUrl(rootDir) {
  const confPath = join(resolve(rootDir), TAURI_CONF_RELATIVE);
  if (!existsSync(confPath)) return null;
  let conf;
  try { conf = JSON.parse(readFileSync(confPath, 'utf8')); } catch (error) {
    throw new HarnessError('FRONTEND_DEVURL_MISMATCH', `${confPath} is not readable JSON: ${error.message}`);
  }
  const devUrl = conf?.build?.devUrl;
  return typeof devUrl === 'string' && devUrl.length > 0 ? devUrl : null;
}

export function parseDevUrl(devUrl) {
  try {
    const parsed = new URL(devUrl);
    const port = parsed.port ? Number(parsed.port) : (parsed.protocol === 'https:' ? 443 : 80);
    return { host: parsed.hostname, port };
  } catch {
    return null;
  }
}

// A silent drift here puts the run straight back on the error page this module
// removes (the app would load a URL nobody serves, or this server would serve a
// port the app never asks for). Both are typed, fail-closed blocks.
export function assertFrontendDevUrlMatches(rootDir, { host = FRONTEND_HOST, port = FRONTEND_PORT } = {}) {
  const devUrl = readTauriDevUrl(rootDir);
  if (devUrl === null) return { devUrl: null, checked: false };
  const parsed = parseDevUrl(devUrl);
  if (parsed === null) {
    throw new HarnessError('FRONTEND_DEVURL_MISMATCH', `tauri.conf.json declares an unparsable devUrl ${JSON.stringify(devUrl)}`);
  }
  if (parsed.port !== port || (parsed.host !== host && parsed.host !== 'localhost')) {
    throw new HarnessError('FRONTEND_DEVURL_MISMATCH', `tauri.conf.json declares devUrl ${JSON.stringify(devUrl)} but this run serves ${host}:${port}`);
  }
  return { devUrl, checked: true };
}

// Is something ALREADY listening? Probed, never assumed: a port that is free is
// only known to be free after a connect attempt was refused. A listener found
// here is foreign by construction (this run has opened none yet) and is never
// killed, signalled, or reused.
export async function probePortOccupied({
  host = FRONTEND_HOST,
  port = FRONTEND_PORT,
  timeoutMs = BUDGETS.frontendPortProbeMs,
  connect = createConnection,
} = {}) {
  return new Promise(resolvePromise => {
    let settled = false;
    const finish = occupied => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { socket.destroy(); } catch { /* already gone */ }
      resolvePromise(occupied);
    };
    const timer = setTimeout(() => finish(false), timeoutMs);
    let socket;
    try {
      socket = connect({ host, port });
    } catch {
      clearTimeout(timer);
      return resolvePromise(false);
    }
    socket.once('connect', () => finish(true));
    socket.once('error', () => finish(false));
  });
}

// One bounded GET of the served index: 200 AND the app's own root document. A
// status/body that is not the app is a block, not a pass.
export async function fetchIndexBody({
  url,
  timeoutMs = BUDGETS.frontendRequestMs,
  request = defaultHttpRequest,
} = {}) {
  return request({ url, timeoutMs });
}

function defaultHttpRequest({ url, timeoutMs }) {
  return new Promise((resolvePromise, rejectPromise) => {
    let parsed;
    try { parsed = new URL(url); } catch (error) { return rejectPromise(error); }
    const req = httpRequest({
      protocol: parsed.protocol,
      host: parsed.hostname,
      port: parsed.port,
      path: `${parsed.pathname}${parsed.search}`,
      method: 'GET',
      headers: { accept: 'text/html,application/xhtml+xml' },
    }, response => {
      const chunks = [];
      response.on('data', chunk => chunks.push(chunk));
      response.on('end', () => {
        clearTimeout(timer);
        resolvePromise({ status: response.statusCode, body: Buffer.concat(chunks).toString('utf8') });
      });
    });
    const timer = setTimeout(() => {
      try { req.destroy(); } catch { /* already gone */ }
      rejectPromise(new Error(`request to ${url} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
    req.once('error', error => { clearTimeout(timer); rejectPromise(error); });
    req.end();
  });
}

// The static dist server: the app's own index for the root and for SPA routes,
// real files for real paths, 404 for a missing asset with an extension, 403 for
// a path that escapes the dist root. No directory listing, no rebuild, no watch.
export function createDistServer(distDir) {
  const root = resolve(distDir);
  const index = join(root, FRONTEND_INDEX_FILE);
  return createServer((req, res) => {
    let pathname;
    try {
      pathname = decodeURIComponent(new URL(req.url ?? '/', FRONTEND_DEV_URL).pathname);
    } catch {
      res.writeHead(400, { 'content-type': 'text/plain; charset=utf-8' });
      return res.end('bad request');
    }
    if (pathname.endsWith('/')) pathname += FRONTEND_INDEX_FILE;
    const candidate = resolve(root, `.${pathname}`);
    if (candidate !== root && !candidate.startsWith(`${root}${sep}`)) {
      res.writeHead(403, { 'content-type': 'text/plain; charset=utf-8' });
      return res.end('forbidden');
    }
    let target = null;
    if (existsSync(candidate) && statSync(candidate).isFile()) target = candidate;
    else if (extname(pathname) === '') target = index;
    if (target === null) {
      res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
      return res.end('not found');
    }
    let body;
    try { body = readFileSync(target); } catch {
      res.writeHead(500, { 'content-type': 'text/plain; charset=utf-8' });
      return res.end('unreadable');
    }
    res.writeHead(200, {
      'content-type': contentTypeFor(target),
      'content-length': String(body.length),
      'cache-control': 'no-store',
    });
    return res.end(body);
  });
}

function listen(server, { host, port, timeoutMs = BUDGETS.frontendReadyMs }) {
  return new Promise((resolvePromise, rejectPromise) => {
    const onError = error => {
      clearTimeout(timer);
      server.removeListener('listening', onListening);
      rejectPromise(error);
    };
    const onListening = () => {
      clearTimeout(timer);
      server.removeListener('error', onError);
      resolvePromise(server.address());
    };
    const timer = setTimeout(() => {
      server.removeListener('listening', onListening);
      server.removeListener('error', onError);
      rejectPromise(new Error(`listen on ${host}:${port} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
    server.once('error', onError);
    server.once('listening', onListening);
    server.listen(port, host);
  });
}

// Serve the frontend the debug binary expects, or fail typed. Called BEFORE the
// app is spawned (the webview loads the dev URL during boot) and torn down by
// the registry's own cleanup.
export async function ensureFrontendServed({
  rootDir,
  registry,
  evidence = null,
  host = FRONTEND_HOST,
  port = FRONTEND_PORT,
  distDir = null,
  deps = {},
} = {}) {
  if (!rootDir) throw new HarnessError('ASSERTION_FAILURE', 'ensureFrontendServed requires the checkout root');
  const dist = resolveFrontendDist(rootDir, distDir);
  const devUrl = assertFrontendDevUrlMatches(rootDir, { host, port });
  const occupied = await (deps.probePortOccupied ?? probePortOccupied)({
    host, port, timeoutMs: deps.portProbeMs ?? BUDGETS.frontendPortProbeMs, connect: deps.connect,
  });
  if (occupied) {
    throw new HarnessError('FRONTEND_PORT_OCCUPIED', `${host}:${port} is already held by a listener this run did not start; refusing to kill, signal, or reuse it (the app's devUrl must be served by this run alone)`);
  }
  const server = (deps.createServer ?? createDistServer)(dist.distDir);
  let address;
  try {
    address = await (deps.listen ?? listen)(server, { host, port, timeoutMs: deps.listenTimeoutMs ?? BUDGETS.frontendReadyMs });
  } catch (error) {
    try { server.close(); } catch { /* never bound */ }
    if (error?.code === 'EADDRINUSE') {
      throw new HarnessError('FRONTEND_PORT_OCCUPIED', `${host}:${port} was taken between the free-port probe and the bind (EADDRINUSE); a foreign listener is never reused`);
    }
    throw new HarnessError('FRONTEND_NOT_SERVED', `static frontend server failed to listen on ${host}:${port}: ${error?.message ?? error}`);
  }
  const boundPort = typeof address?.port === 'number' ? address.port : port;
  const url = `http://${host}:${boundPort}/`;
  if (registry && typeof registry.registerServer === 'function') {
    registry.registerServer(server, 'frontend-dist');
  }
  let served;
  try {
    served = await (deps.fetchIndexBody ?? fetchIndexBody)({ url, timeoutMs: deps.requestMs ?? BUDGETS.frontendRequestMs });
  } catch (error) {
    throw new HarnessError('FRONTEND_NOT_SERVED', `served frontend at ${url} did not answer within the bound: ${error?.message ?? error}`);
  }
  const body = String(served?.body ?? '');
  if (served?.status !== 200 || !body.includes(FRONTEND_ROOT_MARKER)) {
    throw new HarnessError('FRONTEND_NOT_SERVED', `served frontend at ${url} answered status=${JSON.stringify(served?.status)} bytes=${body.length} without the app root document (${FRONTEND_ROOT_MARKER}); the webview would render an error page, not the app`);
  }
  const record = {
    action: 'frontend.served',
    host,
    port: boundPort,
    url,
    distDir: dist.distDir,
    indexPath: dist.indexPath,
    indexBytes: body.length,
    rootMarker: FRONTEND_ROOT_MARKER,
    declaredDevUrl: devUrl.devUrl,
    route: 'static-ui-dist',
    rejectsDevServerRoute: 'bun scripts/dev-frontend.mjs rebuilds and watches the tree mid-run (HMR), which is not a deterministic QA fixture',
  };
  evidence?.action?.(record);
  return { ...record, server };
}
