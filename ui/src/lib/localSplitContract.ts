import type {
  LocalSplitSpawnOptions,
  LocalSplitSpawnReceipt,
  PaneAttachTuple,
  PanePresentationReceipt,
  PreparedLocalSplit,
  SplitAttachAttempt,
  SplitErrorCode,
  SplitErrorDetails,
  SplitIdentity,
  SplitOperationRequest,
  SplitOperationResponse,
  SplitOperationResult,
} from "./types";

export const LOCAL_SPLIT_LIFECYCLE_CAPABILITY = "localSplitLifecycleV1";
export const LOCAL_SPLIT_VALIDITY_MS = 600_000;

export const ATTEMPT_TOTAL_BUDGET_MS = 15_000;
export const STAGE_CREATE_OR_STATUS_MAX_MS = 9_000;
export const STAGE_ATTACH_OR_LISTENER_MAX_MS = 4_000;
export const STAGE_PRESENTATION_MAX_MS = 2_000;
export const STAGE_CWD_PROBE_MAX_MS = 500;
export const CANCEL_ACK_MAX_MS = 3_000;
export const DAEMON_CANCEL_CLEANUP_MAX_MS = 2_500;
export const WARM_NATIVE_READY_TARGET_MS = 2_000;

export function clipStageBudget(remainingTotalMs: number, stageCapMs: number): number {
  return Math.max(0, Math.min(remainingTotalMs, stageCapMs));
}

export function hasLocalSplitCapability(capabilities?: readonly string[] | null): boolean {
  if (!capabilities || !Array.isArray(capabilities)) {
    return false;
  }
  return capabilities.includes(LOCAL_SPLIT_LIFECYCLE_CAPABILITY);
}

export function isRecoverableLegacySession(details?: { incarnation?: string | null } | null): boolean {
  return !details?.incarnation;
}

export function matchesIncarnation(
  details: { incarnation?: string | null } | null | undefined,
  expected: string,
): boolean {
  return details?.incarnation === expected;
}

export function matchesAttachTuple(a: PaneAttachTuple, b: PaneAttachTuple): boolean {
  return (
    a.backendSessionId === b.backendSessionId &&
    a.incarnation === b.incarnation &&
    a.daemonEpoch === b.daemonEpoch &&
    a.frontendSessionId === b.frontendSessionId &&
    a.paneIdentity === b.paneIdentity &&
    a.bindingKey === b.bindingKey &&
    a.attemptGeneration === b.attemptGeneration
  );
}

export interface AttemptBudget {
  readonly startTimeMs: number;
  readonly totalBudgetMs: number;
  remainingMs(nowMs?: number): number;
  stageBudget(stageCapMs: number, nowMs?: number): number;
  isExpired(nowMs?: number): boolean;
}

export function createAttemptBudget(
  startTimeMs: number = performance.now(),
  totalBudgetMs: number = ATTEMPT_TOTAL_BUDGET_MS,
): AttemptBudget {
  return {
    startTimeMs,
    totalBudgetMs,
    remainingMs(nowMs: number = performance.now()): number {
      const elapsed = Math.max(0, nowMs - startTimeMs);
      return Math.max(0, totalBudgetMs - elapsed);
    },
    stageBudget(stageCapMs: number, nowMs: number = performance.now()): number {
      return clipStageBudget(this.remainingMs(nowMs), stageCapMs);
    },
    isExpired(nowMs: number = performance.now()): boolean {
      return this.remainingMs(nowMs) <= 0;
    },
  };
}

export type {
  LocalSplitSpawnOptions,
  LocalSplitSpawnReceipt,
  PaneAttachTuple,
  PanePresentationReceipt,
  PreparedLocalSplit,
  SplitAttachAttempt,
  SplitErrorCode,
  SplitErrorDetails,
  SplitIdentity,
  SplitOperationRequest,
  SplitOperationResponse,
  SplitOperationResult,
};
