import { mkdir, writeFile, rm, readlink } from 'node:fs/promises';
import { resolve, join, basename } from 'node:path';
import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { WebOutcomeGateway } from './web-outcome-gateway.mjs';
import { driveTerminalInput } from './web-outcome-browser.mjs';
import { executablePathMatches } from '../herdr-reference-fixtures.mjs';

/** Asynchronously probe process identity to prevent PID reuse kills. */
export async function probeOwnedIdentity(pid) {
  if (typeof pid !== 'number' || pid <= 0) return { pid, alive: false, executable: null };
  return new Promise(resolve => {
    if (process.platform === 'win32') {
      const child = spawn('powershell', ['-NoProfile', '-Command', `(Get-CimInstance Win32_Process -Filter "ProcessId = ${pid}").ExecutablePath`]);
      let out = '';
      child.stdout?.on('data', chunk => { out += chunk; });
      child.on('close', code => {
        const exe = out.trim();
        resolve({ pid, alive: code === 0 && exe.length > 0, executable: exe.length > 0 ? exe : null });
      });
      child.on('error', () => resolve({ pid, alive: false, executable: null }));
    } else {
      if (process.platform === 'linux') {
        // ps comm= is only the process name; identity checks need the exact executable path.
        readlink(`/proc/${pid}/exe`).then(
          (exe) => resolve({ pid, alive: true, executable: exe }),
          () => resolve({ pid, alive: false, executable: null }),
        );
        return;
      }
      const child = spawn('ps', ['-p', String(pid), '-o', 'comm=']);
      let out = '';
      child.stdout?.on('data', chunk => { out += chunk; });
      child.on('close', code => {
        const exe = out.trim();
        resolve({ pid, alive: code === 0 && exe.length > 0, executable: exe.length > 0 ? exe : null });
      });
      child.on('error', () => resolve({ pid, alive: false, executable: null }));
    }
  });
}

/**
 * CDP pipe client operating over Chrome --remote-debugging-pipe (fd 3 in, fd 4 out).
 * Targets the attached session via Target.createTarget and Target.attachToTarget.
 */
export class CdpPipeConnection {
  constructor(proc, options = {}) {
    this.proc = proc;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = Buffer.alloc(0);
    this.closed = false;
    this.targetId = null;
    this.sessionId = null;
    this.defaultTimeoutMs = options.timeoutMs ?? 15000;

    const inStream = proc.stdio?.[4];
    if (inStream) {
      inStream.on('data', chunk => this._onData(chunk));
      inStream.on('error', err => this._rejectAll(new Error(`CDP read error: ${err.message}`)));
      inStream.on('close', () => this._rejectAll(new Error('CDP pipe closed')));
      inStream.on('end', () => this._rejectAll(new Error('CDP pipe ended')));
    }
    if (typeof proc.on === 'function') {
      proc.on('error', err => this._rejectAll(new Error(`Browser process error: ${err.message}`)));
      proc.on('exit', (code, sig) => this._rejectAll(new Error(`Browser process exited with code ${code} signal ${sig}`)));
    }
  }

  _onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    let nullIdx;
    while ((nullIdx = this.buffer.indexOf(0)) !== -1) {
      const msgBuf = this.buffer.subarray(0, nullIdx);
      this.buffer = this.buffer.subarray(nullIdx + 1);
      if (msgBuf.length === 0) continue;
      try {
        const text = msgBuf.toString('utf-8');
        const json = JSON.parse(text);
        if (json.id && this.pending.has(json.id)) {
          const { resolve, reject, timer } = this.pending.get(json.id);
          this.pending.delete(json.id);
          if (timer) clearTimeout(timer);
          if (json.error) reject(new Error(`CDP Error (${json.error.code}): ${json.error.message}`));
          else resolve(json.result);
        }
      } catch (_) {}
    }
  }

  _rejectAll(err) {
    this.closed = true;
    for (const [, { reject, timer }] of this.pending) {
      if (timer) clearTimeout(timer);
      reject(err);
    }
    this.pending.clear();
  }

  send(method, params = {}, sessionId = undefined, timeoutMs = this.defaultTimeoutMs) {
    if (this.closed) return Promise.reject(new Error(`CDP connection closed; cannot send ${method}`));
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      const payload = { id, method, params };
      if (sessionId) payload.sessionId = sessionId;

      let timer = null;
      if (timeoutMs > 0) {
        timer = setTimeout(() => {
          if (this.pending.has(id)) {
            this.pending.delete(id);
            reject(new Error(`CDP Command timeout (${timeoutMs}ms) for ${method}`));
          }
        }, timeoutMs);
      }

      this.pending.set(id, { resolve, reject, timer });
      try {
        const outStream = this.proc.stdio?.[3];
        if (outStream && !outStream.destroyed) {
          outStream.write(Buffer.concat([Buffer.from(JSON.stringify(payload)), Buffer.from([0])]));
        } else {
          if (timer) clearTimeout(timer);
          this.pending.delete(id);
          reject(new Error('CDP out stream missing or destroyed'));
        }
      } catch (err) {
        if (timer) clearTimeout(timer);
        this.pending.delete(id);
        reject(err);
      }
    });
  }

  async initTarget(targetUrl, viewport = { width: 1280, height: 800 }) {
    const { targetId } = await this.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await this.send('Target.attachToTarget', {
      targetId,
      flatten: true,
    });
    this.targetId = targetId;
    this.sessionId = sessionId;

    await this.send('Emulation.setDeviceMetricsOverride', {
      width: viewport.width,
      height: viewport.height,
      deviceScaleFactor: 1,
      mobile: viewport.name === 'mobile',
    }, sessionId);

    await this.send('Page.enable', {}, sessionId);
    await this.send('Runtime.enable', {}, sessionId);

    if (targetUrl && targetUrl !== 'about:blank') {
      await this.send('Page.navigate', { url: targetUrl }, sessionId);
    }

    return sessionId;
  }

  async navigate(url) {
    if (!this.sessionId) throw new Error('CDP target session not attached; call initTarget first');
    await this.send('Page.navigate', { url }, this.sessionId);
  }

  async evaluate(expression, timeoutMs = undefined) {
    if (!this.sessionId) throw new Error('CDP target session not attached; call initTarget first');
    const transportTimeoutMs = typeof timeoutMs === 'number' && timeoutMs > 0
      ? timeoutMs + 5000
      : this.defaultTimeoutMs + 5000;
    const res = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    }, this.sessionId, transportTimeoutMs);
    if (res?.exceptionDetails) {
      throw new Error(`Evaluation failed: ${res.exceptionDetails.exception?.description ?? res.exceptionDetails.text}`);
    }
    return res?.result?.value;
  }

  async screenshot() {
    if (!this.sessionId) throw new Error('CDP target session not attached; call initTarget first');
    const { data } = await this.send('Page.captureScreenshot', { format: 'png' }, this.sessionId);
    return Buffer.from(data, 'base64');
  }

  async close() {
    if (this.closed) return;
    try {
      await this.send('Browser.close', {}, undefined, 3000);
    } catch (_) {}
    this._rejectAll(new Error('CDP connection closed'));
  }
}

/** Default single-browser owned Chrome launcher with explicit user-data-dir and exact process exit tracking. */
export async function defaultChromeLauncher(profileDir, targetUrl, options = {}) {
  const spawnFn = options.spawn ?? spawn;
  const probeFn = options.probeIdentity ?? probeOwnedIdentity;
  const executable = options.browserExecutable || process.env.CHROME_BIN ||
    (process.platform === 'darwin' ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : 'google-chrome');
  const args = [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--remote-debugging-pipe',
    `--user-data-dir=${profileDir}`,
    'about:blank',
  ];
  const proc = spawnFn(executable, args, {
    stdio: ['ignore', 'pipe', 'pipe', 'pipe', 'pipe'],
    windowsHide: true,
  });

  let exitResolved = false;
  const exitPromise = new Promise(resolve => {
    if (typeof proc.once === 'function') {
      proc.once('exit', (code, sig) => {
        exitResolved = true;
        resolve({ code, sig });
      });
      proc.once('error', (err) => {
        exitResolved = true;
        resolve({ code: -1, sig: null, error: err });
      });
    } else {
      resolve({ code: 0, sig: null });
    }
  });

  const cdp = new CdpPipeConnection(proc, options);
  const recordedPid = proc.pid;
  const recordedExe = executable;

  let closing = false;
  const browser = {
    proc,
    cdp,
    recordedPid,
    recordedExe,
    navigate: url => cdp.navigate(url),
    evaluate: (expr, timeoutMs) => cdp.evaluate(expr, timeoutMs),
    screenshot: () => cdp.screenshot(),
    close: async () => {
      if (closing) return;
      closing = true;
      try { await cdp.close(); } catch (_) {}

      // If process already exited or no valid PID, avoid any signals
      if (exitResolved || typeof recordedPid !== 'number' || recordedPid <= 0) {
        exitResolved = true;
        return;
      }

      // Exact live executable identity probe before sending any signals
      const live = await probeFn(recordedPid, recordedExe);
      if (!live.alive) {
        exitResolved = true;
        return;
      }

      // Strict exact normalized path equality - NO endsWith / basename fallback
      if (!executablePathMatches(live.executable, recordedExe, process.platform)) {
        throw new Error(`CLEANUP_INCOMPLETE: live executable (${live.executable}) does not match recorded (${recordedExe}) for pid ${recordedPid}`);
      }

      // Identity verified: send SIGTERM to direct process handle
      if (typeof proc.kill === 'function') {
        try { proc.kill('SIGTERM'); } catch (_) {}
      }

      // Bounded await for exit after SIGTERM
      let termTimer;
      const termTimerPromise = new Promise(r => { termTimer = setTimeout(r, 2000); });
      await Promise.race([exitPromise, termTimerPromise]);
      if (termTimer) clearTimeout(termTimer);

      if (!exitResolved) {
        // Still alive after SIGTERM: probe identity again before SIGKILL
        const liveKill = await probeFn(recordedPid, recordedExe);
        if (liveKill.alive) {
          if (!executablePathMatches(liveKill.executable, recordedExe, process.platform)) {
            throw new Error(`CLEANUP_INCOMPLETE: live executable (${liveKill.executable}) does not match recorded (${recordedExe}) for pid ${recordedPid} before SIGKILL`);
          }
          if (typeof proc.kill === 'function') {
            try { proc.kill('SIGKILL'); } catch (_) {}
          }
          let killTimer;
          const killTimerPromise = new Promise(r => { killTimer = setTimeout(r, 2000); });
          await Promise.race([exitPromise, killTimerPromise]);
          if (killTimer) clearTimeout(killTimer);
        } else {
          exitResolved = true;
        }
      }

      // If still alive after SIGKILL deadline, throw CLEANUP_INCOMPLETE
      if (!exitResolved) {
        throw new Error(`CLEANUP_INCOMPLETE: process ${recordedPid} failed to exit within deadline after SIGKILL`);
      }
    },
  };

  try {
    await cdp.initTarget(targetUrl, options.viewport ?? { width: 1280, height: 800 });
  } catch (err) {
    await browser.close();
    throw err;
  }

  return browser;
}

/** Deferred factory: importing this file starts nothing. Only the verifier calls it. */
export function createOpenWebOutcome({ uiDist, evidenceDir, timeoutMs = 15000,
  target, rebindTarget, exitFrame, gapFrame, createRoute, profileDir }) {
  if (!uiDist || !evidenceDir || !target || !rebindTarget)
    throw new Error('TASK9_FACTORY_REQUIRES_ISOLATED_TARGETS_AND_CANDIDATE');
  return async function openWebOutcome(options) {
    // Unique fresh profile directory per owned-engine skill reference (browser/references/owned-engine/README.md)
    const runProfileDir = resolve(options.profileDir ?? profileDir ?? join(evidenceDir, `browser-profile-${randomUUID()}`));
    await mkdir(runProfileDir, { recursive: true });
    const gateway = new WebOutcomeGateway({ ...options, uiDist: resolve(uiDist),
      target, rebindTarget, timeoutMs, exitFrame, gapFrame, createRoute, profileDir: runProfileDir });
    let browser;
    try {
      const port = await gateway.start();
      const origin = `http://127.0.0.1:${port}`;
      const targetUrl = `${origin}/index.html`;

      // Single owned Chrome launcher seam
      const launcher = options.launcher ?? defaultChromeLauncher;
      const browserInstance = await launcher(runProfileDir, targetUrl, { ...options, timeoutMs });
      browser = browserInstance;
      gateway.browser = browser;

      const clickExisting = async selector => {
        await browser.evaluate(`new Promise((resolve, reject) => {
          let timer;
          const observer = new MutationObserver(check);
          function check() {
            const el = [...document.querySelectorAll(${JSON.stringify(selector)})]
              .find(el => el.getClientRects().length && !el.disabled);
            if (!el) return;
            observer.disconnect(); clearTimeout(timer); el.click(); resolve(true);
          }
          observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true });
          timer = setTimeout(() => { observer.disconnect(); reject(new Error('TASK9_EXISTING_ACTION_UNAVAILABLE')); }, ${timeoutMs});
          check();
        })`, timeoutMs);
      };
      return { browser, gateway, target, rebindTarget, timeoutMs,
        profileDir: runProfileDir,
        identityEvidence: 'fixture-identity-proof-not-machine-pid',
        isRealPtyIdentity: false,
        input: intent => driveTerminalInput(browser, intent),
        correctToken: async () => {
          gateway.authorized = true; gateway.autoReady = true;
          await clickExisting('[data-testid="remote-terminal-outcome"] button');
        },
        focusSelector: '[data-testid="remote-view-mode-chat"]',
        closeOwner: async () => {
          if (options.viewport.name === 'mobile') await clickExisting('button[aria-label="Change workspace context"]');
          await clickExisting('button[aria-label="Machines"]');
          await clickExisting('[data-testid="mobile-host-drawer-disconnect"]');
          await clickExisting('[data-testid="mobile-host-drawer-confirm-disconnect"]');
        },
        rebindOwner: async () => {
          if (options.viewport.name === 'mobile') {
            await clickExisting('button[aria-label^="Sessions ("]');
            await clickExisting(`[data-testid="session-switcher-tab-${rebindTarget.sessionId}"]`);
          } else await clickExisting('button[aria-label="Next terminal tab"]');
        },
        newTerminalOwner: async () => {
          const mobile = options.viewport.name === 'mobile';
          if (mobile) await clickExisting('button[aria-label^="Sessions ("]');
          const actionTestId = mobile ? 'session-switcher-new-terminal' : 'remote-new-terminal';
          await clickExisting(`[data-testid="${actionTestId}"]`);
          return actionTestId;
        },
        armTicketCompletion: async (target, attemptId, waitMs = timeoutMs) => browser.evaluate(`window.__task9Wire.armTicketCompletion(${JSON.stringify(target)}, ${JSON.stringify(attemptId)}, ${waitMs})`, waitMs),
        waitTicketHeld: async (key, waitMs = timeoutMs) => browser.evaluate(`window.__task9Wire.waitTicketHeld(${JSON.stringify(key)}, ${waitMs})`, waitMs),
        releaseTicketCompletion: async key => browser.evaluate(`window.__task9Wire.releaseTicketCompletion(${JSON.stringify(key)})`),
        readRetiredObserver: async key => browser.evaluate(`window.__task9Wire.readRetiredObserver(${JSON.stringify(key)})`, timeoutMs),
        snapshotOwner: async target => browser.evaluate(`window.__ferryxRecoveryQA.snapshot(${JSON.stringify({ sessionId: target.sessionId, daemonEpoch: target.daemonEpoch })})`),
        currentOwner: async () => browser.evaluate('window.__ferryxRecoveryQA.currentTarget()'),
        waitRetiredStaleCompletion: async (target, attemptId, waitMs = timeoutMs) => browser.evaluate(`window.__ferryxRecoveryQA.waitForRetiredStaleCompletion(${JSON.stringify(target)}, ${JSON.stringify(attemptId)}, ${waitMs})`, waitMs),
        saveScreenshot: async (filename, bytes) => {
          await mkdir(evidenceDir, { recursive: true });
          const path = join(evidenceDir, filename);
          const data = bytes ?? (await browser.screenshot());
          await writeFile(path, data); return path;
        },
        dispose: async () => {
          const path = join(evidenceDir, `web-outcome-wire-${randomUUID()}.json`);
          try {
            await mkdir(evidenceDir, { recursive: true });
            await writeFile(path, JSON.stringify({ requests: gateway.rawRequests,
              deliveries: gateway.deliveries, frames: gateway.rawFrames }, null, 2));
            const observation = await browser.evaluate(`(() => {
              const qa = window.__ferryxRecoveryQA;
              let current, currentError, snapshot, snapshotError;
              try { current = qa?.currentTarget(); } catch (error) { currentError = String(error); }
              try { snapshot = qa?.snapshot(${JSON.stringify(target)}); } catch (error) { snapshotError = String(error); }
              return { current, currentError, snapshot, snapshotError, observerPresent: !!qa,
                text: document.body.innerText,
                gridPresent: !!document.querySelector('[data-testid="remote-terminal-grid"]') };
            })()`);
            await writeFile(path.replace('.json', '-page.json'), JSON.stringify(observation, null, 2));
            await writeFile(path.replace('.json', '-page.png'), await browser.screenshot());
          } catch (_) {}

          let closeError = null;
          try {
            if (browser && typeof browser.close === 'function') {
              await browser.close();
            }
            try { await rm(runProfileDir, { recursive: true, force: true }); } catch (_) {}
          } catch (err) {
            closeError = err;
          } finally {
            await gateway.close();
          }
          if (closeError) throw closeError;
        },
      };
    } catch (error) {
      if (browser && typeof browser.close === 'function') {
        try { await browser.close(); } catch (_) {}
      }
      try { await rm(runProfileDir, { recursive: true, force: true }); } catch (_) {}
      await gateway.close();
      throw error;
    }
  };
}
