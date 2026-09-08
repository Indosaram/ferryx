import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import net from "node:net";

const PORT = 5211;
const URL = `http://127.0.0.1:${PORT}/qa-cont-sidebar.html`;
const SCREENSHOT_DIR = path.resolve(process.cwd(), "docs/session-continuation-20260908/sidebar/screenshots");
const CACHE_DIR = path.resolve(process.cwd(), "ui/.vite-qa-cont-sidebar");

await fs.mkdir(SCREENSHOT_DIR, { recursive: true });

console.log(`Starting isolated Node Vite server on port ${PORT}...`);
const viteProcess = spawn("node", ["./ui/node_modules/vite/bin/vite.js", "--config", "ui/qa-cont-sidebar.config.mjs"], {
  stdio: ["ignore", "pipe", "pipe"],
});

let serverReadyResolve;
let serverReadyReject;
const serverReadyPromise = new Promise((resolve, reject) => {
  serverReadyResolve = resolve;
  serverReadyReject = reject;
});

const startTimeout = setTimeout(() => {
  serverReadyReject(new Error("Timeout (10s) waiting for Vite server ready signal"));
}, 10000);

viteProcess.stdout.on("data", (data) => {
  const text = data.toString();
  if (text.includes("Local:") || text.includes("ready in")) {
    clearTimeout(startTimeout);
    serverReadyResolve(true);
  }
});
viteProcess.stderr.on("data", (data) => {
  const msg = data.toString();
  if (!msg.includes("warn - The `content` option in your Tailwind CSS configuration is missing or empty")) {
    console.error("[Vite stderr]", msg);
  }
});

await serverReadyPromise;

// Verify server connectivity with fetch
const probeResponse = await fetch(URL, { signal: AbortSignal.timeout(3000) });
if (!probeResponse.ok) {
  throw new Error(`Server returned status ${probeResponse.status} for ${URL}`);
}
console.log("Vite server confirmed ready (status 200)");

let webview;
const evidenceLog = [];
const teardownReceipt = {
  serverPid: viteProcess.pid,
  serverExited: false,
  serverExitCode: null,
  serverExitSignal: null,
  portFreed: false,
  webviewClosed: false,
  cacheCleaned: false,
};

function logEvidence(phase, assertion, pass, details) {
  const record = { phase, assertion, pass: Boolean(pass), details };
  evidenceLog.push(record);
  console.log(`[${pass ? "PASS" : "FAIL"}] [${phase}] ${assertion}`, details ? JSON.stringify(details) : "");
  if (!pass) {
    console.error("ASSERTION FAILED:", phase, assertion, details);
    throw new Error(`Assertion failed in ${phase}: ${assertion}`);
  }
}

async function verifyPortFreed(port, maxWaitMs = 3000) {
  const startTime = Date.now();
  while (Date.now() - startTime < maxWaitMs) {
    const isFree = await new Promise((resolve) => {
      const client = net.connect({ port, host: "127.0.0.1" });
      client.on("connect", () => {
        client.destroy();
        resolve(false);
      });
      client.on("error", () => {
        resolve(true);
      });
    });
    if (isFree) return true;
    await new Promise((r) => setTimeout(r, 50));
  }
  return false;
}

try {
  console.log("Opening Bun.WebView (1024x768)...");
  webview = new Bun.WebView({ width: 1024, height: 768 });
  await webview.navigate(URL);

  // Await deterministic app readiness signal from React mount
  console.log("Awaiting DOM / React readiness signal...");
  await webview.evaluate(`
    new Promise((resolve, reject) => {
      if (window.__QA_READY__) return resolve(true);
      const timer = setTimeout(() => reject(new Error("Timeout waiting for QA app mount")), 5000);
      window.addEventListener("qa:ready", () => {
        clearTimeout(timer);
        resolve(true);
      }, { once: true });
    })
  `);
  console.log("React QA harness mounted.");

  // Helper to get state
  async function getState() {
    const json = await webview.evaluate(`JSON.stringify(window.__QA__.getState())`);
    return JSON.parse(json);
  }

  // --- Scenario 1: Initial State (Active orca-local has 2 tabs: 1 terminal + 1 browser) ---
  console.log("\n=== Scenario 1: Initial Render (Active orca-local with tabs) ===");
  const s1 = await getState();
  logEvidence("Scenario 1", "orca-local has 2 initial live tabs", s1.liveTabCount === 2, { liveTabs: s1.liveTabs });
  logEvidence("Scenario 1", "orca-local is expanded", s1.orcaLocalExpanded === true, { orcaLocalExpanded: s1.orcaLocalExpanded });
  logEvidence("Scenario 1", "orca-local worktree list rendered in DOM", s1.orcaLocalHasWorktreeList === true, { hasList: s1.orcaLocalHasWorktreeList });
  logEvidence("Scenario 1", "emptyWorkspaceIds contains neither workspace", !s1.emptyWorkspaceIds.includes("orca-local") && !s1.emptyWorkspaceIds.includes("orca-parked"), { emptyWorkspaceIds: s1.emptyWorkspaceIds });

  const shot1 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "01-initial-active-with-tabs.png"), shot1);
  console.log("Saved screenshot: 01-initial-active-with-tabs.png");

  // --- Scenario 2: CLOSE_TAB for Terminal Tab (term-1), Browser Tab remains ---
  console.log("\n=== Scenario 2: Close Terminal Tab (term-1) via workspaceReducer ===");
  await webview.evaluate(`window.__QA__.closeTab("term-1")`);
  const s2 = await getState();
  logEvidence("Scenario 2", "orca-local has 1 tab left (browser-1)", s2.liveTabCount === 1 && s2.liveTabs[0].id === "browser-1", { liveTabs: s2.liveTabs });
  logEvidence("Scenario 2", "orca-local remains expanded while browser tab open", s2.orcaLocalExpanded === true, { orcaLocalExpanded: s2.orcaLocalExpanded });
  logEvidence("Scenario 2", "orca-local worktree list still rendered", s2.orcaLocalHasWorktreeList === true, { hasList: s2.orcaLocalHasWorktreeList });
  logEvidence("Scenario 2", "emptyWorkspaceIds still does not contain orca-local", !s2.emptyWorkspaceIds.includes("orca-local"), { emptyWorkspaceIds: s2.emptyWorkspaceIds });

  const shot2 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "02-terminal-tab-closed-browser-remains.png"), shot2);
  console.log("Saved screenshot: 02-terminal-tab-closed-browser-remains.png");

  // --- Scenario 3: CLOSE_TAB for Last Tab (browser-1) -> Empty Worktree Collapse ---
  console.log("\n=== Scenario 3: Close Last Tab (browser-1) -> Automatic Collapse ===");
  await webview.evaluate(`window.__QA__.closeTab("browser-1")`);
  const s3 = await getState();
  logEvidence("Scenario 3", "orca-local has 0 tabs", s3.liveTabCount === 0, { liveTabs: s3.liveTabs });
  logEvidence("Scenario 3", "emptyWorkspaceIds now contains orca-local", s3.emptyWorkspaceIds.includes("orca-local"), { emptyWorkspaceIds: s3.emptyWorkspaceIds });
  logEvidence("Scenario 3", "orca-local automatically collapsed (aria-expanded=false)", s3.orcaLocalExpanded === false, { orcaLocalExpanded: s3.orcaLocalExpanded });
  logEvidence("Scenario 3", "orca-local worktree list removed from DOM", s3.orcaLocalHasWorktreeList === false, { hasList: s3.orcaLocalHasWorktreeList });

  const shot3 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "03-last-tab-closed-collapsed.png"), shot3);
  console.log("Saved screenshot: 03-last-tab-closed-collapsed.png");

  // --- Scenario 4: User clicks Project Chevron on Empty Workspace ---
  console.log("\n=== Scenario 4: Click Chevron on Empty Workspace ===");
  await webview.evaluate(`window.__QA__.clickChevron("orca-local")`);
  const s4 = await getState();
  logEvidence("Scenario 4", "orca-local remains collapsed after chevron click", s4.orcaLocalExpanded === false, { orcaLocalExpanded: s4.orcaLocalExpanded });
  logEvidence("Scenario 4", "orca-local worktree list remains absent", s4.orcaLocalHasWorktreeList === false, { hasList: s4.orcaLocalHasWorktreeList });

  const shot4 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "04-chevron-click-while-empty-remains-collapsed.png"), shot4);
  console.log("Saved screenshot: 04-chevron-click-while-empty-remains-collapsed.png");

  // --- Scenario 5: User clicks Project Title on Empty Workspace ---
  console.log("\n=== Scenario 5: Click Title on Empty Workspace ===");
  await webview.evaluate(`window.__QA__.clickTitle("orca-local")`);
  const s5 = await getState();
  logEvidence("Scenario 5", "orca-local remains collapsed after title click", s5.orcaLocalExpanded === false, { orcaLocalExpanded: s5.orcaLocalExpanded });
  logEvidence("Scenario 5", "orca-local worktree list remains absent", s5.orcaLocalHasWorktreeList === false, { hasList: s5.orcaLocalHasWorktreeList });

  const shot5 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "05-title-click-while-empty-remains-collapsed.png"), shot5);
  console.log("Saved screenshot: 05-title-click-while-empty-remains-collapsed.png");

  // --- Scenario 6: Reopen Terminal Tab via workspaceReducer ADD_TAB_WITH_SESSION ---
  console.log("\n=== Scenario 6: Reopen Terminal Tab -> Automatic Expansion Permitted ===");
  await webview.evaluate(`window.__QA__.addTerminalTab({ id: "term-2", label: "Terminal 2" })`);
  const s6 = await getState();
  logEvidence("Scenario 6", "orca-local has 1 live tab", s6.liveTabCount === 1, { liveTabs: s6.liveTabs });
  logEvidence("Scenario 6", "emptyWorkspaceIds no longer contains orca-local", !s6.emptyWorkspaceIds.includes("orca-local"), { emptyWorkspaceIds: s6.emptyWorkspaceIds });
  logEvidence("Scenario 6", "orca-local expansion restored (aria-expanded=true)", s6.orcaLocalExpanded === true, { orcaLocalExpanded: s6.orcaLocalExpanded });
  logEvidence("Scenario 6", "orca-local worktree list restored in DOM", s6.orcaLocalHasWorktreeList === true, { hasList: s6.orcaLocalHasWorktreeList });

  const shot6 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "06-reopen-terminal-tab-restores-expansion.png"), shot6);
  console.log("Saved screenshot: 06-reopen-terminal-tab-restores-expansion.png");

  // --- Scenario 7: Close to Empty, Click Multiple Times, Reopen Browser Tab ---
  console.log("\n=== Scenario 7: Multi-Click While Empty, Then Reopen Browser Tab ===");
  await webview.evaluate(`window.__QA__.closeTab("term-2")`);
  // Click chevron 3 times while empty
  await webview.evaluate(`(() => {
    window.__QA__.clickChevron("orca-local");
    window.__QA__.clickTitle("orca-local");
    window.__QA__.clickChevron("orca-local");
  })()`);
  const s7Mid = await getState();
  logEvidence("Scenario 7", "orca-local remains collapsed after multiple clicks while empty", s7Mid.orcaLocalExpanded === false, { orcaLocalExpanded: s7Mid.orcaLocalExpanded });

  // Reopen browser tab
  await webview.evaluate(`window.__QA__.addBrowserTab({ id: "browser-2", label: "Docs", url: "https://example.com" })`);
  const s7 = await getState();
  logEvidence("Scenario 7", "browser tab reopened", s7.liveTabCount === 1 && s7.liveTabs[0].id === "browser-2", { liveTabs: s7.liveTabs });
  logEvidence("Scenario 7", "emptyWorkspaceIds no longer contains orca-local", !s7.emptyWorkspaceIds.includes("orca-local"), { emptyWorkspaceIds: s7.emptyWorkspaceIds });
  logEvidence("Scenario 7", "orca-local expansion restored after browser tab addition", s7.orcaLocalExpanded === true, { orcaLocalExpanded: s7.orcaLocalExpanded });
  logEvidence("Scenario 7", "orca-local worktree list restored in DOM", s7.orcaLocalHasWorktreeList === true, { hasList: s7.orcaLocalHasWorktreeList });

  const shot7 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "07-browser-tab-reopen-after-multiple-clicks.png"), shot7);
  console.log("Saved screenshot: 07-browser-tab-reopen-after-multiple-clicks.png");

  // --- Scenario 8: Parked Workspace State (orca-parked) ---
  console.log("\n=== Scenario 8: Parked Workspace Tab Closure, Collapse, and Restoration ===");
  // Step 1: orca-parked currently has 1 parked tab
  const s8Init = await getState();
  logEvidence("Scenario 8", "orca-parked not in emptyWorkspaceIds while having parked tab", !s8Init.emptyWorkspaceIds.includes("orca-parked"), { emptyWorkspaceIds: s8Init.emptyWorkspaceIds });

  // Step 2: Close parked tab
  await webview.evaluate(`window.__QA__.closeParkedTab("orca-parked")`);
  const s8Empty = await getState();
  logEvidence("Scenario 8", "orca-parked added to emptyWorkspaceIds after closing parked tab", s8Empty.emptyWorkspaceIds.includes("orca-parked"), { emptyWorkspaceIds: s8Empty.emptyWorkspaceIds });

  // Step 3: Try to expand parked empty workspace via chevron click
  await webview.evaluate(`window.__QA__.clickChevron("orca-parked")`);
  const s8Click = await getState();
  logEvidence("Scenario 8", "orca-parked remains collapsed after chevron click while empty", s8Click.orcaParkedExpanded === false, { orcaParkedExpanded: s8Click.orcaParkedExpanded });
  logEvidence("Scenario 8", "orca-parked worktree list not in DOM", s8Click.orcaParkedHasWorktreeList === false, { hasList: s8Click.orcaParkedHasWorktreeList });

  // Step 4: Add tab back to parked workspace
  await webview.evaluate(`window.__QA__.addParkedTab("orca-parked")`);
  const s8Restored = await getState();
  logEvidence("Scenario 8", "orca-parked removed from emptyWorkspaceIds after adding tab", !s8Restored.emptyWorkspaceIds.includes("orca-parked"), { emptyWorkspaceIds: s8Restored.emptyWorkspaceIds });
  logEvidence("Scenario 8", "orca-parked automatically restored to expanded state", s8Restored.orcaParkedExpanded === true, { orcaParkedExpanded: s8Restored.orcaParkedExpanded });
  logEvidence("Scenario 8", "orca-parked worktree list rendered in DOM", s8Restored.orcaParkedHasWorktreeList === true, { hasList: s8Restored.orcaParkedHasWorktreeList });

  // Step 5: Normal user toggling permitted now that tabs exist
  await webview.evaluate(`window.__QA__.clickChevron("orca-parked")`);
  const s8ToggledCollapsed = await getState();
  logEvidence("Scenario 8", "orca-parked collapsed after chevron click when non-empty", s8ToggledCollapsed.orcaParkedExpanded === false, { orcaParkedExpanded: s8ToggledCollapsed.orcaParkedExpanded });

  await webview.evaluate(`window.__QA__.clickChevron("orca-parked")`);
  const s8ToggledExpanded = await getState();
  logEvidence("Scenario 8", "orca-parked re-expanded after second chevron click", s8ToggledExpanded.orcaParkedExpanded === true, { orcaParkedExpanded: s8ToggledExpanded.orcaParkedExpanded });

  const shot8 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "08-parked-workspace-empty-and-restore.png"), shot8);
  console.log("Saved screenshot: 08-parked-workspace-empty-and-restore.png");

  console.log("\nAll 8 Browser QA scenarios completed with 100% binary PASS!");
} finally {
  console.log("\n=== Teardown & Resource Cleanup ===");
  if (webview) {
    try {
      webview.close();
      teardownReceipt.webviewClosed = true;
      console.log("Bun.WebView closed successfully.");
    } catch (err) {
      console.error("Error closing Bun.WebView:", err);
    }
  }

  // Terminate Vite process
  if (viteProcess && !viteProcess.killed) {
    const exitPromise = new Promise((resolve) => {
      viteProcess.on("exit", (code, signal) => {
        teardownReceipt.serverExited = true;
        teardownReceipt.serverExitCode = code;
        teardownReceipt.serverExitSignal = signal;
        resolve();
      });
    });

    viteProcess.kill("SIGTERM");
    const killTimeout = setTimeout(() => {
      try {
        viteProcess.kill("SIGKILL");
      } catch {}
    }, 2000);

    await exitPromise;
    clearTimeout(killTimeout);
    console.log(`Node Vite process (PID ${viteProcess.pid}) exited.`);
  }

  // Verify port is freed
  teardownReceipt.portFreed = await verifyPortFreed(PORT, 3000);
  console.log(`Port ${PORT} freed: ${teardownReceipt.portFreed}`);

  // Clean Vite cache
  try {
    await fs.rm(CACHE_DIR, { recursive: true, force: true });
    teardownReceipt.cacheCleaned = true;
    console.log(`Isolated cache directory ${CACHE_DIR} removed.`);
  } catch (err) {
    console.error("Error cleaning Vite cache:", err);
  }

  // Write evidence record
  await fs.writeFile(
    path.resolve(process.cwd(), "docs/session-continuation-20260908/sidebar/qa-evidence.json"),
    JSON.stringify({ evidenceLog, teardownReceipt }, null, 2),
    "utf8",
  );
  console.log("Evidence and teardown receipt written to docs/session-continuation-20260908/sidebar/qa-evidence.json");
}
