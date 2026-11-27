import { describe, expect, it } from "vitest";
import { advanceVisit, EMPTY_VISIT } from "./timelineWatermark";

function ids(count: number, prefix = "m"): { id: string }[] {
  return Array.from({ length: count }, (_, index) => ({ id: `${prefix}-${index}` }));
}

describe("timelineWatermark (t3code thread-visit model)", () => {
  it("marks everything read while the reader is at the bottom", () => {
    expect(advanceVisit(EMPTY_VISIT, ids(3), true)).toEqual({ lastSeenId: "m-2", watermarkId: null });
  });

  it("puts the watermark at the first message that arrived while the reader was away", () => {
    const seen = advanceVisit(EMPTY_VISIT, ids(3), true);
    const grown = [...ids(3), { id: "m-3" }, { id: "m-4" }];
    expect(advanceVisit(seen, grown, false)).toEqual({ lastSeenId: "m-2", watermarkId: "m-3" });
  });

  it("clears the watermark and advances when the reader returns to the bottom", () => {
    const seen = advanceVisit(EMPTY_VISIT, ids(3), true);
    const away = advanceVisit(seen, [...ids(3), { id: "m-3" }], false);
    expect(advanceVisit(away, [...ids(3), { id: "m-3" }, { id: "m-4" }], true)).toEqual({
      lastSeenId: "m-4",
      watermarkId: null,
    });
  });

  it("keeps the watermark across an older-page prepend that drops the seen marker", () => {
    const seen = advanceVisit(EMPTY_VISIT, ids(3), true);
    const away = advanceVisit(seen, [...ids(3), { id: "m-3" }], false);
    const prepended = [...ids(2, "old"), ...ids(3), { id: "m-3" }];
    expect(advanceVisit(away, prepended, false)).toEqual({ lastSeenId: "m-2", watermarkId: "m-3" });
  });

  it("treats a fresh page with no visit history as fully read instead of marking everything unread", () => {
    expect(advanceVisit(EMPTY_VISIT, ids(5), false)).toEqual({ lastSeenId: "m-4", watermarkId: null });
  });
});
