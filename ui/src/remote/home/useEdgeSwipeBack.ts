// iOS-style edge swipe back for the phone Session screen. A drag that starts within the left
// edge follows the finger; released far enough (or flung fast enough) to the right it calls
// onBack once, otherwise the screen springs back. Only installed where nothing else owns the
// edge gesture: a plain Safari tab has its own back swipe, and the two would fight.
import { useEffect, useRef, useState } from "react";

/** A drag counts only when it starts at or left of this clientX. */
export const EDGE_SWIPE_START_MAX_X = 24;
/** Horizontal travel that commits the back. */
export const EDGE_SWIPE_COMMIT_PX = 80;
/** Rightward release speed (px/ms) that commits the back regardless of travel. */
export const EDGE_SWIPE_COMMIT_VELOCITY = 0.5;

const AXIS_LOCK_PX = 8;
/** A release this long after the last move is a stop, not a fling. */
const FLING_WINDOW_MS = 100;
const SPRING_BACK = "transform 250ms cubic-bezier(0.2, 0.8, 0.2, 1)";

/**
 * True where the app owns the left-edge gesture: an installed standalone PWA (display-mode or
 * iOS `navigator.standalone`) or the Tauri mobile shell.
 */
export function edgeSwipeHostSupported(): boolean {
  if (typeof window === "undefined") return false;
  if ("__TAURI_INTERNALS__" in window) return true;
  if ((navigator as Navigator & { standalone?: boolean }).standalone === true) return true;
  return typeof window.matchMedia === "function" && window.matchMedia("(display-mode: standalone)").matches;
}

export type EdgeSwipeBackOptions = {
  /** Install the gesture (the caller decides layout and host). */
  readonly enabled: boolean;
  /** QA escape: install even when `enabled` is false. */
  readonly forceEnabled?: boolean;
  /** The existing history back path; called at most once per gesture. */
  readonly onBack: () => void;
};

/** Returns a callback ref for the element the gesture drives (the Session screen root). */
export function useEdgeSwipeBack({ enabled, forceEnabled = false, onBack }: EdgeSwipeBackOptions): (element: HTMLElement | null) => void {
  const [element, setElement] = useState<HTMLElement | null>(null);
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;
  const active = enabled || forceEnabled;
  useEffect(() => {
    if (!active || !element) return;
    return installEdgeSwipe(element, () => onBackRef.current());
  }, [active, element]);
  return setElement;
}

type Drag = {
  readonly startX: number;
  readonly startY: number;
  axis: "x" | "y" | null;
  lastX: number;
  lastT: number;
  velocity: number;
};

function now(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function reducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function installEdgeSwipe(element: HTMLElement, back: () => void): () => void {
  let drag: Drag | null = null;
  let pointerId: number | undefined;
  let touchId: number | null = null;
  const frames: number[] = [];
  // Touch input arrives as touch events where the platform has them (they can stop the page
  // scrolling); pointer events cover mouse and pen, and touch only where TouchEvent is absent.
  const touchEventsOwnTouch = typeof TouchEvent !== "undefined";

  const setOffset = (px: number) => {
    element.style.transition = "none";
    element.style.transform = px > 0 ? `translate3d(${px}px, 0, 0)` : "";
  };

  const springBack = () => {
    if (!element.style.transform || reducedMotion()) {
      element.style.transition = "";
      element.style.transform = "";
      return;
    }
    element.style.transition = SPRING_BACK;
    element.style.transform = "";
    element.addEventListener("transitionend", () => {
      element.style.transition = "";
    }, { once: true });
  };

  // After a committed back the screen keeps its drag offset so the pop transition starts from
  // where the finger left it; two frames later the old snapshot is taken and the offset can go.
  const afterSnapshot = (callback: () => void) => {
    if (typeof requestAnimationFrame !== "function") {
      callback();
      return;
    }
    frames.push(requestAnimationFrame(() => {
      frames.push(requestAnimationFrame(callback));
    }));
  };

  const begin = (x: number, y: number): boolean => {
    if (x > EDGE_SWIPE_START_MAX_X) return false;
    drag = { startX: x, startY: y, axis: null, lastX: x, lastT: now(), velocity: 0 };
    return true;
  };

  /** Returns the locked axis, or null while the drag is still too short to tell. */
  const move = (x: number, y: number): "x" | "y" | null => {
    if (!drag) return null;
    const dx = x - drag.startX;
    const dy = y - drag.startY;
    if (drag.axis === null) {
      if (Math.max(Math.abs(dx), Math.abs(dy)) < AXIS_LOCK_PX) return null;
      drag.axis = Math.abs(dx) > Math.abs(dy) ? "x" : "y";
    }
    if (drag.axis === "y") return "y";
    const t = now();
    const dt = t - drag.lastT;
    if (dt > 0) drag.velocity = (x - drag.lastX) / dt;
    drag.lastX = x;
    drag.lastT = t;
    setOffset(Math.max(0, dx));
    return "x";
  };

  const finish = (x: number, y: number, cancelled: boolean) => {
    const ended = drag;
    drag = null;
    stopTracking();
    if (!ended || ended.axis !== "x" || cancelled) {
      springBack();
      return;
    }
    const dx = x - ended.startX;
    const dy = y - ended.startY;
    const fling = now() - ended.lastT < FLING_WINDOW_MS && ended.velocity > EDGE_SWIPE_COMMIT_VELOCITY;
    const commit = dx > 0 && ((dx >= EDGE_SWIPE_COMMIT_PX && Math.abs(dx) > Math.abs(dy)) || fling);
    if (!commit) {
      springBack();
      return;
    }
    back();
    afterSnapshot(springBack);
  };

  const abandon = () => {
    drag = null;
    stopTracking();
    springBack();
  };

  const onPointerMove = (event: PointerEvent) => {
    if (event.pointerId !== pointerId) return;
    if (move(event.clientX, event.clientY) === "y") abandon();
  };
  const onPointerUp = (event: PointerEvent) => {
    if (event.pointerId !== pointerId) return;
    finish(event.clientX, event.clientY, false);
  };
  const onPointerCancel = (event: PointerEvent) => {
    if (event.pointerId !== pointerId) return;
    finish(event.clientX, event.clientY, true);
  };
  const onPointerDown = (event: PointerEvent) => {
    if (drag) return;
    if (event.pointerType === "touch" && touchEventsOwnTouch) return;
    if (event.button !== 0) return;
    if (!begin(event.clientX, event.clientY)) return;
    pointerId = event.pointerId;
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerCancel);
  };

  const ownTouch = (list: TouchList): Touch | undefined => Array.from(list).find((touch) => touch.identifier === touchId);
  const onTouchMove = (event: TouchEvent) => {
    const touch = ownTouch(event.changedTouches);
    if (!touch) return;
    const axis = move(touch.clientX, touch.clientY);
    if (axis === "y") abandon();
    // Once the drag is horizontal the page underneath must not scroll with it.
    else if (axis === "x" && event.cancelable) event.preventDefault();
  };
  const onTouchEnd = (event: TouchEvent) => {
    const touch = ownTouch(event.changedTouches);
    if (!touch) return;
    finish(touch.clientX, touch.clientY, event.type === "touchcancel");
  };
  const onTouchStart = (event: TouchEvent) => {
    if (drag || event.touches.length !== 1) return;
    const touch = event.changedTouches[0];
    if (!touch || !begin(touch.clientX, touch.clientY)) return;
    touchId = touch.identifier;
    window.addEventListener("touchmove", onTouchMove, { passive: false });
    window.addEventListener("touchend", onTouchEnd);
    window.addEventListener("touchcancel", onTouchEnd);
  };

  function stopTracking() {
    pointerId = undefined;
    touchId = null;
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", onPointerUp);
    window.removeEventListener("pointercancel", onPointerCancel);
    window.removeEventListener("touchmove", onTouchMove);
    window.removeEventListener("touchend", onTouchEnd);
    window.removeEventListener("touchcancel", onTouchEnd);
  }

  element.addEventListener("pointerdown", onPointerDown);
  element.addEventListener("touchstart", onTouchStart, { passive: true });
  return () => {
    element.removeEventListener("pointerdown", onPointerDown);
    element.removeEventListener("touchstart", onTouchStart);
    stopTracking();
    drag = null;
    if (typeof cancelAnimationFrame === "function") frames.forEach((frame) => cancelAnimationFrame(frame));
    element.style.transition = "";
    element.style.transform = "";
  };
}
