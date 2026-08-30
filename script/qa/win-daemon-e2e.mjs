// Headless Windows daemon E2E: proves daemon -> ConPTY input/output path
// and verified PTY shell working directory on a real Windows host.
//
// PASS = handshakeOk && errorProbeOk && spawnOk && writeOk && outputMarkerOk && cwdOk
// Run: bun script/qa/win-daemon-e2e.mjs [repoRoot]
// Self-test: bun script/qa/win-daemon-e2e.mjs --self-test

import { readFileSync } from "node:fs";
import { createConnection } from "node:net";
import { join } from "node:path";

export function normalizePath(p) {
  if (typeof p !== "string") return "";
  let s = p.trim().replace(/^([/\\]{2}[?.][/\\])/, "").replace(/\\/g, "/");
  s = s.replace(/^([a-zA-Z]):/, (_, d) => `${d.toLowerCase()}:`);
  while (s.length > 3 && s.endsWith("/")) s = s.slice(0, -1);
  return s.toLowerCase();
}

export function pathsMatch(a, b) {
  return normalizePath(a) === normalizePath(b);
}

export function parsePtyOutput(text) {
  const marker = text.match(/FERRYX_MARKER=([a-zA-Z0-9_-]+)/)?.[1] ?? null;
  const cwd = text.match(/FERRYX_CWD=([^\r\n]+)/)?.[1]?.trim() ?? null;
  return { marker, cwd };
}

function connectClient(port) {
  return new Promise((resolve, reject) => {
    const socket = createConnection({ port, host: "127.0.0.1" });
    let buffer = "";
    const pending = [], listeners = new Set();

    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      buffer += chunk;
      let idx;
      while ((idx = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, idx).trim();
        buffer = buffer.slice(idx + 1);
        if (!line) continue;
        try {
          const parsed = JSON.parse(line);
          for (const l of listeners) l(parsed);
          pending.shift()?.resolve(parsed);
        } catch (err) { pending.shift()?.reject(err); }
      }
    });
    socket.on("error", (err) => { while (pending.length) pending.shift()?.reject(err); reject(err); });
    socket.on("connect", () => resolve({
      request(obj, timeoutMs = 15000) {
        return new Promise((res, rej) => {
          const timer = setTimeout(() => {
            const idx = pending.findIndex((p) => p.resolve === res);
            if (idx !== -1) pending.splice(idx, 1);
            rej(new Error(`Timeout waiting response for ${obj.type} (${timeoutMs}ms)`));
          }, timeoutMs);
          pending.push({
            resolve: (v) => { clearTimeout(timer); res(v); },
            reject: (e) => { clearTimeout(timer); rej(e); },
          });
          socket.write(`${JSON.stringify(obj)}\n`);
        });
      },
      onMessage: (fn) => (listeners.add(fn), () => listeners.delete(fn)),
      close: () => socket.destroy(),
    }));
  });
}

function attachStream(port, sessionId, timeoutMs = 15000) {
  return new Promise(async (resolve, reject) => {
    try {
      const client = await connectClient(port);
      let decoded = "";
      const waiters = [];

      const notify = () => {
        const parsed = parsePtyOutput(decoded);
        for (let i = waiters.length - 1; i >= 0; i--) {
          if (parsed.marker === waiters[i].marker && parsed.cwd) {
            clearTimeout(waiters[i].timer);
            waiters.splice(i, 1)[0].resolve({ decoded, parsed });
          }
        }
      };

      client.onMessage((msg) => {
        if (msg.type === "attachOk" && msg.history) {
          decoded += Buffer.from(msg.history, "base64").toString("utf8");
          notify();
        } else if (msg.type === "output" && msg.data) {
          decoded += Buffer.from(msg.data, "base64").toString("utf8");
          notify();
        }
      });

      const hs = await client.request({ type: "handshake", version: 2 }, timeoutMs);
      if (hs.type !== "handshakeOk") throw new Error(`Stream handshake failed: ${JSON.stringify(hs)}`);
      const att = await client.request({ type: "attach", sessionId, afterSequence: null }, timeoutMs);
      if (att.type !== "attachOk") throw new Error(`Stream attach failed: ${JSON.stringify(att)}`);

      resolve({
        waitForPtySignal(marker, waitTimeoutMs = timeoutMs) {
          const parsed = parsePtyOutput(decoded);
          if (parsed.marker === marker && parsed.cwd) return Promise.resolve({ decoded, parsed });
          return new Promise((res, rej) => {
            const timer = setTimeout(() => {
              const idx = waiters.findIndex((w) => w.resolve === res);
              if (idx !== -1) waiters.splice(idx, 1);
              rej(new Error(`Timeout waiting marker "${marker}". Decoded: ${JSON.stringify(decoded.slice(-200))}`));
            }, waitTimeoutMs);
            waiters.push({ marker, resolve: res, reject: rej, timer });
          });
        },
        close: () => (waiters.forEach((w) => clearTimeout(w.timer)), client.close()),
      });
    } catch (err) { reject(err); }
  });
}

export function runPureSelfTest() {
  const n1 = normalizePath("\\\\?\\C:\\Users\\sook\\ferryx-winbuild\\orca-lite");
  const n2 = normalizePath("c:/users/sook/ferryx-winbuild/orca-lite/");
  if (n1 !== "c:/users/sook/ferryx-winbuild/orca-lite" || n1 !== n2) throw new Error("normalizePath mismatch");
  if (!pathsMatch("\\\\?\\C:\\foo\\bar", "c:/foo/bar")) throw new Error("pathsMatch mismatch");
  const parsed = parsePtyOutput("Prompt> FERRYX_MARKER=sig-42\r\nFERRYX_CWD=C:\\Users\\sook\r\nPS > ");
  if (parsed.marker !== "sig-42" || parsed.cwd !== "C:\\Users\\sook") throw new Error("parsePtyOutput mismatch");
  console.log("SELF-TEST PASS: pure normalization and PTY signal parser verified.");
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    console.log("Usage: bun script/qa/win-daemon-e2e.mjs [--self-test] [--port <num>] [--port-file <path>] [repoRoot]");
    return;
  }
  if (args.includes("--self-test")) return runPureSelfTest();

  const pIdx = args.indexOf("--port"), pfIdx = args.indexOf("--port-file");
  const customPort = pIdx !== -1 ? Number(args[pIdx + 1]) : null;
  const customPortFile = pfIdx !== -1 ? args[pfIdx + 1] : null;
  const repoRoot = args.find((a, i) => !a.startsWith("-") && i !== pIdx + 1 && i !== pfIdx + 1)
    ?? "C:\\Users\\sook\\ferryx-winbuild\\orca-lite";

  const portFile = customPortFile ?? join(process.env.LOCALAPPDATA ?? "C:\\ProgramData", "Ferryx", "runtime", "daemon.port");
  const port = customPort ?? Number(readFileSync(portFile, "utf8").trim());
  if (!Number.isInteger(port) || port <= 0) throw new Error(`Invalid daemon port: ${port}`);
  console.log(`INFO: target daemon port ${port}, repoRoot=${repoRoot}`);

  let controlClient = null, streamClient = null, sessionId = null;
  try {
    controlClient = await connectClient(port);
    const hs = await controlClient.request({ type: "handshake", version: 2 });
    if (hs.type !== "handshakeOk") throw new Error("handshake failed");

    // Inline structured error probe
    const errProbe = await controlClient.request({ type: "describeSession", sessionId: `probe-bad-${Date.now()}` });
    const errorProbeOk = errProbe?.type === "error" && typeof errProbe?.message === "string";
    if (!errorProbeOk) throw new Error(`Expected structured error response, got ${JSON.stringify(errProbe)}`);

    await controlClient.request({ type: "registerWorkspace", workspaceId: "e2e-ws", repoRoot });
    const sp = await controlClient.request({
      type: "spawn", clientRequestId: `e2e-${Date.now()}`, workspaceId: "e2e-ws", cwd: repoRoot, cols: 80, rows: 24,
      shell: "powershell.exe",
    });
    sessionId = sp.sessionId ?? sp.session_id;
    if (sp.type !== "spawnOk" || !sessionId) throw new Error("spawn failed");

    // Subscribe/attach to output stream BEFORE writing
    streamClient = await attachStream(port, sessionId);
    const before = await controlClient.request({ type: "describeSession", sessionId });
    const endBefore = Number(before.session?.endSequence ?? before.end_sequence ?? 0);

    const uniqueMarker = `ferryx-win-e2e-${Date.now()}`;
    const ptySignalPromise = streamClient.waitForPtySignal(uniqueMarker, 15000);

    // Platform command emitting both unique marker and actual PTY CWD
    const cmd = `Write-Output "FERRYX_MARKER=${uniqueMarker}"; Write-Output "FERRYX_CWD=$((Get-Location).Path)"\r`;
    const wr = await controlClient.request({ type: "write", sessionId, data: Buffer.from(cmd).toString("base64") });
    if (wr.type !== "writeOk" && wr.writeOk !== true) throw new Error("write rejected");

    // Await streamed PTY output signal (event-driven, bounded timeout, no fixed sleep)
    const { parsed } = await ptySignalPromise;
    const after = await controlClient.request({ type: "describeSession", sessionId });
    const endAfter = Number(after.session?.endSequence ?? after.end_sequence ?? 0);

    const outputMarkerOk = parsed.marker === uniqueMarker;
    const cwdOk = Boolean(parsed.cwd && pathsMatch(parsed.cwd, repoRoot));

    const summary = {
      verdict: outputMarkerOk && cwdOk && endAfter > endBefore ? "PASS" : "FAIL",
      handshakeOk: true,
      errorProbeOk: true,
      spawnOk: true,
      writeOk: true,
      outputMarkerOk,
      cwdOk,
      sessionId,
      requestedRepoRoot: repoRoot,
      ptyExtractedCwd: parsed.cwd,
      normalizedRequestedRepoRoot: normalizePath(repoRoot),
      normalizedPtyCwd: normalizePath(parsed.cwd),
      uniqueMarker,
      endSequenceBefore: endBefore,
      endSequenceAfter: endAfter,
    };

    console.log("MACHINE_READABLE_SUMMARY:", JSON.stringify(summary, null, 2));
    if (summary.verdict !== "PASS") throw new Error(`Summary check failed: ${JSON.stringify(summary)}`);
    console.log(`E2E PASS: marker and actual PTY CWD verified (${summary.ptyExtractedCwd})`);
  } finally {
    if (sessionId && controlClient) {
      try { await controlClient.request({ type: "close", sessionId }, 3000); } catch {}
    }
    if (streamClient) streamClient.close();
    if (controlClient) controlClient.close();
  }
}

if (import.meta.main || process.argv[1]?.endsWith("win-daemon-e2e.mjs")) {
  try {
    await main();
    process.exit(0);
  } catch (err) {
    console.error("E2E FAIL:", err.message);
    process.exit(1);
  }
}
