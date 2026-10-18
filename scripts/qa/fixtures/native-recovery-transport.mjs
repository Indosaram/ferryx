// Faults apply to an isolated, forwarding transport, never the PTY owner.
// The adapter must forward real requests and publish actual ingress/delivery
// receipts. This controller supplies no product receipts or synthetic success.
export const NATIVE_FAULTS = Object.freeze({
  'native-stream-ends': { streamEnds: 7, outageMs: 0 },
  'native-remote-mahord': { streamEnds: 7, outageMs: 31_000 },
  'local-outage': { streamEnds: 0, outageMs: 31_000, minimumFailures: 6 },
  'stale-epoch': { streamEnds: 0, outageMs: 0, staleEpoch: true },
});

export async function driveNativeFaults(scenario, { transport, observe, target, signal }) {
  const plan = NATIVE_FAULTS[scenario];
  let recoveryCursor = observe.cursor();
  // Every wait is armed before the corresponding real forwarding action.
  for (let index = 0; index < plan.streamEnds; index += 1) {
    const cursor = observe.cursor();
    const ended = observe.wait('stream-ended', cursor, (event) => event.target === target.backendSessionId);
    const retried = observe.wait('attempt', cursor, (event) => event.target === target.backendSessionId);
    const holdRecovery = index + 1 === plan.streamEnds;
    if (holdRecovery) recoveryCursor = cursor;
    await transport.endCurrentStream({ target, holdRecovery, signal });
    await ended;
    await retried;
    if (index + 1 < plan.streamEnds) {
      const next = observe.wait('stream-open', cursor, (event) => event.target === target.backendSessionId);
      await next;
    }
  }
  if (plan.outageMs) {
    const cursor = observe.cursor();
    recoveryCursor = cursor;
    const unavailable = observe.wait('outage-start', cursor);
    await transport.disconnect({ target, signal });
    const start = await unavailable;
    // A timer deadline is intentional here: elapsed outage duration IS the
    // behavior. No sleep/poll loop. Provider's hold observes its monotonic clock.
    const elapsed = observe.wait('outage-elapsed', cursor, (event) =>
      event.startedAt === start.monotonicMs && event.monotonicMs - start.monotonicMs >= plan.outageMs);
    await transport.holdOutage({ target, minimumMs: plan.outageMs, signal });
    await elapsed;
  }
  if (plan.staleEpoch) {
    const cursor = observe.cursor();
    recoveryCursor = cursor;
    const changed = observe.wait('authority', cursor, (event) =>
      event.target === target.backendSessionId && event.daemonEpoch !== target.daemonEpoch);
    // Routes the same existing process through the isolated owner, leaving the
    // frontend's previously persisted tuple stale. Never restarts a real daemon.
    await transport.changeOwnerEpoch({ target, holdRecovery: true, signal });
    await changed;
  }
  return recoveryCursor;
}
