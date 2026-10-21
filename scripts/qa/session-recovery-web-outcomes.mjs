/**
 * Task9 browser outcome executors. No CLI, process launch, or production access.
 * The verifier supplies an already served candidate and isolated browser/gateway.
 * Required adapter signatures and evidence provenance are frozen alongside this file.
 */
import { randomUUID } from 'node:crypto';

export const WEB_OUTCOME_SCENARIOS = Object.freeze([
  'confirmed-exit', 'auth-revocation', 'close-rebind-pending-dial',
  'output-replay-gap', 'no-focus-steal', 'cold-start-unknown',
]);

const STATUS = '[data-testid="remote-terminal-outcome"], [data-testid="remote-terminal-outcome-unknown"]';
const GRID = '[data-testid="remote-terminal-grid"]';
const TERMINAL = '[data-testid="remote-view-mode-terminal"]';
const CHAT = '[data-testid="remote-view-mode-chat"]';
const GAP = '[data-testid="remote-terminal-replay-gap"]';

function requireEvidence(condition, code, detail) {
  if (!condition) throw Object.assign(new Error(`${code}: ${detail}`), { code });
}

function sameTarget(a, b) {
  return ['sessionId', 'daemonEpoch', 'incarnation', 'pid'].every(key =>
    a?.[key] !== undefined && a[key] === b?.[key]);
}

function sameLifetime(a, b) {
  return ['sessionId', 'incarnation', 'pid'].every(key => a?.[key] !== undefined && a[key] === b?.[key]);
}

function sameWebTarget(a, b) {
  return ['sessionId', 'daemonEpoch'].every(key => a?.[key] !== undefined && a[key] === b?.[key]);
}

export function hasLegacyPresentationProof(proof, target) {
  const receipt = proof?.fullGridReceipt;
  return sameWebTarget(proof, target) && sameWebTarget(receipt, target) &&
    typeof proof.socketId === 'string' && proof.socketId.length > 0 && proof.socketId === receipt.socketId &&
    typeof receipt.receiptId === 'string' && receipt.receiptId.length > 0 &&
    Number.isInteger(proof.generation) && proof.generation > 0 &&
    Number.isInteger(proof.presentationSequence) && proof.presentationSequence > 0 && proof.outcome === 'ready';
}

// Install the MutationObserver before the trigger, and return a handle immediately.
// The promise is retained in the page, not awaited by evaluate during registration.
export async function armDOM(browser, expression, timeoutMs) {
  const id = randomUUID();
  await browser.evaluate(`(() => {
    window.__task9Outcomes ??= new Map();
    let observer, timer, abandon, inspect;
    const cleanup = () => {
      observer.disconnect(); clearTimeout(timer);
      window.removeEventListener('ferryx-web-owner-observation', inspect);
    };
    const promise = new Promise((resolve, reject) => {
      abandon = () => reject(new Error('TASK9_DOM_CANCELLED'));
      inspect = () => {
        try {
          const value = (${expression})();
          if (!value) return;
          cleanup(); resolve(value);
        } catch (error) { cleanup(); reject(error); }
      };
      observer = new MutationObserver(inspect);
      observer.observe(document.documentElement, {
        subtree: true, childList: true, characterData: true, attributes: true
      });
      window.addEventListener('ferryx-web-owner-observation', inspect);
      timer = setTimeout(() => {
        cleanup(); reject(new Error('TASK9_DOM_DEADLINE'));
      }, ${JSON.stringify(timeoutMs)});
      inspect();
    });
    promise.catch(() => {});
    window.__task9Outcomes.set(${JSON.stringify(id)}, {
      promise, cancel: () => { cleanup(); abandon(); }
    });
  })()`);
  return {
    read: () => browser.evaluate(`window.__task9Outcomes.get(${JSON.stringify(id)}).promise`),
    dispose: () => browser.evaluate(`(() => {
      window.__task9Outcomes.get(${JSON.stringify(id)})?.cancel();
      window.__task9Outcomes.delete(${JSON.stringify(id)});
    })()`),
  };
}

export function outcomeExpression(target, outcomes) {
  return `() => {
    const target = ${JSON.stringify(target)};
    const outcomes = ${JSON.stringify(outcomes)};

    // The product installs this observer when RemoteTerminal mounts its owner; keep waiting
    // (bounded by the DOM deadline) instead of failing on the first pre-mount inspection.
    if (!window.__ferryxRecoveryQA) return null;

    let snap;
    try {
      snap = window.__ferryxRecoveryQA.snapshot({
        sessionId: target.sessionId,
        daemonEpoch: target.daemonEpoch ?? null,
      });
    } catch (error) {
      // The owner for this target registers when its socket dial starts; until then keep waiting.
      if (String(error && error.message) === 'WEB_OWNER_TARGET_EVIDENCE_REQUIRED') return null;
      throw error;
    }
    if (!snap ||
        snap.retainedTarget?.sessionId !== target.sessionId ||
        snap.retainedTarget?.daemonEpoch !== (target.daemonEpoch ?? null)) {
      return null;
    }

    const snapIncarnation = snap.incarnation ?? null;
    const snapOutcome = snap.outcome ?? null;
    const isSettled = snap.pendingTimers === 0 && snap.inFlight === false && (
      snap.activity === 'settled' || (typeof snap.exitSettlements === 'number' && snap.exitSettlements > 0)
    );
    const snapActivity = isSettled ? 'settled' : (!snap.disposed ? 'active' : 'disposed');

    if (outcomes.includes('ready')) {
      if (snap.identityConfirmed && !snap.disposed) {
        const live = document.querySelector('[data-testid="remote-terminal-connection-status"]');
        const grid = document.querySelector(${JSON.stringify(GRID)});
        const outcome = document.querySelector(${JSON.stringify(STATUS)});
        if (live && live.textContent.includes('Live') && grid && grid.getClientRects().length > 0 && !outcome) {
          if (snapOutcome !== 'ready') return null;
          return {
            outcome: snapOutcome,
            reason: 'live',
            sessionId: snap.retainedTarget.sessionId,
            daemonEpoch: snap.retainedTarget.daemonEpoch,
            incarnation: snapIncarnation,
            socketId: snap.socketId,
            generation: snap.generation,
            presentationSequence: snap.presentationSequence,
            activity: snapActivity,
            text: live.textContent.trim(),
          };
        }
      }
    }

    if (outcomes.includes('exited') || outcomes.includes('affirmative_exit')) {
      const outcome = document.querySelector('[data-testid="remote-terminal-outcome"]');
      if (outcome && outcome.getClientRects().length > 0) {
        const text = outcome.textContent || '';
        if (text.includes('Terminal exited') || text.includes('exited')) {
          if (snapOutcome !== 'affirmative_exit') return null;
          return {
            outcome: snapOutcome,
            reason: text.replace('Retry', '').trim(),
            sessionId: snap.retainedTarget.sessionId,
            daemonEpoch: snap.retainedTarget.daemonEpoch,
            incarnation: snapIncarnation,
            socketId: snap.socketId,
            generation: snap.generation,
            presentationSequence: snap.presentationSequence,
            activity: isSettled ? 'settled' : (snap.exitSettlements === null && snap.activity !== 'settled' ? null : 'unsettled'),
            text: text.trim(),
          };
        }
      }
    }

    if (outcomes.includes('blocked')) {
      const outcome = document.querySelector('[data-testid="remote-terminal-outcome"]');
      const unknown = document.querySelector('[data-testid="remote-terminal-outcome-unknown"]');
      if (outcome && outcome.getClientRects().length > 0) {
        const text = outcome.textContent || '';
        if (snapOutcome === 'blocked' && !text.includes('Terminal exited') && !text.includes('exited')) {
          return {
            outcome: snapOutcome,
            reason: text.replace('Retry', '').trim(),
            sessionId: snap.retainedTarget.sessionId,
            daemonEpoch: snap.retainedTarget.daemonEpoch,
            incarnation: snapIncarnation,
            socketId: snap.socketId,
            generation: snap.generation,
            presentationSequence: snap.presentationSequence,
            activity: snapActivity,
            text: text.trim(),
          };
        }
      }
      if (unknown && unknown.getClientRects().length > 0) {
        const text = unknown.textContent || '';
        if (snapOutcome !== 'blocked') return null;
        return {
          outcome: snapOutcome,
          reason: text.replace('Retry', '').trim(),
          sessionId: snap.retainedTarget.sessionId,
          daemonEpoch: snap.retainedTarget.daemonEpoch,
          incarnation: snapIncarnation,
          socketId: snap.socketId,
          generation: snap.generation,
          presentationSequence: snap.presentationSequence,
          activity: snapActivity,
          text: text.trim(),
        };
      }
    }

    if (outcomes.includes('retryable')) {
      if (snap.inFlight || snap.pendingTimers > 0 || !snap.identityConfirmed) {
        const statusEls = Array.from(document.querySelectorAll('[role="status"]'));
        const connecting = statusEls.find(el => el.textContent.includes('Connecting') && el.getClientRects().length > 0);
        if (connecting) {
          if (snapOutcome !== 'retryable') return null;
          return {
            outcome: snapOutcome,
            reason: 'Connecting',
            sessionId: snap.retainedTarget.sessionId,
            daemonEpoch: snap.retainedTarget.daemonEpoch,
            incarnation: snapIncarnation,
            socketId: snap.socketId,
            generation: snap.generation,
            presentationSequence: snap.presentationSequence,
            activity: snapActivity,
            text: connecting.textContent.trim(),
          };
        }
      }
    }

    return null;
  }`;
}

async function click(browser, selector) {
  await browser.evaluate(`(() => {
    const el = document.querySelector(${JSON.stringify(selector)});
    if (!el || el.disabled || el.getClientRects().length === 0)
      throw new Error('TASK9_ACTION_UNAVAILABLE: ' + ${JSON.stringify(selector)});
    el.click();
  })()`);
}

// This is a fixture event subscription, not a timeout used as proof of absence.
function armWire(gateway, event, target, timeoutMs) {
  let timer, listener, abandon;
  const promise = new Promise((resolve, reject) => {
    abandon = () => reject(new Error('TASK9_WIRE_CANCELLED'));
    listener = receipt => {
      if (!sameLifetime(receipt.target, target)) return;
      clearTimeout(timer); gateway.off(event, listener); resolve(receipt);
    };
    gateway.on(event, listener);
    timer = setTimeout(() => {
      gateway.off(event, listener); reject(new Error(`TASK9_WIRE_DEADLINE: ${event}`));
    }, timeoutMs);
  });
  promise.catch(() => {});
  return { read: () => promise, dispose: () => {
    clearTimeout(timer); gateway.off(event, listener); abandon();
  } };
}

async function transition(run, { command, event, target = run.target, outcomes, action }) {
  const wire = armWire(run.gateway, event, target, run.timeoutMs);
  const dom = await armDOM(run.browser, outcomeExpression(target, outcomes), run.timeoutMs);
  try {
    if (action) await action();
    else await run.gateway.inject(command, { target });
    const [receipt, rendered] = await Promise.all([wire.read(), dom.read()]);
    run.timeline.push({ command, event, receipt, rendered });
    return { receipt, rendered };
  } finally {
    wire.dispose(); await dom.dispose();
  }
}

async function capture(run, label) {
  // requestAnimationFrame is the actual rendering boundary, not a guessed flush count.
  await run.browser.evaluate('new Promise(resolve => requestAnimationFrame(() => resolve(true)))');
  const bytes = await run.browser.screenshot({ format: 'png' });
  const path = await run.saveScreenshot(`${run.scenario}-${run.viewport.name}-${run.theme}-${label}.png`, bytes);
  requireEvidence(typeof path === 'string' && path.length > 0, 'TASK9_SCREENSHOT_RECEIPT', label);
  run.captures.push({ path, label, viewport: run.viewport, theme: run.theme });
}

async function assertLedger(run, explicitCreateId = null) {
  // A fence seals all received HTTP/WS messages through the scenario's final browser
  // acknowledgement. It also reads the actual recovery-owner scheduler, not fixture flags.
  const fenceTarget = { sessionId: run.target.sessionId, daemonEpoch: run.target.daemonEpoch ?? null };
  const ledger = await run.gateway.fence({ browser: run.browser, target: fenceTarget });
  requireEvidence(ledger.complete === true && ledger.fenceId && ledger.source,
    'TASK9_LEDGER_INCOMPLETE', 'Require sealed raw request/delivery and owner-scheduler evidence');
  const creates = ledger.requests.filter(r => ['createSession', 'spawnTerminalDetailed'].includes(r.type));
  requireEvidence(creates.length === (explicitCreateId ? 1 : 0) &&
    creates.every(r => r.requestId === explicitCreateId), 'TASK9_AUTOMATIC_CREATE', JSON.stringify(creates));
  const mutations = ledger.deliveries.filter(r => ['write', 'signal'].includes(r.kind));
  for (const intent of run.intents) {
    const delivered = mutations.filter(r => r.kind === intent.kind && r.payload === intent.payload);
    requireEvidence(delivered.length === 1 && sameTarget(delivered[0].target, intent.target),
      'TASK9_MUTATION_REPLAY', JSON.stringify({ intent, delivered }));
  }
  requireEvidence(mutations.length === run.intents.length, 'TASK9_UNEXPECTED_MUTATION', JSON.stringify(mutations));
  requireEvidence(ledger.selectionRequests.length <= run.selectionCount,
    'TASK9_SELECTION_RETRY', JSON.stringify(ledger.selectionRequests));
  run.ledger = ledger;
  return ledger;
}

async function sendIdentifiedMutations(run) {
  const payload = `task9-${randomUUID()}`;
  for (const intent of [{ kind: 'write', payload }, { kind: 'signal', payload: 'interrupt' }]) {
    const wire = armWire(run.gateway, `delivery-${intent.kind}`, run.target, run.timeoutMs);
    try {
      // Driver uses real input/keyboard/pointer events, never calling component callbacks.
      await run.input(intent);
      const delivery = await wire.read();
      requireEvidence(delivery.payload === intent.payload && delivery.requestId && delivery.socketId,
        'TASK9_DELIVERY_IDENTITY', JSON.stringify(delivery));
      run.intents.push({ ...intent, target: run.target, delivery });
    } finally { wire.dispose(); }
  }
}

async function ready(run) {
  const grid = await armDOM(run.browser, `() => {
    const el = document.querySelector(${JSON.stringify(GRID)});
    return el && el.getClientRects().length && el.textContent.includes('task9-current-target')
      ? { text: el.textContent } : null;
  }`, run.timeoutMs);
  try {
    await transition(run, { command: 'ready', event: 'presentation-sent', outcomes: ['ready'] });
    run.timeline.push({ event: 'rendered-grid', rendered: await grid.read() });
  } finally { await grid.dispose(); }
  const proof = await run.gateway.currentTarget();
  const currentBinding = await run.gateway.validatedBinding();
  requireEvidence(sameLifetime(currentBinding, run.target) && hasLegacyPresentationProof(proof, currentBinding),
    'TASK9_CURRENT_TARGET_PROOF', JSON.stringify(proof));
  run.target = { ...currentBinding };
  run.timeline.push({ event: 'current-target', proof });
}

async function confirmedExit(run) {
  await ready(run); await sendIdentifiedMutations(run);
  const result = await transition(run, { command: 'confirmed-exit', event: 'exit-sent', outcomes: ['exited'] });
  requireEvidence(result.rendered.activity === 'settled', 'TASK8_EXIT_ACTIVITY', JSON.stringify(result.rendered));
  await capture(run, 'exited');
  const ledger = await assertLedger(run);
  requireEvidence(ledger.owner.pendingTimers === 0 && ledger.owner.inFlight === false && ledger.owner.exitSettlements === 1,
    'TASK9_EXIT_NOT_CANCELLED', JSON.stringify(ledger.owner));
}

async function authRevocation(run) {
  await ready(run); await sendIdentifiedMutations(run);
  await transition(run, { command: 'revoke-token', event: 'auth-rejected', outcomes: ['blocked'] });
  await capture(run, 'blocked');
  const blocked = await assertLedger(run);
  requireEvidence(blocked.owner.pendingTimers === 0 && blocked.owner.inFlight === false,
    'TASK9_AUTH_RETRY_ACTIVE', JSON.stringify(blocked.owner));
  await transition(run, { command: 'correct-token', event: 'presentation-sent', outcomes: ['ready'],
    action: () => run.correctToken() });
  await capture(run, 'corrected'); await assertLedger(run);
  const corrected = await run.gateway.currentTarget();
  requireEvidence(hasLegacyPresentationProof(corrected, run.target) && sameTarget(run.target, await run.gateway.validatedBinding()),
    'TASK9_AUTH_TARGET_CHANGED', 'Token correction');
}

async function closeRebind(run) {
  // Both cancellation paths run in separate retained browser fixtures, selected by run.variant.
  const pending = armWire(run.gateway, 'dial-held', run.target, run.timeoutMs);
  try { await run.gateway.inject('hold-dial', { target: run.target }); await pending.read(); }
  finally { pending.dispose(); }
  const next = run.variant === 'close' ? run.target : run.rebindTarget;
  const cancelled = armWire(run.gateway, 'dial-cancelled', run.target, run.timeoutMs);
  try {
    if (run.variant === 'rebind') {
      run.selectionCount++;
      await transition(run, { command: 'rebind', event: 'presentation-sent', target: next, outcomes: ['ready'],
        action: () => run.rebindOwner() });
    } else {
      // Disconnect removes the owner; it cannot leave a cancelled outcome DOM behind.
      const removed = await armDOM(run.browser, `() => !document.querySelector(${JSON.stringify(GRID)})
        ? { ownerRemoved: true } : null`, run.timeoutMs);
      try { await run.closeOwner(); run.timeline.push(await removed.read()); }
      finally { await removed.dispose(); }
    }
    run.timeline.push({ event: 'dial-cancelled', receipt: await cancelled.read() });
  } finally { cancelled.dispose(); }
  // Release the exact old completion after cancellation. Adapter waits for actual browser
  // rejection acknowledgement; no sleeps or assumptions that a turn has flushed it.
  await run.gateway.inject('release-stale-dial', { target: run.target });
  await capture(run, run.variant);
  const ledger = await assertLedger(run);
  requireEvidence(ledger.owner.cancelledAttemptIds.length === 1 && ledger.owner.staleCompletionsRejected === 1,
    'TASK9_STALE_DIAL_ACCEPTED', JSON.stringify(ledger.owner));
  if (run.variant === 'rebind') {
    const observed = await run.gateway.currentTarget();
    requireEvidence(hasLegacyPresentationProof(observed, next),
      'TASK9_REBIND_TARGET', JSON.stringify({ expected: next, observed }));
  }
}

async function outputReplayGap(run) {
  await ready(run); await sendIdentifiedMutations(run);
  const gap = await armDOM(run.browser, `() => {
    const el = document.querySelector(${JSON.stringify(GAP)});
    const grid = document.querySelector(${JSON.stringify(GRID)});
    if (!el || !grid || !el.getClientRects().length || !grid.textContent.includes('task9-gap-tail')) return null;
    return { text: el.textContent, grid: grid.textContent };
  }`, run.timeoutMs);
  const wire = armWire(run.gateway, 'replay-gap-sent', run.target, run.timeoutMs);
  try {
    await run.gateway.inject('overflow-output-ring', { target: run.target, tail: 'task9-gap-tail' });
    const [receipt, rendered] = await Promise.all([wire.read(), gap.read()]);
    requireEvidence(receipt.requestedCursor < receipt.oldestAvailableCursor && receipt.bytesWritten > receipt.capacity,
      'TASK9_GAP_NOT_REAL', JSON.stringify(receipt));
    run.timeline.push({ event: 'replay-gap', receipt, rendered });
  } finally { wire.dispose(); await gap.dispose(); }
  requireEvidence(hasLegacyPresentationProof(await run.gateway.currentTarget(), run.target), 'TASK9_GAP_TARGET', 'Gap dropped web binding');
  await capture(run, 'gap'); await assertLedger(run);
}

async function noFocusSteal(run) {
  let phase = 'initial-ready';
  try {
  await ready(run); await sendIdentifiedMutations(run);
  phase = 'focus-setup';
  await run.browser.evaluate(`(() => {
    const el = document.querySelector(${JSON.stringify(run.focusSelector)});
    if (!el || !el.getClientRects().length) throw new Error('TASK9_ACTIVE_PANE_MISSING');
    el.focus();
    if (document.activeElement !== el) throw new Error('TASK9_FOCUS_SETUP');
    window.__task9Focus = { el, stolen: [] };
    window.__task9Focus.listener = event => {
      if (event.target !== el) window.__task9Focus.stolen.push(event.target.outerHTML);
    };
    document.addEventListener('focusin', window.__task9Focus.listener, true);
  })()`);
  try {
    phase = 'disconnect-retryable-dom';
    await transition(run, { command: 'disconnect', event: 'transport-closed', outcomes: ['retryable'] });
    phase = 'reconnect-ready';
    await ready(run);
    phase = 'focus-retention';
    const focus = await run.browser.evaluate(`({ retained: document.activeElement === window.__task9Focus.el,
      stolen: window.__task9Focus.stolen })`);
    requireEvidence(focus.retained && focus.stolen.length === 0, 'TASK9_FOCUS_STOLEN', JSON.stringify(focus));
    run.timeline.push({ event: 'focus', ...focus });
    phase = 'capture-and-ledger';
    await capture(run, 'retained-focus'); await assertLedger(run);
  } finally {
    await run.browser.evaluate('document.removeEventListener("focusin", window.__task9Focus.listener, true)');
  }
  } catch (error) {
    let diagnostic;
    try {
      diagnostic = await run.browser.evaluate(`(() => {
        let owner;
        try { owner = window.__ferryxRecoveryQA?.snapshot(${JSON.stringify({ sessionId: run.target.sessionId, daemonEpoch: run.target.daemonEpoch ?? null })}); }
        catch (error) { owner = { error: String(error) }; }
        return { owner, statuses: Array.from(document.querySelectorAll('[role="status"]')).map(el => ({
          text: el.textContent, visible: el.getClientRects().length > 0 })),
          focus: window.__task9Focus ? { retained: document.activeElement === window.__task9Focus.el,
            stolen: window.__task9Focus.stolen } : null };
      })()`);
    } catch (diagnosticError) { diagnostic = { error: String(diagnosticError) }; }
    throw new Error(`${error instanceof Error ? error.message : String(error)}; TASK9_FOCUS_PHASE ${JSON.stringify({
      phase, viewport: run.viewport, theme: run.theme, target: run.target,
      timeline: run.timeline, diagnostic,
    })}`, { cause: error });
  }
}

async function coldStartUnknown(run) {
  const rendered = await armDOM(run.browser, outcomeExpression(run.target, ['retryable', 'blocked']), run.timeoutMs);
  try {
    const unknown = await rendered.read();
    requireEvidence(unknown.activity === 'active' && unknown.reason,
      'TASK8_UNKNOWN_ACTIVITY', JSON.stringify(unknown));
    run.timeline.push({ event: 'historical-identity', rendered: unknown });
  }
  finally { await rendered.dispose(); }
  const before = await assertLedger(run);
  requireEvidence(before.owner.identityConfirmed === false && sameWebTarget(before.owner.retainedTarget, run.target),
    'TASK9_UNKNOWN_IDENTITY_LOST', JSON.stringify(before.owner));
  await capture(run, 'unknown');
  const oldSnapshot = await run.snapshotOwner(run.target);
  requireEvidence(oldSnapshot.identityConfirmed === false && oldSnapshot.inFlight && !oldSnapshot.disposed &&
    oldSnapshot.generation > 0 && sameWebTarget(oldSnapshot.retainedTarget, run.target),
    'TASK9_COLDSTART_OLD_ATTEMPT_NOT_PENDING', JSON.stringify(oldSnapshot));
  const oldAttemptId = String(oldSnapshot.generation);
  const completionKey = await run.armTicketCompletion(run.target, oldAttemptId, run.timeoutMs);
  const create = armWire(run.gateway, 'create-request', run.target, run.timeoutMs);
  let releasedTicket = false;
  try {
    const held = await run.waitTicketHeld(completionKey, run.timeoutMs);
    requireEvidence(held.status === 200 && held.requestId && !held.delivered && sameWebTarget(held.target, run.target),
      'TASK9_COLDSTART_TICKET_NOT_HELD', JSON.stringify(held));
    const actionTestId = await run.newTerminalOwner();
    const request = await create.read();
    requireEvidence(actionTestId && request.requestId && request.browserActionId && request.browserActionTestId === actionTestId,
      'TASK9_EXPLICIT_CREATE_RECEIPT', JSON.stringify(request));
    // This is the sole exception, correlated to a real button action and received API request.
    await assertLedger(run, request.requestId);
    run.timeline.push({ event: 'explicit-new-terminal', request });
    const live = await armDOM(run.browser, outcomeExpression(run.rebindTarget, ['ready']), run.timeoutMs);
    try { await live.read(); } finally { await live.dispose(); }
    const beforeRelease = await run.snapshotOwner(run.target);
    requireEvidence(beforeRelease.disposed && beforeRelease.pendingTimers === 0 && !beforeRelease.inFlight &&
      beforeRelease.cancelledAttemptIds.includes(oldAttemptId), 'TASK9_COLDSTART_OLD_OWNER_NOT_DISPOSED', JSON.stringify(beforeRelease));
    const released = await run.releaseTicketCompletion(completionKey);
    releasedTicket = true;
    requireEvidence(released.status === 200 && released.requestId === held.requestId && sameWebTarget(released.target, run.target),
      'TASK9_COLDSTART_RELEASED_TICKET_MISMATCH', JSON.stringify({ held, released }));
    const rejected = await run.readRetiredObserver(completionKey);
    requireEvidence(rejected.disposed && rejected.pendingTimers === 0 && !rejected.inFlight &&
      rejected.staleCompletionsRejected === 1 && rejected.cancelledAttemptIds.includes(oldAttemptId) &&
      rejected.rejectedAttemptIds.includes(oldAttemptId), 'TASK9_COLDSTART_LATE_COMPLETION_NOT_REJECTED', JSON.stringify(rejected));
    const current = await run.currentOwner();
    requireEvidence(hasLegacyPresentationProof(await run.gateway.currentTarget(), run.rebindTarget) &&
      sameWebTarget(current, run.rebindTarget) && current.socketId && current.presentationSequence > 0,
      'TASK9_COLDSTART_NEW_OWNER_NOT_LIVE', JSON.stringify(current));
    requireEvidence(sameTarget(await run.gateway.validatedBinding(), run.rebindTarget),
      'TASK9_COLDSTART_NEW_OWNER_IDENTITY', JSON.stringify(current));
    await assertLedger(run, request.requestId);
    run.timeline.push({ event: 'late-old-owner-completion-rejected', attemptId: oldAttemptId,
      oldOwner: rejected, newOwner: current, held, released });
  } finally {
    create.dispose();
    if (!releasedTicket) {
      const held = await run.waitTicketHeld(completionKey, run.timeoutMs);
      await run.releaseTicketCompletion(completionKey);
      run.timeline.push({ event: 'late-old-owner-completion-released-after-failed-assertion', held });
    }
  }
  await capture(run, 'explicit-new-terminal');
}

const EXECUTORS = {
  'confirmed-exit': confirmedExit,
  'auth-revocation': authRevocation,
  'close-rebind-pending-dial': closeRebind,
  'output-replay-gap': outputReplayGap,
  'no-focus-steal': noFocusSteal,
  'cold-start-unknown': coldStartUnknown,
};

export async function executeWebOutcome(scenario, ctx) {
  requireEvidence(WEB_OUTCOME_SCENARIOS.includes(scenario), 'TASK9_UNKNOWN_SCENARIO', scenario);
  requireEvidence(typeof ctx.openWebOutcome === 'function', 'TASK9_GATEWAY_ADAPTER_REQUIRED', 'openWebOutcome(options)');
  const evidence = [];
  for (const viewport of [{ name: 'desktop', width: 1280, height: 800 }, { name: 'mobile', width: 390, height: 844 }]) {
    for (const theme of ['light', 'dark']) {
      for (const variant of scenario === 'close-rebind-pending-dial' ? ['close', 'rebind'] : [null]) {
        const run = await ctx.openWebOutcome({ scenario, viewport, theme, variant,
          initial: scenario === 'cold-start-unknown' ? 'historical-owner-unavailable' : 'held', chatDefault: true });
        Object.assign(run, { scenario, viewport, theme, variant, timeline: [], captures: [], intents: [], selectionCount: 0 });
        try {
          requireEvidence(Number.isFinite(run.timeoutMs) && run.timeoutMs > 0 && sameTarget(run.target, run.target),
            'TASK9_FIXTURE_IDENTITY_REQUIRED', 'Exact isolated target and bounded deadline');
          // Observe chat default before making the explicit QA terminal choice. Never seed terminal mode.
          const chat = await armDOM(run.browser, `() => {
            const el = document.querySelector(${JSON.stringify(CHAT)});
            const isChatActive = el && (
              el.classList.contains('bg-chat-surface-raised') ||
              el.getAttribute('aria-pressed') === 'true' ||
              el.classList.contains('bg-chat-surface-hover')
            );
            return el && el.getClientRects().length && isChatActive &&
              !document.querySelector(${JSON.stringify(GRID)}) ? { chatDefault: true } : null;
          }`, run.timeoutMs);
          try { run.timeline.push(await chat.read()); } finally { await chat.dispose(); }
          await click(run.browser, TERMINAL);
          await EXECUTORS[scenario](run);
          evidence.push({ scenario, viewport, theme, variant, target: run.target,
            identityEvidence: run.identityEvidence,
            timeline: run.timeline, captures: run.captures, ledger: run.ledger });
        } finally { await run.dispose(); }
      }
    }
  }
  return { scenario, status: 'passed', evidence };
}

export const runConfirmedExitScenario = ctx => executeWebOutcome('confirmed-exit', ctx);
export const runAuthRevocationScenario = ctx => executeWebOutcome('auth-revocation', ctx);
export const runCloseRebindPendingDialScenario = ctx => executeWebOutcome('close-rebind-pending-dial', ctx);
export const runOutputReplayGapScenario = ctx => executeWebOutcome('output-replay-gap', ctx);
export const runNoFocusStealScenario = ctx => executeWebOutcome('no-focus-steal', ctx);
export const runColdStartUnknownScenario = ctx => executeWebOutcome('cold-start-unknown', ctx);
