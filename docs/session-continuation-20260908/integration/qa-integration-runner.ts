import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import net from "node:net";

const PORT = 5214;
const URL = `http://127.0.0.1:${PORT}/qa-integration.html`;
const BASE_DIR = "/Users/indo/code/project/orca-lite/docs/session-continuation-20260908/integration";
const SCREENSHOT_DIR = path.join(BASE_DIR, "screenshots");
const CACHE_DIR = path.join(BASE_DIR, ".vite-qa-integration");

await fs.mkdir(SCREENSHOT_DIR, { recursive: true });

console.log(`Starting isolated Node Vite server on port ${PORT}...`);
const viteProcess = spawn(
  "node",
  ["/Users/indo/code/project/orca-lite/ui/node_modules/vite/bin/vite.js", "--config", path.join(BASE_DIR, "qa-integration.config.mjs")],
  {
    cwd: BASE_DIR,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      NODE_PATH: "/Users/indo/code/project/orca-lite/ui/node_modules",
    },
  }
);

let serverReadyResolve: (v: boolean) => void;
let serverReadyReject: (err: Error) => void;
const serverReadyPromise = new Promise<boolean>((res, rej) => {
  serverReadyResolve = res;
  serverReadyReject = rej;
});

const startTimeout = setTimeout(() => {
  serverReadyReject(new Error("Timeout (10s) waiting for Vite server ready signal"));
}, 10000);

viteProcess.stdout?.on("data", (data: Buffer) => {
  const text = data.toString();
  if (text.includes("Local:") || text.includes("ready in")) {
    clearTimeout(startTimeout);
    serverReadyResolve(true);
  }
});
viteProcess.stderr?.on("data", (data: Buffer) => {
  const msg = data.toString();
  console.error("[Vite stderr]", msg);
});

await serverReadyPromise;

// Verify server connectivity
const probe = await fetch(URL, { signal: AbortSignal.timeout(3000) });
if (!probe.ok) throw new Error(`Server returned ${probe.status} for ${URL}`);
console.log("Vite server confirmed ready (status 200)");

let webview: any;
const evidenceLog: Array<{ phase: string; assertion: string; pass: boolean; details?: any }> = [];

function logEvidence(phase: string, assertion: string, pass: boolean, details?: any) {
  const record = { phase, assertion, pass: Boolean(pass), details };
  evidenceLog.push(record);
  console.log(`[${pass ? "PASS" : "FAIL"}] [${phase}] ${assertion}`, details ? JSON.stringify(details) : "");
  if (!pass) {
    throw new Error(`Assertion failed in ${phase}: ${assertion}`);
  }
}

async function verifyPortFreed(port: number, timeoutMs = 3000): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    const client = net.connect({ port, host: "127.0.0.1" });
    const timer = setTimeout(() => {
      client.destroy();
      resolve(false);
    }, timeoutMs);

    client.on("connect", () => {
      clearTimeout(timer);
      client.destroy();
      resolve(false);
    });

    client.on("error", () => {
      clearTimeout(timer);
      client.destroy();
      resolve(true);
    });
  });
}

try {
  console.log("Opening Bun.WebView (1024x768)...");
  webview = new (Bun as any).WebView({ width: 1024, height: 768 });
  await webview.navigate(URL);

  console.log("Awaiting app readiness...");
  try {
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
  } catch (err) {
    const errs = await webview.evaluate(`JSON.stringify(window.__ERRORS__ || [])`);
    console.error("Browser errors recorded:", errs);
    throw err;
  }
  console.log("Integration app mounted.");

  async function getState() {
    const json = await webview.evaluate(`JSON.stringify(window.__QA__.getState())`);
    return JSON.parse(json);
  }

  // --- Scenario 1: Initial Combined Idle State ---
  console.log("\n=== Scenario 1: Combined Idle Layout ===");
  const s1 = await getState();
  console.log("Scenario 1 state:", s1);
  logEvidence("Scenario 1", "Sidebar has orca-local worktrees rendered", s1.hasWorktreeList === true, { hasList: s1.hasWorktreeList });
  logEvidence("Scenario 1", "orca-local is initially expanded", s1.orcaLocalExpanded === true, { expanded: s1.orcaLocalExpanded });
  logEvidence("Scenario 1", "Terminal pane height matches leaf container", Math.abs(s1.terminalHeight - s1.leafHeight) < 1 && s1.terminalHeight > 500, { termH: s1.terminalHeight, leafH: s1.leafHeight });
  logEvidence("Scenario 1", "Top handle backing strip is null", s1.hasHandleBacking === false, { hasHandleBacking: s1.hasHandleBacking });
  logEvidence("Scenario 1", "Bottom overlay backing strip is null", s1.hasBottomBacking === false, { hasBottomBacking: s1.hasBottomBacking });
  logEvidence("Scenario 1", "Overlay toolbar hidden by default", s1.toolbarHidden === true, { toolbarHidden: s1.toolbarHidden });

  const shot1 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "scenario-01-combined-idle.png"), shot1);
  console.log("Saved scenario-01-combined-idle.png");

  // --- Scenario 2: Terminal Hover Handle Overlay (Deterministic transitionend / Animation completion) ---
  console.log("\n=== Scenario 2: Terminal Hover Handle Overlay ===");
  const s2Hover = await webview.evaluate(`
    new Promise((resolve, reject) => {
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const toolbar = document.querySelector('[data-testid="pane-toolbar"]');
      if (!leaf || !toolbar) return reject(new Error("leaf or toolbar not found"));

      const timeout = setTimeout(() => reject(new Error("Timeout waiting for toolbar opacity transition")), 2000);

      const checkFinished = () => {
        void window.getComputedStyle(toolbar).opacity;
        const anims = toolbar.getAnimations ? toolbar.getAnimations() : [];
        if (anims.length > 0) {
          Promise.all(anims.map(a => a.finished)).then(() => {
            clearTimeout(timeout);
            const opacity = window.getComputedStyle(toolbar).opacity;
            const rect = toolbar.getBoundingClientRect();
            resolve(JSON.stringify({
              opacity,
              height: rect.height,
              isVisible: toolbar.classList.contains("opacity-100"),
            }));
          }, reject);
        } else {
          // transitionend listener
          const onEnd = (e) => {
            if (e.propertyName === "opacity") {
              clearTimeout(timeout);
              toolbar.removeEventListener("transitionend", onEnd);
              const opacity = window.getComputedStyle(toolbar).opacity;
              const rect = toolbar.getBoundingClientRect();
              resolve(JSON.stringify({
                opacity,
                height: rect.height,
                isVisible: toolbar.classList.contains("opacity-100"),
              }));
            }
          };
          toolbar.addEventListener("transitionend", onEnd);
        }
      };

      checkFinished();

      // Dispatch mousemove within 16px hotspot
      const rect = leaf.getBoundingClientRect();
      leaf.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true,
        cancelable: true,
        clientX: rect.left + 200,
        clientY: rect.top + 8,
      }));
    })
  `);
  const s2 = JSON.parse(s2Hover);
  logEvidence("Scenario 2", "Handle toolbar becomes visible on hover (opacity > 0.9)", parseFloat(s2.opacity) > 0.9, { opacity: s2.opacity });
  logEvidence("Scenario 2", "Handle toolbar height is exactly 12px (h-3)", Math.round(s2.height) === 12, { height: s2.height });

  const shot2 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "scenario-02-terminal-handle-hover.png"), shot2);
  console.log("Saved scenario-02-terminal-handle-hover.png");

  // Reset hover deterministically before next scenario
  await webview.evaluate(`
    new Promise((resolve, reject) => {
      const leaf = document.querySelector('[data-testid="pane-leaf"]');
      const toolbar = document.querySelector('[data-testid="pane-toolbar"]');
      if (!leaf || !toolbar) return resolve(true);

      const timeout = setTimeout(() => resolve(true), 2000);
      const onEnd = (e) => {
        if (e.propertyName === "opacity") {
          clearTimeout(timeout);
          toolbar.removeEventListener("transitionend", onEnd);
          resolve(true);
        }
      };
      toolbar.addEventListener("transitionend", onEnd);

      const rect = leaf.getBoundingClientRect();
      leaf.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true,
        cancelable: true,
        clientX: rect.left + 200,
        clientY: rect.top + 50,
      }));
      void window.getComputedStyle(toolbar).opacity;
    })
  `);

  // --- Scenario 3: Close Last Tab -> Automatic Collapse (Deterministic transitionend on chevron) ---
  console.log("\n=== Scenario 3: Close Last Tab -> Sidebar Collapse ===");
  await webview.evaluate(`
    new Promise((resolve, reject) => {
      const chevron = document.querySelector('button[aria-label*="orca-local"][aria-expanded] svg');
      const timeout = setTimeout(() => resolve(true), 2000);

      if (chevron) {
        const onEnd = (e) => {
          if (e.propertyName === "transform") {
            clearTimeout(timeout);
            chevron.removeEventListener("transitionend", onEnd);
            resolve(true);
          }
        };
        chevron.addEventListener("transitionend", onEnd);
      }

      window.__QA__.closeTab("term-1").then(() => {
        if (chevron) {
          void window.getComputedStyle(chevron).transform;
          const anims = chevron.getAnimations ? chevron.getAnimations() : [];
          if (anims.length > 0) {
            Promise.all(anims.map(a => a.finished)).then(() => {
              clearTimeout(timeout);
              resolve(true);
            });
          }
        }
      });
    })
  `);

  const s3 = await getState();
  logEvidence("Scenario 3", "Tab count is 0", s3.tabCount === 0, { tabCount: s3.tabCount });
  logEvidence("Scenario 3", "orca-local is in emptyWorkspaceIds", s3.emptyWorkspaceIds.includes("orca-local"), { emptyWorkspaceIds: s3.emptyWorkspaceIds });
  logEvidence("Scenario 3", "orca-local aria-expanded is false", s3.orcaLocalExpanded === false, { expanded: s3.orcaLocalExpanded });
  logEvidence("Scenario 3", "orca-local worktree list is unmounted", s3.hasWorktreeList === false, { hasList: s3.hasWorktreeList });

  const shot3 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "scenario-03-sidebar-last-tab-collapsed.png"), shot3);
  console.log("Saved scenario-03-sidebar-last-tab-collapsed.png");

  // --- Scenario 4: Clicks on Empty Project Guarded (RequestAnimationFrame & computed style check) ---
  console.log("\n=== Scenario 4: Empty Project Clicks Guarded ===");
  await webview.evaluate(`
    new Promise((resolve) => {
      const chevron = document.querySelector('button[aria-label*="orca-local"][aria-expanded]');
      if (chevron) chevron.click();
      const title = document.querySelector('button[aria-label="orca-local"]');
      if (title) title.click();
      void document.body.offsetHeight;
      requestAnimationFrame(() => resolve(true));
    })
  `);

  const s4 = await getState();
  logEvidence("Scenario 4", "orca-local remains collapsed after clicks while empty", s4.orcaLocalExpanded === false, { expanded: s4.orcaLocalExpanded });
  logEvidence("Scenario 4", "Worktree list remains unmounted", s4.hasWorktreeList === false, { hasList: s4.hasWorktreeList });

  const shot4 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "scenario-04-sidebar-empty-clicks-guarded.png"), shot4);
  console.log("Saved scenario-04-sidebar-empty-clicks-guarded.png");

  // --- Scenario 5: Remote Directory Autocomplete Picker (MutationObserver on options) ---
  console.log("\n=== Scenario 5: Remote Directory Autocomplete Picker ===");
  await webview.evaluate(`window.__QA__.openAddProject()`);

  // Await dialog input element mounted in DOM
  await webview.evaluate(`
    new Promise((resolve, reject) => {
      const existing = document.querySelector('[data-testid="remote-repo-path-input"]');
      if (existing) return resolve(true);

      const timeout = setTimeout(() => reject(new Error("Timeout waiting for remote input mount")), 3000);
      const observer = new MutationObserver(() => {
        const input = document.querySelector('[data-testid="remote-repo-path-input"]');
        if (input) {
          clearTimeout(timeout);
          observer.disconnect();
          resolve(true);
        }
      });
      observer.observe(document.body, { childList: true, subtree: true });
    })
  `);

  // Type path with trailing backslash and await options mount via MutationObserver
  const s5State = await webview.evaluate(`
    new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("Timeout waiting for remote combobox options")), 3000);

      const observer = new MutationObserver(() => {
        const options = Array.from(document.querySelectorAll('[role="option"]')).map(o => o.textContent?.trim());
        if (options.length >= 2 && options.some(o => o.includes("ferryx"))) {
          clearTimeout(timeout);
          observer.disconnect();
          const input = document.querySelector('[data-testid="remote-repo-path-input"]');
          const goBtn = Array.from(document.querySelectorAll("button")).find(b => b.textContent?.trim() === "Go");
          const addFolderBtn = document.querySelector('[data-testid="add-project-confirm-remote"]');
          resolve(JSON.stringify({
            inputValue: input ? input.value : null,
            optionCount: options.length,
            options,
            hasGoBtn: Boolean(goBtn),
            confirmDisabled: addFolderBtn ? addFolderBtn.disabled : null,
          }));
        }
      });
      observer.observe(document.body, { childList: true, subtree: true });

      const input = document.querySelector('[data-testid="remote-repo-path-input"]');
      const nativeSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
      nativeSetter.call(input, "C:\\\\Users\\\\developer\\\\code\\\\");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    })
  `);
  const s5 = JSON.parse(s5State);
  logEvidence("Scenario 5", "Input has trailing backslash", s5.inputValue === "C:\\Users\\developer\\code\\", { input: s5.inputValue });
  logEvidence("Scenario 5", "Child suggestions displayed without Go/Enter", s5.optionCount >= 2 && s5.options.some((o: string) => o.includes("ferryx")), { options: s5.options });
  logEvidence("Scenario 5", "No Go button present", s5.hasGoBtn === false, { hasGoBtn: s5.hasGoBtn });

  const shot5 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "scenario-05-remote-directory-combobox.png"), shot5);
  console.log("Saved scenario-05-remote-directory-combobox.png");

  // --- Scenario 6: Close Dialog & Reopen Tab -> Restores Sidebar Expansion (transitionend on chevron) ---
  console.log("\n=== Scenario 6: Reopen Tab -> Restores Expansion ===");
  await webview.evaluate(`window.__QA__.closeAddProject()`);

  await webview.evaluate(`
    new Promise((resolve) => {
      const chevron = document.querySelector('button[aria-label*="orca-local"][aria-expanded] svg');
      const timeout = setTimeout(() => resolve(true), 2000);

      if (chevron) {
        const onEnd = (e) => {
          if (e.propertyName === "transform") {
            clearTimeout(timeout);
            chevron.removeEventListener("transitionend", onEnd);
            resolve(true);
          }
        };
        chevron.addEventListener("transitionend", onEnd);
      }

      window.__QA__.reopenTab().then(() => {
        if (chevron) {
          void window.getComputedStyle(chevron).transform;
          const anims = chevron.getAnimations ? chevron.getAnimations() : [];
          if (anims.length > 0) {
            Promise.all(anims.map(a => a.finished)).then(() => {
              clearTimeout(timeout);
              resolve(true);
            });
          }
        }
      });
    })
  `);

  const s6 = await getState();
  logEvidence("Scenario 6", "Tab count restored to 1", s6.tabCount === 1, { tabCount: s6.tabCount });
  logEvidence("Scenario 6", "orca-local removed from emptyWorkspaceIds", !s6.emptyWorkspaceIds.includes("orca-local"), { emptyWorkspaceIds: s6.emptyWorkspaceIds });
  logEvidence("Scenario 6", "orca-local automatically re-expanded", s6.orcaLocalExpanded === true, { expanded: s6.orcaLocalExpanded });
  logEvidence("Scenario 6", "orca-local worktree list restored in DOM", s6.hasWorktreeList === true, { hasList: s6.hasWorktreeList });

  const shot6 = await webview.screenshot();
  await Bun.write(path.join(SCREENSHOT_DIR, "scenario-06-sidebar-reopen-restored.png"), shot6);
  console.log("Saved scenario-06-sidebar-reopen-restored.png");

  console.log("\nAll 6 integration scenarios PASSED!");
} finally {
  console.log("\n--- Cleaning up resources ---");
  if (webview) {
    webview.close();
    console.log("Webview closed.");
  }
  let serverExited = false;
  if (viteProcess && viteProcess.exitCode === null) {
    const exitPromise = new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        try {
          viteProcess.kill("SIGKILL");
        } catch {}
        resolve();
      }, 3000);

      viteProcess.once("exit", () => {
        clearTimeout(timer);
        serverExited = true;
        resolve();
      });
    });

    viteProcess.kill("SIGTERM");
    console.log(`Vite server (PID ${viteProcess.pid}) sent SIGTERM, awaiting process exit...`);
    await exitPromise;
    console.log(`Vite server (PID ${viteProcess.pid}) exited.`);
  } else if (viteProcess && viteProcess.exitCode !== null) {
    serverExited = true;
  }
  const portFreed = await verifyPortFreed(PORT);
  console.log(`Port ${PORT} freed: ${portFreed}`);

  try {
    await fs.rm(CACHE_DIR, { recursive: true, force: true });
    console.log(`Cleaned Vite cache dir: ${CACHE_DIR}`);
  } catch {}

  const resultData = {
    evidenceLog,
    teardownReceipt: {
      serverPid: viteProcess?.pid,
      serverExited,
      portFreed,
      webviewClosed: true,
      cacheCleaned: true,
    },
  };
  await Bun.write(
    path.join(BASE_DIR, "integration-qa-evidence.json"),
    JSON.stringify(resultData, null, 2)
  );
  console.log("Written integration-qa-evidence.json");
}
