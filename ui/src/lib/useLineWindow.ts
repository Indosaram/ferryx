import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";

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
  contentKey?: unknown;
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
  /** Seeks using cumulative offsets, then corrects after the target row is measured. */
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
  contentKey,
}: LineWindowOptions): LineWindow {
  const printing = usePrintMode();
  const active = enabled && !printing;
  const [scrollElement, setScrollElement] = useState<HTMLElement | null>(null);
  const [listElement, setListElement] = useState<HTMLElement | null>(null);
  const heights = useRef(new Map<number, number>());
  const [revision, setRevision] = useState(0);
  const [top, setTop] = useState(0);
  const [viewport, setViewport] = useState(0);
  const offsetRef = useRef(0);
  const pendingSeek = useRef<{ index: number; align: "center" | "start" } | null>(null);
  const pendingAnchor = useRef<{ anchor: number; intraRow: number; revision: number } | null>(null);
  const offsets = useMemo(() => {
    const result = new Array<number>(count + 1).fill(0);
    for (let index = 0; index < count; index++) {
      result[index + 1] = result[index] + (heights.current.get(index) ?? estimatedRowHeight);
    }
    return result;
  }, [count, estimatedRowHeight, revision]);
  const findRow = useCallback((position: number) => {
    let low = 0;
    let high = count;
    while (low < high) {
      const middle = Math.floor((low + high) / 2);
      if (offsets[middle + 1] <= position) low = middle + 1;
      else high = middle;
    }
    return Math.min(low, Math.max(0, count - 1));
  }, [count, offsets]);
  const viewHeight = viewport || LINE_WINDOW_FALLBACK_ROWS * estimatedRowHeight;
  const anchor = Math.min(pendingAnchor.current?.anchor ?? findRow(top), Math.max(0, count - 1));
  const logicalPosition = useRef({ anchor, intraRow: 0 });
  if (!pendingAnchor.current) logicalPosition.current = { anchor, intraRow: Math.max(0, top - offsets[anchor]) };
  const start = active ? Math.max(0, anchor - overscan) : 0;
  const anchorInset = Math.min(pendingAnchor.current?.intraRow ?? 0, Math.max(0, offsets[anchor + 1] - offsets[anchor] - 1));
  const windowTop = pendingAnchor.current ? offsets[anchor] + anchorInset : top;
  const end = active ? Math.min(count, findRow(windowTop + viewHeight) + overscan + 1) : count;
  const updateScroll = useCallback(() => {
    if (!scrollElement) return;
    if (pendingAnchor.current) return;
    setTop(Math.max(0, scrollElement.scrollTop - offsetRef.current));
    setViewport(scrollElement.clientHeight);
  }, [scrollElement]);
  const seek = useCallback((index: number, align: "center" | "start") => {
    if (!scrollElement || !count) return false;
    const row = Math.max(0, Math.min(count - 1, index));
    const height = offsets[row + 1] - offsets[row];
    const position = offsets[row] - (align === "center" ? Math.max(0, (viewHeight - height) / 2) : 0);
    scrollElement.scrollTop = Math.max(0, offsetRef.current + position);
    updateScroll();
    return true;
  }, [count, offsets, scrollElement, updateScroll, viewHeight]);
  const scrollToIndex = useCallback((index: number, align: "center" | "start" = "center") => {
    pendingSeek.current = { index, align };
    return seek(index, align);
  }, [seek]);
  useLayoutEffect(() => {
    pendingAnchor.current = { ...logicalPosition.current, revision };
    heights.current.clear();
    setRevision((value) => value + 1);
  }, [contentKey, count, estimatedRowHeight]);
  useLayoutEffect(() => {
    if (!active || !scrollElement || !listElement) return;
    if (pendingAnchor.current?.revision === revision) return;
    const rect = listElement.getBoundingClientRect();
    if (rect.height > 0) {
      offsetRef.current = Math.max(0, rect.top - scrollElement.getBoundingClientRect().top - scrollElement.clientTop + scrollElement.scrollTop);
    }
    const rows = listElement.querySelectorAll<HTMLElement>(`[${LINE_WINDOW_ROW_ATTRIBUTE}]`);
    let changed = false;
    let anchorShift = 0;
    rows.forEach((row, position) => {
      const index = start + position;
      const height = row.getBoundingClientRect().height;
      const previous = heights.current.get(index) ?? estimatedRowHeight;
      if (height > 0 && Math.abs(previous - height) > 0.5) {
        heights.current.set(index, height);
        if (index < anchor) anchorShift += height - previous;
        changed = true;
      }
    });
    if (changed) {
      if (!pendingAnchor.current) scrollElement.scrollTop += anchorShift;
      setRevision((value) => value + 1);
    } else if (pendingAnchor.current) {
      scrollElement.scrollTop = offsetRef.current + offsets[anchor] + anchorInset;
      pendingAnchor.current = null;
    } else if (pendingSeek.current) {
      const target = pendingSeek.current;
      seek(target.index, target.align);
      if (target.index >= start && target.index < end) pendingSeek.current = null;
    }
    updateScroll();
  });
  useEffect(() => {
    if (!active || !scrollElement) return;
    let width = scrollElement.clientWidth;
    scrollElement.addEventListener("scroll", updateScroll, { passive: true });
    let observer: ResizeObserver | undefined;
    if (typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(() => {
        if (width !== scrollElement.clientWidth) {
          width = scrollElement.clientWidth;
          pendingAnchor.current = { ...logicalPosition.current, revision: -1 };
          heights.current.clear();
          setRevision((value) => value + 1);
        }
        updateScroll();
      });
      observer.observe(scrollElement);
    }
    return () => {
      scrollElement.removeEventListener("scroll", updateScroll);
      observer?.disconnect();
    };
  }, [active, estimatedRowHeight, scrollElement, updateScroll]);
  return {
    active,
    start,
    end,
    padTop: active ? offsets[start] : 0,
    padBottom: active ? offsets[count] - offsets[end] : 0,
    rowHeight: estimatedRowHeight,
    scrollRef: setScrollElement,
    listRef: setListElement,
    scrollToIndex,
  };
}
