import { switchDebug } from "./switchDebug";

type NativeTerminalLifecycleOperation<T> = () => Promise<T>;

const lifecycleTails = new Map<string, Promise<void>>();
const attachedSessionIds = new Set<string>();
const sessionGenerations = new Map<string, number>();

type PendingDetachment = {
  readonly sessionId: string;
  readonly generation: number;
  readonly operation: NativeTerminalLifecycleOperation<void>;
  readonly resolve: (detached: boolean) => void;
  readonly reject: (error: unknown) => void;
};

const pendingDetachments = new Map<string, PendingDetachment>();
const detachmentsWaitingForPresentation = new Map<string, PendingDetachment[]>();

function bumpSessionGeneration(sessionId: string): number {
  const next = (sessionGenerations.get(sessionId) ?? 0) + 1;
  sessionGenerations.set(sessionId, next);
  return next;
}

function getSessionGeneration(sessionId: string): number {
  return sessionGenerations.get(sessionId) ?? 0;
}

function executeDetachment(pending: PendingDetachment): void {
  if (sessionGenerations.get(pending.sessionId) !== pending.generation) {
    switchDebug("terminal.lifecycle.detach.skipped.stale", {
      backendSessionId: pending.sessionId,
      scheduledGeneration: pending.generation,
      currentGeneration: sessionGenerations.get(pending.sessionId),
    });
    pending.resolve(false);
    return;
  }

  switchDebug("terminal.lifecycle.detach.execute", {
    backendSessionId: pending.sessionId,
    generation: pending.generation,
  });

  const runDetachment = async (): Promise<void> => {
    if (sessionGenerations.get(pending.sessionId) !== pending.generation) {
      switchDebug("terminal.lifecycle.detach.skipped.stale", {
        backendSessionId: pending.sessionId,
        scheduledGeneration: pending.generation,
        currentGeneration: sessionGenerations.get(pending.sessionId),
      });
      return;
    }
    await pending.operation();
  };

  void enqueueNativeTerminalLifecycle(pending.sessionId, runDetachment).then(
    () => {
      const isCurrent = sessionGenerations.get(pending.sessionId) === pending.generation;
      if (isCurrent) {
        attachedSessionIds.delete(pending.sessionId);
        switchDebug("terminal.lifecycle.detach.executed", {
          backendSessionId: pending.sessionId,
          generation: pending.generation,
        });
        pending.resolve(true);
      } else {
        pending.resolve(false);
      }
    },
    (error: unknown) => {
      const isCurrent = sessionGenerations.get(pending.sessionId) === pending.generation;
      if (isCurrent) {
        attachedSessionIds.delete(pending.sessionId);
      }
      switchDebug("terminal.lifecycle.detach.execute.error", {
        backendSessionId: pending.sessionId,
        generation: pending.generation,
        error: String(error),
      });
      pending.reject(error);
    },
  );
}

function releasePresentationWaiters(sessionId: string): void {
  const pending = detachmentsWaitingForPresentation.get(sessionId);
  if (!pending) return;
  switchDebug("terminal.lifecycle.presentation.release", {
    backendSessionId: sessionId,
    detachmentCount: pending.length,
    detachedSessionIds: pending.map((detachment) => detachment.sessionId),
  });
  detachmentsWaitingForPresentation.delete(sessionId);
  for (const detachment of pending) executeDetachment(detachment);
}

/**
 * Cancels a held detachment for a session that is attaching again.
 *
 * A detachment parked under another session's presentation is no longer in
 * `pendingDetachments`, so the cancellation scan in `attachNativeTerminalLifecycle`
 * cannot see it. Without this, re-attaching that same surface would still let the
 * parked teardown fire once the other session presents.
 */
function cancelHeldDetachmentsFor(sessionId: string): boolean {
  let cancelled = false;
  for (const [replacementSessionId, waiting] of detachmentsWaitingForPresentation) {
    const remaining = waiting.filter((detachment) => {
      if (detachment.sessionId !== sessionId) return true;
      detachment.resolve(false);
      cancelled = true;
      return false;
    });
    if (remaining.length === waiting.length) continue;
    if (remaining.length === 0) {
      detachmentsWaitingForPresentation.delete(replacementSessionId);
    } else {
      detachmentsWaitingForPresentation.set(replacementSessionId, remaining);
    }
  }
  return cancelled;
}

function holdDetachmentForPresentation(
  pending: PendingDetachment,
  replacementSessionId: string,
): void {
  pendingDetachments.delete(pending.sessionId);
  const inherited = detachmentsWaitingForPresentation.get(pending.sessionId) ?? [];
  if (inherited.length > 0) {
    detachmentsWaitingForPresentation.delete(pending.sessionId);
  }
  const waiting = detachmentsWaitingForPresentation.get(replacementSessionId) ?? [];
  detachmentsWaitingForPresentation.set(
    replacementSessionId,
    [...waiting, ...inherited, pending],
  );
  switchDebug("terminal.lifecycle.detach.held", {
    outgoingBackendSessionId: pending.sessionId,
    replacementBackendSessionId: replacementSessionId,
    inheritedCount: inherited.length,
  });
}

function startLifecycleOperation<T>(
  sessionId: string,
  operation: NativeTerminalLifecycleOperation<T>,
): Promise<T> {
  let result: Promise<T>;
  try {
    result = Promise.resolve(operation());
  } catch (error) {
    result = Promise.reject(error);
  }

  const tail = result.then(
    () => undefined,
    () => undefined,
  );
  lifecycleTails.set(sessionId, tail);
  void tail.finally(() => {
    if (lifecycleTails.get(sessionId) === tail) {
      lifecycleTails.delete(sessionId);
    }
  });
  return result;
}

/**
 * Serializes lifecycle mutations for each desktop native compositor surface.
 *
 * React StrictMode intentionally replays effects (setup -> cleanup -> setup), and a
 * tab switch can clean up one backend session while mounting another. Tauri invokes
 * are asynchronous, so issuing attach/detach directly lets an older cleanup overtake
 * a newer attach and tear down that session's daemon output pump. A queue per backend
 * session preserves lifecycle order without blocking independent split-pane surfaces.
 *
 * When the compositor is idle the operation is started synchronously, preserving the
 * pre-existing React/IPC boundary. Only operations that overlap an in-flight lifecycle
 * mutation are deferred.
 */
export function attachNativeTerminalLifecycle<T>(
  sessionId: string,
  operation: NativeTerminalLifecycleOperation<T>,
): Promise<T> {
  switchDebug("terminal.lifecycle.attach.requested", {
    backendSessionId: sessionId,
    pendingDetachmentCount: pendingDetachments.size,
    alreadyAttached: attachedSessionIds.has(sessionId),
  });
  for (const pending of pendingDetachments.values()) {
    if (pending.sessionId === sessionId) {
      pendingDetachments.delete(sessionId);
      pending.resolve(false);
      switchDebug("terminal.lifecycle.detach.cancelled", {
        backendSessionId: sessionId,
      });
    } else if (!attachedSessionIds.has(sessionId)) {
      // Only an attach that actually takes over the compositor can stand in for
      // an outgoing surface, so its detachment waits for this one to present.
      // A re-attach of a surface that is still attached (React replaying an
      // effect, a sibling pane re-mounting) replaces nothing: parking an
      // unrelated pane's detachment under it would tear that live pane down as
      // soon as this one presents, leaving a blank split pane.
      holdDetachmentForPresentation(pending, sessionId);
    }
  }
  if (cancelHeldDetachmentsFor(sessionId)) {
    switchDebug("terminal.lifecycle.detach.held.cancelled", {
      backendSessionId: sessionId,
    });
  }
  if (attachedSessionIds.has(sessionId)) {
    const generation = bumpSessionGeneration(sessionId);
    switchDebug("terminal.lifecycle.attach.reused", {
      backendSessionId: sessionId,
      generation,
    });
    return Promise.resolve(undefined as T);
  }

  const generation = bumpSessionGeneration(sessionId);
  attachedSessionIds.add(sessionId);
  switchDebug("terminal.lifecycle.attach.execute", {
    backendSessionId: sessionId,
    generation,
  });
  return enqueueNativeTerminalLifecycle(sessionId, operation).catch((error: unknown) => {
    attachedSessionIds.delete(sessionId);
    releasePresentationWaiters(sessionId);
    switchDebug("terminal.lifecycle.attach.error", {
      backendSessionId: sessionId,
      generation,
      error: String(error),
    });
    throw error;
  });
}

/**
 * Re-attaches a session by forcing a fresh attachment even if TS state
 * previously considered it attached.
 */
export function reattachNativeTerminalLifecycle<T>(
  sessionId: string,
  operation: NativeTerminalLifecycleOperation<T>,
): Promise<T> {
  attachedSessionIds.delete(sessionId);
  return attachNativeTerminalLifecycle(sessionId, operation);
}

/**
 * Marks a replacement surface as rendered at its final bounds.
 *
 * React cleans up the outgoing pane before setting up its replacement. Keeping
 * that outgoing native surface alive until this signal prevents a blank
 * compositor frame without using timing delays. A real unmount with no
 * replacement still detaches in the next microtask.
 */
export function presentNativeTerminalLifecycle(sessionId: string): void {
  switchDebug("terminal.lifecycle.present", {
    backendSessionId: sessionId,
  });
  releasePresentationWaiters(sessionId);
}

export function detachNativeTerminalLifecycle(
  sessionId: string,
  operation: NativeTerminalLifecycleOperation<void>,
): Promise<boolean> {
  const generation = getSessionGeneration(sessionId);
  switchDebug("terminal.lifecycle.detach.requested", {
    backendSessionId: sessionId,
    generation,
  });
  return new Promise<boolean>((resolve, reject) => {
    const pending: PendingDetachment = {
      sessionId,
      generation,
      operation,
      resolve,
      reject,
    };
    pendingDetachments.set(sessionId, pending);
    switchDebug("terminal.lifecycle.detach.queued", {
      backendSessionId: sessionId,
      generation,
      pendingDetachmentCount: pendingDetachments.size,
    });
    queueMicrotask(() => {
      if (pendingDetachments.get(sessionId) !== pending) return;
      pendingDetachments.delete(sessionId);
      releasePresentationWaiters(sessionId);
      executeDetachment(pending);
    });
  });
}

function enqueueNativeTerminalLifecycle<T>(
  sessionId: string,
  operation: NativeTerminalLifecycleOperation<T>,
): Promise<T> {
  const previous = lifecycleTails.get(sessionId);
  if (!previous) {
    return startLifecycleOperation(sessionId, operation);
  }

  const result = previous.then(operation);
  const tail = result.then(
    () => undefined,
    () => undefined,
  );
  lifecycleTails.set(sessionId, tail);
  void tail.finally(() => {
    if (lifecycleTails.get(sessionId) === tail) {
      lifecycleTails.delete(sessionId);
    }
  });
  return result;
}

export function resetNativeTerminalLifecycleForTest(): void {
  lifecycleTails.clear();
  attachedSessionIds.clear();
  sessionGenerations.clear();
  pendingDetachments.clear();
  detachmentsWaitingForPresentation.clear();
}
