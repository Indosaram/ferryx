import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useEdgeSwipeBack } from "./useEdgeSwipeBack";

function Screen(props: { enabled: boolean; forceEnabled?: boolean; onBack: () => void }) {
  const ref = useEdgeSwipeBack(props);
  return <div ref={ref} data-testid="screen" />;
}

/** A mouse-type pointer event (touch-type pointers defer to touch events where those exist). */
function pointer(type: string, clientX: number, clientY = 200) {
  const event = new PointerEvent(type, { bubbles: true, cancelable: true, clientX, clientY, button: 0 });
  Object.defineProperty(event, "pointerId", { value: 1 });
  Object.defineProperty(event, "pointerType", { value: "mouse" });
  return event;
}

/** Drags from startX to endX in 20px steps, with time standing still between moves (no fling). */
function drag(target: Element, startX: number, endX: number) {
  target.dispatchEvent(pointer("pointerdown", startX));
  for (let x = startX + 20; x < endX; x += 20) target.dispatchEvent(pointer("pointermove", x));
  target.dispatchEvent(pointer("pointermove", endX));
  target.dispatchEvent(pointer("pointerup", endX));
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("useEdgeSwipeBack", () => {
  it("calls onBack once for a drag that starts at the left edge and travels far enough", () => {
    const onBack = vi.fn();
    const { getByTestId } = render(<Screen enabled onBack={onBack} />);
    const screen = getByTestId("screen");
    drag(screen, 10, 130);
    expect(onBack).toHaveBeenCalledTimes(1);
    // The release ends the gesture: a stray move or second release does nothing more.
    screen.dispatchEvent(pointer("pointermove", 200));
    screen.dispatchEvent(pointer("pointerup", 200));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("follows the finger while dragging", () => {
    const { getByTestId } = render(<Screen enabled onBack={vi.fn()} />);
    const screen = getByTestId("screen");
    screen.dispatchEvent(pointer("pointerdown", 10));
    screen.dispatchEvent(pointer("pointermove", 60));
    expect(screen.style.transform).toContain("50px");
    expect(screen.style.transition).toBe("none");
  });

  it("springs back without going back when the drag is too short", () => {
    const clock = vi.spyOn(performance, "now");
    clock.mockReturnValue(0);
    const onBack = vi.fn();
    const { getByTestId } = render(<Screen enabled onBack={onBack} />);
    const screen = getByTestId("screen");
    screen.dispatchEvent(pointer("pointerdown", 10));
    clock.mockReturnValue(500);
    screen.dispatchEvent(pointer("pointermove", 50));
    clock.mockReturnValue(1000);
    screen.dispatchEvent(pointer("pointerup", 50));
    expect(onBack).not.toHaveBeenCalled();
    expect(screen.style.transform).toBe("");
  });

  it("ignores a drag that starts away from the edge", () => {
    const onBack = vi.fn();
    const { getByTestId } = render(<Screen enabled onBack={onBack} />);
    const screen = getByTestId("screen");
    drag(screen, 120, 320);
    expect(onBack).not.toHaveBeenCalled();
    expect(screen.style.transform).toBe("");
  });

  it("does nothing while disabled, and installs when forced", () => {
    const onBack = vi.fn();
    const { getByTestId, rerender } = render(<Screen enabled={false} onBack={onBack} />);
    drag(getByTestId("screen"), 10, 130);
    expect(onBack).not.toHaveBeenCalled();
    rerender(<Screen enabled={false} forceEnabled onBack={onBack} />);
    drag(getByTestId("screen"), 10, 130);
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});
