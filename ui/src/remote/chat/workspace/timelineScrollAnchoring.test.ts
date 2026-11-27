import { describe, expect, it } from "vitest";
import {
  anchoredScrollTop,
  FOLLOW_THRESHOLD_PX,
  isViewportAtBottom,
  snapshotViewport,
} from "./timelineScrollAnchoring";

function messages(count: number, prefix = "m"): { id: string }[] {
  return Array.from({ length: count }, (_, index) => ({ id: `${prefix}-${index}` }));
}

describe("timelineScrollAnchoring", () => {
  it("reads the bottom within the same threshold the pill uses", () => {
    expect(FOLLOW_THRESHOLD_PX).toBe(80);
    expect(isViewportAtBottom(700, 1000, 300)).toBe(true);
    expect(isViewportAtBottom(620, 1000, 300)).toBe(false);
  });

  it("snapshots the first id and count of the current page", () => {
    expect(snapshotViewport(10, 900, 300, messages(3))).toEqual({
      top: 10,
      height: 900,
      count: 3,
      atBottom: false,
      firstId: "m-0",
    });
    expect(snapshotViewport(0, 0, 300, [])).toEqual({
      top: 0,
      height: 0,
      count: 0,
      atBottom: true,
      firstId: null,
    });
  });

  it("anchors only a prepended older page above a reader away from the bottom", () => {
    const older = messages(2, "old");
    const current = messages(4, "new");
    const previous: Parameters<typeof anchoredScrollTop>[0] = {
      top: 100,
      height: 1000,
      count: 4,
      atBottom: false,
      firstId: "new-0",
    };
    expect(anchoredScrollTop(previous, [...older, ...current], 1200, 100)).toBe(300);
  });

  it("keeps the bottom-follow rule when the change is not a prepended page", () => {
    const current = messages(4, "new");
    const base: Parameters<typeof anchoredScrollTop>[0] = {
      top: 100,
      height: 1000,
      count: 4,
      atBottom: false,
      firstId: "new-0",
    };
    // nothing changed before
    expect(anchoredScrollTop(base, current, 1000, 100)).toBeNull();
    // the reader sat at the bottom: appending keeps the follow rule
    expect(anchoredScrollTop({ ...base, atBottom: true }, [...current, { id: "new-9" }], 1200, 300)).toBeNull();
    // the old first turn is gone (page replaced, not prepended)
    expect(anchoredScrollTop(base, messages(4, "other"), 1200, 100)).toBeNull();
    // content did not grow
    expect(anchoredScrollTop(base, [...messages(2, "old"), ...current], 1000, 100)).toBeNull();
  });
});
