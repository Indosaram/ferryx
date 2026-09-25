import { describe, expect, it } from "vitest";

import { POPOVER_WIDTH, resolvePopoverPlacement } from "./NotificationCenterPopover";

const VIEWPORT = { width: 1200, height: 900 };

describe("resolvePopoverPlacement", () => {
  it("opens below a trigger in the sidebar top row instead of above the window", () => {
    const placement = resolvePopoverPlacement(
      { top: 10, bottom: 32, left: 220 },
      VIEWPORT,
      POPOVER_WIDTH,
    );

    expect(placement.top).toBe(38);
    expect(placement.bottom).toBeUndefined();
    expect(placement.left).toBe(220);
    expect(placement.maxHeight).toBe(VIEWPORT.height - 32 - 6 - 8);
  });

  it("keeps opening above a trigger in the bottom strip", () => {
    const placement = resolvePopoverPlacement(
      { top: 858, bottom: 880, left: 8 },
      VIEWPORT,
      POPOVER_WIDTH,
    );

    expect(placement.bottom).toBe(VIEWPORT.height - 858 + 6);
    expect(placement.top).toBeUndefined();
  });

  it("clamps the left edge into the viewport", () => {
    expect(resolvePopoverPlacement({ top: 10, bottom: 32, left: -50 }, VIEWPORT, POPOVER_WIDTH).left).toBe(8);
    expect(resolvePopoverPlacement({ top: 10, bottom: 32, left: 1190 }, VIEWPORT, POPOVER_WIDTH).left).toBe(
      VIEWPORT.width - POPOVER_WIDTH - 8,
    );
  });

  it("never returns a height too small to render the header", () => {
    const placement = resolvePopoverPlacement({ top: 900, bottom: 902, left: 8 }, VIEWPORT, POPOVER_WIDTH);
    expect(placement.maxHeight).toBeGreaterThanOrEqual(120);
  });
});
