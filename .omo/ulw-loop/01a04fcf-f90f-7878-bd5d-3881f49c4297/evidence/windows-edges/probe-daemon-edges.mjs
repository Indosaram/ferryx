import { createConnection } from "node:net";

const [portArg, repoRoot] = process.argv.slice(2);
const port = Number(portArg);
if (!Number.isInteger(port) || !repoRoot) throw new Error("usage: probe-daemon-edges.mjs <port> <repoRoot>");
const skipWsl = process.argv.includes("--skip-wsl");

function connect() {
  return new Promise((resolve, reject) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    let buffer = "";
    const pending = [];
    const listeners = new Set();
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      buffer += chunk;
      let newline;
      while ((newline = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (!line) continue;
        const message = JSON.parse(line);
        for (const listener of listeners) listener(message);
        pending.shift()?.(message);
      }
    });
    socket.on("error", reject);
    socket.on("connect", () => resolve({
      request(message, timeoutMs = 15_000) {
        return new Promise((res, rej) => {
          const timer = setTimeout(() => rej(new Error(`timeout: ${message.type}`)), timeoutMs);
          pending.push((response) => { clearTimeout(timer); res(response); });
          socket.write(`${JSON.stringify(message)}\n`);
        });
      },
      onMessage(listener) { listeners.add(listener); return () => listeners.delete(listener); },
      close() { socket.destroy(); },
    }));
  });
}

function expectError(response, pattern, label) {
  if (response?.type !== "error" || !pattern.test(response.message ?? "")) {
    throw new Error(`${label}: expected structured error, got ${JSON.stringify(response)}`);
  }
  console.log(`${label}_PASS message=${JSON.stringify(response.message)}`);
}

async function spawn(client, id, overrides = {}) {
  return client.request({
    type: "spawn", clientRequestId: id, workspaceId: "edge-ws",
    cwd: repoRoot, cols: 80, rows: 24, ...overrides,
  });
}

async function closeIfSpawned(client, response) {
  if (response?.type === "spawnOk") await client.request({ type: "close", sessionId: response.sessionId });
}

async function probeWsl(client) {
  const response = await spawn(client, "edge-wsl", { shell: "wsl.exe" });
  if (response.type !== "spawnOk") throw new Error(`WSL spawn failed: ${JSON.stringify(response)}`);
  const sessionId = response.sessionId;
  const stream = await connect();
  let decoded = "";
  const marker = `FERRYX_WSL_${Date.now()}`;
  try {
    const markerOutput = new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error(`WSL output marker timeout: ${JSON.stringify(decoded.slice(-500))}`)),
        30_000,
      );
      stream.onMessage((message) => {
        if (message.type === "attachOk" && message.history) decoded += Buffer.from(message.history, "base64").toString();
        if (message.type === "output" && message.data) decoded += Buffer.from(message.data, "base64").toString();
        if (decoded.includes(`${marker}|PWD=`)) {
          clearTimeout(timer);
          resolve(decoded);
        }
      });
    });
    await stream.request({ type: "handshake", version: 2 });
    await stream.request({ type: "attach", sessionId, afterSequence: null });
    const command = `printf '${marker}|PWD=%s\\n' "$PWD"\r\n`;
    await client.request({ type: "write", sessionId, data: Buffer.from(command).toString("base64") });
    await markerOutput;
    const match = decoded.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").match(new RegExp(`${marker}\\|PWD=([^\\r\\n]+)`));
    if (!match?.[1]) throw new Error(`WSL output marker/CWD missing: ${JSON.stringify(decoded.slice(-500))}`);
    console.log(`WSL_OUTPUT_PASS cwd=${JSON.stringify(match[1].trim())}`);
  } finally {
    stream.close();
    await client.request({ type: "close", sessionId }).catch(() => {});
  }
}

const client = await connect();
try {
  const handshake = await client.request({ type: "handshake", version: 2 });
  if (handshake.type !== "handshakeOk") throw new Error(`handshake: ${JSON.stringify(handshake)}`);

  expectError(await spawn(client, "edge-unregistered"), /not registered/i, "UNREGISTERED_WORKSPACE");
  const registration = await client.request({ type: "registerWorkspace", workspaceId: "edge-ws", repoRoot });
  if (registration.type !== "registerWorkspaceOk") throw new Error(`registration: ${JSON.stringify(registration)}`);

  const omitted = await spawn(client, "edge-shell-omitted");
  if (omitted.type !== "spawnOk") throw new Error(`omitted shell: ${JSON.stringify(omitted)}`);
  console.log("MISSING_SHELL_DEFAULT_PASS");
  await closeIfSpawned(client, omitted);

  const empty = await spawn(client, "edge-shell-empty", { shell: "" });
  if (empty.type !== "spawnOk") throw new Error(`empty shell: ${JSON.stringify(empty)}`);
  console.log("EMPTY_SHELL_DEFAULT_PASS");
  await closeIfSpawned(client, empty);

  expectError(
    await spawn(client, "edge-shell-invalid", { shell: "Z:\\definitely-missing\\ferryx-shell.exe" }),
    /failed to spawn|cannot find|not found|os error/i,
    "INVALID_SHELL",
  );
  expectError(
    await spawn(client, "edge-cwd-missing", { cwd: `${repoRoot}\\definitely-missing-cwd` }),
    /CWD does not exist/i,
    "MISSING_CWD",
  );
  expectError(
    await spawn(client, "edge-cwd-outside", { cwd: "C:\\Users\\sook" }),
    /outside|workspace|worktree|not registered/i,
    "OUTSIDE_CWD",
  );
  if (skipWsl) console.log("WSL_OUTPUT_NOT_RUN environment=direct-wsl-create-instance-failed");
  else await probeWsl(client);
  console.log("WINDOWS_PROTOCOL_EDGES_PASS");
} finally {
  client.close();
}
