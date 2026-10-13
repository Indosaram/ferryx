#!/usr/bin/env bun
/**
 * scripts/qa/mobile-chat-visual-qa.mjs
 *
 * Real-browser (system Chrome via playwright-core) QA for the mobile chat surface.
 * Probes the rendered DOM at phone widths and writes screenshots + a JSON report.
 *
 *   bun scripts/qa/mobile-chat-visual-qa.mjs --evidence-dir <dir> [--url <harness url>]
 *
 * The Vite dev server and the harness page (ui/mobile-chat-qa.html) must already be running.
 */

import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(fileURLToPath(import.meta.url), "..", "..", "..");

function parseArgs(argv) {
  const args = { evidenceDir: null, url: "http://127.0.0.1:5173/mobile-chat-qa.html" };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--evidence-dir") args.evidenceDir = argv[++i];
    else if (argv[i] === "--url") args.url = argv[++i];
    else if (argv[i] === "--help" || argv[i] === "-h") {
      console.log("usage: bun scripts/qa/mobile-chat-visual-qa.mjs --evidence-dir <dir> [--url <harness url>]");
      process.exit(0);
    } else {
      console.error(`mobile-chat-visual-qa: unrecognized argument: ${argv[i]}`);
      process.exit(2);
    }
  }
  if (!args.evidenceDir) {
    console.error("mobile-chat-visual-qa: --evidence-dir is required");
    process.exit(2);
  }
  return args;
}

function loadPlaywright() {
  const require = createRequire(join(repo, "package.json"));
  const candidates = [process.env.PLAYWRIGHT_CORE_PATH, join(repo, "ui", "node_modules", "playwright-core"), "playwright-core"].filter(Boolean);
  const failures = [];
  for (const candidate of candidates) {
    try {
      return require(candidate);
    } catch (error) {
      failures.push(`${candidate}: ${error.message}`);
    }
  }
  throw new Error(`playwright-core not resolvable\n${failures.join("\n")}`);
}

const UNNAMED_BUTTON_PROBE = () => {
  const workspace = document.querySelector('[data-testid="mobile-chat-workspace"]');
  if (!workspace) return { error: "mobile-chat-workspace not found" };
  const buttons = Array.from(workspace.querySelectorAll("button"));
  const nameOf = (el) => {
    const labelledBy = el.getAttribute("aria-labelledby");
    const labelledText = labelledBy
      ? labelledBy
          .split(/\s+/)
          .map((id) => document.getElementById(id)?.textContent ?? "")
          .join(" ")
      : "";
    return (
      el.getAttribute("aria-label") ||
      labelledText.trim() ||
      (el.textContent ?? "").trim() ||
      el.getAttribute("title") ||
      ""
    ).trim();
  };
  const unnamed = buttons
    .map((el, index) => ({ index, testid: el.getAttribute("data-testid") ?? null, name: nameOf(el) }))
    .filter((entry) => entry.name.length === 0);
  return { total: buttons.length, unnamed };
};

const LAYOUT_PROBE = (expectedWidth) => {
  const rect = (selector) => {
    const el = document.querySelector(selector);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { top: r.top, left: r.left, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
  };
  const overlaps = (a, b) => Boolean(a && b) && a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
  const pill = rect('[data-testid="scroll-to-latest-pill"]');
  const composer = rect('[data-testid="mobile-chat-composer"]');
  const drawerEl = document.querySelector('[data-testid="remote-sidebar-drawer"], [data-testid="terminal-drawer"]');
  const drawer = rect('[data-testid="remote-sidebar-drawer"], [data-testid="terminal-drawer"]');
  const drawerInteractive = Boolean(drawerEl) && !drawerEl.hasAttribute("inert");
  const composerEl = document.querySelector('[data-testid="mobile-chat-composer"]');
  const streamEl = document.querySelector('[data-testid="chat-message-stream"]');
  const sendOrStopBtn =
    document.querySelector('[data-testid="send-button"]') ||
    document.querySelector('[data-testid="stop-button"]');
  const sendOrStopRect = sendOrStopBtn ? sendOrStopBtn.getBoundingClientRect() : null;
  const sendOrStopBottomWithinViewport = sendOrStopRect
    ? sendOrStopRect.bottom <= window.innerHeight + 1
    : null;
  const textareaEl = document.querySelector('[data-testid="chat-composer-textarea"]');
  const textareaScrollable = textareaEl
    ? textareaEl.scrollHeight > textareaEl.clientHeight
    : null;
  const isSidebarDrawer = drawerEl?.getAttribute("data-testid") === "remote-sidebar-drawer";
  const attachBtn = document.querySelector('[data-testid="attach-file-button"]');
  const toolbarContainer = attachBtn ? attachBtn.parentElement : null;
  const toolbarRect = toolbarContainer ? toolbarContainer.getBoundingClientRect() : null;
  const toolbarVisible = toolbarRect
    ? toolbarRect.bottom <= window.innerHeight + 1 && toolbarRect.top < toolbarRect.bottom
    : null;
  return {
    pill,
    composer,
    drawer,
    drawerInteractive,
    pillOverlapsComposer: overlaps(pill, composer),
    drawerOverlapsComposer: drawerInteractive && !isSidebarDrawer ? overlaps(drawer, composer) : false,
    closedDrawerIsInert: drawerEl ? drawerEl.hasAttribute("inert") : undefined,
    hiddenDrawerIsTransparent: drawerEl ? window.getComputedStyle(drawerEl).opacity === "0" : undefined,
    documentHorizontalOverflow:
      document.documentElement.scrollWidth > window.innerWidth + 1 ||
      (typeof expectedWidth === "number" && document.documentElement.scrollWidth > expectedWidth + 1),
    streamScrollable: streamEl ? streamEl.scrollHeight > streamEl.clientHeight : null,
    sendOrStopBottomWithinViewport,
    textareaScrollable,
    toolbarVisible,
    viewport: { width: window.innerWidth, height: window.innerHeight },
  };
};

const FOCUS_PROBE = (selector) => {
  const el = document.querySelector(selector);
  if (!el) return null;
  el.focus();
  const style = window.getComputedStyle(el);
  const hasOutline = style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0;
  const hasRing = style.boxShadow.includes("115, 115, 115");
  return {
    selector,
    outlineWidth: style.outlineWidth,
    outlineStyle: style.outlineStyle,
    boxShadow: style.boxShadow,
    hasVisibleIndicator: hasOutline || hasRing,
  };
};

const OCCLUSION_PROBE = async () => {
  const stream = document.querySelector('[data-testid="chat-message-stream"]');
  const composer = document.querySelector('[data-testid="mobile-chat-composer"]');
  if (!stream || !composer) return { error: "stream or composer missing" };
  stream.scrollTop = stream.scrollHeight;
  let previous = -1;
  for (let attempt = 0; attempt < 30; attempt += 1) {
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    if (stream.scrollTop === previous && stream.scrollTop >= stream.scrollHeight - stream.clientHeight - 1) break;
    previous = stream.scrollTop;
    stream.scrollTop = stream.scrollHeight;
  }
  const copies = Array.from(document.querySelectorAll('[data-testid="message-copy-button"]'));
  const last = copies[copies.length - 1] ?? null;
  if (!last) return { error: "no message copy control rendered" };
  const copyRect = last.getBoundingClientRect();
  const composerRect = composer.getBoundingClientRect();
  const hit = document.elementFromPoint(copyRect.left + copyRect.width / 2, copyRect.top + copyRect.height / 2);
  return {
    maxScroll: stream.scrollHeight - stream.clientHeight,
    scrollTop: stream.scrollTop,
    copyBottom: copyRect.bottom,
    composerTop: composerRect.top,
    fullyAboveComposer: copyRect.bottom <= composerRect.top,
    hitTestOwnsCopyControl: Boolean(hit && (hit === last || last.contains(hit))),
    hitTestTarget: hit ? `${hit.tagName.toLowerCase()}${hit.getAttribute("data-testid") ? `[${hit.getAttribute("data-testid")}]` : ""}` : null,
  };
};
const TOP_BAR_PROBE = (expectedWidth) => {
  const topBars = Array.from(document.querySelectorAll('[data-testid="mobile-top-bar"]'));
  if (topBars.length !== 1) {
    return {
      count: topBars.length,
      error: `expected exactly 1 mobile-top-bar, found ${topBars.length}`,
    };
  }
  const topBar = topBars[0];
  const row = topBar.firstElementChild || topBar;
  const rowRect = row.getBoundingClientRect();
  const rowHeight = rowRect.height;
  const rowHeightOk = Math.abs(rowHeight - 56) <= 1;

  const buttons = Array.from(topBar.querySelectorAll("button"));
  const nameOf = (el) => {
    const labelledBy = el.getAttribute("aria-labelledby");
    const labelledText = labelledBy
      ? labelledBy
          .split(/\s+/)
          .map((id) => document.getElementById(id)?.textContent ?? "")
          .join(" ")
      : "";
    return (
      el.getAttribute("aria-label") ||
      labelledText.trim() ||
      (el.textContent ?? "").trim() ||
      el.getAttribute("title") ||
      ""
    ).trim();
  };

  const buttonDetails = buttons.map((el, i) => {
    const name = nameOf(el);
    const r = el.getBoundingClientRect();
    const hasName = name.length > 0;
    const minSizeOk = r.width >= 31 && r.height >= 31;
    return {
      index: i,
      name,
      width: r.width,
      height: r.height,
      hasName,
      minSizeOk,
      testId: el.getAttribute("data-testid") ?? null,
    };
  });
  const buttonFailures = buttonDetails.filter((b) => !b.hasName || !b.minSizeOk);

  const scrollingEl = document.scrollingElement || document.documentElement;
  const scrollWidth = scrollingEl.scrollWidth;
  const innerWidth = window.innerWidth;
  const targetWidth = typeof expectedWidth === "number" ? expectedWidth : innerWidth;
  const noHorizontalScroll =
    scrollWidth <= innerWidth &&
    (typeof expectedWidth !== "number" || scrollWidth <= expectedWidth);

  const titleEl =
    topBar.querySelector(".truncate.text-base") ||
    topBar.querySelector("span.truncate");
  const titleStyle = titleEl ? window.getComputedStyle(titleEl) : null;
  const textOverflow = titleStyle ? titleStyle.textOverflow : "";
  const clientWidth = titleEl ? titleEl.clientWidth : 0;
  const titleEllipsisOk = textOverflow === "ellipsis";
  const titleTruncated = Boolean(titleEl && titleEl.scrollWidth > clientWidth);
  const titleWidthOk = !titleTruncated || clientWidth >= 48;
  const contextSpan = topBar.querySelector('[aria-label="Current desktop context"]');
  const contextSpanClientWidth = contextSpan ? contextSpan.clientWidth : 0;
  const contextSpanVisible = !contextSpan || contextSpanClientWidth >= 24;
  const hostBadge = topBar.querySelector('[data-testid="remote-connection-badge"]');
  const hostLabel = hostBadge?.querySelector("span");
  const hostLabelClientWidth = hostLabel?.clientWidth ?? 0;
  const hostLabelScrollWidth = hostLabel?.scrollWidth ?? 0;
  const hostLabelTruncated = Boolean(hostLabel && hostLabelScrollWidth > hostLabelClientWidth);
  const hostLabelVisible = !hostBadge || !hostLabelTruncated || hostLabelClientWidth >= 24;

  return {
    count: topBars.length,
    rowHeight,
    rowHeightOk,
    buttonCount: buttons.length,
    buttonFailures,
    scrollWidth,
    innerWidth,
    targetWidth,
    noHorizontalScroll,
    titleTextOverflow: textOverflow,
    titleClientWidth: clientWidth,
    titleEllipsisOk,
    titleWidthOk,
    titleTruncated,
    contextSpanPresent: Boolean(contextSpan),
    contextSpanClientWidth,
    contextSpanVisible,
    hostBadgePresent: Boolean(hostBadge),
    hostLabelClientWidth,
    hostLabelScrollWidth,
    hostLabelTruncated,
    hostLabelVisible,
  };
};

async function run() {
  const args = parseArgs(process.argv.slice(2));
  mkdirSync(args.evidenceDir, { recursive: true });
  const { chromium } = loadPlaywright();
  const browser = await chromium.launch({ channel: "chrome", headless: true });
  const report = { url: args.url, probes: {}, shots: [], consoleErrors: [] };

  try {
    for (const [label, viewport] of [
      ["390x844", { width: 390, height: 844 }],
      ["375x812", { width: 375, height: 812 }],
      ["320x844", { width: 320, height: 844 }],
    ]) {
      const context = await browser.newContext({ viewport, deviceScaleFactor: 2, hasTouch: true, isMobile: true });
      const page = await context.newPage();
      page.on("console", (message) => {
        if (message.type() === "error") report.consoleErrors.push(`${label}: ${message.text()}`);
      });
      page.on("pageerror", (error) => report.consoleErrors.push(`${label}: pageerror ${error.message}`));

      for (const state of [
        "stream",
        "running",
        "long",
        "empty",
        "topbar-attention",
        "topbar-long-title",
        "switcher-open",
        "menu-open",
        "terminal-mode",
        "drawer-open",
      ]) {
        await page.goto(`${args.url}?state=${state}`, { waitUntil: "domcontentloaded" });
        await page.waitForSelector('[data-testid="mobile-top-bar"]', { timeout: 15000 });
        if (state === "terminal-mode") {
          await page.waitForSelector('[data-testid="qa-terminal-placeholder"]', { timeout: 15000 });
        } else if (state === "drawer-open") {
          await page.waitForSelector('[data-testid="remote-sidebar-drawer"]', { timeout: 15000 });
        } else {
          await page.waitForSelector('[data-testid="mobile-chat-workspace"]', { timeout: 15000 });
        }
        await page.waitForTimeout(900);
        const shot = join(args.evidenceDir, `mobile-chat-${state}-${label}.png`);
        await page.screenshot({ path: shot });
        report.shots.push(shot);
        if (label === "390x844" && state !== "terminal-mode") {
          report.probes[`unnamed-buttons-${state}`] = await page.evaluate(UNNAMED_BUTTON_PROBE);
        }
        if (state !== "terminal-mode") {
          report.probes[`layout-${state}-${label}`] = await page.evaluate(LAYOUT_PROBE, viewport.width);
        }
        report.probes[`topbar-${state}-${label}`] = await page.evaluate(TOP_BAR_PROBE, viewport.width);

        if (state === "switcher-open" || state === "menu-open") {
          const sheetBefore = await page.evaluate(() => {
            const dialogs = Array.from(document.querySelectorAll('[role="dialog"]'));
            if (dialogs.length !== 1) {
              return { count: dialogs.length, error: `expected exactly 1 role="dialog", found ${dialogs.length}` };
            }
            const dialog = dialogs[0];
            const r = dialog.getBoundingClientRect();
            const bottom = r.bottom;
            const innerHeight = window.innerHeight;
            const bottomMatchesInnerHeight = Math.abs(bottom - innerHeight) <= 1;
            return {
              count: dialogs.length,
              bottom,
              innerHeight,
              bottomMatchesInnerHeight,
              ariaLabel: dialog.getAttribute("aria-label"),
            };
          });
          await page.keyboard.press("Escape");
          await page.waitForTimeout(350);
          const sheetAfter = await page.evaluate(() => {
            const dialogs = Array.from(document.querySelectorAll('[role="dialog"]'));
            return {
              count: dialogs.length,
              dialogGone: dialogs.length === 0,
            };
          });
          report.probes[`sheet-${state}-${label}`] = {
            before: sheetBefore,
            after: sheetAfter,
          };
        }
      }

      await page.goto(`${args.url}?state=long`, { waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="mobile-chat-workspace"]', { timeout: 15000 });
      const stream = await page.$('[data-testid="chat-message-stream"]');
      await stream?.evaluate((el) => {
        el.scrollTop = 0;
      });
      await page.waitForTimeout(500);
      const scrollShot = join(args.evidenceDir, `mobile-chat-scrolled-${label}.png`);
      await page.screenshot({ path: scrollShot });
      report.shots.push(scrollShot);
      if (label === "390x844") {
        report.probes[`layout-scrolled-${label}`] = await page.evaluate(LAYOUT_PROBE, viewport.width);
        await page.fill('[data-testid="chat-composer-textarea"]', "focus probe");
        await page.waitForTimeout(120);
        await page.keyboard.press("Tab");
        report.probes["focus-send"] = await page.evaluate(FOCUS_PROBE, '[data-testid="send-button"]');
        report.probes["focus-attach"] = await page.evaluate(FOCUS_PROBE, '[data-testid="attach-file-button"]');
        report.probes["focus-composer"] = await page.evaluate(FOCUS_PROBE, '[data-testid="chat-composer-textarea"]');
        report.probes["focus-terminal-toggle"] = await page.evaluate(FOCUS_PROBE, '[data-testid="terminal-toggle-button"]');
        report.probes["focus-copy"] = await page.evaluate(FOCUS_PROBE, '[data-testid="message-copy-button"]');
      }

      const occlusion = await page.evaluate(OCCLUSION_PROBE);
      report.probes[`occlusion-latest-${label}`] = occlusion;

      await page.goto(`${args.url}?state=stream`, { waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="mobile-chat-workspace"]', { timeout: 15000 });
      await page.click('[data-testid="worked-for-toggle"]').catch(() => undefined);
      await page.waitForTimeout(250);
      await page.keyboard.press("Tab");
      report.probes[`focus-work-row-${label}`] = await page.evaluate(FOCUS_PROBE, '[data-testid="work-row"]');
      report.probes[`unnamed-buttons-work-expanded-${label}`] = await page.evaluate(UNNAMED_BUTTON_PROBE);
      const expandedShot = join(args.evidenceDir, `mobile-chat-work-expanded-${label}.png`);
      await page.screenshot({ path: expandedShot });
      report.shots.push(expandedShot);

      await page.goto(`${args.url}?state=stream`, { waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="mobile-top-bar"]', { timeout: 15000 });
      const toggle = await page.$('button[aria-label="Change workspace context"]');
      if (!toggle) {
        throw new Error(`${label}: missing required trigger button aria-label="Change workspace context"`);
      }
      await toggle.click();
      await page.waitForTimeout(450);
      await page.waitForSelector('[data-testid="remote-sidebar-drawer"]', { timeout: 15000 });
      const drawerShot = join(args.evidenceDir, `mobile-chat-drawer-${label}.png`);
      await page.screenshot({ path: drawerShot });
      report.shots.push(drawerShot);
      report.probes[`drawer-open-${label}`] = await page.evaluate(LAYOUT_PROBE, viewport.width);
      await page.keyboard.press("Tab");
      report.probes[`focus-drawer-expand-${label}`] = await page.evaluate(
        FOCUS_PROBE,
        '[data-testid="remote-sidebar-drawer"] button[aria-label="Close worktree list"], [data-testid="remote-sidebar-drawer"] button',
      );
      const closeBtn =
        (await page.$('[aria-label="Close worktree list"]')) ||
        (await page.$('[aria-label="Dismiss worktree list"]'));
      if (closeBtn) {
        await closeBtn.click();
      } else {
        await page.keyboard.press("Escape");
      }
      await page.waitForTimeout(450);
      report.probes[`drawer-closed-inert-${label}`] = await page.evaluate(() => {
        const drawer = document.querySelector('[data-testid="remote-sidebar-drawer"]');
        if (!drawer) return { drawerPresent: false, closed: true };
        const focusables = Array.from(drawer.querySelectorAll("button, a, input, textarea, [tabindex]"));
        return {
          drawerPresent: true,
          hasInertAttribute: drawer.hasAttribute("inert"),
          ariaHidden: drawer.getAttribute("aria-hidden"),
          focusableCount: focusables.length,
          focusableAreInert: focusables.every((el) => el.closest("[inert]") !== null),
        };
      });

      await page.goto(`${args.url}?state=stream`, { waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="mobile-chat-workspace"]', { timeout: 15000 });
      const fileInput = await page.$('[data-testid="file-upload-input"]');
      if (fileInput) {
        const png = Buffer.from(
          "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==",
          "base64",
        );
        const attachmentPath = join(args.evidenceDir, "qa-attachment.png");
        writeFileSync(attachmentPath, png);
        await fileInput.setInputFiles(attachmentPath);
        await page.waitForTimeout(300);
        const attachShot = join(args.evidenceDir, `mobile-chat-attachments-${label}.png`);
        await page.screenshot({ path: attachShot });
        report.shots.push(attachShot);
        report.probes[`attachments-${label}`] = await page.evaluate(() => {
          const banner = document.querySelector('[data-testid="chat-composer-attachments-blocked"]');
          const send = document.querySelector('[data-testid="send-button"]');
          const row = document.querySelector('[data-testid="chat-composer-attachments"]');
          return {
            bannerText: banner?.textContent?.trim() ?? null,
            bannerRole: banner?.getAttribute("role") ?? null,
            sendDisabled: send ? send.hasAttribute("disabled") : null,
            rowScrollWidth: row ? row.scrollWidth : null,
            rowClientWidth: row ? row.clientWidth : null,
          };
        });
      }

      await context.close();
    }
  } finally {
    await browser.close();
  }

  const reportPath = join(args.evidenceDir, "mobile-chat-qa-report.json");
  writeFileSync(reportPath, JSON.stringify(report, null, 2));

  const failures = [];
  const fail = (message) => failures.push(message);
  for (const label of ["320x844", "375x812", "390x844"]) {
    if (!report.probes[`drawer-open-${label}`]) fail(`missing required probe: drawer-open-${label}`);
    if (!report.probes[`focus-drawer-expand-${label}`]) fail(`missing required probe: focus-drawer-expand-${label}`);
    if (!report.probes[`drawer-closed-inert-${label}`]) fail(`missing required probe: drawer-closed-inert-${label}`);
  }
  for (const [key, value] of Object.entries(report.probes)) {
    if (key.startsWith("unnamed-buttons") && value?.unnamed?.length > 0) {
      fail(`${key}: ${value.unnamed.length} unnamed button(s) ${JSON.stringify(value.unnamed)}`);
    }
    if (key.startsWith("layout-") || key.startsWith("drawer-open")) {
      if (value?.pillOverlapsComposer === true) fail(`${key}: the scroll pill overlaps the composer`);
      if (value?.drawerOverlapsComposer === true) fail(`${key}: the open terminal drawer overlaps the composer`);
      if (value?.documentHorizontalOverflow === true) fail(`${key}: the document scrolls horizontally`);
      const isExpandedState = key.startsWith("layout-long-") || key.startsWith("layout-scrolled-");
      if (!isExpandedState && typeof value?.composer?.height === "number" && value.composer.height > 112) {
        fail(`${key}: the composer is ${Math.round(value.composer.height)}px tall (expected <= 112px)`);
      }
      if (value?.sendOrStopBottomWithinViewport === false) {
        fail(`${key}: the send/stop button's bottom is outside the viewport`);
      }
      if (isExpandedState && value?.composer) {
        if (value?.textareaScrollable !== true) {
          fail(`${key}: composer textarea does not scroll (scrollHeight <= clientHeight)`);
        }
        if (value?.toolbarVisible !== true) {
          fail(`${key}: composer toolbar is not visible`);
        }
      }
    }
    if (key.startsWith("topbar-")) {
      if (value?.count !== 1) {
        fail(`${key}: expected exactly one mobile-top-bar, found ${value?.count}`);
      }
      if (value?.rowHeightOk !== true) {
        fail(`${key}: row height is ${value?.rowHeight}px (expected 56 +-1)`);
      }
      if (value?.buttonFailures?.length > 0) {
        fail(`${key}: button probe failures: ${JSON.stringify(value.buttonFailures)}`);
      }
      if (value?.noHorizontalScroll !== true) {
        fail(`${key}: horizontal overflow (scrollWidth ${value?.scrollWidth} > targetWidth ${value?.targetWidth})`);
      }
      if (value?.titleEllipsisOk !== true) {
        fail(`${key}: title element text-overflow is '${value?.titleTextOverflow}' (expected ellipsis)`);
      }
      if (value?.titleWidthOk !== true) {
        fail(`${key}: titleWidthOk failed: truncated title clientWidth is ${value?.titleClientWidth}px (expected >= 48)`);
      }
      if (value?.hostLabelVisible !== true) {
        fail(
          `${key}: hostLabelVisible failed: badge label clientWidth is ${value?.hostLabelClientWidth}px (expected >= 24 or untruncated)`
        );
      }
      if (value?.contextSpanVisible !== true) {
        fail(
          `${key}: contextSpanVisible failed: context span clientWidth is ${value?.contextSpanClientWidth}px (expected >= 24)`
        );
      }
    }
    if (key.startsWith("sheet-")) {
      if (value?.before?.count !== 1) {
        fail(`${key}: expected 1 role="dialog" before escape, found ${value?.before?.count}`);
      }
      if (value?.before?.bottomMatchesInnerHeight !== true) {
        fail(`${key}: dialog bottom (${value?.before?.bottom}) does not match innerHeight (${value?.before?.innerHeight})`);
      }
      if (value?.after?.dialogGone !== true) {
        fail(`${key}: dialog still present after Escape (${value?.after?.count} remaining)`);
      }
    }
    if (key.startsWith("occlusion-") && value) {
      if (value.fullyAboveComposer !== true) fail(`${key}: the latest message copy control is not fully above the composer`);
      if (value.hitTestOwnsCopyControl !== true) fail(`${key}: elementFromPoint at the copy control resolves to ${value.hitTestTarget}`);
    }
    if (key.startsWith("focus-") && value && value.hasVisibleIndicator !== true) {
      fail(`${key}: no visible focus indicator`);
    }
    if (key.startsWith("drawer-closed-inert") && value?.drawerPresent && value.hasInertAttribute !== true) {
      fail(`${key}: the closed drawer is not inert`);
    }
    if (key.startsWith("attachments-") && value && value.sendDisabled !== true) {
      fail(`${key}: the attachment state send button is not disabled`);
    }
  }
  console.log(JSON.stringify({ reportPath, gate: failures.length === 0 ? "PASS" : "FAIL", failures, probes: report.probes, consoleErrors: report.consoleErrors }, null, 2));
  if (failures.length > 0) process.exit(1);
}

await run();
