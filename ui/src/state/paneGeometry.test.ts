import { describe, expect, it } from "vitest";

import {
  applySeamRatio,
  computeSplitsGeometry,
  findAlignedSplitRatio,
  findCollinearSeam,
  resolveSeam,
  setCollinearRatioAtPath,
} from "./paneGeometry";
import { type PaneNode } from "./paneTree";

function leaf(leafId: string): PaneNode {
  return { type: "leaf", leafId };
}

function split(
  direction: "horizontal" | "vertical",
  first: PaneNode,
  second: PaneNode,
  ratio = 0.5,
): PaneNode {
  return { type: "split", direction, first, second, ratio };
}

describe("computeSplitsGeometry", () => {
  it("returns empty array for a leaf node", () => {
    expect(computeSplitsGeometry(leaf("a"))).toEqual([]);
  });

  it("computes normalized geometry for 2x2 horizontal-first tree", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.5),
      split("vertical", leaf("b1"), leaf("b2"), 0.5),
      0.5,
    );

    const splits = computeSplitsGeometry(tree);
    expect(splits).toHaveLength(3);

    const rootSplit = splits.find((s) => s.path === "");
    expect(rootSplit).toBeDefined();
    expect(rootSplit?.direction).toBe("horizontal");
    expect(rootSplit?.coord).toBe(0.5);
    expect(rootSplit?.spanStart).toBe(0);
    expect(rootSplit?.spanEnd).toBe(1);

    const leftSplit = splits.find((s) => s.path === "first");
    expect(leftSplit?.direction).toBe("vertical");
    expect(leftSplit?.coord).toBe(0.5);
    expect(leftSplit?.spanStart).toBe(0);
    expect(leftSplit?.spanEnd).toBe(0.5);

    const rightSplit = splits.find((s) => s.path === "second");
    expect(rightSplit?.direction).toBe("vertical");
    expect(rightSplit?.coord).toBe(0.5);
    expect(rightSplit?.spanStart).toBe(0.5);
    expect(rightSplit?.spanEnd).toBe(1);
  });
});

describe("findCollinearSeam", () => {
  it("finds collinear splits across horizontal columns", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.5),
      split("vertical", leaf("b1"), leaf("b2"), 0.5),
      0.5,
    );
    const splits = computeSplitsGeometry(tree);

    const seamFirst = findCollinearSeam(splits, "first");
    expect(seamFirst.map((s) => s.path).sort()).toEqual(["first", "second"]);

    const seamSecond = findCollinearSeam(splits, "second");
    expect(seamSecond.map((s) => s.path).sort()).toEqual(["first", "second"]);

    const seamRoot = findCollinearSeam(splits, "");
    expect(seamRoot.map((s) => s.path)).toEqual([""]);
  });

  it("finds collinear splits across vertical rows", () => {
    const tree = split(
      "vertical",
      split("horizontal", leaf("a1"), leaf("b1"), 0.5),
      split("horizontal", leaf("a2"), leaf("b2"), 0.5),
      0.5,
    );
    const splits = computeSplitsGeometry(tree);

    const seamFirst = findCollinearSeam(splits, "first");
    expect(seamFirst.map((s) => s.path).sort()).toEqual(["first", "second"]);

    const seamSecond = findCollinearSeam(splits, "second");
    expect(seamSecond.map((s) => s.path).sort()).toEqual(["first", "second"]);

    const seamRoot = findCollinearSeam(splits, "");
    expect(seamRoot.map((s) => s.path)).toEqual([""]);
  });

  it("does not weld a split with its own ancestor or descendant", () => {
    const tree = split(
      "horizontal",
      leaf("A"),
      split(
        "horizontal",
        leaf("B"),
        split("horizontal", leaf("C"), leaf("D"), 0.1),
        0.9,
      ),
      0.9,
    );
    const splits = computeSplitsGeometry(tree);

    const seamSecond = findCollinearSeam(splits, "second");
    expect(seamSecond.map((s) => s.path)).toEqual(["second"]);

    const seamNested = findCollinearSeam(splits, "second.second");
    expect(seamNested.map((s) => s.path)).toEqual(["second.second"]);
  });

  it("does not group unaligned splits", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.3),
      split("vertical", leaf("b1"), leaf("b2"), 0.7),
      0.5,
    );
    const splits = computeSplitsGeometry(tree);

    const seamFirst = findCollinearSeam(splits, "first");
    expect(seamFirst.map((s) => s.path)).toEqual(["first"]);

    const seamSecond = findCollinearSeam(splits, "second");
    expect(seamSecond.map((s) => s.path)).toEqual(["second"]);
  });
});

describe("resolveSeam and applySeamRatio (frozen seam)", () => {
  it("resolves a frozen seam at drag start and applies updates without re-evaluating membership", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.5),
      split("vertical", leaf("b1"), leaf("b2"), 0.5),
      0.5,
    );

    const seam = resolveSeam(tree, "first");
    expect(seam).not.toBeNull();
    expect(seam?.members.map((m) => m.path).sort()).toEqual(["first", "second"]);

    const updated = applySeamRatio(tree, seam!, 0.7);
    if (updated.type !== "split") throw new Error("expected split");
    if (updated.first.type !== "split" || updated.second.type !== "split") {
      throw new Error("expected child splits");
    }
    expect(updated.first.ratio).toBe(0.7);
    expect(updated.second.ratio).toBe(0.7);
  });

  it("prevents mid-drag capture when an unaligned divider sweeps past another divider", () => {
    let tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.3),
      split("vertical", leaf("b1"), leaf("b2"), 0.6),
      0.5,
    );

    const frozenSeam = resolveSeam(tree, "first");
    expect(frozenSeam?.members.map((m) => m.path)).toEqual(["first"]);

    tree = applySeamRatio(tree, frozenSeam!, 0.596);
    if (tree.type !== "split" || tree.first.type !== "split" || tree.second.type !== "split") {
      throw new Error("expected splits");
    }
    expect(tree.first.ratio).toBe(0.596);
    expect(tree.second.ratio).toBe(0.6);

    tree = applySeamRatio(tree, frozenSeam!, 0.7);
    if (tree.type !== "split" || tree.first.type !== "split" || tree.second.type !== "split") {
      throw new Error("expected splits");
    }
    expect(tree.first.ratio).toBe(0.7);
    expect(tree.second.ratio).toBe(0.6);
  });

  it("excludes ancestor and descendant splits from the seam to prevent drag deadlock", () => {
    const tree = split(
      "horizontal",
      leaf("A"),
      split(
        "horizontal",
        leaf("B"),
        split("horizontal", leaf("C"), leaf("D"), 0.1),
        0.9,
      ),
      0.9,
    );

    const seam = resolveSeam(tree, "second");
    expect(seam?.members.map((m) => m.path)).toEqual(["second"]);

    const updated = applySeamRatio(tree, seam!, 0.5);
    if (updated.type !== "split" || updated.second.type !== "split") throw new Error("expected splits");
    expect(updated.second.ratio).toBe(0.5);
  });

  it("falls back to isolated update when seam member paths no longer resolve", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.5),
      split("vertical", leaf("b1"), leaf("b2"), 0.5),
      0.5,
    );

    const staleSeam = {
      direction: "vertical" as const,
      targetPath: "first",
      target: { origin: 0, span: 0.5 },
      members: [
        { path: "first", origin: 0, span: 0.5 },
        { path: "third", origin: 0.5, span: 0.5 },
      ],
    };

    const updated = applySeamRatio(tree, staleSeam, 0.7);
    if (updated.type !== "split" || updated.first.type !== "split" || updated.second.type !== "split") {
      throw new Error("expected splits");
    }
    expect(updated.first.ratio).toBe(0.7);
    expect(updated.second.ratio).toBe(0.5);
  });

  it("uses midpoint fallback when seam member bounds invert", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.1),
      split("vertical", leaf("b1"), leaf("b2"), 0.9),
      0.2,
    );

    const seam = resolveSeam(tree, "first");
    expect(seam?.members.map((m) => m.path)).toEqual(["first"]);
  });
});

describe("setCollinearRatioAtPath", () => {
  it("updates both halves of a collinear 2x2 seam simultaneously", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.5),
      split("vertical", leaf("b1"), leaf("b2"), 0.5),
      0.5,
    );

    const updated = setCollinearRatioAtPath(tree, "first", 0.7);
    if (updated.type !== "split") throw new Error("expected split");
    if (updated.first.type !== "split" || updated.second.type !== "split") {
      throw new Error("expected child splits");
    }

    expect(updated.first.ratio).toBe(0.7);
    expect(updated.second.ratio).toBe(0.7);
  });

  it("supports isolated updates when requested", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.5),
      split("vertical", leaf("b1"), leaf("b2"), 0.5),
      0.5,
    );

    const updated = setCollinearRatioAtPath(tree, "first", 0.7, { isolated: true });
    if (updated.type !== "split") throw new Error("expected split");
    if (updated.first.type !== "split" || updated.second.type !== "split") {
      throw new Error("expected child splits");
    }

    expect(updated.first.ratio).toBe(0.7);
    expect(updated.second.ratio).toBe(0.5);
  });

  it("clamps ratio to respect MIN_PANE_RATIO across all seam members", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.5),
      split("vertical", leaf("b1"), leaf("b2"), 0.5),
      0.5,
    );

    const updated = setCollinearRatioAtPath(tree, "first", 0.99);
    if (updated.type !== "split") throw new Error("expected split");
    if (updated.first.type !== "split" || updated.second.type !== "split") {
      throw new Error("expected child splits");
    }

    expect(updated.first.ratio).toBe(0.9);
    expect(updated.second.ratio).toBe(0.9);
  });
});

describe("findAlignedSplitRatio", () => {
  it("detects adjacent vertical split ratio when splitting sibling leaf", () => {
    const tree = split(
      "horizontal",
      split("vertical", leaf("a1"), leaf("a2"), 0.7),
      leaf("b"),
      0.5,
    );

    const ratio = findAlignedSplitRatio(tree, "b", "vertical");
    expect(ratio).toBe(0.7);
  });

  it("falls back to default ratio when no adjacent split exists", () => {
    const tree = split(
      "horizontal",
      leaf("a"),
      leaf("b"),
      0.5,
    );

    const ratio = findAlignedSplitRatio(tree, "b", "vertical");
    expect(ratio).toBe(0.5);
  });
});
