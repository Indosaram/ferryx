import { createServer } from "../../../ui/node_modules/vite";
import react from "../../../ui/node_modules/@vitejs/plugin-react";
import { resolve } from "node:path";
import { mkdir } from "node:fs/promises";
import net from "node:net";

const root = resolve(import.meta.dir, "../../..");
const outDir = resolve(import.meta.dir);
const screenshotDir = resolve(outDir, "screenshots");
await mkdir(screenshotDir, { recursive: true });

// Allocate a dynamic free port that is guaranteed not 5173
const netServer = net.createServer();
await new Promise(r => netServer.listen(0, "127.0.0.1", r));
const freePort = netServer.address().port;
netServer.close();

console.log(`[QA Harness] Ephemeral port allocated: ${freePort} (isolated, not 5173)`);

// Load Tailwind and config to render exact product CSS
const tailwind = (await import("../../../ui/node_modules/tailwindcss")).default;
const autoprefixer = (await import("../../../ui/node_modules/autoprefixer")).default;
const tailwindConfig = (await import("../../../ui/tailwind.config.js")).default;

const vite = await createServer({
  configFile: false,
  root,
  plugins: [react()],
  css: {
    postcss: {
      plugins: [
        tailwind({
          ...tailwindConfig,
          content: [
            resolve(root, "ui/index.html"),
            resolve(root, "ui/src/**/*.{js,ts,jsx,tsx}"),
            resolve(root, "docs/evidence/remote-machines-20260915/**/*.{html,tsx}"),
          ],
        }),
        autoprefixer(),
      ],
    },
  },
  resolve: {
    alias: {
      "@": resolve(root, "ui/src"),
      react: resolve(root, "ui/node_modules/react"),
      "react-dom": resolve(root, "ui/node_modules/react-dom"),
    },
  },
  server: {
    host: "127.0.0.1",
    port: freePort,
    strictPort: true,
    hmr: false,
  },
});

await vite.listen();
const harnessUrl = `http://127.0.0.1:${freePort}/docs/evidence/remote-machines-20260915/index.html`;
console.log(`[QA Harness] Vite server ready at: ${harnessUrl}`);

const results = {
  timestamp: new Date().toISOString(),
  harnessUrl,
  port: freePort,
  engine: "Bun.WebView (Chrome backend, headless)",
  bunVersion: Bun.version,
  viewports: {},
  summary: {
    totalChecks: 0,
    passedChecks: 0,
    failedChecks: 0,
  },
  defects: [],
  limitations: [],
};

function recordCheck(viewport, name, pass, detail = null) {
  results.summary.totalChecks++;
  if (pass) {
    results.summary.passedChecks++;
    console.log(`  ✓ [${viewport}] ${name}`);
  } else {
    results.summary.failedChecks++;
    console.error(`  ✗ [${viewport}] ${name} - FAIL:`, detail);
  }
  if (!results.viewports[viewport]) results.viewports[viewport] = [];
  results.viewports[viewport].push({ name, pass, detail });
}

async function evaluate(view, code) {
  return await view.evaluate(code);
}

async function waitFor(view, fnCode, timeoutMs = 5000, intervalMs = 50) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await view.evaluate(fnCode);
      if (res) return res;
    } catch {}
    await new Promise(r => setTimeout(r, intervalMs));
  }
  throw new Error(`Timeout waiting for expression: ${fnCode}`);
}

const VIEWPORTS = [
  { width: 1280, height: 800, label: "1280" },
  { width: 390, height: 844, label: "390" },
];

try {
  for (const vp of VIEWPORTS) {
    console.log(`\n========================================`);
    console.log(`Testing Viewport: ${vp.label} (${vp.width}x${vp.height})`);
    console.log(`========================================`);

    const view = new Bun.WebView({
      backend: "chrome",
      headless: true,
      width: vp.width,
      height: vp.height,
    });

    try {
      await view.navigate(harnessUrl);
      await view.cdp("Emulation.setDeviceMetricsOverride", {
        width: vp.width,
        height: vp.height,
        deviceScaleFactor: 1,
        mobile: false,
      });
      // Wait for React to hydrate
      await waitFor(view, `Boolean(window.__qa && document.querySelector('#mock-parent-settings-dialog'))`, 8000);
      await new Promise(r => setTimeout(r, 500));

      // ----------------------------------------------------
      // 1. EMPTY LIST SCENARIO
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 1: Empty List`);
      await evaluate(view, `window.__qa.setScenario("empty")`);
      await waitFor(view, `document.body.innerText.includes("No remote machines configured")`);

      const emptyButtonsCount = await evaluate(view, `document.querySelectorAll('button[aria-label="Add Machine"]').length`);
      recordCheck(vp.label, "Single 'Add Machine' button in toolbar", emptyButtonsCount === 1, { count: emptyButtonsCount });

      const hasLegacyTabs = await evaluate(view, `Boolean(Array.from(document.querySelectorAll('button')).find(b => ['All', 'Paired', 'SSH'].includes(b.textContent.trim()) && b.getAttribute('aria-pressed') !== null))`);
      recordCheck(vp.label, "No legacy filter tabs (All/Paired/SSH)", !hasLegacyTabs);

      const emptyDocScroll = await evaluate(view, `document.documentElement.scrollWidth <= ${vp.width}`);
      recordCheck(vp.label, "Narrow layout: no horizontal document overflow in empty state", emptyDocScroll);

      const emptyShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-empty-list.png`), emptyShot);

      // ----------------------------------------------------
      // 2. MIXED LIST SCENARIO
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 2: Mixed List`);
      await evaluate(view, `window.__qa.setScenario("mixed")`);
      await waitFor(view, `document.querySelectorAll('[role="list"][aria-label="Remote machines"] > div').length >= 4`);

      const listItemsCount = await evaluate(view, `document.querySelectorAll('[role="list"][aria-label="Remote machines"] > div').length`);
      recordCheck(vp.label, "Unified list renders all machines without section splits", listItemsCount === 4, { count: listItemsCount });

      const badges = await evaluate(view, `Array.from(document.querySelectorAll('[role="list"] *')).filter(e => ['Paired', 'SSH'].includes(e.textContent.trim())).map(e => e.textContent.trim())`);
      recordCheck(vp.label, "Common row Type Badges ('Paired' and 'SSH')", badges.includes("Paired") && badges.includes("SSH"), badges);

      const statuses = await evaluate(view, `Array.from(document.querySelectorAll('[data-testid="machine-status"]')).map(e => ({ text: e.textContent.trim(), code: e.getAttribute('data-code') }))`);
      recordCheck(vp.label, "Common row concise Status indicators present", statuses.length === 4, statuses);

      const mixedDocScroll = await evaluate(view, `document.documentElement.scrollWidth <= ${vp.width}`);
      recordCheck(vp.label, "Narrow layout: no horizontal overflow in mixed list", mixedDocScroll);

      const mixedShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-mixed-list.png`), mixedShot);

      // ----------------------------------------------------
      // 3. PAIRED DETAILS EXPANDED
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 3: Paired Machine Details Expanded`);
      // Find Details button for "Linux Build Box"
      await evaluate(view, `(() => {
        const rows = Array.from(document.querySelectorAll('[role="list"] > div'));
        const pairedRow = rows.find(r => r.textContent.includes("Linux Build Box"));
        const btn = pairedRow.querySelector('button[aria-label*="Details"]');
        btn.click();
      })()`);

      await waitFor(view, `document.body.innerText.includes("Machine ID:") && document.body.innerText.includes("box-linux-builder")`);
      const pairedMeta = await evaluate(view, `(() => {
        const text = document.body.innerText;
        return {
          hasMachineId: text.includes("Machine ID:"),
          hasRelay: text.includes("Relay:"),
          hasTransport: text.includes("Transport:"),
          hasGeneration: text.includes("Generation:"),
          hasGrant: text.includes("Grant:"),
          hasAuthStatus: text.includes("Auth Status:"),
        };
      })()`);
      recordCheck(vp.label, "Paired details conceal technical metadata", Object.values(pairedMeta).every(Boolean), pairedMeta);

      const pairedActions = await evaluate(view, `(() => {
        return {
          checkCaps: Boolean(document.querySelector('button[aria-label*="Check capabilities"]')),
          repair: Boolean(document.querySelector('button[aria-label*="Re-pair"]')),
          forget: Boolean(document.querySelector('button[aria-label*="Forget"]')),
        };
      })()`);
      recordCheck(vp.label, "Paired actions available (Check capabilities, Re-pair, Forget)", Object.values(pairedActions).every(Boolean), pairedActions);

      const pairedDetailsShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-paired-details.png`), pairedDetailsShot);

      // ----------------------------------------------------
      // 4. SSH DETAILS EXPANDED
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 4: SSH Machine Details Expanded`);
      await evaluate(view, `(() => {
        const rows = Array.from(document.querySelectorAll('[role="list"] > div'));
        const sshRow = rows.find(r => r.textContent.includes("Dev Server"));
        const btn = sshRow.querySelector('button[aria-label*="Details"]');
        btn.click();
      })()`);

      await waitFor(view, `document.body.innerText.includes("Hostname:") && document.body.innerText.includes("192.168.1.50")`);
      const sshMeta = await evaluate(view, `(() => {
        const text = document.body.innerText;
        return {
          hasHostname: text.includes("Hostname:"),
          hasPort: text.includes("Port:"),
          hasUsername: text.includes("Username:"),
          hasAuthMethod: text.includes("Auth method:"),
        };
      })()`);
      recordCheck(vp.label, "SSH details conceal technical metadata", Object.values(sshMeta).every(Boolean), sshMeta);

      const sshActions = await evaluate(view, `(() => {
        return {
          testConn: Boolean(document.querySelector('button[aria-label*="Test connection"]')),
          prepIntegration: Boolean(document.querySelector('button[aria-label*="Prepare agent integration"]')),
          edit: Boolean(document.querySelector('button[aria-label*="Edit"]')),
          toggleDisable: Boolean(document.querySelector('button[aria-label*="Toggle disable"]')),
          delete: Boolean(document.querySelector('button[aria-label*="Delete"]')),
        };
      })()`);
      recordCheck(vp.label, "SSH actions available (Test, Prepare, Edit, Disable, Delete)", Object.values(sshActions).every(Boolean), sshActions);

      const sshDetailsShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-ssh-details.png`), sshDetailsShot);

      // Close both details
      await evaluate(view, `(() => {
        const rows = Array.from(document.querySelectorAll('[role="list"] > div'));
        rows.forEach(r => {
          const btn = r.querySelector('button[aria-label*="Details"][aria-expanded="true"]');
          if (btn) btn.click();
        });
      })()`);

      // ----------------------------------------------------
      // 5. MODAL: PAIR WITH PIN
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 5: Add Machine Modal - Pair with PIN`);
      // Click Add Machine button
      await evaluate(view, `document.querySelector('button[aria-label="Add Machine"]').click()`);
      await waitFor(view, `Boolean(document.querySelector('[role="dialog"][aria-label="Add Machine"]'))`);

      const activeTab = await evaluate(view, `document.querySelector('[role="tablist"] [role="tab"][aria-selected="true"]')?.textContent.trim()`);
      recordCheck(vp.label, "Add Machine Modal opens on 'Pair with PIN' tab", activeTab === "Pair with PIN", { activeTab });

      // Check modal initial focus
      const initialFocusedEl = await evaluate(view, `(() => {
        const el = document.activeElement;
        return {
          tagName: el?.tagName,
          id: el?.id,
          role: el?.getAttribute('role'),
          ariaSelected: el?.getAttribute('aria-selected'),
          ariaLabel: el?.getAttribute('aria-label'),
        };
      })()`);
      console.log(`  ℹ Initial modal activeElement:`, initialFocusedEl);

      const pinFieldExists = await evaluate(view, `Boolean(document.querySelector('input[aria-label="Machine PIN"], input[placeholder*="PIN"], input#pin-input'))`);
      recordCheck(vp.label, "PIN input field present and accessible", pinFieldExists);

      const pinModalShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-modal-pin.png`), pinModalShot);

      // ----------------------------------------------------
      // 6. MODAL: CONNECT WITH SSH (BASIC & ADVANCED)
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 6: Add Machine Modal - Connect with SSH (Advanced)`);
      await evaluate(view, `(() => {
        const tabs = Array.from(document.querySelectorAll('[role="tab"]'));
        const sshTab = tabs.find(t => t.textContent.includes("Connect with SSH"));
        sshTab?.click();
      })()`);

      await waitFor(view, `Boolean(document.querySelector('input[aria-label="Hostname"]'))`);
      const basicSshFields = await evaluate(view, `(() => {
        return {
          label: Boolean(document.querySelector('input[aria-label="Label"]')),
          hostname: Boolean(document.querySelector('input[aria-label="Hostname"]')),
          username: Boolean(document.querySelector('input[aria-label="Username"]')),
          port: Boolean(document.querySelector('input[aria-label="Port"]')),
        };
      })()`);
      recordCheck(vp.label, "SSH basic fields (Label, Hostname, Username, Port) present", Object.values(basicSshFields).every(Boolean), basicSshFields);

      // Toggle Advanced SSH Options
      await evaluate(view, `(() => {
        const btns = Array.from(document.querySelectorAll('button'));
        const advBtn = btns.find(b => b.textContent.includes("Advanced Options"));
        advBtn?.click();
      })()`);

      await waitFor(view, `Boolean(document.querySelector('[aria-label="Authentication method"], input[aria-label="Identity file"]'))`);
      const advSshFields = await evaluate(view, `(() => {
        return {
          authMethod: Boolean(document.querySelector('[aria-label="Authentication method"]')),
          identityFile: Boolean(document.querySelector('input[aria-label="Identity file"]')),
          jumpHost: Boolean(document.querySelector('input[aria-label="Jump host"]')),
        };
      })()`);
      recordCheck(vp.label, "SSH advanced collapsible fields (Auth Method, Key, Jump Host) revealed", Object.values(advSshFields).every(Boolean), advSshFields);

      // Verify narrow layout containment of modal form
      const modalScrollWidth = await evaluate(view, `(() => {
        const modal = document.querySelector('[role="dialog"] > div');
        return { clientWidth: modal?.clientWidth, scrollWidth: modal?.scrollWidth };
      })()`);
      recordCheck(vp.label, "Narrow layout: modal SSH form controls fit within dialog", modalScrollWidth.scrollWidth <= modalScrollWidth.clientWidth + 2, modalScrollWidth);

      const sshAdvModalShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-modal-ssh-advanced.png`), sshAdvModalShot);

      // ----------------------------------------------------
      // 7. MODAL: IMPORT SSH CONFIG
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 7: Add Machine Modal - Import SSH Config`);
      await evaluate(view, `(() => {
        const tabs = Array.from(document.querySelectorAll('[role="tab"]'));
        const importTab = tabs.find(t => t.textContent.includes("Import SSH Config"));
        importTab?.click();
      })()`);

      await waitFor(view, `document.body.innerText.includes("System SSH Config") || document.body.innerText.includes("Paste Configuration")`);
      const importOptions = await evaluate(view, `(() => {
        return {
          hasSystem: document.body.innerText.includes("System SSH Config") || document.body.innerText.includes("System Config"),
          hasPaste: document.body.innerText.includes("Paste Configuration"),
        };
      })()`);
      recordCheck(vp.label, "Import SSH Config options available (System & Paste)", Object.values(importOptions).every(Boolean), importOptions);

      const importModalShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-modal-import.png`), importModalShot);

      // ----------------------------------------------------
      // 8. MODAL: FAILURE STATE & INPUT PRESERVATION
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 8: Add Machine Modal - Failure State & Input Retention`);
      // Switch back to PIN
      await evaluate(view, `(() => {
        const tabs = Array.from(document.querySelectorAll('[role="tab"]'));
        const pinTab = tabs.find(t => t.textContent.includes("Pair with PIN"));
        pinTab?.click();
      })()`);

      await waitFor(view, `Boolean(document.querySelector('input[aria-label="Machine PIN"], input[placeholder*="PIN"], input#add-machine-pin'))`);

      // Fill failure PIN: "000000" using native setter so React state updates
      await evaluate(view, `(() => {
        const pinInp = document.querySelector('input[aria-label="Machine PIN"], input[placeholder*="PIN"], input#add-machine-pin');
        const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
        setter.call(pinInp, '000000');
        pinInp.dispatchEvent(new Event('input', { bubbles: true }));
        pinInp.dispatchEvent(new Event('change', { bubbles: true }));
      })()`);

      // Submit pair form
      await evaluate(view, `(() => {
        const submitBtn = Array.from(document.querySelectorAll('[role="dialog"] button[type="submit"]')).find(b => b.textContent.includes("Pair Machine"));
        submitBtn?.click();
      })()`);

      await waitFor(view, `Boolean(document.querySelector('[role="alert"]'))`);
      const alertText = await evaluate(view, `document.querySelector('[role="alert"]')?.textContent.trim()`);
      recordCheck(vp.label, "Failure error alert displayed on invalid PIN", alertText.includes("Could not pair") || alertText.includes("Failed"), { alertText });

      const preservedPin = await evaluate(view, `document.querySelector('input[aria-label="Machine PIN"], input[placeholder*="PIN"], input#add-machine-pin')?.value`);
      recordCheck(vp.label, "User input preserved on failure (not cleared)", preservedPin === "000000", { preservedPin });

      const failureShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-modal-failure.png`), failureShot);

      // ----------------------------------------------------
      // 9. MODAL: SUCCESS STATE
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 9: Add Machine Modal - Success State`);
      // Fill valid PIN: "123456" using native setter
      await evaluate(view, `(() => {
        const pinInp = document.querySelector('input[aria-label="Machine PIN"], input[placeholder*="PIN"], input#add-machine-pin');
        const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
        setter.call(pinInp, '123456');
        pinInp.dispatchEvent(new Event('input', { bubbles: true }));
        pinInp.dispatchEvent(new Event('change', { bubbles: true }));
      })()`);

      // Submit pair form
      await evaluate(view, `(() => {
        const submitBtn = Array.from(document.querySelectorAll('[role="dialog"] button[type="submit"]')).find(b => b.textContent.includes("Pair Machine"));
        submitBtn?.click();
      })()`);

      await waitFor(view, `document.body.innerText.includes("connected and verified successfully")`);
      const hasAddProject = await evaluate(view, `(() => {
        const btns = Array.from(document.querySelectorAll('[role="dialog"] button'));
        return btns.some(b => b.textContent.includes("Add Project"));
      })()`);
      recordCheck(vp.label, "Success view displays verified machine and offers 'Add Project'", hasAddProject);

      const successShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-modal-success.png`), successShot);

      // Close modal by clicking "Done"
      await evaluate(view, `(() => {
        const btns = Array.from(document.querySelectorAll('[role="dialog"] button'));
        const doneBtn = btns.find(b => b.textContent.trim() === "Done");
        doneBtn?.click();
      })()`);

      await waitFor(view, `!document.querySelector('[role="dialog"]')`);
      recordCheck(vp.label, "Modal dismissed after success completion", true);

      // Verify newly added row is selected
      const isNewRowSelected = await evaluate(view, `Boolean(document.querySelector('[data-machine-id*="box-paired-success"][data-selected="true"]'))`);
      recordCheck(vp.label, "Newly added machine auto-selected in list", isNewRowSelected);

      // ----------------------------------------------------
      // 10. KEYBOARD FOCUS, ESCAPE & PARENT ISOLATION
      // ----------------------------------------------------
      console.log(`\n[${vp.label}] Scenario 10: Keyboard Focus, Escape & Parent Isolation`);
      // Reset counters and record trigger
      await evaluate(view, `window.__qa.resetEscapeCounts()`);
      await evaluate(view, `document.querySelector('button[aria-label="Add Machine"]').focus()`);
      const triggerFocusedBefore = await evaluate(view, `document.activeElement.getAttribute('aria-label') === "Add Machine"`);
      recordCheck(vp.label, "Trigger button focused before modal open", triggerFocusedBefore);

      // Open modal via click
      await evaluate(view, `document.querySelector('button[aria-label="Add Machine"]').click()`);
      await waitFor(view, `Boolean(document.querySelector('[role="dialog"]'))`);

      // Focus inside modal (e.g. PIN input)
      await evaluate(view, `document.querySelector('input[aria-label="Machine PIN"], input[placeholder*="PIN"], input#pin-input')?.focus()`);
      const isInputFocused = await evaluate(view, `document.activeElement.tagName === "INPUT"`);
      recordCheck(vp.label, "Focus located inside modal before Escape", isInputFocused);

      // Press Escape using view.press("Escape")
      await view.press("Escape");
      await new Promise(r => setTimeout(r, 200));

      const isModalClosed = await evaluate(view, `!document.querySelector('[role="dialog"]')`);
      recordCheck(vp.label, "Escape closes the Add Machine modal", isModalClosed);

      const parentEscCount = await evaluate(view, `window.__qa.getParentEscapeCount()`);
      const windowEscCount = await evaluate(view, `window.__qa.getWindowEscapeCount()`);
      console.log(`  ℹ Escape event metrics: parentEscapeCount=${parentEscCount}, windowEscapeCount=${windowEscCount}`);

      // Parent Escape isolation check:
      // When Escape is pressed in modal, parent dialog must NOT receive the Escape event!
      const isParentIsolated = parentEscCount === 0;
      recordCheck(vp.label, "Escape parent isolation (parent dialog escape count is 0)", isParentIsolated, { parentEscCount, windowEscCount });

      if (!isParentIsolated) {
        results.defects.push({
          viewport: vp.label,
          category: "Keyboard / Accessibility",
          severity: "Medium",
          summary: "Escape key bubbles to parent settings dialog instead of being isolated to AddMachineModal",
          detail: `Parent received ${parentEscCount} Escape events. Synthetic React stopPropagation did not isolate the event from parent container or native listeners.`,
        });
      }

      // Check trigger focus restoration
      const triggerRestored = await evaluate(view, `document.activeElement.getAttribute('aria-label') === "Add Machine"`);
      recordCheck(vp.label, "Trigger focus restored to 'Add Machine' button after Escape", triggerRestored, {
        activeElementTag: await evaluate(view, `document.activeElement.tagName`),
        activeElementLabel: await evaluate(view, `document.activeElement.getAttribute('aria-label')`),
      });

      const escapeShot = await view.screenshot();
      await Bun.write(resolve(screenshotDir, `${vp.label}-modal-escape-isolated.png`), escapeShot);

    } finally {
      view.close();
    }
  }
} finally {
  await vite.close();
}

// Write results.json
await Bun.write(resolve(outDir, "results.json"), JSON.stringify(results, null, 2));
console.log(`\n========================================`);
console.log(`QA Visual Run Complete!`);
console.log(`Total checks: ${results.summary.totalChecks}, Passed: ${results.summary.passedChecks}, Failed: ${results.summary.failedChecks}`);
console.log(`Results saved to: ${resolve(outDir, "results.json")}`);
console.log(`========================================\n`);

if (results.summary.failedChecks > 0) {
  process.exitCode = 1;
}
