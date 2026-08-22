export type PaneDirection = "horizontal" | "vertical";

export type PaneLeafNode = { type: "leaf"; leafId: string };

export type PaneSplitNode = {
  type: "split";
  direction: PaneDirection;
  first: PaneNode;
  second: PaneNode;
  ratio: number;
};

export type PaneNode = PaneLeafNode | PaneSplitNode;

export const MIN_PANE_RATIO = 0.1;
export const MAX_PANE_RATIO = 0.9;

/** Clamps a ratio into the usable range, falling back to an even split for non-finite input. */
export function clampRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return 0.5;
  return Math.min(MAX_PANE_RATIO, Math.max(MIN_PANE_RATIO, ratio));
}

export function createLeafNode(leafId?: string): PaneNode {
  return { type: "leaf", leafId: leafId ?? createLeafId() };
}

/**
 * Replaces the leaf `targetLeafId` with a split containing that leaf and a new leaf.
 * `position` decides which side the new leaf takes. Returns the original tree when
 * the target leaf does not exist.
 */
export function splitLeaf(
  root: PaneNode,
  targetLeafId: string,
  newLeafId: string,
  direction: PaneDirection,
  position: "first" | "second" = "second",
  ratio = 0.5,
): PaneNode {
  return splitLeafWithSubtree(root, targetLeafId, createLeafNode(newLeafId), direction, position, ratio);
}

/**
 * Replaces `targetLeafId` with a split containing the existing target leaf and an
 * arbitrary pane subtree. This is the pane-tree equivalent of Orca moving a whole
 * tab (including any terminal panes owned by that tab) into a newly-created split group.
 */
export function splitLeafWithSubtree(
  root: PaneNode,
  targetLeafId: string,
  subtree: PaneNode,
  direction: PaneDirection,
  position: "first" | "second" = "second",
  ratio = 0.5,
): PaneNode {
  return mapLeaf(root, targetLeafId, (leaf) => ({
    type: "split",
    direction,
    first: position === "first" ? subtree : leaf,
    second: position === "first" ? leaf : subtree,
    ratio: clampRatio(ratio),
  }));
}

/**
 * Removes the leaf `targetLeafId`, collapsing its parent split into the surviving sibling.
 * Returns `null` when the removed leaf was the whole tree, and the original tree when the
 * leaf does not exist.
 */
export function removeLeaf(root: PaneNode, targetLeafId: string): PaneNode | null {
  if (root.type === "leaf") return root.leafId === targetLeafId ? null : root;

  const first = removeLeaf(root.first, targetLeafId);
  if (first === null) return root.second;
  if (first !== root.first) return { ...root, first };

  const second = removeLeaf(root.second, targetLeafId);
  if (second === null) return root.first;
  if (second !== root.second) return { ...root, second };

  return root;
}

/**
 * Sets the ratio of the split addressed by a dot-notation `path` such as "" (root),
 * "first", or "second.first". Returns the original tree when the path does not resolve
 * to a split node.
 */
export function setRatioAtPath(root: PaneNode, path: string, ratio: number): PaneNode {
  const segments = path.split(".").filter((segment) => segment.length > 0);
  return applyRatio(root, segments, clampRatio(ratio));
}

export function findFirstLeafId(root: PaneNode): string {
  let node = root;
  while (node.type === "split") node = node.first;
  return node.leafId;
}

export function collectLeafIds(root: PaneNode): string[] {
  const ids: string[] = [];
  const visit = (node: PaneNode) => {
    if (node.type === "leaf") {
      ids.push(node.leafId);
      return;
    }
    visit(node.first);
    visit(node.second);
  };
  visit(root);
  return ids;
}

/**
 * Returns the first leaf id inside the sibling subtree of `targetLeafId`,
 * or `null` when the leaf is missing or is the tree root.
 */
export function findSiblingLeafId(root: PaneNode, targetLeafId: string): string | null {
  if (root.type === "leaf") return null;

  const inFirst = containsLeaf(root.first, targetLeafId);
  const inSecond = containsLeaf(root.second, targetLeafId);

  if (inFirst) {
    return root.first.type === "leaf"
      ? findFirstLeafId(root.second)
      : findSiblingLeafId(root.first, targetLeafId);
  }
  if (inSecond) {
    return root.second.type === "leaf"
      ? findFirstLeafId(root.first)
      : findSiblingLeafId(root.second, targetLeafId);
  }
  return null;
}

/**
 * Swaps the positions of two leaves. Returns the original tree when either leaf is
 * missing or when both ids refer to the same leaf.
 */
export function swapLeaves(root: PaneNode, sourceLeafId: string, targetLeafId: string): PaneNode {
  if (sourceLeafId === targetLeafId) return root;
  if (!containsLeaf(root, sourceLeafId) || !containsLeaf(root, targetLeafId)) return root;

  const swap = (node: PaneNode): PaneNode => {
    if (node.type === "leaf") {
      if (node.leafId === sourceLeafId) return { type: "leaf", leafId: targetLeafId };
      if (node.leafId === targetLeafId) return { type: "leaf", leafId: sourceLeafId };
      return node;
    }
    const first = swap(node.first);
    const second = swap(node.second);
    if (first === node.first && second === node.second) return node;
    return { ...node, first, second };
  };

  return swap(root);
}

/** Resets every split in the tree to an even 0.5 ratio. */
export function equalizeRatios(root: PaneNode): PaneNode {
  if (root.type === "leaf") return root;
  const first = equalizeRatios(root.first);
  const second = equalizeRatios(root.second);
  if (first === root.first && second === root.second && root.ratio === 0.5) return root;
  return { ...root, first, second, ratio: 0.5 };
}

function applyRatio(node: PaneNode, segments: string[], ratio: number): PaneNode {
  if (node.type === "leaf") return node;

  if (segments.length === 0) {
    return node.ratio === ratio ? node : { ...node, ratio };
  }

  const [head, ...rest] = segments;
  if (head !== "first" && head !== "second") return node;

  const child = applyRatio(node[head], rest, ratio);
  if (child === node[head]) return node;
  return { ...node, [head]: child };
}

function mapLeaf(node: PaneNode, targetLeafId: string, replace: (leaf: PaneLeafNode) => PaneNode): PaneNode {
  if (node.type === "leaf") return node.leafId === targetLeafId ? replace(node) : node;

  const first = mapLeaf(node.first, targetLeafId, replace);
  if (first !== node.first) return { ...node, first };

  const second = mapLeaf(node.second, targetLeafId, replace);
  if (second !== node.second) return { ...node, second };

  return node;
}

function containsLeaf(node: PaneNode, leafId: string): boolean {
  if (node.type === "leaf") return node.leafId === leafId;
  return containsLeaf(node.first, leafId) || containsLeaf(node.second, leafId);
}

function createLeafId() {
  const randomPart = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `pane:${randomPart}`;
}
