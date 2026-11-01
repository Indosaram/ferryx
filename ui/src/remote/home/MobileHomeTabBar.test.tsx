import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MobileHomeTabBar } from "./MobileHomeTabBar";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("MobileHomeTabBar", () => {
  it("vibrates once when a different tab is picked, and not for the tab already shown", () => {
    const vibrate = vi.fn(() => true);
    vi.stubGlobal("navigator", { ...navigator, vibrate });
    const onSelect = vi.fn();
    render(<MobileHomeTabBar active="supervisor" inboxCount={0} onSelect={onSelect} />);

    fireEvent.click(screen.getByTestId("mobile-home-tab-supervisor"));
    expect(vibrate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("mobile-home-tab-inbox"));
    expect(vibrate).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenLastCalledWith("inbox");
  });

  it("vibrates when the arrow keys move to another tab", () => {
    const vibrate = vi.fn(() => true);
    vi.stubGlobal("navigator", { ...navigator, vibrate });
    const onSelect = vi.fn();
    render(<MobileHomeTabBar active="supervisor" inboxCount={0} onSelect={onSelect} />);

    fireEvent.keyDown(screen.getByTestId("mobile-home-tabs"), { key: "ArrowRight" });
    expect(vibrate).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenLastCalledWith("inbox");
  });
});
