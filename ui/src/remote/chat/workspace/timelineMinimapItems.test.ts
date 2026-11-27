import { describe, expect, it } from "vitest";
import type { MobileChatMessageProps } from "../MobileChatMessage";
import {
  deriveTimelineMinimapItems,
  resolveTimelineMinimapCurrentIndex,
  resolveTimelineMinimapHeightStyle,
  resolveTimelineMinimapIndexFromPointer,
  resolveTimelineMinimapTopPercent,
  TIMELINE_MINIMAP_MIN_ITEMS,
} from "./timelineMinimapItems";

function turn(id: string, role: "user" | "assistant", content: string): MobileChatMessageProps {
  return { id, role, content, timestamp: 1 };
}

describe("deriveTimelineMinimapItems", () => {
  it("pairs each user turn with the assistant text that answers it", () => {
    const items = deriveTimelineMinimapItems([
      turn("u1", "user", "First ask"),
      turn("a1", "assistant", "First answer"),
      turn("u2", "user", "Second ask"),
      turn("a2", "assistant", "Second answer"),
    ]);
    expect(items).toEqual([
      { id: "u1", userText: "First ask", assistantText: "First answer" },
      { id: "u2", userText: "Second ask", assistantText: "Second answer" },
    ]);
  });

  it("skips assistant-only noise and stops the pairing at the next user turn", () => {
    const items = deriveTimelineMinimapItems([
      turn("a0", "assistant", "Opening remark"),
      turn("u1", "user", "Ask"),
      turn("a1", "assistant", "Answer"),
      turn("a2", "assistant", "Follow-up detail"),
    ]);
    expect(items).toEqual([{ id: "u1", userText: "Ask", assistantText: "Follow-up detail" }]);
  });
});

describe("minimap geometry", () => {
  it("shows the rail from two markers on and caps its height", () => {
    expect(TIMELINE_MINIMAP_MIN_ITEMS).toBe(2);
    expect(resolveTimelineMinimapHeightStyle(2)).toBe("8px");
    expect(resolveTimelineMinimapHeightStyle(10)).toBe("72px");
    expect(resolveTimelineMinimapHeightStyle(100)).toBe("320px");
  });

  it("spreads markers evenly with the ends on their half gap", () => {
    expect(resolveTimelineMinimapTopPercent(0, 4)).toBe(0);
    expect(resolveTimelineMinimapTopPercent(3, 4)).toBe(100);
    expect(resolveTimelineMinimapTopPercent(1, 4)).toBeCloseTo(100 / 3, 10);
    expect(resolveTimelineMinimapTopPercent(7, 1)).toBe(0);
  });

  it("maps a pointer to the nearest marker and clamps", () => {
    expect(resolveTimelineMinimapIndexFromPointer({ itemCount: 5, railTop: 100, railHeight: 200, pointerY: 100 })).toBe(0);
    expect(resolveTimelineMinimapIndexFromPointer({ itemCount: 5, railTop: 100, railHeight: 200, pointerY: 200 })).toBe(2);
    expect(resolveTimelineMinimapIndexFromPointer({ itemCount: 5, railTop: 100, railHeight: 200, pointerY: 400 })).toBe(4);
    expect(resolveTimelineMinimapIndexFromPointer({ itemCount: 0, railTop: 0, railHeight: 0, pointerY: 0 })).toBe(0);
  });
});

describe("resolveTimelineMinimapCurrentIndex", () => {
  const items = deriveTimelineMinimapItems([
    turn("u1", "user", "One"),
    turn("a1", "assistant", "One answer"),
    turn("u2", "user", "Two"),
    turn("a2", "assistant", "Two answer"),
    turn("u3", "user", "Three"),
    turn("a3", "assistant", "Three answer"),
  ]);
  const topById = new Map<string, number>([["u1", 0], ["u2", 500], ["u3", 900]]);

  it("reads the last turn whose user message starts above the viewport middle", () => {
    expect(resolveTimelineMinimapCurrentIndex({ items, topById, scrollTop: 550, viewportHeight: 200 })).toBe(1);
    expect(resolveTimelineMinimapCurrentIndex({ items, topById, scrollTop: 0, viewportHeight: 200 })).toBe(0);
    expect(resolveTimelineMinimapCurrentIndex({ items, topById, scrollTop: 900, viewportHeight: 200 })).toBe(2);
  });

  it("reads as latest when nothing is measured", () => {
    expect(resolveTimelineMinimapCurrentIndex({ items, topById: new Map(), scrollTop: 0, viewportHeight: 200 })).toBeNull();
    expect(resolveTimelineMinimapCurrentIndex({ items: [], topById: new Map(), scrollTop: 0, viewportHeight: 200 })).toBeNull();
  });
});
