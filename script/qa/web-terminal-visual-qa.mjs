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

function parseArgs(argv) {
  const options = {
    title: "Terminal QA",
    command: "",
    input: "",
    evidenceDir: "",
  };
  for (let index = 2; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!value || !["--title", "--command", "--input", "--evidence-dir"].includes(flag)) {
      throw new Error("Usage: web-terminal-visual-qa.mjs --title <title> --command <command> --input <text> --evidence-dir <directory>");
    }
    if (flag === "--title") options.title = value;
    if (flag === "--command") options.command = value;
    if (flag === "--input") options.input = value;
    if (flag === "--evidence-dir") options.evidenceDir = value;
  }
  if (!options.command || !options.evidenceDir) {
    throw new Error("--command and --evidence-dir are required");
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

async function captureTranscript(command, input) {
  return new Promise((resolveTranscript, reject) => {
    const terminal = pty.spawn("/bin/sh", ["-lc", command], {
      name: "xterm-256color",
      cols: 80,
      rows: 24,
      cwd: repoRoot,
      env: process.env,
    });
    let transcript = "";
    let inputWriteCount = 0;
    let inputSent = false;
    let completed = false;
    const complete = () => {
      if (completed) return;
      completed = true;
      clearTimeout(timer);
      terminal.kill();
      resolveTranscript({ transcript, inputWriteCount });
    };
    const timer = setTimeout(() => {
      terminal.kill();
      if (!completed) {
        completed = true;
        terminal.kill();
        reject(new Error(`PTY command timed out with transcript ${JSON.stringify(transcript)}`));
      }
    }, 10_000);
    terminal.onData((data) => {
      transcript += data;
      if (!inputSent && transcript.includes("ready")) {
        inputSent = true;
        terminal.write(`${input}\n`);
        inputWriteCount += 1;
      } else if (inputSent && transcript.split(input).length - 1 >= 2) {
        complete();
      }
    });
    terminal.onExit(({ exitCode }) => {
      if (completed) return;
      completed = true;
      clearTimeout(timer);
      if (exitCode === 0) {
        resolveTranscript({ transcript, inputWriteCount });
      } else {
        reject(new Error(`PTY command exited ${exitCode}: ${transcript}`));
      }
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
  let cleanup = { browserClosed: false, serverExited: false };

  await mkdir(evidenceDir, { recursive: true });
  try {
    const { transcript, inputWriteCount } = await captureTranscript(options.command, options.input);
    if (!transcript.includes("ready") || !transcript.includes(options.input)) {
      throw new Error(`PTY transcript did not render both readiness and pasted input: ${JSON.stringify(transcript)}`);
    }
    if (inputWriteCount !== 1) {
      throw new Error(`Expected one paste write, received ${inputWriteCount}`);
    }
    await writeFile(ansiPath, transcript);
    await writeFile(textPath, transcript.replace(/\x1B\[[0-?]*[ -/]*[@-~]/g, ""));
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
    process.env.AGENT_BROWSER_SESSION = "ferryx-terminal-qa";
    await execute(agentBrowser, ["set", "viewport", "1280", "720"]);
    await execute(agentBrowser, ["open", url]);
    await waitFor(async () => (await execute(agentBrowser, ["eval", "Boolean(window.__terminalQaReady)"])).includes("true"), 10_000, "xterm browser surface did not report ready");
    await execute(agentBrowser, ["screenshot", pngPath]);
    const png = await readFile(pngPath);
    if (!png.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) {
      throw new Error("Browser capture is not a PNG");
    }
    const metadata = {
      title: options.title,
      command: options.command,
      input: options.input,
      inputWriteCount,
      inputWriteBytes: Buffer.byteLength(`${options.input}\n`),
      transcriptContainsReady: true,
      transcriptContainsCompletePaste: true,
      terminalAnsiPath: "terminal-ansi.txt",
      terminalTextPath: "terminal.txt",
      screenshotPath: "terminal.png",
      screenshotBytes: (await stat(pngPath)).size,
      screenshotSha256: createHash("sha256").update(png).digest("hex"),
      cleanup,
    };
    await writeFile(metadataPath, `${JSON.stringify(metadata, null, 2)}\n`);
    console.log(`PASS: ${options.title}; one immediate PTY paste rendered in xterm at ${pngPath}`);
  } finally {
    try { await execute(agentBrowser, ["close"]); cleanup.browserClosed = true; } catch {}
    if (server?.listening) {
      await new Promise((resolveClose) => server.close(resolveClose));
      cleanup.serverExited = true;
    }
    if (oldBrowserSession === undefined) delete process.env.AGENT_BROWSER_SESSION;
    else process.env.AGENT_BROWSER_SESSION = oldBrowserSession;
    const metadata = JSON.parse(await readFile(metadataPath, "utf8").catch(() => "{}"));
    await writeFile(metadataPath, `${JSON.stringify({ ...metadata, cleanup }, null, 2)}\n`);
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
