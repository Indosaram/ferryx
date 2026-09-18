// Task-5 browser evidence driver (Playwright, isolated browser context).
//
// Every wait subscribes to the exact media event BEFORE the action that produces it and
// resolves on that event with a bounded failure timeout. There is no sleep, no polling and
// no "wait a bit then check" anywhere in this file.
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import os from "node:os";
import { pathToFileURL } from "node:url";

// Playwright is a machine-level tool here, deliberately not a dependency of ui/package.json:
// this harness must not mutate the shared lockfile other lanes are building against.
const playwrightEntry =
  process.env.QA_PLAYWRIGHT ??
  path.join(os.homedir(), ".bun/install/global/node_modules/playwright/index.js");
const { chromium } = await import(pathToFileURL(playwrightEntry).href).then((m) => m.default ?? m);

const baseUrl = process.argv[2];
const outDir = process.argv[3];
// The same lifecycle is exercised at a desktop and a narrow width; the label keeps both
// evidence sets side by side instead of overwriting each other.
const label = process.argv[4] ?? "desktop";
const width = Number(process.argv[5] ?? 1280);
const height = Number(process.argv[6] ?? 800);
if (!baseUrl || !outDir) throw new Error("usage: drive.mjs <baseUrl> <outDir> [label] [width] [height]");
const shotDir = path.join(outDir, "screenshots", label);
mkdirSync(shotDir, { recursive: true });

const actions = [];
function record(step, detail) {
  actions.push({ step, at: new Date().toISOString(), ...detail });
  console.log(`[step] ${step} ${JSON.stringify(detail)}`);
}
function assert(condition, message, detail) {
  if (!condition) {
    actions.push({ step: "ASSERTION_FAILED", message, ...detail });
    writeFileSync(path.join(outDir, `actions-${label}.json`), JSON.stringify(actions, null, 2));
    throw new Error(`assertion failed: ${message} ${JSON.stringify(detail ?? {})}`);
  }
}

/** Subscribes in-page to `type`, runs `trigger`, resolves with a media snapshot. */
const awaitMediaEvent = `
(async ({ type, trigger, timeoutMs, minTimeUpdates }) => {
  const video = document.querySelector('[data-testid="file-preview-video-element"]');
  if (!video) throw new Error("no video element");
  const seen = [];
  const settled = new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      video.removeEventListener(type, onEvent);
      reject(new Error("timed out waiting for " + type + " after " + timeoutMs + "ms"));
    }, timeoutMs);
    function onEvent() {
      seen.push({ currentTime: video.currentTime, readyState: video.readyState, paused: video.paused });
      if (seen.length < (minTimeUpdates ?? 1)) return;
      clearTimeout(timer);
      video.removeEventListener(type, onEvent);
      resolve(seen);
    }
    video.addEventListener(type, onEvent);
  });
  if (trigger === "play") await video.play();
  if (trigger === "pause") video.pause();
  if (typeof trigger === "number") video.currentTime = trigger;
  const events = await settled;
  return {
    events,
    currentTime: video.currentTime,
    duration: video.duration,
    paused: video.paused,
    readyState: video.readyState,
    videoWidth: video.videoWidth,
    videoHeight: video.videoHeight,
  };
})`;

/** Resolves on the next presented video frame (proof the engine decoded and painted). */
const awaitPresentedFrame = `
(async ({ timeoutMs }) => {
  const video = document.querySelector('[data-testid="file-preview-video-element"]');
  if (!video) throw new Error("no video element");
  if (typeof video.requestVideoFrameCallback !== "function") throw new Error("no requestVideoFrameCallback");
  return await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("timed out waiting for a presented frame")), timeoutMs);
    video.requestVideoFrameCallback((_now, metadata) => {
      clearTimeout(timer);
      resolve({ mediaTime: metadata.mediaTime, presentedFrames: metadata.presentedFrames, width: metadata.width, height: metadata.height });
    });
  });
})`;

// QA_CHROMIUM pins an already-downloaded browser build so the run never reaches the network.
const browser = await chromium.launch({
  headless: true,
  ...(process.env.QA_CHROMIUM ? { executablePath: process.env.QA_CHROMIUM } : {}),
});
const context = await browser.newContext({ viewport: { width, height } });
const page = await context.newPage();
const consoleErrors = [];
page.on("console", (message) => {
  if (message.type() === "error") consoleErrors.push(message.text());
});
page.on("pageerror", (error) => consoleErrors.push(String(error)));

try {
  await page.goto(baseUrl, { waitUntil: "load" });
  await page.waitForSelector('[data-testid="file-preview-video-element"]');

  // --- 1. metadata only, no autoplay -------------------------------------------------
  const metadata = await page.evaluate(
    `(async () => {
      const video = document.querySelector('[data-testid="file-preview-video-element"]');
      const snapshot = () => ({
        preload: video.getAttribute("preload"),
        controls: video.hasAttribute("controls"),
        autoplay: video.autoplay,
        paused: video.paused,
        currentTime: video.currentTime,
        duration: video.duration,
        readyState: video.readyState,
        videoWidth: video.videoWidth,
        src: video.getAttribute("src"),
      });
      if (video.readyState >= 1) return snapshot();
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("timed out waiting for loadedmetadata")), 10000);
        video.addEventListener("loadedmetadata", () => { clearTimeout(timer); resolve(); }, { once: true });
      });
      return snapshot();
    })()`,
  );
  assert(metadata.preload === "metadata", "preload is metadata", metadata);
  assert(metadata.controls === true, "native controls are enabled", metadata);
  assert(metadata.autoplay === false && metadata.paused === true, "no autoplay", metadata);
  assert(metadata.currentTime === 0, "playback has not started", metadata);
  assert(Math.abs(metadata.duration - 6) < 0.2, "fixture duration decoded", metadata);
  assert(metadata.videoWidth === 320, "video track decoded at 320px wide", metadata);
  record("metadata-loaded", metadata);
  await page.screenshot({ path: path.join(shotDir, "01-metadata-paused.png") });

  // --- 2. play through the native control bar ----------------------------------------
  // Preferred trigger: a real user gesture on the native control surface (focus the video,
  // press Space, which the built-in controls map to play). If the shipped control surface
  // does not take the gesture in this build, fall back to the element API and say so.
  const video = page.locator('[data-testid="file-preview-video-element"]');
  await video.focus();
  let playTrigger = "native-controls-space-key";
  let playing;
  try {
    playing = await Promise.all([
      page.evaluate(awaitMediaEvent + "({ type: 'playing', timeoutMs: 8000 })"),
      page.keyboard.press("Space"),
    ]).then(([result]) => result);
  } catch (error) {
    playTrigger = `element-play-api (native gesture unavailable: ${error.message})`;
    playing = await page.evaluate(awaitMediaEvent + "({ type: 'playing', trigger: 'play', timeoutMs: 15000 })");
  }
  assert(playing.paused === false, "engine reports playback running", playing);
  record("play-started", { trigger: playTrigger, ...playing });

  const progressed = await page.evaluate(
    awaitMediaEvent + "({ type: 'timeupdate', timeoutMs: 15000, minTimeUpdates: 3 })",
  );
  const times = progressed.events.map((event) => event.currentTime);
  assert(times[times.length - 1] > times[0], "currentTime advanced while playing", { times });
  assert(progressed.readyState >= 3, "engine has future data (really decoding)", progressed);
  const frame = await page.evaluate(awaitPresentedFrame + "({ timeoutMs: 10000 })");
  assert(frame.presentedFrames > 0, "frames were presented to the compositor", frame);
  assert(frame.width === 320 && frame.height === 240, "presented frame has fixture dimensions", frame);
  record("playing", { times, presented: frame });
  await page.screenshot({ path: path.join(shotDir, "02-playing.png") });

  // --- 3. seek (drives real HTTP range requests) --------------------------------------
  const paused = await page.evaluate(awaitMediaEvent + "({ type: 'pause', trigger: 'pause', timeoutMs: 10000 })");
  assert(paused.paused === true, "paused before seeking", paused);
  const seeked = await page.evaluate(awaitMediaEvent + "({ type: 'seeked', trigger: 4.2, timeoutMs: 15000 })");
  assert(Math.abs(seeked.currentTime - 4.2) < 0.35, "seeked to 4.2s", seeked);
  // rVFC only fires for presented frames, so resume briefly: a frame whose mediaTime is past
  // the seek target proves the engine decoded from the new position, not from the old buffer.
  await page.evaluate(awaitMediaEvent + "({ type: 'playing', trigger: 'play', timeoutMs: 15000 })");
  const seekFrame = await page.evaluate(awaitPresentedFrame + "({ timeoutMs: 10000 })");
  assert(seekFrame.mediaTime > 4.0, "presented frame comes from the seek target", seekFrame);
  const pausedAfterSeek = await page.evaluate(
    awaitMediaEvent + "({ type: 'pause', trigger: 'pause', timeoutMs: 10000 })",
  );
  assert(pausedAfterSeek.currentTime > 4.0, "playback continued past the seek target", pausedAfterSeek);
  record("seeked", { seeked, presented: seekFrame, pausedAt: pausedAfterSeek.currentTime });
  await page.screenshot({ path: path.join(shotDir, "03-seeked.png") });

  // --- 4. recoverable failure on an undecodable source --------------------------------
  await page.evaluate(
    `window.__qa.replace(window.__qa.payloadFor("bad-source", "broken.webm", "/not-a-video", "video/webm"))`,
  );
  await page.waitForSelector('[data-testid="file-preview-video-error"]', { timeout: 15000 });
  const failureState = await page.evaluate(
    `({
      alert: document.querySelector('[data-testid="file-preview-video-error"]').textContent,
      log: window.__qa.log.filter((entry) => entry.event === "onFailure"),
      mediaErrorCode: document.querySelector('[data-testid="file-preview-video-element"]').error?.code ?? null,
    })`,
  );
  assert(failureState.log.length === 1, "one failure reported", failureState);
  assert(failureState.log[0].detail.reason === "UnsupportedFormat", "failure reason is UnsupportedFormat", failureState);
  assert(/cannot be played/i.test(failureState.alert), "recoverable error is visible", failureState);
  record("unsupported-source-failure", failureState);
  await page.screenshot({ path: path.join(shotDir, "04-error-recoverable.png") });

  await page.click('[data-testid="file-preview-video-retry"]');
  await page.click('[data-testid="file-preview-video-external"]');
  const callbacks = await page.evaluate(`window.__qa.log.map((entry) => entry.event)`);
  assert(callbacks.includes("onReload"), "retry invoked onReload", { callbacks });
  assert(callbacks.includes("onExternalOpen"), "external open invoked onExternalOpen", { callbacks });
  record("recovery-callbacks", { callbacks });

  // --- 5. replacement tears the old element down --------------------------------------
  const staleHandle = await page.evaluateHandle(
    `document.querySelector('[data-testid="file-preview-video-element"]')`,
  );
  await page.evaluate(
    `window.__qa.replace(window.__qa.payloadFor("fixture-webm-2", "preview-fixture.webm", "/media/preview-fixture.webm", "video/webm"))`,
  );
  // Wait for the REPLACEMENT element specifically: the old one is still mounted until React
  // flushes, so a bare testid match would observe the element that is about to be torn down.
  await page.waitForSelector(
    '[data-testid="file-preview-video-element"][src="/media/preview-fixture.webm"]',
  );
  const staleState = await staleHandle.evaluate(
    (element) => ({ connected: element.isConnected, src: element.getAttribute("src"), paused: element.paused, currentSrc: element.currentSrc }),
  );
  assert(staleState.connected === false, "replaced element detached", staleState);
  assert(staleState.src === null, "replaced element lost its capability src", staleState);
  assert(staleState.paused === true, "replaced element paused", staleState);
  const replacedAlert = await page.evaluate(
    `document.querySelector('[data-testid="file-preview-video-error"]') === null`,
  );
  assert(replacedAlert === true, "stale failure cleared on replacement", { replacedAlert });
  record("replacement-teardown", staleState);

  const replayed = await page.evaluate(awaitMediaEvent + "({ type: 'playing', trigger: 'play', timeoutMs: 15000 })");
  const replayedProgress = await page.evaluate(
    awaitMediaEvent + "({ type: 'timeupdate', timeoutMs: 15000, minTimeUpdates: 2 })",
  );
  assert(replayed.paused === false, "replacement plays", replayed);
  const replayedTimes = replayedProgress.events.map((event) => event.currentTime);
  assert(replayedTimes[replayedTimes.length - 1] > 0, "replacement advances", { replayedTimes });
  record("replacement-playing", { replayedTimes });
  await page.screenshot({ path: path.join(shotDir, "05-replacement-playing.png") });

  // --- 6. close: pause, detach src, load ----------------------------------------------
  const closingHandle = await page.evaluateHandle(
    `document.querySelector('[data-testid="file-preview-video-element"]')`,
  );
  await page.evaluate(`window.__qa.close()`);
  await page.waitForSelector('[data-testid="qa-closed"]');
  const closedState = await closingHandle.evaluate(
    (element) => ({ connected: element.isConnected, src: element.getAttribute("src"), paused: element.paused, networkState: element.networkState, readyState: element.readyState }),
  );
  assert(closedState.paused === true, "closed preview paused the element", closedState);
  assert(closedState.src === null, "closed preview detached the capability src", closedState);
  assert(closedState.readyState === 0, "closed preview reset the media engine", closedState);
  record("closed", closedState);
  await page.screenshot({ path: path.join(shotDir, "06-closed.png") });

  const failuresAfterClose = await page.evaluate(
    `window.__qa.log.filter((entry) => entry.event === "onFailure").length`,
  );
  assert(failuresAfterClose === 1, "teardown produced no stale failure", { failuresAfterClose });
  record("stale-events-safe", { failuresAfterClose, consoleErrors });

  const qaLog = await page.evaluate(`window.__qa.log`);
  writeFileSync(path.join(outDir, `harness-callback-log-${label}.json`), JSON.stringify(qaLog, null, 2));
  writeFileSync(path.join(outDir, `actions-${label}.json`), JSON.stringify(actions, null, 2));
  console.log(`BROWSER_EVIDENCE_OK ${label} ${width}x${height}`);
} finally {
  await context.close();
  await browser.close();
}
