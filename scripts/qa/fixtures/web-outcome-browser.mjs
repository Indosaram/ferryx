/** Browser-side wire receipts. Installed before the candidate bundle, not a fake UI. */
export function browserReceiptScript({ token, theme, holdTicketTarget = null }) {
  return `<script>(${installBrowserReceipts.toString()})(${JSON.stringify({ token, theme, holdTicketTarget })})</script>`;
}

function installBrowserReceipts({ token, theme, holdTicketTarget }) {
  localStorage.setItem('ferryx_remote_token', token);
  localStorage.setItem('ferryx_remote_token_local:' + location.origin, token);
  localStorage.setItem('ferryx.settings.appearance', JSON.stringify({ theme }));
  document.documentElement.classList.toggle('dark', theme === 'dark');
  document.documentElement.setAttribute('data-theme', theme);
  document.documentElement.style.colorScheme = theme;
  const NativeSocket = window.WebSocket;
  const nativeFetch = window.fetch.bind(window);
  const nativeSetTimeout = window.setTimeout.bind(window);
  const pending = new Set();
  const sockets = new Set();
  const events = [];
  let action = null;
  let sequence = 0;
  const observed = new Map();
  const waiters = new Map();
  const heldSockets = new Map();
  const retiredObservers = new Map();
  const pendingSocketCompletions = [];
  let ticketCompletion = null;
  const record = (kind, detail) => events.push({ sequence: ++sequence, kind, ...detail });
  const observe = id => {
    observed.set(id, true);
    waiters.get(id)?.();
    waiters.delete(id);
  };
  // Capturing actual user/QA DOM clicks associates the next request with its action.
  document.addEventListener('click', event => {
    const el = event.target.closest('button');
    if (!el) return;
    action = { id: crypto.randomUUID(), testId: el.dataset.testid, text: el.textContent };
    record('click', { action });
  }, true);
  window.fetch = (input, init) => {
    const url = new URL(typeof input === 'string' ? input : input.url, location.href);
    if (url.origin !== location.origin) throw new Error('TASK9_NON_ISOLATED_FETCH');
    const requestId = crypto.randomUUID();
    const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
    headers.set('x-task9-request-id', requestId);
    if (action) {
      headers.set('x-task9-action-id', action.id);
      headers.set('x-task9-action-testid', action.testId ?? '');
    }
    const promise = nativeFetch(input, { ...init, headers });
    pending.add(promise);
    promise.then(response => record('http-response', { requestId, status: response.status }),
      error => record('http-error', { requestId, error: String(error) })).finally(() => pending.delete(promise));
    if (holdTicketTarget && !ticketCompletion && url.pathname === '/api/v1/socket-ticket' &&
        JSON.parse(init?.body ?? '{}').target === '/api/v1/terminal/' + holdTicketTarget.sessionId) {
      let release;
      const released = new Promise(resolve => { release = resolve; });
      const entry = { target: { ...holdTicketTarget }, requestId, release, released, delivered: false };
      ticketCompletion = entry;
      // Receive the real single-use ticket completely before withholding delivery. An
      // owner abort after this point cannot turn the buffered body into AbortError.
      entry.received = promise.then(async response => {
        const body = await response.arrayBuffer();
        entry.status = response.status;
        record('ticket-completion-held', { requestId, target: entry.target, status: response.status });
        return { body, status: response.status, statusText: response.statusText, headers: response.headers };
      });
      entry.received.catch(() => {});
      return entry.received.then(async response => {
        await released;
        entry.delivered = true;
        record('ticket-completion-delivered', { requestId, target: entry.target });
        return new Response(response.body, response);
      });
    }
    return promise;
  };
  window.WebSocket = class ReceiptSocket extends NativeSocket {
    constructor(url, protocols) {
      const parsed = new URL(url, location.href);
      if (parsed.hostname !== location.hostname || parsed.port !== location.port)
        throw new Error('TASK9_NON_ISOLATED_SOCKET');
      const socketId = crypto.randomUUID();
      parsed.searchParams.set('task9SocketId', socketId);
      super(parsed.href, protocols);
      sockets.add(this);
      this.__task9Id = socketId;
      this.addEventListener('message', event => {
        record('message', { socketId, raw: event.data });
        if (typeof event.data !== 'string') return;
        try {
          const frame = JSON.parse(event.data);
          if (frame.task9ReceiptId) observe(frame.task9ReceiptId);
        } catch { /* OSC/binary output is retained as raw data. */ }
      });
      this.addEventListener('close', event => {
        record('close', { socketId, code: event.code }); sockets.delete(this);
        observe('close:' + socketId);
      });
    }
    send(data) {
      record('send', { socketId: this.__task9Id, raw: data });
      return super.send(data);
    }
  };
  window.__task9Wire = {
    events,
    armTicketCompletion(target, attemptId, timeoutMs) {
      const entry = ticketCompletion;
      if (!entry || entry.target.sessionId !== target.sessionId || entry.target.daemonEpoch !== target.daemonEpoch)
        throw new Error('TASK9_TICKET_COMPLETION_TARGET_MISMATCH');
      if (entry.key) throw new Error('TASK9_TICKET_COMPLETION_ALREADY_ARMED');
      entry.key = entry.requestId + ':' + attemptId;
      const qa = window.__ferryxRecoveryQA;
      if (!qa) throw new Error('TASK9_OWNER_OBSERVER_MISSING');
      retiredObservers.set(entry.key, qa.waitForRetiredStaleCompletion(
        { sessionId: target.sessionId, daemonEpoch: target.daemonEpoch }, attemptId, timeoutMs));
      retiredObservers.get(entry.key).catch(() => {});
      return entry.key;
    },
    async waitTicketHeld(key, timeoutMs) {
      const entry = ticketCompletion;
      if (!entry || entry.key !== key) throw new Error('TASK9_TICKET_COMPLETION_NOT_ARMED');
      let timer;
      try {
        await Promise.race([entry.received, new Promise((_, reject) => {
          timer = nativeSetTimeout(() => reject(new Error('TASK9_TICKET_COMPLETION_DEADLINE')), timeoutMs);
        })]);
        return { requestId: entry.requestId, target: entry.target, status: entry.status, delivered: entry.delivered };
      } finally { window.clearTimeout(timer); }
    },
    releaseTicketCompletion(key) {
      const entry = ticketCompletion;
      if (!entry || entry.key !== key || entry.status === undefined)
        throw new Error('TASK9_TICKET_COMPLETION_NOT_HELD');
      entry.release();
      return { requestId: entry.requestId, target: entry.target, status: entry.status };
    },
    wait(id, timeoutMs) {
      if (observed.has(id)) return Promise.resolve(true);
      return new Promise((resolve, reject) => {
        const timer = nativeSetTimeout(() => { waiters.delete(id); reject(new Error('TASK9_BROWSER_RECEIPT_DEADLINE')); }, timeoutMs);
        waiters.set(id, () => { clearTimeout(timer); resolve(true); });
      });
    },
    armSocketCompletion(target, attemptId, timeoutMs) {
      const key = target.sessionId + ':' + target.daemonEpoch + ':' + attemptId;
      if (heldSockets.has(key)) throw new Error('TASK9_SOCKET_COMPLETION_ALREADY_ARMED');
      const qa = window.__ferryxRecoveryQA;
      if (!qa) throw new Error('TASK9_OWNER_OBSERVER_MISSING');
      let notifyHeld;
      let releaseSocket;
      const held = new Promise(resolve => { notifyHeld = resolve; });
      const released = new Promise(resolve => { releaseSocket = resolve; });
      const gate = { target, attemptId, notifyHeld, held, releaseSocket, released, socket: null };
      heldSockets.set(key, gate);
      retiredObservers.set(key, qa.waitForRetiredStaleCompletion({ sessionId: target.sessionId, daemonEpoch: target.daemonEpoch }, attemptId, timeoutMs));
      retiredObservers.get(key).catch(() => {});
      const pendingIndex = pendingSocketCompletions.findIndex(item => item.sessionId === target.sessionId && item.daemonEpoch === target.daemonEpoch);
      if (pendingIndex >= 0) {
        const [pending] = pendingSocketCompletions.splice(pendingIndex, 1);
        gate.pendingDelivery = pending;
        attachSocket(gate, pending.socket);
      }
      return key;
    },
    async waitSocketHeld(key, timeoutMs) {
      let timer;
      try {
        const entry = heldSockets.get(key);
        if (!entry) throw new Error('TASK9_SOCKET_COMPLETION_NOT_ARMED');
        await Promise.race([entry.held, new Promise((_, reject) => {
          timer = nativeSetTimeout(() => reject(new Error('TASK9_SOCKET_COMPLETION_DEADLINE')), timeoutMs);
        })]);
        return { readyState: entry.socket.readyState, url: entry.socket.url };
      } finally { window.clearTimeout(timer); }
    },
    releaseSocketCompletion(key) {
      const entry = heldSockets.get(key);
      if (!entry?.socket) throw new Error('TASK9_SOCKET_COMPLETION_NOT_HELD');
      entry.releaseSocket();
      entry.pendingDelivery?.resolve();
      heldSockets.delete(key);
      return { readyState: entry.socket.readyState, url: entry.socket.url };
    },
    async readRetiredObserver(key) {
      const observer = retiredObservers.get(key);
      if (!observer) throw new Error('TASK9_RETIRED_OBSERVER_NOT_ARMED');
      return await observer;
    },
    async fence(id) {
      // Existing outstanding fetches must finish/abort; no number of microtasks is guessed.
      let timer;
      try {
        await Promise.race([Promise.allSettled([...pending]), new Promise((resolve, reject) => {
          timer = nativeSetTimeout(() => reject(new Error('TASK9_HTTP_FENCE_DEADLINE')), 15000);
        })]);
      } finally { clearTimeout(timer); }
      const controls = [...sockets].filter(socket => socket.readyState === NativeSocket.OPEN);
      await Promise.all(controls.map(socket => new Promise((resolve, reject) => {
        const receiptId = id + ':' + socket.__task9Id;
        this.wait(receiptId, 15000).then(resolve, reject);
        socket.send(JSON.stringify({ type: 'task9Fence', receiptId }));
      })));
      return { events: [...events], sockets: controls.map(s => s.__task9Id) };
    },
  };
  window.__task9SocketCompletion = {
    async deferResolvedSocket(path, socket) {
      const url = new URL(path, location.href);
      const sessionId = decodeURIComponent(url.pathname.slice('/api/v1/terminal/'.length));
      const daemonEpoch = url.searchParams.get('daemonEpoch');
      const entry = [...heldSockets.values()].find(candidate => candidate.target.sessionId === sessionId &&
        candidate.target.daemonEpoch === daemonEpoch && !candidate.socket);
      if (entry) {
        attachSocket(entry, socket);
        await entry.released;
      } else {
        await new Promise(resolve => pendingSocketCompletions.push({ sessionId, daemonEpoch, socket, resolve }));
      }
    },
  };
  function attachSocket(entry, socket) {
    entry.socket = socket;
    entry.notifyHeld();
      entry.pendingDelivery?.resolve();
  }
}

export async function driveTerminalInput(browser, { kind, payload }) {
  await browser.evaluate(`(() => {
    const grid = document.querySelector('[data-testid="remote-terminal-grid"]');
    const sink = document.querySelector('[data-testid="remote-terminal-input-sink"]');
    if (!grid || !sink) throw new Error('TASK9_TERMINAL_INPUT_MISSING');
    const intent = ${JSON.stringify({ kind, payload })};
    if (intent.kind === 'signal') {
      grid.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'c', code: 'KeyC', ctrlKey: true, bubbles: true, cancelable: true
      }));
    } else {
      sink.focus();
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(sink, intent.payload);
      sink.dispatchEvent(new InputEvent('input', {
        data: intent.payload, inputType: 'insertText', bubbles: true
      }));
    }
  })()`);
}
