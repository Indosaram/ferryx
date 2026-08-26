import { switchDebug } from "./switchDebug";

type NativeTerminalLifecycleOperation<T> = () => Promise<T>;

const lifecycleTails = new Map<string, Promise<void>>();
const attachedSessionIds = new Set<string>();

type PendingDetachment = {
  readonly sessionId: string;
  readonly operation: NativeTerminalLifecycleOperation<void>;
  readonly resolve: () => void;
  readonly reject: (error: unknown) => void;
};

const pendingDetachments = new Map<string, PendingDetachment>();
const detachmentsWaitingForPresentation = new Map<string, PendingDetachment[]>();

function executeDetachment(pending: PendingDetachment): void {
  switchDebug("terminal.lifecycle.detach.execute", {
    backendSessionId: pending.sessionId,
  });
  void enqueueNativeTerminalLifecycle(pending.sessionId, pending.operation).then(
    () => {
      attachedSessionIds.delete(pending.sessionId);
      switchDebug("terminal.lifecycle.detach.executed", {
        backendSessionId: pending.sessionId,
      });
      pending.resolve();
    },
    (error: unknown) => {
      attachedSessionIds.delete(pending.sessionId);
      switchDebug("terminal.lifecycle.detach.execute.error", {
        backendSessionId: pending.sessionId,
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
      pending.resolve();
      switchDebug("terminal.lifecycle.detach.cancelled", {
        backendSessionId: sessionId,
      });
    } else {
      holdDetachmentForPresentation(pending, sessionId);
    }
  }
  if (attachedSessionIds.has(sessionId)) {
    switchDebug("terminal.lifecycle.attach.reused", {
      backendSessionId: sessionId,
    });
    return Promise.resolve(undefined as T);
  }

  attachedSessionIds.add(sessionId);
  switchDebug("terminal.lifecycle.attach.execute", {
    backendSessionId: sessionId,
  });
  return enqueueNativeTerminalLifecycle(sessionId, operation).catch((error: unknown) => {
    attachedSessionIds.delete(sessionId);
    releasePresentationWaiters(sessionId);
    switchDebug("terminal.lifecycle.attach.error", {
      backendSessionId: sessionId,
      error: String(error),
    });
    throw error;
  });
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
): Promise<void> {
  switchDebug("terminal.lifecycle.detach.requested", {
    backendSessionId: sessionId,
  });
  return new Promise((resolve, reject) => {
    const pending: PendingDetachment = {
      sessionId,
      operation,
      resolve,
      reject,
    };
    pendingDetachments.set(sessionId, pending);
    switchDebug("terminal.lifecycle.detach.queued", {
      backendSessionId: sessionId,
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
