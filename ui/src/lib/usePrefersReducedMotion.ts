import { useSyncExternalStore } from "react";

import { REDUCED_MOTION_QUERY } from "../tokens";

function reducedMotionQuery(): MediaQueryList | null {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return null;
  return window.matchMedia(REDUCED_MOTION_QUERY) ?? null;
}

/**
 * The current `prefers-reduced-motion: reduce` value, for event handlers and non-React code
 * that read it at the moment they animate. Components use `usePrefersReducedMotion`.
 */
export function prefersReducedMotion(): boolean {
  return reducedMotionQuery()?.matches === true;
}

function subscribe(onChange: () => void): () => void {
  const query = reducedMotionQuery();
  if (!query) return () => {};
  if (typeof query.addEventListener === "function") {
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }
  // Safari < 14 only implements the deprecated listener API.
  query.addListener?.(onChange);
  return () => query.removeListener?.(onChange);
}

/** Subscribes to `prefers-reduced-motion: reduce` and re-renders when the user changes it. */
export function usePrefersReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, prefersReducedMotion, () => false);
}
