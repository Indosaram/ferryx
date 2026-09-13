/* Paste into DevTools of the explicitly allocated Ferryx Windows test window.
 * Calls real IPC. Does not submit until p14.submit(caseName) is called.
 * No dependency imports, fake builders, or custom audio calls.
 */
(() => {
  'use strict';
  if (globalThis.p14) throw new Error('P14 already installed in this window');
  const ipc = globalThis.__TAURI_INTERNALS__;
  if (!ipc?.invoke) throw new Error('Requires the real Ferryx Tauri webview');
  const original = ipc.invoke;
  const runId = crypto.randomUUID();
  const receipts = [];
  let pendingClick;
  let active;
  const cases = new Map([
    ['targeted-system', { targeted: true, sound: 'system' }],
    ['targeted-silent', { targeted: true, sound: 'silent' }],
    ['idless-system', { targeted: false, sound: 'system' }],
    ['idless-silent', { targeted: false, sound: 'silent' }],
  ]);
  // Observe real queue-drain responses without consuming or replacing them.
  // The existing frontend remains the sole normal activation consumer.
  ipc.invoke = async function (command, args, options) {
    const result = await original.call(this, command, args, options);
    if (command === 'cmd_notification_take_activations') {
      receipts.push({ kind: 'drain', result: structuredClone(result) });
      if (pendingClick && result.some(t =>
        t.workspaceId === active.target.workspaceId &&
        t.sessionId === active.target.sessionId)) {
        const waiter = pendingClick;
        pendingClick = undefined;
        clearTimeout(waiter.timer);
        if (result.length !== 1) waiter.reject(new Error('Unexpected activation batch'));
        else waiter.resolve(structuredClone(result));
      }
    }
    return result;
  };
  globalThis.p14 = {
    runId,
    receipts,
    async submit(caseName, target) {
      const spec = cases.get(caseName);
      if (!spec) throw new Error('Unknown P14 case');
      if (pendingClick) throw new Error('Finish the pending click observation first');
      if (receipts.some(r => r.kind === 'submit' && r.caseName === caseName)) {
        throw new Error('Case already submitted; use a fresh window/run for retries');
      }
      if (spec.targeted && (!target?.workspaceId || !target?.sessionId)) {
        throw new Error('Supply an existing owned workspace/frontend session target');
      }
      if (!spec.targeted && target !== undefined) throw new Error('Idless case forbids target');
      const marker = `P14-${runId}-${caseName}`;
      const request = {
        source: 'terminal-bell', sound: spec.sound, terminalTitle: marker,
        ...(spec.targeted ? { target } : {}),
      };
      const result = await ipc.invoke('cmd_notification_dispatch', { request });
      if (!result.submitted) throw new Error(`Submission rejected: ${JSON.stringify(result)}`);
      active = { caseName, marker, target };
      const receipt = { kind: 'submit', runId, ...active, result };
      receipts.push(receipt);
      console.log(JSON.stringify(receipt));
      return receipt;
    },
    armClick() {
      if (!active?.target || pendingClick) throw new Error('Requires one active targeted case');
      // Call BEFORE clicking the toast; deadline only bounds failure, never readiness.
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pendingClick = undefined;
          reject(new Error('No matching real activation drain within 60 seconds'));
        }, 60000);
        pendingClick = { resolve, reject, timer };
        console.log('P14 click observation armed; click the captured owned toast now');
      });
    },
    async verifyDrained() {
      if (pendingClick) throw new Error('Await the real activation first');
      const result = await ipc.invoke('cmd_notification_take_activations');
      if (result.length !== 0) throw new Error('Activation queue was not drained exactly once');
      return { empty: true };
    },
    dispose() {
      if (pendingClick) throw new Error('Finish pending observation before disposal');
      ipc.invoke = original;
      delete globalThis.p14;
    },
  };
  console.log(`P14 observer ready, runId=${runId}; no notification submitted`);
})();
