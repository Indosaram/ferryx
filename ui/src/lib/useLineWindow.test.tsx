import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useLineWindow } from "./useLineWindow";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function Fixture() {
  const window = useLineWindow({ count: 1000, enabled: true, estimatedRowHeight: 20, overscan: 2 });
  return <>
    <button onClick={() => window.scrollToIndex(800, "start")}>Seek</button>
    <div ref={window.scrollRef} data-testid="scroll">
      <div ref={window.listRef}>
        <div data-testid="top" style={{ height: window.padTop }} />
        {Array.from({ length: window.end - window.start }, (_, offset) => {
          const index = window.start + offset;
          return <div key={index} data-line-window-row="" data-index={index}>{index}</div>;
        })}
        <div data-testid="bottom" style={{ height: window.padBottom }} />
      </div>
    </div>
  </>;
}

it("uses measured wrapped row heights when seeking and scrolling", () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const index = this.getAttribute("data-index");
    return new DOMRect(0, 0, 200, index === null ? 0 : Number(index) % 2 === 0 ? 60 : 20);
  });
  render(<Fixture />);
  fireEvent.click(screen.getByText("Seek"));
  const scroller = screen.getByTestId("scroll");
  expect(scroller.querySelector('[data-index="800"]')).not.toBeNull();
  expect(scroller.scrollTop).toBeGreaterThan(800 * 20);
  expect(scroller.querySelectorAll("[data-line-window-row]").length).toBeLessThan(100);
  act(() => { scroller.scrollTop = 0; fireEvent.scroll(scroller); });
  expect(scroller.querySelector('[data-index="0"]')).not.toBeNull();
  expect(Number.parseFloat(screen.getByTestId("top").style.height)).toBe(0);
});

it("keeps the logical row visible after width changes invalidate measurements", () => {
  let width = 400;
  let notifyResize = () => {};
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { notifyResize = callback; }
    observe() {}
    disconnect() {}
    unobserve() {}
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => width);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return new DOMRect(0, 0, width, this.hasAttribute("data-index") ? (width === 400 ? 20 : 60) : 0);
  });
  render(<Fixture />);
  fireEvent.click(screen.getByText("Seek"));
  act(() => { width = 200; notifyResize(); });
  const scroller = screen.getByTestId("scroll");
  expect(scroller.querySelector('[data-index="800"]')).not.toBeNull();
  expect(scroller.querySelectorAll("[data-line-window-row]").length).toBeLessThan(100);
});

it("bounds an intra-row anchor when a tall wrapped row shrinks", () => {
  let width = 200;
  let notifyResize = () => {};
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { notifyResize = callback; }
    observe() {}
    disconnect() {}
    unobserve() {}
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => width);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return new DOMRect(0, 0, width, this.hasAttribute("data-index") ? (width === 200 ? 200 : 20) : 0);
  });
  render(<Fixture />);
  fireEvent.click(screen.getByText("Seek"));
  const scroller = screen.getByTestId("scroll");
  act(() => { scroller.scrollTop += 180; fireEvent.scroll(scroller); });
  act(() => { width = 400; notifyResize(); });
  expect(scroller.scrollTop).toBe(800 * 20 + 19);
  expect(scroller.querySelector('[data-index="800"]')).not.toBeNull();
});
