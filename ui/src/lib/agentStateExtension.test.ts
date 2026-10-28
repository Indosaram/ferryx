// @vitest-environment node
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { transformSync } from "esbuild";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";

const EXTENSION_PATH = path.resolve(
  __dirname,
  "../../../src-tauri/resources/agent-extensions/ferryx-agent-state.ts",
);

type Captured = { state: string; detail?: string; agent?: string };

const captured: Captured[] = [];
let moduleCounter = 0;
const moduleDir = mkdtempSync(path.join(os.tmpdir(), "ferryx-ext-test-"));
// Own the endpoint instead of unlinking a shared path used by another test process.
const SOCKET_PATH = process.platform === "win32"
  ? `\\\\.\\pipe\\${path.basename(moduleDir)}`
  : path.join(moduleDir, "state.sock");
const savedEnvironment = Object.fromEntries([
  "FERRYX_AGENT_STATE_SOCKET", "FERRYX_SESSION_ID", "FERRYX_AGENT_STATE_PORT", "FERRYX_AGENT_STATE_TOKEN",
].map((key) => [key, process.env[key]]));
let server: net.Server;

// Re-checked whenever the socket server records a payload, so waits resolve on the data event
// itself; the timeout only bounds a failing test.
const captureListeners = new Set<() => void>();

function recordCapture(entry: Captured): void {
  captured.push(entry);
  for (const listener of [...captureListeners]) listener();
}

function waitFor(predicate: () => boolean, timeoutMs = 3000): Promise<void> {
  if (predicate()) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const check = () => {
      if (!predicate()) return;
      clearTimeout(timer);
      captureListeners.delete(check);
      resolve();
    };
    const timer = setTimeout(() => {
      captureListeners.delete(check);
      reject(new Error("timed out waiting for the extension to publish"));
    }, timeoutMs);
    captureListeners.add(check);
  });
}

// Mirrors the daemon's per-agent rewrite (`rewrite_identity` in daemon/agent_extension.rs).
async function loadExtension(agent = "omo") {
  process.env.FERRYX_AGENT_STATE_SOCKET = SOCKET_PATH;
  process.env.FERRYX_SESSION_ID = "session-ext-test";
  delete process.env.FERRYX_AGENT_STATE_PORT;
  delete process.env.FERRYX_AGENT_STATE_TOKEN;

  const source = readFileSync(EXTENSION_PATH, "utf8")
    .replace("// FERRYX_INTEGRATION_ID=omo", `// FERRYX_INTEGRATION_ID=${agent}`)
    .replace('const AGENT_ID = "omo";', `const AGENT_ID = "${agent}";`);
  const compiled = transformSync(source, { loader: "ts", format: "esm", target: "es2022" }).code;
  moduleCounter += 1;
  // Bun's resolver rejects long data: URLs (NameTooLong), so each load gets its own file.
  const modulePath = path.join(moduleDir, `ferryx-agent-state-${moduleCounter}.mjs`);
  writeFileSync(modulePath, compiled, "utf8");
  const mod = await import(pathToFileURL(modulePath).href);
  return mod.default as (pi: unknown) => void;
}

function makeHarness() {
  const handlers = new Map<string, (event: unknown, ctx?: unknown) => void>();
  const busHandlers = new Map<string, (data: unknown) => void>();

  const pi = {
    on(event: string, handler: (event: unknown, ctx?: unknown) => void) {
      handlers.set(event, handler);
    },
    events: {
      on(channel: string, handler: (data: unknown) => void) {
        busHandlers.set(channel, handler);
        return () => {};
      },
    },
  };

  return {
    pi,
    busHandlers,
    tuiCtx: { mode: "tui", isIdle: () => false },
    emit(event: string, ctx?: unknown) {
      handlers.get(event)?.({}, ctx);
    },
    bus(channel: string, data: unknown) {
      busHandlers.get(channel)?.(data);
    },
  };
}

describe("ferryx-agent-state extension", () => {
  beforeAll(async () => {
    server = net.createServer((socket) => {
      let buffer = "";
      socket.on("data", (chunk) => {
        buffer += chunk.toString("utf8");
      });
      socket.on("close", () => {
        for (const line of buffer.split("\n")) {
          if (line.trim().length === 0) continue;
          const payload = JSON.parse(line) as { type?: string; state?: string; detail?: string; agent?: string };
          if (payload.type === "agentState" && typeof payload.state === "string") {
            recordCapture({ state: payload.state, detail: payload.detail, agent: payload.agent });
          }
        }
      });
    });
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(SOCKET_PATH, () => {
        server.off("error", reject);
        resolve();
      });
    });
  });

  afterAll(async () => {
    if (server?.listening) {
      await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
    }
    rmSync(moduleDir, { recursive: true, force: true });
    for (const [key, value] of Object.entries(savedEnvironment)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });

  beforeEach(() => {
    captured.length = 0;
  });

  it("subscribes to the events senpi actually emits", async () => {
    const load = await loadExtension();
    const h = makeHarness();
    load(h.pi);

    expect(h.busHandlers.has("herdr:blocked")).toBe(true);
    expect(h.busHandlers.has("ask-user:asked")).toBe(true);
    expect(h.busHandlers.has("ferryx:blocked")).toBe(false);
  });

  it("a waiting question publishes blocked with its text, and answering publishes working", async () => {
    const load = await loadExtension();
    const h = makeHarness();
    load(h.pi);

    h.emit("session_start", h.tuiCtx);
    h.emit("agent_start", h.tuiCtx);
    await waitFor(() => captured.length >= 1);

    h.bus("ask-user:asked", {
      request: {
        requestId: "q1",
        waitForAnswer: true,
        questions: [{ header: "Auth method", question: "Which library should we use?" }],
      },
    });

    await waitFor(() => captured.at(-1)?.state === "blocked");
    expect(captured.at(-1)?.detail).toBe("Auth method — Which library should we use?");

    h.bus("herdr:blocked", { active: false, id: "q1" });

    await waitFor(() => captured.at(-1)?.state === "working");
    expect(captured.at(-1)?.detail).toBeUndefined();
  });

  it("a question the agent keeps working through never publishes blocked", async () => {
    const load = await loadExtension();
    const h = makeHarness();
    load(h.pi);

    h.emit("session_start", h.tuiCtx);
    h.emit("agent_start", h.tuiCtx);
    await waitFor(() => captured.length >= 1);
    const baseline = captured.length;

    h.bus("ask-user:asked", {
      request: {
        requestId: "q2",
        waitForAnswer: false,
        questions: [{ header: "Style", question: "Which style do you prefer?" }],
      },
    });
    h.bus("herdr:blocked", { active: false, id: "q2" });
    h.emit("agent_settled", { mode: "tui", isIdle: () => true });

    await waitFor(() => captured.length > baseline);
    expect(captured.slice(baseline).some((entry) => entry.state === "blocked")).toBe(false);
    expect(captured.at(-1)?.state).toBe("idle");
  });

  it("a host dialog with no question event still blocks and carries its label", async () => {
    const load = await loadExtension();
    const h = makeHarness();
    load(h.pi);

    h.emit("session_start", h.tuiCtx);
    h.emit("agent_start", h.tuiCtx);
    await waitFor(() => captured.length >= 1);

    h.bus("herdr:blocked", { active: true, id: "host-1", label: "Pick a workspace" });

    await waitFor(() => captured.at(-1)?.state === "blocked");
    expect(captured.at(-1)?.detail).toBe("Pick a workspace");
  });

  it("settling the turn clears the blocked detail", async () => {
    const load = await loadExtension();
    const h = makeHarness();
    load(h.pi);

    h.emit("session_start", h.tuiCtx);
    h.bus("ask-user:asked", {
      request: { requestId: "q3", waitForAnswer: true, questions: [{ header: "H", question: "Q?" }] },
    });
    await waitFor(() => captured.at(-1)?.state === "blocked");

    h.bus("herdr:blocked", { active: false, id: "q3" });
    h.emit("agent_settled", { mode: "tui", isIdle: () => true });

    await waitFor(() => captured.at(-1)?.state === "idle");
    expect(captured.at(-1)?.detail).toBeUndefined();
  });

  // omo and pi (senpi) share one extensions directory when SENPI_CODING_AGENT_DIR=~/.omo/agent,
  // so whichever identity the file declares, the report must name the process that loaded it.
  // Measured argv: omo runs `bun …/senpi/dist/bundle/cli.js --extension …/omo-ai/plugin`, pi runs
  // `bun …/senpi/dist/bundle/cli.js` (.omo-parity/facts/local-1.md:295,370).
  it.each([
    ["omo", ["/bin/bun", "/g/@code-yeongyu/senpi/dist/bundle/cli.js", "--extension", "/g/omo-ai/plugin", "--session", "x"], "omo"],
    ["omo", ["/bin/bun", "/g/@code-yeongyu/senpi/dist/bundle/cli.js"], "pi"],
    ["pi", ["/bin/bun", "/g/@code-yeongyu/senpi/dist/bundle/cli.js", "--extension=/g/omo-ai/plugin"], "omo"],
    ["pi", ["/bin/bun", "/g/@code-yeongyu/senpi/dist/bundle/cli.js", "-e", "/tmp/probe.ts"], "pi"],
    ["omp", ["/bin/bun", "/g/omo-ai/plugin"], "omp"],
  ])("a %s-declared file run as %j reports %s", async (declared, argv, expected) => {
    const savedArgv = process.argv;
    process.argv = argv;
    try {
      const load = await loadExtension(declared);
      const h = makeHarness();
      load(h.pi);
      h.emit("session_start", h.tuiCtx);
      await waitFor(() => captured.length >= 1);
      expect(captured.at(-1)?.agent).toBe(expected);
    } finally {
      process.argv = savedArgv;
    }
  });
});
