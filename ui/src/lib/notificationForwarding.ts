import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { NotificationTarget } from "./types";
import { isDesktopRuntime } from "./windowIdentity";

/**
 * Broadcast by the window that drained a notification click it does not own, carrying
 * `{ from, target }`. Exactly one window drains the click queue (the primary), so a click is
 * consumed once; the window whose store holds the session then navigates, and every other window
 * ignores it.
 */
export const NOTIFICATION_TARGET_FORWARD_EVENT = "ferryx://notification-target";

export type ForwardedNotificationTarget = { from: string; target: NotificationTarget };

/**
 * Routes a drained click: the draining window navigates when it owns the session, otherwise the
 * click is forwarded to the other windows. It never does both, so no window handles it twice.
 */
export function routeNotificationTarget(input: {
  target: NotificationTarget;
  ownsTarget: (target: NotificationTarget) => boolean;
  navigate: (target: NotificationTarget) => void;
  forward: (target: NotificationTarget) => void;
}): "navigated" | "forwarded" {
  if (input.ownsTarget(input.target)) {
    input.navigate(input.target);
    return "navigated";
  }
  input.forward(input.target);
  return "forwarded";
}

export function parseForwardedNotificationTarget(raw: unknown): ForwardedNotificationTarget | null {
  if (!raw || typeof raw !== "object") return null;
  const { from, target } = raw as Partial<ForwardedNotificationTarget>;
  if (typeof from !== "string" || !target || typeof target !== "object") return null;
  const { workspaceId, sessionId } = target as Partial<NotificationTarget>;
  if (typeof workspaceId !== "string" || !workspaceId || typeof sessionId !== "string" || !sessionId) return null;
  return { from, target: { workspaceId, sessionId } };
}

/** Hands a click this window does not own to the other windows. */
export function forwardNotificationTarget(from: string, target: NotificationTarget): void {
  if (!isDesktopRuntime()) return;
  const payload: ForwardedNotificationTarget = { from, target };
  void Promise.resolve()
    .then(() => emit(NOTIFICATION_TARGET_FORWARD_EVENT, payload))
    .catch((error: unknown) => console.warn("Could not forward a notification click", error));
}

/** Raises this window; the shell only raised the window it guessed owned the session. */
export function raiseThisWindow(): void {
  void Promise.resolve()
    .then(() => getCurrentWindow().setFocus())
    .catch(() => undefined);
}

/**
 * Listens for clicks another window forwarded. `ownsTarget` is checked here, so only the window
 * holding the session navigates (and raises itself).
 */
export function subscribeForwardedNotificationTargets(
  selfLabel: string,
  ownsTarget: (target: NotificationTarget) => boolean,
  navigate: (target: NotificationTarget) => void,
): () => void {
  if (!isDesktopRuntime()) return () => undefined;
  let disposed = false;
  let unlisten: (() => void) | null = null;
  void listen<unknown>(NOTIFICATION_TARGET_FORWARD_EVENT, (event) => {
    const forwarded = parseForwardedNotificationTarget(event.payload);
    if (!forwarded || forwarded.from === selfLabel || !ownsTarget(forwarded.target)) return;
    navigate(forwarded.target);
    raiseThisWindow();
  })
    .then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    })
    .catch((error: unknown) => console.warn("Notification click forwarding unavailable", error));
  return () => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  };
}
