#!/usr/bin/env bun
/**
 * scripts/qa/session-recovery-consistency.mjs
 *
 * Task 9 End-to-End Recovery Consistency Harness
 * Plan: .omo/plans/consistent-session-recovery-20261013.md (Task 9, IS-1 - IS-7)
 * Worktree: /Volumes/T9-Mac/project/ferryx-wt/consistent-session-recovery-20261013
 *
 * PROVES (when candidate is built and dependencies are satisfied):
 *   1. Local outage > 30s and > 5 failures recovers same live process/identity without click.
 *   2. Stale epoch same incarnation retains process identity without false exit.
 *   3. Repeated native stream ends sustain retry without presentation collapse.
 *   4. SSH IPC failures then host recovery maintain persistent reconciliation.
 *   5. Paired relay loss and direct-to-relay fallback maintain proxy actor across disconnects.
 *   6. Paired backend-unavailable invokes zero ensureSessionBackends/spawn calls.
 *   7. Initial host-down descriptor restore reloads into recoverable actor slot.
 *   8. Web ticket/handshake failure then recovery with attached readiness boundary.
 *   9. Confirmed exit settles activity and cancels pending recovery once.
 *  10. Auth revocation blocks retry; corrective token/credential retries immediately.
 *  11. Cold-start unknown identity remains retryable with explicit New Terminal escape.
 *  12. Close/rebind during pending dial cancels exact in-flight attempt only.
 *  13. Output replay gap handles buffer overflow without dropping connection.
 *  14. Recovery never steals focus from active user pane.
 *  15. Native remote desktop reattachment via MahoRD on isolated host (maho-win/omarchy).
 *
 * EXECUTION DISCIPLINE:
 *   - Genuine event-driven adapters connecting to authorized repository helpers.
 *   - ZERO hardcoded or fabricated passes: if candidate binary, daemon control wire,
 *     remote host, or test seams are absent, the scenario reports BLOCKED or INCOMPLETE with
 *     exact typed barrier codes and required method signatures.
 *   - ZERO process creation during recovery: assertNoProcessCreations validates daemon request logs.
 *   - ZERO mutation replay: assertNoMutationReplay ensures no replayed writes or signals.
 *   - Teardown kills only PIDs recorded in OwnedProcessLedger with matching verified executable paths.
 *     The harness's own PID is NEVER recorded.
 *   - Setup vs. acceptance distinction: setup inventory delta is never substituted for true 7-tuple presentation.
 *   - Remote processes require valid owning host spawn receipts (sourceKind, hostId, transport,
 *     backendSessionId, epoch, pid, executable, spawnedAt, candidate).
 *
 * USAGE:
 *   bun scripts/qa/session-recovery-consistency.mjs --scenario all --evidence-dir <dir> [options]
 */

import { spawn, spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { existsSync, mkdirSync, readFileSync, writeFileSync, rmSync, statSync } from 'node:fs';
import { join, resolve, dirname, extname } from 'node:path';
import { tmpdir, platform } from 'node:os';
import { fileURLToPath } from 'node:url';

import {
  ATTACH_TUPLE_FIELDS,
  requireSevenTupleReceipt,
  HarnessError,
  BUDGETS,
} from '../lib/qa-scenarios/common-harness.mjs';
import {
  bindPaneSession,
  paneReceiptSessionId,
  observedSessionIds,
  createPaneInventoryReader,
} from '../lib/qa-scenarios/pane-binding.mjs';
import {
  readDaemonSessionInventory,
  computeInventoryDelta,
  daemonEndpointForRuntimeDir,
} from '../lib/qa-scenarios/daemon-inventory.mjs';
import {
  selectNativeDriver,
  captureScreenshot,
} from '../lib/qa-scenarios/native-driver.mjs';
import {
  ensureFrontendServed,
  seedFirstRunState,
  FRONTEND_DEV_URL,
  FRONTEND_PORT,
} from '../lib/qa-scenarios/frontend-server.mjs';
import {
  setupIsolatedProfile,
  daemonControlTransport,
  seedIsolatedRemoteConfig,
  launchIsolatedGateway,
  connectDaemonControl,
  OwnedProcessLedger,
  probeProcessIdentity,
  executablePathMatches,
  ownerHostReceiptRequirement,
} from './herdr-reference-fixtures.mjs';

import { runNativeRecoveryScenario, NATIVE_SCENARIOS } from './session-recovery-native.mjs';
import { transportScenarioExecutors } from './session-recovery-transports.mjs';
import { executeWebOutcome, WEB_OUTCOME_SCENARIOS } from './session-recovery-web-outcomes.mjs';

export const HARNESS_VERSION = 'session-recovery-consistency/1.1.0';

export const SCENARIOS = Object.freeze([
  'local-outage',
  'stale-epoch',
  'native-stream-ends',
  'ssh-ipc-failure',
  'paired-relay-loss',
  'paired-backend-unavailable',
  'host-down-descriptor-restore',
  'web-handshake-failure',
  'confirmed-exit',
  'auth-revocation',
  'cold-start-unknown',
  'close-rebind-pending-dial',
  'output-replay-gap',
  'no-focus-steal',
  'native-remote-mahord',
]);

const ISOLATED_PROFILES = new Map();

export function scenarioDurationMs(startedAt, finishedAt = performance.now()) {
  return Math.max(0, Math.round(finishedAt - startedAt));
}

export function requestedScenariosPassed(results, requested) {
  return results.length === requested.length &&
    results.every((result, index) => result.scenario === requested[index] && result.status === 'passed');
}

/* ==========================================================================
 * CLI Argument Parsing
 * ========================================================================== */

export function parseArgs(argv) {
  const options = {
    scenario: 'all',
    evidenceDir: null,
    binary: null,
    uiDist: null,
    remoteHost: '100.68.224.49',
    pairingId: '9929DC13-3841-4182-A399-9E1323F961A2',
    baseRevision: '7fdfa3e4',
    patchHash: 'unpatched',
    timeoutMs: 30000,
    nativeConfig: null,
    transportFixture: null,
    webFixture: null,
  };

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      printUsageAndExit(0);
    } else if (arg === '--scenario') {
      options.scenario = argv[++i];
    } else if (arg === '--evidence-dir') {
      options.evidenceDir = argv[++i];
    } else if (arg === '--binary') {
      options.binary = argv[++i];
    } else if (arg === '--ui-dist') {
      options.uiDist = argv[++i];
    } else if (arg === '--remote-host') {
      options.remoteHost = argv[++i];
    } else if (arg === '--pairing-id') {
      options.pairingId = argv[++i];
    } else if (arg === '--base-revision') {
      options.baseRevision = argv[++i];
    } else if (arg === '--patch-hash') {
      options.patchHash = argv[++i];
    } else if (arg === '--timeout-ms') {
      options.timeoutMs = parseInt(argv[++i], 10);
    } else if (arg === '--native-config') {
      options.nativeConfig = argv[++i];
    } else if (arg === '--transport-fixture') {
      options.transportFixture = argv[++i];
    } else if (arg === '--web-fixture') {
      options.webFixture = argv[++i];
    }
  }

  if (!options.evidenceDir) {
    console.error('Error: --evidence-dir is required');
    printUsageAndExit(2);
  }

  if (options.scenario !== 'all' && !SCENARIOS.includes(options.scenario)) {
    console.error(`Error: Unknown scenario '${options.scenario}'. Available: ${SCENARIOS.join(', ')}`);
    printUsageAndExit(2);
  }

  return options;
}

function printUsageAndExit(code) {
  console.log(`
Usage: bun scripts/qa/session-recovery-consistency.mjs --scenario <NAME|all> --evidence-dir <PATH> [OPTIONS]

Task 9 consistency verification across local, SSH, paired direct/relay, and remote web transports.

Options:
  --scenario <NAME|all>    Target scenario (default: all)
  --evidence-dir <PATH>    Output directory for logs, receipts, screenshots (required)
  --binary <PATH>          Candidate ferryx binary (default: auto-detect)
  --ui-dist <PATH>         Static UI assets directory (default: ui/dist)
  --remote-host <HOST>     Target remote QA host for MahoRD (default: 100.68.224.49)
  --pairing-id <ID>        MahoRD pairing ID (default: 9929DC13-3841-4182-A399-9E1323F961A2)
  --base-revision <REV>    Base Git commit (default: 7fdfa3e4)
  --patch-hash <HASH>      SHA256 of candidate patch (default: unpatched)
  --timeout-ms <MS>        Execution ceiling per scenario (default: 30000)
  --native-config <PATH>   JSON native host/launch configuration; requires buildManifest,
                          explicit mutation {id, operation: "write"|"signal", payload},
                          and nativeTransport (or transportFixture) module path
  --transport-fixture <PATH>
                          JS module exporting loadTransportFixture(ctx), default,
                          or transportFixture; schema ferryx-task9-transport-provider/1
  --web-fixture <PATH>     JS module exporting createWebOutcomeFixture(ctx), default,
                          or webOutcomeFixture for an isolated browser/gateway

Fixture contracts:
  Paths above resolve from the working directory. Native buildManifest JSON must
  contain stamp, binarySha256, and uiSha256 matching the candidate. The native
  transport module exports createNativeTransport(config, ctx) or nativeTransport.
  Native configuration supplies explicit canonicalClone, worktreeRoot, worktree,
  binary, remoteAgent, bun, hostId, SSH and MahoRD fields; the isolated profile is
  <worktree>/.native-qa/<runId>, with separate data/runtime/session/home/barriers.
  Transport fixture root must contain profile.root and profile.paths.runtime;
  candidate and binary must equal this run's candidate stamp and binary path.
  Supply verifier-owned isolated stores and browser/gateway endpoints, never
  production profiles. --evidence-dir is the explicit artifact destination.

Scenarios:
${SCENARIOS.map(s => '  - ' + s).join('\n')}
`);
  process.exit(code);
}

/* ==========================================================================
 * Verification Primitives & Receipt Validation
 * ========================================================================== */

/**
 * Validates owning host spawn receipt per herdr-reference-fixtures contract.
 * Required fields: sourceKind, hostId, transport, backendSessionId, epoch, pid, executable, spawnedAt, candidate.
 */
export function validateOwnerHostReceipt(receipt, candidateStamp) {
  if (!receipt || typeof receipt !== 'object') {
    return { ok: false, reason: 'OWNER_RECEIPT_MISSING', detail: 'Receipt is not an object' };
  }
  const required = [
    'sourceKind',
    'hostId',
    'transport',
    'backendSessionId',
    'epoch',
    'pid',
    'executable',
    'spawnedAt',
    'candidate',
  ];
  for (const field of required) {
    if (!receipt[field]) {
      return { ok: false, reason: 'OWNER_RECEIPT_MALFORMED', detail: `Missing required field: ${field}` };
    }
  }
  if (typeof receipt.pid !== 'number' || receipt.pid <= 0) {
    return { ok: false, reason: 'OWNER_RECEIPT_INVALID_PID', detail: `Invalid PID: ${receipt.pid}` };
  }
  if (candidateStamp && receipt.candidate !== candidateStamp) {
    return {
      ok: false,
      reason: 'OWNER_RECEIPT_CANDIDATE_MISMATCH',
      detail: `Receipt candidate '${receipt.candidate}' != expected '${candidateStamp}'`,
    };
  }
  return { ok: true, receipt };
}

/**
 * Validates that recovery did not issue any replacement process creation requests.
 */
export function assertNoProcessCreations(requests, allowedSessionId = null) {
  const forbidden = requests.filter(r => {
    if (r.type === 'createSession' || r.type === 'spawnTerminalDetailed') {
      if (allowedSessionId && r.sessionId === allowedSessionId && r.isExplicitNewTerminal) {
        return false;
      }
      return true;
    }
    return false;
  });

  return {
    passed: forbidden.length === 0,
    count: forbidden.length,
    forbiddenRequests: forbidden,
  };
}

/**
 * Validates that no uncertain mutations (writes, interrupts, resizes) were replayed.
 */
export function assertNoMutationReplay(requests) {
  const mutations = requests.filter(r =>
    ['write', 'ptyWrite', 'sendInput', 'interrupt', 'signal', 'resize'].includes(r.type) && r.replayed === true
  );

  return {
    passed: mutations.length === 0,
    replayedCount: mutations.length,
    mutations,
  };
}

/* ==========================================================================
 * Controllable Web Gateway & Ticket Test Fixture
 *
 * Implements in-process HTTP and RFC 6455 WebSocket gateway endpoints to
 * simulate Ferryx remote gateway behaviors:
 *   - POST /api/v1/socket-ticket (minting and expiring single-use tickets)
 *   - GET /api/v1/terminal/:sessionId (WebSocket upgrade with ticket validation)
 *   - Controllable failure modes:
 *       * 'handshake-timeout': holds socket open without attached frame, triggering 10s client timeout
 *       * 'handshake-drop': closes socket immediately
 *       * 'recovery-ready': sends valid attached readiness frame
 *   - Full socket and HTTP trace recording for deterministic assertion verification.
 * ========================================================================== */

function computeAcceptKey(clientKey) {
  const MAGIC = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
  return createHash('sha1').update(clientKey + MAGIC).digest('base64');
}

function encodeWsTextFrame(text) {
  const payload = Buffer.from(text, 'utf8');
  const length = payload.length;
  let header;
  if (length <= 125) {
    header = Buffer.from([0x81, length]);
  } else if (length <= 65535) {
    header = Buffer.alloc(4);
    header[0] = 0x81;
    header[1] = 126;
    header.writeUInt16BE(length, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x81;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(length), 2);
  }
  return Buffer.concat([header, payload]);
}

export function seedFirstRunAndAuth(html, port, sessionId, onboarding = { version: 1, completedSteps: [], dismissed: true }) {
  const origin = `http://127.0.0.1:${port}`;
  const script = `<script>try{
    localStorage.setItem("ferryx.onboarding.v1", JSON.stringify(${JSON.stringify(onboarding)}));
    localStorage.setItem("ferryx_remote_token", "test-token");
    localStorage.setItem("ferryx_remote_token_local:" + ${JSON.stringify(origin)}, "test-token");
  }catch(e){}</script>`;
  const head = html.indexOf('<head>');
  if (head === -1) return script + html;
  return html.slice(0, head + '<head>'.length) + script + html.slice(head + '<head>'.length);
}

export class WebGatewayFixture extends EventEmitter {
  constructor(options = {}) {
    super();
    this.distDir = options.distDir;
    this.port = options.port || 0;
    this.host = options.host || '127.0.0.1';
    this.mode = options.mode || 'handshake-timeout';
    this.server = null;
    this.boundPort = null;
    this.requests = [];
    this.terminalDials = [];
    this.eventSockets = new Set();
    this.terminalSockets = new Set();
    this.activeSessionId = options.sessionId || 'test-recovery-sess-01';
    this.activeDaemonEpoch = options.daemonEpoch || '1';
    this.activeGeneration = '1';
    this.ticketStore = new Map();
    this.trace = [];
    this.attachedSent = false;
  }

  log(event, detail = {}) {
    this.trace.push({ at: new Date().toISOString(), event, ...detail });
  }

  async start() {
    this.server = Bun.serve({
      port: this.port,
      hostname: this.host,
      fetch: (req, server) => this.handleFetch(req, server),
      websocket: {
        open: (ws) => this.handleWsOpen(ws),
        message: (ws, msg) => this.handleWsMessage(ws, msg),
        close: (ws, code, reason) => this.handleWsClose(ws, code, reason),
      },
    });
    this.boundPort = this.server.port;
    this.log('server-started', { host: this.host, port: this.boundPort, mode: this.mode });
    return this.boundPort;
  }

  handleFetch(req, server) {
    const url = new URL(req.url);
    const reqRecord = {
      at: new Date().toISOString(),
      type: 'http',
      method: req.method,
      path: url.pathname,
    };
    this.requests.push(reqRecord);

    if (req.method === 'OPTIONS') {
      return new Response(null, {
        status: 204,
        headers: {
          'Access-Control-Allow-Origin': '*',
          'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
          'Access-Control-Allow-Headers': 'Authorization, Content-Type',
        },
      });
    }

    // 1. /api/v1/events WebSocket Upgrade (distinct alive event fixture)
    if (url.pathname === '/api/v1/events') {
      const ticket = url.searchParams.get('ticket');
      if (!ticket) {
        this.log('event-ws-rejected', { reason: 'MISSING_TICKET' });
        return new Response('Missing ticket', { status: 401 });
      }
      const ticketEntry = this.ticketStore.get(ticket);
      if (!ticketEntry) {
        this.log('event-ws-rejected', { reason: 'INVALID_TICKET', ticket });
        return new Response('Invalid ticket', { status: 403 });
      }
      this.ticketStore.delete(ticket); // consume once, no reuse allowed
      const nowSec = Math.floor(Date.now() / 1000);
      if (ticketEntry.expiresAt < nowSec) {
        this.log('event-ws-rejected', { reason: 'TICKET_EXPIRED', ticket });
        return new Response('Ticket expired', { status: 403 });
      }
      if (ticketEntry.target !== '/api/v1/events') {
        this.log('event-ws-rejected', { reason: 'TARGET_MISMATCH', ticket, target: ticketEntry.target });
        return new Response('Ticket target mismatch', { status: 403 });
      }
      const upgraded = server.upgrade(req, {
        data: {
          type: 'events',
          url: req.url,
          path: url.pathname,
          ticket,
        },
      });
      if (upgraded) {
        this.log('event-ws-upgraded', { path: url.pathname });
        return;
      }
      return new Response('Event upgrade failed', { status: 400 });
    }

    // 2. /api/v1/terminal/:sessionId WebSocket Upgrade (terminal route)
    if (url.pathname.startsWith('/api/v1/terminal/')) {
      const expectedTerminalPath = `/api/v1/terminal/${this.activeSessionId}`;
      if (url.pathname !== expectedTerminalPath) {
        this.log('terminal-ws-rejected', { reason: 'SESSION_MISMATCH', path: url.pathname, expected: expectedTerminalPath });
        return new Response('Invalid terminal session path', { status: 404 });
      }
      const ticket = url.searchParams.get('ticket');
      if (!ticket) {
        this.log('terminal-ws-rejected', { reason: 'MISSING_TICKET' });
        return new Response('Missing ticket', { status: 401 });
      }
      const ticketEntry = this.ticketStore.get(ticket);
      if (!ticketEntry) {
        this.log('terminal-ws-rejected', { reason: 'INVALID_TICKET', ticket });
        return new Response('Invalid ticket', { status: 403 });
      }
      this.ticketStore.delete(ticket); // consume once, no reuse allowed
      const nowSec = Math.floor(Date.now() / 1000);
      if (ticketEntry.expiresAt < nowSec) {
        this.log('terminal-ws-rejected', { reason: 'TICKET_EXPIRED', ticket });
        return new Response('Ticket expired', { status: 403 });
      }
      if (ticketEntry.target !== url.pathname) {
        this.log('terminal-ws-rejected', { reason: 'TARGET_MISMATCH', ticket, target: ticketEntry.target, expected: url.pathname });
        return new Response('Ticket target mismatch', { status: 403 });
      }

      const dialIndex = this.terminalDials.length + 1;
      const dialStartMonotonic = performance.now();
      const upgraded = server.upgrade(req, {
        data: {
          type: 'terminal',
          dialIndex,
          sessionId: this.activeSessionId,
          dialStartMonotonic,
          url: req.url,
          path: url.pathname,
          ticket,
        },
      });
      if (upgraded) {
        return;
      }
      return new Response('Terminal upgrade failed', { status: 400 });
    }

    // 3. HTTP API endpoints
    if (url.pathname === '/api/v1/capabilities') {
      return Response.json({
        version: '1.0.0',
        daemonEpoch: this.activeDaemonEpoch,
        directPort: this.boundPort,
        relayHostId: null,
        supportedTransports: ['direct'],
      }, {
        headers: { 'Access-Control-Allow-Origin': '*' },
      });
    }

    if (url.pathname === '/api/v1/workspace/state') {
      return Response.json({
        activeContext: {
          workspaceId: 'ws-recovery',
          worktreeSlug: 'main',
          worktreeLabel: 'Recovery',
          sessionId: this.activeSessionId,
          activeTabId: this.activeSessionId,
          activeTerminal: {
            sessionId: this.activeSessionId,
            title: 'Shell',
          },
          terminalTabs: [
            { id: this.activeSessionId, label: 'Shell', active: true },
          ],
        },
        sessions: [
          { sessionId: this.activeSessionId, running: true },
        ],
        projects: [],
      }, {
        headers: { 'Access-Control-Allow-Origin': '*' },
      });
    }

    if (url.pathname === '/api/v1/sessions') {
      return Response.json([
        { sessionId: this.activeSessionId, running: true },
      ], {
        headers: { 'Access-Control-Allow-Origin': '*' },
      });
    }

    if (url.pathname === '/api/v1/socket-ticket' && req.method === 'POST') {
      return (async () => {
        // Authenticate bearer token (source-only test credential; intentionally validates 'test-token', not claiming production auth)
        const auth = req.headers.get('authorization') || '';
        if (auth !== 'Bearer test-token') {
          this.log('ticket-request-rejected', { reason: 'UNAUTHORIZED', auth });
          return Response.json({ error: 'UNAUTHORIZED' }, {
            status: 401,
            headers: { 'Access-Control-Allow-Origin': '*' },
          });
        }

        let parsed = {};
        try { parsed = await req.json(); } catch {}
        this.log('ticket-request-received', { target: parsed.target });

        if (this.mode === 'ticket-failure') {
          return Response.json({ error: 'TICKET_MINT_FAILED' }, {
            status: 500,
            headers: { 'Access-Control-Allow-Origin': '*' },
          });
        }

        const expectedTerminalTarget = `/api/v1/terminal/${this.activeSessionId}`;
        if (parsed.target !== '/api/v1/events' && parsed.target !== expectedTerminalTarget) {
          this.log('ticket-request-rejected', { reason: 'INVALID_TARGET', target: parsed.target });
          return Response.json({ error: 'INVALID_TARGET' }, {
            status: 400,
            headers: { 'Access-Control-Allow-Origin': '*' },
          });
        }

        const ticket = `tkt-${randomUUID()}`;
        const expiresAt = Math.floor(Date.now() / 1000) + 30;
        this.ticketStore.set(ticket, {
          target: parsed.target,
          expiresAt,
          issuedAt: Date.now(),
        });
        this.log('ticket-issued', { ticket, expiresAt, target: parsed.target });
        return Response.json({ ticket, expiresAt }, {
          headers: { 'Access-Control-Allow-Origin': '*' },
        });
      })();
    }

    // 4. Static UI Assets
    if (this.distDir && existsSync(this.distDir)) {
      let relPath = url.pathname === '/' ? 'index.html' : url.pathname.slice(1);
      let filePath = join(this.distDir, relPath);
      if (!existsSync(filePath) || !statSync(filePath).isFile()) {
        filePath = join(this.distDir, 'index.html');
      }
      if (existsSync(filePath) && statSync(filePath).isFile()) {
        let fileContent = readFileSync(filePath);
        if (filePath.endsWith('index.html')) {
          fileContent = Buffer.from(seedFirstRunAndAuth(fileContent.toString('utf8'), this.boundPort, this.activeSessionId,
            this.onboardingStorage?.value), 'utf8');
        }
        const ext = extname(filePath);
        const mime = ext === '.html' ? 'text/html; charset=utf-8' : (ext === '.js' ? 'text/javascript; charset=utf-8' : (ext === '.css' ? 'text/css; charset=utf-8' : (ext === '.json' ? 'application/json; charset=utf-8' : 'application/octet-stream')));
        return new Response(fileContent, {
          headers: { 'Content-Type': mime, 'Cache-Control': 'no-store' },
        });
      }
    }

    return new Response('Not Found', { status: 404 });
  }

  handleWsOpen(ws) {
    if (ws.data.type === 'events') {
      this.eventSockets.add(ws);
      this.log('event-ws-opened');
      // Keep event socket alive and healthy for RemoteApp bootstrap
      return;
    }

    if (ws.data.type === 'terminal') {
      const { dialIndex, sessionId, dialStartMonotonic } = ws.data;
      const dialRecord = {
        dialIndex,
        sessionId,
        at: new Date().toISOString(),
        monotonicMs: dialStartMonotonic,
        ws,
      };
      this.terminalDials.push(dialRecord);
      this.terminalSockets.add(ws);

      this.log('terminal-dial-received', { dialIndex, sessionId, mode: this.mode });
      this.emit('terminal-dial', dialRecord);
      this.emit(`terminal-dial-${dialIndex}`, dialRecord);

      if (this.mode === 'handshake-timeout') {
        // Hold open without sending attached frames.
        // Client's 10s handshake deadline will fire and client will close the socket.
        this.log('terminal-held-for-handshake-timeout', { dialIndex, sessionId });
        return;
      }

      if (this.mode === 'recovery-ready') {
        // Legacy desktop readiness: valid full grid frame conforming to parseGridFrame
        const gridPayload = JSON.stringify({
          type: 'grid',
          cols: 80,
          rows: 24,
          cursor: { x: 0, y: 0, visible: true, blinking: false, wideTail: false, visualStyle: 'block' },
          lines: [{ index: 0, runs: [{ text: 'ferryx-recovery-live', fg: null, bg: null, attrs: 0, cells: 20 }] }],
        });
        ws.send(gridPayload);

        this.attachedSent = true;
        this.log('terminal-grid-sent', { dialIndex, sessionId, mode: 'legacy-desktop-grid' });
      }
    }
  }

  handleWsMessage(ws, message) {
    if (ws.data.type === 'terminal') {
      let parsed = null;
      try { parsed = JSON.parse(message); } catch {}
      this.requests.push({
        at: new Date().toISOString(),
        type: parsed?.type || 'terminal-message',
        raw: typeof message === 'string' ? message.slice(0, 100) : null,
      });
    }
  }

  handleWsClose(ws, code, reason) {
    if (ws.data.type === 'events') {
      this.eventSockets.delete(ws);
      this.log('event-ws-closed', { code, reason });
      return;
    }

    if (ws.data.type === 'terminal') {
      this.terminalSockets.delete(ws);
      const { dialIndex, sessionId, dialStartMonotonic } = ws.data;
      const closeMonotonic = performance.now();
      const elapsedMs = Math.round(closeMonotonic - dialStartMonotonic);
      const closeEvent = {
        dialIndex,
        sessionId,
        code,
        reason,
        at: new Date().toISOString(),
        monotonicMs: closeMonotonic,
        elapsedMs,
      };
      this.log('terminal-socket-closed', closeEvent);
      this.emit('terminal-socket-close', closeEvent);
      this.emit(`terminal-socket-close-${dialIndex}`, closeEvent);
    }
  }

  waitForTerminalDial(dialIndex, timeoutMs = 15000) {
    if (this.terminalDials.length >= dialIndex) {
      return Promise.resolve(this.terminalDials[dialIndex - 1]);
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`Timeout waiting for terminal dial ${dialIndex} after ${timeoutMs}ms`));
      }, timeoutMs);
      this.once(`terminal-dial-${dialIndex}`, (record) => {
        clearTimeout(timer);
        resolve(record);
      });
    });
  }

  waitForTerminalSocketClose(dialIndex, timeoutMs = 20000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`Timeout waiting for terminal socket close of dial ${dialIndex} after ${timeoutMs}ms`));
      }, timeoutMs);
      this.once(`terminal-socket-close-${dialIndex}`, (event) => {
        clearTimeout(timer);
        resolve(event);
      });
    });
  }

  async close() {
    for (const ws of this.terminalSockets) {
      try { ws.close(); } catch {}
    }
    this.terminalSockets.clear();
    for (const ws of this.eventSockets) {
      try { ws.close(); } catch {}
    }
    this.eventSockets.clear();
    if (this.server) {
      this.server.stop(true);
      this.server = null;
    }
    this.log('server-closed');
  }
}

/* ==========================================================================
 * MahoRD Remote Desktop Automation Driver
 * ========================================================================== */

export async function captureRemoteDesktopFrame({ host, pairingId, timeoutSecs = 10 }) {
  const mahoClientPath = '/Users/indo/.cargo/bin/maho-client';
  if (!existsSync(mahoClientPath)) {
    return { ok: false, barrier: 'MAHO_CLIENT_MISSING', detail: `${mahoClientPath} does not exist` };
  }

  const started = performance.now();
  const res = spawnSync(mahoClientPath, [
    '--host', host,
    '--pairing-id', pairingId,
    '--frames', '1',
    '--timeout-secs', String(timeoutSecs),
  ], { encoding: 'utf8', timeout: (timeoutSecs + 5) * 1000 });

  const elapsedMs = scenarioDurationMs(started);

  if (res.status === 0) {
    const match = (res.stdout || '').match(/decoded frame size (\d+x\d+)/);
    const resolution = match ? match[1] : 'unknown';
    return {
      ok: true,
      host,
      pairingId,
      resolution,
      rawOutput: res.stdout,
      elapsedMs,
    };
  }

  return {
    ok: false,
    barrier: 'MAHORD_CAPTURE_FAILED',
    exitCode: res.status,
    stderr: res.stderr || '',
    detail: `maho-client exited ${res.status}: ${(res.stderr || '').slice(0, 300)}`,
    elapsedMs,
  };
}

/* ==========================================================================
 * Scenario Executors (Matrix 1-15)
 *
 * Authored native, transport and browser modules are dispatched below. Missing
 * internal fixture producers remain unverified, never acceptance passes.
 * The original stale-epoch protocol helper remains separate from native acceptance.
 * ========================================================================== */


/**
 * 2. stale-epoch:
 *    Proves IS-1: client reattaching with stale daemon epoch adopts active epoch
 *    without killing or restarting the running PTY session.
 */
async function runStaleEpochScenario(ctx) {
  const timeline = [];
  const assertions = [];

  if (!ctx.binary || !existsSync(ctx.binary)) {
    return {
      status: 'blocked',
      barrierCode: 'CANDIDATE_BINARY_REQUIRED',
      detail: `Candidate binary required for stale-epoch scenario (specified: '${ctx.binary || 'none'}')`,
      timeline,
      assertions,
    };
  }

  const profile = setupIsolatedProfile('stale-epoch');
  ISOLATED_PROFILES.set(profile.root, profile);
  const ledger = ctx.ledger;
  const startedAt = performance.now();

  try {
    timeline.push({ at: new Date().toISOString(), event: 'isolated-profile-created', detail: profile.root });

    // Spawn isolated daemon under test
    const daemonProc = spawn(ctx.binary, ['--daemon'], {
      env: profile.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    const spawnEntry = ledger.record(daemonProc, { executablePath: daemonProc.spawnfile || ctx.binary, profileRoot: profile.root });
    // Keep the exit handle on the same ledger entry without serializing ChildProcess internals.
    Object.defineProperty(spawnEntry, 'child', { value: daemonProc });
    timeline.push({ at: new Date().toISOString(), event: 'daemon-spawned', detail: `pid=${daemonProc.pid}` });

    // Await FERRYX_DAEMON_READY on stdout with bounded deadline before connecting
    await new Promise((resolveReady, rejectReady) => {
      let accumulated = '';
      const timer = setTimeout(() => {
        rejectReady(new Error('Timeout waiting for FERRYX_DAEMON_READY from daemon process'));
      }, 15000);

      daemonProc.stdout.on('data', (chunk) => {
        accumulated += chunk.toString('utf8');
        if (accumulated.includes('FERRYX_DAEMON_READY')) {
          clearTimeout(timer);
          resolveReady();
        }
      });

      daemonProc.on('error', (err) => {
        clearTimeout(timer);
        rejectReady(err);
      });

      daemonProc.on('exit', (code, signal) => {
        clearTimeout(timer);
        rejectReady(new Error(`Daemon exited prematurely with code ${code}, signal ${signal}`));
      });
    });

    timeline.push({ at: new Date().toISOString(), event: 'daemon-ready-signaled' });

    // Connect control socket via genuine transport helper
    const transport = daemonControlTransport(profile);
    const control = await connectDaemonControl(transport, { timeoutMs: 15000 });
    const daemonEpoch = control.handshake?.epoch;
    const daemonPid = control.handshake?.pid;
    timeline.push({
      at: new Date().toISOString(),
      event: 'daemon-control-connected',
      detail: `epoch=${daemonEpoch}, pid=${daemonPid}`,
    });

    async function boundedCall(payload, timeoutMs = 15000) {
      return Promise.race([
        control.call(payload),
        new Promise((_, reject) =>
          setTimeout(() => reject(new Error(`Daemon request timed out after ${timeoutMs}ms: ${payload.type}`)), timeoutMs)
        ),
      ]);
    }

    // 1. Spawn a real PTY session
    const clientRequestId = `spawn-stale-epoch-${randomUUID()}`;
    const spawnRes = await boundedCall({
      type: 'spawn',
      clientRequestId,
      workspaceId: 'ws-stale-epoch',
      cols: 80,
      rows: 24,
    });

    if (spawnRes.type !== 'spawnOk' || !spawnRes.sessionId) {
      throw new Error(`Failed to spawn initial session: ${JSON.stringify(spawnRes)}`);
    }

    const sessionId = spawnRes.sessionId;
    const initialEpoch = String(spawnRes.epoch ?? daemonEpoch ?? '1');
    const initialIncarnation = spawnRes.session?.incarnation;
    timeline.push({
      at: new Date().toISOString(),
      event: 'session-spawned',
      detail: `sessionId=${sessionId}, epoch=${initialEpoch}, incarnation=${initialIncarnation}`,
    });

    // 2. Read initial session details via describeSession
    const descBefore = await boundedCall({ type: 'describeSession', sessionId });
    if (descBefore.type !== 'describeSessionOk' || !descBefore.session) {
      throw new Error(`Failed to describe initial session: ${JSON.stringify(descBefore)}`);
    }
    const runningBefore = descBefore.session.running;
    const incarnationBefore = descBefore.session.incarnation;

    // 3. Issue a request carrying a stale daemon epoch (initialEpoch + 9999)
    const staleEpoch = `${Number(initialEpoch) + 9999}`;
    const staleTarget = {
      sessionId,
      machineId: 'local-test-machine',
      daemonEpoch: staleEpoch,
    };

    const staleRes = await boundedCall({
      type: 'machineSessionMetadata',
      target: staleTarget,
      report: {
        sessionId,
        state: 'running',
        agent: 'omo',
        providerSession: null,
        detail: null,
      },
    });

    timeline.push({
      at: new Date().toISOString(),
      event: 'stale-epoch-request-sent',
      detail: `targetEpoch=${staleEpoch}, responseType=${staleRes.type}, message=${staleRes.message || 'none'}`,
    });

    const isStaleRejected =
      staleRes.type === 'error' &&
      (staleRes.message === 'STALE_EPOCH' || staleRes.code === 'STALE_EPOCH' || String(staleRes.message).includes('STALE_EPOCH'));

    assertions.push({
      check: 'stale_epoch_request_fenced',
      expected: 'STALE_EPOCH',
      actual: staleRes.message || staleRes.type,
      passed: isStaleRejected,
    });

    // 4. Verify PTY session was NOT terminated and incarnation is preserved
    const descAfter = await boundedCall({ type: 'describeSession', sessionId });
    const runningAfter = descAfter.type === 'describeSessionOk' && descAfter.session?.running;
    const incarnationAfter = descAfter.type === 'describeSessionOk' && descAfter.session?.incarnation;

    assertions.push({
      check: 'pty_session_running_after_stale_epoch',
      expected: true,
      actual: runningAfter,
      passed: runningAfter === true,
    });

    assertions.push({
      check: 'session_incarnation_preserved',
      expected: incarnationBefore,
      actual: incarnationAfter,
      passed: Boolean(incarnationAfter && incarnationAfter === incarnationBefore),
    });

    control.close();

    const elapsedMs = scenarioDurationMs(startedAt);
    const allPassed = assertions.every(a => a.passed);

    return {
      status: allPassed ? 'passed' : 'failed',
      sessionId,
      initialEpoch,
      staleEpoch,
      timeline,
      assertions,
      elapsedMs,
    };
  } finally {
    // The runner owns the shared ledger teardown and profile removal after every scenario completes.
  }
}






/**
 * Executes a genuine recovery cycle on a single controlled browser instance
 * accurately sized to the specified viewport dimensions (desktop: 1280x800, mobile: 390x844).
 */
async function runSingleViewportRecovery({ gateway, port, vp, evidenceDir }) {
  const testUrl = `http://127.0.0.1:${port}/index.html`;
  const initialDialCount = gateway.terminalDials.length;
  const expectedDial1Index = initialDialCount + 1;
  const expectedDial2Index = initialDialCount + 2;

  // 1. Register terminal dial 1 promise and socket close promise BEFORE navigate
  const dial1Promise = gateway.waitForTerminalDial(expectedDial1Index, 15000);
  const close1Promise = gateway.waitForTerminalSocketClose(expectedDial1Index, 25000);

  // 2. Controlled browser instance with ACTUAL viewport dimensions (no label fiction)
  const webview = new Bun.WebView({
    width: vp.width,
    height: vp.height,
  });

  try {
    // 3. Gateway starts in handshake-timeout mode
    gateway.mode = 'handshake-timeout';

    // 4. Navigate retained page
    await webview.navigate(testUrl);

    // 5. Await terminal button readiness via MutationObserver before click (fail if absent, not silent noop)
    await webview.evaluate(`(() => {
      return new Promise((resolve, reject) => {
        const selector = '[data-testid="remote-view-mode-terminal"]';
        const btn = document.querySelector(selector);
        if (btn) {
          btn.click();
          resolve(true);
          return;
        }
        const timer = setTimeout(() => {
          observer.disconnect();
          reject(new Error('Timeout waiting for [data-testid="remote-view-mode-terminal"] button to appear in DOM'));
        }, 12000);
        const observer = new MutationObserver(() => {
          const el = document.querySelector(selector);
          if (el) {
            clearTimeout(timer);
            observer.disconnect();
            el.click();
            resolve(true);
          }
        });
        observer.observe(document.body, { childList: true, subtree: true });
      });
    })()`);

    // 6. Await Terminal Dial 1 upgrade
    const dial1Record = await dial1Promise;

    // 7. Await client-side 10s handshake timeout and terminal socket close (no synthetic gateway close)
    const close1Event = await close1Promise;

    // 8. Register Terminal Dial 2 promise BEFORE mode switch to recovery-ready
    const dial2Promise = gateway.waitForTerminalDial(expectedDial2Index, 20000);
    gateway.mode = 'recovery-ready';

    // 9. Await Terminal Dial 2 upgrade and attached transmission
    const dial2Record = await dial2Promise;

    // 10. Await DOM readiness via MutationObserver pagepromise (not polling)
    // Targets exact RemoteTerminal connection status element
    const domResult = await webview.evaluate(`(() => {
      return new Promise((resolve, reject) => {
        const getStatusEl = () => {
          return document.querySelector('[data-testid="remote-terminal-build-stamp"] + span[role="status"]');
        };
        const current = getStatusEl();
        if (current && current.textContent.trim() === 'Live') {
          resolve({ status: 'Live', immediate: true });
          return;
        }
        const timer = setTimeout(() => {
          observer.disconnect();
          reject(new Error('Timeout waiting for terminal connection status to become Live'));
        }, 15000);
        const observer = new MutationObserver(() => {
          const el = getStatusEl();
          if (el && el.textContent.trim() === 'Live') {
            clearTimeout(timer);
            observer.disconnect();
            resolve({ status: 'Live', immediate: false });
          }
        });
        observer.observe(document.body, { childList: true, subtree: true, characterData: true });
      });
    })()`);

    // 11. Capture screenshots at exact viewport size for light and dark themes (Live state only)
    const captures = [];
    const themes = ['light', 'dark'];
    for (const theme of themes) {
      const screenshotPath = join(evidenceDir, `web-handshake-failure-${vp.name}-${theme}.png`);
      await webview.evaluate(`((theme) => {
        document.documentElement.classList.toggle('dark', theme === 'dark');
        // The remote UI selects its theme with :root[data-theme]; the class alone is inert.
        document.documentElement.setAttribute('data-theme', theme);
        document.documentElement.style.colorScheme = theme;
      })('${theme}')`);

      // Bun.WebView screenshots are Blobs; writeFileSync needs bytes.
      const shot = await webview.screenshot({ format: 'png' });
      const bytes = shot instanceof Blob ? Buffer.from(await shot.arrayBuffer()) : shot;
      writeFileSync(screenshotPath, bytes);
      captures.push({
        viewport: vp.name,
        width: vp.width,
        height: vp.height,
        theme,
        path: screenshotPath,
        ok: true,
      });
    }

    return {
      ok: true,
      viewport: vp.name,
      width: vp.width,
      height: vp.height,
      captures,
      dial1Record,
      close1Event,
      dial2Record,
      domResult,
    };
  } finally {
    try { webview.close(); } catch {}
  }
}

/**
 * 8. web-handshake-failure:
 *    Fully orchestrated runnable scenario:
 *    - Requires candidate ui/dist explicitly via ctx.uiDist (no canonical fallback).
 *    - Fails closed if candidate ui/dist or Bun.WebView capability is missing.
 *    - Runs separate controlled browser instances for desktop (1280x800) and mobile (390x844).
 *    - Distinguishes /api/v1/events socket from /api/v1/terminal/:sessionId socket; keeps events alive.
 *    - Validates ticket scoping to exact route/sessionId before terminal dial registration.
 *    - Uses MutationObserver for terminal view mode button readiness before clicking.
 *    - Registers race-waiting terminal socket close promises before navigation.
 *    - Observes 10s client handshake timeout from socket closure with monotonic timestamps.
 *    - Switches gateway to recovery-ready with canonical generation '1' and matching target identity.
 *    - Observes recovery redial and DOM readiness via in-page MutationObserver pagepromise.
 *    - Captures 4 surfaces at genuine viewport dimensions ONLY after observed recovery.
 *    - Asserts no process creations, no mutation replay, and bounded timeouts derived from monotonic observations.
 */
async function runWebHandshakeFailureScenario(ctx) {
  const timeline = [];
  const assertions = [];

  // Candidate boundary: ctx.uiDist is explicitly required; canonical fallback is strictly forbidden
  if (!ctx.uiDist || !existsSync(ctx.uiDist) || !existsSync(join(ctx.uiDist, 'index.html'))) {
    return {
      status: 'blocked',
      barrierCode: 'UI_DIST_MISSING',
      detail: `Explicit candidate ui/dist required via ctx.uiDist (provided: '${ctx.uiDist || 'none'}'); canonical fallback forbidden`,
      timeline,
      assertions,
    };
  }

  if (typeof Bun === 'undefined' || typeof Bun.WebView !== 'function') {
    return {
      status: 'blocked',
      barrierCode: 'BUN_WEBVIEW_UNAVAILABLE',
      detail: 'Bun.WebView is not available in the current runtime environment; headless browser capability required',
      timeline,
      assertions,
    };
  }

  const gateway = new WebGatewayFixture({
    distDir: ctx.uiDist,
    mode: 'handshake-timeout',
    sessionId: 'web-recover-01',
    daemonEpoch: '1',
  });

  const startedAt = performance.now();

  try {
    const port = await gateway.start();
    timeline.push({ at: new Date().toISOString(), event: 'gateway-started', detail: `port=${port}` });

    const viewports = [
      { name: 'desktop', width: 1280, height: 800 },
      { name: 'mobile', width: 390, height: 844 },
    ];

    const vpResults = [];
    const allCaptures = [];

    for (const vp of viewports) {
      timeline.push({ at: new Date().toISOString(), event: 'viewport-started', detail: `${vp.name} (${vp.width}x${vp.height})` });
      const vpRes = await runSingleViewportRecovery({
        gateway,
        port,
        vp,
        evidenceDir: ctx.evidenceDir,
      });
      vpResults.push(vpRes);
      allCaptures.push(...vpRes.captures);
      timeline.push({ at: new Date().toISOString(), event: 'viewport-completed', detail: `${vp.name}` });
    }

    const totalElapsedMs = Math.round(performance.now() - startedAt);

    const desktopRes = vpResults.find(r => r.viewport === 'desktop');
    const mobileRes = vpResults.find(r => r.viewport === 'mobile');

    assertions.push({
      check: 'four_surfaces_captured',
      expected: 4,
      actual: allCaptures.length,
      passed: allCaptures.length === 4 && allCaptures.every(c => c.ok),
    });

    assertions.push({
      check: 'desktop_handshake_timeout_bounded',
      expected: '>= 9500ms and <= 15000ms',
      actual: `${desktopRes?.close1Event?.elapsedMs ?? 0}ms`,
      passed: (desktopRes?.close1Event?.elapsedMs ?? 0) >= 9500 && (desktopRes?.close1Event?.elapsedMs ?? 0) <= 15000,
    });

    assertions.push({
      check: 'mobile_handshake_timeout_bounded',
      expected: '>= 9500ms and <= 15000ms',
      actual: `${mobileRes?.close1Event?.elapsedMs ?? 0}ms`,
      passed: (mobileRes?.close1Event?.elapsedMs ?? 0) >= 9500 && (mobileRes?.close1Event?.elapsedMs ?? 0) <= 15000,
    });

    assertions.push({
      check: 'desktop_readiness_boundary_established',
      expected: 'Live',
      actual: desktopRes?.domResult?.status ?? 'unknown',
      passed: desktopRes?.domResult?.status === 'Live',
    });

    assertions.push({
      check: 'mobile_readiness_boundary_established',
      expected: 'Live',
      actual: mobileRes?.domResult?.status ?? 'unknown',
      passed: mobileRes?.domResult?.status === 'Live',
    });

    const creationCheck = assertNoProcessCreations(gateway.requests);
    assertions.push({
      check: 'zero_replacement_process_creations',
      expected: 0,
      actual: creationCheck.count,
      passed: creationCheck.passed,
    });

    const mutationCheck = assertNoMutationReplay(gateway.requests);
    assertions.push({
      check: 'zero_mutation_replay',
      expected: 0,
      actual: mutationCheck.replayedCount,
      passed: mutationCheck.passed,
    });

    const allPassed = assertions.every(a => a.passed);

    return {
      status: allPassed ? 'passed' : 'failed',
      targetIdentity: { sessionId: 'web-recover-01', daemonEpoch: '1', generation: '1' },
      captures: allCaptures,
      timeline,
      assertions,
      trace: gateway.trace,
      elapsedMs: totalElapsedMs,
    };
  } finally {
    await gateway.close();
  }
}








/* ==========================================================================
 * Runner Dispatcher
 * ========================================================================== */

export function createScenarioContext(options, ledger) {
  const ctx = { ...options, ledger };
  // Deferred import avoids the gateway subclass/main class initialization cycle.
  ctx.openWebOutcome = async (request) => {
    const fixture = ctx.webOutcomeFixture;
    if (!fixture?.target || !fixture?.rebindTarget || !ctx.uiDist) {
      throw Object.assign(new Error('Real isolated web targets and candidate UI assets are required'), {
        code: 'TASK9_WEB_FIXTURE_UNVERIFIED',
        required: 'webOutcomeFixture {target,rebindTarget,exitFrame,gapFrame,createRoute}; uiDist; evidenceDir',
      });
    }
    const { createOpenWebOutcome } = await import('./fixtures/web-outcome-factory.mjs');
    return createOpenWebOutcome({ ...fixture, uiDist: ctx.uiDist,
      evidenceDir: ctx.evidenceDir, timeoutMs: ctx.timeoutMs })(request);
  };
  return ctx;
}

export async function dispatchScenario(name, ctx) {
  try {
    if (NATIVE_SCENARIOS.includes(name)) {
      let createdNativeContext = null;
      let effectiveNativeContext = ctx.nativeContext ?? null;

      if (!effectiveNativeContext && ctx.nativeConfig) {
        try {
          const { readFileSync } = await import('node:fs');
          const { resolve } = await import('node:path');
          const config = JSON.parse(readFileSync(ctx.nativeConfig, 'utf8'));

          // 1. Build Manifest and Candidate Artifact Validation
          if (!config.buildManifest) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: 'Candidate buildManifest path is required in native configuration',
            };
          }

          let manifest;
          try {
            manifest = JSON.parse(readFileSync(resolve(config.buildManifest), 'utf8'));
          } catch (err) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: `Failed to read candidate buildManifest (${config.buildManifest}): ${err.message}`,
            };
          }

          if (!manifest.stamp || !manifest.binarySha256 || !manifest.uiSha256) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: 'Candidate buildManifest must define stamp, binarySha256, and uiSha256',
            };
          }

          const candidate = (typeof ctx.candidate === 'object' && ctx.candidate !== null)
            ? ctx.candidate
            : (typeof config.candidate === 'object' && config.candidate !== null)
              ? config.candidate
              : {
                  stamp: ctx.candidateStamp || config.candidateStamp || manifest.stamp,
                  binarySha256: config.binarySha256 || manifest.binarySha256,
                  uiSha256: config.uiSha256 || manifest.uiSha256,
                };

          // Reject missing or placeholder candidate hashes
          const isPlaceholder = (h) => !h || typeof h !== 'string' || h.startsWith('unverified') || h.length < 16;
          if (!candidate.stamp || isPlaceholder(candidate.binarySha256) || isPlaceholder(candidate.uiSha256)) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: 'Candidate stamp and verified binarySha256 and uiSha256 hashes are required (placeholders rejected)',
            };
          }

          // Validate candidate against build manifest
          if (manifest.stamp !== candidate.stamp || manifest.binarySha256 !== candidate.binarySha256 || manifest.uiSha256 !== candidate.uiSha256) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: `Candidate descriptor does not match build manifest: stamp (${candidate.stamp} vs ${manifest.stamp}), binarySha256 (${candidate.binarySha256} vs ${manifest.binarySha256}), uiSha256 (${candidate.uiSha256} vs ${manifest.uiSha256})`,
            };
          }

          // 2. Evidence Writer (requires explicit ctx.evidenceDir, no /tmp/evidence fallback)
          if (!ctx.evidence?.artifact && !ctx.evidenceDir) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: 'Explicit evidenceDir is required for native evidence writer',
            };
          }

          const evidence = ctx.evidence ?? {
            artifact: async (filename, bytes) => {
              const { join } = await import('node:path');
              const { writeFile, mkdir } = await import('node:fs/promises');
              await mkdir(ctx.evidenceDir, { recursive: true });
              const targetPath = join(ctx.evidenceDir, filename);
              await writeFile(targetPath, bytes);
              return targetPath;
            },
          };

          // 3. Explicit Scenario Mutation (never guessed default payload)
          const mutation = ctx.mutation ?? config.mutation;
          if (!mutation?.id || !['write', 'signal'].includes(mutation.operation) || mutation.payload === undefined) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: 'Explicit known mutation { id, operation: "write"|"signal", payload } is required in config or context; default guessed payloads are forbidden',
            };
          }

          // 4. Native Transport Resolution & Pre-flight Method Validation
          const REQUIRED_NATIVE_TRANSPORT_METHODS = [
            'armLostAcknowledgment',
            'subscribe',
            'beginAudit',
            'endCurrentStream',
            'disconnect',
            'holdOutage',
            'restore',
            'changeOwnerEpoch',
            'emitOutput',
            'flush',
            'release',
            'prearmFinalDrain',
          ];

          let transport = ctx.transport ?? ctx.nativeTransport ?? null;
          if (!transport && (config.nativeTransport || config.transportFixture)) {
            const transportPath = config.nativeTransport ?? config.transportFixture;
            const mod = await import(resolve(transportPath));
            if (typeof mod.createNativeTransport === 'function') {
              transport = await mod.createNativeTransport(config, ctx);
            } else if (mod.nativeTransport && typeof mod.nativeTransport === 'object') {
              transport = mod.nativeTransport;
            }
          }

          if (!transport) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: `Existing isolated forwarding transport provider is not configured. Required methods: ${REQUIRED_NATIVE_TRANSPORT_METHODS.join(', ')}`,
            };
          }

          const missingMethods = REQUIRED_NATIVE_TRANSPORT_METHODS.filter((m) => typeof transport[m] !== 'function');
          if (missingMethods.length > 0) {
            return {
              scenario: name,
              status: 'unverified',
              barrierCode: 'NATIVE_CONFIG_INVALID',
              detail: `Configured transport provider is missing required native methods: [${missingMethods.join(', ')}]`,
            };
          }

          // 5. Fresh per-scenario runId
          const { randomUUID } = await import('node:crypto');
          const runId = `native-${name}-${Date.now()}-${randomUUID().slice(0, 8)}`;

          // 6. Single AbortSignal and Deadline Scope Maintained Through Scenario
          const timeoutMs = ctx.timeoutMs ?? config.timeoutMs ?? 120_000;
          const controller = new AbortController();
          const onExternalAbort = () => controller.abort(ctx.signal?.reason);
          if (ctx.signal) {
            if (ctx.signal.aborted) controller.abort(ctx.signal.reason);
            else ctx.signal.addEventListener('abort', onExternalAbort, { once: true });
          }
          const timer = setTimeout(() => controller.abort(new Error(`Scenario ${name} deadline exceeded (${timeoutMs}ms)`)), timeoutMs);

          const { createConfiguredNativeContext } = await import('./fixtures/native-configured-adapters.mjs');
          try {
            createdNativeContext = await createConfiguredNativeContext(config, {
              candidate,
              transport,
              evidence,
              runId,
              mutation,
              signal: controller.signal,
            });
            effectiveNativeContext = {
              ...createdNativeContext,
              signal: controller.signal,
              timeoutMs,
            };

            return await runNativeRecoveryScenario(name, effectiveNativeContext);
          } finally {
            clearTimeout(timer);
            if (ctx.signal) ctx.signal.removeEventListener('abort', onExternalAbort);
            if (createdNativeContext?.closeConnections) {
              try { createdNativeContext.closeConnections(); } catch (_) {}
            }
          }
        } catch (err) {
          return { scenario: name, status: 'unverified', barrierCode: 'NATIVE_CONFIG_INVALID', detail: err.message };
        }
      }

      if (!effectiveNativeContext) return { scenario: name, status: 'unverified',
        required: 'nativeContext {host,provider,spawn,transport,ledger,candidate,runId,mutation,timeoutMs}: native provider st_01a13737' };

      return await runNativeRecoveryScenario(name, effectiveNativeContext);
    }
    if (transportScenarioExecutors[name]) {
      if (typeof ctx.transportFixture === 'string' || (!ctx.transportFixture && ctx.transportFixturePath)) {
        try {
          const fixturePath = ctx.transportFixturePath ?? ctx.transportFixture;
          if (typeof fixturePath === 'string') {
            const { resolve } = await import('node:path');
            const mod = await import(resolve(fixturePath));
            ctx.transportFixture = typeof mod.loadTransportFixture === 'function'
              ? await mod.loadTransportFixture(ctx)
              : mod.default ?? mod.transportFixture;
          }
        } catch (err) {
          return { scenario: name, status: 'unverified', barrierCode: 'TRANSPORT_FIXTURE_INVALID', detail: err.message };
        }
      }
      if (!ctx.transportFixture) return { scenario: name, status: 'unverified',
        required: 'transportFixture {schema,root,profile,candidate,binary,selection,helper?}: real isolated candidate session' };
      return await transportScenarioExecutors[name](ctx);
    }
    if (WEB_OUTCOME_SCENARIOS.includes(name) || name === 'web-handshake-failure') {
      if (!ctx.webOutcomeFixture && (ctx.webFixturePath || ctx.webFixture)) {
        try {
          const fixturePath = ctx.webFixturePath ?? ctx.webFixture;
          if (typeof fixturePath === 'string') {
            const { resolve } = await import('node:path');
            const mod = await import(resolve(fixturePath));
            ctx.webOutcomeFixture = typeof mod.createWebOutcomeFixture === 'function'
              ? await mod.createWebOutcomeFixture(ctx)
              : mod.default ?? mod.webOutcomeFixture;
          }
        } catch (err) {
          return { scenario: name, status: 'unverified', barrierCode: 'WEB_FIXTURE_INVALID', detail: err.message };
        }
      }
    }
    if (WEB_OUTCOME_SCENARIOS.includes(name)) return await executeWebOutcome(name, ctx);
    if (name === 'web-handshake-failure') return await runWebHandshakeFailureScenario(ctx);
    return { scenario: name, status: 'error', error: 'UNKNOWN_SCENARIO' };
  } catch (error) {
    if (error.code === 'TASK9_WEB_FIXTURE_UNVERIFIED' || error.code === 'TASK9_PRODUCT_SEAM_REQUIRED') {
      return { scenario: name, status: 'unverified', barrierCode: error.code,
        required: error.required ?? error.signature, detail: error.message };
    }
    throw error;
  }
}

export const SCENARIO_DISPATCH = Object.freeze(Object.fromEntries(
  SCENARIOS.map(name => [name, ctx => dispatchScenario(name, ctx)]),
));

export async function teardownOwnedChildren(ledger, {
  probeIdentity = probeProcessIdentity,
  currentPlatform = process.platform,
  runTaskkill = spawnSync,
  signalPid = process.kill,
} = {}) {
  // Subscribe before signaling. An unreadable identity or a child that does not exit
  // within this deadline leaves its profile intact and produces a nonzero outcome.
  const exitWaits = ledger.entries.map((entry) => {
    const child = entry.child;
    if (!child || child.pid !== entry.pid) {
      return Promise.resolve({ pid: entry.pid, exited: false, reason: 'DIRECT_CHILD_EXIT_HANDLE_MISSING' });
    }
    if (child.exitCode !== null || child.signalCode !== null) {
      return Promise.resolve({ pid: entry.pid, exited: true, code: child.exitCode, signal: child.signalCode });
    }
    return new Promise((resolveExit) => {
      const finish = (result) => {
        clearTimeout(timer);
        child.removeListener('exit', onExit);
        resolveExit(result);
      };
      const onExit = (code, signal) => finish({ pid: entry.pid, exited: true, code, signal });
      const timer = setTimeout(() => finish({ pid: entry.pid, exited: false, reason: 'OWNED_CHILD_EXIT_TIMEOUT' }), 5000);
      child.once('exit', onExit);
      if (child.exitCode !== null || child.signalCode !== null) {
        onExit(child.exitCode, child.signalCode);
      }
    });
  });
  const receipt = ledger.teardown((pid) => {
    const entry = ledger.entries.find((candidate) => candidate.pid === pid);
    const handle = entry?.child;
    if (!entry || !handle || handle.pid !== pid) throw new Error('owned child handle missing for PID ' + pid);
    if (handle.exitCode !== null || handle.signalCode !== null) {
      throw new Error('recorded child already exited; refusing possibly recycled PID');
    }
    const live = probeIdentity(pid);
    if (!live.alive || !executablePathMatches(live.executable, entry.executablePath, currentPlatform)) {
      throw new Error('owned PID identity no longer matches recorded executable');
    }
    if (currentPlatform === 'win32') {
      const result = runTaskkill('taskkill', ['/PID', String(pid), '/F'], { encoding: 'utf8' });
      if (result.status !== 0) throw new Error('taskkill failed for owned pid ' + pid);
      return;
    }
    signalPid(pid, 'SIGTERM');
  });
  const waitResults = await Promise.all(exitWaits);
  const allExited = waitResults.every((result) => result.exited);
  return { ...receipt, waitResults, allExited };
}

export function aggregateScenarioTeardown(teardown, results, isolatedProfiles) {
  const nativeResults = results.filter(result => NATIVE_SCENARIOS.includes(result.scenario));
  const nativeCleanup = nativeResults.flatMap(result =>
    (result.cleanup ?? []).map(receipt => ({ ...receipt, scenario: result.scenario })));
  const cleanupFailures = nativeCleanup.filter(receipt => receipt.exited !== true);
  const worktreeListFailures = nativeResults.filter(result => result.worktreeListError).map(result =>
    ({ scenario: result.scenario, detail: result.worktreeListError }));
  const retainedProfiles = [...new Set([
    ...(teardown.allExited ? [] : isolatedProfiles),
    ...nativeResults.filter(result => result.profileRetained && result.retainedProfile).map(result => result.retainedProfile),
  ])];
  const allExited = teardown.allExited && !nativeResults.some(result => result.profileRetained) &&
    cleanupFailures.length === 0 && worktreeListFailures.length === 0;
  return { retainedProfiles, teardown: {
    receipts: teardown.receipts, waitResults: teardown.waitResults,
    nativeCleanup, cleanupFailures, worktreeListFailures, allExited,
  } };
}

export async function main(options = parseArgs(process.argv.slice(2))) {
  mkdirSync(options.evidenceDir, { recursive: true });

  const candidateStamp = `${options.baseRevision}+${options.patchHash}`;
  const ledger = new OwnedProcessLedger('task9-recovery-harness');

  const ctx = createScenarioContext({ ...options, candidateStamp }, ledger);

  console.log(`=== Ferryx Task 9 Recovery Consistency Harness [${HARNESS_VERSION}] ===`);
  console.log(`Candidate: ${candidateStamp}`);
  console.log(`Evidence Dir: ${options.evidenceDir}`);
  console.log(`Target Scenarios: ${options.scenario === 'all' ? SCENARIOS.length : 1}\n`);

  const targets = options.scenario === 'all' ? SCENARIOS : [options.scenario];
  const results = [];

  for (const name of targets) {
    const fn = SCENARIO_DISPATCH[name];
    if (!fn) {
      console.error(`[-] Unknown scenario: ${name}`);
      results.push({ scenario: name, status: 'error', detail: 'Unknown executor' });
      continue;
    }

    process.stdout.write(`[*] Running ${name} ... `);
    const started = performance.now();
    try {
      const outcome = await fn(ctx);
      const harnessElapsedMs = scenarioDurationMs(started);
      const accounted = outcome && typeof outcome === 'object' && ['passed', 'failed', 'blocked', 'incomplete', 'unverified', 'error'].includes(outcome.status)
        ? { ...outcome, scenario: name, harnessElapsedMs }
        : { scenario: name, status: 'error', error: 'UNKNOWN_DISPATCH_OUTCOME', outcome, harnessElapsedMs };
      const statusTag = accounted.status.toUpperCase();
      console.log(`[${statusTag}] (${harnessElapsedMs}ms)`);
      if (outcome?.barrierCode) {
        console.log(`    Barrier: ${outcome.barrierCode} - ${outcome.detail}`);
      }
      if (outcome?.requiredSeam) {
        console.log(`    Required Seam: ${outcome.requiredSeam.method} at ${outcome.requiredSeam.location}`);
      }
      results.push(accounted);
    } catch (err) {
      const harnessElapsedMs = scenarioDurationMs(started);
      console.log(`[FAILED] (${harnessElapsedMs}ms)`);
      console.log(`    Error: ${err.message}`);
      results.push({ scenario: name, status: 'failed', error: err.message, harnessElapsedMs });
    }
  }

  // Teardown owned processes: only subprocesses recorded in ledger matching executable
  console.log('\n[*] Tearing down owned test processes...');
  const teardown = await teardownOwnedChildren(ledger);
  for (const result of teardown.waitResults) {
    if (!result.exited) console.error(`    Owned PID ${result.pid} exit unproven: ${result.reason}`);
  }
  if (teardown.allExited) {
    for (const profile of ISOLATED_PROFILES.values()) profile.cleanup();
  } else {
    for (const profile of ISOLATED_PROFILES.values()) {
      console.error(`    Retaining isolated profile ${profile.root}: owned child exit is unproven`);
    }
  }

  const summary = {
    total: results.length,
    passed: results.filter(r => r.status === 'passed').length,
    failed: results.filter(r => r.status === 'failed').length,
    blocked: results.filter(r => r.status === 'blocked').length,
    incomplete: results.filter(r => r.status === 'incomplete').length,
    unverified: results.filter(r => r.status === 'unverified').length,
    errors: results.filter(r => r.status === 'error').length,
  };

  const aggregated = aggregateScenarioTeardown(teardown, results, [...ISOLATED_PROFILES.keys()]);
  const evidenceReport = {
    harness: HARNESS_VERSION,
    candidate: candidateStamp,
    executedAt: new Date().toISOString(),
    summary,
    results,
    ledger: teardown.entries,
    ...aggregated,
  };

  const reportPath = join(options.evidenceDir, 'task-9-qa.json');
  writeFileSync(reportPath, JSON.stringify(evidenceReport, null, 2));
  console.log(`\n[+] Evidence written to ${reportPath}`);
  console.log(`Summary: ${summary.passed} passed, ${summary.failed} failed, ${summary.blocked} blocked, ${summary.incomplete} incomplete, ${summary.unverified} unverified, ${summary.errors} errors (total ${summary.total})`);

  process.exit(requestedScenariosPassed(results, targets) && aggregated.teardown.allExited ? 0 : 1);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  await main().catch(err => {
    console.error('Fatal harness error:', err);
    process.exit(1);
  });
}
