// A short vibration for a confirmed tap where the platform offers one (Android browsers and
// WebViews). iOS Safari has no `navigator.vibrate`, so this is a no-op there.
export type HapticKind = "selection" | "success" | "warning";

const PATTERNS: Record<HapticKind, number | number[]> = {
  selection: 8,
  success: [10, 40, 10],
  warning: 20,
};

export function haptic(kind: HapticKind): void {
  if (typeof navigator === "undefined" || typeof navigator.vibrate !== "function") return;
  try {
    navigator.vibrate(PATTERNS[kind]);
  } catch {
    // some WebViews throw instead of ignoring the call; the tap itself already happened
  }
}
