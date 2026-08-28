#!/usr/bin/env node

import { spawn, execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const uiDir = path.resolve(__dirname, "..");
const repoRoot = path.resolve(uiDir, "..");

const SECTIONS = [
  { label: "General", file: "general.png" },
  { label: "Appearance", file: "appearance.png" },
  { label: "Terminal", file: "terminal.png" },
  { label: "Keyboard Shortcuts", file: "keyboard-shortcuts.png" },
  { label: "Workspace", file: "workspace.png" },
  { label: "Agents", file: "agents.png" },
  { label: "Browser", file: "browser.png" },
  { label: "Notifications", file: "notifications.png" },
  { label: "Remote Access", file: "remote-access.png" },
];

// Parse CLI arguments
let outDir = path.resolve(repoRoot, ".omo/plans/evidence/settings-panel/before");
for (let i = 2; i < process.argv.length; i++) {
  const arg = process.argv[i];
  if (arg === "--out-dir" && process.argv[i + 1]) {
    outDir = path.resolve(process.argv[i + 1]);
    i++;
  } else if (arg.startsWith("--out-dir=")) {
    outDir = path.resolve(arg.slice("--out-dir=".length));
  }
}

fs.mkdirSync(outDir, { recursive: true });

const AGENT_BROWSER_BIN = "/opt/homebrew/bin/agent-browser";

function runAgentBrowser(cmd) {
  return execSync(`${AGENT_BROWSER_BIN} ${cmd}`, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function killPortProcesses(port = 5199) {
  try {
    const raw = execSync(`lsof -ti :${port}`, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    const pids = raw.trim().split(/\s+/).filter(Boolean);
    for (const pidStr of pids) {
      const pid = Number(pidStr);
      if (pid && pid !== process.pid) {
        try {
          process.kill(pid, "SIGKILL");
        } catch {}
      }
    }
  } catch {}
}

function cleanupServer(viteProc) {
  if (viteProc && viteProc.pid) {
    try {
      viteProc.kill("SIGKILL");
    } catch {}
  }
  killPortProcesses(5199);
}

async function main() {
  console.log(`Starting Settings QA Capture Harness`);
  console.log(`Output Directory: ${outDir}`);

  // Clear any existing listener on port 5199
  killPortProcesses(5199);

  // Spawn Vite dev server on port 5199
  console.log("Spawning Vite dev server on port 5199...");
  const viteProc = spawn("bun", ["run", "dev", "--", "--port", "5199", "--strictPort"], {
    cwd: uiDir,
    stdio: ["ignore", "pipe", "pipe"],
  });

  viteProc.on("error", (err) => {
    console.error("Failed to start Vite server:", err);
  });

  // Poll until Vite server responds with 200 (no fixed sleep)
  const harnessUrl = "http://localhost:5199/qa/settingsHarness.html";
  const deadline = Date.now() + 20000;
  let serverReady = false;

  while (Date.now() < deadline) {
    try {
      const res = await fetch(harnessUrl);
      if (res.ok) {
        serverReady = true;
        break;
      }
    } catch {
      // Server starting up, retry
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  if (!serverReady) {
    cleanupServer(viteProc);
    throw new Error(`Timed out waiting for Vite server at ${harnessUrl}`);
  }

  console.log(`Vite server ready at ${harnessUrl}`);

  const results = [];

  try {
    // Set viewport for clean desktop screenshot
    runAgentBrowser("set viewport 1400 900");

    // Open harness page
    console.log(`Opening ${harnessUrl}...`);
    runAgentBrowser(`open "${harnessUrl}"`);

    // Poll until __qaReady is true
    let ready = false;
    const readyDeadline = Date.now() + 10000;
    while (Date.now() < readyDeadline) {
      try {
        const out = runAgentBrowser("eval \"Boolean(window.__qaReady && window.__qaClickSection)\"");
        if (out.includes("true")) {
          ready = true;
          break;
        }
      } catch {}
      await new Promise((r) => setTimeout(r, 100));
    }

    if (!ready) {
      throw new Error("Harness page loaded but __qaReady was not set");
    }

    for (const section of SECTIONS) {
      // Click section
      runAgentBrowser(`eval "window.__qaClickSection('${section.label}')"`);

      // Wait a paint frame set
      runAgentBrowser("eval \"new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))\"");

      const outPath = path.join(outDir, section.file);
      runAgentBrowser(`screenshot "${outPath}"`);

      // Verify file exists and size > 20KB (20000 bytes)
      let pass = false;
      let size = 0;
      if (fs.existsSync(outPath)) {
        const stats = fs.statSync(outPath);
        size = stats.size;
        pass = size > 20000;
      }

      results.push({
        label: section.label,
        file: section.file,
        size,
        pass,
      });

      console.log(`[${pass ? "PASS" : "FAIL"}] ${section.label} -> ${section.file} (${size} bytes)`);
    }
  } finally {
    // Cleanup agent browser and vite server
    console.log("\nCleaning up browser and server processes...");
    try {
      runAgentBrowser("close");
    } catch (e) {
      console.warn("agent-browser close notice:", e.message);
    }
    cleanupServer(viteProc);
  }

  // Summary report
  console.log("\n=== CAPTURE SUMMARY ===");
  let allPass = true;
  for (const r of results) {
    console.log(`${r.pass ? "PASS" : "FAIL"} | ${r.label.padEnd(20)} | ${r.file.padEnd(25)} | ${r.size} bytes`);
    if (!r.pass) allPass = false;
  }

  // Verify server cleanup: check listening sockets on 5199
  await new Promise((resolve) => setTimeout(resolve, 200));
  let portCheck = "";
  try {
    portCheck = execSync("lsof -iTCP:5199 -sTCP:LISTEN", { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {}

  console.log("\n=== SERVER CLEANUP RECEIPT ===");
  if (portCheck) {
    console.error("FAIL: Port 5199 still has active listener:\n" + portCheck);
    process.exit(1);
  } else {
    console.log("PASS: Port 5199 is dead (lsof -iTCP:5199 -sTCP:LISTEN is empty).");
  }

  if (!allPass || results.length !== SECTIONS.length) {
    console.error("One or more screenshot captures failed or did not meet >20KB criteria.");
    process.exit(1);
  }

  console.log(`\nAll ${SECTIONS.length} section screenshots successfully captured and verified!`);
}

main().catch((err) => {
  console.error("Capture script error:", err);
  killPortProcesses(5199);
  process.exit(1);
});
