import { clampRatio, MAX_PANE_RATIO, MIN_PANE_RATIO, type PaneDirection, type PaneNode } from "./paneTree";

export type NormalizedRect = {
  x: number;
  y: number;
  w: number;
  h: number;
};

export type SplitGeometry = {
  path: string;
  direction: PaneDirection;
  ratio: number;
  rect: NormalizedRect;
  coord: number;
  spanStart: number;
  spanEnd: number;
};

export type ResolvedSeam = {
  direction: PaneDirection;
  targetPath: string;
  target: { origin: number; span: number };
  members: Array<{ path: string; origin: number; span: number }>;
};

export const COLLINEAR_EPSILON = 0.001;

function isDescendantOrSelfPath(path: string, other: string): boolean {
  return path === other || path.startsWith(`${other}.`) || other.startsWith(`${path}.`);
}

export function computeSplitsGeometry(
  node: PaneNode,
  path = "",
  rect: NormalizedRect = { x: 0, y: 0, w: 1, h: 1 },
  list: SplitGeometry[] = [],
): SplitGeometry[] {
  if (node.type === "leaf") return list;

  const isH = node.direction === "horizontal";
  const ratio = clampRatio(node.ratio);
  const coord = isH ? rect.x + rect.w * ratio : rect.y + rect.h * ratio;
  const spanStart = isH ? rect.y : rect.x;
  const spanEnd = isH ? rect.y + rect.h : rect.x + rect.w;

  list.push({
    path,
    direction: node.direction,
    ratio,
    rect,
    coord,
    spanStart,
    spanEnd,
  });

  const firstRect: NormalizedRect = isH
    ? { x: rect.x, y: rect.y, w: rect.w * ratio, h: rect.h }
    : { x: rect.x, y: rect.y, w: rect.w, h: rect.h * ratio };

  const secondRect: NormalizedRect = isH
    ? { x: rect.x + rect.w * ratio, y: rect.y, w: rect.w * (1 - ratio), h: rect.h }
    : { x: rect.x, y: rect.y + rect.h * ratio, w: rect.w, h: rect.h * (1 - ratio) };

  computeSplitsGeometry(node.first, path ? `${path}.first` : "first", firstRect, list);
  computeSplitsGeometry(node.second, path ? `${path}.second` : "second", secondRect, list);

  return list;
}

export function computeLeafRects(
  node: PaneNode,
  rect: NormalizedRect = { x: 0, y: 0, w: 1, h: 1 },
  map = new Map<string, NormalizedRect>(),
): Map<string, NormalizedRect> {
  if (node.type === "leaf") {
    map.set(node.leafId, rect);
    return map;
  }

  const isH = node.direction === "horizontal";
  const ratio = clampRatio(node.ratio);

  const firstRect: NormalizedRect = isH
    ? { x: rect.x, y: rect.y, w: rect.w * ratio, h: rect.h }
    : { x: rect.x, y: rect.y, w: rect.w, h: rect.h * ratio };

  const secondRect: NormalizedRect = isH
    ? { x: rect.x + rect.w * ratio, y: rect.y, w: rect.w * (1 - ratio), h: rect.h }
    : { x: rect.x, y: rect.y + rect.h * ratio, w: rect.w, h: rect.h * (1 - ratio) };

  computeLeafRects(node.first, firstRect, map);
  computeLeafRects(node.second, secondRect, map);

  return map;
}

export function areSplitsCollinearAdjacent(
  a: SplitGeometry,
  b: SplitGeometry,
  epsilon = COLLINEAR_EPSILON,
): boolean {
  if (a.direction !== b.direction) return false;
  if (isDescendantOrSelfPath(a.path, b.path)) return false;
  if (Math.abs(a.coord - b.coord) > epsilon) return false;

  const overlap = Math.min(a.spanEnd, b.spanEnd) - Math.max(a.spanStart, b.spanStart);
  return overlap > 1e-6 || Math.abs(overlap) <= epsilon;
}

export function findCollinearSeam(
  splits: SplitGeometry[],
  targetPath: string,
  epsilon = COLLINEAR_EPSILON,
): SplitGeometry[] {
  const target = splits.find((s) => s.path === targetPath);
  if (!target) return [];

  const seam: SplitGeometry[] = [target];
  const visited = new Set<string>([target.path]);
  const queue: SplitGeometry[] = [target];

  while (queue.length > 0) {
    const curr = queue.shift()!;
    for (const other of splits) {
      if (visited.has(other.path)) continue;
      if (areSplitsCollinearAdjacent(curr, other, epsilon)) {
        visited.add(other.path);
        seam.push(other);
        queue.push(other);
      }
    }
  }

  return seam;
}

export function resolveSeam(
  root: PaneNode,
  targetPath: string,
  epsilon = COLLINEAR_EPSILON,
): ResolvedSeam | null {
  const splits = computeSplitsGeometry(root);
  const seam = findCollinearSeam(splits, targetPath, epsilon);
  const target = seam.find((s) => s.path === targetPath);
  if (!target) return null;

  const isH = target.direction === "horizontal";
  const toMember = (s: SplitGeometry) => ({
    path: s.path,
    origin: isH ? s.rect.x : s.rect.y,
    span: isH ? s.rect.w : s.rect.h,
  });

  return {
    direction: target.direction,
    targetPath,
    target: toMember(target),
    members: seam.map(toMember),
  };
}

function memberPathsResolve(root: PaneNode, seam: ResolvedSeam): boolean {
  for (const member of seam.members) {
    let node: PaneNode = root;
    for (const segment of member.path.split(".").filter(Boolean)) {
      if (node.type !== "split") return false;
      if (segment !== "first" && segment !== "second") return false;
      node = node[segment];
    }
    if (node.type !== "split" || node.direction !== seam.direction) return false;
  }
  return true;
}

export function applySeamRatio(
  root: PaneNode,
  seam: ResolvedSeam,
  newRatio: number,
): PaneNode {
  if (!memberPathsResolve(root, seam)) {
    const segments = seam.targetPath.split(".").filter(Boolean);
    return applySingleRatio(root, segments, clampRatio(newRatio));
  }

  const desiredCoord = seam.target.origin + seam.target.span * clampRatio(newRatio);

  let minCoord = -Infinity;
  let maxCoord = Infinity;

  for (const m of seam.members) {
    const sMin = m.origin + m.span * MIN_PANE_RATIO;
    const sMax = m.origin + m.span * MAX_PANE_RATIO;
    if (sMin > minCoord) minCoord = sMin;
    if (sMax < maxCoord) maxCoord = sMax;
  }

  const clampedCoord =
    minCoord > maxCoord
      ? (minCoord + maxCoord) / 2
      : Math.max(minCoord, Math.min(maxCoord, desiredCoord));

  const ratioByPath = new Map<string, number>();
  for (const m of seam.members) {
    const r = (clampedCoord - m.origin) / m.span;
    ratioByPath.set(m.path, Number(clampRatio(r).toFixed(4)));
  }

  function applyRatios(node: PaneNode, path = ""): PaneNode {
    if (node.type === "leaf") return node;

    let nextRatio = node.ratio;
    if (ratioByPath.has(path)) {
      nextRatio = ratioByPath.get(path)!;
    }

    const nextPath = (branch: string) => (path ? `${path}.${branch}` : branch);
    const first = applyRatios(node.first, nextPath("first"));
    const second = applyRatios(node.second, nextPath("second"));

    if (first === node.first && second === node.second && nextRatio === node.ratio) {
      return node;
    }

    return { ...node, first, second, ratio: nextRatio };
  }

  return applyRatios(root);
}

export function setCollinearRatioAtPath(
  root: PaneNode,
  targetPath: string,
  newRatio: number,
  options?: { isolated?: boolean; seam?: ResolvedSeam | null },
): PaneNode {
  if (options?.isolated) {
    const segments = targetPath.split(".").filter((s) => s.length > 0);
    return applySingleRatio(root, segments, clampRatio(newRatio));
  }

  const seam = options?.seam ?? resolveSeam(root, targetPath);
  if (!seam) return root;

  return applySeamRatio(root, seam, newRatio);
}

export function findAlignedSplitRatio(
  root: PaneNode,
  targetLeafId: string,
  direction: PaneDirection,
  defaultRatio = 0.5,
  epsilon = COLLINEAR_EPSILON,
): number {
  const leafMap = computeLeafRects(root);
  const targetRect = leafMap.get(targetLeafId);
  if (!targetRect) return defaultRatio;

  const splits = computeSplitsGeometry(root);
  const isH = direction === "horizontal";
  const leafPerpStart = isH ? targetRect.y : targetRect.x;
  const leafPerpEnd = isH ? targetRect.y + targetRect.h : targetRect.x + targetRect.w;
  const leafCoordStart = isH ? targetRect.x : targetRect.y;
  const leafCoordSpan = isH ? targetRect.w : targetRect.h;

  let bestCandidate: SplitGeometry | null = null;
  let minDistance = Infinity;

  for (const s of splits) {
    if (s.direction !== direction) continue;
    if (s.coord <= leafCoordStart + epsilon || s.coord >= leafCoordStart + leafCoordSpan - epsilon) continue;

    const touchesPerp =
      Math.max(s.spanStart, leafPerpStart) <= Math.min(s.spanEnd, leafPerpEnd) + epsilon;
    if (!touchesPerp) continue;

    const distance = Math.min(
      Math.abs(s.spanStart - leafPerpEnd),
      Math.abs(s.spanEnd - leafPerpStart),
      Math.abs(s.spanStart - leafPerpStart),
      Math.abs(s.spanEnd - leafPerpEnd),
    );

    if (distance < minDistance) {
      minDistance = distance;
      bestCandidate = s;
    }
  }

  if (bestCandidate) {
    const alignedRatio = (bestCandidate.coord - leafCoordStart) / leafCoordSpan;
    return Number(clampRatio(alignedRatio).toFixed(4));
  }

  return defaultRatio;
}

function applySingleRatio(node: PaneNode, segments: string[], ratio: number): PaneNode {
  if (node.type === "leaf") return node;

  if (segments.length === 0) {
    return node.ratio === ratio ? node : { ...node, ratio };
  }

  const [head, ...rest] = segments;
  if (head !== "first" && head !== "second") return node;

  const child = applySingleRatio(node[head], rest, ratio);
  if (child === node[head]) return node;
  return { ...node, [head]: child };
}
