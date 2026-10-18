/** QA-only read-only observations. This module never starts or cancels recovery. */
export type WebOwnerTarget = Readonly<{
  sessionId: string;
  daemonEpoch: string | null;
}>;

export type WebOwnerObservation = Readonly<{
  retainedTarget: WebOwnerTarget;
  socketId: string | null;
  generation: number;
  presentationSequence: number;
  pendingTimers: number;
  inFlight: boolean;
  disposed: boolean;
  identityConfirmed: boolean;
  exitSettlements: number | null;
  cancelledAttemptIds: readonly string[];
  staleCompletionsRejected: number;
  rejectedAttemptIds: readonly string[];
  incarnation?: string | null;
  outcome?: string | null;
  activity?: "active" | "settled" | "disposed" | null;
  replayGapsObserved?: number;
}>;

type Query = WebOwnerTarget & Readonly<{ incarnation?: string | null; socketId?: string; generation?: number; attemptId?: string }>;
type Entry = {
  readonly target: WebOwnerTarget;
  readonly read: () => WebOwnerObservation;
  readonly waiters: Set<() => void>;
  readonly retiredWaiters: Set<() => void>;
  readonly rejectedAttempts: Set<string>;
};

const entries: Entry[] = [];
const deadlines = new Map<() => void, () => void>();

function matching(target: Query): Entry {
  const matches = entries.filter(entry => entry.target.sessionId === target.sessionId &&
    entry.target.daemonEpoch === target.daemonEpoch &&
    (target.incarnation === undefined || entry.read().incarnation === target.incarnation) &&
    (target.socketId === undefined || entry.read().socketId === target.socketId) &&
    (target.generation === undefined || entry.read().generation === target.generation ||
      entry.read().cancelledAttemptIds.includes(String(target.generation))));
  const entry = matches.at(-1);
  if (matches.length > 1) throw new Error("WEB_OWNER_TARGET_AMBIGUOUS");
  if (!entry) throw new Error("WEB_OWNER_TARGET_EVIDENCE_REQUIRED");
  return entry;
}

export const webRecoveryQA = {
  currentTarget() {
    const active = entries.filter(entry => !entry.read().disposed);
    if (active.length !== 1) throw new Error("WEB_OWNER_CURRENT_TARGET_AMBIGUOUS_OR_ABSENT");
    const observation = active[0].read();
    if (!observation.identityConfirmed || !observation.socketId || !observation.presentationSequence)
      throw new Error("WEB_OWNER_CURRENT_BINDING_EVIDENCE_REQUIRED");
    return { ...observation.retainedTarget, socketId: observation.socketId,
      generation: observation.generation, presentationSequence: observation.presentationSequence,
      incarnation: observation.incarnation ?? null, outcome: observation.outcome ?? null,
      exitSettlements: observation.exitSettlements,
    };
  },
  snapshot(target: Query) {
    return matching(target).read();
  },
  waitForStaleCompletion(target: Query): Promise<WebOwnerObservation> {
    const entry = matching(target);
    const rejected = () => target.attemptId === undefined
      ? entry.rejectedAttempts.size > 0 : entry.rejectedAttempts.has(target.attemptId);
    if (rejected()) return Promise.resolve(entry.read());
    return new Promise((resolve, reject) => {
      const finish = () => {
        if (!rejected()) return;
        clearTimeout(timer);
        entry.waiters.delete(finish);
        deadlines.delete(finish);
        resolve(entry.read());
      };
      const cancel = () => {
        clearTimeout(timer);
        entry.waiters.delete(finish);
        deadlines.delete(finish);
        reject(new Error("WEB_OWNER_OBSERVATION_DISPOSED"));
      };
      const timer = setTimeout(() => {
        entry.waiters.delete(finish);
        deadlines.delete(finish);
        reject(new Error("WEB_OWNER_STALE_COMPLETION_DEADLINE"));
      }, 15000);
      entry.waiters.add(finish);
      deadlines.set(finish, cancel);
    });
  },
  waitForRetiredStaleCompletion(target: Query, attemptId: string, timeoutMs: number): Promise<WebOwnerObservation> {
    const entry = matching(target);
    if (!attemptId || !Number.isFinite(timeoutMs) || timeoutMs <= 0)
      return Promise.reject(new Error("WEB_OWNER_RETIRED_COMPLETION_ARGUMENT_INVALID"));
    if (entry.rejectedAttempts.has(attemptId)) return Promise.resolve(entry.read());
    return new Promise(resolve => {
      const finish = () => {
        if (!entry.rejectedAttempts.has(attemptId)) return;
        clearTimeout(timer);
        entry.retiredWaiters.delete(finish);
        deadlines.delete(finish);
        resolve(entry.read());
      };
      const cancel = () => {
        clearTimeout(timer);
        entry.retiredWaiters.delete(finish);
        deadlines.delete(finish);
        resolve(entry.read());
      };
      const timer = setTimeout(() => {
        entry.retiredWaiters.delete(finish);
        deadlines.delete(finish);
        resolve(entry.read());
      }, timeoutMs);
      entry.retiredWaiters.add(finish);
      deadlines.set(finish, cancel);
    });
  },
};

declare global {
  interface Window {
    __ferryxRecoveryQA?: typeof webRecoveryQA;
    __task9Wire?: unknown;
  }
}

export function observeWebOwner(target: WebOwnerTarget, read: () => WebOwnerObservation) {
  if (typeof window === "undefined" || !window.__task9Wire) return null;
  const entry: Entry = { target: Object.freeze({ ...target }), read, waiters: new Set(), retiredWaiters: new Set(), rejectedAttempts: new Set() };
  // Retain recent retired owners for late completion evidence, not unbounded page history.
  if (entries.length === 32) {
    const retired = entries.findIndex(item => item.read().disposed);
    if (retired < 0) throw new Error("WEB_OWNER_OBSERVATION_CAPACITY");
    for (const waiter of [...entries[retired].waiters, ...entries[retired].retiredWaiters]) deadlines.get(waiter)?.();
    entries.splice(retired, 1);
  }
  entries.push(entry);
  window.__ferryxRecoveryQA = webRecoveryQA;
  return {
    // Wake QA predicates when owner state changes without a DOM mutation.
    publish() {
      window.dispatchEvent(new Event("ferryx-web-owner-observation"));
    },
    staleCompletionRejected(attemptId: string) {
      entry.rejectedAttempts.add(attemptId);
      for (const waiter of [...entry.waiters]) waiter();
      for (const waiter of [...entry.retiredWaiters]) waiter();
    },
    dispose() {
      for (const waiter of [...entry.waiters]) deadlines.get(waiter)?.();
    },
  };
}

if (import.meta.hot) import.meta.hot.dispose(() => {
  for (const cancel of [...deadlines.values()]) cancel();
  entries.length = 0;
  if (window.__ferryxRecoveryQA === webRecoveryQA) delete window.__ferryxRecoveryQA;
});
