import { randomUUID } from 'node:crypto';
import { WebGatewayFixture } from '../session-recovery-consistency.mjs';
import { browserReceiptScript } from './web-outcome-browser.mjs';

function missing(signature) {
  throw Object.assign(new Error(`TASK9_PRODUCT_SEAM_REQUIRED: ${signature}`), {
    code: 'TASK9_PRODUCT_SEAM_REQUIRED', signature,
  });
}

function grid(text, task9ReceiptId) {
  return { type: 'grid', cols: 80, rows: 24, task9ReceiptId,
    cursor: { x: 0, y: 0, visible: true, blinking: false, wideTail: false, visualStyle: 'block' },
    lines: [{ index: 0, runs: [{ text, fg: null, bg: null, attrs: 0, cells: text.length }] }] };
}

/** Uses the existing native Bun WS server/static serving/bootstrap, with owned controls. */
export class WebOutcomeGateway extends WebGatewayFixture {
  constructor(options) {
    super({ distDir: options.uiDist, sessionId: options.target.sessionId,
      daemonEpoch: options.target.daemonEpoch, mode: 'handshake-timeout' });
    this.options = options;
    this.targets = new Map([options.target, options.rebindTarget].map(t => [t.sessionId, t]));
    this.onboardingStorage = Object.freeze({
      key: 'ferryx.onboarding.v1',
      value: Object.freeze({ version: 1, completedSteps: ['intro', 'features', 'permissions', 'agents', 'project'], dismissed: true }),
    });
    this.token = `task9-${randomUUID()}`;
    this.authorized = true;
    this.autoReady = false;
    this.rawRequests = [];
    this.deliveries = [];
    this.rawFrames = [];
    this.selectionRequests = [];
    this.createdSessionIds = new Set();
    this.held = new Map();
    this.heldDials = new Map();
    this.sequence = 0;
    this.proof = null;
    this.capacity = 512 * 1024;
    this.ring = Buffer.alloc(0);
    this.outputCursor = 0;
    this.requestedCursor = 0;
    this.operation = null;
    if (options.scenario === 'close-rebind-pending-dial') this.operation = 'hold-dial';
  }

  receipt(target, detail) {
    return { target, sequence: ++this.sequence, source: 'isolated-gateway-wire', ...detail };
  }

  async handleFetch(req, server) {
    const url = new URL(req.url);
    const rawBody = req.method === 'GET' || req.method === 'HEAD' ? null : await req.clone().text();
    const record = this.receipt(this.targets.get(this.activeSessionId), {
      requestId: req.headers.get('x-task9-request-id') ?? randomUUID(),
      path: url.pathname, method: req.method, rawBody,
      browserActionId: req.headers.get('x-task9-action-id'),
      browserActionTestId: req.headers.get('x-task9-action-testid'), type: 'http',
    });
    this.rawRequests.push(record);
    if (url.pathname === '/api/v1/pair/exchange' && req.method === 'POST') {
      const body = JSON.parse(rawBody);
      if (body.code !== '123456' || !this.authorized)
        return Response.json({ error: 'INVALID_PAIR_CODE' }, { status: 403 });
      return Response.json({ token: this.token });
    }
    if (url.pathname === '/api/v1/socket-ticket') {
      if (!this.authorized || req.headers.get('authorization') !== `Bearer ${this.token}`) {
        record.status = 401;
        this.emit('auth-rejected', record);
        return Response.json({ error: 'UNAUTHORIZED' }, { status: 401 });
      }
      const parsed = JSON.parse(rawBody);
      const targetPath = parsed.target;
      const sessionId = targetPath?.startsWith('/api/v1/terminal/')
        ? decodeURIComponent(targetPath.slice('/api/v1/terminal/'.length)) : null;
      if (targetPath !== '/api/v1/events' && !this.targets.has(sessionId))
        return Response.json({ error: 'INVALID_TARGET' }, { status: 400 });
      const issue = () => {
        const ticket = `task9-ticket-${randomUUID()}`;
        const expiresAt = Math.floor(Date.now() / 1000) + 30;
        this.ticketStore.set(ticket, { target: targetPath, expiresAt });
        return Response.json({ ticket, expiresAt });
      };
      if (this.operation === 'hold-dial' && sessionId === this.options.target.sessionId) {
        return new Promise(resolve => {
          const entry = { issue, record, settle: response => {
            if (this.held.get(record.requestId) !== entry) return false;
            this.held.delete(record.requestId);
            clearTimeout(entry.timer);
            req.signal.removeEventListener('abort', onAbort);
            resolve(response);
            return true;
          } };
          const onAbort = () => {
            if (entry.settle(Response.json({ error: 'FIXTURE_DIAL_CANCELLED' }, { status: 499 })))
              this.emit('dial-cancelled', { ...record, target: this.targets.get(sessionId), attemptId: record.requestId });
          };
          this.held.set(record.requestId, entry);
          entry.timer = setTimeout(() => entry.settle(
            Response.json({ error: 'FIXTURE_DIAL_DEADLINE' }, { status: 504 })), this.options.timeoutMs);
          req.signal.addEventListener('abort', onAbort, { once: true });
          if (req.signal.aborted) onAbort();
          else {
            this.emit(this.options.scenario === 'cold-start-unknown' ? 'coldstart-old-owner-held' : 'dial-held', {
              ...record, target: this.targets.get(sessionId), attemptId: record.requestId,
              completionId: this.options.scenario === 'cold-start-unknown' ? this.coldStartCompletionId : undefined,
            });
          }
        });
      }
      return issue();
    }
    // Routes required for explicit New terminal must come from the actual candidate,
    // not guessed path matching. Their exact method/path is supplied by the product seam.
    const createRoute = this.options.createRoute;
    if (createRoute && req.method === createRoute.method && url.pathname === createRoute.path &&
        (url.pathname !== '/api/v1/workspace/select' || JSON.parse(rawBody)?.createTerminal === true)) {
      record.type = 'createSession';
      this.emit('create-request', record);
      const selected = JSON.parse(rawBody);
      const response = createRoute.response(this.targets.get(this.activeSessionId), selected);
      if (!this.targets.has(response.sessionId)) throw new Error('TASK9_CREATE_TARGET_REQUIRED');
      this.activeSessionId = response.sessionId;
      this.createdSessionIds.add(response.sessionId);
      this.autoReady = true;
      for (const ws of this.eventSockets) ws.send(JSON.stringify({
        event: 'remote_active_selection_changed',
        payload: { workspaceId: selected.workspaceId, worktreeSlug: selected.worktreeSlug, tabId: response.sessionId },
      }));
      return Response.json(response);
    }
    if (url.pathname === '/api/v1/workspace/select' && req.method === 'POST') {
      this.selectionRequests.push(record);
      const selected = JSON.parse(rawBody);
      const sessionId = selected.tabId ?? selected.sessionId;
      if (!this.targets.has(sessionId)) return Response.json({ error: 'INVALID_TARGET' }, { status: 400 });
      this.activeSessionId = sessionId;
      this.autoReady = true;
      for (const ws of this.eventSockets) ws.send(JSON.stringify({ event: 'remote_active_selection_changed', payload: selected }));
      return Response.json({ ok: true });
    }
    if (url.pathname === '/api/v1/terminal/preferences') {
      return Response.json(this.options.preferences ?? {});
    }
    const terminalPrefix = '/api/v1/terminal/';
    if (url.pathname.startsWith(terminalPrefix)) {
      const sessionId = decodeURIComponent(url.pathname.slice(terminalPrefix.length));
      if (this.targets.has(sessionId)) {
        this.activeSessionId = sessionId;
        const before = this.terminalDials.length;
        const response = super.handleFetch(req, server);
        record.socketId = url.searchParams.get('task9SocketId');
        record.dialIndex = before + 1;
        return response;
      }
      return super.handleFetch(req, server);
    }
    const response = await super.handleFetch(req, server);
    if (response?.headers.get('content-type')?.startsWith('text/html')) {
      const html = await response.text();
      return new Response(html.replace('</head>', browserReceiptScript({ token: this.token, theme: this.options.theme,
        holdTicketTarget: this.options.scenario === 'cold-start-unknown' ? this.options.target : null }) + '</head>'),
        { status: response.status, headers: response.headers });
    }
    if (url.pathname === '/api/v1/workspace/state' && response) {
      const body = await response.json();
      const target = this.targets.get(this.activeSessionId);
      body.activeContext.daemonEpoch = target.daemonEpoch;
      body.activeContext.activeTerminal = { ...body.activeContext.activeTerminal, ...target };
      body.activeContext.terminalTabs = [...this.targets.values()].map(t => ({ id: t.sessionId, label: t.sessionId,
        active: t.sessionId === this.activeSessionId }));
      body.sessions = [...this.targets.values()]
        .filter(t => this.options.initial !== 'historical-owner-unavailable' || this.createdSessionIds.has(t.sessionId))
        .map(t => ({ ...t, running: true }));
      return Response.json(body);
    }
    if (url.pathname === '/api/v1/sessions') {
      if (this.options.initial === 'historical-owner-unavailable' && this.createdSessionIds.size === 0)
        return Response.json({ error: 'OWNER_UNAVAILABLE' }, { status: 503 });
      const sessions = [...this.targets.values()]
        .filter(t => this.options.initial !== 'historical-owner-unavailable' || this.createdSessionIds.has(t.sessionId))
        .map(t => ({ ...t, running: true }));
      return Response.json(sessions, {
        headers: { 'Access-Control-Allow-Origin': '*' },
      });
    }
    return response;
  }

  handleWsOpen(ws) {
    super.handleWsOpen(ws);
    if (ws.data.type !== 'terminal') return;
    ws.data.socketId = new URL(ws.data.url).searchParams.get('task9SocketId');
    if (this.operation === 'hold-dial' && ws.data.sessionId === this.options.target.sessionId) {
      this.heldDials.set(ws.data.socketId, ws);
      this.emit('dial-held', this.receipt(this.targets.get(ws.data.sessionId), {
        socketId: ws.data.socketId, attemptId: ws.data.socketId,
      }));
    } else if (this.autoReady) this.sendPresentation(ws);
  }

  send(ws, frame, event) {
    const receiptId = randomUUID();
    const payload = { ...(typeof frame === 'string' ? JSON.parse(frame) : frame), task9ReceiptId: receiptId };
    const sent = ws.send(JSON.stringify(payload));
    if (sent === 0) throw new Error('TASK9_SOCKET_SEND_REJECTED');
    const receipt = this.receipt(this.targets.get(ws.data.sessionId), {
      socketId: ws.data.socketId, receiptId, raw: JSON.stringify(payload),
    });
    this.rawFrames.push(receipt);
    if (event) this.emit(event, receipt);
    return receipt;
  }

  sendPresentation(ws) {
    const receipt = this.send(ws, grid('task9-current-target'), 'presentation-sent');
    this.proof = { sessionId: receipt.target.sessionId, daemonEpoch: receipt.target.daemonEpoch,
      socketId: receipt.socketId, receiptId: receipt.receiptId };
    return receipt;
  }

  handleWsMessage(ws, message) {
    const raw = typeof message === 'string' ? message : new TextDecoder().decode(message);
    let parsed;
    try { parsed = JSON.parse(raw); } catch { parsed = { type: 'write', data: raw }; }
    if (parsed.type === 'task9Fence') {
      ws.send(JSON.stringify({ type: 'task9FenceAck', task9ReceiptId: parsed.receiptId }));
      return;
    }
    const record = this.receipt(this.targets.get(ws.data.sessionId), {
      requestId: randomUUID(), socketId: ws.data.socketId, rawBody: raw, type: parsed.type,
    });
    this.rawRequests.push(record);
    let kind, payload;
    if (parsed.type === 'write' || parsed.type === 'remoteWrite') {
      kind = parsed.data === '\u0003' ? 'signal' : 'write';
      payload = kind === 'signal' ? 'interrupt' : parsed.data;
    } else if (parsed.type === 'signal') { kind = 'signal'; payload = parsed.signal; }
    if (kind) {
      const delivery = { ...record, kind, payload };
      this.deliveries.push(delivery);
      this.emit('delivery-' + kind, delivery);
    }
  }

  handleWsClose(ws, code, reason) {
    super.handleWsClose(ws, code, reason);
    if (ws.data.type !== 'terminal') return;
    const receipt = this.receipt(this.targets.get(ws.data.sessionId), { socketId: ws.data.socketId, code, reason });
    this.emit('transport-closed', receipt);
    if (this.heldDials.has(ws.data.socketId)) this.emit('dial-cancelled', { ...receipt, attemptId: ws.data.socketId });
  }

  async inject(command, payload) {
    const sockets = [...this.terminalSockets].filter(ws => ws.data.sessionId === payload.target.sessionId);
    switch (command) {
      case 'ready':
        this.operation = null; this.autoReady = true;
        if (sockets.length) sockets.forEach(ws => this.sendPresentation(ws));
        return;
      case 'disconnect':
        this.autoReady = false;
        sockets.forEach(ws => ws.close(1012, 'fixture transport outage'));
        return;
      case 'revoke-token':
        this.authorized = false; this.ticketStore.clear();
        sockets.forEach(ws => ws.close(1008, 'fixture token revoked'));
        return;
      case 'hold-dial':
        this.operation = command; this.autoReady = false;
        // If terminal choice already began the request, re-emit its actual ingress
        // receipt, not a made-up held completion for an already attached socket.
        for (const entry of this.held.values()) this.emit('dial-held', {
          ...entry.record, target: payload.target, attemptId: entry.record.requestId,
        });
        return;
      case 'release-stale-dial':
        for (const entry of this.held.values()) entry.settle(entry.issue());
        for (const ws of this.heldDials.values()) {
          if (ws.readyState === 1) this.sendPresentation(ws);
        }
        // Only the actual product owner can acknowledge rejection after late resolution.
        if (!this.browser) missing('browser binding for recovery observation');
        return this.browser.evaluate(`(() => {
          if (!window.__ferryxRecoveryQA) throw new Error('TASK9_PRODUCT_SEAM_REQUIRED: __ferryxRecoveryQA.waitForStaleCompletion(target)');
          return window.__ferryxRecoveryQA.waitForStaleCompletion(${JSON.stringify({ sessionId: payload.target.sessionId, daemonEpoch: payload.target.daemonEpoch })});
        })()`);
      case 'confirmed-exit':
        if (!this.options.exitFrame) missing('Task8 consumed exit wire frame codec: exitFrame(target, exitCode)');
        sockets.forEach(ws => this.send(ws, this.options.exitFrame(payload.target, 0), 'exit-sent'));
        return;
      case 'overflow-output-ring': {
        const bytes = Buffer.from('x'.repeat(this.capacity + 1) + payload.tail);
        this.outputCursor += bytes.length;
        this.ring = Buffer.concat([this.ring, bytes]).subarray(-this.capacity);
        const oldestAvailableCursor = this.outputCursor - this.ring.length;
        if (!this.options.gapFrame) missing('Task8 grid output gap codec: gapFrame(target, requestedCursor, oldestAvailableCursor)');
        for (const ws of sockets) {
          const receipt = this.send(ws, this.options.gapFrame(payload.target, this.requestedCursor, oldestAvailableCursor));
          this.send(ws, grid(payload.tail));
          this.emit('replay-gap-sent', { ...receipt, requestedCursor: this.requestedCursor,
            oldestAvailableCursor, bytesWritten: bytes.length, capacity: this.capacity });
        }
        return;
      }
      default: throw new Error('TASK9_UNKNOWN_INJECTION: ' + command);
    }
  }

  async currentTarget() {
    const browser = await this.browser.evaluate(`(() => {
      if (!window.__ferryxRecoveryQA) throw new Error('TASK9_PRODUCT_SEAM_REQUIRED: __ferryxRecoveryQA.currentTarget()');
      return window.__ferryxRecoveryQA.currentTarget();
    })()`);
    if (!this.proof || browser.sessionId !== this.proof.sessionId || browser.socketId !== this.proof.socketId)
      throw new Error('TASK9_CURRENT_SOCKET_MISMATCH');
    if (browser.daemonEpoch !== this.proof.daemonEpoch ||
      browser.daemonEpoch !== this.targets.get(browser.sessionId)?.daemonEpoch)
      throw new Error('TASK9_CURRENT_EPOCH_NOT_VALIDATED');
    if (browser.outcome !== 'ready' || !Number.isInteger(browser.generation) || browser.generation <= 0 ||
      !Number.isInteger(browser.presentationSequence) || browser.presentationSequence <= 0)
      throw new Error('TASK9_CURRENT_PRESENTATION_NOT_READY');
    // Browser fields are never filled from fixture process identity. Receipt sequence
    // and browser presentationSequence belong to different counters.
    return { ...browser, fullGridReceipt: { ...this.proof },
      evidenceScope: 'legacy-web-presentation', processSurvivalProven: false };
  }

  async validatedBinding() {
    const proof = await this.currentTarget();
    return this.targets.get(proof.sessionId);
  }

  async fence({ target }) {
    const query = this.options.initial === 'historical-owner-unavailable'
      ? { sessionId: target.sessionId, daemonEpoch: target.daemonEpoch } : target;
    const fenceId = randomUUID();
    const wire = await this.browser.evaluate(`window.__task9Wire.fence(${JSON.stringify(fenceId)})`);
    const owner = await this.browser.evaluate(`(() => {
      if (!window.__ferryxRecoveryQA) throw new Error('TASK9_PRODUCT_SEAM_REQUIRED: __ferryxRecoveryQA.snapshot(target)');
      return window.__ferryxRecoveryQA.snapshot(${JSON.stringify(query)});
    })()`);
    return { complete: true, fenceId, source: 'isolated HTTP/WS ingress + product owner snapshot',
      requests: [...this.rawRequests], deliveries: [...this.deliveries],
      selectionRequests: [...this.selectionRequests], frames: [...this.rawFrames], browser: wire, owner };
  }

  async close() {
    for (const entry of this.held.values()) {
      entry.settle(Response.json({ error: 'FIXTURE_CLOSED' }, { status: 503 }));
    }
    await super.close();
  }
}
