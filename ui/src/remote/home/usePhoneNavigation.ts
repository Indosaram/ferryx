// Phone navigation for the supervisor-first home: three home destinations behind a bottom tab
// bar, and a Session screen pushed on top. A tab switch replaces the history entry (as platform
// tab bars do); a Session screen and every overlay push one, so the back gesture closes the
// topmost overlay first, then returns from the session, then leaves.
import { useCallback, useEffect, useRef, useState } from "react";

export type PhoneDestination = "supervisor" | "inbox" | "worktrees";

export type PhoneScreen =
  | { readonly kind: "home"; readonly destination: PhoneDestination }
  | { readonly kind: "session"; readonly from: PhoneDestination };

/** The key this hook owns inside `history.state`; other keys there are left untouched. */
export const PHONE_HISTORY_KEY = "ferryxPhone";

const HOME_DEFAULT: PhoneScreen = { kind: "home", destination: "supervisor" };

function isDestination(value: unknown): value is PhoneDestination {
  return value === "supervisor" || value === "inbox" || value === "worktrees";
}

function screenFromState(state: unknown): PhoneScreen | null {
  if (!state || typeof state !== "object") return null;
  const raw = (state as Record<string, unknown>)[PHONE_HISTORY_KEY];
  if (!raw || typeof raw !== "object") return null;
  const entry = raw as Record<string, unknown>;
  if (entry.kind === "home" && isDestination(entry.destination)) return { kind: "home", destination: entry.destination };
  if (entry.kind === "session" && isDestination(entry.from)) return { kind: "session", from: entry.from };
  return null;
}

function writeHistory(screen: PhoneScreen, mode: "push" | "replace"): void {
  const base = window.history.state && typeof window.history.state === "object" ? window.history.state : {};
  const next = { ...base, [PHONE_HISTORY_KEY]: screen };
  if (mode === "push") window.history.pushState(next, "");
  else window.history.replaceState(next, "");
}

export type PhoneNavigation = {
  readonly screen: PhoneScreen;
  /** The destination the tab bar shows as selected (the one a Session screen returns to). */
  readonly destination: PhoneDestination;
  readonly selectTab: (destination: PhoneDestination) => void;
  readonly openSession: () => void;
  readonly back: () => void;
  readonly reset: () => void;
};

/**
 * `enabled` is false on wide layouts and before sign-in: then the hook neither reads nor
 * writes history, and the screen stays at the Supervisor home default.
 */
export function usePhoneNavigation(enabled: boolean): PhoneNavigation {
  const [screen, setScreen] = useState<PhoneScreen>(HOME_DEFAULT);
  const screenRef = useRef(screen);
  screenRef.current = screen;

  useEffect(() => {
    if (!enabled) {
      // Widened past the phone layout: keep the person's tab and drop only the pushed Session
      // screen, whose history entry the wide layout does not own. Sign-out calls reset().
      setScreen((current) => (current.kind === "session" ? { kind: "home", destination: current.from } : current));
      return;
    }
    // Anchor the current entry as the home the back gesture lands on.
    writeHistory(screenRef.current, "replace");
    const onPop = (event: PopStateEvent) => {
      // An entry this hook did not write is the one it anchored on: that is the home.
      const next = screenFromState(event.state);
      const current = screenRef.current;
      setScreen(next ?? { kind: "home", destination: current.kind === "home" ? current.destination : current.from });
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [enabled]);

  const selectTab = useCallback(
    (destination: PhoneDestination) => {
      const current = screenRef.current;
      if (current.kind === "home" && current.destination === destination) return;
      const next: PhoneScreen = { kind: "home", destination };
      setScreen(next);
      if (enabled) writeHistory(next, "replace");
    },
    [enabled],
  );

  const openSession = useCallback(() => {
    const current = screenRef.current;
    if (current.kind === "session") return;
    const next: PhoneScreen = { kind: "session", from: current.destination };
    setScreen(next);
    if (enabled) writeHistory(next, "push");
  }, [enabled]);

  const back = useCallback(() => {
    const current = screenRef.current;
    if (current.kind !== "session") return;
    setScreen({ kind: "home", destination: current.from });
    // Pop the entry openSession pushed so the back gesture does not revisit the session.
    if (enabled && screenFromState(window.history.state)?.kind === "session") window.history.back();
  }, [enabled]);

  const reset = useCallback(() => {
    setScreen(HOME_DEFAULT);
    if (enabled) writeHistory(HOME_DEFAULT, "replace");
  }, [enabled]);

  return {
    screen,
    destination: screen.kind === "home" ? screen.destination : screen.from,
    selectTab,
    openSession,
    back,
    reset,
  };
}

/** The key an open overlay owns inside `history.state`. */
export const OVERLAY_HISTORY_KEY = "ferryxOverlay";

let overlaySeq = 0;

/**
 * Gives an open sheet or drawer its own history entry, so the platform back gesture closes it
 * before it touches the page underneath. Closing it any other way pops that entry again. Works
 * in every layout and in paired mode: it never reads or writes the phone screen key.
 */
export function useOverlayHistory(open: boolean, onClose: () => void): void {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    if (!open) return;
    const id = `overlay-${(overlaySeq += 1)}`;
    const base = window.history.state && typeof window.history.state === "object" ? window.history.state : {};
    window.history.pushState({ ...base, [OVERLAY_HISTORY_KEY]: id }, "");
    let owned = true;
    const onPop = (event: PopStateEvent) => {
      const state = event.state as Record<string, unknown> | null;
      if (state?.[OVERLAY_HISTORY_KEY] === id) return;
      owned = false;
      closeRef.current();
    };
    window.addEventListener("popstate", onPop);
    return () => {
      window.removeEventListener("popstate", onPop);
      const state = window.history.state as Record<string, unknown> | null;
      if (owned && state?.[OVERLAY_HISTORY_KEY] === id) window.history.back();
    };
  }, [open]);
}
