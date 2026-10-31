import { useEffect } from "react";

type WakeLockSentinelLike = {
  readonly released: boolean;
  release(): Promise<void>;
  addEventListener?(type: "release", listener: () => void): void;
};

type WakeLockLike = {
  request(type: "screen"): Promise<WakeLockSentinelLike>;
};

function screenWakeLock(): WakeLockLike | undefined {
  if (typeof navigator === "undefined") return undefined;
  return (navigator as Navigator & { wakeLock?: WakeLockLike }).wakeLock;
}

/** True when the browser exposes the Screen Wake Lock API (secure contexts only). */
export function isScreenWakeLockSupported(): boolean {
  return screenWakeLock() !== undefined;
}

/**
 * Holds a screen wake lock while `enabled`. The browser drops the lock whenever the page is
 * hidden, so it is requested again each time the page becomes visible; disabling or unmounting
 * releases it. Browsers without `navigator.wakeLock` are a no-op.
 */
export function useScreenWakeLock(enabled: boolean): void {
  useEffect(() => {
    const wakeLock = screenWakeLock();
    if (!enabled || !wakeLock) return undefined;
    let active = true;
    let sentinel: WakeLockSentinelLike | null = null;
    let requesting = false;

    const acquire = () => {
      if (!active || requesting || document.visibilityState !== "visible") return;
      if (sentinel !== null && !sentinel.released) return;
      requesting = true;
      wakeLock.request("screen").then(
        (next) => {
          requesting = false;
          if (active) {
            sentinel = next;
          } else {
            void next.release().catch(() => undefined);
          }
        },
        () => {
          // Refused (battery saver, not visible, permissions policy): the next visibility change
          // retries; nothing else depends on the lock.
          requesting = false;
        },
      );
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") acquire();
    };

    acquire();
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      active = false;
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      if (sentinel !== null && !sentinel.released) void sentinel.release().catch(() => undefined);
      sentinel = null;
    };
  }, [enabled]);
}
