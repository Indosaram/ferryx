import { cpus, platform, arch, release } from "node:os";
import { WebSocketTerminalTransport } from "../../lib/terminalTransport/remoteTransport.ts";

// Bun's HTTP/WebSocket server, fetch, performance clock, and node:os APIs are
// portable on macOS, Windows and Linux; no shell, PTY or OS-specific code is used.
export function summarize(samples) {
  if (samples.length === 0) throw new Error("Cannot summarize an empty measurement");
  const sorted = [...samples].sort((a, b) => a - b);
  const rank = (percentile) => sorted[Math.ceil(percentile * sorted.length) - 1];
  return {
    sampleCount: sorted.length,
    min: sorted[0], p50: rank(0.5), p90: rank(0.9), p95: rank(0.95),
    p99: rank(0.99), max: sorted.at(-1),
  };
}

function bounded(action, timeoutMs, label) {
  let timer;
  return Promise.race([
    Promise.resolve().then(action),
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Timed out: ${label}`)), timeoutMs);
    }),
  ]).finally(() => clearTimeout(timer));
}

function startHost(index) {
  const hostId = `host-${index}`;
  // Deliberately reuse a session ID across hosts to exercise host isolation.
  const sessionId = "soak-session";
  const token = crypto.randomUUID();
  const target = `/api/v1/terminal/${sessionId}`;
  const tickets = new Set();
  const encoder = new TextEncoder();
  const server = Bun.serve({
    hostname: "127.0.0.1", port: 0,
    async fetch(request, server) {
      const url = new URL(request.url);
      if (url.pathname === "/api/v1/socket-ticket" && request.method === "POST") {
        if (request.headers.get("authorization") !== `Bearer ${token}`) {
          return Response.json({ code: "UNAUTHORIZED", message: "Invalid token", details: null }, { status: 401 });
        }
        const body = await request.json();
        if (body.target !== target) {
          return Response.json({ code: "INVALID_TARGET", message: "Unknown target", details: null }, { status: 400 });
        }
        const ticket = crypto.randomUUID();
        tickets.add(ticket);
        return Response.json({ ticket });
      }
      if (url.pathname === target && tickets.delete(url.searchParams.get("ticket"))) {
        if (server.upgrade(request)) return;
      }
      return Response.json({ code: "NOT_FOUND", message: "Unknown route or ticket", details: null }, { status: 404 });
    },
    websocket: {
      open(socket) { socket.send(encoder.encode(`READY:${hostId}`)); },
      message(socket, message) {
        // Not an in-process client mock: binary input traverses a real loopback
        // socket, then an independent host handler returns an application echo.
        const input = typeof message === "string" ? message : new TextDecoder().decode(message);
        socket.send(encoder.encode(`ECHO:${hostId}:${input}`));
      },
    },
  });
  const transport = new WebSocketTerminalTransport(`http://127.0.0.1:${server.port}`, token);
  return { hostId, sessionId, server, transport, samples: [], sequence: 0 };
}

async function expectOutput(host, expected, trigger, timeoutMs) {
  let unsubscribe;
  let received = "";
  const decoder = new TextDecoder();
  try {
    return await bounded(() => new Promise((resolve, reject) => {
      // Register before attach/write; neither WebSocket open nor echo can race
      // the subscription. Timeout is a liveness guard, not a latency gate.
      unsubscribe = host.transport.onOutput(host.sessionId, (data) => {
        received += typeof data === "string" ? data : decoder.decode(data, { stream: true });
        if (received === expected) resolve(performance.now());
        else if (!expected.startsWith(received)) reject(new Error(`Unexpected output for ${host.hostId}: ${received}`));
      });
      Promise.resolve().then(trigger).catch(reject);
    }), timeoutMs, `output from ${host.hostId}`);
  } finally {
    unsubscribe?.();
  }
}

async function sample(host, timeoutMs) {
  const payload = `${host.hostId}:${host.sequence++}:`.padEnd(64, "x");
  let started;
  const ended = await expectOutput(host, `ECHO:${host.hostId}:${payload}`, () => {
    started = performance.now();
    host.transport.write(host.sessionId, payload);
  }, timeoutMs);
  return ended - started;
}

export async function runSoak(options = {}) {
  const config = { hostCount: 4, durationMs: 60_000, warmupSamples: 100, timeoutMs: 5000, ...options };
  for (const key of ["hostCount", "warmupSamples", "timeoutMs"]) {
    if (!Number.isSafeInteger(config[key]) || config[key] < (key === "warmupSamples" ? 0 : 1)) {
      throw new Error(`Invalid ${key}`);
    }
  }
  if (!Number.isFinite(config.durationMs) || config.durationMs < 0) throw new Error("Invalid durationMs");
  const hosts = [];
  try {
    for (let i = 0; i < config.hostCount; i++) hosts.push(startHost(i));
    await Promise.all(hosts.map(async (host) => {
      await expectOutput(host, `READY:${host.hostId}`, () => host.transport.attach(host.sessionId), config.timeoutMs);
      for (let i = 0; i < config.warmupSamples; i++) await sample(host, config.timeoutMs);
    }));
    const startedAt = new Date().toISOString();
    const start = performance.now();
    const deadline = start + config.durationMs;
    // Closed-loop load: one outstanding input per host, all hosts concurrent.
    // Duration is the measurement itself, not a sleep or readiness poll.
    await Promise.all(hosts.map(async (host) => {
      do {
        host.samples.push(await sample(host, config.timeoutMs));
      } while (performance.now() < deadline);
    }));
    const durationMs = performance.now() - start;
    return {
      schemaVersion: 1,
      measurement: "WebSocketTerminalTransport.write-to-application-echo",
      topology: "independent loopback Bun echo gateways; no SSH, PTY, Rust gateway or renderer",
      loadModel: "closed-loop; one outstanding 64-byte input per host; concurrent hosts",
      percentileMethod: "nearest-rank",
      startedAt, requestedDurationMs: config.durationMs, durationMs,
      hostCount: config.hostCount, warmupSamplesPerHost: config.warmupSamples,
      livenessTimeoutMs: config.timeoutMs,
      environment: { platform: platform(), arch: arch(), release: release(), cpu: cpus()[0]?.model, bun: Bun.version },
      latencyMs: summarize(hosts.flatMap((host) => host.samples)),
      hosts: hosts.map((host) => ({ hostId: host.hostId, latencyMs: summarize(host.samples) })),
    };
  } finally {
    for (const host of hosts) {
      await host.transport.close(host.sessionId);
      await host.server.stop(true);
    }
  }
}

if (import.meta.main) {
  try {
    const keys = { "--hosts": "hostCount", "--duration-ms": "durationMs", "--warmup": "warmupSamples", "--timeout-ms": "timeoutMs" };
    const options = {};
    const args = process.argv.slice(2);
    for (let i = 0; i < args.length; i += 2) {
      const key = keys[args[i]];
      if (!key || args[i + 1] === undefined) throw new Error(`Unknown or incomplete argument: ${args[i]}`);
      options[key] = Number(args[i + 1]);
    }
    console.log(JSON.stringify(await runSoak(options), null, 2));
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
}
