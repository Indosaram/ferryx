import { getTerminalRemoteStatus, isStructuredIpcError, listTerminalSessions, onTerminalRemoteStatus, retryTerminalRemoteSession } from "./tauri";
import type { RemoteSessionStatusResponse, SshRecoveryStatus, TerminalRemoteStatus, TerminalSession } from "./types";
import type { WorkspaceAction } from "../state/workspaceStore";
import {
  RecoveryScheduler,
  type RecoveryClock,
  defaultRecoveryClock,
  type LocalOrSshRecoveryIdentity,
  type RecoveryReceiptProof,
  type RecoveryAttemptResult,
  type RecoveryOutcome,
} from "./sessionRecovery";

/** Subscribe first; status snapshots may never overwrite a newer stream observation. */
export function startSshRecovery(options: {
  sessions: TerminalSession[];
  dispatch: (action: WorkspaceAction) => void;
  onError: (error: unknown) => void;
  subscribe?: typeof onTerminalRemoteStatus;
  status?: (id: string) => Promise<RemoteSessionStatusResponse>;
  retry?: (id: string) => Promise<unknown>;
  list?: () => Promise<Array<{ sessionId: string; daemonEpoch?: string | null }>>;
  rearms?: Map<string, number>;
  clock?: RecoveryClock;
  initialDelayMs?: number;
  maxDelayMs?: number;
  backoffFactor?: number;
  attemptDeadlineMs?: number;
  onOutcome?: (outcome: RecoveryOutcome & { readonly backendSessionId?: string }) => void;
  /** Observes completion of the actual session attempt, including cancelled continuations. */
  onAttemptSettled?: (attempt: RecoveryReceiptProof & {
    readonly backendSessionId: string;
    readonly attemptNumber: number;
  }) => void;
}) {
  let stopped = false;
  let unlisten: (() => void) | undefined;
  const latest = new Map<string, SshRecoveryStatus>();
  const latestVersions = new Map<string, number>();
  const maxObservedGenerations = new Map<string, number>();
  const terminalFences = new Set<string>();

  const setLatest = (sessionId: string, status: SshRecoveryStatus) => {
    latest.set(sessionId, status);
    latestVersions.set(sessionId, (latestVersions.get(sessionId) ?? 0) + 1);
    maxObservedGenerations.set(
      sessionId,
      Math.max(maxObservedGenerations.get(sessionId) ?? 0, status.generation)
    );
  };

  const sessionMap = new Map<string, TerminalSession>();
  for (const s of options.sessions) {
    if (s.backendSessionId) {
      sessionMap.set(s.backendSessionId, s);
    }
  }
  const ids = new Set(sessionMap.keys());

  const list = coalesceListCalls(options.list ?? listTerminalSessions);
  const probe = options.status ?? getTerminalRemoteStatus;
  const retry = options.retry ?? retryTerminalRemoteSession;
  const clock = options.clock ?? defaultRecoveryClock;
  const subscribeFn = options.subscribe ?? onTerminalRemoteStatus;

  const toStatus = (sessionId: string, result: RemoteSessionStatusResponse): SshRecoveryStatus =>
    result.details
      ? { sessionId, state: result.details.state, generation: result.details.generation, failure: result.details.failure, replayGap: result.details.replayGap }
      : { sessionId, state: result.legacyDirectSsh ? "legacyLost" : "missing", generation: 0, failure: null, replayGap: null };

  const rearms = options.rearms ?? new Map<string, number>();
  const pendingMutationGenerations = new Map<string, number>();

  const rearm = async (sessionId: string, generation?: number): Promise<{ ok: boolean; blockedReason?: "auth_revoked" | "protocol_error" | "identity_conflict" }> => {
    if (stopped || terminalFences.has(sessionId)) return { ok: false };
    const used = rearms.get(sessionId) ?? 0;
    rearms.set(sessionId, used + 1);
    if (generation !== undefined) {
      pendingMutationGenerations.set(sessionId, generation);
    }
    try {
      await retry(sessionId);
      return { ok: true };
    } catch (error) {
      if (isStructuredIpcError(error)) {
        const detailsKind = typeof error.details?.kind === "string" ? error.details.kind : "";
        if (detailsKind === "authentication" || detailsKind === "hostKey" || error.code === "AUTHENTICATION_FAILED") {
          return { ok: false, blockedReason: "auth_revoked" };
        }
        if (detailsKind === "protocol" || error.code === "PROTOCOL_ERROR") {
          return { ok: false, blockedReason: "protocol_error" };
        }
        if (detailsKind === "staleGeneration") {
          return { ok: false, blockedReason: "identity_conflict" };
        }
        return { ok: false };
      }
      throw error;
    } finally {
      if (generation !== undefined && pendingMutationGenerations.get(sessionId) === generation) {
        pendingMutationGenerations.delete(sessionId);
      }
    }
  };

  const apply = (status: SshRecoveryStatus, daemonEpoch?: string | null) => {
    if (stopped) return;
    if (status.state === "connected") {
      rearms.delete(status.sessionId);
    } else if (status.state === "expired" || status.state === "legacyLost") {
      terminalFences.add(status.sessionId);
    }
    options.dispatch({ type: "SESSION_REMOTE_STATUS", status, daemonEpoch });
  };

  const reconcile = async (
    status: SshRecoveryStatus,
    isCancelled?: () => boolean,
    capturedLatest?: SshRecoveryStatus
  ) => {
    try {
      const sessions = await list();
      if (stopped || (isCancelled && isCancelled())) return;
      if (capturedLatest !== undefined && latest.get(status.sessionId) !== capturedLatest) return;
      if (latest.get(status.sessionId) === status) {
        apply(status, sessions.find(s => s.sessionId === status.sessionId)?.daemonEpoch);
      }
    } catch (error) {
      if (!stopped && (!isCancelled || !isCancelled())) options.onError(error);
    }
  };

  const schedulers = new Map<string, RecoveryScheduler<RemoteSessionStatusResponse>>();

  const getOrCreateScheduler = (backendId: string): RecoveryScheduler<RemoteSessionStatusResponse> => {
    let existing = schedulers.get(backendId);
    if (existing && !existing.getPhase().startsWith("cancel") && existing.getPhase() !== "exited") {
      return existing;
    }

    const sessionObj = sessionMap.get(backendId);
    const identity: LocalOrSshRecoveryIdentity = {
      kind: "ssh",
      frontendSessionId: sessionObj?.id ?? backendId,
      backendSessionId: backendId,
      expectedIncarnation: sessionObj?.incarnation ?? null,
    };

    const scheduler = new RecoveryScheduler<RemoteSessionStatusResponse>({
      identity,
      clock,
      initialDelayMs: options.initialDelayMs ?? 250,
      maxDelayMs: options.maxDelayMs ?? 4000,
      backoffFactor: options.backoffFactor ?? 2,
      attemptDeadlineMs: options.attemptDeadlineMs ?? 5000,
      attemptAction: async (attemptSignal): Promise<RecoveryAttemptResult<RemoteSessionStatusResponse>> => {
        const identityKey = scheduler.getIdentityKey();
        try {
          if (stopped || attemptSignal.isCancelled()) {
            return { kind: "cancelled" };
          }
          if (terminalFences.has(backendId)) {
            return { kind: "exited" };
          }

          const observationBefore = latest.get(backendId);
          if (observationBefore && (observationBefore.state === "expired" || observationBefore.state === "legacyLost")) {
            terminalFences.add(backendId);
            return { kind: "exited" };
          }

          // Check if prior observation is already blocked; never dial on blocked failure
          const failureKindBefore = observationBefore?.failure?.kind;
          if (observationBefore?.state === "disconnected") {
            if (failureKindBefore === "authentication" || failureKindBefore === "hostKey") {
              return { kind: "blocked", reason: "auth_revoked", error: observationBefore.failure };
            }
            if (failureKindBefore === "protocol") {
              return { kind: "blocked", reason: "protocol_error", error: observationBefore.failure };
            }
            if (failureKindBefore === "staleGeneration") {
              return { kind: "blocked", reason: "identity_conflict", error: observationBefore.failure };
            }
          }

          if (stopped || attemptSignal.isCancelled() || terminalFences.has(backendId)) {
            return { kind: "cancelled" };
          }

          // Initial PROBE first: query actual daemon state
          let result = await probe(backendId);
          if (stopped || attemptSignal.isCancelled() || terminalFences.has(backendId)) {
            return { kind: "cancelled" };
          }

          // EXACT FRESHNESS GUARD IMMEDIATELY AFTER FIRST PROBE:
          // A snapshot probe may NEVER overwrite or act upon intermediate stream events!
          const observationAfterFirstProbe = latest.get(backendId);
          if (terminalFences.has(backendId) || observationAfterFirstProbe !== observationBefore) {
            // Intermediate event arrived while probe was pending: probe is stale. Discard immediately without rearm!
            return { kind: "retryable", reason: "transport" };
          }

          let probedStatus = toStatus(backendId, result);

          // If a rearm mutation for this exact generation is still pending from an earlier attempt,
          // do not dial again or await uncertain mutation; return retryable boundedly.
          if (
            probedStatus.state === "disconnected" &&
            pendingMutationGenerations.get(backendId) === probedStatus.generation
          ) {
            return { kind: "retryable", reason: "transport" };
          }

          // Classify probed failures BEFORE any rearm decision
          if (probedStatus.state === "expired" || probedStatus.state === "legacyLost") {
            terminalFences.add(backendId);
            setLatest(backendId, probedStatus);
            apply(probedStatus);
            await reconcile(probedStatus, () => attemptSignal.isCancelled(), probedStatus);
            return { kind: "exited" };
          }

          if (result.details?.failure) {
            const fKind = result.details.failure.kind;
            if (fKind === "authentication" || fKind === "hostKey") {
              setLatest(backendId, probedStatus);
              apply(probedStatus);
              return { kind: "blocked", reason: "auth_revoked", error: result.details.failure };
            }
            if (fKind === "protocol") {
              setLatest(backendId, probedStatus);
              apply(probedStatus);
              return { kind: "blocked", reason: "protocol_error", error: result.details.failure };
            }
            if (fKind === "staleGeneration") {
              setLatest(backendId, probedStatus);
              apply(probedStatus);
              return { kind: "blocked", reason: "identity_conflict", error: result.details.failure };
            }
          }

          // If first probe shows disconnected or unknown to this daemon without details, rearm within same bounded attempt
          const needsRearm =
            (!result.details && !result.legacyDirectSsh) ||
            probedStatus.state === "disconnected";

          if (needsRearm) {
            const rearmRes = await rearm(backendId, probedStatus.generation);
            if (!rearmRes.ok && rearmRes.blockedReason) {
              return { kind: "blocked", reason: rearmRes.blockedReason };
            }
            if (stopped || attemptSignal.isCancelled() || terminalFences.has(backendId)) {
              return { kind: "cancelled" };
            }

            // Freshness check after rearm before second probe:
            const observationAfterRearm = latest.get(backendId);
            if (terminalFences.has(backendId) || observationAfterRearm !== observationBefore) {
              return { kind: "retryable", reason: "transport" };
            }

            result = await probe(backendId);
            if (stopped || attemptSignal.isCancelled() || terminalFences.has(backendId)) {
              return { kind: "cancelled" };
            }

            // Freshness check after second probe:
            const observationAfterSecondProbe = latest.get(backendId);
            if (terminalFences.has(backendId) || observationAfterSecondProbe !== observationBefore) {
              return { kind: "retryable", reason: "transport" };
            }

            probedStatus = toStatus(backendId, result);
          }

          // All freshness checks passed; apply probed status safely
          setLatest(backendId, probedStatus);
          apply(probedStatus);
          await reconcile(probedStatus, () => attemptSignal.isCancelled(), probedStatus);

          if (stopped || attemptSignal.isCancelled()) {
            return { kind: "cancelled" };
          }

          if (probedStatus.state === "connected") {
            return { kind: "ready", value: result };
          }

          if (probedStatus.state === "expired" || probedStatus.state === "legacyLost") {
            terminalFences.add(backendId);
            return { kind: "exited" };
          }

          if (result.details?.failure) {
            const fKind = result.details.failure.kind;
            if (fKind === "authentication" || fKind === "hostKey") {
              return { kind: "blocked", reason: "auth_revoked", error: result.details.failure };
            }
            if (fKind === "protocol") {
              return { kind: "blocked", reason: "protocol_error", error: result.details.failure };
            }
            if (fKind === "staleGeneration") {
              return { kind: "blocked", reason: "identity_conflict", error: result.details.failure };
            }
          }

          if (probedStatus.state === "reconnecting") {
            return { kind: "retryable", reason: "transport" };
          }

          return { kind: "retryable", reason: probedStatus.state === "missing" ? "owner_unknown" : "transport" };
        } catch (error) {
          if (!stopped && !attemptSignal.isCancelled()) {
            options.onError(error);
          }
          return { kind: "retryable", reason: "transport", error };
        } finally {
          options.onAttemptSettled?.({
            backendSessionId: backendId,
            token: attemptSignal.token,
            identityKey,
            attemptNumber: attemptSignal.attemptNumber,
          });
        }
      },
    });

    if (options.onOutcome) {
      scheduler.subscribe((outcome) => {
        options.onOutcome?.({ ...outcome, backendSessionId: backendId });
      });
    }

    schedulers.set(backendId, scheduler);
    return scheduler;
  };

  const handleStreamEvent = async (status: TerminalRemoteStatus) => {
    if (stopped || !ids.has(status.sessionId) || terminalFences.has(status.sessionId)) return;

    // 1. Monotonic authority fence: reject stale generation against max observed probe or stream BEFORE publication
    const maxObserved = maxObservedGenerations.get(status.sessionId);
    if (maxObserved !== undefined && status.generation < maxObserved) {
      return;
    }
    maxObservedGenerations.set(status.sessionId, Math.max(maxObserved ?? 0, status.generation));

    const previous = latest.get(status.sessionId)?.state;
    const scheduler = getOrCreateScheduler(status.sessionId);

    if (status.state === "connected") {
      setLatest(status.sessionId, status);
      apply(status);
      void reconcile(status, undefined, status);

      const proof: RecoveryReceiptProof = {
        token: scheduler.getToken(),
        identityKey: scheduler.getIdentityKey(),
      };
      scheduler.reportReady(status, proof);
      return;
    }

    if (status.state === "reconnecting") {
      setLatest(status.sessionId, status);
      apply(status);
      void reconcile(status, undefined, status);

      if (scheduler.getPhase() === "ready") {
        scheduler.start(false);
      }
      return;
    }

    if (status.state === "expired") {
      // Stream contract: TerminalRemoteStatus is produced by src-tauri/src/terminal/remote.rs (emit_remote_status)
      // on target EOF/exit. Because TerminalRemoteStatus lacks an incarnation field, we must not mint a local scheduler
      // token as liveness proof alone. Validate against fresh matching describe owner authority before terminal fence:
      const capturedIdentityKey = scheduler.getIdentityKey();
      const capturedLatest = latest.get(status.sessionId);
      const capturedVersion = latestVersions.get(status.sessionId) ?? 0;
      const capturedEventGen = status.generation;

      let probeRes: RemoteSessionStatusResponse;
      try {
        probeRes = await probe(status.sessionId);
      } catch {
        // Ambiguous probe failure: maintain pending retry, do not fence
        if (!stopped && ids.has(status.sessionId) && !terminalFences.has(status.sessionId)) {
          scheduler.wake();
        }
        return;
      }

      // Check unchanged current owner and no newer event after await (incl same-gen connected, rebind, or close):
      if (
        stopped ||
        !ids.has(status.sessionId) ||
        terminalFences.has(status.sessionId) ||
        schedulers.get(status.sessionId) !== scheduler ||
        scheduler.getIdentityKey() !== capturedIdentityKey ||
        latest.get(status.sessionId) !== capturedLatest ||
        (latestVersions.get(status.sessionId) ?? 0) !== capturedVersion
      ) {
        return; // Intervening event (e.g. newer connected, rebind, close) invalidated this held probe
      }

      const probed = toStatus(status.sessionId, probeRes);

      // Match probed authority identity + generation, not state alone:
      // For remote details, descriptor backendSessionId must match and generation must match the expired event.
      const authorityMatches =
        probed.sessionId === status.sessionId &&
        (probeRes.details
          ? probeRes.details.descriptor.backendSessionId === status.sessionId &&
            probeRes.details.generation === capturedEventGen
          : probeRes.legacyDirectSsh === true);

      // Verify that describe authority confirms affirmative exit for this exact identity and generation
      if (authorityMatches && (probed.state === "expired" || probed.state === "legacyLost")) {
        terminalFences.add(status.sessionId);
        setLatest(status.sessionId, probed);
        apply(probed);
        await reconcile(probed, undefined, probed);
        const proof: RecoveryReceiptProof = {
          token: scheduler.getToken(),
          identityKey: capturedIdentityKey,
        };
        scheduler.reportExit(undefined, proof);
      } else {
        // Target is not confirmed exited (e.g. newer connected, different generation, or ambiguous):
        // maintain pending retry on ambiguity; do not fence.
        if (probed.generation >= capturedEventGen) {
          setLatest(status.sessionId, probed);
          apply(probed);
          void reconcile(probed, undefined, probed);
          if (probed.state === "connected") {
            const proof: RecoveryReceiptProof = {
              token: scheduler.getToken(),
              identityKey: capturedIdentityKey,
            };
            scheduler.reportReady(probed, proof);
          } else if (probed.state === "disconnected") {
            scheduler.wake();
          }
        } else {
          scheduler.wake();
        }
      }
      return;
    }

    if (status.state === "disconnected") {
      setLatest(status.sessionId, status);
      apply(status);
      void reconcile(status, undefined, status);

      if (previous !== "disconnected") {
        scheduler.wake();
      }
      return;
    }
  };

  const subscriptionIdentity: LocalOrSshRecoveryIdentity = {
    kind: "ssh",
    frontendSessionId: "stream_subscription",
    backendSessionId: "stream_subscription",
    expectedIncarnation: null,
  };

  const subscriptionScheduler = new RecoveryScheduler<() => void>({
    identity: subscriptionIdentity,
    clock,
    initialDelayMs: options.initialDelayMs ?? 250,
    maxDelayMs: options.maxDelayMs ?? 4000,
    backoffFactor: options.backoffFactor ?? 2,
    attemptDeadlineMs: options.attemptDeadlineMs ?? 5000,
    attemptAction: async (attemptSignal): Promise<RecoveryAttemptResult<() => void>> => {
      if (stopped || attemptSignal.isCancelled()) {
        return { kind: "cancelled" };
      }
      try {
        const dispose = await subscribeFn(handleStreamEvent);
        if (stopped || attemptSignal.isCancelled()) {
          dispose();
          return { kind: "cancelled" };
        }
        unlisten = dispose;
        return { kind: "ready", value: dispose };
      } catch (error) {
        if (!stopped && !attemptSignal.isCancelled()) {
          options.onError(error);
        }
        return { kind: "retryable", reason: "transport", error };
      }
    },
  });

  if (options.onOutcome) {
    subscriptionScheduler.subscribe((outcome) => {
      options.onOutcome?.({ ...outcome, backendSessionId: "stream_subscription" });
    });
  }

  const subscribed = new Promise<void>((resolve) => {
    if (subscriptionScheduler.getPhase() === "ready") {
      resolve();
      return;
    }
    const unsub = subscriptionScheduler.subscribe((outcome) => {
      if (outcome.phase === "ready") {
        unsub();
        resolve();
      }
    });
    subscriptionScheduler.start(true);
  });

  const ready = subscribed.then(async () => {
    if (stopped) return;
    await Promise.all([...ids].map(async sessionId => {
      if (stopped || terminalFences.has(sessionId)) return;
      const scheduler = getOrCreateScheduler(sessionId);
      await new Promise<void>((resolve) => {
        let settled = false;
        const finish = () => {
          if (!settled) {
            settled = true;
            unsub();
            resolve();
          }
        };
        const unsub = scheduler.subscribe((outcome) => {
          if (outcome.phase === "ready" || outcome.phase === "exited" || outcome.phase === "blocked" || outcome.phase === "retryable") {
            finish();
          }
        });
        scheduler.start(true);
        if (!scheduler.isInFlight() && !scheduler.isTimerPending()) {
          finish();
        }
      });
    }));
  });

  return {
    subscribed,
    ready,
    closeSession: (backendId: string) => {
      ids.delete(backendId);
      const scheduler = schedulers.get(backendId);
      if (scheduler) {
        scheduler.dispose();
        schedulers.delete(backendId);
      }
      latest.delete(backendId);
      maxObservedGenerations.delete(backendId);
      latestVersions.set(backendId, (latestVersions.get(backendId) ?? 0) + 1);
      terminalFences.add(backendId);
    },
    stop: () => {
      stopped = true;
      unlisten?.();
      unlisten = undefined;
      subscriptionScheduler.dispose();
      for (const scheduler of schedulers.values()) {
        scheduler.dispose();
      }
      schedulers.clear();
    },
  };
}

/**
 * One list call in flight at a time. Callers that arrive while it runs share a single follow-up
 * call, so each still sees a result issued after it asked, but a burst of remote status events
 * cannot queue one list per event behind the shared daemon connection.
 */
export function coalesceListCalls<T>(list: () => Promise<T>): () => Promise<T> {
  let inFlight: Promise<T> | null = null;
  let queued: Promise<T> | null = null;
  const run = (): Promise<T> => {
    const current = list().finally(() => {
      if (inFlight === current) inFlight = null;
    });
    inFlight = current;
    return current;
  };
  return () => {
    if (!inFlight) return run();
    if (!queued) {
      const next = inFlight.catch(() => undefined).then(() => {
        queued = null;
        return run();
      });
      queued = next;
    }
    return queued;
  };
}
