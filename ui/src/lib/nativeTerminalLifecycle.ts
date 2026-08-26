type NativeTerminalLifecycleOperation<T> = () => Promise<T>;

const lifecycleTails = new Map<string, Promise<void>>();
const attachedSessionIds = new Set<string>();
const pendingDetachments = new Map<string, () => void>();

function startLifecycleOperation<T>(
  sessionId: string,
  operation: NativeTerminalLifecycleOperation<T>,
): Promise<T> {
  let result: Promise<T>;
  try {
    result = operation();
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
  pendingDetachments.get(sessionId)?.();
  pendingDetachments.delete(sessionId);
  if (attachedSessionIds.has(sessionId)) {
    return Promise.resolve(undefined as T);
  }

  attachedSessionIds.add(sessionId);
  return enqueueNativeTerminalLifecycle(sessionId, operation).catch((error: unknown) => {
    attachedSessionIds.delete(sessionId);
    throw error;
  });
}

export function detachNativeTerminalLifecycle(
  sessionId: string,
  operation: NativeTerminalLifecycleOperation<void>,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const cancel = () => resolve();
    pendingDetachments.set(sessionId, cancel);
    queueMicrotask(() => {
      if (pendingDetachments.get(sessionId) !== cancel) {
        return;
      }
      pendingDetachments.delete(sessionId);
      void enqueueNativeTerminalLifecycle(sessionId, operation).then(
        () => {
          attachedSessionIds.delete(sessionId);
          resolve();
        },
        (error: unknown) => {
          attachedSessionIds.delete(sessionId);
          reject(error);
        },
      );
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
