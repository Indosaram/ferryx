import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";

/**
 * Windowed (virtualized) rendering for long lists of fixed-pitch rows, used by the
 * file preview's source view and the CSV table/raw views.
 *
 * Model: every row has the same pitch (`rowHeight`). Only rows in [start, end) are
 * mounted; the caller renders a top spacer of `padTop` px and a bottom spacer of
 * `padBottom` px around them, so the scroll height always equals the full document.
 * The pitch starts at `estimatedRowHeight` and is replaced by the average distance
 * between the first and last mounted rows (elements carrying `data-line-window-row`
 * inside the `listRef` element) once real layout exists.
 *
 * jsdom has no layout: clientHeight is 0 and every rect is empty, so the estimate is
 * kept and the viewport falls back to `LINE_WINDOW_FALLBACK_ROWS` rows. Tests move
 * the window by assigning `scrollTop` on the scroller and dispatching `scroll`.
 *
 * Printing: between `beforeprint` and `afterprint` (and while the `print` media query
 * matches) the window is disabled, so every row is mounted and the print stylesheet
 * can paginate the whole document.
 */

/** Viewport assumed when the scroller reports no height (jsdom, not laid out yet). */
export const LINE_WINDOW_FALLBACK_ROWS = 40;
/** Rows mounted above and below the visible range. */
export const LINE_WINDOW_OVERSCAN = 20;
/** Attribute that marks a windowed row; the row pitch is measured from these. */
export const LINE_WINDOW_ROW_ATTRIBUTE = "data-line-window-row";

export type LineWindowOptions = {
  /** Total number of rows in the document. */
  count: number;
  /** Window only when true (callers keep small documents fully rendered). */
  enabled: boolean;
  /** Row pitch in px used until a real measurement is available. */
  estimatedRowHeight: number;
  overscan?: number;
};

export type LineWindow = {
  /** True when rows are actually windowed (enabled and not printing). */
  active: boolean;
  /** First mounted row index (inclusive). */
  start: number;
  /** Last mounted row index (exclusive). */
  end: number;
  /** Height in px of the spacer before the first mounted row. */
  padTop: number;
  /** Height in px of the spacer after the last mounted row. */
  padBottom: number;
  rowHeight: number;
  /** Callback ref for the scrolling element. */
  scrollRef: (element: HTMLElement | null) => void;
  /** Callback ref for the element whose top edge is row 0 (it holds the spacers and rows). */
  listRef: (element: HTMLElement | null) => void;
  /**
   * Scrolls by arithmetic (row-0 offset + index * rowHeight), so it works when the row is
   * not mounted, and moves the window immediately so the row mounts on the next commit.
   * Returns false when the scroller is not attached yet.
   */
  scrollToIndex: (index: number, align?: "center" | "start") => boolean;
};

/** True while the document is being printed. */
export function usePrintMode(): boolean {
  const [printing, setPrinting] = useState(false);
  useEffect(() => {
    if (typeof window === "undefined") return;
    // The engine lays the page out for print right after `beforeprint` returns, so the
    // full render has to be committed synchronously inside the listener.
    const enter = () => flushSync(() => setPrinting(true));
    const leave = () => setPrinting(false);
    window.addEventListener("beforeprint", enter);
    window.addEventListener("afterprint", leave);
    let query: MediaQueryList | null = null;
    const onMediaChange = (event: MediaQueryListEvent) => {
      if (event.matches) enter();
      else leave();
    };
    if (typeof window.matchMedia === "function") {
      const candidate = window.matchMedia("print");
      if (candidate && typeof candidate.addEventListener === "function") {
        query = candidate;
        query.addEventListener("change", onMediaChange);
      }
    }
    return () => {
      window.removeEventListener("beforeprint", enter);
      window.removeEventListener("afterprint", leave);
      query?.removeEventListener("change", onMediaChange);
    };
  }, []);
  return printing;
}

export function useLineWindow({
  count,
  enabled,
  estimatedRowHeight,
  overscan = LINE_WINDOW_OVERSCAN,
}: LineWindowOptions): LineWindow {
  const printing = usePrintMode();
  const active = enabled && !printing;

  const [scrollElement, setScrollElement] = useState<HTMLElement | null>(null);
  const [listElement, setListElement] = useState<HTMLElement | null>(null);
  const [rowHeight, setRowHeight] = useState(estimatedRowHeight);
  const [viewport, setViewport] = useState(0);
  // Index of the row at the top of the viewport. Stored as a row index rather than raw
  // scrollTop so scroll events within one row do not re-render.
  const [anchor, setAnchor] = useState(0);
  const rowHeightRef = useRef(estimatedRowHeight);
  /** Distance in px from the scroller's content top to row 0 (padding, sticky headers). */
  const offsetRef = useRef(0);

  const readAnchor = useCallback((element: HTMLElement) => {
    setAnchor(Math.max(0, Math.floor((element.scrollTop - offsetRef.current) / rowHeightRef.current)));
  }, []);

  const measure = useCallback(() => {
    if (!scrollElement) return;
    if (listElement) {
      const listRect = listElement.getBoundingClientRect();
      // Zero-size rects mean there is no layout (jsdom, detached, display:none): keep estimates.
      if (listRect.height > 0) {
        const rows = listElement.querySelectorAll<HTMLElement>(`[${LINE_WINDOW_ROW_ATTRIBUTE}]`);
        let pitch = 0;
        if (rows.length >= 2) {
          const first = rows[0].getBoundingClientRect();
          const last = rows[rows.length - 1].getBoundingClientRect();
          pitch = (last.top - first.top) / (rows.length - 1);
        } else if (rows.length === 1) {
          pitch = rows[0].getBoundingClientRect().height;
        }
        if (pitch > 0 && Math.abs(pitch - rowHeightRef.current) > 0.01) {
          rowHeightRef.current = pitch;
          setRowHeight(pitch);
        }
        const scrollerTop = scrollElement.getBoundingClientRect().top + scrollElement.clientTop;
        offsetRef.current = Math.max(0, listRect.top - scrollerTop + scrollElement.scrollTop);
      }
    }
    setViewport(scrollElement.clientHeight);
    readAnchor(scrollElement);
  }, [listElement, readAnchor, scrollElement]);

  // Re-measure whenever windowing turns on, the elements change, or the row count changes.
  useLayoutEffect(() => {
    if (active) measure();
  }, [active, measure, count]);

  useEffect(() => {
    if (!active || !scrollElement) return;
    const element = scrollElement;
    const onScroll = () => readAnchor(element);
    element.addEventListener("scroll", onScroll, { passive: true });
    let observer: ResizeObserver | null = null;
    if (typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(() => measure());
      observer.observe(element);
    }
    return () => {
      element.removeEventListener("scroll", onScroll);
      observer?.disconnect();
    };
  }, [active, measure, readAnchor, scrollElement]);

  const scrollToIndex = useCallback(
    (index: number, align: "center" | "start" = "center") => {
      const element = scrollElement;
      if (!element) return false;
      const pitch = rowHeightRef.current;
      const view = element.clientHeight > 0 ? element.clientHeight : LINE_WINDOW_FALLBACK_ROWS * pitch;
      const top = offsetRef.current + index * pitch - (align === "center" ? (view - pitch) / 2 : 0);
      element.scrollTop = Math.max(0, top);
      // A programmatic scroll fires `scroll` asynchronously (and never in jsdom), so move
      // the window now instead of waiting for the event.
      readAnchor(element);
      return true;
    },
    [readAnchor, scrollElement],
  );

  if (!active) {
    return {
      active,
      start: 0,
      end: count,
      padTop: 0,
      padBottom: 0,
      rowHeight,
      scrollRef: setScrollElement,
      listRef: setListElement,
      scrollToIndex,
    };
  }

  const viewportPx = viewport > 0 ? viewport : LINE_WINDOW_FALLBACK_ROWS * rowHeight;
  const viewportRows = Math.ceil(viewportPx / rowHeight);
  const start = Math.min(count, Math.max(0, anchor - overscan));
  const end = Math.min(count, Math.max(start, anchor + viewportRows + overscan));
  return {
    active,
    start,
    end,
    padTop: start * rowHeight,
    padBottom: (count - end) * rowHeight,
    rowHeight,
    scrollRef: setScrollElement,
    listRef: setListElement,
    scrollToIndex,
  };
}
