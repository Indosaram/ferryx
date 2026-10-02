#!/usr/bin/env node

import { createRequire } from "node:module";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const qaDir = resolve(__filename, "..");
const repoRoot = resolve(qaDir, "../..");

function parseArgs(argv) {
  const args = {
    evidenceDir: join(repoRoot, "docs/evidence/ssh-reboot-recovery"),
    url: "http://127.0.0.1:5188/ssh-reboot-recovery-harness.html",
    chromeBin: null,
    playwrightCore: null,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const item = argv[i];
    if (item === "--evidence-dir") {
      args.evidenceDir = resolve(argv[++i]);
    } else if (item === "--url") {
      args.url = argv[++i];
    } else if (item === "--chrome-bin") {
      args.chromeBin = resolve(argv[++i]);
    } else if (item === "--playwright-core") {
      args.playwrightCore = resolve(argv[++i]);
    } else if (item === "--help" || item === "-h") {
      console.log("Usage: node scripts/qa/ssh-reboot-recovery-runner.mjs [options]");
      console.log("  --evidence-dir <dir>    Directory for screenshots and results.json");
      console.log("  --url <url>             URL of the served harness");
      console.log("  --chrome-bin <path>     Explicit path to Chrome executable");
      console.log("  --playwright-core <path>Explicit path to playwright-core package");
      process.exit(0);
    }
  }

  if (!args.chromeBin) {
    if (process.env.CHROME_BIN) {
      args.chromeBin = process.env.CHROME_BIN;
    } else if (process.platform === "win32") {
      args.chromeBin = "C:/Program Files/Google/Chrome/Application/chrome.exe";
    } else if (process.platform === "darwin") {
      args.chromeBin = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    } else {
      args.chromeBin = "/usr/bin/google-chrome";
    }
  }

  return args;
}

function resolvePlaywrightCore(customPath) {
  const requireAnchors = [
    join(repoRoot, "ui", "package.json"),
    join(repoRoot, "package.json"),
  ];

  const candidatePaths = [
    customPath,
    process.env.PLAYWRIGHT_CORE_PATH,
    join(repoRoot, "ui", "node_modules", "playwright-core"),
    join(repoRoot, "node_modules", "playwright-core"),
    join(repoRoot, "ui", "node_modules", "playwright"),
    join(repoRoot, "node_modules", "playwright"),
    "playwright-core",
    "playwright",
  ].filter(Boolean);

  const errors = [];
  for (const anchor of requireAnchors) {
    if (!existsSync(anchor)) continue;
    let req;
    try {
      req = createRequire(anchor);
    } catch {
      continue;
    }

    for (const cand of candidatePaths) {
      try {
        const mod = req(cand);
        const pw = mod.chromium ? mod : req(cand);
        if (pw && pw.chromium) {
          return { pw, source: cand };
        }
      } catch (e) {
        errors.push(`${anchor} -> ${cand}: ${e.message}`);
      }
    }
  }

  throw new Error("Unable to resolve playwright-core. Attempts:\n" + errors.join("\n"));
}

const VIEWPORTS = [
  { name: "compact", width: 548, height: 340 },
  { name: "standard", width: 1024, height: 640 },
];

async function checkVisualInvariants(page, vp, phaseName, checks) {
  const scrollOffset = await page.evaluate(() => ({
    x: window.scrollX || document.documentElement.scrollLeft || 0,
    y: window.scrollY || document.documentElement.scrollTop || 0,
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));

  checks.push({
    id: `no-scroll-offset-${phaseName}-${vp.name}`,
    pass: scrollOffset.x === 0 && scrollOffset.y === 0,
    expected: { x: 0, y: 0 },
    actual: { x: scrollOffset.x, y: scrollOffset.y },
  });

  checks.push({
    id: `no-horizontal-overflow-${phaseName}-${vp.name}`,
    pass: scrollOffset.scrollWidth <= scrollOffset.clientWidth + 2,
    expected: `<= ${scrollOffset.clientWidth + 2}`,
    actual: scrollOffset.scrollWidth,
  });

  const iconRect = await page.evaluate(() => {
    const el = document.querySelector('[data-testid="terminal-pane-overlay"] svg')
      || document.querySelector('[data-testid="terminal-pane-overlay"] img');
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { width: r.width, height: r.height };
  });

  if (iconRect) {
    checks.push({
      id: `agent-icon-contained-${phaseName}-${vp.name}`,
      pass: iconRect.width > 0 && iconRect.width <= 48 && iconRect.height > 0 && iconRect.height <= 48,
      expected: "width <= 48 && height <= 48",
      actual: { width: iconRect.width, height: iconRect.height },
    });
  }

  const boundsCheck = await page.evaluate(() => {
    const overlay = document.querySelector('[data-testid="terminal-pane-overlay"] > div');
    const recoverBtn = document.querySelector('[data-testid="ssh-recover-session-button"]');
    const alertEl = document.querySelector('p[role="alert"]');
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    const checkElem = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return {
        fitsX: r.left >= -1 && r.right <= vw + 1,
        fitsY: r.top >= -1 && r.bottom <= vh + 1,
        rect: { left: r.left, right: r.right, top: r.top, bottom: r.bottom },
      };
    };

    return {
      overlayCard: checkElem(overlay),
      recoverButton: checkElem(recoverBtn),
      alertBox: checkElem(alertEl),
    };
  });

  if (boundsCheck.overlayCard) {
    checks.push({
      id: `overlay-card-in-viewport-${phaseName}-${vp.name}`,
      pass: boundsCheck.overlayCard.fitsX && boundsCheck.overlayCard.fitsY,
      expected: "inside viewport bounds",
      actual: boundsCheck.overlayCard.rect,
    });
  }

  if (boundsCheck.recoverButton) {
    checks.push({
      id: `recover-button-in-viewport-${phaseName}-${vp.name}`,
      pass: boundsCheck.recoverButton.fitsX && boundsCheck.recoverButton.fitsY,
      expected: "inside viewport bounds",
      actual: boundsCheck.recoverButton.rect,
    });
  }

  if (boundsCheck.alertBox) {
    checks.push({
      id: `alert-box-in-viewport-${phaseName}-${vp.name}`,
      pass: boundsCheck.alertBox.fitsX && boundsCheck.alertBox.fitsY,
      expected: "inside viewport bounds",
      actual: boundsCheck.alertBox.rect,
    });
  }
}

async function runScenarioSuccess(page, baseUrl, vp, evidenceDir, checks, screenshots) {
  const targetUrl = `${baseUrl}?scenario=expired_recovery_success`;
  await page.goto(targetUrl, { waitUntil: "domcontentloaded" });

  await page.waitForSelector('[data-testid="qa-harness-ready"][data-scenario="expired_recovery_success"]', { state: "attached", timeout: 15000 });
  await page.waitForSelector('[data-testid="terminal-pane-overlay"]', { timeout: 10000 });
  await page.waitForSelector('[data-testid="ssh-recover-session-button"]:not([disabled])', { timeout: 10000 });

  await checkVisualInvariants(page, vp, "expired", checks);

  checks.push({
    id: `expired-overlay-present-${vp.name}`,
    pass: await page.isVisible('[data-testid="terminal-pane-overlay"]'),
    expected: true,
    actual: true,
  });

  const initialShot = join(evidenceDir, `expired-state-${vp.width}x${vp.height}.png`);
  await page.screenshot({ path: initialShot });
  screenshots.push(initialShot);

  await page.click('[data-testid="ssh-recover-session-button"]');

  await page.waitForSelector('[data-testid="qa-signal-pending"]', { state: "attached", timeout: 10000 });
  await page.waitForSelector('[data-testid="ssh-recover-session-button"][aria-busy="true"]', { timeout: 10000 });

  await checkVisualInvariants(page, vp, "pending", checks);

  const isButtonDisabled = await page.isDisabled('[data-testid="ssh-recover-session-button"]');
  checks.push({
    id: `pending-button-disabled-${vp.name}`,
    pass: isButtonDisabled,
    expected: true,
    actual: isButtonDisabled,
  });

  const pendingShot = join(evidenceDir, `pending-state-${vp.width}x${vp.height}.png`);
  await page.screenshot({ path: pendingShot });
  screenshots.push(pendingShot);

  await page.evaluate(() => {
    window.__FERRYX_REBOOT_QA__?.resolveRecovery();
  });

  await page.waitForSelector('[data-testid="qa-signal-recovered"]', { state: "attached", timeout: 10000 });
  await page.waitForSelector('[data-testid="terminal-pane-overlay"]', { state: "detached", timeout: 10000 });
  await page.waitForSelector('[data-testid="native-terminal"]', { state: "visible", timeout: 10000 });

  await checkVisualInvariants(page, vp, "recovered", checks);

  const recoveredPaneId = await page.getAttribute('[data-testid="native-terminal"]', "data-session-id");
  const recoveredBackendId = await page.getAttribute('[data-testid="native-terminal"]', "data-backend-id");

  checks.push({
    id: `recovered-same-pane-id-${vp.name}`,
    pass: recoveredPaneId === "pane-ssh-reboot-1",
    expected: "pane-ssh-reboot-1",
    actual: recoveredPaneId,
  });

  checks.push({
    id: `recovered-same-backend-id-${vp.name}`,
    pass: recoveredBackendId === "backend-ssh-stable-42",
    expected: "backend-ssh-stable-42",
    actual: recoveredBackendId,
  });

  const harnessState = await page.evaluate(() => window.__FERRYX_REBOOT_QA__?.getState());
  checks.push({
    id: `no-shell-fallback-spawned-${vp.name}`,
    pass: harnessState?.openShellCallCount === 0 && harnessState?.reconnectCallCount === 1,
    expected: { openShellCallCount: 0, reconnectCallCount: 1 },
    actual: { openShellCallCount: harnessState?.openShellCallCount, reconnectCallCount: harnessState?.reconnectCallCount },
  });

  const recoveredShot = join(evidenceDir, `recovered-state-${vp.width}x${vp.height}.png`);
  await page.screenshot({ path: recoveredShot });
  screenshots.push(recoveredShot);
}

async function runScenarioFailure(page, baseUrl, vp, evidenceDir, checks, screenshots) {
  const targetUrl = `${baseUrl}?scenario=expired_recovery_failure`;
  await page.goto(targetUrl, { waitUntil: "domcontentloaded" });

  await page.waitForSelector('[data-testid="qa-harness-ready"][data-scenario="expired_recovery_failure"]', { state: "attached", timeout: 15000 });
  await page.waitForSelector('[data-testid="ssh-recover-session-button"]:not([disabled])', { timeout: 10000 });

  await checkVisualInvariants(page, vp, "failure-initial", checks);

  await page.click('[data-testid="ssh-recover-session-button"]');
  await page.waitForSelector('[data-testid="qa-signal-pending"]', { state: "attached", timeout: 10000 });

  await page.evaluate(() => {
    window.__FERRYX_REBOOT_QA__?.rejectRecovery({
      code: "REMOTE_RETRY_FAILED",
      message: "Remote process not found on host after machine reboot",
    });
  });

  await page.waitForSelector('[data-testid="qa-signal-failed"]', { state: "attached", timeout: 10000 });
  await page.waitForSelector('p[role="alert"]', { timeout: 10000 });
  await page.waitForSelector('[data-testid="ssh-recover-session-button"]:not([disabled])', { timeout: 10000 });

  await checkVisualInvariants(page, vp, "failed", checks);

  const alertContent = await page.textContent('p[role="alert"]');
  checks.push({
    id: `failure-error-alert-rendered-${vp.name}`,
    pass: alertContent && alertContent.includes("Remote process not found on host"),
    expected: "includes 'Remote process not found on host'",
    actual: alertContent?.trim(),
  });

  const harnessState = await page.evaluate(() => window.__FERRYX_REBOOT_QA__?.getState());
  checks.push({
    id: `failure-preserves-no-fallback-${vp.name}`,
    pass: harnessState?.openShellCallCount === 0,
    expected: 0,
    actual: harnessState?.openShellCallCount,
  });

  const failedShot = join(evidenceDir, `failed-state-${vp.width}x${vp.height}.png`);
  await page.screenshot({ path: failedShot });
  screenshots.push(failedShot);
}

async function runScenarioUnavailable(page, baseUrl, vp, evidenceDir, checks, screenshots) {
  const targetUrl = `${baseUrl}?scenario=unavailable_record_safety`;
  await page.goto(targetUrl, { waitUntil: "domcontentloaded" });

  await page.waitForSelector('[data-testid="qa-harness-ready"][data-scenario="unavailable_record_safety"]', { state: "attached", timeout: 15000 });
  await page.waitForSelector('[data-testid="terminal-pane-overlay"]', { timeout: 10000 });

  await checkVisualInvariants(page, vp, "unavailable", checks);

  const recoverBtnVisible = await page.isVisible('[data-testid="ssh-recover-session-button"]');
  checks.push({
    id: `unavailable-hides-recover-button-${vp.name}`,
    pass: recoverBtnVisible === false,
    expected: false,
    actual: recoverBtnVisible,
  });

  const newShellBtnVisible = await page.isVisible('button:has-text("Open new shell")');
  checks.push({
    id: `unavailable-offers-new-shell-${vp.name}`,
    pass: newShellBtnVisible === true,
    expected: true,
    actual: newShellBtnVisible,
  });

  const harnessState = await page.evaluate(() => window.__FERRYX_REBOOT_QA__?.getState());
  checks.push({
    id: `unavailable-reconnect-never-called-${vp.name}`,
    pass: harnessState?.reconnectCallCount === 0,
    expected: 0,
    actual: harnessState?.reconnectCallCount,
  });

  const unavailableShot = join(evidenceDir, `unavailable-record-state-${vp.width}x${vp.height}.png`);
  await page.screenshot({ path: unavailableShot });
  screenshots.push(unavailableShot);
}

export async function main() {
  const args = parseArgs(process.argv.slice(2));
  mkdirSync(args.evidenceDir, { recursive: true });

  const { pw, source: pwSource } = resolvePlaywrightCore(args.playwrightCore);
  console.log(`Resolved playwright-core from: ${pwSource}`);
  console.log(`Target Chrome binary: ${args.chromeBin}`);
  console.log(`Evidence output directory: ${args.evidenceDir}`);

  const launchOpts = {
    headless: true,
    args: ["--no-sandbox", "--disable-setuid-sandbox", "--disable-dev-shm-usage"],
  };

  if (existsSync(args.chromeBin)) {
    launchOpts.executablePath = args.chromeBin;
  } else {
    launchOpts.channel = "chrome";
  }

  const browser = await pw.chromium.launch(launchOpts);
  const startTime = new Date().toISOString();
  const allChecks = [];
  const allScreenshots = [];
  const uncaughtPageErrors = [];

  try {
    for (const vp of VIEWPORTS) {
      console.log(`Running suite at viewport ${vp.name} (${vp.width}x${vp.height})...`);
      const context = await browser.newContext({
        viewport: { width: vp.width, height: vp.height },
        deviceScaleFactor: 1,
      });

      const page = await context.newPage();
      page.on("pageerror", (err) => {
        uncaughtPageErrors.push({ viewport: vp.name, error: err.message || String(err) });
      });

      try {
        await runScenarioSuccess(page, args.url, vp, args.evidenceDir, allChecks, allScreenshots);
        await runScenarioFailure(page, args.url, vp, args.evidenceDir, allChecks, allScreenshots);
        await runScenarioUnavailable(page, args.url, vp, args.evidenceDir, allChecks, allScreenshots);
      } finally {
        await page.close().catch(() => undefined);
        await context.close().catch(() => undefined);
      }
    }
  } finally {
    await browser.close().catch(() => undefined);
  }

  allChecks.push({
    id: "uncaught-page-errors-none",
    pass: uncaughtPageErrors.length === 0,
    expected: 0,
    actual: uncaughtPageErrors.length,
  });

  const endTime = new Date().toISOString();
  const passedCount = allChecks.filter((c) => c.pass).length;
  const failedCount = allChecks.filter((c) => !c.pass).length;
  const verdict = failedCount === 0 ? "PASS" : "FAIL";

  const resultReport = {
    testSuite: "ssh-reboot-recovery-headless-browser-qa",
    verdict,
    startedAt: startTime,
    completedAt: endTime,
    tooling: {
      playwrightCoreSource: pwSource,
      chromeBinary: args.chromeBin,
      platform: process.platform,
      nodeVersion: process.version,
    },
    boundaryMocks: {
      nativeTerminal: "scripts/qa/ssh-reboot-recovery-native-mock.tsx (stubbed canvas container data-testid=native-terminal, isolating libghostty-vt/WGPU/xterm hardware runtime)",
      dagBadge: "scripts/qa/ssh-reboot-recovery-dag-mock.tsx (stubbed null, isolating DAG state watcher and layout pipeline)",
      ipcRuntime: "scripts/qa/ssh-reboot-recovery-tauri-mock.ts (pure toIpcError and StructuredIpcError mapping, isolating Tauri window and binary IPC stream)",
      reconnectWiring: "ui/src/lib/sshRebootRecovery.ts (production reconnectSshSession helper invoked with injected retryRemoteSession callback)",
      backendHelperRpc: "Verified separately via remote helper unit tests (stable kernel per-boot identity, recovery store, host.lock)",
    },
    viewports: VIEWPORTS,
    checks: allChecks,
    screenshots: allScreenshots,
    summary: {
      totalChecks: allChecks.length,
      passed: passedCount,
      failed: failedCount,
    },
  };

  const resultsPath = join(args.evidenceDir, "results.json");
  writeFileSync(resultsPath, JSON.stringify(resultReport, null, 2), "utf8");
  console.log(`Wrote verification report to ${resultsPath}`);
  console.log(`Summary: ${passedCount}/${allChecks.length} checks passed (verdict: ${verdict})`);

  if (failedCount > 0) {
    process.exit(1);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(__filename)) {
  main().catch((err) => {
    console.error("FATAL: ssh-reboot-recovery-runner failed:", err);
    process.exit(1);
  });
}
