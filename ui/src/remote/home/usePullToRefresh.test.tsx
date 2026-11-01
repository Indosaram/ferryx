import { act, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PullToRefreshIndicator } from "./PullToRefreshIndicator";
import { REFRESH_MIN_MS, usePullToRefresh, type RefreshHandler } from "./usePullToRefresh";

function Harness({ onRefresh }: { onRefresh: RefreshHandler }) {
  const ref = useRef<HTMLDivElement>(null);
  const state = usePullToRefresh(ref, onRefresh);
  return (
    <div ref={ref} data-testid="list" data-distance={state.pullDistance} data-refreshing={String(state.refreshing)}>
      <PullToRefreshIndicator {...state} />
      <button type="button">row</button>
    </div>
  );
}

const list = () => screen.getByTestId("list");
const touches = (x: number, y: number) => [{ clientX: x, clientY: y }];

/** One finger from (100, 100) to (100 + dx, 100 + dy) in four moves, then lifted. */
function pull(dy: number, dx = 0, release = true) {
  fireEvent.touchStart(list(), { touches: touches(100, 100) });
  for (let step = 1; step <= 4; step += 1) {
    fireEvent.touchMove(list(), { touches: touches(100 + (dx * step) / 4, 100 + (dy * step) / 4) });
  }
  if (release) fireEvent.touchEnd(list(), { touches: [], changedTouches: touches(100 + dx, 100 + dy) });
}

function setScrollTop(value: number) {
  Object.defineProperty(list(), "scrollTop", { configurable: true, value });
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("usePullToRefresh", () => {
  it("refreshes once when a pull from the top passes the threshold and holds the spinner until it settles", async () => {
    let settle: () => void = () => undefined;
    const onRefresh = vi.fn(() => new Promise<void>((resolve) => { settle = resolve; }));
    render(<Harness onRefresh={onRefresh} />);
    pull(160);
    expect(onRefresh).toHaveBeenCalledTimes(1);
    expect(list().dataset.refreshing).toBe("true");
    expect(screen.getByTestId("pull-refresh-indicator")).toHaveTextContent("Refreshing");
    expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");

    // The promise settling early still leaves the spinner up for the minimum time.
    await act(async () => {
      settle();
      await vi.advanceTimersByTimeAsync(REFRESH_MIN_MS - 1);
    });
    expect(list().dataset.refreshing).toBe("true");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
      await Promise.resolve();
    });
    expect(list().dataset.refreshing).toBe("false");
    expect(list().dataset.distance).toBe("0");
  });

  it("tracks the dampened distance while pulling", () => {
    render(<Harness onRefresh={vi.fn()} />);
    pull(120, 0, false);
    expect(Number(list().dataset.distance)).toBe(60);
  });

  it("springs back without refreshing below the threshold", () => {
    const onRefresh = vi.fn();
    render(<Harness onRefresh={onRefresh} />);
    pull(40);
    expect(onRefresh).not.toHaveBeenCalled();
    expect(list().dataset.distance).toBe("0");
  });

  it("does not start a pull when the list is scrolled", () => {
    const onRefresh = vi.fn();
    render(<Harness onRefresh={onRefresh} />);
    setScrollTop(100);
    pull(160);
    expect(onRefresh).not.toHaveBeenCalled();
    expect(list().dataset.distance).toBe("0");
  });

  it("ignores further pulls while a refresh is pending", async () => {
    const onRefresh = vi.fn(() => new Promise<void>(() => undefined));
    render(<Harness onRefresh={onRefresh} />);
    pull(160);
    pull(160);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(REFRESH_MIN_MS * 3);
    });
    pull(160);
    expect(onRefresh).toHaveBeenCalledTimes(1);
    expect(list().dataset.refreshing).toBe("true");
  });

  it("never activates for a tap, an upward drag or a mostly horizontal swipe", () => {
    const onRefresh = vi.fn();
    render(<Harness onRefresh={onRefresh} />);
    pull(6);
    pull(-160);
    pull(160, 200);
    expect(onRefresh).not.toHaveBeenCalled();
    expect(list().dataset.distance).toBe("0");
  });

  it("only prevents the native scroll while a pull is active", () => {
    render(<Harness onRefresh={vi.fn()} />);
    fireEvent.touchStart(list(), { touches: touches(100, 100) });
    expect(fireEvent.touchMove(list(), { touches: touches(100, 104) })).toBe(true);
    expect(fireEvent.touchMove(list(), { touches: touches(100, 140) })).toBe(false);
    fireEvent.touchEnd(list(), { touches: [], changedTouches: touches(100, 140) });
    setScrollTop(50);
    fireEvent.touchStart(list(), { touches: touches(100, 100) });
    expect(fireEvent.touchMove(list(), { touches: touches(100, 160) })).toBe(true);
  });
});
