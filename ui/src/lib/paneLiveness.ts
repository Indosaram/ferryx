import { terminalInputQueue } from "./nativeTerminalInputQueue";

export type LivenessStageVerdict =
  | "IDLE"
  | "BLOCKED_IN_QUEUE"
  | "BLOCKED_IN_IPC_WRITE"
  | "BLOCKED_IN_READER_PAUSED"
  | "BLOCKED_IN_KERNEL_STOPPED"
  | "BLOCKED_IN_ATTRIBUTED_SUSPENSION"
  | "BLOCKED_IN_PRESENTATION"
  | "UNKNOWN_APPLICATION_RESPONSE"
  | "UNKNOWN";

export interface PaneLivenessSnapshot {
  readonly telemetryAvailable: boolean;
  /**
   * Set only by `observePaneLivenessAsync`: the native snapshot IPC lost the fixed 100 ms race,
   * so every native-derived field below is absent because the deadline fired - not because the
   * native lane reported nothing. Absent (`undefined`) means the deadline never fired.
   */
  readonly nativeSnapshotDeadlineFired?: boolean;
  readonly sessionId?: string | null;
  readonly vtSessionId?: string | null;
  readonly operationId?: string | null;
  readonly stage?:
    | "accepted"
    | "dispatch"
    | "backend_write_start"
    | "backend_write_result"
    | "vt_consumed"
    | "presented"
    | null;
  readonly queuedHeadAgeMs?: number | null;
  readonly queuedCount?: number | null;
  readonly executingRunningAgeMs?: number | null;
  readonly writePendingMs?: number | null;
  readonly writeSuccess?: boolean | null;
  readonly readerPaused?: boolean | null;
  readonly kernelStopped?: boolean | null;
  readonly registrySuspended?: boolean | null;
  readonly suspended?: boolean | null;
  readonly suspensionSource?: string | null;
  readonly verifiedActuationReceipt?: boolean | null;
  readonly daemonEpoch?: string | null;
  readonly vtEpoch?: string | null;
  readonly hubStartSequence?: number | null;
  readonly hubEndSequence?: number | null;
  readonly vtConsumedSequence?: number | null;
  readonly hasUnpresentedFrames?: boolean | null;
  readonly presentationReceiptReceived?: boolean | null;
}

export function classifyPaneLiveness(snapshot: PaneLivenessSnapshot): LivenessStageVerdict {
  if (!snapshot.telemetryAvailable) {
    return "UNKNOWN";
  }

  if (snapshot.readerPaused === true) {
    return "BLOCKED_IN_READER_PAUSED";
  }

  if (snapshot.suspended === true || snapshot.kernelStopped === true) {
    if (snapshot.verifiedActuationReceipt === true && snapshot.suspensionSource === "ferryx-lifecycle") {
      return "BLOCKED_IN_ATTRIBUTED_SUSPENSION";
    }
    if (snapshot.kernelStopped === true || snapshot.suspensionSource === "external-kernel") {
      return "BLOCKED_IN_KERNEL_STOPPED";
    }
    return "UNKNOWN";
  }

  const isSlowExecution =
    (snapshot.executingRunningAgeMs !== null &&
      snapshot.executingRunningAgeMs !== undefined &&
      snapshot.executingRunningAgeMs > 250) ||
    (snapshot.writePendingMs !== null &&
      snapshot.writePendingMs !== undefined &&
      snapshot.writePendingMs > 250);

  if (isSlowExecution) {
    return "BLOCKED_IN_IPC_WRITE";
  }

  if (snapshot.hasUnpresentedFrames === true) {
    return "BLOCKED_IN_PRESENTATION";
  }

  const hubSession = snapshot.sessionId;
  const vtSession = snapshot.vtSessionId ?? snapshot.sessionId;
  if (
    snapshot.daemonEpoch !== null &&
    snapshot.daemonEpoch !== undefined &&
    snapshot.vtEpoch !== null &&
    snapshot.vtEpoch !== undefined &&
    snapshot.daemonEpoch === snapshot.vtEpoch &&
    hubSession &&
    vtSession &&
    hubSession === vtSession
  ) {
    if (
      snapshot.hubEndSequence !== null &&
      snapshot.hubEndSequence !== undefined &&
      snapshot.vtConsumedSequence !== null &&
      snapshot.vtConsumedSequence !== undefined &&
      snapshot.hubEndSequence > snapshot.vtConsumedSequence
    ) {
      return "BLOCKED_IN_PRESENTATION";
    }
  }

  if (
    snapshot.queuedHeadAgeMs !== null &&
    snapshot.queuedHeadAgeMs !== undefined &&
    snapshot.queuedHeadAgeMs > 250
  ) {
    return "BLOCKED_IN_QUEUE";
  }

  if (
    snapshot.stage === "dispatch" ||
    snapshot.stage === "backend_write_start" ||
    (snapshot.executingRunningAgeMs !== null && snapshot.executingRunningAgeMs !== undefined)
  ) {
    return "UNKNOWN";
  }

  if (snapshot.writeSuccess === true && snapshot.presentationReceiptReceived !== true) {
    if (
      snapshot.daemonEpoch !== null &&
      snapshot.daemonEpoch !== undefined &&
      snapshot.vtEpoch !== null &&
      snapshot.vtEpoch !== undefined &&
      snapshot.daemonEpoch === snapshot.vtEpoch &&
      hubSession &&
      vtSession &&
      hubSession === vtSession &&
      snapshot.hubEndSequence !== null &&
      snapshot.hubEndSequence !== undefined &&
      snapshot.vtConsumedSequence !== null &&
      snapshot.vtConsumedSequence !== undefined &&
      snapshot.hubEndSequence <= snapshot.vtConsumedSequence
    ) {
      return "BLOCKED_IN_PRESENTATION";
    }
    return "UNKNOWN";
  }

  const hasIdentity = Boolean(snapshot.sessionId && snapshot.sessionId.trim().length > 0);
  const noQueued =
    (snapshot.queuedHeadAgeMs === null || snapshot.queuedHeadAgeMs === undefined) &&
    (snapshot.queuedCount === null || snapshot.queuedCount === undefined || snapshot.queuedCount === 0);
  const noExecuting = snapshot.executingRunningAgeMs === null || snapshot.executingRunningAgeMs === undefined;
  const noWritePending = snapshot.writePendingMs === null || snapshot.writePendingMs === undefined;
  const isNotStopped = snapshot.suspended === false && snapshot.kernelStopped === false;
  const isNotPaused = snapshot.readerPaused === false;
  const isNotLagged = snapshot.hasUnpresentedFrames === false;

  if (hasIdentity && noQueued && noExecuting && noWritePending && isNotStopped && isNotPaused && isNotLagged) {
    return "IDLE";
  }

  return "UNKNOWN";
}

export function observePaneLiveness(
  sessionId: string,
  options?: {
    daemonEpoch?: string | null;
    readerPaused?: boolean | null;
    kernelStopped?: boolean | null;
    registrySuspended?: boolean | null;
    suspended?: boolean | null;
    hasUnpresentedFrames?: boolean | null;
    hubEndSequence?: number | null;
    vtConsumedSequence?: number | null;
  },
): LivenessStageVerdict {
  if (!sessionId || sessionId.trim().length === 0) {
    return "UNKNOWN";
  }

  const queuedHeadAgeMs = terminalInputQueue.getQueuedHeadAgeMs(sessionId);
  const executingRunningAgeMs = terminalInputQueue.getRunningAgeMs(sessionId);
  const inFlightId = terminalInputQueue.getInFlightRequestId(sessionId);

  const snapshot: PaneLivenessSnapshot = {
    telemetryAvailable: true,
    sessionId,
    vtSessionId: sessionId,
    queuedHeadAgeMs,
    executingRunningAgeMs,
    stage: inFlightId ? "dispatch" : null,
    daemonEpoch: options?.daemonEpoch ?? null,
    vtEpoch: options?.daemonEpoch ?? null,
    readerPaused: options?.readerPaused ?? null,
    kernelStopped: options?.kernelStopped ?? null,
    registrySuspended: options?.registrySuspended ?? null,
    suspended: options?.suspended ?? null,
    hasUnpresentedFrames: options?.hasUnpresentedFrames ?? null,
    hubEndSequence: options?.hubEndSequence ?? null,
    vtConsumedSequence: options?.vtConsumedSequence ?? null,
  };

  return classifyPaneLiveness(snapshot);
}

/**
 * One asynchronous liveness observation. `nativeSnapshotDeadlineFired` distinguishes the two
 * ways the native snapshot can be absent: a fired deadline versus a native lane that reported
 * nothing within the bound.
 */
export interface PaneLivenessObservation {
  readonly verdict: LivenessStageVerdict;
  readonly nativeSnapshotDeadlineFired: boolean;
}

export async function observePaneLivenessAsync(
  sessionId: string,
  options?: {
    daemonEpoch?: string | null;
    readerPaused?: boolean | null;
    kernelStopped?: boolean | null;
    registrySuspended?: boolean | null;
    suspended?: boolean | null;
    hubEndSequence?: number | null;
  },
): Promise<PaneLivenessObservation> {
  if (!sessionId || sessionId.trim().length === 0) {
    return { verdict: "UNKNOWN", nativeSnapshotDeadlineFired: false };
  }

  const queuedHeadAgeMs = terminalInputQueue.getQueuedHeadAgeMs(sessionId);
  const executingRunningAgeMs = terminalInputQueue.getRunningAgeMs(sessionId);
  const inFlightId = terminalInputQueue.getInFlightRequestId(sessionId);

  let nativeSnapshot: Partial<PaneLivenessSnapshot> | null = null;
  let nativeSnapshotDeadlineFired = false;
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const observed = await Promise.race([
        invoke<Partial<PaneLivenessSnapshot> | null>("cmd_native_terminal_pane_liveness", {
          sessionId,
        }).then((snapshot) => ({ snapshot })),
        new Promise<{ snapshot: null }>((resolve) =>
          setTimeout(() => resolve({ snapshot: null }), 100),
        ),
      ]);
      nativeSnapshot = observed.snapshot;
      // The race resolves null for two different reasons; record the deadline one so an artifact
      // can tell "no native telemetry" from "the IPC outran the 100 ms bound".
      nativeSnapshotDeadlineFired = observed.snapshot === null;
    } catch {
      nativeSnapshot = null;
    }
  }

  const snapshot: PaneLivenessSnapshot = {
    telemetryAvailable: true,
    nativeSnapshotDeadlineFired,
    sessionId,
    vtSessionId: nativeSnapshot?.vtSessionId ?? sessionId,
    queuedHeadAgeMs,
    queuedCount: terminalInputQueue.getQueuedCount(sessionId),
    executingRunningAgeMs,
    stage: inFlightId ? "dispatch" : null,
    daemonEpoch: options?.daemonEpoch ?? null,
    vtEpoch: nativeSnapshot?.vtEpoch ?? null,
    readerPaused: options?.readerPaused ?? null,
    kernelStopped: options?.kernelStopped ?? null,
    registrySuspended: options?.registrySuspended ?? null,
    suspended: options?.suspended ?? null,
    hasUnpresentedFrames: nativeSnapshot?.hasUnpresentedFrames ?? null,
    hubEndSequence: options?.hubEndSequence ?? null,
    vtConsumedSequence: nativeSnapshot?.vtConsumedSequence ?? null,
  };

  return {
    verdict: classifyPaneLiveness(snapshot),
    nativeSnapshotDeadlineFired,
  };
}
