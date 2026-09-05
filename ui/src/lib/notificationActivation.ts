import { onNotificationActivated, takeNotificationActivations } from "./tauri";
import type { NotificationTarget } from "./types";

export interface NotificationActivationDeps {
  onNotificationActivated: typeof onNotificationActivated;
  takeNotificationActivations: typeof takeNotificationActivations;
}

/**
 * Wire OS notification click navigation. The listener is registered FIRST, then the queue is
 * drained once so a click that fired before registration is still recovered exactly once. The
 * activation event is only a wake signal; every drain reads the same authoritative queue.
 *
 * Drains are serialized on a single chain so overlapping wakes replay the queue strictly in
 * arrival order and never interleave. A failed drain is swallowed (it neither breaks the chain
 * nor surfaces as an unhandled rejection), and once disposed no further navigation occurs even
 * for a drain that was already in flight.
 */
export type NotificationActivationErrorSource = "drain" | "listen";

export function subscribeNotificationActivations(
  navigate: (target: NotificationTarget) => void,
  deps: NotificationActivationDeps = { onNotificationActivated, takeNotificationActivations },
  onError: (error: unknown, source: NotificationActivationErrorSource) => void = (error, source) =>
    console.error(`notification activation ${source} failed`, error),
): () => void {
  let disposed = false;
  let unlisten: (() => void) | null = null;
  let chain: Promise<void> = Promise.resolve();

  const drain = (): void => {
    chain = chain.then(async () => {
      if (disposed) return;
      let targets: NotificationTarget[];
      try {
        targets = await deps.takeNotificationActivations();
      } catch (error) {
        // A transient IPC failure must not break serialization or leak an unhandled
        // rejection, but it is reported so a broken drain is never lost silently; the next
        // wake drains the same queue again.
        onError(error, "drain");
        return;
      }
      for (const target of targets) {
        // Re-check per target: a dispose mid-drain cancels the remaining navigations.
        if (disposed) return;
        navigate(target);
      }
    });
  };

  void deps
    .onNotificationActivated(() => {
      drain();
    })
    .then((dispose) => {
      if (disposed) {
        dispose();
        return;
      }
      unlisten = dispose;
      // Recover clicks that queued before this listener existed.
      drain();
    })
    .catch((error) => {
      // Listener registration failed: nothing to drain or clean up, but surface it so the
      // feature does not fail silently.
      onError(error, "listen");
    });

  return () => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  };
}
