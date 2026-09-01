import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const projectRoot = resolve(import.meta.dirname, "../../..");
const extensionPath = resolve(
  projectRoot,
  "src-tauri/resources/agent-extensions/ferryx-agent-state.ts",
);

// Cross-repo contract that fails silently under mocked contexts: the pi runtime
// never exposes ctx.providerSession, so the extension must derive the provider
// reference from ctx.sessionManager.getSessionId(). A mock-shaped context passed
// task-6 tests while the live pipeline captured nothing, so this test executes
// the shipped TypeScript under bun against a real unix socket.
interface AgentStatePayload {
  sessionId: string;
  state: string;
  agent: string;
  providerSession?: { key: string; id: string };
}

function runExtensionSessionStart(options: {
  providerSessionId: string | null;
  exposeSessionManager: boolean;
}): AgentStatePayload {
  const socketDir = mkdtempSync(`${tmpdir()}/ferryx-ext-contract-`);
  const socketPath = `${socketDir}/agent-state.sock`;
  const runner = `
    const net = await import("node:net");
    const { rmSync } = await import("node:fs");
    const socketPath = ${JSON.stringify(socketPath)};
    const socketDir = ${JSON.stringify(socketDir)};
    const extensionPath = ${JSON.stringify(extensionPath)};
    const providerSessionId = ${JSON.stringify(options.providerSessionId)};
    const exposeSessionManager = ${JSON.stringify(options.exposeSessionManager)};
    const sessionContext = exposeSessionManager
      ? { mode: "tui", isIdle: () => true, sessionManager: { getSessionId: () => providerSessionId } }
      : { mode: "tui", isIdle: () => true };
    const reports = [];
    const server = net.createServer((socket) => {
      socket.on("data", (chunk) => {
        for (const line of chunk.toString("utf8").split("\\n")) {
          if (line.trim().length > 0) reports.push(JSON.parse(line));
        }
        socket.destroy();
      });
    });
    const timeout = setTimeout(() => {
      console.error("extension never sent a state report");
      process.exit(2);
    }, 5000);
    server.listen(socketPath, async () => {
      process.env.FERRYX_AGENT_STATE_SOCKET = socketPath;
      process.env.FERRYX_SESSION_ID = "session:637ed674-32db-4e68-a45f-905395491653";
      const extension = await import(extensionPath);
      const handlers = {};
      extension.default({
        events: { on: () => undefined },
        on: (event, handler) => { handlers[event] = handler; },
      });
      handlers["session_start"]({}, sessionContext);
      const wait = async () => {
        while (reports.length === 0) await new Promise((r) => setTimeout(r, 10));
        clearTimeout(timeout);
        server.close();
        rmSync(socketDir, { recursive: true, force: true });
        console.log(JSON.stringify(reports[0]));
        process.exit(0);
      };
      await wait();
    });
  `;
  const stdout = execFileSync("bun", ["-e", runner], {
    encoding: "utf8",
    timeout: 15000,
  });
  return JSON.parse(stdout.trim()) as AgentStatePayload;
}

describe("ferryx-agent-state extension payload contract", () => {
  it("reports the agent-owned provider session id from the real pi context", () => {
    const payload = runExtensionSessionStart({
      providerSessionId: "01a05841-1890-78ba-a82b-a82f63b73b63",
      exposeSessionManager: true,
    });

    expect(payload.sessionId).toBe("session:637ed674-32db-4e68-a45f-905395491653");
    expect(payload.agent).toBe("omo");
    expect(payload.providerSession).toEqual({
      key: "session_id",
      id: "01a05841-1890-78ba-a82b-a82f63b73b63",
    });
  });

  it("keeps working when the runtime exposes no session manager", () => {
    const payload = runExtensionSessionStart({
      providerSessionId: null,
      exposeSessionManager: false,
    });

    expect(payload.sessionId).toBe("session:637ed674-32db-4e68-a45f-905395491653");
    expect(payload.agent).toBe("omo");
    expect(payload.providerSession).toBeUndefined();
  });
});
