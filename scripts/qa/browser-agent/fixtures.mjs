// Browser-agent QA fixture server (plan maho-browser-merge, T8 / "Concrete QA contract").
//
// Binds 127.0.0.1:18761 (origin A) and 127.0.0.1:18762 (origin B) EXCLUSIVELY. An occupied port is
// a hard error; there is no fallback port, because grants and redirects in Q1-Q7 name these exact
// origins. Both servers share one recorder so cross-origin effects (redirect, iframe) are visible.
//
// Routes (served on both origins):
//   GET  /                 index of fixture links
//   GET  /form             Name field + Submit; POST /form records the submission and renders "Saved"
//   GET  /replace          hostile page: overwrites page globals (document.querySelector, JSON, ...),
//                          replaceable target node, duplicate role+name buttons, password input with
//                          a value, iframe from the other origin
//   GET  /frame            iframe body (embedded by the other origin's /replace) with FRAME_SENTINEL
//   GET  /redirect-other   302 to the other origin's /form
//   GET  /dialog           button that opens confirm(); ?open=1 opens it on load
//   GET  /upload           file input form; POST /upload records byte count + SHA-256 per file
//   GET  /events           SSE: a `ready` frame (current seq) then one `record` frame per recorded event
//   GET  /__state          JSON snapshot of the recorder
//   POST /__event          page-reported events (key/click/input with event.isTrusted, dialog, ...)
//   GET  /__control/stream SSE consumed by /replace pages; POST /__control {"action":"replace"}
//                          replaces the target node in every connected /replace page
//
// Waiting is event driven: subscribe with subscribeEvents() BEFORE acting, then await a matching
// record with a bounded timeout. Nothing here sleeps or polls.

import { createHash } from 'node:crypto';

export const HOST = '127.0.0.1';
export const PORTS = Object.freeze({ a: 18761, b: 18762 });
export const ORIGINS = Object.freeze({ a: `http://${HOST}:${PORTS.a}`, b: `http://${HOST}:${PORTS.b}` });
/** Present as the password input value on /replace; must never appear in any agent output. */
export const PASSWORD_SENTINEL = 'ferryx-fixture-password-7f3c9a';
/** Present only inside the cross-origin iframe; must never appear in a parent-page observe. */
export const FRAME_SENTINEL = 'ferryx-fixture-frame-secret-41d2';
/** Plan C2: at most 64 MiB per upload request. */
export const MAX_UPLOAD_REQUEST_BYTES = 64 * 1024 * 1024;
const MAX_EVENT_BODY_BYTES = 16 * 1024;
const INTERNAL_PATHS = new Set(['/__state', '/__event', '/events', '/__control', '/__control/stream', '/favicon.ico']);
const BUCKETS = Object.freeze({ request: 'requests', 'form-submit': 'formSubmits', upload: 'uploads', input: 'inputs', dialog: 'dialogs', control: 'controls' });

const encoder = new TextEncoder();

function emptyState(seq = 0) {
  return { seq, requests: [], formSubmits: [], uploads: [], inputs: [], dialogs: [], controls: [], pageEvents: [] };
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function json(value, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json', 'cache-control': 'no-store' } });
}

// Installed first in <head> on every page, before /replace tampers with globals: it captures the
// originals it needs and reports key/click/input events with event.isTrusted to the page's origin.
// Password field keys are never reported.
const REPORTER = `<script>
(function () {
  var post = window.fetch.bind(window);
  var stringify = JSON.stringify.bind(JSON);
  function describe(t) {
    if (!t || !t.tagName) return null;
    return { tag: t.tagName.toLowerCase(), id: t.id || null, name: t.getAttribute('name'), type: t.getAttribute('type'), gen: t.getAttribute('data-gen'), text: (t.textContent || '').trim().slice(0, 80) };
  }
  function report(type, data) {
    var body = { type: type, page: location.pathname, href: location.href };
    for (var k in data) body[k] = data[k];
    return post('/__event', { method: 'POST', headers: { 'content-type': 'application/json' }, body: stringify(body), keepalive: true }).catch(function () {});
  }
  ['keydown', 'click', 'input'].forEach(function (name) {
    document.addEventListener(name, function (e) {
      var secret = !!(e.target && e.target.type === 'password');
      report('input', { event: name, isTrusted: e.isTrusted, target: describe(e.target), key: secret ? null : (e.key || null) });
    }, true);
  });
  window.__ferryxFixtureReport = report;
})();
</script>`;

function html(body, { status = 200, title = 'Ferryx fixture' } = {}) {
  return new Response(`<!doctype html><html><head><meta charset="utf-8"><title>${escapeHtml(title)}</title>${REPORTER}</head><body>${body}</body></html>`, {
    status,
    headers: { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' },
  });
}

function formPage() {
  return html(`<h1>Form fixture</h1>
<form method="post" action="/form">
  <label for="name">Name</label>
  <input id="name" name="name" type="text" autocomplete="off">
  <button type="submit">Submit</button>
</form>`, { title: 'Form fixture' });
}

function savedPage(name, count) {
  return html(`<h1>Saved</h1><p id="saved-name">${escapeHtml(name)}</p><p id="submit-count">${count}</p><a href="/form">Back</a>`, { title: 'Saved' });
}

function replacePage(other) {
  return html(`<h1>Replace fixture</h1>
<section id="slot"><button id="target" data-gen="0">Target action</button></section>
<section aria-label="Duplicates"><button class="dup" data-dup="1">Delete</button><button class="dup" data-dup="2">Delete</button></section>
<label for="pw">Password</label><input id="pw" name="pw" type="password" value="${PASSWORD_SENTINEL}">
<button id="replace-now">Replace target node</button>
<p id="status">gen 0</p>
<iframe title="Other origin frame" src="${other}/frame" width="320" height="120"></iframe>
<script>
(function () {
  var report = window.__ferryxFixtureReport;
  var byId = document.getElementById.bind(document);
  var create = document.createElement.bind(document);
  var gen = 0;
  function wire(node) {
    node.addEventListener('click', function (e) { report('target-activated', { gen: node.getAttribute('data-gen'), isTrusted: e.isTrusted }); });
  }
  function replaceTarget(reason) {
    var old = byId('target');
    gen += 1;
    var next = create('button');
    next.id = 'target';
    next.setAttribute('data-gen', String(gen));
    next.textContent = 'Target action';
    wire(next);
    old.replaceWith(next);
    byId('status').textContent = 'gen ' + gen;
    report('node-replaced', { gen: gen, reason: reason });
  }
  wire(byId('target'));
  Array.prototype.forEach.call(document.getElementsByClassName('dup'), function (b) {
    b.addEventListener('click', function (e) { report('duplicate-activated', { dup: b.getAttribute('data-dup'), isTrusted: e.isTrusted }); });
  });
  byId('replace-now').addEventListener('click', function () { replaceTarget('button'); });
  var control = new EventSource('/__control/stream');
  control.addEventListener('replace', function () { replaceTarget('control'); });
  control.addEventListener('open', function () { report('control-ready', {}); });

  // Hostile globals: the agent's isolated world must not depend on any of these.
  document.querySelector = function () { return null; };
  document.querySelectorAll = function () { return []; };
  document.getElementById = function () { return null; };
  Element.prototype.getBoundingClientRect = function () { return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 }; };
  HTMLElement.prototype.click = function () { report('tampered-click', { id: this.id || null }); };
  window.JSON = { parse: function () { throw new Error('tampered JSON.parse'); }, stringify: function () { return '"tampered"'; } };
  report('globals-tampered', {});
})();
</script>`, { title: 'Replace fixture' });
}

function framePage() {
  return html(`<p id="frame-secret">${FRAME_SENTINEL}</p><button id="frame-button">Frame button</button>`, { title: 'Frame fixture' });
}

function dialogPage(openOnLoad) {
  return html(`<h1>Dialog fixture</h1>
<button id="open-confirm">Open confirm</button>
<button id="mutate">Mutate</button>
<p id="out">Idle</p>
<script>
(function () {
  var report = window.__ferryxFixtureReport;
  var out = document.getElementById('out');
  function openConfirm() {
    report('dialog-opened', {});
    var result = window.confirm('Ferryx fixture confirm?');
    out.textContent = result ? 'Confirmed' : 'Cancelled';
    report('dialog', { result: result });
  }
  document.getElementById('open-confirm').addEventListener('click', openConfirm);
  document.getElementById('mutate').addEventListener('click', function (e) {
    out.textContent = 'Mutated';
    report('mutation', { isTrusted: e.isTrusted });
  });
  ${openOnLoad ? "window.addEventListener('load', openConfirm);" : ''}
})();
</script>`, { title: 'Dialog fixture' });
}

function uploadPage() {
  return html(`<h1>Upload fixture</h1>
<form method="post" action="/upload" enctype="multipart/form-data">
  <label for="file">File</label>
  <input id="file" name="file" type="file" multiple>
  <button type="submit">Upload</button>
</form>`, { title: 'Upload fixture' });
}

function uploadedPage(files) {
  const items = files.map((f) => `<li>${escapeHtml(f.filename)} size=${f.size} sha256=${f.sha256}</li>`).join('');
  return html(`<h1>Uploaded</h1><ul id="uploads">${items}</ul>`, { title: 'Uploaded' });
}

function indexPage() {
  const links = ['/form', '/replace', '/redirect-other', '/dialog', '/upload'].map((p) => `<li><a href="${p}">${p}</a></li>`).join('');
  return html(`<h1>Ferryx browser-agent fixtures</h1><ul>${links}</ul>`, { title: 'Fixtures' });
}

function sseFrame(event, data) {
  return encoder.encode(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
}

/** Opens an SSE stream registered in `streams` until the client aborts or the server stops. */
function openSse(req, streams, first) {
  let entry;
  const stream = new ReadableStream({
    start(controller) {
      entry = { controller };
      streams.add(entry);
      if (first) controller.enqueue(first());
      req.signal.addEventListener('abort', () => {
        if (!streams.delete(entry)) return;
        try { controller.close(); } catch { /* already closed */ }
      }, { once: true });
    },
    cancel() { streams.delete(entry); },
  });
  return new Response(stream, { headers: { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' } });
}

function broadcast(streams, bytes) {
  for (const entry of streams) {
    try { entry.controller.enqueue(bytes); } catch { streams.delete(entry); }
  }
}

function closeStreams(streams) {
  for (const entry of streams) {
    try { entry.controller.close(); } catch { /* already closed */ }
  }
  streams.clear();
}

async function readBoundedJson(req, limit) {
  if (Number(req.headers.get('content-length') ?? '0') > limit) return { error: 'too-large' };
  const buf = new Uint8Array(await req.arrayBuffer());
  if (buf.byteLength > limit) return { error: 'too-large' };
  try { return { value: JSON.parse(new TextDecoder().decode(buf)) }; } catch { return { error: 'malformed' }; }
}

/**
 * Starts both fixture origins. Rejects (after stopping anything it bound) if either port is busy.
 * Returns { origins, ports, state(), reset(), on(listener), stop() }. stop() is idempotent and
 * resolves once both listeners are stopped and every SSE stream is closed.
 */
export async function startFixtures({ host = HOST, ports = PORTS } = {}) {
  const origins = Object.freeze({ a: `http://${host}:${ports.a}`, b: `http://${host}:${ports.b}` });
  let state = emptyState();
  const eventStreams = new Set();
  const controlStreams = new Set();
  const listeners = new Set();
  const snapshot = () => structuredClone(state);

  const record = (kind, fields) => {
    state.seq += 1;
    const entry = { seq: state.seq, kind, at: new Date().toISOString(), ...fields };
    state[BUCKETS[kind] ?? 'pageEvents'].push(entry);
    broadcast(eventStreams, sseFrame('record', entry));
    for (const listener of listeners) listener(entry);
    return entry;
  };

  const handler = (self, other) => async (req) => {
    const url = new URL(req.url);
    const path = url.pathname;
    if (!INTERNAL_PATHS.has(path)) record('request', { origin: self, method: req.method, path, search: url.search });

    if (req.method === 'GET') {
      switch (path) {
        case '/': return indexPage();
        case '/form': return formPage();
        case '/replace': return replacePage(other);
        case '/frame': return framePage();
        case '/redirect-other': return new Response(null, { status: 302, headers: { location: `${other}/form`, 'cache-control': 'no-store' } });
        case '/dialog': return dialogPage(url.searchParams.get('open') === '1');
        case '/upload': return uploadPage();
        case '/__state': return json(snapshot());
        case '/events': return openSse(req, eventStreams, () => sseFrame('ready', { seq: state.seq, origin: self }));
        case '/__control/stream': return openSse(req, controlStreams, null);
        default: return new Response('not found', { status: 404 });
      }
    }

    if (req.method === 'POST') {
      switch (path) {
        case '/form': {
          const form = await req.formData();
          const name = String(form.get('name') ?? '');
          const fields = Object.fromEntries([...form.keys()].map((k) => [k, String(form.get(k))]));
          record('form-submit', { origin: self, name, fields });
          return savedPage(name, state.formSubmits.filter((s) => s.origin === self).length);
        }
        case '/upload': {
          const declared = Number(req.headers.get('content-length') ?? '0');
          if (declared > MAX_UPLOAD_REQUEST_BYTES) {
            record('upload-rejected', { origin: self, declared });
            return new Response('upload too large', { status: 413 });
          }
          const form = await req.formData();
          const files = [];
          for (const value of form.getAll('file')) {
            if (typeof value === 'string') continue;
            const bytes = new Uint8Array(await value.arrayBuffer());
            files.push({ filename: value.name, size: bytes.byteLength, sha256: createHash('sha256').update(bytes).digest('hex') });
          }
          for (const f of files) record('upload', { origin: self, ...f });
          if (files.length === 0) record('upload-empty', { origin: self });
          return uploadedPage(files);
        }
        case '/__event': {
          const body = await readBoundedJson(req, MAX_EVENT_BODY_BYTES);
          if (body.error) return json({ error: body.error }, 400);
          const { type, ...rest } = body.value ?? {};
          const kind = type === 'input' || type === 'dialog' ? type : `page:${String(type)}`;
          record(kind, { origin: self, ...rest });
          return json({ ok: true });
        }
        case '/__control': {
          const body = await readBoundedJson(req, MAX_EVENT_BODY_BYTES);
          if (body.error) return json({ error: body.error }, 400);
          if (body.value?.action !== 'replace') return json({ error: 'unknown action' }, 400);
          const delivered = controlStreams.size;
          broadcast(controlStreams, sseFrame('replace', {}));
          record('control', { origin: self, action: 'replace', delivered });
          return json({ delivered });
        }
        default: return new Response('not found', { status: 404 });
      }
    }
    return new Response('method not allowed', { status: 405 });
  };

  const servers = [];
  const bind = (port, self, other) => {
    try {
      return Bun.serve({
        hostname: host,
        port,
        reusePort: false,
        idleTimeout: 0,
        maxRequestBodySize: MAX_UPLOAD_REQUEST_BYTES + 1024 * 1024,
        fetch: handler(self, other),
      });
    } catch (error) {
      throw new Error(`fixture port ${host}:${port} is unavailable (${error?.code ?? error?.message ?? error}); `
        + 'the browser-agent QA contract requires this exact port and never falls back', { cause: error });
    }
  };
  try {
    servers.push(bind(ports.a, origins.a, origins.b));
    servers.push(bind(ports.b, origins.b, origins.a));
  } catch (error) {
    await Promise.all(servers.map((s) => s.stop(true)));
    throw error;
  }

  let stopping = null;
  return {
    origins,
    ports: Object.freeze({ ...ports }),
    state: snapshot,
    /** Clears recorded effects between scenarios; seq stays monotonic, SSE subscribers stay connected. */
    reset() { state = emptyState(state.seq); },
    /** In-process listener; external observers use subscribeEvents(). Returns an unsubscribe fn. */
    on(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    stop() {
      stopping ??= (async () => {
        listeners.clear();
        closeStreams(eventStreams);
        closeStreams(controlStreams);
        await Promise.all(servers.map((s) => s.stop(true)));
      })();
      return stopping;
    },
  };
}

/** Teardown receipt: each fixture port must be bindable again after stop(). */
export function verifyPortsReleased({ host = HOST, ports = PORTS } = {}) {
  return Object.values(ports).map((port) => {
    try {
      const probe = Bun.listen({ hostname: host, port, socket: { data() {} } });
      probe.stop(true);
      return { port, released: true };
    } catch (error) {
      return { port, released: false, error: String(error?.code ?? error?.message ?? error) };
    }
  });
}

function deadline(promise, ms, label, onTimeout) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => { onTimeout?.(); reject(new Error(`deadline ${ms}ms: ${label}`)); }, ms);
    }),
  ]).finally(() => clearTimeout(timer));
}

/**
 * SSE subscriber for <origin>/events. Resolves only after the `ready` frame arrives, so a caller
 * that awaits this before acting cannot miss the effect. waitFor() first scans records received
 * since ready, then awaits new ones until its bounded timeout.
 */
export async function subscribeEvents(origin, { timeoutMs = 10_000 } = {}) {
  const abort = new AbortController();
  const records = [];
  const waiters = new Set();
  let readyResolve;
  let readyReject;
  const ready = new Promise((resolve, reject) => { readyResolve = resolve; readyReject = reject; });
  ready.catch(() => {});
  let closedError = null;

  const deliver = (event, data) => {
    if (event === 'ready') { readyResolve(data); return; }
    if (event !== 'record') return;
    records.push(data);
    for (const w of waiters) {
      if (w.predicate(data)) { waiters.delete(w); w.resolve(data); }
    }
  };
  const failAll = (error) => {
    closedError = error;
    readyReject(error);
    for (const w of waiters) w.reject(error);
    waiters.clear();
  };

  const response = await deadline(fetch(`${origin}/events`, { signal: abort.signal, headers: { accept: 'text/event-stream' } }),
    timeoutMs, `connect ${origin}/events`, () => abort.abort());
  if (!response.ok || !response.body) throw new Error(`${origin}/events returned ${response.status}`);

  (async () => {
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let split;
        while ((split = buffer.indexOf('\n\n')) !== -1) {
          const block = buffer.slice(0, split);
          buffer = buffer.slice(split + 2);
          let event = 'message';
          const data = [];
          for (const line of block.split('\n')) {
            if (line.startsWith('event:')) event = line.slice(6).trim();
            else if (line.startsWith('data:')) data.push(line.slice(5).trimStart());
          }
          if (data.length) deliver(event, JSON.parse(data.join('\n')));
        }
      }
      failAll(new Error(`${origin}/events stream ended`));
    } catch (error) {
      failAll(abort.signal.aborted ? new Error(`${origin}/events closed`) : error);
    }
  })();

  const readyFrame = await deadline(ready, timeoutMs, `ready from ${origin}/events`, () => abort.abort());
  return {
    origin,
    readySeq: readyFrame.seq,
    records,
    waitFor(predicate, { timeoutMs: waitMs = 10_000, label = 'fixture record' } = {}) {
      const hit = records.find(predicate);
      if (hit) return Promise.resolve(hit);
      if (closedError) return Promise.reject(closedError);
      let waiter;
      const p = new Promise((resolve, reject) => { waiter = { predicate, resolve, reject }; waiters.add(waiter); });
      return deadline(p, waitMs, `${label} on ${origin}`, () => waiters.delete(waiter));
    },
    close() { abort.abort(); },
  };
}
