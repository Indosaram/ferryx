import net from "node:net";
import { once } from "node:events";
import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.unstubAllEnvs();
  vi.resetModules();
});

it("delivers authenticated state over a loopback TCP endpoint", async () => {
  const server = net.createServer();
  const listening = once(server, "listening");
  server.listen(0, "127.0.0.1");
  await listening;
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Missing TCP address");
  vi.stubEnv("FERRYX_AGENT_STATE_SOCKET", "");
  vi.stubEnv("FERRYX_AGENT_STATE_PORT", String(address.port));
  vi.stubEnv("FERRYX_AGENT_STATE_TOKEN", "test-session-token");
  vi.stubEnv("FERRYX_SESSION_ID", "test-session");
  const abort = new AbortController();
  const timeout = setTimeout(() => abort.abort(), 2000);
  const received = new Promise<string>((resolve, reject) => {
    abort.signal.addEventListener("abort", () => reject(new Error("No state report received")), { once: true });
    server.once("connection", (socket) => {
      let report = "";
      socket.on("data", (bytes) => {
        report += bytes.toString();
        if (report.includes("\n")) {
          socket.destroy();
          resolve(report.slice(0, report.indexOf("\n")));
        }
      });
    });
  });
  const handlers = new Map<string, (event: unknown, context: unknown) => void>();
  try {
    const extension = await import("../../../src-tauri/resources/agent-extensions/ferryx-agent-state");
    extension.default({ on: (event: string, handler: (event: unknown, context: unknown) => void) => handlers.set(event, handler) });
    handlers.get("session_start")?.({}, { mode: "tui", isIdle: () => true });
    expect(JSON.parse(await received)).toMatchObject({
      type: "agentState", sessionId: "test-session", token: "test-session-token", state: "idle",
    });
  } finally {
    clearTimeout(timeout);
    server.close();
  }
});
