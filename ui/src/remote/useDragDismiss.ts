import {
  useCallback,
  useEffect,
  useRef,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from "react";

/**
 * Swipe-to-dismiss for the phone overlays (bottom sheet, left drawer). Pointer events only, so
 * one path serves touch and mouse. A gesture stays pending until it moves past the slop along the
 * dismiss axis and more along than across, so taps and cross-axis scrolling are left alone. While
 * dragging, the panel follows the pointer through an inline transform; on release it dismisses
 * (past 30% of its size, or a flick) or springs back.
 *
 * Inline transform/opacity are written `!important`: the reduced-motion stylesheet pins
 * `transform: none !important` on the overlay animation classes, and a running entry animation
 * would otherwise win over a plain inline value.
 */

export type DragDismissAxis = "y-down" | "x-left";

export const DRAG_SLOP_PX = 8;
export const DRAG_DISMISS_FRACTION = 0.3;
/** px per ms */
export const DRAG_FLICK_VELOCITY = 0.5;
export const DRAG_FLICK_MIN_PX = 24;
export const DRAG_RESISTANCE = 0.3;
export const DRAG_SETTLE_MS = 220;
const DRAG_EASE = "cubic-bezier(.2,.8,.2,1)";
/** A pause this long before release means the finger stopped: not a flick. */
const VELOCITY_STALE_MS = 100;
/** Only the click that immediately follows a drag is swallowed. */
const CLICK_SUPPRESS_MS = 400;

const NO_DRAG_SELECTOR = 'input, textarea, select, [contenteditable="true"], [data-no-drag-dismiss]';

/** Native events already claimed by an inner overlay, so a nested one never drags its parent too. */
const claimed = new WeakSet<Event>();

export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)")?.matches === true
  );
}

export type UseDragDismissOptions = {
  axis: DragDismissAxis;
  panelRef: RefObject<HTMLElement | null>;
  /** Scrim whose opacity follows the drag progress. */
  backdropRef?: RefObject<HTMLElement | null>;
  /** The panel is mounted and interactive; gestures are ignored otherwise. */
  active: boolean;
  /** Called once a release crosses the threshold; the panel is already on its way out. */
  onDismiss: () => void;
  /**
   * Call onDismiss only after the panel has slid out, for an overlay that unmounts on close with
   * no exit animation of its own. Default: call it at once and let the owner finish the exit.
   */
  deferDismiss?: boolean;
};

export type DragDismissHandlers = {
  onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void;
  onPointerUp: (event: ReactPointerEvent<HTMLElement>) => void;
  onPointerCancel: (event: ReactPointerEvent<HTMLElement>) => void;
  onClickCapture: (event: ReactMouseEvent<HTMLElement>) => void;
};

type Gesture = {
  pointerId: number | undefined;
  startX: number;
  startY: number;
  dragging: boolean;
  size: number;
  offset: number;
  lastAlong: number;
  lastTime: number;
  velocity: number;
};

function setImportant(element: HTMLElement, property: string, value: string) {
  element.style.setProperty(property, value, "important");
}

export function useDragDismiss({
  axis,
  panelRef,
  backdropRef,
  active,
  onDismiss,
  deferDismiss = false,
}: UseDragDismissOptions): { handlers: DragDismissHandlers; reset: () => void } {
  const gestureRef = useRef<Gesture | null>(null);
  const suppressClickUntilRef = useRef(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dismissRef = useRef(onDismiss);
  dismissRef.current = onDismiss;
  const activeRef = useRef(active);
  activeRef.current = active;

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  /** Signed distance in the dismiss direction (down for y-down, left for x-left). */
  const along = (dx: number, dy: number) => (axis === "y-down" ? dy : -dx);
  const across = (dx: number, dy: number) => (axis === "y-down" ? dx : dy);
  const translate = (px: number) => (axis === "y-down" ? `translate3d(0, ${px}px, 0)` : `translate3d(${-px}px, 0, 0)`);
  const exitTransform = axis === "y-down" ? "translate3d(0, 100%, 0)" : "translate3d(-100%, 0, 0)";

  /** Drop every inline style the gesture wrote (re-open, refused close). */
  const reset = useCallback(() => {
    clearTimer();
    gestureRef.current = null;
    const panel = panelRef.current;
    if (panel) {
      panel.style.removeProperty("transform");
      panel.style.removeProperty("transition");
      panel.style.removeProperty("pointer-events");
    }
    const backdrop = backdropRef?.current;
    if (backdrop) {
      backdrop.style.removeProperty("opacity");
      backdrop.style.removeProperty("transition");
    }
  }, [clearTimer, panelRef, backdropRef]);

  // Leaving the interactive state drops a half-made gesture; (re-)entering it drops whatever a
  // previous drag left inline, so a sheet re-opened mid-exit is not stuck off screen.
  useEffect(() => {
    if (active) reset();
    else gestureRef.current = null;
  }, [active, reset]);

  useEffect(() => clearTimer, [clearTimer]);

  // Once the browser starts panning a touch it cancels the pointer stream (pointercancel), even
  // on a pan-y scroller resting at the top. Holding the touchmove default while the gesture heads
  // the dismiss way keeps the stream ours. A tap never moves, so it is never prevented.
  useEffect(() => {
    const panel = panelRef.current;
    if (!active || !panel) return;
    const onTouchMove = (event: TouchEvent) => {
      const gesture = gestureRef.current;
      if (!gesture || !event.cancelable) return;
      if (gesture.dragging) {
        event.preventDefault();
        return;
      }
      const touch = event.touches[0];
      if (!touch) return;
      const dx = touch.clientX - gesture.startX;
      const dy = touch.clientY - gesture.startY;
      const a = axis === "y-down" ? dy : -dx;
      const c = axis === "y-down" ? dx : dy;
      if (a > 2 && a > Math.abs(c)) event.preventDefault();
    };
    panel.addEventListener("touchmove", onTouchMove, { passive: false });
    return () => panel.removeEventListener("touchmove", onTouchMove);
  }, [active, panelRef, axis]);

  const applyProgress = (offset: number, size: number) => {
    const panel = panelRef.current;
    if (panel) {
      setImportant(panel, "transition", "none");
      setImportant(panel, "transform", translate(offset));
    }
    const backdrop = backdropRef?.current;
    if (backdrop) {
      setImportant(backdrop, "transition", "none");
      setImportant(backdrop, "opacity", String(1 - Math.min(1, Math.max(0, offset / size))));
    }
  };

  const settleBack = () => {
    if (prefersReducedMotion()) {
      reset();
      return;
    }
    const panel = panelRef.current;
    const backdrop = backdropRef?.current;
    if (panel) {
      setImportant(panel, "transition", `transform ${DRAG_SETTLE_MS}ms ${DRAG_EASE}`);
      setImportant(panel, "transform", "translate3d(0, 0, 0)");
    }
    if (backdrop) {
      setImportant(backdrop, "transition", `opacity ${DRAG_SETTLE_MS}ms ${DRAG_EASE}`);
      setImportant(backdrop, "opacity", "1");
    }
    clearTimer();
    timerRef.current = setTimeout(reset, DRAG_SETTLE_MS);
  };

  const dismiss = () => {
    const panel = panelRef.current;
    const backdrop = backdropRef?.current;
    const reduced = prefersReducedMotion();
    if (panel) {
      setImportant(panel, "transition", reduced ? "none" : `transform ${DRAG_SETTLE_MS}ms ${DRAG_EASE}`);
      setImportant(panel, "transform", exitTransform);
      panel.style.setProperty("pointer-events", "none");
    }
    if (backdrop) {
      setImportant(backdrop, "transition", reduced ? "none" : `opacity ${DRAG_SETTLE_MS}ms ${DRAG_EASE}`);
      setImportant(backdrop, "opacity", "0");
    }
    clearTimer();
    const close = () => {
      dismissRef.current();
      // The owner may refuse the close; then the panel comes back instead of hiding off screen.
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        if (activeRef.current) settleBack();
      }, DRAG_SETTLE_MS + 100);
    };
    if (!deferDismiss || reduced || !panel) {
      close();
      return;
    }
    const finish = () => {
      panel.removeEventListener("transitionend", onEnd);
      clearTimer();
      close();
    };
    const onEnd = (event: TransitionEvent) => {
      if (event.target === panel && event.propertyName === "transform") finish();
    };
    panel.addEventListener("transitionend", onEnd);
    timerRef.current = setTimeout(finish, DRAG_SETTLE_MS + 60);
  };

  const releaseCapture = (element: HTMLElement, pointerId: number | undefined) => {
    if (pointerId === undefined) return;
    try {
      if (element.hasPointerCapture?.(pointerId)) element.releasePointerCapture(pointerId);
    } catch {
      // capture already released by the platform
    }
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    suppressClickUntilRef.current = 0;
    if (!active || claimed.has(event.nativeEvent)) return;
    if (event.isPrimary === false) return;
    if (event.pointerType === "mouse" && event.button !== 0) return;
    const panel = panelRef.current;
    const target = event.target;
    if (!panel || !(target instanceof Element) || !panel.contains(target)) return;
    if (target.closest(NO_DRAG_SELECTOR)) return;
    if (axis === "y-down") {
      // In content that is scrolled down, a downward swipe scrolls it back up first.
      for (let node: Element | null = target; node && node !== panel; node = node.parentElement) {
        if (node.scrollTop > 0) return;
      }
    }
    claimed.add(event.nativeEvent);
    gestureRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      dragging: false,
      size: 1,
      offset: 0,
      lastAlong: 0,
      lastTime: performance.now(),
      velocity: 0,
    };
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    const gesture = gestureRef.current;
    const panel = panelRef.current;
    if (!gesture || !panel || event.pointerId !== gesture.pointerId) return;
    const dx = event.clientX - gesture.startX;
    const dy = event.clientY - gesture.startY;
    const a = along(dx, dy);

    if (!gesture.dragging) {
      const c = across(dx, dy);
      if (Math.abs(a) <= DRAG_SLOP_PX && Math.abs(c) <= DRAG_SLOP_PX) return;
      // Cross-axis or toward-open movement is not a dismiss: leave it to scrolling and taps.
      if (a <= DRAG_SLOP_PX || a <= Math.abs(c)) {
        gestureRef.current = null;
        return;
      }
      clearTimer();
      const rect = panel.getBoundingClientRect();
      let size = axis === "y-down" ? rect.height : rect.width;
      if (!(size > 0)) size = axis === "y-down" ? window.innerHeight : window.innerWidth;
      gesture.dragging = true;
      gesture.size = size > 0 ? size : 1;
      if (gesture.pointerId !== undefined) {
        try {
          panel.setPointerCapture(gesture.pointerId);
        } catch {
          // best effort: moves still bubble to the panel without capture
        }
      }
    }

    // Offset is measured from the touch-down point; toward-open movement meets resistance.
    const offset = a >= 0 ? a : a * DRAG_RESISTANCE;
    const now = performance.now();
    const dt = now - gesture.lastTime;
    if (dt > 0) {
      gesture.velocity = (a - gesture.lastAlong) / dt;
      gesture.lastTime = now;
      gesture.lastAlong = a;
    }
    gesture.offset = offset;
    applyProgress(offset, gesture.size);
  };

  const onPointerUp = (event: ReactPointerEvent<HTMLElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    gestureRef.current = null;
    if (!gesture.dragging) return;
    releaseCapture(event.currentTarget, gesture.pointerId);
    const now = performance.now();
    suppressClickUntilRef.current = now + CLICK_SUPPRESS_MS;
    const velocity = now - gesture.lastTime > VELOCITY_STALE_MS ? 0 : gesture.velocity;
    const { offset, size } = gesture;
    if (offset > size * DRAG_DISMISS_FRACTION || (velocity > DRAG_FLICK_VELOCITY && offset > DRAG_FLICK_MIN_PX)) {
      dismiss();
    } else {
      settleBack();
    }
  };

  const onPointerCancel = (event: ReactPointerEvent<HTMLElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    gestureRef.current = null;
    if (!gesture.dragging) return;
    releaseCapture(event.currentTarget, gesture.pointerId);
    settleBack();
  };

  // The click a browser synthesizes after a drag (mouse, mostly) is not a tap on what lies under it.
  const onClickCapture = (event: ReactMouseEvent<HTMLElement>) => {
    if (suppressClickUntilRef.current === 0) return;
    const suppress = performance.now() <= suppressClickUntilRef.current;
    suppressClickUntilRef.current = 0;
    if (!suppress) return;
    event.preventDefault();
    event.stopPropagation();
  };

  return {
    handlers: { onPointerDown, onPointerMove, onPointerUp, onPointerCancel, onClickCapture },
    reset,
  };
}
