#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const pty = createRequire(import.meta.url)("node-pty");
const uiRequire = createRequire(resolve(dirname(fileURLToPath(import.meta.url)), "../../ui/package.json"));

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const agentBrowser = "/opt/homebrew/bin/agent-browser";

const DEFAULT_FIXTURE_PATH = ".omo/evidence/terminal-input-fixes/image/c2-image-paste-fixture.png";
const DEFAULT_EVIDENCE_DIR = ".omo/evidence/terminal-input-fixes/image/scenario";
const DEFAULT_TITLE = "Ferryx C2 Image Paste Scenario (0x16 -> Mock Agent Acknowledgment)";
const EXPECTED_ACK_TOKEN = "[MockAgent Image #1 attached]";

function parseArgs(argv) {
  const options = {
    title: DEFAULT_TITLE,
    fixturePath: DEFAULT_FIXTURE_PATH,
    evidenceDir: DEFAULT_EVIDENCE_DIR,
    triggerByte: 0x16,
    simulateRed: null,
  };

  for (let i = 2; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (flag === "--title" && value) options.title = value;
    else if (flag === "--fixture" && value) options.fixturePath = value;
    else if (flag === "--evidence-dir" && value) options.evidenceDir = value;
    else if (flag === "--trigger-byte" && value) options.triggerByte = parseInt(value, 16) || parseInt(value, 10);
    else if (flag === "--simulate-red" && value) options.simulateRed = value;
    else {
      throw new Error(`Usage: terminal-image-paste-scenario.mjs [--fixture <path>] [--evidence-dir <dir>] [--trigger-byte <hex>] [--simulate-red <type>]`);
    }
  }

  return options;
}

function execute(command, args, options = {}) {
  return new Promise((resolveCommand, reject) => {
    const child = spawn(command, args, { stdio: ["ignore", "pipe", "pipe"], ...options });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolveCommand(stdout);
      } else {
        reject(new Error(`${command} exited ${code}: ${stderr || stdout}`));
      }
    });
  });
}

function htmlFor(title, transcript) {
  return `<!doctype html>
<html><head><meta charset="utf-8"><title>${title}</title>
<link rel="stylesheet" href="/node_modules/@xterm/xterm/css/xterm.css">
<style>html,body{margin:0;width:100%;height:100%;background:#121217}#terminal{height:100%;padding:18px;box-sizing:border-box}.xterm{height:100%;font-family:Menlo,monospace;font-size:16px}</style>
</head><body><div id="terminal"></div><script src="/node_modules/@xterm/xterm/lib/xterm.js"></script><script>
const terminal = new window.Terminal({cols:80,rows:24,disableStdin:true,theme:{background:"#121217",foreground:"#e4e4e7"}});
terminal.open(document.getElementById("terminal"));
terminal.write(${JSON.stringify(transcript)},()=>{window.__terminalQaReady=true});
</script></body></html>`;
}

async function waitFor(predicate, timeoutMs, message) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolveWait) => setTimeout(resolveWait, 50));
  }
  throw new Error(message);
}

async function validateFixture(fixtureRelativePath) {
  const absPath = resolve(repoRoot, fixtureRelativePath);
  const data = await readFile(absPath);
  const magic = Buffer.from("89504e470d0a1a0a", "hex");
  if (!data.subarray(0, 8).equals(magic)) {
    throw new Error(`Fixture ${fixtureRelativePath} does not have a valid PNG magic signature`);
  }
  const sha256 = createHash("sha256").update(data).digest("hex");
  return {
    relativePath: fixtureRelativePath,
    absolutePath: absPath,
    sizeBytes: data.length,
    sha256,
    validPng: true,
  };
}

async function captureMockAgentTranscript({ fixtureMeta, triggerByte, simulateRed }) {
  const byteToSend = simulateRed === "wrong-trigger-byte" ? 0x15 : triggerByte;

  return new Promise((resolveTranscript, reject) => {
    // Child script executed in real PTY in raw mode
    const childNodeCode = `
const fs = require('fs');
const crypto = require('crypto');
process.stdin.setRawMode(true);
process.stdin.resume();
process.stdout.write('\\x1b[36m=== Ferryx Terminal Mock Agent Initialized ===\\x1b[0m\\r\\n');
process.stdout.write('mock-agent (pid ' + process.pid + ') listening in raw mode for terminal image input signal (0x16)...\\r\\n');
process.stdout.write('mock-agent> ready\\r\\n');

process.stdin.on('data', (buf) => {
  for (const byte of buf) {
    if (byte === 0x16) {
      const fixturePath = process.env.FIXTURE_PNG_PATH;
      let fixtureSha = 'unknown';
      let validPng = false;
      let sizeBytes = 0;
      if (fixturePath && fs.existsSync(fixturePath)) {
        const data = fs.readFileSync(fixturePath);
        sizeBytes = data.length;
        fixtureSha = crypto.createHash('sha256').update(data).digest('hex');
        validPng = data.subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex'));
      }
      process.stdout.write('\\r\\n\\x1b[32;1m${EXPECTED_ACK_TOKEN}\\x1b[0m\\r\\n');
      process.stdout.write('  Attachment trigger byte: 0x16 (SYN / Ctrl+V)\\r\\n');
      process.stdout.write('  Fixture path: ' + fixturePath + '\\r\\n');
      process.stdout.write('  Fixture SHA256: ' + fixtureSha + '\\r\\n');
      process.stdout.write('  Fixture size: ' + sizeBytes + ' bytes\\r\\n');
      process.stdout.write('  Fixture valid PNG: ' + validPng + '\\r\\n');
      process.stdout.write('mock-agent> Image attachment processed; ready for user prompt.\\r\\n');
      process.exit(0);
    } else if (byte === 0x03) {
      process.stdout.write('\\r\\n[Interrupt received]\\r\\n');
      process.exit(130);
    } else {
      process.stdout.write('\\r\\nmock-agent: received unexpected byte: 0x' + byte.toString(16).padStart(2, '0') + ' (expected 0x16)\\r\\n');
      process.exit(1);
    }
  }
});
`;

    const terminal = pty.spawn(process.execPath, ["-e", childNodeCode], {
      name: "xterm-256color",
      cols: 80,
      rows: 24,
      cwd: repoRoot,
      env: {
        ...process.env,
        FIXTURE_PNG_PATH: fixtureMeta.absolutePath,
        FIXTURE_SHA256: simulateRed === "wrong-fixture-hash" ? "0000000000000000000000000000000000000000000000000000000000000000" : fixtureMeta.sha256,
      },
    });

    let transcript = "";
    let triggerWriteCount = 0;
    let triggerSent = false;
    let completed = false;

    const timer = setTimeout(() => {
      terminal.kill();
      if (!completed) {
        completed = true;
        reject(new Error(`PTY mock agent timed out with transcript: ${JSON.stringify(transcript)}`));
      }
    }, 10_000);

    terminal.onData((data) => {
      transcript += data;
      if (!triggerSent && transcript.includes("ready")) {
        triggerSent = true;
        if (simulateRed !== "missing-trigger") {
          terminal.write(Buffer.from([byteToSend]));
          triggerWriteCount += 1;
        }
      }
    });

    terminal.onExit(({ exitCode, signal }) => {
      if (completed) return;
      completed = true;
      clearTimeout(timer);
      resolveTranscript({
        transcript,
        triggerWriteCount,
        exitCode,
        signal,
        triggerByteSent: byteToSend,
      });
    });
  });
}

async function main() {
  const options = parseArgs(process.argv);
  const evidenceDir = resolve(repoRoot, options.evidenceDir);
  const xtermBundlePath = uiRequire.resolve("@xterm/xterm");
  const xtermCssPath = uiRequire.resolve("@xterm/xterm/css/xterm.css");
  const [xtermBundle, xtermCss] = await Promise.all([readFile(xtermBundlePath), readFile(xtermCssPath)]);

  const pngPath = resolve(evidenceDir, "terminal.png");
  const ansiPath = resolve(evidenceDir, "terminal-ansi.txt");
  const textPath = resolve(evidenceDir, "terminal.txt");
  const metadataPath = resolve(evidenceDir, "metadata.json");

  const oldBrowserSession = process.env.AGENT_BROWSER_SESSION;
  let server;
  let cleanup = { browserClosed: false, serverExited: false, ptyClosed: false };

  console.log(`[C2 Image Paste QA Scenario]`);
  console.log(`  Validating PNG fixture: ${options.fixturePath}...`);
  const fixtureMeta = await validateFixture(options.fixturePath);
  console.log(`  PNG fixture valid: size=${fixtureMeta.sizeBytes}b, SHA256=${fixtureMeta.sha256}`);

  await mkdir(evidenceDir, { recursive: true });

  try {
    console.log(`  Spawning mock agent in real PTY and delivering image paste trigger byte (0x${options.triggerByte.toString(16)})...`);
    const ptyResult = await captureMockAgentTranscript({
      fixtureMeta,
      triggerByte: options.triggerByte,
      simulateRed: options.simulateRed,
    });
    cleanup.ptyClosed = true;

    const { transcript, triggerWriteCount, exitCode, triggerByteSent } = ptyResult;

    if (exitCode !== 0) {
      throw new Error(`Mock agent PTY exited with failure code ${exitCode}. Transcript:\n${transcript}`);
    }
    if (triggerWriteCount !== 1) {
      throw new Error(`Expected exactly 1 trigger write to PTY, got ${triggerWriteCount}`);
    }
    if (!transcript.includes("ready")) {
      throw new Error(`Mock agent transcript did not signal readiness`);
    }
    if (!transcript.includes(EXPECTED_ACK_TOKEN)) {
      throw new Error(`Mock agent transcript did not contain expected acknowledgment token '${EXPECTED_ACK_TOKEN}'`);
    }
    if (!transcript.includes(fixtureMeta.sha256)) {
      throw new Error(`Mock agent transcript did not contain expected fixture SHA-256 ${fixtureMeta.sha256}`);
    }

    console.log(`  Mock agent successfully acknowledged image attachment token and fixture SHA-256.`);
    console.log(`  Writing ANSI and plaintext transcript files...`);
    await writeFile(ansiPath, transcript);
    await writeFile(textPath, transcript.replace(/\x1B\[[0-?]*[ -/]*[@-~]/g, ""));

    console.log(`  Hosting xterm.js browser surface for visual render...`);
    server = createServer((request, response) => {
      if (request.url === "/terminal.html") {
        response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        response.end(htmlFor(options.title, transcript));
      } else if (request.url === "/node_modules/@xterm/xterm/lib/xterm.js") {
        response.writeHead(200, { "content-type": "application/javascript" });
        response.end(xtermBundle);
      } else if (request.url === "/node_modules/@xterm/xterm/css/xterm.css") {
        response.writeHead(200, { "content-type": "text/css; charset=utf-8" });
        response.end(xtermCss);
      } else {
        response.writeHead(404);
        response.end();
      }
    });

    await new Promise((resolveListen, rejectListen) => {
      server.once("error", rejectListen);
      server.listen(0, "127.0.0.1", resolveListen);
    });

    const address = server.address();
    if (!address || typeof address === "string") throw new Error("QA HTTP server did not expose a TCP port");
    const url = `http://127.0.0.1:${address.port}/terminal.html`;

    process.env.AGENT_BROWSER_SESSION = "ferryx-terminal-image-qa";
    await execute(agentBrowser, ["set", "viewport", "1280", "720"]);
    await execute(agentBrowser, ["open", url]);
    await waitFor(
      async () => (await execute(agentBrowser, ["eval", "Boolean(window.__terminalQaReady)"])).includes("true"),
      10_000,
      "xterm browser surface did not report ready"
    );

    console.log(`  Capturing visual screenshot to ${pngPath}...`);
    await execute(agentBrowser, ["screenshot", pngPath]);

    const png = await readFile(pngPath);
    if (!png.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) {
      throw new Error("Browser screenshot is not a valid PNG");
    }

    const metadata = {
      title: options.title,
      scenario: "DOM image paste -> Ctrl+V IPC -> PTY 0x16 byte -> Mock downstream agent acknowledgment",
      triggerByteHex: `0x${triggerByteSent.toString(16).padStart(2, "0")}`,
      triggerByteDec: triggerByteSent,
      triggerWriteCount,
      fixture: {
        path: fixtureMeta.relativePath,
        sizeBytes: fixtureMeta.sizeBytes,
        sha256: fixtureMeta.sha256,
        validPng: fixtureMeta.validPng,
      },
      agentAcknowledgmentToken: EXPECTED_ACK_TOKEN,
      transcriptContainsReady: true,
      transcriptContainsToken: true,
      transcriptContainsFixtureSha: true,
      terminalAnsiPath: "terminal-ansi.txt",
      terminalTextPath: "terminal.txt",
      screenshotPath: "terminal.png",
      screenshotBytes: (await stat(pngPath)).size,
      screenshotSha256: createHash("sha256").update(png).digest("hex"),
      limitationStatement: (
        "This scenario proves Ferryx's transport trigger (PTY byte 0x16 delivery) and fixture receipt only, " +
        "NOT an actual Claude [Image #N] chip. Downstream interactive Claude Code chip rendering requires " +
        "interactive desktop NSPasteboard access and live Anthropic API."
      ),
      cleanup,
    };

    await writeFile(metadataPath, `${JSON.stringify(metadata, null, 2)}\n`);
    console.log(`PASS: ${options.title}; one 0x16 PTY trigger produced mock agent attachment acknowledgment rendered in xterm at ${pngPath}`);
  } finally {
    try {
      await execute(agentBrowser, ["close"]);
      cleanup.browserClosed = true;
    } catch {}
    if (server?.listening) {
      await new Promise((resolveClose) => server.close(resolveClose));
      cleanup.serverExited = true;
    }
    if (oldBrowserSession === undefined) delete process.env.AGENT_BROWSER_SESSION;
    else process.env.AGENT_BROWSER_SESSION = oldBrowserSession;

    try {
      const existingMeta = JSON.parse(await readFile(metadataPath, "utf8"));
      if (existingMeta) {
        await writeFile(metadataPath, `${JSON.stringify({ ...existingMeta, cleanup }, null, 2)}\n`);
      }
    } catch {}
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
