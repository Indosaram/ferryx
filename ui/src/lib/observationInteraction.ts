// Whether a human is actually at the app. Window focus alone is not observation: the window can
// hold OS focus while the user reads on a second monitor or is away from the desk, and marking a
// completion seen in that state silently swallows the notification, the sound, and the pane frame.

/** How recently the user must have touched the app for a surfaced event to count as observed. */
const OBSERVATION_INTERACTION_WINDOW_MS = 30_000;

let lastInteractionAt = 0;

/** Test seam: record an interaction at an explicit time, or reset the clock with 0. */
export function noteObservationInteraction(at: number = Date.now()): void {
  lastInteractionAt = at;
}

/** Whether a human touched the app recently enough for a surfaced event to count as observed. */
export function hasRecentObservationInteraction(now: number = Date.now()): boolean {
  return now - lastInteractionAt <= OBSERVATION_INTERACTION_WINDOW_MS;
}

if (typeof window !== "undefined") {
  const note = () => {
    lastInteractionAt = Date.now();
  };
  const types = ["keydown", "pointerdown", "wheel"];
  for (const type of types) {
    window.addEventListener(type, note, { capture: true, passive: true });
  }
  // Without this each HMR re-evaluation stacks another set of never-removed listeners.
  import.meta.hot?.dispose(() => {
    for (const type of types) window.removeEventListener(type, note, { capture: true });
  });
}
