import { spawn } from "node:child_process";
import path from "node:path";
import fs from "node:fs";

const outputDir = path.resolve("docs/session-continuation-20260908/ssh");
if (!fs.existsSync(outputDir)) fs.mkdirSync(outputDir, { recursive: true });

console.log("Starting Node Vite QA server on port 5213...");
const viteProcess = spawn("node", ["node_modules/vite/bin/vite.js", "--config", "qa-ssh-autocomplete.config.mjs"], {
  cwd: path.resolve("ui"),
  stdio: "inherit",
});

await Bun.sleep(1500);

const results: Array<{ scenario: number; name: string; pass: boolean; data: any }> = [];

try {
  const view = new Bun.WebView({ width: 1000, height: 750 });

  async function waitForSelector(selector: string, timeout = 5000) {
    const start = Date.now();
    while (Date.now() - start < timeout) {
      const exists = await view.evaluate(`Boolean(document.querySelector(${JSON.stringify(selector)}))`);
      if (exists) return true;
      await Bun.sleep(50);
    }
    throw new Error(`Timeout waiting for ${selector}`);
  }

  async function saveScreenshot(filename: string) {
    const blob = await view.screenshot();
    const filePath = path.join(outputDir, filename);
    await Bun.write(filePath, blob);
    console.log(`Saved screenshot: ${filename} (${blob.size || (blob as any).byteLength} bytes)`);
  }

  // SCENARIO 1: Windows Home Prefill
  console.log("\n--- Scenario 1: Windows Home Prefill ---");
  await view.navigate("http://127.0.0.1:5213/qa-ssh-autocomplete.html?host=windows");
  await waitForSelector("[data-testid=\"remote-repo-path-input\"]");
  await Bun.sleep(400);

  const sc1Data: any = await view.evaluate(`(() => {
    const input = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const confirmBtn = document.querySelector("[data-testid=\\"add-project-confirm-remote\\"]");
    const options = Array.from(document.querySelectorAll("[role=option]")).map(o => o.getAttribute("aria-label"));
    const noGoBtn = !Array.from(document.querySelectorAll("button")).some(b => b.textContent && b.textContent.trim() === "Go");
    return {
      inputValue: input ? input.value : "",
      options,
      confirmDisabled: confirmBtn ? confirmBtn.disabled : true,
      confirmText: confirmBtn ? confirmBtn.textContent.trim() : "",
      noGoBtn,
    };
  })()`);
  console.log("Scenario 1 Data:", sc1Data);
  const sc1Pass = sc1Data.inputValue === "C:\\Users\\developer" &&
    sc1Data.options.includes("code") &&
    sc1Data.options.includes("Documents") &&
    sc1Data.confirmDisabled === false &&
    sc1Data.confirmText === "Add this folder" &&
    sc1Data.noGoBtn === true;
  results.push({ scenario: 1, name: "Windows Home Prefill", pass: sc1Pass, data: sc1Data });
  await saveScreenshot("scenario-01-windows-home-prefill.png");

  // SCENARIO 2: Windows Separator Navigation Without Go/Enter
  console.log("\n--- Scenario 2: Windows Separator Navigation Without Go/Enter ---");
  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value") && Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    if (setter && el) {
      setter.call(el, "C:\\\\Users\\\\developer\\\\code\\\\");
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    }
  })()`);
  await Bun.sleep(400);

  const sc2Data: any = await view.evaluate(`(() => {
    const input = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const confirmBtn = document.querySelector("[data-testid=\\"add-project-confirm-remote\\"]");
    const options = Array.from(document.querySelectorAll("[role=option]")).map(o => o.getAttribute("aria-label"));
    const noGoBtn = !Array.from(document.querySelectorAll("button")).some(b => b.textContent && b.textContent.trim() === "Go");
    return {
      inputValue: input ? input.value : "",
      options,
      confirmDisabled: confirmBtn ? confirmBtn.disabled : true,
      noGoBtn,
    };
  })()`);
  console.log("Scenario 2 Data:", sc2Data);
  const sc2Pass = sc2Data.inputValue === "C:\\Users\\developer\\code\\" &&
    sc2Data.options.includes("ferryx") &&
    sc2Data.options.includes("frontend") &&
    sc2Data.options.includes("folder-with-a-very-long-name-for-path-width-verification") &&
    sc2Data.noGoBtn === true;
  results.push({ scenario: 2, name: "Windows Separator Immediate Children", pass: sc2Pass, data: sc2Data });
  await saveScreenshot("scenario-02-windows-separator-immediate-children.png");

  // SCENARIO 3: Prefix Narrows in Same Input
  console.log("\n--- Scenario 3: Prefix Narrows in Same Input ---");
  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value") && Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    if (setter && el) {
      setter.call(el, "C:\\\\Users\\\\developer\\\\code\\\\fe");
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    }
  })()`);
  await Bun.sleep(400);

  const sc3Data: any = await view.evaluate(`(() => {
    const input = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const confirmBtn = document.querySelector("[data-testid=\\"add-project-confirm-remote\\"]");
    const options = Array.from(document.querySelectorAll("[role=option]")).map(o => o.getAttribute("aria-label"));
    return {
      inputValue: input ? input.value : "",
      options,
      confirmDisabled: confirmBtn ? confirmBtn.disabled : true,
    };
  })()`);
  console.log("Scenario 3 Data:", sc3Data);
  const sc3Pass = sc3Data.inputValue === "C:\\Users\\developer\\code\\fe" &&
    sc3Data.options.length === 1 &&
    sc3Data.options[0] === "ferryx" &&
    sc3Data.confirmDisabled === true;
  results.push({ scenario: 3, name: "Prefix Narrowing In Same Input", pass: sc3Pass, data: sc3Data });
  await saveScreenshot("scenario-03-windows-prefix-narrowing.png");

  // SCENARIO 4: Tab Autocomplete Completes Path and Allows Continued Browsing
  console.log("\n--- Scenario 4: Tab Autocomplete Completes Path ---");
  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    if (el) {
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", code: "Tab", bubbles: true, cancelable: true }));
    }
  })()`);
  await Bun.sleep(400);

  const sc4Data: any = await view.evaluate(`(() => {
    const input = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const active = document.activeElement === input;
    const options = Array.from(document.querySelectorAll("[role=option]")).map(o => o.getAttribute("aria-label"));
    return {
      inputValue: input ? input.value : "",
      active,
      options,
    };
  })()`);
  console.log("Scenario 4 Data:", sc4Data);
  const sc4Pass = sc4Data.inputValue.startsWith("C:\\Users\\developer\\code\\ferryx") &&
    sc4Data.inputValue.endsWith("\\");
  results.push({ scenario: 4, name: "Tab Autocomplete Path", pass: sc4Pass, data: sc4Data });
  await saveScreenshot("scenario-04-windows-tab-autocomplete.png");

  // SCENARIO 5: Linux Arrow Key Candidate Browsing and Enter Navigation
  console.log("\n--- Scenario 5: Linux Arrow Key Candidate Browsing and Enter ---");
  await view.navigate("http://127.0.0.1:5213/qa-ssh-autocomplete.html?host=linux");
  await waitForSelector("[data-testid=\"remote-repo-path-input\"]");
  await Bun.sleep(400);

  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    if (el) {
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", code: "ArrowDown", bubbles: true, cancelable: true }));
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", code: "ArrowDown", bubbles: true, cancelable: true }));
    }
  })()`);
  await Bun.sleep(200);

  const sc5Selection: any = await view.evaluate(`(() => {
    const selected = document.querySelector("[role=option][aria-selected=\\"true\\"]");
    return {
      selectedOption: selected ? selected.getAttribute("aria-label") : null,
    };
  })()`);
  console.log("Scenario 5 Selection after ArrowDown x2:", sc5Selection);

  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    if (el) {
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true }));
    }
  })()`);
  await Bun.sleep(400);

  const sc5Data: any = await view.evaluate(`(() => {
    const input = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    return {
      inputValue: input ? input.value : "",
      isRegistered: Boolean(document.body.dataset.registered),
    };
  })()`);
  console.log("Scenario 5 Data after Enter:", sc5Data);
  const sc5Pass = sc5Selection.selectedOption === "Documents" &&
    sc5Data.inputValue === "/home/developer/Documents/" &&
    sc5Data.isRegistered === false;
  results.push({ scenario: 5, name: "Linux Arrow Key and Enter Browsing", pass: sc5Pass, data: { ...sc5Selection, ...sc5Data } });
  await saveScreenshot("scenario-05-linux-arrow-navigation-enter.png");

  // SCENARIO 6: Escape Closes Dropdown, Reopening on Click
  console.log("\n--- Scenario 6: Escape Closes Dropdown, Reopening on Click ---");
  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    if (el) {
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true }));
    }
  })()`);
  await Bun.sleep(200);

  const sc6EscData: any = await view.evaluate(`(() => ({
    hasListbox: Boolean(document.querySelector("[role=listbox]")),
    isClosed: Boolean(document.body.dataset.closed),
  }))()`);

  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    if (el) el.click();
  })()`);
  await Bun.sleep(200);

  const sc6ReopenData: any = await view.evaluate(`(() => ({
    hasListbox: Boolean(document.querySelector("[role=listbox]")),
  }))()`);
  console.log("Scenario 6 Escape & Reopen Data:", { sc6EscData, sc6ReopenData });
  const sc6Pass = sc6EscData.hasListbox === false &&
    sc6EscData.isClosed === false &&
    sc6ReopenData.hasListbox === true;
  results.push({ scenario: 6, name: "Escape Closes Dropdown and Click Reopens", pass: sc6Pass, data: { sc6EscData, sc6ReopenData } });
  await saveScreenshot("scenario-06-linux-escape-close-and-reopen.png");

  // SCENARIO 7: Error State, Retry, and Malformed Path Validation
  console.log("\n--- Scenario 7: Error State and Retry ---");
  await view.evaluate(`(() => {
    const el = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value") && Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    if (setter && el) {
      setter.call(el, "/home/developer/denied/");
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    }
  })()`);
  await Bun.sleep(400);

  const sc7Data: any = await view.evaluate(`(() => {
    const alert = document.querySelector("[role=alert]");
    const confirmBtn = document.querySelector("[data-testid=\\"add-project-confirm-remote\\"]");
    const retryBtn = Array.from(document.querySelectorAll("button")).find(b => b.textContent && b.textContent.trim() === "Retry");
    return {
      hasAlert: Boolean(alert),
      alertText: alert ? alert.textContent.trim() : "",
      hasRetry: Boolean(retryBtn),
      confirmDisabled: confirmBtn ? confirmBtn.disabled : true,
    };
  })()`);
  console.log("Scenario 7 Data:", sc7Data);
  const sc7Pass = sc7Data.hasAlert === true &&
    (sc7Data.alertText ? sc7Data.alertText.includes("Permission denied") : false) &&
    sc7Data.hasRetry === true &&
    sc7Data.confirmDisabled === true;
  results.push({ scenario: 7, name: "Error Handling and Retry Display", pass: sc7Pass, data: sc7Data });
  await saveScreenshot("scenario-07-error-and-retry.png");

  // SCENARIO 8: Host Switch Discards Cache and Restores Target Home
  console.log("\n--- Scenario 8: Host Switch Isolation ---");
  await view.evaluate(`(() => {
    const select = document.querySelector("[data-testid=\\"remote-host-select\\"]");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype, "value") && Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype, "value").set;
    if (setter && select) {
      setter.call(select, "windows");
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }
  })()`);
  await Bun.sleep(500);

  const sc8Data: any = await view.evaluate(`(() => {
    const select = document.querySelector("[data-testid=\\"remote-host-select\\"]");
    const input = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const options = Array.from(document.querySelectorAll("[role=option]")).map(o => o.getAttribute("aria-label"));
    return {
      hostValue: select ? select.value : "",
      inputValue: input ? input.value : "",
      options,
    };
  })()`);
  console.log("Scenario 8 Data:", sc8Data);
  const sc8Pass = sc8Data.hostValue === "windows" &&
    sc8Data.inputValue === "C:\\Users\\developer" &&
    sc8Data.options.includes("code");
  results.push({ scenario: 8, name: "Host Switch Cache Invalidation & Home Prefill", pass: sc8Pass, data: sc8Data });
  await saveScreenshot("scenario-08-host-switch-isolation.png");

  // SCENARIO 9: Final Project Registration Distinct from Autocomplete
  console.log("\n--- Scenario 9: Final Project Registration ---");
  await view.evaluate(`(() => {
    const codeOption = Array.from(document.querySelectorAll("[role=option]")).find(o => o.getAttribute("aria-label") === "code");
    if (codeOption) codeOption.click();
  })()`);
  await Bun.sleep(400);

  const sc9NavData: any = await view.evaluate(`(() => {
    const input = document.querySelector("[data-testid=\\"remote-repo-path-input\\"]");
    const confirmBtn = document.querySelector("[data-testid=\\"add-project-confirm-remote\\"]");
    return {
      inputValue: input ? input.value : "",
      confirmDisabled: confirmBtn ? confirmBtn.disabled : true,
      registeredBefore: document.body.dataset.registered,
    };
  })()`);
  console.log("Scenario 9 Navigation into Code:", sc9NavData);

  await view.evaluate(`(() => {
    const confirmBtn = document.querySelector("[data-testid=\\"add-project-confirm-remote\\"]");
    if (confirmBtn) confirmBtn.click();
  })()`);
  await Bun.sleep(400);

  const sc9Data: any = await view.evaluate(`(() => {
    const raw = document.body.dataset.registered;
    return {
      registered: raw ? JSON.parse(raw) : null,
    };
  })()`);
  console.log("Scenario 9 Final Registered Data:", sc9Data);
  const sc9Pass = sc9NavData.registeredBefore === undefined &&
    sc9Data.registered !== null &&
    sc9Data.registered.workspaceId === "ssh:qa" &&
    sc9Data.registered.repoRoot === "C:\\Users\\developer\\code";
  results.push({ scenario: 9, name: "Final Project Registration Distinct From Browsing", pass: sc9Pass, data: sc9Data });
  await saveScreenshot("scenario-09-final-project-registration.png");

  view.close();

  console.log("\n================ QA RESULTS ================");
  let allPass = true;
  for (const r of results) {
    console.log(`Scenario ${r.scenario}: [${r.pass ? "PASS" : "FAIL"}] ${r.name}`);
    if (!r.pass) allPass = false;
  }
  console.log(`\nTOTAL: ${results.filter(r => r.pass).length}/${results.length} PASSED (All Pass: ${allPass})`);

  fs.writeFileSync(path.join(outputDir, "browser-qa-results.json"), JSON.stringify({
    timestamp: new Date().toISOString(),
    allPass,
    results,
  }, null, 2));

} finally {
  console.log("Terminating Vite QA process...");
  viteProcess.kill("SIGTERM");
}
