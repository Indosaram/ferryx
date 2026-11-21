/** One explicit desktop selection is one publication intent. Never retry or rebase it. */
export interface DesktopContinuityAccount {
  readonly accountOrigin: string;
  readonly accountId: string;
  readonly sessionToken: string;
}

export interface DesktopContinuityCredentials {
  readonly accountOrigin: string;
  readonly sessionToken: string;
}

export interface DesktopContinuityTarget {
  readonly machineId: string;
  readonly workspaceId: string;
  readonly worktreeSlug: string | null;
  readonly sessionId: string;
}

export interface DesktopContinuityRevision {
  readonly revision: number;
}

export interface DesktopContinuityIntent {
  readonly credentials: DesktopContinuityCredentials;
  readonly generation: number;
  readonly baseline: Promise<
    | { readonly ok: true; readonly revision: number; readonly error: null }
    | { readonly ok: false; readonly revision: 0; readonly error: unknown }
  >;
}

export interface DesktopContinuityDependencies {
  currentAccount(): DesktopContinuityAccount | null;
  read(credentials: DesktopContinuityCredentials, signal: AbortSignal): Promise<DesktopContinuityRevision | null>;
  write(account: DesktopContinuityAccount, target: DesktopContinuityTarget, expectedRevision: number,
    signal: AbortSignal): Promise<DesktopContinuityRevision>;
  onError(error: unknown): void;
  schedule(callback: () => Promise<void>): () => void;
}

function sameAccount(left: DesktopContinuityAccount | null, right: DesktopContinuityAccount): boolean {
  return left?.accountOrigin === right.accountOrigin && left.accountId === right.accountId &&
    left.sessionToken === right.sessionToken;
}

function sameCredentials(left: DesktopContinuityCredentials | null,
  right: DesktopContinuityCredentials): boolean {
  return left?.accountOrigin === right.accountOrigin && left.sessionToken === right.sessionToken;
}

export function createDesktopContinuityPublisher(dependencies: DesktopContinuityDependencies) {
  let generation = 0;
  let cancelScheduled: (() => void) | undefined;
  let request: AbortController | undefined;
  let inFlight: { account: DesktopContinuityAccount; controller: AbortController;
    expectedRevision: number; acknowledged: Promise<DesktopContinuityRevision | null> } | undefined;

  const invalidate = (preserveWrite = false) => {
    generation += 1;
    cancelScheduled?.();
    cancelScheduled = undefined;
    if (!preserveWrite || request !== inFlight?.controller) request?.abort();
    request = undefined;
    if (!preserveWrite) {
      inFlight?.controller.abort();
      inFlight = undefined;
    }
  };

  return {
    /** Call on sign-out, account/token/origin change, loss of window leadership, or unmount.
     * Invalidation is local only: it never clears the account-wide pointer. */
    invalidate: () => invalidate(),
    /** Capture the baseline for one explicit selection now, before its pane, machine, or account
     * identity resolve. A publication that lands after another device's write then conflicts
     * instead of adopting that device's revision as its own baseline. */
    begin(credentials: DesktopContinuityCredentials): DesktopContinuityIntent {
      const previous = inFlight && sameCredentials(credentials, inFlight.account) &&
        sameAccount(dependencies.currentAccount(), inFlight.account) ? inFlight : undefined;
      invalidate(previous !== undefined);
      const intentGeneration = generation;
      const controller = new AbortController();
      request = controller;
      // Project exactly the two credential fields: a caller that hands over a richer account
      // object must not leak its account id into the stored baseline request.
      const snapshot = { accountOrigin: credentials.accountOrigin, sessionToken: credentials.sessionToken };
      // Capture the baseline for this event, not after a delayed PUT or a conflict. A newer
      // event gets a new baseline; an old event never adopts a conflict's current revision.
      const baseline = dependencies.read(snapshot, controller.signal).then(
        async (pointer) => {
          const observed = pointer?.revision ?? 0;
          // Preserve the event-time read: advance it only across our own acknowledged CAS,
          // never across a conflicting write from another device.
          const ack = previous ? await previous.acknowledged : null;
          const revision = previous?.expectedRevision === observed && ack ? ack.revision : observed;
          return { ok: true, revision, error: null } as const;
        },
        (error: unknown) => ({ ok: false, revision: 0, error } as const),
      );
      return { credentials: snapshot, generation: intentGeneration, baseline };
    },
    /** Publish one committed selection with the baseline its `begin` captured. A superseded
     * intent has no baseline left to trust and is dropped. */
    select(target: DesktopContinuityTarget, intent: DesktopContinuityIntent): void {
      const controller = request;
      if (intent.generation !== generation || !controller || controller.signal.aborted) return;
      const account = dependencies.currentAccount();
      if (!account || !sameCredentials(account, intent.credentials)) return;
      const selectedAccount = { ...account };
      const selectedTarget = { ...target };
      const current = () => generation === intent.generation && !controller.signal.aborted &&
        sameAccount(dependencies.currentAccount(), selectedAccount);
      cancelScheduled = dependencies.schedule(async () => {
        cancelScheduled = undefined;
        try {
          const result = await intent.baseline;
          if (!current()) return;
          if (!result.ok) throw result.error;
          const write = dependencies.write(selectedAccount, selectedTarget, result.revision, controller.signal);
          const pending = { account: selectedAccount, controller, expectedRevision: result.revision,
            acknowledged: write.catch(() => null) };
          inFlight = pending;
          try { await write; } finally { if (inFlight === pending) inFlight = undefined; }
        } catch (error: unknown) {
          // Losing CAS is a normal outcome: keep the winner and never rebase this intent.
          if (typeof error === "object" && error !== null && "code" in error &&
            error.code === "CONTINUITY_CONFLICT") return;
          if (current()) dependencies.onError(error);
        }
      });
    },
  };
}
