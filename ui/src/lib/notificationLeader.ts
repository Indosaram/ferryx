import { emit, listen } from "@tauri-apps/api/event";

import { PRIMARY_WINDOW_LABEL, currentWindowLabel, isAppWindowLabel, isDesktopRuntime } from "./windowIdentity";

/** Broadcast by every Ferryx window when its native focus changes: `{ label, focused }`. */
export const WINDOW_FOCUS_EVENT = "ferryx://window-focus";

export type WindowFocusPayload = { label: string; focused: boolean };

/** Broadcast by every Ferryx window when its unread badge contribution changes. */
export const WINDOW_BADGE_EVENT = "ferryx://window-badge";

export type WindowBadgePayload = { label: string; count: number };

/** Broadcast by a window that just started so the others republish their focus, claims and badge. */
export const WINDOW_STATE_REQUEST_EVENT = "ferryx://window-state-request";

/** Broadcast by every Ferryx window when the set of backend sessions in its store changes. */
export const WINDOW_SESSIONS_EVENT = "ferryx://window-sessions";

export type WindowSessionsPayload = { label: string; backendSessionIds: string[] };

/**
 * Notification ownership rule (multi-window).
 *
 * A terminal session lives in exactly one window's workspace store, and session-scoped backend
 * events (bell, agent state) are emitted only to the window that owns the session. Notifying is
 * therefore decided by ownership, not by focus:
 *
 *   notify  <=>  ownsSession  OR  (isPrimary AND no other window claims the session)
 *
 * - ownsSession: some session in THIS window's store has `backendSessionId === payload.sessionId`
 *   (payload ids are backend ids). The owner notifies even when another Ferryx window is focused,
 *   because the focused window never receives the event; the owner keeps today's per-window focus
 *   suppression (NotificationCoordinator.isFocused) for what it is showing itself.
 * - A backend that cannot resolve a live owner falls back to broadcasting, so every window sees
 *   the event. A non-owner secondary window never notifies; the primary window notifies only for a
 *   session no other window has announced (WINDOW_SESSIONS_EVENT), so a broadcast yields exactly
 *   one notification. A session announced late can race this; that window then stays silent.
 *
 * The dock badge is app-global state rather than a per-event effect, so it uses the focus-based
 * badge rule below: the focused window publishes it, else the primary window.
 */
export function shouldNotifyForSession(input: {
  ownsSession: boolean;
  isPrimary: boolean;
  claimedByOtherWindow: boolean;
}): boolean {
  return input.ownsSession || (input.isPrimary && !input.claimedByOtherWindow);
}

/** Backend session ids each other Ferryx window last announced holding in its store. */
export class SessionClaimTracker {
  private readonly claims = new Map<string, Set<string>>();

  constructor(readonly selfLabel: string) {}

  apply(payload: unknown): void {
    if (!payload || typeof payload !== "object") return;
    const { label, backendSessionIds } = payload as Partial<WindowSessionsPayload>;
    if (typeof label !== "string" || !isAppWindowLabel(label) || label === this.selfLabel) return;
    if (!Array.isArray(backendSessionIds)) return;
    const ids = backendSessionIds.filter((id): id is string => typeof id === "string" && id.length > 0);
    if (ids.length === 0) this.claims.delete(label);
    else this.claims.set(label, new Set(ids));
  }

  /** A closed window's sessions are adopted elsewhere; its stale claims must not mute main. */
  forget(label: string): void {
    this.claims.delete(label);
  }

  isClaimedByOtherWindow(backendSessionId: string): boolean {
    for (const ids of this.claims.values()) if (ids.has(backendSessionId)) return true;
    return false;
  }
}

export function isBadgeLeader(input: {
  selfFocused: boolean;
  anyAppWindowFocused: boolean;
  isPrimary: boolean;
}): boolean {
  return input.selfFocused || (!input.anyAppWindowFocused && input.isPrimary);
}

/** Labels of Ferryx windows that last reported themselves focused. */
export class WindowFocusTracker {
  private readonly focusedLabels = new Set<string>();
  private readonly listeners = new Set<() => void>();

  constructor(readonly selfLabel: string) {}

  apply(payload: unknown): void {
    if (!payload || typeof payload !== "object") return;
    const { label, focused } = payload as Partial<WindowFocusPayload>;
    if (typeof label !== "string" || !isAppWindowLabel(label) || typeof focused !== "boolean") return;
    const changed = focused ? !this.focusedLabels.has(label) : this.focusedLabels.has(label);
    if (!changed) return;
    if (focused) this.focusedLabels.add(label);
    else this.focusedLabels.delete(label);
    for (const listener of this.listeners) listener();
  }

  /** A closed window never reports blur; forget it so it cannot hold leadership. */
  forget(label: string): void {
    this.apply({ label, focused: false });
  }

  isSelfFocused(): boolean {
    return this.focusedLabels.has(this.selfLabel);
  }

  isAnyFocused(): boolean {
    return this.focusedLabels.size > 0;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }
}

let tracker: WindowFocusTracker | null = null;
let claimTracker: SessionClaimTracker | null = null;
let started = false;
const otherBadgeCounts = new Map<string, number>();
const windowStateListeners = new Set<() => void>();
let lastFocus: boolean | null = null;
let lastClaims: string[] | null = null;
let lastBadgeCount: number | null = null;

function notifyWindowState(): void {
  for (const listener of windowStateListeners) listener();
}

function applyBadge(payload: unknown, selfLabel: string): void {
  if (!payload || typeof payload !== "object") return;
  const { label, count } = payload as Partial<WindowBadgePayload>;
  if (typeof label !== "string" || !isAppWindowLabel(label) || label === selfLabel) return;
  if (typeof count !== "number" || !Number.isFinite(count) || count < 0) return;
  if (otherBadgeCounts.get(label) === count) return;
  if (count === 0) otherBadgeCounts.delete(label);
  else otherBadgeCounts.set(label, count);
  notifyWindowState();
}

function emitQuietly(event: string, payload: unknown): void {
  void Promise.resolve()
    .then(() => emit(event, payload))
    .catch(() => undefined);
}

function sharedTracker(): WindowFocusTracker {
  if (!tracker) tracker = new WindowFocusTracker(currentWindowLabel());
  return tracker;
}

function sharedClaims(): SessionClaimTracker {
  if (!claimTracker) claimTracker = new SessionClaimTracker(currentWindowLabel());
  return claimTracker;
}

/**
 * Starts listening to every Ferryx window's focus broadcast. This window's own focus is published
 * by `nativeWindowFocus.ts` (the single `onFocusChanged` subscription) through
 * `publishWindowFocus`. Idempotent; a no-op outside the desktop shell.
 */
export function startWindowFocusBroadcast(): void {
  if (started || !isDesktopRuntime()) return;
  started = true;
  const self = sharedTracker();
  const claims = sharedClaims();
  self.subscribe(notifyWindowState);
  const republish = () => {
    if (lastFocus !== null) emitQuietly(WINDOW_FOCUS_EVENT, { label: self.selfLabel, focused: lastFocus });
    if (lastClaims !== null) emitQuietly(WINDOW_SESSIONS_EVENT, { label: self.selfLabel, backendSessionIds: lastClaims });
    if (lastBadgeCount !== null) emitQuietly(WINDOW_BADGE_EVENT, { label: self.selfLabel, count: lastBadgeCount });
  };
  void Promise.all([
    listen<unknown>(WINDOW_FOCUS_EVENT, (event) => self.apply(event.payload)),
    listen<unknown>(WINDOW_SESSIONS_EVENT, (event) => claims.apply(event.payload)),
    listen<unknown>(WINDOW_BADGE_EVENT, (event) => applyBadge(event.payload, self.selfLabel)),
    listen<unknown>(WINDOW_STATE_REQUEST_EVENT, (event) => {
      const from = (event.payload as { label?: unknown } | null)?.label;
      if (from !== self.selfLabel) republish();
    }),
  ])
    .then(() => emitQuietly(WINDOW_STATE_REQUEST_EVENT, { label: self.selfLabel }))
    .catch((error: unknown) => console.warn("Window state broadcast unavailable", error));
  if (typeof window !== "undefined") {
    // Leaving leadership, claims and badge on unload keeps a closing window from muting the others.
    window.addEventListener("beforeunload", () => {
      publishWindowFocus(false);
      publishWindowSessions([]);
      publishWindowBadgeCount(0);
    });
  }
}

/** Broadcasts this window's unread badge contribution; the badge leader shows the app total. */
export function publishWindowBadgeCount(count: number): void {
  if (!started || lastBadgeCount === count) return;
  lastBadgeCount = count;
  emitQuietly(WINDOW_BADGE_EVENT, { label: sharedTracker().selfLabel, count });
}

/** The app-wide unread total: this window's count plus every other window's last broadcast. */
export function appBadgeTotal(selfCount: number): number {
  let total = selfCount;
  for (const count of otherBadgeCounts.values()) total += count;
  return total;
}

/** Fires when another window's focus or badge contribution changes. */
export function subscribeWindowStateChanges(listener: () => void): () => void {
  windowStateListeners.add(listener);
  return () => {
    windowStateListeners.delete(listener);
  };
}

/** Broadcasts the backend session ids held in this window's workspace store (mounted + parked). */
export function publishWindowSessions(backendSessionIds: readonly string[]): void {
  if (!started) return;
  lastClaims = [...backendSessionIds];
  emitQuietly(WINDOW_SESSIONS_EVENT, { label: sharedClaims().selfLabel, backendSessionIds: lastClaims });
}

/** Forgets a closed window's focus and session claims. */
export function forgetWindow(label: string): void {
  sharedTracker().forget(label);
  sharedClaims().forget(label);
  if (otherBadgeCounts.delete(label)) notifyWindowState();
}

/**
 * Whether this window fires the OS notification/sound for a backend session event it received.
 * `ownsSession` must be computed against this window's store by backendSessionId. Outside the
 * multi-window desktop shell (or before tracking starts) this is the single-window behavior:
 * every event the store resolved notifies.
 */
export function shouldThisWindowNotifyFor(backendSessionId: string | null | undefined, ownsSession: boolean): boolean {
  if (!started) return true;
  return shouldNotifyForSession({
    ownsSession,
    isPrimary: sharedClaims().selfLabel === PRIMARY_WINDOW_LABEL,
    claimedByOtherWindow: Boolean(backendSessionId) && sharedClaims().isClaimedByOtherWindow(backendSessionId as string),
  });
}

/** Records and broadcasts this window's native focus to every Ferryx window. */
export function publishWindowFocus(focused: boolean): void {
  if (!started) return;
  const self = sharedTracker();
  const payload: WindowFocusPayload = { label: self.selfLabel, focused };
  lastFocus = focused;
  self.apply(payload);
  // `emit` reaches every window, including this one; apply() ignores the duplicate.
  emitQuietly(WINDOW_FOCUS_EVENT, payload);
}

/** Whether this window publishes the app-global dock badge. */
export function shouldThisWindowOwnBadge(isPrimary: boolean, fallbackSelfFocused: boolean): boolean {
  if (!started) return true;
  const self = sharedTracker();
  const selfFocused = self.isSelfFocused() || fallbackSelfFocused;
  return isBadgeLeader({ selfFocused, anyAppWindowFocused: selfFocused || self.isAnyFocused(), isPrimary });
}

export function resetNotificationLeaderForTests(): void {
  tracker = null;
  claimTracker = null;
  started = false;
  otherBadgeCounts.clear();
  windowStateListeners.clear();
  lastFocus = null;
  lastClaims = null;
  lastBadgeCount = null;
}
