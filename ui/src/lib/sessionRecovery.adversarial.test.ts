import { describe, expect, it } from "vitest";
import {
  createRecoveryScheduler,
  formatRecoveryKey,
  isAuthorityCorrection,
  type AuthorityCorrectionReceipt,
  type RecoveryAttemptResult,
  type RecoveryClock,
  type RecoveryIdentity,
} from "./sessionRecovery";

interface DeterministicClock extends RecoveryClock {
  readonly pendingTimers: number;
  readonly timerList: Array<{ id: number; triggerTime: number }>;
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
    get timerList() {
      return Array.from(timers.values()).map(t => ({ id: t.id, triggerTime: t.triggerTime }));
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
        // Crucial: allow microtask queue to drain after each timer callback!
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

describe("sessionRecovery - independent adversarial verification", () => {
  it("adversarial: synchronous AbortSignal listener reentrancy must not duplicate timers or violate single-flight", async () => {
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

    // Advance to deadline
    await clock.tick(1000);

    // Invariant: MUST NOT have both inFlight and pending timer simultaneously,
    // and MUST NOT have more than 1 timer registered!
    expect(clock.pendingTimers).toBeLessThanOrEqual(1);
    const violatingConcurrentState = scheduler.isInFlight() && scheduler.isTimerPending();
    expect(violatingConcurrentState).toBe(false);
  });

  it("adversarial: abort-listener cancel and rebind must cleanly finalize without timer duplication", async () => {
    const clock = createDeterministicClock(0);

    // Sub-case A: abort listener calls cancel()
    const cancelScheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-abort-cancel", backendSessionId: "be-abort-cancel" },
      clock,
      attemptDeadlineMs: 1000,
      attemptAction: async ({ signal }) => {
        signal.addEventListener("abort", () => {
          cancelScheduler.cancel();
        });
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    cancelScheduler.start(true);
    await clock.tick(1000); // Trigger timeout
    expect(cancelScheduler.getPhase()).toBe("cancelled");
    expect(clock.pendingTimers).toBe(0);
    expect(cancelScheduler.isInFlight()).toBe(false);
    expect(cancelScheduler.isTimerPending()).toBe(false);

    // Sub-case B: abort listener calls rebind()
    const rebindScheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-abort-rebind", backendSessionId: "be-abort-rebind" },
      clock,
      attemptDeadlineMs: 1000,
      attemptAction: async ({ signal }) => {
        signal.addEventListener("abort", () => {
          rebindScheduler.rebind({
            kind: "local",
            frontendSessionId: "fe-abort-rebind-new",
            backendSessionId: "be-abort-rebind-new",
          });
        });
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    rebindScheduler.start(true);
    await clock.tick(1000); // Trigger timeout
    expect(rebindScheduler.getIdentityKey()).toBe(
      formatRecoveryKey({
        kind: "local",
        frontendSessionId: "fe-abort-rebind-new",
        backendSessionId: "be-abort-rebind-new",
      })
    );
    expect(clock.pendingTimers).toBe(0);
    expect(rebindScheduler.isInFlight()).toBe(false);
    expect(rebindScheduler.isTimerPending()).toBe(false);
  });

  it("adversarial: outcome listener reentrancy must not duplicate timers", async () => {
    const clock = createDeterministicClock(0);
    let attempts = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-listener", backendSessionId: "be-listener" },
      clock,
      initialDelayMs: 250,
      onOutcomeChange: (outcome) => {
        if (outcome.phase === "retryable" && outcome.reason === "transport") {
          scheduler.wake("reentrant-outcome-wake");
        }
      },
      attemptAction: async () => {
        attempts++;
        if (attempts === 1) {
          return { kind: "retryable", reason: "transport" };
        }
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    scheduler.start(true);
    await clock.tick(0);

    expect(clock.pendingTimers).toBeLessThanOrEqual(1);
    const violatingConcurrentState = scheduler.isInFlight() && scheduler.isTimerPending();
    expect(violatingConcurrentState).toBe(false);
  });

  it("adversarial: abort-aware live action count distinguishes freed scheduler slot from outstanding transport", async () => {
    const clock = createDeterministicClock(0);
    let outstandingTransportProcesses = 0;
    let attemptsStarted = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-transport-count", backendSessionId: "be-transport-count" },
      clock,
      attemptDeadlineMs: 1000,
      initialDelayMs: 250,
      attemptAction: async ({ signal }) => {
        attemptsStarted++;
        outstandingTransportProcesses++;
        signal.addEventListener("abort", () => {
          // Outstanding transport cleans up its transport handle upon abort
          outstandingTransportProcesses--;
        });
        return new Promise<RecoveryAttemptResult>(() => {}); // transport hangs indefinitely
      },
    });

    scheduler.start(true);
    expect(attemptsStarted).toBe(1);
    expect(scheduler.isInFlight()).toBe(true);
    expect(outstandingTransportProcesses).toBe(1);

    // Timeout occurs: deadline frees the scheduler slot, while abort signal cleans up transport
    await clock.tick(1000);
    expect(scheduler.isInFlight()).toBe(false);
    expect(outstandingTransportProcesses).toBe(0); // Proves abort-signal notification settled outstanding transport
  });

  it("adversarial: explicit retry must NOT reset backoff delay to initialDelayMs upon failure", async () => {
    const clock = createDeterministicClock(0);
    let attempts = 0;

    const scheduler = createRecoveryScheduler({
      identity: { kind: "paired", proxyBackendId: "p1", hostId: "h1", remoteTarget: "t1", authorityGeneration: 1 },
      clock,
      initialDelayMs: 250,
      maxDelayMs: 4000,
      backoffFactor: 2,
      attemptAction: async () => {
        attempts++;
        return { kind: "retryable", reason: "transport" };
      },
    });

    scheduler.start(true);
    // Failure 1: delay 250
    await clock.tick(0);
    // Failure 2: delay 500
    await clock.tick(250);
    // Failure 3: delay 1000
    await clock.tick(500);

    expect(attempts).toBe(3);
    const preOutcome = scheduler.getOutcome();
    expect(preOutcome.phase).toBe("retryable");
    if (preOutcome.phase === "retryable") {
      expect(preOutcome.attempt).toBe(3);
      expect(preOutcome.nextDelayMs).toBe(1000);
    }

    // Call retryExplicit: must NOT reset accumulated failure cadence unless ready is achieved
    scheduler.retryExplicit();
    await clock.tick(0); // attempt 4 runs and fails

    const postOutcome = scheduler.getOutcome();
    expect(postOutcome.phase).toBe("retryable");
    if (postOutcome.phase === "retryable") {
      // Normative contract: Cadence reset occurs ONLY upon validated ready.
      // Next failure delay must NOT revert to initial 250ms!
      expect(postOutcome.nextDelayMs).toBeGreaterThan(250);
    }
  });

  it("adversarial: reportReady must NOT bypass blocked phase even with valid proof", async () => {
    const clock = createDeterministicClock(0);

    const scheduler = createRecoveryScheduler({
      identity: { kind: "paired", proxyBackendId: "p1", hostId: "h1", remoteTarget: "t1", authorityGeneration: 1 },
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "auth_revoked" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    const validProof = {
      token: scheduler.getToken(),
      identityKey: scheduler.getIdentityKey(),
    };

    // Even with matching token/key proof, external reportReady must be rejected in blocked phase!
    const accepted = scheduler.reportReady({ details: "unauthorized" }, validProof);
    expect(accepted).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("adversarial: cancelled state must be preserved and cannot be overwritten by reportExit even with valid proof", async () => {
    const clock = createDeterministicClock(0);

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-cancel-preserv", backendSessionId: "be-cancel-preserv" },
      clock,
      attemptAction: async () => {
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    scheduler.start(true);
    scheduler.cancel();
    expect(scheduler.getPhase()).toBe("cancelled");

    const validProof = {
      token: scheduler.getToken(),
      identityKey: scheduler.getIdentityKey(),
    };

    // Calling reportExit after cancellation must NOT overwrite cancelled state
    const accepted = scheduler.reportExit({ exitCode: 0 }, validProof);
    expect(accepted).toBe(false);
    expect(scheduler.getPhase()).toBe("cancelled");
  });

  it("adversarial: stale external receipts for all report types must be rejected by proof validation", async () => {
    const clock = createDeterministicClock(0);

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-stale", backendSessionId: "be-stale" },
      clock,
      attemptAction: async () => {
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    scheduler.start(true);
    const initialToken = scheduler.getToken();
    const initialKey = scheduler.getIdentityKey();

    // Rebind to a new identity
    const newIdentity: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-stale-new",
      backendSessionId: "be-stale-new",
    };
    scheduler.rebind(newIdentity);
    const reboundToken = scheduler.getToken();
    const reboundKey = scheduler.getIdentityKey();

    expect(reboundToken).toBeGreaterThan(initialToken);
    expect(reboundKey).not.toBe(initialKey);

    // Stale proof carrying the old token or old identityKey
    const staleProofOldToken = { token: initialToken, identityKey: reboundKey };
    const staleProofOldKey = { token: reboundToken, identityKey: initialKey };

    expect(scheduler.reportReady({ status: "stale" }, staleProofOldToken)).toBe(false);
    expect(scheduler.reportReady({ status: "stale" }, staleProofOldKey)).toBe(false);

    expect(scheduler.reportRetryable("transport", undefined, undefined, staleProofOldToken)).toBe(false);
    expect(scheduler.reportRetryable("transport", undefined, undefined, staleProofOldKey)).toBe(false);

    expect(scheduler.reportBlocked("auth_revoked", undefined, staleProofOldToken)).toBe(false);
    expect(scheduler.reportBlocked("auth_revoked", undefined, staleProofOldKey)).toBe(false);

    expect(scheduler.reportExit({ exitCode: 0 }, staleProofOldToken)).toBe(false);
    expect(scheduler.reportExit({ exitCode: 0 }, staleProofOldKey)).toBe(false);

    expect(scheduler.getIdentityKey()).toBe(reboundKey);
    expect(scheduler.getToken()).toBe(reboundToken);
  });

  it("adversarial: abort signal must fire upon timeout, cancel, and rebind (with explicit start)", async () => {
    const clock = createDeterministicClock(0);
    const signals: AbortSignal[] = [];

    const scheduler = createRecoveryScheduler({
      identity: { kind: "local", frontendSessionId: "fe-abort", backendSessionId: "be-abort" },
      clock,
      attemptDeadlineMs: 1000,
      initialDelayMs: 250,
      attemptAction: async ({ signal }) => {
        signals.push(signal);
        return new Promise<RecoveryAttemptResult>(() => {});
      },
    });

    // 1. Timeout aborts signal
    scheduler.start(true);
    expect(signals.length).toBe(1);
    expect(signals[0].aborted).toBe(false);
    await clock.tick(1000);
    expect(signals[0].aborted).toBe(true);

    // 2. Next attempt starts on backoff tick
    await clock.tick(250);
    expect(signals.length).toBe(2);
    expect(signals[1].aborted).toBe(false);

    // Rebind aborts active attempt
    scheduler.rebind({ kind: "local", frontendSessionId: "fe-rebound", backendSessionId: "be-rebound" });
    expect(signals[1].aborted).toBe(true);

    // 3. Explicitly trigger start(true) after rebind as required by contract
    scheduler.start(true);
    expect(signals.length).toBe(3);
    expect(signals[2].aborted).toBe(false);

    // Cancel aborts active attempt
    scheduler.cancel();
    expect(signals[2].aborted).toBe(true);
  });

  it("adversarial: same-target corrective authority must unblock while mismatched target or guessed incarnation is rejected", async () => {
    // 1. Paired target: changing hostId or remoteTarget must NOT be accepted as authority correction of retained session
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
      hostId: "host-2", // Switched host!
      remoteTarget: "target-2", // Switched target!
      authorityGeneration: 1,
    };
    // Normative contract: target replacement is NOT authority correction of retained session
    expect(isAuthorityCorrection(currentPaired, switchedTargetPaired, "auth_revoked")).toBe(false);

    // 2. Local/SSH target: changing expectedIncarnation must NOT be accepted as identity_conflict correction (normative contract forbids guessing replacement)
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
      expectedIncarnation: "inc-2", // Guessed new incarnation!
    };
    expect(isAuthorityCorrection(currentLocal, guessedIncarnationLocal, "identity_conflict")).toBe(false);

    // 3. Same-target revalidation: same retained incarnation and target after credentials/control refreshed
    // MUST be allowed to unblock via explicit retry
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

    // Normative contract: Control conflict waits for controller-release/ownership refresh or explicit Retry;
    // both no-argument AND same-identity retryExplicit must be rejected (false/unchanged) until actual typed fresh correction!
    expect(localScheduler.retryExplicit()).toBe(false);
    expect(localScheduler.getPhase()).toBe("blocked");

    expect(localScheduler.retryExplicit(currentLocal)).toBe(false);
    expect(localScheduler.getPhase()).toBe("blocked");

    // Only actual affirmative typed correction receipt with clearedConflict: true can unblock!
    const validConflictReceipt: AuthorityCorrectionReceipt = {
      currentToken: localScheduler.getToken(),
      currentIdentityKey: localScheduler.getIdentityKey(),
      revalidatedIdentity: currentLocal,
      clearedConflict: true,
    };
    const retryAccepted = localScheduler.retryExplicit(validConflictReceipt);
    expect(retryAccepted).toBe(true);
    expect(localScheduler.getPhase()).toBe("retryable");
  });

  it("adversarial: rebind must NOT silently bypass blocked phase to retryable without authority revalidation", async () => {
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
        return { kind: "blocked", reason: "auth_revoked" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    // Calling rebind while blocked must NOT bypass blocked phase into retryable without affirmative authority revalidation
    scheduler.rebind({
      kind: "paired",
      proxyBackendId: "p1",
      hostId: "h1",
      remoteTarget: "t1",
      authorityGeneration: 1, // Same uncorrected authority
    });
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("adversarial: reportExit must NOT overwrite blocked phase with exited even with valid proof", async () => {
    const clock = createDeterministicClock(0);
    const initialIdentity: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-block-exit",
      backendSessionId: "be-block-exit",
    };

    const scheduler = createRecoveryScheduler({
      identity: initialIdentity,
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "identity_conflict" };
      },
    });

    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    const validProof = {
      token: scheduler.getToken(),
      identityKey: scheduler.getIdentityKey(),
    };

    // Normative contract: blocked: old ready/exit/failure receipt -> unchanged / ignore
    // With matching current token and key, reportExit must STILL be rejected while in blocked phase!
    const accepted = scheduler.reportExit({ exitCode: 1 }, validProof);
    expect(accepted).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("adversarial: retryExplicit must NOT allow arbitrary identity or target switching during non-blocked retry", async () => {
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

    // Attempting to switch target through retryExplicit must be rejected:
    // "New target transition must cancel old scheduler/create new owner only by explicit selection, not count as recovery."
    const switchedTarget: RecoveryIdentity = {
      kind: "paired",
      proxyBackendId: "p1",
      hostId: "h2", // Switched host
      remoteTarget: "t2", // Switched target
      authorityGeneration: 1,
    };

    scheduler.retryExplicit(switchedTarget);
    // Identity key must NOT be silently switched to a new target
    expect(scheduler.getIdentityKey()).toBe(formatRecoveryKey(initialIdentity));
  });

  it("adversarial case 1: same identity without affirmative revalidation -> blocked", async () => {
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
        return { kind: "blocked", reason: "auth_revoked" };
      },
    });
    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    // Calling retryExplicit without affirmative revalidation receipt must fail
    expect(scheduler.retryExplicit()).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");

    // Calling retryExplicit with same identity but no proof/receipt must fail
    expect(scheduler.retryExplicit(initialIdentity)).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("adversarial case 2: fresh current-key/token matching retained target/incarnation plus valid credentials/control -> retry", async () => {
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
        return { kind: "blocked", reason: "auth_expired" };
      },
    });
    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    const validReceipt: AuthorityCorrectionReceipt = {
      currentToken: scheduler.getToken(),
      currentIdentityKey: scheduler.getIdentityKey(),
      revalidatedIdentity: {
        ...initialIdentity,
        authorityGeneration: 2, // Renewed authority generation for same retained target!
      },
      credentialRefreshed: true,
    };

    expect(scheduler.retryExplicit(validReceipt)).toBe(true);
    expect(scheduler.getPhase()).toBe("retryable");
    expect(scheduler.getOutcome().phase).toBe("retryable");
  });

  it("adversarial case 3: stale corrective receipt -> blocked", async () => {
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
        return { kind: "blocked", reason: "auth_revoked" };
      },
    });
    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    const staleTokenReceipt: AuthorityCorrectionReceipt = {
      currentToken: scheduler.getToken() - 1, // Stale token!
      currentIdentityKey: scheduler.getIdentityKey(),
      revalidatedIdentity: {
        ...initialIdentity,
        authorityGeneration: 2,
      },
    };
    expect(scheduler.retryExplicit(staleTokenReceipt)).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");

    const wrongKeyReceipt: AuthorityCorrectionReceipt = {
      currentToken: scheduler.getToken(),
      currentIdentityKey: '["wrong","key"]', // Mismatched key!
      revalidatedIdentity: {
        ...initialIdentity,
        authorityGeneration: 2,
      },
    };
    expect(scheduler.retryExplicit(wrongKeyReceipt)).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
  });

  it("adversarial case 4: original expected incarnation returns after identity_conflict -> retry", async () => {
    const clock = createDeterministicClock(0);
    const originalLocal: RecoveryIdentity = {
      kind: "local",
      frontendSessionId: "fe-1",
      backendSessionId: "be-1",
      expectedIncarnation: "inc-expected",
    };
    const scheduler = createRecoveryScheduler({
      identity: originalLocal,
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "identity_conflict" };
      },
    });
    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    // Original expected incarnation is confirmed to have returned upon revalidation
    const revalidationReceipt: AuthorityCorrectionReceipt = {
      currentToken: scheduler.getToken(),
      currentIdentityKey: scheduler.getIdentityKey(),
      revalidatedIdentity: originalLocal,
      clearedConflict: true,
    };

    expect(scheduler.retryExplicit(revalidationReceipt)).toBe(true);
    expect(scheduler.getPhase()).toBe("retryable");
  });

  it("adversarial case 5: different incarnation/host/target -> blocked", async () => {
    // A. Helper negative assertions (must preserve target switching helper negative assertions)
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
      hostId: "host-2", // Switched host!
      remoteTarget: "target-2", // Switched target!
      authorityGeneration: 1,
    };
    expect(isAuthorityCorrection(currentPaired, switchedTargetPaired, "auth_revoked")).toBe(false);

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
      expectedIncarnation: "inc-2", // Guessed new incarnation!
    };
    expect(isAuthorityCorrection(currentLocal, guessedIncarnationLocal, "identity_conflict")).toBe(false);

    // B. Scheduler enforcement: different target/incarnation in retryExplicit must be rejected
    const clock = createDeterministicClock(0);
    const scheduler = createRecoveryScheduler({
      identity: currentPaired,
      clock,
      attemptAction: async () => {
        return { kind: "blocked", reason: "auth_revoked" };
      },
    });
    scheduler.start(true);
    await clock.tick(0);
    expect(scheduler.getPhase()).toBe("blocked");

    const switchedReceipt: AuthorityCorrectionReceipt = {
      currentToken: scheduler.getToken(),
      currentIdentityKey: scheduler.getIdentityKey(),
      revalidatedIdentity: switchedTargetPaired, // Switched target!
      credentialRefreshed: true,
    };

    expect(scheduler.retryExplicit(switchedReceipt)).toBe(false);
    expect(scheduler.getPhase()).toBe("blocked");
  });
});
