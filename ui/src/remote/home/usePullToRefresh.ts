// Native-feeling pull-to-refresh for the phone home lists, built on touch events so it behaves
// the same in iOS WKWebView, Android WebView and mobile browsers. A pull only starts from the top
// of the list with a clearly vertical, downward drag: taps, row presses and horizontal swipes
// never activate it, and touchmove calls preventDefault only while a pull is active. The
// touchmove listener is registered non-passive for the container's lifetime because WebKit
// decides whether a gesture's moves are cancelable when it starts; a listener added from inside
// touchstart is not reliably honored for that gesture in WKWebView.
import { createContext, useCallback, useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";

export type RefreshHandler = () => Promise<unknown> | void;

/** Raw finger travel is multiplied by this, so the list resists the pull. */
export const PULL_RESISTANCE = 0.5;
/** Dampened distance a release must reach to refresh. */
export const PULL_THRESHOLD = 72;
/** Finger travel below this is a tap, never a pull. */
export const PULL_SLOP = 10;
/** Dampened distance the pull stops growing at. */
export const PULL_MAX = 128;
/** Where the content rests while the spinner shows. */
export const PULL_HOLD = 48;
/** The spinner stays at least this long so a fast refresh is still visible. */
export const REFRESH_MIN_MS = 400;

export type PullToRefreshState = {
  /** Dampened pull distance in px; the content translates down by this much. */
  readonly pullDistance: number;
  /** A refresh is running; the spinner spins and further pulls are ignored. */
  readonly refreshing: boolean;
  /** The finger is down and dragging: the content follows it without a transition. */
  readonly pulling: boolean;
};

/**
 * The refresh the phone home destination currently on screen offers. Destinations whose body
 * another component builds (Worktrees) read it here instead of taking a prop.
 */
export const PullToRefreshContext = createContext<RefreshHandler | undefined>(undefined);

/** True when neither the container nor any scroller between it and the touched node is scrolled. */
function atTop(container: HTMLElement, target: EventTarget | null): boolean {
  let node = target instanceof Node ? target : null;
  while (node && node !== container) {
    if (node instanceof HTMLElement && node.scrollTop > 0) return false;
    node = node.parentNode;
  }
  return container.scrollTop <= 0;
}

function firstTouch(event: TouchEvent): { clientX: number; clientY: number } | null {
  return event.touches[0] ?? event.changedTouches[0] ?? null;
}

/**
 * Style and classes for the content that follows the pull: no transition while the finger drags,
 * a short spring back otherwise, none at all under prefers-reduced-motion.
 */
export function pullContentProps(state: PullToRefreshState): { style: CSSProperties | undefined; className: string } {
  return {
    style: state.pullDistance > 0 ? { transform: `translate3d(0, ${state.pullDistance}px, 0)` } : undefined,
    className: state.pulling ? "" : "transition-transform duration-200 ease-out motion-reduce:transition-none",
  };
}

export function usePullToRefresh(containerRef: RefObject<HTMLElement | null>, onRefresh: RefreshHandler | undefined): PullToRefreshState {
  const [pullDistance, setPullDistance] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [pulling, setPulling] = useState(false);
  const onRefreshRef = useRef(onRefresh);
  onRefreshRef.current = onRefresh;
  const refreshingRef = useRef(false);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const runRefresh = useCallback(() => {
    const handler = onRefreshRef.current;
    if (!handler || refreshingRef.current) return;
    refreshingRef.current = true;
    setRefreshing(true);
    setPullDistance(PULL_HOLD);
    const work = new Promise<unknown>((resolve) => resolve(handler())).catch((err: unknown) => {
      console.warn("Pull to refresh failed", err);
    });
    const minimum = new Promise<void>((resolve) => setTimeout(resolve, REFRESH_MIN_MS));
    void Promise.all([work, minimum]).then(() => {
      refreshingRef.current = false;
      if (!mountedRef.current) return;
      setRefreshing(false);
      setPullDistance(0);
    });
  }, []);

  const enabled = onRefresh !== undefined;

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !enabled) return;

    let startX = 0;
    let startY = 0;
    let tracking = false;
    let decided = false;
    let active = false;
    let distance = 0;

    const onMove = (event: TouchEvent) => {
      if (!tracking) return;
      const touch = firstTouch(event);
      if (!touch) return;
      const dx = touch.clientX - startX;
      const dy = touch.clientY - startY;
      if (!decided) {
        if (Math.abs(dx) < PULL_SLOP && Math.abs(dy) < PULL_SLOP) return;
        decided = true;
        if (dy <= 0 || Math.abs(dx) > Math.abs(dy)) {
          // A scroll or a horizontal swipe: hand the gesture back to the browser untouched.
          tracking = false;
          return;
        }
        active = true;
        setPulling(true);
      }
      if (!active) return;
      distance = Math.min(PULL_MAX, Math.max(0, dy * PULL_RESISTANCE));
      if (event.cancelable) event.preventDefault();
      setPullDistance(distance);
    };

    const reset = () => {
      tracking = false;
      decided = false;
      active = false;
      distance = 0;
    };

    const onStart = (event: TouchEvent) => {
      reset();
      if (refreshingRef.current || event.touches.length !== 1) return;
      if (!atTop(container, event.target)) return;
      const touch = firstTouch(event);
      if (!touch) return;
      startX = touch.clientX;
      startY = touch.clientY;
      tracking = true;
    };

    const finish = (refresh: boolean) => {
      const wasActive = active;
      const reached = distance >= PULL_THRESHOLD;
      reset();
      if (!wasActive) return;
      setPulling(false);
      if (refresh && reached) runRefresh();
      else setPullDistance(0);
    };
    const onEnd = () => finish(true);
    const onCancel = () => finish(false);

    container.addEventListener("touchstart", onStart, { passive: true });
    container.addEventListener("touchmove", onMove, { passive: false });
    container.addEventListener("touchend", onEnd);
    container.addEventListener("touchcancel", onCancel);
    return () => {
      reset();
      container.removeEventListener("touchstart", onStart);
      container.removeEventListener("touchmove", onMove);
      container.removeEventListener("touchend", onEnd);
      container.removeEventListener("touchcancel", onCancel);
      setPulling(false);
      if (!refreshingRef.current) setPullDistance(0);
    };
  }, [containerRef, enabled, runRefresh]);

  return { pullDistance, refreshing, pulling };
}
