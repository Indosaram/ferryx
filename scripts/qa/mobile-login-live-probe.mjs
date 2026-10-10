#!/usr/bin/env node
// scripts/qa/mobile-login-live-probe.mjs
// Node 24 headless CDP probe for Windows Edge using --remote-debugging-pipe.
// Designed to run on maho-win without external npm dependencies.

import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

function findEdgeBinary() {
  if (process.env.EDGE_PATH) {
    return process.env.EDGE_PATH;
  }
  if (process.platform === 'win32') {
    const candidates = [
      'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
      'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
      path.join(process.env.LOCALAPPDATA || '', 'Microsoft', 'Edge', 'Application', 'msedge.exe')
    ];
    for (const c of candidates) {
      if (c) return c;
    }
    return 'msedge.exe';
  }
  if (process.platform === 'darwin') {
    return '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge';
  }
  return 'microsoft-edge';
}

function redactUrl(rawUrl) {
  try {
    const parsed = new URL(rawUrl);
    if (parsed.searchParams.has('code')) {
      parsed.searchParams.set('code', '[REDACTED]');
    }
    if (parsed.searchParams.has('token')) {
      parsed.searchParams.set('token', '[REDACTED]');
    }
    return parsed.toString();
  } catch {
    return String(rawUrl).replace(/([?&](?:code|token)=)[^&#\s]+/gi, '$1[REDACTED]');
  }
}

class CdpPipeConnection {
  constructor(proc) {
    this.proc = proc;
    this.nextId = 1;
    this.pending = new Map();
    this.handlers = new Set();
    this.buffer = Buffer.alloc(0);

    const inStream = proc.stdio[4];
    inStream.on('data', (chunk) => this._onData(chunk));
    inStream.on('error', (err) => {
      this._rejectAll(new Error(`CDP read error: ${err.message}`));
    });
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
        this._dispatch(json);
      } catch (err) {
        console.error('[CDP Parse Error]', err);
      }
    }
  }

  _dispatch(msg) {
    if (msg.id && this.pending.has(msg.id)) {
      const { resolve, reject, timer } = this.pending.get(msg.id);
      this.pending.delete(msg.id);
      if (timer) clearTimeout(timer);
      if (msg.error) {
        reject(new Error(`CDP Error (${msg.error.code}): ${msg.error.message}`));
      } else {
        resolve(msg.result);
      }
      return;
    }
    for (const handler of this.handlers) {
      try {
        handler(msg);
      } catch (err) {
        console.error('[CDP Event Handler Error]', err);
      }
    }
  }

  _rejectAll(err) {
    for (const [, { reject, timer }] of this.pending) {
      if (timer) clearTimeout(timer);
      reject(err);
    }
    this.pending.clear();
  }

  send(method, params = {}, sessionId = undefined, timeoutMs = 15000) {
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      const payload = { id, method, params };
      if (sessionId) {
        payload.sessionId = sessionId;
      }

      let timer = null;
      if (timeoutMs > 0) {
        timer = setTimeout(() => {
          if (this.pending.has(id)) {
            this.pending.delete(id);
            reject(new Error(`CDP command timeout after ${timeoutMs}ms: ${method}`));
          }
        }, timeoutMs);
      }

      this.pending.set(id, { resolve, reject, timer });

      const jsonStr = JSON.stringify(payload);
      const buf = Buffer.concat([Buffer.from(jsonStr, 'utf-8'), Buffer.from([0])]);

      const outStream = this.proc.stdio[3];
      outStream.write(buf, (err) => {
        if (err) {
          if (timer) clearTimeout(timer);
          this.pending.delete(id);
          reject(new Error(`Failed to write to CDP pipe: ${err.message}`));
        }
      });
    });
  }

  onEvent(fn) {
    this.handlers.add(fn);
    return () => this.handlers.delete(fn);
  }
}

async function captureScreenshotIfRequested(cdp, sessionId, label) {
  const dir = process.env.PROBE_SCREENSHOT_DIR;
  const wantBase64 = process.env.CAPTURE_SCREENSHOT === '1' || process.env.CAPTURE_SCREENSHOT === 'true';

  if (!dir && !wantBase64) {
    return;
  }

  try {
    const screenshot = await cdp.send(
      'Page.captureScreenshot',
      { format: 'png' },
      sessionId,
      15000
    );
    if (screenshot && screenshot.data) {
      if (dir) {
        await fs.mkdir(dir, { recursive: true });
        const filePath = path.join(dir, `screenshot-${label.toLowerCase()}.png`);
        const imgBuffer = Buffer.from(screenshot.data, 'base64');
        await fs.writeFile(filePath, imgBuffer);
        console.log(`[Screenshot Saved (${label})] ${filePath} (${imgBuffer.length} bytes)`);
      }
      if (wantBase64) {
        console.log(`\n--- SCREENSHOT BASE64 START (${label}) ---`);
        console.log(screenshot.data);
        console.log(`--- SCREENSHOT BASE64 END (${label}) ---`);
      }
    }
  } catch (err) {
    console.warn(`[Screenshot Warning (${label})] ${err.message}`);
  }
}

async function main() {
  const rawTargetUrl = process.argv[2] || 'https://relay.ferryx.dev/';
  const mockConsume = process.env.PROBE_MOCK_CONSUME === '1' || process.env.PROBE_MOCK_CONSUME === 'true';
  const mockLogin = (process.env.PROBE_MOCK_LOGIN === '1' || process.env.PROBE_MOCK_LOGIN === 'true') || mockConsume;
  const expectFixed = process.env.PROBE_EXPECT_FIXED === '1' || process.env.PROBE_EXPECT_FIXED === 'true';
  const seedSavedSession = process.env.PROBE_SAVED_SESSION === '1' || process.env.PROBE_SAVED_SESSION === 'true';

  let targetUrl = rawTargetUrl;
  if (mockConsume) {
    try {
      const parsed = new URL(targetUrl);
      if (!parsed.searchParams.has('code')) {
        parsed.searchParams.set('code', 'probe-synthetic');
        targetUrl = parsed.toString();
      }
    } catch {
      if (!targetUrl.includes('code=')) {
        targetUrl += (targetUrl.includes('?') ? '&' : '?') + 'code=probe-synthetic';
      }
    }
  }

  const customWidth = parseInt(process.env.PROBE_WIDTH || '', 10);
  const viewportWidth = Number.isFinite(customWidth) && customWidth > 0 ? customWidth : 390;
  const viewportHeight = 844;
  const isMobile = viewportWidth < 768;

  const tempDirPrefix = path.join(os.tmpdir(), 'ferryx-edge-probe-');
  const userDir = await fs.mkdtemp(tempDirPrefix);

  const edgeBin = findEdgeBinary();
  const exceptions = [];
  const consoleMessages = [];

  let loginRequestCount = 0;
  let loginPollCount = 0;
  let loginConsumeCount = 0;
  let machinesRequestCount = 0;

  const args = [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--remote-debugging-pipe',
    `--user-data-dir=${userDir}`
  ];

  const proc = spawn(edgeBin, args, {
    stdio: ['ignore', 'pipe', 'pipe', 'pipe', 'pipe'],
    windowsHide: true
  });

  proc.on('error', (err) => {
    console.error(`[Process Error] Failed to launch Edge binary (${edgeBin}):`, err.message);
  });

  const cdp = new CdpPipeConnection(proc);

  const cleanup = async () => {
    try {
      await cdp.send('Browser.close', {}, undefined, 5000).catch(() => {});
    } catch {}

    const exitPromise = new Promise((resolve) => {
      if (proc.exitCode !== null || proc.killed) {
        return resolve();
      }
      proc.once('exit', () => resolve());
      setTimeout(() => {
        try {
          proc.kill();
        } catch {}
        resolve();
      }, 3000);
    });

    try {
      proc.kill();
    } catch {}
    await exitPromise;

    try {
      await fs.rm(userDir, { recursive: true, force: true }).catch(() => {});
    } catch {}
  };

  process.on('SIGINT', async () => {
    await cleanup();
    process.exit(130);
  });
  process.on('SIGTERM', async () => {
    await cleanup();
    process.exit(143);
  });

  try {
    // 1. Create target about:blank and attach with flatten: true
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await cdp.send('Target.attachToTarget', {
      targetId,
      flatten: true
    });

    // 2. Set viewport metrics
    await cdp.send(
      'Emulation.setDeviceMetricsOverride',
      {
        width: viewportWidth,
        height: viewportHeight,
        deviceScaleFactor: 1,
        mobile: isMobile
      },
      sessionId
    );

    // 3. Event listeners for Runtime exceptions, console logs, and Fetch requests
    cdp.onEvent(async (event) => {
      if (event.sessionId !== sessionId) return;

      if (event.method === 'Runtime.exceptionThrown') {
        const details = event.params.exceptionDetails;
        const text = details?.exception?.description || details?.text || JSON.stringify(details);
        exceptions.push(text);
      } else if (event.method === 'Runtime.consoleAPICalled') {
        const argsStr = (event.params.args || [])
          .map((a) => a.value ?? a.description ?? '')
          .join(' ');
        consoleMessages.push(`[${event.params.type}] ${argsStr}`);
      } else if (event.method === 'Fetch.requestPaused') {
        const { requestId, request } = event.params;
        const reqUrl = request.url || '';

        if (mockConsume && reqUrl.includes('/api/account/v1/login/consume')) {
          loginConsumeCount++;
          const bodyJson = JSON.stringify({
            token: 'probe-test-token',
            email: 'probe@example.invalid',
            accountId: 'probe'
          });
          const base64Body = Buffer.from(bodyJson, 'utf-8').toString('base64');
          try {
            await cdp.send(
              'Fetch.fulfillRequest',
              {
                requestId,
                responseCode: 200,
                responseHeaders: [
                  { name: 'Content-Type', value: 'application/json' },
                  { name: 'Access-Control-Allow-Origin', value: '*' }
                ],
                body: base64Body
              },
              sessionId,
              5000
            );
          } catch (err) {
            console.error('[Fetch fulfill error for login/consume]', err);
          }
        } else if ((mockConsume || seedSavedSession) && reqUrl.includes('/api/account/v1/machines')) {
          machinesRequestCount++;
          const bodyJson = JSON.stringify([]);
          const base64Body = Buffer.from(bodyJson, 'utf-8').toString('base64');
          try {
            await cdp.send(
              'Fetch.fulfillRequest',
              {
                requestId,
                responseCode: 200,
                responseHeaders: [
                  { name: 'Content-Type', value: 'application/json' },
                  { name: 'Access-Control-Allow-Origin', value: '*' }
                ],
                body: base64Body
              },
              sessionId,
              5000
            );
          } catch (err) {
            console.error('[Fetch fulfill error for machines]', err);
          }
        } else if (mockLogin && reqUrl.includes('/api/account/v1/login/request')) {
          loginRequestCount++;
          const bodyJson = JSON.stringify({ loginHandle: 'probe-handle' });
          const base64Body = Buffer.from(bodyJson, 'utf-8').toString('base64');
          try {
            await cdp.send(
              'Fetch.fulfillRequest',
              {
                requestId,
                responseCode: 200,
                responseHeaders: [
                  { name: 'Content-Type', value: 'application/json' },
                  { name: 'Access-Control-Allow-Origin', value: '*' }
                ],
                body: base64Body
              },
              sessionId,
              5000
            );
          } catch (err) {
            console.error('[Fetch fulfill error for login/request]', err);
          }
        } else if (mockLogin && reqUrl.includes('/login/poll')) {
          loginPollCount++;
          const bodyJson = JSON.stringify({ status: 'pending' });
          const base64Body = Buffer.from(bodyJson, 'utf-8').toString('base64');
          try {
            await cdp.send(
              'Fetch.fulfillRequest',
              {
                requestId,
                responseCode: 200,
                responseHeaders: [
                  { name: 'Content-Type', value: 'application/json' },
                  { name: 'Access-Control-Allow-Origin', value: '*' }
                ],
                body: base64Body
              },
              sessionId,
              5000
            );
          } catch (err) {
            console.error('[Fetch fulfill error for login/poll]', err);
          }
        } else {
          if ((mockLogin || mockConsume || seedSavedSession) && reqUrl.includes('/api/account/')) {
            console.warn(`[Intercepted Other Account Request] ${reqUrl}`);
          }
          try {
            await cdp.send('Fetch.continueRequest', { requestId }, sessionId, 5000);
          } catch (err) {
            console.error('[Fetch continueRequest error]', err);
          }
        }
      }
    });

    // 4. Enable Runtime, Page, and Fetch
    await cdp.send('Runtime.enable', {}, sessionId);
    await cdp.send('Page.enable', {}, sessionId);

    if (mockLogin || mockConsume || seedSavedSession) {
      await cdp.send(
        'Fetch.enable',
        {
          patterns: [
            { urlPattern: '*login/request*' },
            { urlPattern: '*login/poll*' },
            { urlPattern: '*login/consume*' },
            { urlPattern: '*api/account*' }
          ]
        },
        sessionId
      );
    }

    // 5. Add bindings
    await cdp.send('Runtime.addBinding', { name: 'probeReady' }, sessionId);
    if (mockLogin || mockConsume || seedSavedSession) {
      await cdp.send('Runtime.addBinding', { name: 'probePostSubmitReady' }, sessionId);
      await cdp.send('Runtime.addBinding', { name: 'probeMachinePageReady' }, sessionId);
    }

    // 6. Pre-navigation document scripts
    let targetOrigin = 'https://relay.ferryx.dev';
    try {
      targetOrigin = new URL(targetUrl).origin;
    } catch {}

    const initScript = `
      (() => {
        ${seedSavedSession ? `
        try {
          const origin = ${JSON.stringify(targetOrigin)};
          const token = 'probe-saved-session-token';
          window.localStorage.setItem('ferryx_remote_token_account', token);
          window.localStorage.setItem('ferryx.account.tokenOrigin', origin);
          window.localStorage.setItem('ferryx.account.origin', origin);
        } catch (e) {
          console.error('[Probe Session Seed Error]', e);
        }
        ` : ''}

        function checkRoot() {
          const root = document.getElementById('root');
          if (root && root.children.length > 0) {
            if (window.probeReady) {
              window.probeReady(JSON.stringify({
                hasChildren: true,
                childCount: root.children.length
              }));
              return true;
            }
          }
          return false;
        }

        if (!checkRoot()) {
          const observer = new MutationObserver(() => {
            if (checkRoot()) {
              observer.disconnect();
            }
          });
          observer.observe(document.documentElement || document, {
            childList: true,
            subtree: true
          });
        }
      })();
    `;
    await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: initScript }, sessionId);

    // 7. Setup exact bounded 30s promise waiting for probeReady binding
    const readyPromise = new Promise((resolve, reject) => {
      const timeoutTimer = setTimeout(() => {
        cleanupSub();
        reject(new Error('TIMEOUT: #root did not receive children within 30s.'));
      }, 30000);

      const cleanupSub = cdp.onEvent((event) => {
        if (event.sessionId === sessionId && event.method === 'Runtime.bindingCalled') {
          if (event.params.name === 'probeReady') {
            clearTimeout(timeoutTimer);
            cleanupSub();
            resolve(event.params.payload);
          }
        }
      });
    });

    // 8. Navigate to target URL
    await cdp.send('Page.navigate', { url: targetUrl }, sessionId);

    let isReady = false;
    let readyPayload = null;
    try {
      readyPayload = await readyPromise;
      isReady = true;
    } catch (err) {
      console.warn(`[Probe Notice] ${err.message}`);
    }

    // 9. Extract initial body.innerText, build dataset, current URL
    const evalResult = await cdp.send(
      'Runtime.evaluate',
      {
        expression: `
          (() => {
            const bodyText = document.body ? document.body.innerText : '';
            const root = document.getElementById('root');
            const rootDataset = root ? { ...root.dataset } : {};
            const buildDataset = document.documentElement ? { ...document.documentElement.dataset } : {};
            const url = window.location.href;
            return {
              bodyText,
              rootDataset,
              buildDataset,
              url
            };
          })()
        `,
        returnByValue: true
      },
      sessionId
    );

    const data = evalResult?.result?.value || {};
    const finalUrl = data.url || targetUrl;
    const redactedUrl = redactUrl(finalUrl);

    console.log('=== INITIAL PROBE RESULT ===');
    console.log(`Ready Event Fired: ${isReady}`);
    if (readyPayload) {
      console.log(`Ready Payload: ${readyPayload}`);
    }
    console.log(`Viewport: ${viewportWidth}x${viewportHeight} (mobile: ${isMobile})`);
    console.log(`URL: ${redactedUrl}`);
    console.log(`\n--- Build / Root Dataset ---`);
    console.log(JSON.stringify({ root: data.rootDataset, document: data.buildDataset }, null, 2));

    console.log(`\n--- Initial body.innerText ---`);
    console.log(data.bodyText ? data.bodyText.trim() : '[EMPTY]');

    await captureScreenshotIfRequested(cdp, sessionId, 'INITIAL');

    // 10. Await machine page / authenticated state if PROBE_MOCK_CONSUME=1 or PROBE_SAVED_SESSION=1
    if (mockConsume || seedSavedSession) {
      console.log(`\n=== RUNNING MACHINE PAGE OBSERVER (${mockConsume ? 'PROBE_MOCK_CONSUME' : 'PROBE_SAVED_SESSION'}) ===`);

      const machinePagePromise = new Promise((resolve, reject) => {
        const timeoutTimer = setTimeout(() => {
          cleanupSub();
          reject(new Error('TIMEOUT: AccountMachinesPage did not settle to empty/list state within 25s.'));
        }, 25000);

        const cleanupSub = cdp.onEvent((event) => {
          if (event.sessionId === sessionId && event.method === 'Runtime.bindingCalled') {
            if (event.params.name === 'probeMachinePageReady') {
              clearTimeout(timeoutTimer);
              cleanupSub();
              resolve(event.params.payload);
            }
          }
        });
      });

      // Inject observer for settled AccountMachinesPage state
      await cdp.send(
        'Runtime.evaluate',
        {
          expression: `
            (() => {
              function checkSettledMachineState() {
                const emptyEl = document.querySelector('[data-testid="machines-empty"]');
                const listEl = document.querySelector('[data-testid="account-machines-list"]');
                const errorEl = document.querySelector('[data-testid="machine-list-error"]');
                const loadingEl = document.querySelector('[data-testid="machines-loading"]');

                // Must have empty or list testid, AND loading must not be active
                if ((emptyEl || listEl) && !loadingEl) {
                  if (window.probeMachinePageReady) {
                    window.probeMachinePageReady(JSON.stringify({
                      settled: true,
                      hasEmptyTestId: !!emptyEl,
                      hasListTestId: !!listEl,
                      hasError: !!errorEl,
                      errorMessage: errorEl ? errorEl.innerText.trim() : null
                    }));
                    return true;
                  }
                }
                return false;
              }

              if (!checkSettledMachineState()) {
                const obs = new MutationObserver(() => {
                  if (checkSettledMachineState()) {
                    obs.disconnect();
                  }
                });
                obs.observe(document.body || document.documentElement, {
                  childList: true,
                  subtree: true,
                  attributes: true
                });
              }
            })()
          `
        },
        sessionId
      );

      let machinePageDetected = false;
      let machinePageDetails = null;
      try {
        machinePageDetails = await machinePagePromise;
        machinePageDetected = true;
      } catch (err) {
        console.warn(`[Machine Page Observer Notice] ${err.message}`);
      }

      if (mockConsume && loginConsumeCount === 0) {
        throw new Error('SECURITY_ASSERTION_FAILED: CDP Fetch did NOT intercept /api/account/v1/login/consume! Mocking failed.');
      }

      console.log(`Fetch Interception Verified: login/consume=${loginConsumeCount}, machines=${machinesRequestCount}`);
      console.log(`Machine Page Settled: ${machinePageDetected}`);
      if (machinePageDetails) {
        console.log(`Machine Page Details: ${machinePageDetails}`);
      }

      const postConsumeEval = await cdp.send(
        'Runtime.evaluate',
        {
          expression: `
            (() => {
              const bodyText = document.body ? document.body.innerText : '';
              const url = window.location.href;
              const hasEmpty = !!document.querySelector('[data-testid="machines-empty"]');
              const hasList = !!document.querySelector('[data-testid="account-machines-list"]');
              const hasLoading = !!document.querySelector('[data-testid="machines-loading"]');
              const hasError = !!document.querySelector('[data-testid="machine-list-error"]');
              return { bodyText, url, hasEmpty, hasList, hasLoading, hasError };
            })()
          `,
          returnByValue: true
        },
        sessionId
      );
      const postConsumeData = postConsumeEval?.result?.value || {};
      console.log(`\n--- Settled Machine Page body.innerText ---`);
      console.log(postConsumeData.bodyText ? postConsumeData.bodyText.trim() : '[EMPTY]');

      await captureScreenshotIfRequested(cdp, sessionId, mockConsume ? 'POST_CONSUME' : 'SAVED_SESSION');

      // Assertions if PROBE_EXPECT_FIXED=1
      if (expectFixed) {
        if (mockConsume && loginConsumeCount !== 1) {
          throw new Error(`PROBE_EXPECT_FIXED assertion failed: expected exactly 1 consume request, got ${loginConsumeCount}`);
        }
        if (!machinePageDetected || !postConsumeData.hasEmpty) {
          throw new Error(`PROBE_EXPECT_FIXED assertion failed: machine page did not settle to [data-testid="machines-empty"]`);
        }
        if (postConsumeData.hasLoading) {
          throw new Error(`PROBE_EXPECT_FIXED assertion failed: machine page still contains [data-testid="machines-loading"]`);
        }
        if (postConsumeData.hasError) {
          throw new Error(`PROBE_EXPECT_FIXED assertion failed: machine page rendered [data-testid="machine-list-error"]`);
        }
        if (exceptions.length > 0) {
          throw new Error(`PROBE_EXPECT_FIXED assertion failed: ${exceptions.length} runtime exceptions thrown`);
        }
        console.log('PROBE_EXPECT_FIXED: Machine page consume assertions passed cleanly (1 consume, settled empty state, 0 errors).');
      }
    }

    // 11. PROBE_MOCK_LOGIN=1 (and not mockConsume / savedSession): Fill email, observe post-submit state
    if (mockLogin && !mockConsume && !seedSavedSession) {
      console.log('\n=== RUNNING MOCK LOGIN FLOW (PROBE_MOCK_LOGIN=1) ===');

      const postSubmitPromise = new Promise((resolve, reject) => {
        const timeoutTimer = setTimeout(() => {
          cleanupSub();
          reject(new Error('TIMEOUT: Neither [data-testid="magic-link-waiting"] nor #account-code-input appeared within 15s after submit.'));
        }, 15000);

        const cleanupSub = cdp.onEvent((event) => {
          if (event.sessionId === sessionId && event.method === 'Runtime.bindingCalled') {
            if (event.params.name === 'probePostSubmitReady') {
              clearTimeout(timeoutTimer);
              cleanupSub();
              resolve(event.params.payload);
            }
          }
        });
      });

      await cdp.send(
        'Runtime.evaluate',
        {
          expression: `
            (() => {
              function checkPostSubmitState() {
                const waitingEl = document.querySelector('[data-testid="magic-link-waiting"]');
                const codeInputEl = document.querySelector('#account-code-input') || document.querySelector('input[name="code"]');

                if (waitingEl || codeInputEl) {
                  if (window.probePostSubmitReady) {
                    window.probePostSubmitReady(JSON.stringify({
                      hasWaitingBanner: !!waitingEl,
                      hasCodeInput: !!codeInputEl,
                      waitingTestId: waitingEl ? waitingEl.getAttribute('data-testid') : null,
                      codeInputId: codeInputEl ? codeInputEl.id : null,
                      codeInputVisible: codeInputEl ? (codeInputEl.offsetParent !== null) : false
                    }));
                    return true;
                  }
                }
                return false;
              }

              if (!checkPostSubmitState()) {
                const obs = new MutationObserver(() => {
                  if (checkPostSubmitState()) {
                    obs.disconnect();
                  }
                });
                obs.observe(document.body || document.documentElement, {
                  childList: true,
                  subtree: true,
                  attributes: true
                });
              }
            })()
          `
        },
        sessionId
      );

      const fillAndSubmitResult = await cdp.send(
        'Runtime.evaluate',
        {
          expression: `
            (() => {
              const input = document.querySelector('input[type="email"]') ||
                            document.querySelector('input[name="email"]') ||
                            document.querySelector('input[autocomplete="email"]') ||
                            document.querySelector('input');
              if (!input) {
                return { success: false, reason: 'No input element found for email' };
              }

              const nativeInputValueSetter = Object.getOwnPropertyDescriptor(
                window.HTMLInputElement.prototype,
                'value'
              )?.set;

              const testEmail = 'probe-mock-tester@ferryx.dev';
              if (nativeInputValueSetter) {
                nativeInputValueSetter.call(input, testEmail);
              } else {
                input.value = testEmail;
              }

              input.dispatchEvent(new Event('input', { bubbles: true }));
              input.dispatchEvent(new Event('change', { bubbles: true }));

              const form = input.closest('form');
              if (form) {
                if (typeof form.requestSubmit === 'function') {
                  form.requestSubmit();
                } else {
                  form.submit();
                }
                return { success: true, method: 'requestSubmit', email: testEmail };
              }

              const submitBtn = document.querySelector('button[type="submit"]') ||
                                Array.from(document.querySelectorAll('button')).find(b =>
                                  /continue|sign in|send|login/i.test(b.innerText)
                                );
              if (submitBtn) {
                submitBtn.click();
                return { success: true, method: 'buttonClick', email: testEmail };
              }

              return { success: false, reason: 'Neither form nor submit button found' };
            })()
          `,
          returnByValue: true
        },
        sessionId
      );

      console.log('Fill & Submit Action:', JSON.stringify(fillAndSubmitResult?.result?.value || {}));

      let postSubmitDetected = false;
      let postSubmitDetails = null;
      try {
        postSubmitDetails = await postSubmitPromise;
        postSubmitDetected = true;
      } catch (err) {
        console.warn(`[Post-Submit Observer Notice] ${err.message}`);
      }

      if (loginRequestCount === 0) {
        throw new Error('SECURITY_ASSERTION_FAILED: CDP Fetch did NOT intercept /api/account/v1/login/request! Mocking failed.');
      }

      console.log(`Fetch Interception Verified: login/request=${loginRequestCount}, login/poll=${loginPollCount}`);
      console.log(`Post-Submit Mutation Detected: ${postSubmitDetected}`);
      if (postSubmitDetails) {
        console.log(`Post-Submit State Details: ${postSubmitDetails}`);
      }

      const inputPresenceEval = await cdp.send(
        'Runtime.evaluate',
        {
          expression: `
            (() => {
              const codeInput = document.querySelector('#account-code-input') || document.querySelector('input[name="code"]');
              const emailInput = document.querySelector('input[type="email"]') || document.querySelector('input[name="email"]');
              const waitingBanner = document.querySelector('[data-testid="magic-link-waiting"]');
              return {
                hasCodeInput: !!codeInput,
                hasEmailInput: !!emailInput,
                hasWaitingBanner: !!waitingBanner,
                codeInputVisible: codeInput ? (codeInput.offsetParent !== null) : false
              };
            })()
          `,
          returnByValue: true
        },
        sessionId
      );

      const inputData = inputPresenceEval?.result?.value || {};
      console.log('\n--- Input Existence Post-Submit ---');
      console.log(JSON.stringify(inputData, null, 2));

      const postSubmitEval = await cdp.send(
        'Runtime.evaluate',
        {
          expression: `
            (() => {
              const bodyText = document.body ? document.body.innerText : '';
              const url = window.location.href;
              return { bodyText, url };
            })()
          `,
          returnByValue: true
        },
        sessionId
      );

      const postData = postSubmitEval?.result?.value || {};
      console.log(`\n--- Post-Submit body.innerText ---`);
      console.log(postData.bodyText ? postData.bodyText.trim() : '[EMPTY]');

      await captureScreenshotIfRequested(cdp, sessionId, 'POST_SUBMIT');

      // Assertions if PROBE_EXPECT_FIXED=1
      if (expectFixed) {
        if (!inputData.hasWaitingBanner) {
          throw new Error('PROBE_EXPECT_FIXED assertion failed: expected [data-testid="magic-link-waiting"] to be true');
        }
        if (inputData.hasCodeInput) {
          throw new Error('PROBE_EXPECT_FIXED assertion failed: expected #account-code-input to be false (eliminated post-submit)');
        }
        if (exceptions.length > 0) {
          throw new Error(`PROBE_EXPECT_FIXED assertion failed: ${exceptions.length} runtime exceptions thrown`);
        }
        console.log('PROBE_EXPECT_FIXED: Login post-submit assertions passed cleanly (waiting=true, code=false, 0 errors).');
      }
    }

    console.log(`\n--- Exceptions (${exceptions.length}) ---`);
    if (exceptions.length > 0) {
      exceptions.forEach((e, idx) => console.log(`[${idx + 1}] ${e}`));
    } else {
      console.log('None');
    }

    if (consoleMessages.length > 0) {
      console.log(`\n--- Console Log Messages (${consoleMessages.length}) ---`);
      consoleMessages.forEach((m) => console.log(m));
    }

    console.log('====================');
  } finally {
    await cleanup();
  }
}

main().catch((err) => {
  console.error('[Fatal Error]', err);
  process.exit(1);
});
