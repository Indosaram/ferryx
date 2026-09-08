import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import net from "node:net";

const PORT = 5212;
const URL = `http://127.0.0.1:${PORT}/qa-cont-terminal.html`;
const SCREENSHOT_DIR = path.resolve(process.cwd(), "docs/session-continuation-20260908/terminal/screenshots");

await fs.mkdir(SCREENSHOT_DIR, { recursive: true });

console.log("Starting isolated Node Vite server on port", PORT);
const viteProcess = spawn("node", ["./ui/node_modules/vite/bin/vite.js", "--config", "ui/qa-cont-terminal.config.mjs"], {
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
  console.error("[Vite stderr]", data.toString());
});

await serverReadyPromise;

// Verify server connectivity with single bounded fetch
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

  // Await initial terminal bounds IPC presentation
  console.log("Awaiting initial terminal attachment & bounds presentation...");
  await webview.evaluate(`window.__QA__.waitForIpc("cmd_native_terminal_set_bounds", 4000)`);

  // --- Scenario 1: Initial Idle State (Full Height, No Strips, Handle Hidden) ---
  console.log("\n=== Scenario 1: Initial Idle State ===");
  const idleStateJson = await webview.evaluate(`
    (() => {
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const toolbar = document.querySelector('[data-testid="pane-toolbar"]');
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');
      const handleBacking = document.querySelector('[data-testid="terminal-pane-handle-backing"]');
      const bottomBacking = document.querySelector('[data-testid="terminal-pane-bottom-backing"]');
      const errorBacking = document.querySelector('[data-testid="native-terminal-error-backing"]');

      const leafRect = leaf ? leaf.getBoundingClientRect() : null;
      const termRect = terminalPane ? terminalPane.getBoundingClientRect() : null;

      return JSON.stringify({
        leafHeight: leafRect ? leafRect.height : 0,
        termHeight: termRect ? termRect.height : 0,
        termTop: termRect ? termRect.top : 0,
        leafTop: leafRect ? leafRect.top : 0,
        termMarginTop: terminalPane ? terminalPane.style.marginTop : null,
        termInlineHeight: terminalPane ? terminalPane.style.height : null,
        termClassList: terminalPane ? Array.from(terminalPane.classList) : [],
        toolbarOpacity0: toolbar ? toolbar.classList.contains("opacity-0") : false,
        toolbarPointerEventsNone: toolbar ? toolbar.classList.contains("pointer-events-none") : false,
        handleBackingNull: handleBacking === null,
        bottomBackingNull: bottomBacking === null,
        errorBackingNull: errorBacking === null,
        presented: terminalPane ? terminalPane.getAttribute("data-native-terminal-presented") : null,
      });
    })()
  `);
  const s1 = JSON.parse(idleStateJson);

  logEvidence("Scenario 1", "No top handle backing strip in DOM", s1.handleBackingNull, { handleBackingNull: s1.handleBackingNull });
  logEvidence("Scenario 1", "No bottom overlay backing strip in DOM", s1.bottomBackingNull, { bottomBackingNull: s1.bottomBackingNull });
  logEvidence("Scenario 1", "No error backing strip in DOM", s1.errorBackingNull, { errorBackingNull: s1.errorBackingNull });
  logEvidence("Scenario 1", "Handle toolbar hidden initially (opacity-0, pointer-events-none)", s1.toolbarOpacity0 && s1.toolbarPointerEventsNone, { opacity0: s1.toolbarOpacity0, pointerEventsNone: s1.toolbarPointerEventsNone });
  logEvidence("Scenario 1", "Terminal pane has h-full class and empty inline margins/height", s1.termMarginTop === "" && s1.termInlineHeight === "" && s1.termClassList.includes("h-full"), { marginTop: s1.termMarginTop, inlineHeight: s1.termInlineHeight, hasHFull: s1.termClassList.includes("h-full") });
  logEvidence("Scenario 1", "Terminal height matches leaf height exactly (>600px)", Math.abs(s1.termHeight - s1.leafHeight) < 1 && s1.termHeight > 600, { termHeight: s1.termHeight, leafHeight: s1.leafHeight });
  logEvidence("Scenario 1", "Terminal presentation active", s1.presented === "true", { presented: s1.presented });

  const shot1 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "01-idle-full-height.png"), shot1);
  console.log("Saved screenshot 01-idle-full-height.png");

  // --- Scenario 2: Hover inside 16px Hotspot (y <= 16px) with MutationObserver ---
  console.log("\n=== Scenario 2: Hover inside 16px Hotspot ===");
  const hoverStateJson = await webview.evaluate(`
    new Promise((resolve, reject) => {
      const toolbar = document.querySelector('[data-testid="pane-toolbar"]');
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');

      const timeout = setTimeout(() => reject(new Error("Timeout waiting for toolbar opacity-100 transition")), 3000);
      const observer = new MutationObserver(() => {
        if (toolbar.classList.contains("opacity-100")) {
          clearTimeout(timeout);
          observer.disconnect();

          const leafRect = leaf.getBoundingClientRect();
          const toolbarRect = toolbar.getBoundingClientRect();
          const termRect = terminalPane.getBoundingClientRect();
          const splitRightBtn = toolbar.querySelector('[aria-label="Split pane right"]');
          const btnRect = splitRightBtn ? splitRightBtn.getBoundingClientRect() : null;

          resolve(JSON.stringify({
            leafHeight: leafRect.height,
            termHeight: termRect.height,
            toolbarOpacity100: toolbar.classList.contains("opacity-100"),
            toolbarPointerEventsAuto: toolbar.classList.contains("pointer-events-auto"),
            toolbarHeight: toolbarRect.height,
            btnWidth: btnRect ? btnRect.width : 0,
            btnHeight: btnRect ? btnRect.height : 0,
          }));
        }
      });
      observer.observe(toolbar, { attributes: true, attributeFilter: ["class"] });

      // Dispatch mousemove within 16px hotspot
      const rect = leaf.getBoundingClientRect();
      leaf.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true,
        cancelable: true,
        clientX: rect.left + 300,
        clientY: rect.top + 8,
      }));
    })
  `);
  const s2 = JSON.parse(hoverStateJson);

  logEvidence("Scenario 2", "Handle toolbar becomes visible and interactive on hover", s2.toolbarOpacity100 && s2.toolbarPointerEventsAuto, { opacity100: s2.toolbarOpacity100, pointerEventsAuto: s2.toolbarPointerEventsAuto });
  logEvidence("Scenario 2", "Handle toolbar height is exactly 12px (h-3)", Math.round(s2.toolbarHeight) === 12, { toolbarHeight: s2.toolbarHeight });
  logEvidence("Scenario 2", "Buttons in toolbar maintain overflowing 20px size (size-5)", Math.round(s2.btnHeight) === 20 && Math.round(s2.btnWidth) === 20, { btnWidth: s2.btnWidth, btnHeight: s2.btnHeight });
  logEvidence("Scenario 2", "Terminal height unchanged during hover (no reflow)", Math.abs(s2.termHeight - s2.leafHeight) < 1, { termHeight: s2.termHeight, leafHeight: s2.leafHeight });

  const shot2 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "02-hover-handle-overlay.png"), shot2);
  console.log("Saved screenshot 02-hover-handle-overlay.png");

  // --- Scenario 3: Handle Reach / Drag Surface Interaction ---
  console.log("\n=== Scenario 3: Handle Reach / Interaction ===");
  const handleInteractionJson = await webview.evaluate(`
    (() => {
      window.__QA__.clearIpcCalls();
      const toolbar = document.querySelector('[data-testid="pane-toolbar"]');
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');

      toolbar.dispatchEvent(new PointerEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        pointerId: 1,
      }));

      const ipc = window.__QA__.getIpcCalls();
      const mouseIpcDispatched = ipc.some(c => c.command === "cmd_native_terminal_mouse");

      return JSON.stringify({
        mouseIpcDispatched,
        termMarginTop: terminalPane ? terminalPane.style.marginTop : null,
        termInlineHeight: terminalPane ? terminalPane.style.height : null,
      });
    })()
  `);
  const s3 = JSON.parse(handleInteractionJson);

  logEvidence("Scenario 3", "Pressing drag handle does not dispatch terminal mouse IPC", !s3.mouseIpcDispatched, { mouseIpcDispatched: s3.mouseIpcDispatched });
  logEvidence("Scenario 3", "Terminal maintains full-height inline styles during handle press", s3.termMarginTop === "" && s3.termInlineHeight === "", { marginTop: s3.termMarginTop, inlineHeight: s3.termInlineHeight });

  // --- Scenario 4: Pointer Leaves Hotspot (y > 16px) with MutationObserver ---
  console.log("\n=== Scenario 4: Pointer Leaves Hotspot ===");
  const leaveStateJson = await webview.evaluate(`
    new Promise((resolve, reject) => {
      const toolbar = document.querySelector('[data-testid="pane-toolbar"]');
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');

      const timeout = setTimeout(() => reject(new Error("Timeout waiting for toolbar opacity-0 transition")), 3000);
      const observer = new MutationObserver(() => {
        if (toolbar.classList.contains("opacity-0")) {
          clearTimeout(timeout);
          observer.disconnect();

          const leafRect = leaf.getBoundingClientRect();
          const termRect = terminalPane.getBoundingClientRect();

          resolve(JSON.stringify({
            leafHeight: leafRect.height,
            termHeight: termRect.height,
            toolbarOpacity0: toolbar.classList.contains("opacity-0"),
            toolbarPointerEventsNone: toolbar.classList.contains("pointer-events-none"),
          }));
        }
      });
      observer.observe(toolbar, { attributes: true, attributeFilter: ["class"] });

      // Dispatch mousemove outside 16px hotspot
      const rect = leaf.getBoundingClientRect();
      leaf.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true,
        cancelable: true,
        clientX: rect.left + 300,
        clientY: rect.top + 40,
      }));
    })
  `);
  const s4 = JSON.parse(leaveStateJson);

  logEvidence("Scenario 4", "Handle toolbar hides when pointer leaves hotspot", s4.toolbarOpacity0 && s4.toolbarPointerEventsNone, { opacity0: s4.toolbarOpacity0, pointerEventsNone: s4.toolbarPointerEventsNone });
  logEvidence("Scenario 4", "Terminal height unchanged after hover leaves", Math.abs(s4.termHeight - s4.leafHeight) < 1, { termHeight: s4.termHeight, leafHeight: s4.leafHeight });

  const shot3 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "03-after-hover.png"), shot3);
  console.log("Saved screenshot 03-after-hover.png");

  // --- Scenario 5: Attention Border Overlay (Needs Attention = true) with MutationObserver ---
  console.log("\n=== Scenario 5: Attention Border Overlay ===");
  const attentionJson = await webview.evaluate(`
    new Promise((resolve, reject) => {
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');

      const timeout = setTimeout(() => reject(new Error("Timeout waiting for attention frame overlay in DOM")), 3000);
      const observer = new MutationObserver(() => {
        const bottom = leaf.querySelector('[data-testid="attention-frame-bottom"]');
        const cornerL = leaf.querySelector('[data-testid="attention-frame-corner-left"]');
        const cornerR = leaf.querySelector('[data-testid="attention-frame-corner-right"]');
        if (bottom && cornerL && cornerR) {
          clearTimeout(timeout);
          observer.disconnect();

          const leafRect = leaf.getBoundingClientRect();
          const termRect = terminalPane.getBoundingClientRect();
          const cornerLRect = cornerL.getBoundingClientRect();

          resolve(JSON.stringify({
            bottomPresent: true,
            cornerLPresent: true,
            cornerRPresent: true,
            bottomPointerEventsNone: bottom.classList.contains("pointer-events-none"),
            cornerLHeight: cornerLRect.height,
            leafHeight: leafRect.height,
            termHeight: termRect.height,
            termMarginTop: terminalPane.style.marginTop,
            termInlineHeight: terminalPane.style.height,
          }));
        }
      });
      observer.observe(leaf, { childList: true, subtree: true });

      window.__QA__.toggleAttention();
    })
  `);
  const s5 = JSON.parse(attentionJson);

  logEvidence("Scenario 5", "Attention frame elements present with pointer-events-none", s5.bottomPresent && s5.cornerLPresent && s5.cornerRPresent && s5.bottomPointerEventsNone, { bottomPresent: s5.bottomPresent, cornerL: s5.cornerLPresent, cornerR: s5.cornerRPresent });
  logEvidence("Scenario 5", "Attention corner has fixed 20px height (h-5)", Math.round(s5.cornerLHeight) === 20, { cornerLHeight: s5.cornerLHeight });
  logEvidence("Scenario 5", "Attention frame does not reserve layout space (terminal retains full height)", Math.abs(s5.termHeight - s5.leafHeight) < 1 && s5.termMarginTop === "" && s5.termInlineHeight === "", { termHeight: s5.termHeight, leafHeight: s5.leafHeight });

  const shot4 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "04-attention-border-overlay.png"), shot4);
  console.log("Saved screenshot 04-attention-border-overlay.png");

  // Toggle attention off and await its removal
  await webview.evaluate(`
    new Promise((resolve, reject) => {
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const timeout = setTimeout(() => reject(new Error("Timeout waiting for attention frame removal")), 3000);
      const observer = new MutationObserver(() => {
        if (!leaf.querySelector('[data-testid="attention-frame-bottom"]')) {
          clearTimeout(timeout);
          observer.disconnect();
          resolve(true);
        }
      });
      observer.observe(leaf, { childList: true, subtree: true });
      window.__QA__.toggleAttention();
    })
  `);

  // --- Scenario 6: Running DAG Badge Overlay with MutationObserver ---
  console.log("\n=== Scenario 6: Running DAG Badge Overlay ===");
  const dagJson = await webview.evaluate(`
    new Promise((resolve, reject) => {
      const surface = document.querySelector('[data-testid="terminal-pane-surface"]');
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');

      const timeout = setTimeout(() => reject(new Error("Timeout waiting for DAG badge in DOM")), 3000);
      const observer = new MutationObserver(() => {
        const badge = surface.querySelector('[data-testid="dag-pane-badge"]');
        if (badge) {
          clearTimeout(timeout);
          observer.disconnect();

          const bottomBacking = document.querySelector('[data-testid="terminal-pane-bottom-backing"]');
          const leafRect = leaf.getBoundingClientRect();
          const termRect = terminalPane.getBoundingClientRect();

          resolve(JSON.stringify({
            badgePresent: true,
            badgeParentIsSurface: badge.parentElement === surface,
            bottomBackingNull: bottomBacking === null,
            leafHeight: leafRect.height,
            termHeight: termRect.height,
          }));
        }
      });
      observer.observe(surface, { childList: true, subtree: true });

      window.__QA__.toggleDagBadge();
    })
  `);
  const s6 = JSON.parse(dagJson);

  logEvidence("Scenario 6", "Running DAG badge overlays terminal surface as DOM sibling", s6.badgePresent && s6.badgeParentIsSurface, { badgePresent: s6.badgePresent, badgeParentIsSurface: s6.badgeParentIsSurface });
  logEvidence("Scenario 6", "No permanent bottom backing strip behind DAG badge", s6.bottomBackingNull, { bottomBackingNull: s6.bottomBackingNull });
  logEvidence("Scenario 6", "Terminal height matches leaf height with DAG badge present", Math.abs(s6.termHeight - s6.leafHeight) < 1, { termHeight: s6.termHeight, leafHeight: s6.leafHeight });

  const shot5 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "05-running-dag-badge.png"), shot5);
  console.log("Saved screenshot 05-running-dag-badge.png");

  // --- Scenario 7: Terminal Input & Mouse Interaction (Authoritative action: "Press", button: "Left") ---
  console.log("\n=== Scenario 7: Terminal Input & Interaction ===");
  const inputJson = await webview.evaluate(`
    (() => {
      window.__QA__.clearIpcCalls();
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');
      const termRect = terminalPane.getBoundingClientRect();

      const ipcWaitPromise = window.__QA__.waitForIpc("cmd_native_terminal_mouse", 3000);

      // Dispatch pointerdown in the terminal body (y = top + 100px)
      terminalPane.dispatchEvent(new PointerEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        clientX: termRect.left + 150,
        clientY: termRect.top + 100,
        button: 0,
        pointerId: 1,
      }));

      return ipcWaitPromise.then((call) => {
        return JSON.stringify({
          mouseCallPresent: true,
          mouseCallAction: call.args?.event?.action,
          mouseCallButton: call.args?.event?.button,
        });
      });
    })()
  `);
  const s7 = JSON.parse(inputJson);

  const actionMatches = s7.mouseCallPresent && s7.mouseCallAction === "Press";
  const buttonMatches = s7.mouseCallPresent && s7.mouseCallButton === "Left";
  logEvidence(
    "Scenario 7",
    "Pointer event in terminal body routes to native terminal mouse IPC (action=Press, button=Left)",
    actionMatches && buttonMatches,
    { mouseCallPresent: s7.mouseCallPresent, action: s7.mouseCallAction, button: s7.mouseCallButton },
  );

  // --- Scenario 8: Foreign Regression Check - Error State / Retention Overlay ---
  console.log("\n=== Scenario 8: Foreign Regression Check - Error-Overlay Retention (Commit 851b763) ===");
  const errStateJson = await webview.evaluate(`
    new Promise((resolve, reject) => {
      const container = document.querySelector('#qa-terminal-container');
      const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');

      const timeout = setTimeout(() => reject(new Error("Timeout waiting for recovery alert in DOM")), 4000);
      const observer = new MutationObserver(() => {
        const alertBtn = container.querySelector('[role="alert"]');
        if (alertBtn) {
          clearTimeout(timeout);
          observer.disconnect();

          const errorBacking = document.querySelector('[data-testid="native-terminal-error-backing"]');

          resolve(JSON.stringify({
            alertPresent: true,
            alertZIndex: alertBtn.classList.contains("z-50"),
            errorBackingNull: errorBacking === null,
            terminalPresentedAttr: terminalPane ? terminalPane.getAttribute("data-native-terminal-presented") : null,
            terminalVisibleAttr: terminalPane ? terminalPane.getAttribute("data-native-terminal-visible") : null,
          }));
        }
      });
      observer.observe(container, { childList: true, subtree: true });

      window.__QA__.triggerBoundsFailure();
    })
  `);
  const s8 = JSON.parse(errStateJson);

  logEvidence("Scenario 8", "Recovery alert rendered with z-50 over terminal (foreign regression check)", s8.alertPresent && s8.alertZIndex, { alertPresent: s8.alertPresent, alertZIndex: s8.alertZIndex });
  logEvidence("Scenario 8", "Last terminal frame uncovered: native-terminal-error-backing is null (foreign regression check)", s8.errorBackingNull, { errorBackingNull: s8.errorBackingNull });
  logEvidence("Scenario 8", "Terminal visible state preserved during error display (foreign regression check)", s8.terminalVisibleAttr === "true", { visibleAttr: s8.terminalVisibleAttr });

  const shot6 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "06-error-retry-overlay.png"), shot6);
  console.log("Saved screenshot 06-error-retry-overlay.png");

} finally {
  if (webview) {
    console.log("Closing Bun.WebView...");
    webview.close();
    teardownReceipt.webviewClosed = true;
  }

  console.log("Terminating Vite child process...");
  await new Promise((resolve) => {
    viteProcess.on("exit", (code, signal) => {
      teardownReceipt.serverExited = true;
      teardownReceipt.serverExitCode = code;
      teardownReceipt.serverExitSignal = signal;
      resolve();
    });
    viteProcess.kill("SIGKILL");
  });

  const freed = await verifyPortFreed(PORT);
  teardownReceipt.portFreed = freed;
  console.log(`Port ${PORT} freed:`, freed);

  try {
    await fs.rm(path.resolve(process.cwd(), "ui/.vite-qa-cont-terminal"), { recursive: true, force: true });
    teardownReceipt.cacheCleaned = true;
  } catch {}

  const allPass = evidenceLog.every((e) => e.pass);
  console.log("\n==========================================");
  console.log(`QA Result: ${allPass ? "ALL PASS" : "FAILURES DETECTED"} (${evidenceLog.filter(e => e.pass).length}/${evidenceLog.length})`);
  console.log("==========================================\n");

  await fs.writeFile(
    path.resolve(process.cwd(), "docs/session-continuation-20260908/terminal/qa-evidence.json"),
    JSON.stringify({ timestamp: new Date().toISOString(), allPass, evidenceLog, teardownReceipt }, null, 2),
  );

  console.log("Teardown receipt:", JSON.stringify(teardownReceipt, null, 2));
}

console.log("Browser QA runner completed successfully.");
