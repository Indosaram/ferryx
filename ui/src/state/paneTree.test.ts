import { describe, expect, it } from "vitest";

import {
  clampRatio,
  collectLeafIds,
  createLeafNode,
  equalizeRatios,
  findFirstLeafId,
  findSiblingLeafId,
  MAX_PANE_RATIO,
  MIN_PANE_RATIO,
  removeLeaf,
  setRatioAtPath,
  splitLeaf,
  swapLeaves,
  type PaneNode,
} from "./paneTree";

function leaf(leafId: string): PaneNode {
  return { type: "leaf", leafId };
}

function split(direction: "horizontal" | "vertical", first: PaneNode, second: PaneNode, ratio = 0.5): PaneNode {
  return { type: "split", direction, first, second, ratio };
}

/** a | (b | c) — a right-nested tree used by several suites. */
function nestedTree(): PaneNode {
  return split("horizontal", leaf("a"), split("vertical", leaf("b"), leaf("c"), 0.4), 0.6);
}

describe("createLeafNode", () => {
  it("uses the provided leaf id", () => {
    expect(createLeafNode("pane-1")).toEqual({ type: "leaf", leafId: "pane-1" });
  });

  it("generates a unique prefixed id when none is provided", () => {
    const first = createLeafNode();
    const second = createLeafNode();

    expect(first.type).toBe("leaf");
    if (first.type !== "leaf" || second.type !== "leaf") throw new Error("expected leaves");
    expect(first.leafId).toMatch(/^pane:/);
    expect(second.leafId).toMatch(/^pane:/);
    expect(first.leafId).not.toBe(second.leafId);
  });
});

describe("clampRatio", () => {
  it("keeps ratios inside the usable range", () => {
    expect(clampRatio(0.5)).toBe(0.5);
    expect(clampRatio(0)).toBe(MIN_PANE_RATIO);
    expect(clampRatio(1)).toBe(MAX_PANE_RATIO);
    expect(clampRatio(-4)).toBe(MIN_PANE_RATIO);
  });

  it("falls back to an even split for non-finite input", () => {
    expect(clampRatio(Number.NaN)).toBe(0.5);
    expect(clampRatio(Number.POSITIVE_INFINITY)).toBe(0.5);
  });
});

describe("splitLeaf", () => {
  it("splits a root leaf with the new pane in the second position by default", () => {
    expect(splitLeaf(leaf("a"), "a", "b", "horizontal")).toEqual(
      split("horizontal", leaf("a"), leaf("b"), 0.5),
    );
  });

  it("places the new pane first when requested", () => {
    expect(splitLeaf(leaf("a"), "a", "b", "vertical", "first")).toEqual(
      split("vertical", leaf("b"), leaf("a"), 0.5),
    );
  });

  it("honours an explicit ratio and clamps out-of-range values", () => {
    const tree = splitLeaf(leaf("a"), "a", "b", "horizontal", "second", 0.25);
    expect(tree).toMatchObject({ ratio: 0.25 });
    expect(splitLeaf(leaf("a"), "a", "b", "horizontal", "second", 5)).toMatchObject({
      ratio: MAX_PANE_RATIO,
    });
  });

  it("splits a nested leaf while preserving the surrounding structure", () => {
    const next = splitLeaf(nestedTree(), "c", "d", "horizontal");

    expect(next).toEqual(
      split(
        "horizontal",
        leaf("a"),
        split("vertical", leaf("b"), split("horizontal", leaf("c"), leaf("d"), 0.5), 0.4),
        0.6,
      ),
    );
  });

  it("returns the original tree when the target leaf is missing", () => {
    const tree = nestedTree();
    expect(splitLeaf(tree, "missing", "d", "horizontal")).toBe(tree);
  });

  it("does not mutate the input tree", () => {
    const tree = nestedTree();
    const snapshot = structuredClone(tree);
    splitLeaf(tree, "b", "d", "vertical");
    expect(tree).toEqual(snapshot);
  });

  it("shares untouched subtrees with the previous tree", () => {
    const tree = nestedTree();
    const next = splitLeaf(tree, "c", "d", "horizontal");

    if (tree.type !== "split" || next.type !== "split") throw new Error("expected splits");
    expect(next.first).toBe(tree.first);
    expect(next).not.toBe(tree);
  });
});

describe("removeLeaf", () => {
  it("returns null when removing the only leaf", () => {
    expect(removeLeaf(leaf("a"), "a")).toBeNull();
  });

  it("collapses the parent split into the surviving sibling", () => {
    const tree = split("horizontal", leaf("a"), leaf("b"));
    expect(removeLeaf(tree, "a")).toEqual(leaf("b"));
    expect(removeLeaf(tree, "b")).toEqual(leaf("a"));
  });

  it("collapses into a sibling subtree rather than a single leaf", () => {
    const next = removeLeaf(nestedTree(), "a");
    expect(next).toEqual(split("vertical", leaf("b"), leaf("c"), 0.4));
  });

  it("removes a deeply nested leaf and keeps the outer split", () => {
    expect(removeLeaf(nestedTree(), "c")).toEqual(split("horizontal", leaf("a"), leaf("b"), 0.6));
  });

  it("returns the original tree when the leaf is missing", () => {
    const tree = nestedTree();
    expect(removeLeaf(tree, "missing")).toBe(tree);
  });

  it("supports removing every leaf one at a time down to null", () => {
    let tree: PaneNode | null = nestedTree();
    for (const id of ["b", "a", "c"]) {
      tree = removeLeaf(tree as PaneNode, id);
    }
    expect(tree).toBeNull();
  });

  it("does not mutate the input tree", () => {
    const tree = nestedTree();
    const snapshot = structuredClone(tree);
    removeLeaf(tree, "b");
    expect(tree).toEqual(snapshot);
  });
});

describe("setRatioAtPath", () => {
  it("sets the ratio at the root for an empty path", () => {
    expect(setRatioAtPath(nestedTree(), "", 0.3)).toMatchObject({ ratio: 0.3 });
  });

  it("sets the ratio on a nested split addressed by dot notation", () => {
    const next = setRatioAtPath(nestedTree(), "second", 0.8);
    expect(next).toEqual(
      split("horizontal", leaf("a"), split("vertical", leaf("b"), leaf("c"), 0.8), 0.6),
    );
  });

  it("resolves multi-segment paths", () => {
    const tree = split("horizontal", leaf("a"), split("vertical", leaf("b"), split("horizontal", leaf("c"), leaf("d"))));
    const next = setRatioAtPath(tree, "second.second", 0.7);

    if (next.type !== "split" || next.second.type !== "split" || next.second.second.type !== "split") {
      throw new Error("expected nested splits");
    }
    expect(next.second.second.ratio).toBe(0.7);
    expect(next.second.ratio).toBe(0.5);
    expect(next.ratio).toBe(0.5);
  });

  it("clamps the ratio into the usable range", () => {
    expect(setRatioAtPath(nestedTree(), "", 0.99)).toMatchObject({ ratio: MAX_PANE_RATIO });
    expect(setRatioAtPath(nestedTree(), "", -1)).toMatchObject({ ratio: MIN_PANE_RATIO });
  });

  it("returns the original tree for paths that do not resolve to a split", () => {
    const tree = nestedTree();
    expect(setRatioAtPath(tree, "first", 0.3)).toBe(tree);
    expect(setRatioAtPath(tree, "second.first.second", 0.3)).toBe(tree);
    expect(setRatioAtPath(tree, "third", 0.3)).toBe(tree);
    expect(setRatioAtPath(leaf("a"), "first", 0.3)).toEqual(leaf("a"));
  });

  it("returns the original tree when the ratio is unchanged", () => {
    const tree = nestedTree();
    expect(setRatioAtPath(tree, "", 0.6)).toBe(tree);
  });

  it("does not mutate the input tree", () => {
    const tree = nestedTree();
    const snapshot = structuredClone(tree);
    setRatioAtPath(tree, "second", 0.25);
    expect(tree).toEqual(snapshot);
  });
});

describe("findFirstLeafId", () => {
  it("returns the id of a root leaf", () => {
    expect(findFirstLeafId(leaf("a"))).toBe("a");
  });

  it("descends into the first branch of nested splits", () => {
    expect(findFirstLeafId(nestedTree())).toBe("a");
    expect(findFirstLeafId(split("vertical", nestedTree(), leaf("z")))).toBe("a");
  });
});

describe("collectLeafIds", () => {
  it("returns a single id for a leaf tree", () => {
    expect(collectLeafIds(leaf("a"))).toEqual(["a"]);
  });

  it("returns ids in first-to-second document order", () => {
    expect(collectLeafIds(nestedTree())).toEqual(["a", "b", "c"]);
  });

  it("tracks ids across repeated splits", () => {
    let tree: PaneNode = createLeafNode("a");
    tree = splitLeaf(tree, "a", "b", "horizontal");
    tree = splitLeaf(tree, "b", "c", "vertical");
    tree = splitLeaf(tree, "a", "d", "vertical", "first");

    expect(collectLeafIds(tree)).toEqual(["d", "a", "b", "c"]);
  });
});

describe("findSiblingLeafId", () => {
  it("returns null for a single-leaf tree", () => {
    expect(findSiblingLeafId(leaf("a"), "a")).toBeNull();
  });

  it("returns the neighbouring leaf in a simple split", () => {
    const tree = split("horizontal", leaf("a"), leaf("b"));
    expect(findSiblingLeafId(tree, "a")).toBe("b");
    expect(findSiblingLeafId(tree, "b")).toBe("a");
  });

  it("returns the closest sibling inside a nested split", () => {
    const tree = nestedTree();
    expect(findSiblingLeafId(tree, "b")).toBe("c");
    expect(findSiblingLeafId(tree, "c")).toBe("b");
  });

  it("returns the first leaf of a sibling subtree when the sibling is a split", () => {
    expect(findSiblingLeafId(nestedTree(), "a")).toBe("b");
  });

  it("returns null when the leaf is missing", () => {
    expect(findSiblingLeafId(nestedTree(), "missing")).toBeNull();
  });
});

describe("swapLeaves", () => {
  it("swaps two sibling leaves", () => {
    const tree = split("horizontal", leaf("a"), leaf("b"), 0.3);
    expect(swapLeaves(tree, "a", "b")).toEqual(split("horizontal", leaf("b"), leaf("a"), 0.3));
  });

  it("swaps leaves across different depths without changing structure or ratios", () => {
    const next = swapLeaves(nestedTree(), "a", "c");

    expect(next).toEqual(
      split("horizontal", leaf("c"), split("vertical", leaf("b"), leaf("a"), 0.4), 0.6),
    );
    expect(collectLeafIds(next)).toEqual(["c", "b", "a"]);
  });

  it("is symmetric in its arguments", () => {
    expect(swapLeaves(nestedTree(), "a", "c")).toEqual(swapLeaves(nestedTree(), "c", "a"));
  });

  it("restores the original tree when applied twice", () => {
    const tree = nestedTree();
    expect(swapLeaves(swapLeaves(tree, "a", "c"), "a", "c")).toEqual(tree);
  });

  it("returns the original tree when both ids are the same", () => {
    const tree = nestedTree();
    expect(swapLeaves(tree, "a", "a")).toBe(tree);
  });

  it("returns the original tree when either leaf is missing", () => {
    const tree = nestedTree();
    expect(swapLeaves(tree, "a", "missing")).toBe(tree);
    expect(swapLeaves(tree, "missing", "a")).toBe(tree);
  });

  it("does not mutate the input tree", () => {
    const tree = nestedTree();
    const snapshot = structuredClone(tree);
    swapLeaves(tree, "a", "c");
    expect(tree).toEqual(snapshot);
  });
});

describe("equalizeRatios", () => {
  it("returns a leaf tree unchanged", () => {
    const tree = leaf("a");
    expect(equalizeRatios(tree)).toBe(tree);
  });

  it("resets every nested split to an even ratio", () => {
    expect(equalizeRatios(nestedTree())).toEqual(
      split("horizontal", leaf("a"), split("vertical", leaf("b"), leaf("c"), 0.5), 0.5),
    );
  });

  it("preserves direction and leaf order", () => {
    const next = equalizeRatios(nestedTree());
    expect(collectLeafIds(next)).toEqual(["a", "b", "c"]);
    expect(next).toMatchObject({ direction: "horizontal", second: { direction: "vertical" } });
  });

  it("returns the original tree when all ratios are already even", () => {
    const tree = split("horizontal", leaf("a"), split("vertical", leaf("b"), leaf("c"), 0.5), 0.5);
    expect(equalizeRatios(tree)).toBe(tree);
  });

  it("does not mutate the input tree", () => {
    const tree = nestedTree();
    const snapshot = structuredClone(tree);
    equalizeRatios(tree);
    expect(tree).toEqual(snapshot);
  });
});

describe("tree operation sequences", () => {
  it("keeps leaf ids consistent across split, swap, resize, and remove", () => {
    let tree: PaneNode = createLeafNode("root");
    tree = splitLeaf(tree, "root", "right", "horizontal", "second", 0.7);
    tree = splitLeaf(tree, "right", "bottom", "vertical");
    expect(collectLeafIds(tree)).toEqual(["root", "right", "bottom"]);

    tree = swapLeaves(tree, "root", "bottom");
    expect(collectLeafIds(tree)).toEqual(["bottom", "right", "root"]);

    tree = setRatioAtPath(tree, "second", 0.25);
    expect(tree).toMatchObject({ ratio: 0.7, second: { ratio: 0.25 } });

    const afterRemove = removeLeaf(tree, "right");
    expect(afterRemove).toEqual(split("horizontal", leaf("bottom"), leaf("root"), 0.7));

    expect(findFirstLeafId(afterRemove as PaneNode)).toBe("bottom");
    expect(findSiblingLeafId(afterRemove as PaneNode, "bottom")).toBe("root");
  });
});
