import { describe, expect, it } from "vitest";
import {
  calculateBackoffDelay,
  checkIncarnationConflict,
  createRecoveryScheduler,
  formatRecoveryKey,
  isAuthorityCorrection,
  type AuthorityCorrectionReceipt,
  type RecoveryAttemptResult,
  type RecoveryClock,
  type RecoveryIdentity,
  type RecoveryOutcome,
  type RecoveryScheduler,
} from "./sessionRecovery";

function waitForRetryableToken(
  scheduler: RecoveryScheduler,
  targetToken: number,
  timeoutMs = 1000
): Promise<RecoveryOutcome> {
  return new Promise<RecoveryOutcome>((resolve, reject) => {
    let timer: any = null;
    let unsub: () => void = () => {};
    timer = setTimeout(() => {
      unsub();
      reject(new Error(`Timed out waiting for retryable token ${targetToken}`));
    }, timeoutMs);
    unsub = scheduler.subscribe((outcome) => {
      if (outcome.phase === "retryable" && outcome.token === targetToken) {
        if (timer) clearTimeout(timer);
        unsub();
        resolve(outcome);
      }
    });
  });
}

function waitForReady(
  scheduler: RecoveryScheduler,
  timeoutMs = 1000
): Promise<RecoveryOutcome> {
  return new Promise<RecoveryOutcome>((resolve, reject) => {
    let timer: any = null;
    let unsub: () => void = () => {};
    timer = setTimeout(() => {
      unsub();
      reject(new Error("Timed out waiting for ready outcome"));
    }, timeoutMs);
    unsub = scheduler.subscribe((outcome) => {
      if (outcome.phase === "ready") {
        if (timer) clearTimeout(timer);
        unsub();
        resolve(outcome);
      }
    });
  });
}

interface DeterministicClock extends RecoveryClock {
  readonly pendingTimers: number;
  advanceTime(ms: number): Promise<void>;
  tick(ms: number): Promise<void>;
}

function createDeterministicClock(startUnixMs: number = 1000): DeterministicClock {
  let currentTime = startUnixMs;
  let nextTimerId = 1;
  interface ScheduledTimer {
    readonly id: number;
    readonly triggerTime: number;
    readonly callback: () => void;
  }
  const timers: Map<number, ScheduledTimer> = new Map();

  const clock: DeterministicClock = {
    now: () => currentTime,
    get pendingTimers() {
      return timers.size;
    },
    setTimeout: (callback: () => void, ms: number) => {
      const id = nextTimerId++;
      timers.set(id, {
        id,
        triggerTime: currentTime + Math.max(0, ms),
        callback,
      });
      return id;
    },
    clearTimeout: (handle: unknown) => {
      if (typeof handle === "number") {
        timers.delete(handle);
      }
    },
    advanceTime: async (ms: number) => {
      const targetTime = currentTime + Math.max(0, ms);
      while (true) {
        let earliestId: number | null = null;
        let earliestTimer: ScheduledTimer | null = null;
        for (const [id, timer] of timers) {
          if (timer.triggerTime <= targetTime) {
            if (!earliestTimer || timer.triggerTime < earliestTimer.triggerTime) {
              earliestId = id;
              earliestTimer = timer;
            }
          }
        }
        if (!earliestTimer || earliestId === null) {
          break;
        }
        currentTime = earliestTimer.triggerTime;
        timers.delete(earliestId);
        earliestTimer.callback();
        // Allow microtask queue to drain after each timer callback
        await Promise.resolve();
      }
      currentTime = targetTime;
    },
    tick: async (ms: number) => {
      await clock.advanceTime(ms);
    },
  };
  return clock;
}

describe("sessionRecovery - contract and single-flight scheduler", () => {
  it(">20 failures then ready unchanged identity", async () => {
    const clock = createDeterministicClock(1000);
    let attempts = 0;
    const initialIdentity: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "frontend-1",
      backendSessionId: "backend-1",
      expectedIncarnation: "inc-1",
    };
    const expectedKey = formatRecoveryKey(initialIdentity);

    const observedKeys: string[] = [];
    const scheduler = createRecoveryScheduler({
      identity: initialIdentity,
      initialDelayMs: 250,
      maxDelayMs: 4000,
      backoffFactor: 2,
      clock,
      attemptAction: async () => {
        attempts++;
        observedKeys.push(scheduler.getIdentityKey());
        if (attempts <= 25) {
          return { kind: "retryable", reason: "transport" };
        }
        return { kind: "ready", details: { status: "attached" } };
      },
    });

    scheduler.start(true);
    await clock.tick(0);

    // Step deterministically through failures 2..25 and 26th success
    while (attempts < 26) {
      const outcome = scheduler.getOutcome();
      if (outcome.phase === "ready") break;
      if (outcome.phase === "retryable") {
        await clock.tick(outcome.nextDelayMs);
      }
    }

    expect(attempts).toBe(26);
    expect(scheduler.getPhase()).toBe("ready");
    expect(scheduler.getIdentityKey()).toBe(expectedKey);
    // Identity must remain unchanged across all attempts
    expect(observedKeys.length).toBe(26);
    expect(observedKeys.every(k => k === expectedKey)).toBe(true);

    // Verifies delay saturation: 250 -> 500 -> 1000 -> 2000 -> 4000 (saturates at 4000)
    expect(calculateBackoffDelay(1, 250, 4000)).toBe(250);
    expect(calculateBackoffDelay(2, 250, 4000)).toBe(500);
    expect(calculateBackoffDelay(3, 250, 4000)).toBe(1000);
    expect(calculateBackoffDelay(4, 250, 4000)).toBe(2000);
    expect(calculateBackoffDelay(5, 250, 4000)).toBe(4000);
    expect(calculateBackoffDelay(20, 250, 4000)).toBe(4000);
    expect(calculateBackoffDelay(25, 250, 4000)).toBe(4000);
  });

  it("one attempt/timer invariant", async () => {
    const clock = createDeterministicClock(0);
    let inFlightCount = 0;
    let maxInFlightCount = 0;
    let resolveAttempt!: (res: RecoveryAttemptResult) => void;

    const scheduler = createRecoveryScheduler({
      identity: {
        kind: "ssh",
        frontendSessionId: "fe-1",
        backendSessionId: "be-1",
        expectedIncarnation: "inc-1",
      },
      clock,
      attemptAction: async () => {
        inFlightCount++;
        maxInFlightCount = Math.max(maxInFlightCount, inFlightCount);
        try {
          return await new Promise<RecoveryAttemptResult>(r => {
            resolveAttempt = r;
          });
        } finally {
          inFlightCount--;
        }
      },
    });

    scheduler.start(true);
    expect(inFlightCount).toBe(1);
    expect(maxInFlightCount).toBe(1);
    expect(clock.pendingTimers).toBe(1); // The attempt deadline timer

    // Calling wake multiple times while in flight must NOT start a second attempt
    scheduler.wake("network_change");
    scheduler.wake("online");
    scheduler.wake("focus");
    expect(inFlightCount).toBe(1);
    expect(maxInFlightCount).toBe(1);

    // Complete the first attempt with retryable
    const token1 = scheduler.getToken();
    const nextRetryable = waitForRetryableToken(scheduler, token1);
    resolveAttempt({ kind: "retryable", reason: "transport" });
    await nextRetryable;
    await clock.tick(0);

    // Because wake was coalesced, the follow-up attempt runs immediately with 0 delay
    expect(inFlightCount).toBe(1);
    expect(maxInFlightCount).toBe(1);
    expect(clock.pendingTimers).toBe(1); // Deadline timer for attempt 2

    // Complete second attempt
    const readyPromise = waitForReady(scheduler);
    resolveAttempt({ kind: "ready" });
    await readyPromise;
    await clock.tick(0);

    expect(inFlightCount).toBe(0);
    expect(maxInFlightCount).toBe(1);
    expect(clock.pendingTimers).toBe(0);
  });

  it("blocked correction stops autonomous attempts and restarts only on validated authority correction", async () => {
    const clock = createDeterministicClock(0);
    let attemptCount = 0;

    const scheduler = createRecoveryScheduler({
      identity: {
        kind: "paired",
        proxyBackendId: "proxy-1",
        hostId: "host-1",
        remoteTarget: "target-1",
        authorityGeneration: 1,
      },
      clock,
      attemptAction: async () => {
        attemptCount++;
        if (attemptCount === 1) {
          return { kind: "blocked", reason: "auth_revoked" };
        }
        return { kind: "ready" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);

    expect(attemptCount).toBe(1);
    expect(scheduler.getPhase()).toBe("blocked");
    expect(clock.pendingTimers).toBe(0);

    // Passive wake events cannot unblock or bypass blocked state
    scheduler.wake("online");
    scheduler.wake("visible");
    await clock.tick(0);
    expect(attemptCount).toBe(1);
    expect(scheduler.getPhase()).toBe("blocked");

    // Retrying with old authority generation is rejected and remains blocked
    const rejectedRetry = scheduler.retryExplicit({
      kind: "paired",
      proxyBackendId: "proxy-1",
      hostId: "host-1",
      remoteTarget: "target-1",
      authorityGeneration: 1,
    });
    expect(rejectedRetry).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
    expect(attemptCount).toBe(1);

    // Retrying without arguments while blocked is also rejected
    const noArgRetry = scheduler.retryExplicit();
    expect(noArgRetry).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
    expect(attemptCount).toBe(1);

    // Corrective action: explicit retry with updated authority token transitions to retryable and executes
    const approvedRetry = scheduler.retryExplicit({
      kind: "paired",
      proxyBackendId: "proxy-1",
      hostId: "host-1",
      remoteTarget: "target-1",
      authorityGeneration: 2,
    });
    expect(approvedRetry).toBe(true);
    await clock.tick(0);

    expect(attemptCount).toBe(2);
    expect(scheduler.getPhase()).toBe("ready");
    expect(scheduler.getIdentityKey()).toBe(
      formatRecoveryKey({
        kind: "paired",
        proxyBackendId: "proxy-1",
        hostId: "host-1",
        remoteTarget: "target-1",
        authorityGeneration: 2,
      })
    );
  });

  it("exit and cancel permanently stop recovery and ignore late arrivals", async () => {
    const clock = createDeterministicClock(0);
    let attempts = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-exit", backendSessionId: "be-exit" },
      clock,
      attemptAction: async () => {
        attempts++;
        return { kind: "exited", exitCode: 0, posixSignal: null };
      },
    });

    scheduler.start(true);
    await clock.tick(0);

    expect(attempts).toBe(1);
    expect(scheduler.getPhase()).toBe("exited");
    expect(clock.pendingTimers).toBe(0);

    // Terminal phase cannot be overwritten by cancel or reportReady
    expect(scheduler.cancel()).toBe(false);
    expect(
      scheduler.reportReady(undefined, {
        token: scheduler.getToken(),
        identityKey: scheduler.getIdentityKey(),
      })
    ).toBe(false);
    expect(scheduler.getPhase()).toBe("exited");

    // Subsequent wake does not run an attempt
    scheduler.wake("online");
    await clock.tick(0);
    expect(attempts).toBe(1);

    // Test cancel
    let cancelAttempts = 0;
    const cancelScheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-cancel", backendSessionId: "be-cancel" },
      clock,
      initialDelayMs: 1000,
      attemptAction: async () => {
        cancelAttempts++;
        return { kind: "retryable", reason: "transport" };
      },
    });

    cancelScheduler.start(true);
    await clock.tick(0);
    expect(cancelAttempts).toBe(1);
    expect(cancelScheduler.getPhase()).toBe("retryable");
    expect(clock.pendingTimers).toBe(1); // Backoff timer pending

    expect(cancelScheduler.cancel()).toBe(true);
    expect(cancelScheduler.getPhase()).toBe("cancelled");
    expect(clock.pendingTimers).toBe(0); // Timer cleared

    // reportExit cannot overwrite cancelled
    expect(
      cancelScheduler.reportExit(
        { exitCode: 1, posixSignal: null },
        {
          token: cancelScheduler.getToken(),
          identityKey: cancelScheduler.getIdentityKey(),
        }
      )
    ).toBe(false);
    expect(cancelScheduler.getPhase()).toBe("cancelled");

    await clock.tick(1000);
    cancelScheduler.wake("wake");
    await clock.tick(0);
    expect(cancelAttempts).toBe(1);
  });

  it("late rebind receipt is discarded and does not dispatch to listeners", async () => {
    const clock = createDeterministicClock(0);
    let resolveOldAttempt!: (res: RecoveryAttemptResult) => void;
    const dispatchedOutcomes: RecoveryOutcome[] = [];

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-old", backendSessionId: "be-old", expectedIncarnation: "inc-1" },
      clock,
      attemptAction: async () => {
        return new Promise<RecoveryAttemptResult>(r => {
          resolveOldAttempt = r;
        });
      },
    });

    scheduler.subscribe(outcome => {
      dispatchedOutcomes.push(outcome);
    });

    scheduler.start(true);
    expect(scheduler.isInFlight()).toBe(true);

    // Rebind to a new identity while attempt is in-flight
    const newIdentity: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-new",
      backendSessionId: "be-new",
      expectedIncarnation: "inc-2",
    };
    scheduler.rebind(newIdentity);
    expect(scheduler.getIdentityKey()).toBe(formatRecoveryKey(newIdentity));

    // Old attempt finally resolves with ready for old session
    resolveOldAttempt({ kind: "ready", details: { old: true } });
    await clock.tick(0);

    // The late response must be discarded; phase should remain retryable for new session
    expect(scheduler.getPhase()).toBe("retryable");
    expect(dispatchedOutcomes.some(o => o.phase === "ready")).toBe(false);
  });

  it("deadline frees slot and fences late resolution", async () => {
    const clock = createDeterministicClock(0);
    let resolveHungAttempt!: (res: RecoveryAttemptResult) => void;
    let attemptCount = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-timeout", backendSessionId: "be-timeout" },
      clock,
      attemptDeadlineMs: 5000,
      initialDelayMs: 250,
      attemptAction: async () => {
        attemptCount++;
        if (attemptCount === 1) {
          // Hung attempt that exceeds deadline
          return new Promise<RecoveryAttemptResult>(r => {
            resolveHungAttempt = r;
          });
        }
        return { kind: "ready" };
      },
    });

    scheduler.start(true);
    expect(scheduler.isInFlight()).toBe(true);
    expect(attemptCount).toBe(1);

    // Advance clock past the 5000ms deadline
    await clock.tick(5000);

    // Deadline fired: in-flight slot freed, outcome became retryable (timeout)
    expect(scheduler.isInFlight()).toBe(false);
    expect(scheduler.getPhase()).toBe("retryable");
    expect(scheduler.getOutcome()).toMatchObject({
      reason: "timeout",
      attempt: 1,
    });
    // Next attempt is now scheduled with backoff delay (250ms)
    expect(clock.pendingTimers).toBe(1);

    // Advance clock by backoff delay to trigger attempt 2
    await clock.tick(250);

    expect(attemptCount).toBe(2);
    expect(scheduler.getPhase()).toBe("ready");

    // Late resolution of attempt 1 arrives after attempt 2 succeeded
    resolveHungAttempt({ kind: "retryable", reason: "transport" });
    await clock.tick(0);

    // Must remain ready; late response is fenced out by attempt token
    expect(scheduler.getPhase()).toBe("ready");
  });

  it("A-timeout / B-pending / A-late proves B still times out", async () => {
    const clock = createDeterministicClock(0);
    let resolveA!: (res: RecoveryAttemptResult) => void;
    let attemptCount = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-race", backendSessionId: "be-race" },
      clock,
      attemptDeadlineMs: 5000,
      initialDelayMs: 250,
      attemptAction: async ({ attemptNumber }) => {
        attemptCount = attemptNumber;
        if (attemptNumber === 1) {
          // Attempt A hangs
          return new Promise<RecoveryAttemptResult>(r => {
            resolveA = r;
          });
        }
        // Attempt B also hangs
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    scheduler.start(true);
    expect(attemptCount).toBe(1);

    // Advance clock by 5000ms: Attempt A deadline fires
    await clock.tick(5000);
    expect(scheduler.getPhase()).toBe("retryable");
    expect(scheduler.getOutcome()).toMatchObject({ reason: "timeout", attempt: 1 });

    // Advance clock by 250ms to start Attempt B
    await clock.tick(250);
    expect(attemptCount).toBe(2);
    expect(scheduler.isInFlight()).toBe(true);
    expect(clock.pendingTimers).toBe(1); // Attempt B deadline timer is active!

    // Now Attempt A finishes late!
    resolveA({ kind: "ready", details: { stale: true } });
    await clock.tick(0);

    // Attempt A's late resolution MUST NOT clear Attempt B's deadline!
    expect(clock.pendingTimers).toBe(1);
    expect(scheduler.isInFlight()).toBe(true);

    // Advance clock by Attempt B's deadline (5000ms)
    await clock.tick(5000);

    // Attempt B deadline MUST fire and transition to timeout retryable!
    expect(scheduler.isInFlight()).toBe(false);
    expect(scheduler.getOutcome()).toMatchObject({ reason: "timeout", attempt: 2 });
  });

  it("burst wakes dedup into at most one accelerated attempt", async () => {
    const clock = createDeterministicClock(0);
    let attemptCount = 0;
    let resolveCurrent!: () => void;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-burst", backendSessionId: "be-burst" },
      clock,
      attemptAction: async () => {
        attemptCount++;
        return new Promise<RecoveryAttemptResult>(r => {
          resolveCurrent = () => r({ kind: "retryable", reason: "transport" });
        });
      },
    });

    scheduler.start(true);
    expect(attemptCount).toBe(1);

    // 10 burst wakes while attempt is running
    for (let i = 0; i < 10; i++) {
      scheduler.wake(`burst-${i}`);
    }

    // Finish attempt 1
    const token1 = scheduler.getToken();
    const nextOutcome1 = waitForRetryableToken(scheduler, token1);
    resolveCurrent();
    await nextOutcome1;
    await clock.tick(0);

    // Only one accelerated attempt runs
    expect(attemptCount).toBe(2);

    // Finish attempt 2
    const token2 = scheduler.getToken();
    const nextOutcome2 = waitForRetryableToken(scheduler, token2);
    resolveCurrent();
    await nextOutcome2;
    await clock.tick(0);

    // Now backed off and waiting on timer; burst 10 wakes on timer
    expect(clock.pendingTimers).toBe(1);
    for (let i = 0; i < 10; i++) {
      scheduler.wake(`timer-burst-${i}`);
    }

    // Only one attempt runs from the timer burst
    expect(attemptCount).toBe(3);
    const token3 = scheduler.getToken();
    const nextOutcome3 = waitForRetryableToken(scheduler, token3);
    resolveCurrent();
    await nextOutcome3;
    await clock.tick(0);
  });

  it("success-only reset: backoff delay resets only upon validated ready", async () => {
    const clock = createDeterministicClock(0);
    let attempts = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-reset", backendSessionId: "be-reset" },
      clock,
      initialDelayMs: 250,
      maxDelayMs: 4000,
      backoffFactor: 2,
      attemptAction: async () => {
        attempts++;
        if (attempts <= 4) {
          return { kind: "retryable", reason: "transport" };
        }
        return { kind: "ready" };
      },
    });

    scheduler.start(true);

    // Attempt 1: 0ms delay (start) -> fails -> delay 250ms
    await clock.tick(0);
    expect(scheduler.getOutcome()).toMatchObject({ attempt: 1, nextDelayMs: 250 });

    // Attempt 2: 250ms delay -> fails -> delay 500ms
    await clock.tick(250);
    expect(scheduler.getOutcome()).toMatchObject({ attempt: 2, nextDelayMs: 500 });

    // Attempt 3: 500ms delay -> fails -> delay 1000ms
    await clock.tick(500);
    expect(scheduler.getOutcome()).toMatchObject({ attempt: 3, nextDelayMs: 1000 });

    // Attempt 4: 1000ms delay -> fails -> delay 2000ms
    await clock.tick(1000);
    expect(scheduler.getOutcome()).toMatchObject({ attempt: 4, nextDelayMs: 2000 });

    // Attempt 5: 2000ms delay -> succeeds (ready)
    await clock.tick(2000);
    expect(scheduler.getPhase()).toBe("ready");
    expect(scheduler.getAttemptCount()).toBe(0);

    // If a new retryable failure is reported later, cadence resets back to initial 250ms
    scheduler.reportRetryable("transport", undefined, undefined, {
      token: scheduler.getToken(),
      identityKey: scheduler.getIdentityKey(),
    });
    expect(scheduler.getOutcome()).toMatchObject({ attempt: 1, nextDelayMs: 250 });
  });

  it("transport AbortSignal fires upon attempt cancellation, timeout, and rebind", async () => {
    const clock = createDeterministicClock(0);
    let observedSignal!: AbortSignal;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-abort", backendSessionId: "be-abort" },
      clock,
      attemptDeadlineMs: 1000,
      initialDelayMs: 250,
      attemptAction: async ({ signal }) => {
        observedSignal = signal;
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    scheduler.start(true);
    expect(observedSignal.aborted).toBe(false);

    // 1. Timeout aborts the attempt signal
    await clock.tick(1000);
    expect(observedSignal.aborted).toBe(true);

    // 2. Next attempt starts; rebind aborts it
    await clock.tick(250);
    expect(observedSignal.aborted).toBe(false);
    scheduler.rebind({ kind: "local", frontendSessionId: "fe-rebound", backendSessionId: "be-rebound" });
    expect(observedSignal.aborted).toBe(true);

    // 3. Next attempt starts; cancel aborts it
    scheduler.start(true);
    await clock.tick(0);
    expect(observedSignal.aborted).toBe(false);
    scheduler.cancel();
    expect(observedSignal.aborted).toBe(true);
  });

  it("external reporting methods reject stale key/token receipts and cannot bypass blocked", async () => {
    const clock = createDeterministicClock(0);
    let resolveAttempt!: (res: RecoveryAttemptResult) => void;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-ext", backendSessionId: "be-ext" },
      clock,
      attemptAction: async () => {
        return new Promise<RecoveryAttemptResult>(r => {
          resolveAttempt = r;
        });
      },
    });

    scheduler.start(true);
    expect(scheduler.isInFlight()).toBe(true);

    const validToken = scheduler.getToken();
    const validKey = scheduler.getIdentityKey();

    // Rejects report with absent proof
    expect(scheduler.reportReady({ external: true })).toBe(false);
    expect(scheduler.getPhase()).toBe("retryable");

    // Rejects report with stale token
    expect(scheduler.reportReady({ external: true }, { token: validToken - 1, identityKey: validKey })).toBe(false);
    expect(scheduler.getPhase()).toBe("retryable");

    // Rejects report with wrong identity key
    expect(scheduler.reportReady({ external: true }, { token: validToken, identityKey: "wrong-key" })).toBe(false);
    expect(scheduler.getPhase()).toBe("retryable");

    // Accepts report with matching token and key
    expect(scheduler.reportReady({ external: true }, { token: validToken, identityKey: validKey })).toBe(true);
    expect(scheduler.getPhase()).toBe("ready");

    // Late in-flight attempt resolution cannot overwrite ready
    resolveAttempt({ kind: "retryable", reason: "transport" });
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("ready");

    // Test transition to blocked
    scheduler.wake();
    expect(scheduler.isInFlight()).toBe(true);
    scheduler.reportBlocked("auth_revoked", undefined, {
      token: scheduler.getToken(),
      identityKey: scheduler.getIdentityKey(),
    });
    expect(scheduler.getPhase()).toBe("blocked");

    // External reportReady CANNOT bypass blocked phase even with valid token/key
    expect(
      scheduler.reportReady({ hacked: true }, {
        token: scheduler.getToken(),
        identityKey: scheduler.getIdentityKey(),
      })
    ).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");

    // Late attempt resolution cannot overwrite blocked
    resolveAttempt({ kind: "ready" });
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("reentrant abort listener and outcome listener wakes preserve single-flight and single-timer invariants", async () => {
    const clock = createDeterministicClock(0);
    let attempts = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-reentrancy", backendSessionId: "be-reentrancy" },
      clock,
      attemptDeadlineMs: 1000,
      initialDelayMs: 250,
      attemptAction: async ({ signal }) => {
        attempts++;
        signal.addEventListener("abort", () => {
          // Reentrant wake on abort
          scheduler.wake("synchronous-abort-wake");
        });
        return new Promise<RecoveryAttemptResult>(() => {}); // hangs until deadline
      },
    });

    scheduler.start(true);
    expect(attempts).toBe(1);
    expect(scheduler.isInFlight()).toBe(true);
    expect(clock.pendingTimers).toBe(1); // Deadline timer for attempt 1

    // Advance to deadline: abort listener wakes Attempt 2 re-entrantly
    await clock.tick(1000);

    // Invariant: MUST NOT have both inFlight and pending timer simultaneously,
    // and MUST NOT have more than 1 timer registered!
    expect(clock.pendingTimers).toBeLessThanOrEqual(1);
    const violatingConcurrentState = scheduler.isInFlight() && scheduler.isTimerPending();
    expect(violatingConcurrentState).toBe(false);
  });

  it("atomic callback boundaries: reentrant cancel during abort notification is not overwritten", async () => {
    const clock = createDeterministicClock(0);
    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-atomic", backendSessionId: "be-atomic" },
      clock,
      attemptDeadlineMs: 1000,
      attemptAction: async ({ signal }) => {
        signal.addEventListener("abort", () => {
          scheduler.cancel();
        });
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    scheduler.start(true);
    await clock.tick(1000); // Triggers deadline timeout and aborts signal

    // Reentrant cancel() executed during abort MUST be preserved as the outcome!
    expect(scheduler.getPhase()).toBe("cancelled");
  });

  it("listener cancel during retryExplicit fences executeAttempt continuation", () => {
    const clock = createDeterministicClock(0);
    let attempts = 0;
    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-fence", backendSessionId: "be-fence" },
      clock,
      attemptAction: async () => {
        attempts++;
        return { kind: "ready" };
      },
    });

    scheduler.subscribe((outcome) => {
      if (outcome.phase === "retryable") {
        scheduler.cancel();
      }
    });

    scheduler.retryExplicit();

    // Since listener cancelled during retryExplicit notification, executeAttempt must NOT run!
    expect(scheduler.getPhase()).toBe("cancelled");
    expect(attempts).toBe(0);
    expect(scheduler.isInFlight()).toBe(false);
  });

  it("same-target authority correction unblocks while target switching or guessed incarnation is rejected", async () => {
    // Paired target: changing hostId or remoteTarget rejected
    const currentPaired: RecoveryIdentity = {
      kind: "paired",
      proxyBackendId: "proxy-1",
      hostId: "host-1",
      remoteTarget: "target-1",
      authorityGeneration: 1,
    };
    const switchedTargetPaired: RecoveryIdentity = {
      kind: "paired",
      proxyBackendId: "proxy-1",
      hostId: "host-2",
      remoteTarget: "target-2",
      authorityGeneration: 2,
    };
    expect(isAuthorityCorrection(currentPaired, switchedTargetPaired, "auth_revoked")).toBe(false);

    // Local target: guessed replacement incarnation rejected
    const currentLocal: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-1",
      backendSessionId: "be-1",
      expectedIncarnation: "inc-1",
    };
    const guessedIncarnationLocal: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-1",
      backendSessionId: "be-1",
      expectedIncarnation: "inc-2",
    };
    expect(isAuthorityCorrection(currentLocal, guessedIncarnationLocal, "identity_conflict")).toBe(false);

    // Same-target control conflict correction accepted
    const clock = createDeterministicClock(0);
    const localScheduler = createRecoveryScheduler({
      identity: currentLocal,
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "control_conflict" };
      },
    });
    localScheduler.start(true);
    await clock.tick(0);
    expect(localScheduler.getPhase()).toBe("blocked");

    // Unblocks with same retained session and incarnation
    const validReceipt: AuthorityCorrectionReceipt = {
      currentToken: localScheduler.getToken(),
      currentIdentityKey: localScheduler.getIdentityKey(),
      revalidatedIdentity: currentLocal,
      clearedConflict: true,
    };
    const retryAccepted = localScheduler.retryExplicit(validReceipt);
    expect(retryAccepted).toBe(true);
    expect(localScheduler.getPhase()).toBe("retryable");
  });

  it("rebind preserves blocked state without affirmative authority correction", async () => {
    const clock = createDeterministicClock(0);
    const scheduler = createRecoveryScheduler({
      identity: {
        kind: "paired",
        proxyBackendId: "p1",
        hostId: "h1",
        remoteTarget: "t1",
        authorityGeneration: 1,
      },
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "auth_revoked" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    // Rebinding with uncorrected authority leaves phase blocked
    scheduler.rebind({
      kind: "paired",
      proxyBackendId: "p1",
      hostId: "h1",
      remoteTarget: "t1",
      authorityGeneration: 1,
    });
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("reportExit does not overwrite blocked phase", async () => {
    const clock = createDeterministicClock(0);
    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-block", backendSessionId: "be-block" },
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "identity_conflict" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    expect(scheduler.reportExit({ exitCode: 1 })).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("retryExplicit rejects target switching across all phases", async () => {
    const clock = createDeterministicClock(0);
    const initialIdentity: RecoveryIdentity = {
      kind: "paired",
      proxyBackendId: "p1",
      hostId: "h1",
      remoteTarget: "t1",
      authorityGeneration: 1,
    };
    const scheduler = createRecoveryScheduler({
      identity: initialIdentity,
      clock,
      attemptAction: async () => {
        return { kind: "retryable", reason: "transport" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("retryable");

    const switchedTarget: RecoveryIdentity = {
      kind: "paired",
      proxyBackendId: "p1",
      hostId: "h2",
      remoteTarget: "t2",
      authorityGeneration: 1,
    };

    expect(scheduler.retryExplicit(switchedTarget)).toBe(false);
    expect(scheduler.getIdentityKey()).toBe(formatRecoveryKey(initialIdentity));
  });

  it("delimiter collision prevention: colons in IDs produce distinct keys", () => {
    const idA: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "a:b",
      backendSessionId: "c",
    };
    const idB: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "a",
      backendSessionId: "b:c",
    };

    const keyA = formatRecoveryKey(idA);
    const keyB = formatRecoveryKey(idB);

    expect(keyA).not.toBe(keyB);
    expect(JSON.parse(keyA)).toEqual(["local", "a:b", "c", null]);
    expect(JSON.parse(keyB)).toEqual(["local", "a", "b:c", null]);
  });

  it("incarnation conflict helper", () => {
    expect(checkIncarnationConflict("inc-1", "inc-2")).toBe(true);
    expect(checkIncarnationConflict("inc-1", "inc-1")).toBe(false);
    expect(checkIncarnationConflict("inc-1", null)).toBe(false);
    expect(checkIncarnationConflict(null, "inc-2")).toBe(false);
    expect(checkIncarnationConflict(undefined, undefined)).toBe(false);
  });

  it("authority correction helper", () => {
    const currentPaired: RecoveryIdentity = {
      kind: "paired",
      proxyBackendId: "p1",
      hostId: "h1",
      remoteTarget: "t1",
      authorityGeneration: 1,
    };
    const sameGenerationPaired: RecoveryIdentity = {
      ...currentPaired,
    };
    const olderGenerationPaired: RecoveryIdentity = {
      ...currentPaired,
      authorityGeneration: 0,
    };
    const newerGenerationPaired: RecoveryIdentity = {
      ...currentPaired,
      authorityGeneration: 2,
    };

    // Stale or identical generation cannot correct auth_revoked
    expect(isAuthorityCorrection(currentPaired, sameGenerationPaired, "auth_revoked")).toBe(false);
    expect(isAuthorityCorrection(currentPaired, olderGenerationPaired, "auth_revoked")).toBe(false);
    // Newer generation is affirmative corrective change
    expect(isAuthorityCorrection(currentPaired, newerGenerationPaired, "auth_revoked")).toBe(true);
  });

  it("same-target authority correction unblocks with typed AuthorityCorrectionReceipt and rejects stale receipt", async () => {
    const clock = createDeterministicClock(0);
    const initialPaired: RecoveryIdentity = {
      kind: "paired",
      proxyBackendId: "p1",
      hostId: "h1",
      remoteTarget: "t1",
      authorityGeneration: 1,
    };
    const scheduler = createRecoveryScheduler({
      identity: initialPaired,
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "control_conflict" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    const validToken = scheduler.getToken();
    const validKey = scheduler.getIdentityKey();

    // Stale receipt with previous token is rejected
    const staleReceipt = {
      currentToken: validToken - 1,
      currentIdentityKey: validKey,
      revalidatedIdentity: initialPaired,
      clearedConflict: true,
    };
    expect(scheduler.retryExplicit(staleReceipt)).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");

    // Valid receipt with current token unblocks
    const validReceipt = {
      currentToken: validToken,
      currentIdentityKey: validKey,
      revalidatedIdentity: initialPaired,
      clearedConflict: true,
    };
    expect(scheduler.retryExplicit(validReceipt)).toBe(true);
    expect(scheduler.getPhase()).toBe("retryable");
  });

  it("identity conflict unblock affirms original expected incarnation, rejects guessed replacement and legacy unconfirmed", () => {
    const retainedLocal: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-1",
      backendSessionId: "be-1",
      expectedIncarnation: "inc-canonical",
    };
    const matchingLocal: RecoveryIdentity = {
      ...retainedLocal,
    };
    const guessedReplacement: RecoveryIdentity = {
      ...retainedLocal,
      expectedIncarnation: "inc-guessed",
    };
    const legacyUnconfirmed: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-1",
      backendSessionId: "be-1",
    };

    const validProof: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(retainedLocal),
      revalidatedIdentity: matchingLocal,
      clearedConflict: true,
    };

    // Original expected incarnation affirmed to match with valid typed proof is accepted as correction
    expect(isAuthorityCorrection(retainedLocal, matchingLocal, "identity_conflict", validProof)).toBe(true);

    // Guessed replacement incarnation is rejected
    const guessedProof: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(retainedLocal),
      revalidatedIdentity: guessedReplacement,
      clearedConflict: true,
    };
    expect(isAuthorityCorrection(retainedLocal, guessedReplacement, "identity_conflict", guessedProof)).toBe(false);

    // Unknown legacy incarnation remains unconfirmed and cannot adopt a new one as correction
    expect(isAuthorityCorrection(legacyUnconfirmed, guessedReplacement, "identity_conflict", guessedProof)).toBe(false);
  });

  it("handles legacy null/undefined incarnation corrections across auth_expired, control_conflict, and identity_conflict deterministically", () => {
    const legacyNullSsh: RecoveryIdentity = {
      kind: "ssh",
      frontendSessionId: "fe-ssh-1",
      backendSessionId: "be-ssh-1",
      expectedIncarnation: null,
    };

    const legacyUndefinedLocal: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-loc-1",
      backendSessionId: "be-loc-1",
    };

    // 1. null credential correction allowed: exact null/undefined retained, credentialRefreshed = true
    const revalNullSsh: RecoveryIdentity = {
      ...legacyNullSsh,
      expectedIncarnation: null,
    };
    const validAuthProof: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyNullSsh),
      revalidatedIdentity: revalNullSsh,
      credentialRefreshed: true,
    };
    expect(isAuthorityCorrection(legacyNullSsh, revalNullSsh, "auth_expired", validAuthProof)).toBe(true);

    // Retaining undefined also treated as null equivalence when unchanged
    const revalUndefinedLocal: RecoveryIdentity = {
      ...legacyUndefinedLocal,
    };
    const validControlProof: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyUndefinedLocal),
      revalidatedIdentity: revalUndefinedLocal,
      clearedConflict: true,
    };
    expect(isAuthorityCorrection(legacyUndefinedLocal, revalUndefinedLocal, "control_conflict", validControlProof)).toBe(true);

    // 2. null-to-known rejected: adopting a newly discovered/guessed incarnation under auth_expired/control_conflict
    const guessedSshIncarnation: RecoveryIdentity = {
      ...legacyNullSsh,
      expectedIncarnation: "inc-newly-discovered",
    };
    const guessedAuthProof: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyNullSsh),
      revalidatedIdentity: guessedSshIncarnation,
      credentialRefreshed: true,
    };
    expect(isAuthorityCorrection(legacyNullSsh, guessedSshIncarnation, "auth_expired", guessedAuthProof)).toBe(false);

    const guessedControlProof: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyUndefinedLocal),
      revalidatedIdentity: {
        ...legacyUndefinedLocal,
        expectedIncarnation: "inc-guessed",
      },
      clearedConflict: true,
    };
    expect(isAuthorityCorrection(legacyUndefinedLocal, { ...legacyUndefinedLocal, expectedIncarnation: "inc-guessed" }, "control_conflict", guessedControlProof)).toBe(false);

    // 3. null identity conflict rejected: legacy null/undefined cannot clear identity_conflict even with clearedConflict proof
    const clearedConflictProofForNull: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyNullSsh),
      revalidatedIdentity: revalNullSsh,
      clearedConflict: true,
    };
    expect(isAuthorityCorrection(legacyNullSsh, revalNullSsh, "identity_conflict", clearedConflictProofForNull)).toBe(false);

    // 4. wrong identity / proof rejected:
    // 4a. proof token/key mismatch
    const mismatchedProofKey: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: "wrong-identity-key",
      revalidatedIdentity: revalNullSsh,
      credentialRefreshed: true,
    };
    expect(isAuthorityCorrection(legacyNullSsh, revalNullSsh, "auth_expired", mismatchedProofKey)).toBe(false);

    // 4b. wrong identity target (frontendSessionId / backendSessionId mismatch)
    const wrongBackendId: RecoveryIdentity = {
      ...legacyNullSsh,
      backendSessionId: "different-backend-session",
    };
    const wrongBackendProof: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyNullSsh),
      revalidatedIdentity: wrongBackendId,
      credentialRefreshed: true,
    };
    expect(isAuthorityCorrection(legacyNullSsh, wrongBackendId, "auth_expired", wrongBackendProof)).toBe(false);

    // 4c. missing or wrong proof flags (e.g. clearedConflict instead of credentialRefreshed for auth_expired)
    const wrongProofFlag: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyNullSsh),
      revalidatedIdentity: revalNullSsh,
      clearedConflict: true, // auth_expired requires credentialRefreshed = true
    };
    expect(isAuthorityCorrection(legacyNullSsh, revalNullSsh, "auth_expired", wrongProofFlag)).toBe(false);

    // 4d. missing proof flag for control_conflict (e.g. credentialRefreshed instead of clearedConflict)
    const wrongControlFlag: AuthorityCorrectionReceipt = {
      currentToken: 1,
      currentIdentityKey: formatRecoveryKey(legacyNullSsh),
      revalidatedIdentity: revalNullSsh,
      credentialRefreshed: true, // control_conflict requires clearedConflict = true
    };
    expect(isAuthorityCorrection(legacyNullSsh, revalNullSsh, "control_conflict", wrongControlFlag)).toBe(false);
  });
});
