/**
 * Consistent session recovery contracts and single-flight scheduler.
 * Implements approved plan §Todos Task 1 (GAP-6).
 *
 * Vocabulary at adapter boundaries:
 *   ready | retryable | blocked | exited | cancelled
 */
import { appendNativeRecoveryEvent } from "./nativeQaReceipts";
import { getDurableNativeBinding } from "./nativeTerminalLifecycle";

export type RecoveryPhase =
  | "ready"
  | "retryable"
  | "blocked"
  | "exited"
  | "cancelled";

export type BlockedReason =
  | "auth_revoked"
  | "auth_expired"
  | "pairing_revoked"
  | "identity_conflict"
  | "control_conflict"
  | "protocol_error";

export type RetryableReason =
  | "transport"
  | "owner_unknown"
  | "owner_unavailable"
  | "timeout"
  | "stream_ended";

export interface LocalOrSshRecoveryIdentity {
  readonly kind: "local" | "ssh";
  readonly frontendSessionId: string;
  readonly backendSessionId: string;
  readonly expectedIncarnation?: string | null;
}

export interface PairedRecoveryIdentity {
  readonly kind: "paired";
  readonly proxyBackendId: string;
  readonly remoteTarget: string;
  readonly hostId: string;
  readonly authorityGeneration: number;
}

export type RecoveryIdentity = LocalOrSshRecoveryIdentity | PairedRecoveryIdentity;

/**
 * Formats a canonical unambiguous composite key for a recovery identity.
 * Uses structured array serialization to strictly prevent delimiter collision vulnerabilities.
 */
export function formatRecoveryKey(identity: RecoveryIdentity): string {
  if (identity.kind === "paired") {
    return JSON.stringify([
      "paired",
      identity.proxyBackendId,
      identity.hostId,
      identity.remoteTarget,
      identity.authorityGeneration,
    ]);
  }
  return JSON.stringify([
    identity.kind,
    identity.frontendSessionId,
    identity.backendSessionId,
    identity.expectedIncarnation ?? null,
  ]);
}

/**
 * Validates incarnation compatibility.
 * An incarnation mismatch is an identity conflict (blocked).
 * A null/undefined incarnation in either operand is treated as unconfirmed/legacy, not an affirmative conflict.
 */
export function checkIncarnationConflict(
  expectedIncarnation: string | null | undefined,
  observedIncarnation: string | null | undefined
): boolean {
  if (
    expectedIncarnation !== null &&
    expectedIncarnation !== undefined &&
    observedIncarnation !== null &&
    observedIncarnation !== undefined &&
    expectedIncarnation !== observedIncarnation
  ) {
    return true;
  }
  return false;
}

export interface AuthorityCorrectionReceipt {
  readonly currentToken: number;
  readonly currentIdentityKey: string;
  readonly revalidatedIdentity: RecoveryIdentity;
  readonly clearedConflict?: boolean;
  readonly credentialRefreshed?: boolean;
}

export function isAuthorityCorrectionReceipt(
  target: unknown
): target is AuthorityCorrectionReceipt {
  return (
    typeof target === "object" &&
    target !== null &&
    "currentToken" in target &&
    "currentIdentityKey" in target &&
    "revalidatedIdentity" in target
  );
}

/**
 * Validates whether a revalidated identity supplies an affirmative corrective authority change
 * required to unblock recovery (Plan §Normative recovery contract: "Revocation never retries with old credentials").
 * Target replacement is NOT recovery of the retained session; target switching and guessed incarnations are rejected.
 */
export function isAuthorityCorrection(
  current: RecoveryIdentity,
  revalidated: RecoveryIdentity,
  blockedReason?: BlockedReason,
  proof?: AuthorityCorrectionReceipt
): boolean {
  if (proof !== undefined) {
    if (proof.currentIdentityKey !== formatRecoveryKey(current)) {
      return false;
    }
  }

  if (current.kind === "paired") {
    if (revalidated.kind !== "paired") {
      return false;
    }
    // Must preserve the retained session and target: proxyBackendId, hostId, and remoteTarget must match!
    if (
      revalidated.proxyBackendId !== current.proxyBackendId ||
      revalidated.hostId !== current.hostId ||
      revalidated.remoteTarget !== current.remoteTarget
    ) {
      return false; // Target switching is NOT recovery of the retained session!
    }

    if (blockedReason === "auth_revoked" || blockedReason === "pairing_revoked") {
      // Authority generation MUST advance
      return revalidated.authorityGeneration > current.authorityGeneration;
    }

    if (blockedReason === "auth_expired") {
      // Credential refresh proof or generation advancement for the retained target (no unverified same-generation fallthrough)
      return (
        revalidated.authorityGeneration > current.authorityGeneration ||
        proof?.credentialRefreshed === true
      );
    }

    if (blockedReason === "control_conflict") {
      return (
        revalidated.authorityGeneration > current.authorityGeneration ||
        proof?.clearedConflict === true
      );
    }

    if (blockedReason === "protocol_error") {
      return (
        revalidated.authorityGeneration > current.authorityGeneration ||
        proof?.clearedConflict === true
      );
    }

    return revalidated.authorityGeneration > current.authorityGeneration;
  }

  if (current.kind === "local" || current.kind === "ssh") {
    if (revalidated.kind !== current.kind) {
      return false;
    }
    // Must preserve the retained session: frontendSessionId and backendSessionId must match
    if (
      revalidated.frontendSessionId !== current.frontendSessionId ||
      revalidated.backendSessionId !== current.backendSessionId
    ) {
      return false;
    }

    // 1. Identity conflict strictly requires confirmed non-null matching original incarnation.
    // Legacy unconfirmed sessions (null/undefined) cannot affirm an incarnation without guessing.
    if (blockedReason === "identity_conflict") {
      if (
        current.expectedIncarnation === null ||
        current.expectedIncarnation === undefined ||
        revalidated.expectedIncarnation !== current.expectedIncarnation
      ) {
        return false; // Refused: cannot resolve identity conflict on unconfirmed legacy identity
      }
      return proof?.clearedConflict === true;
    }

    // 2. For non-identity reasons (auth_expired, control_conflict, etc.):
    // The incarnation must match the retained expectation EXACTLY.
    // If current expectation was unconfirmed (null/undefined), revalidated expectation
    // MUST ALSO remain unconfirmed (null/undefined). Adopting a newly discovered/guessed
    // incarnation under the guise of credential refresh or conflict clearance is strictly rejected.
    const currentInc = current.expectedIncarnation ?? null;
    const revalInc = revalidated.expectedIncarnation ?? null;
    if (currentInc !== revalInc) {
      return false; // Rejects guessed or swapped incarnations
    }

    if (blockedReason === "control_conflict") {
      return proof?.clearedConflict === true;
    }

    if (blockedReason === "auth_expired") {
      return proof?.credentialRefreshed === true;
    }

    return proof?.clearedConflict === true || proof?.credentialRefreshed === true;
  }

  return false;
}

export interface ExitInfo {
  readonly exitCode?: number | null;
  readonly posixSignal?: number | null;
}

export interface RecoveryReceiptProof {
  readonly token: number;
  readonly identityKey: string;
}

export type ExplicitRetryTarget = RecoveryIdentity | AuthorityCorrectionReceipt;

export type RecoveryOutcome =
  | {
      readonly phase: "ready";
      readonly token: number;
      readonly identityKey: string;
      readonly details?: unknown;
    }
  | {
      readonly phase: "retryable";
      readonly token: number;
      readonly identityKey: string;
      readonly reason: RetryableReason;
      readonly attempt: number;
      readonly nextDelayMs: number;
      readonly error?: unknown;
    }
  | {
      readonly phase: "blocked";
      readonly token: number;
      readonly identityKey: string;
      readonly reason: BlockedReason;
      readonly error?: unknown;
    }
  | {
      readonly phase: "exited";
      readonly token: number;
      readonly identityKey: string;
      readonly exitCode?: number | null;
      readonly posixSignal?: number | null;
    }
  | {
      readonly phase: "cancelled";
      readonly token: number;
      readonly identityKey: string;
    };

export function isTerminalPhase(phase: RecoveryPhase): boolean {
  return phase === "exited" || phase === "cancelled";
}

export interface RecoveryClock {
  now(): number;
  setTimeout(callback: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

export const defaultRecoveryClock: RecoveryClock = {
  now: () => Date.now(),
  setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
  clearTimeout: (h: unknown) => {
    if (typeof h === "number" || (typeof h === "object" && h !== null)) {
      globalThis.clearTimeout(h as ReturnType<typeof globalThis.setTimeout>);
    }
  },
};

/**
 * Calculates exponential backoff delay with saturation to avoid overflow.
 * Attempt count is 1-indexed. Returns 0 for attempt <= 0.
 */
export function calculateBackoffDelay(
  attempt: number,
  initialDelayMs: number,
  maxDelayMs: number,
  backoffFactor: number = 2
): number {
  if (attempt <= 0) return 0;
  const exponent = Math.min(attempt - 1, 30);
  const factor = Math.pow(backoffFactor, exponent);
  const delay = initialDelayMs * factor;
  return Math.min(delay, maxDelayMs);
}

export type RecoveryAttemptSignal = {
  readonly token: number;
  readonly attemptNumber: number;
  readonly signal: AbortSignal;
  readonly isCancelled: () => boolean;
};

export type RecoveryAttemptResult<T = unknown> =
  | { readonly kind: "ready"; readonly value?: T; readonly details?: unknown }
  | { readonly kind: "retryable"; readonly reason?: RetryableReason; readonly error?: unknown }
  | { readonly kind: "blocked"; readonly reason: BlockedReason; readonly error?: unknown }
  | { readonly kind: "exited"; readonly exitCode?: number | null; readonly posixSignal?: number | null }
  | { readonly kind: "cancelled" };

export type RecoveryAttemptAction<T = unknown> = (
  signal: RecoveryAttemptSignal
) => Promise<RecoveryAttemptResult<T>>;

export type RecoveryListener = (outcome: RecoveryOutcome) => void;

export interface RecoverySchedulerOptions<T = unknown> {
  readonly identity: RecoveryIdentity;
  readonly attemptAction?: RecoveryAttemptAction<T>;
  readonly initialDelayMs?: number;
  readonly maxDelayMs?: number;
  readonly backoffFactor?: number;
  readonly attemptDeadlineMs?: number;
  readonly clock?: RecoveryClock;
  readonly onOutcomeChange?: RecoveryListener;
}

export class RecoveryScheduler<T = unknown> {
  private identity: RecoveryIdentity;
  private identityKey: string;
  private readonly attemptAction?: RecoveryAttemptAction<T>;
  private readonly initialDelayMs: number;
  private readonly maxDelayMs: number;
  private readonly backoffFactor: number;
  private readonly attemptDeadlineMs: number;
  private readonly clock: RecoveryClock;

  private ownershipToken: number = 1;
  private attemptToken: number = 0;
  private attemptCount: number = 0;
  private currentOutcome: RecoveryOutcome;

  private inFlight: boolean = false;
  private timerHandle: unknown = null;
  private deadlineHandle: unknown = null;
  private currentAbortController: AbortController | null = null;
  private wakePending: boolean = false;
  private listeners: Set<RecoveryListener> = new Set();

  constructor(options: RecoverySchedulerOptions<T>) {
    this.identity = options.identity;
    this.identityKey = formatRecoveryKey(options.identity);
    this.attemptAction = options.attemptAction;
    this.initialDelayMs = options.initialDelayMs ?? 250;
    this.maxDelayMs = options.maxDelayMs ?? 4000;
    this.backoffFactor = options.backoffFactor ?? 2;
    this.attemptDeadlineMs = options.attemptDeadlineMs ?? 5000;
    this.clock = options.clock ?? defaultRecoveryClock;

    this.currentOutcome = {
      phase: "retryable",
      token: this.attemptToken,
      identityKey: this.identityKey,
      reason: "owner_unknown",
      attempt: 0,
      nextDelayMs: 0,
    };

    if (options.onOutcomeChange) {
      this.listeners.add(options.onOutcomeChange);
    }
  }

  public getIdentity(): RecoveryIdentity {
    return this.identity;
  }

  public getIdentityKey(): string {
    return this.identityKey;
  }

  public getPhase(): RecoveryPhase {
    return this.currentOutcome.phase;
  }

  public getOutcome(): RecoveryOutcome {
    return this.currentOutcome;
  }

  public getToken(): number {
    return this.attemptToken;
  }

  public getOwnershipToken(): number {
    return this.ownershipToken;
  }

  public getAttemptCount(): number {
    return this.attemptCount;
  }

  public isInFlight(): boolean {
    return this.inFlight;
  }

  public isTimerPending(): boolean {
    return this.timerHandle !== null;
  }

  public subscribe(listener: RecoveryListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private isSameTarget(other: RecoveryIdentity): boolean {
    if (this.identity.kind === "paired") {
      return (
        other.kind === "paired" &&
        other.proxyBackendId === this.identity.proxyBackendId &&
        other.hostId === this.identity.hostId &&
        other.remoteTarget === this.identity.remoteTarget
      );
    }
    return (
      other.kind === this.identity.kind &&
      other.frontendSessionId === this.identity.frontendSessionId &&
      other.backendSessionId === this.identity.backendSessionId &&
      (other.expectedIncarnation ?? null) === (this.identity.expectedIncarnation ?? null)
    );
  }

  private notifyListeners(outcome: RecoveryOutcome): void {
    const currentToken = this.attemptToken;
    const currentOwnership = this.ownershipToken;
    const targetSessionId = this.identity?.kind === "paired" ? undefined : this.identity?.backendSessionId;
    const attachTuple = targetSessionId ? getDurableNativeBinding(targetSessionId) : undefined;
    if (attachTuple && outcome.phase !== "ready") {
      void appendNativeRecoveryEvent({
        kind: "status",
        attachTuple,
        outcome: outcome.phase,
        producerComponent: "native-recovery-controller",
        payload: {
          outcome: outcome.phase,
          token: outcome.token,
          identityKey: outcome.identityKey,
          ...(outcome.phase === "retryable" ? { reason: outcome.reason, attempt: outcome.attempt, nextDelayMs: outcome.nextDelayMs } : {}),
          ...(outcome.phase === "blocked" ? { reason: outcome.reason } : {}),
        },
      }).catch(() => {});
    }
    for (const listener of this.listeners) {
      try {
        listener(outcome);
      } catch (err) {
        console.error("Error in recovery outcome listener:", err);
      }
      // If a synchronous listener changed ownership or attempt token, stop notifying with stale outcome
      if (this.attemptToken !== currentToken || this.ownershipToken !== currentOwnership) {
        break;
      }
    }
  }

  private clearTimer(): void {
    if (this.timerHandle !== null) {
      this.clock.clearTimeout(this.timerHandle);
      this.timerHandle = null;
    }
  }

  private clearDeadline(): void {
    if (this.deadlineHandle !== null) {
      this.clock.clearTimeout(this.deadlineHandle);
      this.deadlineHandle = null;
    }
  }

  private abortCurrentFlight(reason?: string): void {
    const controller = this.currentAbortController;
    if (controller !== null) {
      this.currentAbortController = null;
      try {
        controller.abort(new Error(reason ?? "recovery cancelled"));
      } catch {
        // Ignored
      }
    }
  }

  private validateReceiptProof(proof?: RecoveryReceiptProof): boolean {
    if (!proof) {
      return false; // Absent proof rejected!
    }
    if (proof.identityKey !== this.identityKey) {
      return false; // Key mismatch rejected!
    }
    if (proof.token !== this.attemptToken) {
      return false; // Stale token rejected!
    }
    return true;
  }

  public start(immediate: boolean = true): void {
    if (isTerminalPhase(this.currentOutcome.phase) || this.currentOutcome.phase === "blocked") {
      return;
    }
    if (this.inFlight || this.timerHandle !== null) {
      return;
    }
    if (immediate) {
      void this.executeAttempt();
    } else {
      const delay = calculateBackoffDelay(
        this.attemptCount + 1,
        this.initialDelayMs,
        this.maxDelayMs,
        this.backoffFactor
      );
      this.scheduleNextAttempt(delay);
    }
  }

  public wake(_reason?: string): void {
    if (isTerminalPhase(this.currentOutcome.phase) || this.currentOutcome.phase === "blocked") {
      return; // No wake resets authority or bypasses blocked/exited
    }
    if (this.inFlight) {
      this.wakePending = true; // Coalesced wake: accelerates after current attempt completes
      return;
    }
    if (this.timerHandle !== null) {
      this.clearTimer();
      void this.executeAttempt();
      return;
    }
    // Idle or ready: accelerate/start attempt immediately
    void this.executeAttempt();
  }

  public reportReady(details?: unknown, proof?: RecoveryReceiptProof): boolean {
    if (isTerminalPhase(this.currentOutcome.phase) || this.currentOutcome.phase === "blocked") {
      return false; // Blocked state requires explicit authority revalidation!
    }
    if (!this.validateReceiptProof(proof)) {
      return false; // Stale token or identity key receipt rejected
    }
    const nextToken = ++this.attemptToken;
    const nextOwnership = this.ownershipToken;
    const key = this.identityKey;
    this.inFlight = false;
    this.wakePending = false;
    this.attemptCount = 0; // Validated-ready-only reset!
    this.clearTimer();
    this.clearDeadline();
    const newOutcome: RecoveryOutcome = {
      phase: "ready",
      token: nextToken,
      identityKey: key,
      details,
    };
    this.currentOutcome = newOutcome;

    this.abortCurrentFlight("session ready");
    if (this.attemptToken !== nextToken || this.ownershipToken !== nextOwnership || isTerminalPhase(this.currentOutcome.phase)) {
      return true; // Reentrant transition occurred during abort
    }
    this.notifyListeners(newOutcome);
    return true;
  }

  public reportRetryable(
    reason: RetryableReason = "transport",
    error?: unknown,
    forceDelayMs?: number,
    proof?: RecoveryReceiptProof
  ): boolean {
    if (isTerminalPhase(this.currentOutcome.phase) || this.currentOutcome.phase === "blocked") {
      return false;
    }
    if (!this.validateReceiptProof(proof)) {
      return false;
    }
    const nextToken = ++this.attemptToken;
    const nextOwnership = this.ownershipToken;
    const key = this.identityKey;
    this.inFlight = false;
    this.clearTimer();
    this.clearDeadline();
    this.attemptCount++;
    const delay =
      forceDelayMs !== undefined
        ? forceDelayMs
        : this.wakePending
          ? 0
          : calculateBackoffDelay(
              this.attemptCount,
              this.initialDelayMs,
              this.maxDelayMs,
              this.backoffFactor
            );
    this.wakePending = false;
    const newOutcome: RecoveryOutcome = {
      phase: "retryable",
      token: nextToken,
      identityKey: key,
      reason,
      attempt: this.attemptCount,
      nextDelayMs: delay,
      error,
    };
    this.currentOutcome = newOutcome;
    this.scheduleNextAttempt(delay);

    this.abortCurrentFlight("retryable failure reported");
    if (this.attemptToken !== nextToken || this.ownershipToken !== nextOwnership || isTerminalPhase(this.currentOutcome.phase)) {
      return true;
    }
    this.notifyListeners(newOutcome);
    return true;
  }

  public reportBlocked(
    reason: BlockedReason,
    error?: unknown,
    proof?: RecoveryReceiptProof
  ): boolean {
    if (isTerminalPhase(this.currentOutcome.phase)) {
      return false;
    }
    if (!this.validateReceiptProof(proof)) {
      return false;
    }
    const nextToken = ++this.attemptToken;
    const nextOwnership = this.ownershipToken;
    const key = this.identityKey;
    this.inFlight = false;
    this.wakePending = false;
    this.clearTimer();
    this.clearDeadline();
    const newOutcome: RecoveryOutcome = {
      phase: "blocked",
      token: nextToken,
      identityKey: key,
      reason,
      error,
    };
    this.currentOutcome = newOutcome;

    this.abortCurrentFlight("session blocked");
    if (this.attemptToken !== nextToken || this.ownershipToken !== nextOwnership || isTerminalPhase(this.currentOutcome.phase)) {
      return true;
    }
    this.notifyListeners(newOutcome);
    return true;
  }

  public reportExit(exitInfo?: ExitInfo, proof?: RecoveryReceiptProof): boolean {
    if (isTerminalPhase(this.currentOutcome.phase) || this.currentOutcome.phase === "blocked") {
      return false; // Terminal phases are immutable; blocked ignores old exit receipts!
    }
    if (!this.validateReceiptProof(proof)) {
      return false;
    }
    const nextToken = ++this.attemptToken;
    const nextOwnership = this.ownershipToken;
    const key = this.identityKey;
    this.inFlight = false;
    this.wakePending = false;
    this.clearTimer();
    this.clearDeadline();
    const newOutcome: RecoveryOutcome = {
      phase: "exited",
      token: nextToken,
      identityKey: key,
      exitCode: exitInfo?.exitCode ?? null,
      posixSignal: exitInfo?.posixSignal ?? null,
    };
    this.currentOutcome = newOutcome;

    this.abortCurrentFlight("session exited");
    if (this.attemptToken !== nextToken || this.ownershipToken !== nextOwnership) {
      return true;
    }
    this.notifyListeners(newOutcome);
    return true;
  }

  public cancel(): boolean {
    if (isTerminalPhase(this.currentOutcome.phase)) {
      return false; // Terminal phases are immutable!
    }
    const nextToken = ++this.attemptToken;
    const nextOwnership = this.ownershipToken;
    const key = this.identityKey;
    this.inFlight = false;
    this.wakePending = false;
    this.clearTimer();
    this.clearDeadline();
    const newOutcome: RecoveryOutcome = {
      phase: "cancelled",
      token: nextToken,
      identityKey: key,
    };
    this.currentOutcome = newOutcome;

    this.abortCurrentFlight("recovery cancelled");
    if (this.attemptToken !== nextToken || this.ownershipToken !== nextOwnership) {
      return true;
    }
    this.notifyListeners(newOutcome);
    return true;
  }

  public dispose(): void {
    this.cancel();
    this.listeners.clear();
  }

  public retryExplicit(target?: ExplicitRetryTarget): boolean {
    if (isTerminalPhase(this.currentOutcome.phase)) {
      return false;
    }

    let revalidatedIdentity: RecoveryIdentity | undefined;
    let proof: AuthorityCorrectionReceipt | undefined;

    if (target !== undefined) {
      if (isAuthorityCorrectionReceipt(target)) {
        if (target.currentToken !== this.attemptToken || target.currentIdentityKey !== this.identityKey) {
          return false; // Stale receipt rejected!
        }
        revalidatedIdentity = target.revalidatedIdentity;
        proof = target;
      } else {
        revalidatedIdentity = target;
      }
    }

    // Target switching through retryExplicit is forbidden in ANY phase!
    if (revalidatedIdentity !== undefined && !this.isSameTarget(revalidatedIdentity)) {
      return false;
    }

    if (this.currentOutcome.phase === "blocked") {
      // Must supply an affirmative corrective authority change for the retained target!
      const identityToValidate = revalidatedIdentity ?? this.identity;
      if (!isAuthorityCorrection(this.identity, identityToValidate, this.currentOutcome.reason, proof)) {
        return false;
      }
      this.identity = identityToValidate;
      this.identityKey = formatRecoveryKey(identityToValidate);
    } else if (revalidatedIdentity !== undefined) {
      if (this.identity.kind === "paired" && revalidatedIdentity.kind === "paired") {
        if (revalidatedIdentity.authorityGeneration > this.identity.authorityGeneration) {
          this.identity = revalidatedIdentity;
          this.identityKey = formatRecoveryKey(revalidatedIdentity);
        }
      }
    }

    const nextOwnership = ++this.ownershipToken;
    const nextToken = ++this.attemptToken;
    this.inFlight = false;
    this.clearTimer();
    this.clearDeadline();
    // Crucial: attemptCount is NOT reset to 0 here! Cadence reset occurs strictly upon validated ready!
    this.wakePending = false;
    const newOutcome: RecoveryOutcome = {
      phase: "retryable",
      token: nextToken,
      identityKey: this.identityKey,
      reason: "owner_unknown",
      attempt: this.attemptCount,
      nextDelayMs: 0,
    };
    this.currentOutcome = newOutcome;

    this.abortCurrentFlight("explicit retry");
    if (
      this.attemptToken !== nextToken ||
      this.ownershipToken !== nextOwnership ||
      isTerminalPhase(this.currentOutcome.phase)
    ) {
      return true;
    }
    this.notifyListeners(newOutcome);
    // Crucial: check AGAIN after notifyListeners in case a listener called cancel() or rebind()!
    if (
      this.attemptToken !== nextToken ||
      this.ownershipToken !== nextOwnership ||
      isTerminalPhase(this.currentOutcome.phase)
    ) {
      return true;
    }
    void this.executeAttempt();
    return true;
  }

  public rebind(newIdentity: RecoveryIdentity): void {
    if (isTerminalPhase(this.currentOutcome.phase)) {
      return;
    }
    const nextOwnership = ++this.ownershipToken;
    const nextToken = ++this.attemptToken;
    const previousBlockedReason =
      this.currentOutcome.phase === "blocked" ? this.currentOutcome.reason : undefined;
    const isBlocked = previousBlockedReason !== undefined;

    this.identity = newIdentity;
    this.identityKey = formatRecoveryKey(newIdentity);
    this.inFlight = false;
    this.attemptCount = 0;
    this.wakePending = false;
    this.clearTimer();
    this.clearDeadline();

    let newOutcome: RecoveryOutcome;
    if (isBlocked) {
      // Calling rebind while blocked must NOT bypass blocked phase into retryable!
      newOutcome = {
        phase: "blocked",
        token: nextToken,
        identityKey: this.identityKey,
        reason: previousBlockedReason!,
      };
    } else {
      newOutcome = {
        phase: "retryable",
        token: nextToken,
        identityKey: this.identityKey,
        reason: "owner_unknown",
        attempt: 0,
        nextDelayMs: 0,
      };
    }
    this.currentOutcome = newOutcome;

    this.abortCurrentFlight("session rebound");
    if (
      this.attemptToken !== nextToken ||
      this.ownershipToken !== nextOwnership ||
      isTerminalPhase(this.currentOutcome.phase)
    ) {
      return;
    }
    this.notifyListeners(newOutcome);
  }

  private scheduleNextAttempt(delayMs: number): void {
    this.clearTimer();
    if (isTerminalPhase(this.currentOutcome.phase) || this.currentOutcome.phase === "blocked") {
      return;
    }
    // Always schedule via the clock's timer queue (even for 0ms delay) to keep transitions asynchronous
    const ms = Math.max(0, delayMs);
    this.timerHandle = this.clock.setTimeout(() => {
      this.timerHandle = null;
      void this.executeAttempt();
    }, ms);
  }

  private async executeAttempt(): Promise<void> {
    if (isTerminalPhase(this.currentOutcome.phase) || this.currentOutcome.phase === "blocked") {
      return;
    }
    if (this.inFlight) {
      return; // Single-flight invariant!
    }
    if (!this.attemptAction) {
      return;
    }

    this.clearTimer();
    const token = ++this.attemptToken;
    const ownership = this.ownershipToken;
    this.inFlight = true;
    this.wakePending = false;
    const currentAttempt = this.attemptCount + 1;

    const abortController = new AbortController();
    this.currentAbortController = abortController;

    if (this.attemptDeadlineMs > 0) {
      this.deadlineHandle = this.clock.setTimeout(() => {
        if (this.attemptToken === token && this.inFlight) {
          const deadlineToken = ++this.attemptToken;
          const deadlineOwnership = this.ownershipToken;
          this.inFlight = false;
          this.deadlineHandle = null;
          this.attemptCount++;
          const nextDelay = calculateBackoffDelay(
            this.attemptCount,
            this.initialDelayMs,
            this.maxDelayMs,
            this.backoffFactor
          );

          // 1. Commit outcome state FIRST
          const outcome: RecoveryOutcome = {
            phase: "retryable",
            token: deadlineToken,
            identityKey: this.identityKey,
            reason: "timeout",
            attempt: this.attemptCount,
            nextDelayMs: nextDelay,
            error: new Error(`Recovery attempt ${currentAttempt} exceeded deadline ${this.attemptDeadlineMs}ms`),
          };
          this.currentOutcome = outcome;

          // 2. Establish next attempt timer (asynchronous)
          this.scheduleNextAttempt(nextDelay);

          // 3. Detach and trigger abort signal
          if (this.currentAbortController === abortController) {
            this.currentAbortController = null;
          }
          try {
            abortController.abort(new Error(`deadline ${this.attemptDeadlineMs}ms exceeded`));
          } catch {
            // Ignored
          }

          // 4. If reentrant abort listener transitioned ownership/tokens, do not notify stale outcome
          if (
            this.attemptToken !== deadlineToken ||
            this.ownershipToken !== deadlineOwnership ||
            isTerminalPhase(this.currentOutcome.phase)
          ) {
            return;
          }

          // 5. Notify listeners
          this.notifyListeners(outcome);
        }
      }, this.attemptDeadlineMs);
    }

    const targetSessionId = this.identity?.kind === "paired" ? undefined : this.identity?.backendSessionId;
    const attachTuple = targetSessionId ? getDurableNativeBinding(targetSessionId) : undefined;
    try {
      const result = await this.attemptAction({
        token,
        attemptNumber: currentAttempt,
        signal: abortController.signal,
        isCancelled: () =>
          this.attemptToken !== token ||
          this.ownershipToken !== ownership ||
          isTerminalPhase(this.currentOutcome.phase),
      });

      // Token check FIRST: If cancelled, timed out, rebound, or externally transitioned, discard late response
      // WITHOUT touching deadlineHandle or scheduler state for any subsequent attempt!
      if (
        this.attemptToken !== token ||
        this.ownershipToken !== ownership ||
        isTerminalPhase(this.currentOutcome.phase)
      ) {
        return; // Stale attempt receipt discarded
      }

      this.clearDeadline();
      if (this.currentAbortController === abortController) {
        this.currentAbortController = null;
      }
      this.inFlight = false;

      switch (result.kind) {
        case "ready": {
          this.attemptCount = 0; // Validated-ready-only reset!
          const outcome: RecoveryOutcome = {
            phase: "ready",
            token: this.attemptToken,
            identityKey: this.identityKey,
            details: result.details ?? result.value,
          };
          this.currentOutcome = outcome;
          this.notifyListeners(outcome);
          break;
        }
        case "retryable": {
          this.attemptCount++;
          if (attachTuple) {
            void appendNativeRecoveryEvent({
              kind: "attempt",
              attachTuple,
              attempt: currentAttempt,
              outcome: "retryable",
              producerComponent: "native-recovery-controller",
              payload: { attempt: currentAttempt, token, outcome: "retryable" },
            }).catch(() => {});
          }
          const delay = this.wakePending
            ? 0
            : calculateBackoffDelay(
                this.attemptCount,
                this.initialDelayMs,
                this.maxDelayMs,
                this.backoffFactor
              );
          this.wakePending = false;
          const outcome: RecoveryOutcome = {
            phase: "retryable",
            token: this.attemptToken,
            identityKey: this.identityKey,
            reason: result.reason ?? "transport",
            attempt: this.attemptCount,
            nextDelayMs: delay,
            error: result.error,
          };
          this.currentOutcome = outcome;
          // Asynchronous timer scheduling!
          this.scheduleNextAttempt(delay);
          this.notifyListeners(outcome);
          break;
        }
        case "blocked": {
          this.wakePending = false;
          const outcome: RecoveryOutcome = {
            phase: "blocked",
            token: this.attemptToken,
            identityKey: this.identityKey,
            reason: result.reason,
            error: result.error,
          };
          this.currentOutcome = outcome;
          this.notifyListeners(outcome);
          break;
        }
        case "exited": {
          this.wakePending = false;
          const outcome: RecoveryOutcome = {
            phase: "exited",
            token: this.attemptToken,
            identityKey: this.identityKey,
            exitCode: result.exitCode ?? null,
            posixSignal: result.posixSignal ?? null,
          };
          this.currentOutcome = outcome;
          this.notifyListeners(outcome);
          break;
        }
        case "cancelled": {
          this.wakePending = false;
          const outcome: RecoveryOutcome = {
            phase: "cancelled",
            token: this.attemptToken,
            identityKey: this.identityKey,
          };
          this.currentOutcome = outcome;
          this.notifyListeners(outcome);
          break;
        }
      }
    } catch (err) {
      // Token check FIRST: If cancelled, timed out, or rebound, discard error
      if (
        this.attemptToken !== token ||
        this.ownershipToken !== ownership ||
        isTerminalPhase(this.currentOutcome.phase)
      ) {
        return;
      }

      this.clearDeadline();
      if (this.currentAbortController === abortController) {
        this.currentAbortController = null;
      }
      this.inFlight = false;
      this.attemptCount++;
      if (attachTuple) {
        void appendNativeRecoveryEvent({
          kind: "attempt",
          attachTuple,
          attempt: currentAttempt,
          outcome: "retryable",
          producerComponent: "native-recovery-controller",
          payload: { attempt: currentAttempt, token, outcome: "retryable" },
        }).catch(() => {});
      }
      const delay = this.wakePending
        ? 0
        : calculateBackoffDelay(
            this.attemptCount,
            this.initialDelayMs,
            this.maxDelayMs,
            this.backoffFactor
          );
      this.wakePending = false;
      const outcome: RecoveryOutcome = {
        phase: "retryable",
        token: this.attemptToken,
        identityKey: this.identityKey,
        reason: "transport",
        attempt: this.attemptCount,
        nextDelayMs: delay,
        error: err,
      };
      this.currentOutcome = outcome;
      this.scheduleNextAttempt(delay);
      this.notifyListeners(outcome);
    }
  }
}

export function createRecoveryScheduler<T = unknown>(
  options: RecoverySchedulerOptions<T>
): RecoveryScheduler<T> {
  return new RecoveryScheduler<T>(options);
}
