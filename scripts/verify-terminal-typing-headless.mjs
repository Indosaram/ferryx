#!/usr/bin/env node
// Headless typing round-trip proof against the live Ferryx daemon.
//
// Unlike verify-terminal-typing.mjs, this does not read a GUI trace file: it
// spawns its own throwaway PTY session over the daemon UDS protocol, writes a
// burst of keystrokes, and asserts the session's ring endSequence grows. It
// never touches sessions it did not create, so it is safe to run while the
// desktop app is in use.
//
// Exit 0 = PASS (ring grew), exit 1 = FAIL.

import net from "node:net";

const SOCKET_PATH = `/tmp/rorca-${process.getuid()}/daemon.sock`;
const WORKSPACE_ID = `headless-typing-probe-${process.pid}`;
const REPO_ROOT = process.cwd();

function connect() {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ path: SOCKET_PATH });
    let buffer = "";
    const pending = [];
    socket.on("data", (chunk) => {
      buffer += chunk.toString("utf8");
      let index = buffer.indexOf("\n");
      while (index >= 0) {
        const line = buffer.slice(0, index);
        buffer = buffer.slice(index + 1);
        if (line.trim()) {
          const next = pending.shift();
          if (next) next(JSON.parse(line));
        }
        index = buffer.indexOf("\n");
      }
    });
    socket.on("error", reject);
    socket.on("connect", () =>
      resolve({
        call: (payload) =>
          new Promise((res) => {
            pending.push(res);
            socket.write(`${JSON.stringify(payload)}\n`);
          }),
        end: () => socket.end(),
      }),
    );
  });
}

// Waits for a session's endSequence to exceed `floor`, polling the daemon.
// Bounded by `budgetMs` so a genuinely dead ring fails instead of hanging.
async function waitForRingGrowth(call, sessionId, floor, budgetMs = 5000) {
  const deadline = Date.now() + budgetMs;
  let last = floor;
  while (Date.now() < deadline) {
    const described = await call({ type: "describeSession", sessionId });
    const session = described.session ?? described ?? {};
    const end = session.endSequence ?? 0;
    last = end;
    if (end > floor) return { grew: true, endSequence: end };
    await new Promise((res) => setTimeout(res, 100));
  }
  return { grew: false, endSequence: last };
}

async function main() {
  const { call, end } = await connect();
  let sessionId = null;
  try {
    const handshake = await call({ type: "handshake", version: 2 });
    console.log(`handshake: ${JSON.stringify(handshake)}`);

    await call({
      type: "registerWorkspace",
      workspaceId: WORKSPACE_ID,
      repoRoot: REPO_ROOT,
    });

    const spawned = await call({
      type: "spawn",
      clientRequestId: `probe-${Date.now()}`,
      workspaceId: WORKSPACE_ID,
      worktree: null,
      cwd: REPO_ROOT,
      cols: 80,
      rows: 24,
    });
    sessionId = spawned.sessionId ?? spawned.session?.sessionId ?? null;
    if (!sessionId) {
      console.error(`FAIL - spawn returned no sessionId: ${JSON.stringify(spawned)}`);
      process.exitCode = 1;
      return;
    }
    console.log(`spawned session: ${sessionId}`);

    // Let the shell settle so the prompt banner is not mistaken for typed echo.
    const settled = await waitForRingGrowth(call, sessionId, 0, 5000);
    const baseline = settled.endSequence;
    console.log(`baseline endSequence after shell start: ${baseline}`);

    const burst = "echo ferryx-headless-typing-probe\r";
    await call({
      type: "write",
      sessionId,
      data: Buffer.from(burst, "utf8").toString("base64"),
    });
    console.log(`wrote ${burst.length} bytes of input`);

    const grown = await waitForRingGrowth(call, sessionId, baseline, 5000);
    console.log(`endSequence after burst: ${grown.endSequence}`);
    const delta = grown.endSequence - baseline;

    if (grown.grew) {
      console.log(`VERDICT: PASS - ring grew by ${delta} bytes (${baseline} -> ${grown.endSequence})`);
    } else {
      console.error(`VERDICT: FAIL - ring did not grow past ${baseline} within budget`);
      process.exitCode = 1;
    }
  } finally {
    if (sessionId) {
      await call({ type: "close", sessionId }).catch(() => {});
      console.log(`closed probe session: ${sessionId}`);
    }
    end();
  }
}

main().catch((error) => {
  console.error(`FAIL - ${error?.message ?? error}`);
  process.exitCode = 1;
});
