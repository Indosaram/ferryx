import { describe, expect, it } from "vitest";

import { isRedundantSplit, type PaneNode } from "./paneTree";

const twoPaneVertical: PaneNode = {
  type: "split",
  direction: "vertical",
  first: { type: "leaf", leafId: "A" },
  second: { type: "leaf", leafId: "B" },
  ratio: 0.5,
};

const twoPaneHorizontal: PaneNode = { ...twoPaneVertical, direction: "horizontal" };

const threePaneNested: PaneNode = {
  type: "split",
  direction: "horizontal",
  first: { type: "leaf", leafId: "A" },
  second: {
    type: "split",
    direction: "vertical",
    first: { type: "leaf", leafId: "B" },
    second: { type: "leaf", leafId: "C" },
    ratio: 0.5,
  },
  ratio: 0.5,
};

describe("isRedundantSplit", () => {
  it("rejects re-splitting a sibling toward the side the pane already occupies", () => {
    expect(isRedundantSplit(twoPaneVertical, "A", "B", "vertical", "first")).toBe(true);
    expect(isRedundantSplit(twoPaneVertical, "B", "A", "vertical", "second")).toBe(true);
    expect(isRedundantSplit(twoPaneHorizontal, "A", "B", "horizontal", "first")).toBe(true);
    expect(isRedundantSplit(twoPaneHorizontal, "B", "A", "horizontal", "second")).toBe(true);
  });

  it("allows swapping siblings along the same axis", () => {
    expect(isRedundantSplit(twoPaneVertical, "A", "B", "vertical", "second")).toBe(false);
    expect(isRedundantSplit(twoPaneVertical, "B", "A", "vertical", "first")).toBe(false);
  });

  it("allows re-splitting siblings along the perpendicular axis", () => {
    expect(isRedundantSplit(twoPaneVertical, "A", "B", "horizontal", "first")).toBe(false);
    expect(isRedundantSplit(twoPaneVertical, "A", "B", "horizontal", "second")).toBe(false);
    expect(isRedundantSplit(twoPaneHorizontal, "A", "B", "vertical", "first")).toBe(false);
  });

  it("applies the sibling rule to nested splits only between actual siblings", () => {
    expect(isRedundantSplit(threePaneNested, "B", "C", "vertical", "first")).toBe(true);
    expect(isRedundantSplit(threePaneNested, "C", "B", "vertical", "second")).toBe(true);

    expect(isRedundantSplit(threePaneNested, "A", "B", "horizontal", "first")).toBe(false);
    expect(isRedundantSplit(threePaneNested, "A", "C", "horizontal", "first")).toBe(false);
    expect(isRedundantSplit(threePaneNested, "B", "A", "horizontal", "second")).toBe(false);
  });

  it("treats a drop onto the dragged pane itself as redundant", () => {
    expect(isRedundantSplit(twoPaneVertical, "A", "A", "vertical", "second")).toBe(true);
    expect(isRedundantSplit(twoPaneVertical, "A", "A", "horizontal", "first")).toBe(true);
  });

  it("reports nothing redundant for a single-leaf tree", () => {
    const single: PaneNode = { type: "leaf", leafId: "A" };
    expect(isRedundantSplit(single, "A", "B", "vertical", "first")).toBe(false);
  });
});
