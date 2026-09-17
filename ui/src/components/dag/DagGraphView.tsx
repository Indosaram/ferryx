import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { Minus, Plus } from "lucide-react";

import type { DagNodeSnapshot, DagRunSnapshot } from "../../lib/dagTypes";
import { deriveDagRunCounts } from "../../lib/dagTypes";
import { dagStore } from "../../state/dagStore";
import { DagEdgeLayer } from "./DagEdgeLayer";
import { DagNodeCard } from "./DagNodeCard";
import { DagNodeInspector } from "./DagNodeInspector";
import {
  CARD_HEIGHT,
  CARD_WIDTH,
  calculateFitCamera,
  calculateEffectiveMinScale,
  calculateNodePosition,
  calculateZoomAtAnchor,
  clampScaleWithRecovery,
  formatDurationMs,
  formatTokenCount,
  GAP_X,
  GAP_Y,
  MAX_SCALE,
  normalizeWheelDeltaPixels,
  PAD_X,
  PAD_Y,
  WAVE_LABEL_HEIGHT,
} from "./dagViewUtils";
import type { Camera } from "./dagViewUtils";

export type DagGraphViewProps = {
  readonly runId?: string | null;
  readonly projectPath?: string;
  readonly snapshot?: DagRunSnapshot;
  /** A host that already titles the run (the pane modal) turns this off to avoid a doubled title. */
  readonly showRunName?: boolean;
};

export function DagGraphView({
  runId,
  projectPath,
  snapshot: propSnapshot,
  showRunName = true,
}: DagGraphViewProps): JSX.Element {
  const storeState = useSyncExternalStore(dagStore.subscribe, () => dagStore.getState());

  const activeRun: DagRunSnapshot | null = useMemo(() => {
    if (propSnapshot) return propSnapshot;

    if (runId) {
      for (const projectRuns of Object.values(storeState.runsByProject)) {
        if (projectRuns[runId]) return projectRuns[runId];
      }
    }

    if (projectPath) {
      const summaries = dagStore.runSummaries(projectPath);
      if (summaries.length > 0) return summaries[0];
    }

    for (const projectRuns of Object.values(storeState.runsByProject)) {
      const runs = Object.values(projectRuns);
      if (runs.length > 0) return runs[0];
    }

    return null;
  }, [propSnapshot, runId, projectPath, storeState]);

  // Viewport & camera refs / state
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const cameraRef = useRef<Camera>({ x: 0, y: 0, scale: 1 });
  const [camera, setCameraState] = useState<Camera>({ x: 0, y: 0, scale: 1 });

  const setCamera = useCallback((next: Camera) => {
    cameraRef.current = next;
    setCameraState(next);
  }, []);

  const lastDimensionsRef = useRef<{ width: number; height: number } | null>(null);
  const lastViewportElementRef = useRef<HTMLDivElement | null>(null);
  const [viewportDimensions, setViewportDimensions] = useState<{ width: number; height: number } | null>(null);
  const isZeroViewportRef = useRef(false);
  const hasFittedCurrentRunRef = useRef(false);
  const pendingFitRef = useRef(false);
  const wasEmptyRef = useRef(true);
  const currentRunIdRef = useRef<string | null>(null);

  // Gesture tracking refs
  const activePointersRef = useRef<Map<number, { clientX: number; clientY: number }>>(new Map());
  const primaryPointerIdRef = useRef<number | null>(null);
  const pinchBaselineRef = useRef<{
    s0: number;
    d0: number;
    anchorX: number;
    anchorY: number;
  } | null>(null);
  const hasDraggedRef = useRef<boolean>(false);

  const setViewportRef = useCallback((node: HTMLDivElement | null) => {
    viewportRef.current = node;
    if (node) {
      lastViewportElementRef.current = node;
    }
  }, []);

  const cancelGesture = useCallback(() => {
    const viewport = viewportRef.current ?? lastViewportElementRef.current;
    if (viewport && primaryPointerIdRef.current !== null) {
      try {
        viewport.releasePointerCapture(primaryPointerIdRef.current);
      } catch {
        // ignore
      }
    }
    for (const [pointerId] of activePointersRef.current) {
      if (viewport) {
        try {
          viewport.releasePointerCapture(pointerId);
        } catch {
          // ignore
        }
      }
    }
    activePointersRef.current.clear();
    primaryPointerIdRef.current = null;
    pinchBaselineRef.current = null;
    hasDraggedRef.current = false;
    if (!viewportRef.current) {
      lastViewportElementRef.current = null;
    }
  }, []);

  // Compute graph geometry
  const {
    waves,
    counts,
    nodeMap,
    bottleneckMap,
    criticalPath,
    criticalPathSet,
    nodePositions,
    contentWidth,
    contentHeight,
  } = useMemo(() => {
    if (!activeRun) {
      return {
        waves: [],
        counts: { total: 0, completed: 0, running: 0, failed: 0, cancelled: 0, skipped: 0 },
        nodeMap: new Map<string, DagNodeSnapshot>(),
        bottleneckMap: new Map<string, number>(),
        criticalPath: [],
        criticalPathSet: new Set<string>(),
        nodePositions: new Map<string, { x: number; y: number }>(),
        contentWidth: 0,
        contentHeight: 0,
      };
    }

    const runNodes = activeRun.nodes;
    const rawWaves = [...activeRun.waves].sort((a, b) => a.index - b.index);

    const assignedNodeIds = new Set<string>();
    rawWaves.forEach((wave) => {
      wave.nodeIds.forEach((id) => assignedNodeIds.add(id));
    });
    const unassignedNodeIds = runNodes
      .filter((n) => !assignedNodeIds.has(n.id))
      .map((n) => n.id);

    const computedWaves = [...rawWaves];
    if (unassignedNodeIds.length > 0) {
      computedWaves.push({
        index: rawWaves.length > 0 ? Math.max(...rawWaves.map((w) => w.index)) + 1 : 0,
        nodeIds: unassignedNodeIds,
      });
    }

    const runCounts = activeRun.counts || deriveDagRunCounts(runNodes);
    const nMap = new Map<string, DagNodeSnapshot>(runNodes.map((n) => [n.id, n]));
    const bMap = new Map<string, number>(
      (activeRun.bottlenecks || []).map((b) => [b.nodeId, b.blockedCount]),
    );
    const cPath = activeRun.criticalPath || [];
    const cpSet = new Set<string>(cPath);

    const positions = new Map<string, { x: number; y: number }>();
    let maxCol = 0;
    let maxRow = 0;

    computedWaves.forEach((wave, colIndex) => {
      maxCol = Math.max(maxCol, colIndex);
      wave.nodeIds.forEach((nodeId, rowIndex) => {
        maxRow = Math.max(maxRow, rowIndex);
        positions.set(nodeId, calculateNodePosition(colIndex, rowIndex));
      });
    });

    const width = PAD_X * 2 + (maxCol + 1) * CARD_WIDTH + maxCol * GAP_X;
    const height = PAD_Y * 2 + (maxRow + 1) * CARD_HEIGHT + maxRow * GAP_Y;

    return {
      waves: computedWaves,
      counts: runCounts,
      nodeMap: nMap,
      bottleneckMap: bMap,
      criticalPath: cPath,
      criticalPathSet: cpSet,
      nodePositions: positions,
      contentWidth: width,
      contentHeight: height,
    };
  }, [activeRun]);

  const isRunEmpty = !activeRun || !activeRun.nodes || activeRun.nodes.length === 0;

  const fitScale = useMemo(() => {
    const dims = viewportDimensions ?? lastDimensionsRef.current;
    if (!dims || dims.width <= 0 || dims.height <= 0 || contentWidth <= 0 || contentHeight <= 0) return 1;
    return calculateFitCamera(
      dims.width,
      dims.height,
      contentWidth,
      contentHeight,
    ).scale;
  }, [viewportDimensions, contentWidth, contentHeight]);

  const effectiveMinScale = calculateEffectiveMinScale(fitScale);

  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);

  const selectedNode = useMemo(
    () =>
      selectedNodeId && activeRun
        ? activeRun.nodes.find((n) => n.id === selectedNodeId) ?? null
        : null,
    [selectedNodeId, activeRun],
  );

  useEffect(() => {
    if (!selectedNodeId) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setSelectedNodeId(null);
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [selectedNodeId]);

  const totalStats = useMemo(() => {
    if (!activeRun?.nodes) return null;
    let totalTokens = 0;
    let hasStats = false;
    for (const node of activeRun.nodes) {
      if (node.runStats?.totalTokens) {
        totalTokens += node.runStats.totalTokens;
        hasStats = true;
      }
    }
    return hasStats ? { totalTokens } : null;
  }, [activeRun?.nodes]);

  const runDurationText = useMemo(() => {
    if (!activeRun?.startedAt) return null;
    const start = Date.parse(activeRun.startedAt);
    const end = activeRun.completedAt
      ? Date.parse(activeRun.completedAt)
      : activeRun.updatedAt
        ? Date.parse(activeRun.updatedAt)
        : Date.now();
    if (Number.isFinite(start) && Number.isFinite(end) && end >= start) {
      return formatDurationMs(end - start);
    }
    return null;
  }, [activeRun?.startedAt, activeRun?.completedAt, activeRun?.updatedAt]);

  // Run switch vs same-run update effect
  useEffect(() => {
    const runIdValue = activeRun?.runId ?? null;
    const dims = lastDimensionsRef.current;
    const isPositiveDims =
      Boolean(dims && dims.width > 0 && dims.height > 0) &&
      !isZeroViewportRef.current &&
      !(viewportRef.current && (viewportRef.current.clientWidth <= 0 || viewportRef.current.clientHeight <= 0));

    if (runIdValue !== currentRunIdRef.current) {
      currentRunIdRef.current = runIdValue;
      cancelGesture();
      hasFittedCurrentRunRef.current = false;
      wasEmptyRef.current = isRunEmpty;

      if (!isRunEmpty && contentWidth > 0 && contentHeight > 0) {
        if (isPositiveDims && dims) {
          const fitCam = calculateFitCamera(
            dims.width,
            dims.height,
            contentWidth,
            contentHeight,
          );
          setCamera(fitCam);
          hasFittedCurrentRunRef.current = true;
          pendingFitRef.current = false;
        } else {
          pendingFitRef.current = true;
        }
      } else {
        pendingFitRef.current = false;
      }
    } else if (wasEmptyRef.current && !isRunEmpty) {
      // Same-run empty -> first-nonempty transition
      wasEmptyRef.current = false;
      cancelGesture();

      if (contentWidth > 0 && contentHeight > 0) {
        if (isPositiveDims && dims) {
          const fitCam = calculateFitCamera(
            dims.width,
            dims.height,
            contentWidth,
            contentHeight,
          );
          setCamera(fitCam);
          hasFittedCurrentRunRef.current = true;
          pendingFitRef.current = false;
        } else {
          pendingFitRef.current = true;
        }
      }
    } else if (isRunEmpty) {
      wasEmptyRef.current = true;
      hasFittedCurrentRunRef.current = false;
    }
  }, [activeRun?.runId, cancelGesture, contentHeight, contentWidth, isRunEmpty, setCamera]);

  // ResizeObserver effect
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width <= 0 || height <= 0) {
          isZeroViewportRef.current = true;
          continue;
        }

        isZeroViewportRef.current = false;
        const prev = lastDimensionsRef.current;
        lastDimensionsRef.current = { width, height };
        setViewportDimensions({ width, height });

        cancelGesture();

        const needsFit =
          pendingFitRef.current ||
          (!hasFittedCurrentRunRef.current && !isRunEmpty);

        if (needsFit && contentWidth > 0 && contentHeight > 0) {
          const fitCam = calculateFitCamera(width, height, contentWidth, contentHeight);
          setCamera(fitCam);
          hasFittedCurrentRunRef.current = !isRunEmpty;
          pendingFitRef.current = false;
        } else if (!prev) {
          // Initial positive measurement: fit camera
          if (contentWidth > 0 && contentHeight > 0) {
            const fitCam = calculateFitCamera(width, height, contentWidth, contentHeight);
            setCamera(fitCam);
            if (!isRunEmpty) {
              hasFittedCurrentRunRef.current = true;
            }
          }
        } else {
          // Subsequent measurement: shift translation by half the delta
          const deltaW = width - prev.width;
          const deltaH = height - prev.height;
          if (deltaW !== 0 || deltaH !== 0) {
            const nextCam = {
              ...cameraRef.current,
              x: cameraRef.current.x + deltaW / 2,
              y: cameraRef.current.y + deltaH / 2,
            };
            setCamera(nextCam);
          }
        }
      }
    });

    observer.observe(viewport);
    return () => {
      observer.disconnect();
    };
  }, [cancelGesture, contentHeight, contentWidth, isRunEmpty, setCamera]);

  // Viewport-scoped non-passive wheel listener
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;

    const onWheel = (e: WheelEvent) => {
      // Pure horizontal wheel leaves camera unchanged and is not a camera gesture
      if (e.deltaY === 0 && e.deltaX !== 0) {
        return;
      }

      e.preventDefault();
      e.stopPropagation();

      const rect = viewport.getBoundingClientRect();
      const px = e.clientX - rect.left;
      const py = e.clientY - rect.top;

      const currentCam = cameraRef.current;
      const deltaPixels = normalizeWheelDeltaPixels(e.deltaY, e.deltaMode, rect.height);
      const clampedDelta = Math.max(-1000, Math.min(1000, deltaPixels));
      const requestedScale = currentCam.scale * Math.exp(-0.002 * clampedDelta);

      const currentFitScale =
        lastDimensionsRef.current && contentWidth > 0 && contentHeight > 0
          ? calculateFitCamera(
              lastDimensionsRef.current.width,
              lastDimensionsRef.current.height,
              contentWidth,
              contentHeight,
            ).scale
          : 1;
      const minScale = calculateEffectiveMinScale(currentFitScale);

      const nextCam = calculateZoomAtAnchor(
        currentCam,
        requestedScale,
        { x: px, y: py },
        minScale,
        MAX_SCALE,
      );
      setCamera(nextCam);
    };

    viewport.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      viewport.removeEventListener("wheel", onWheel);
    };
  }, [contentWidth, contentHeight, setCamera]);

  // Window blur & visibilitychange listeners
  useEffect(() => {
    const onBlur = () => {
      cancelGesture();
    };
    const onVisibilityChange = () => {
      if (document.hidden) {
        cancelGesture();
      }
    };

    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      cancelGesture();
    };
  }, [cancelGesture]);

  // Unmount cleanup
  useEffect(() => {
    return () => {
      cancelGesture();
    };
  }, [cancelGesture]);

  // Pointer event handlers
  const onPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement | null;
    if (
      target &&
      target.closest(
        'button, a, input, textarea, select, [contenteditable], [data-no-pan]',
      )
    ) {
      return;
    }

    const pointerType = e.pointerType || "mouse";

    if (pointerType === "mouse") {
      if (e.button !== 0 || e.buttons !== 1) return;
      if (primaryPointerIdRef.current !== null) return;
      primaryPointerIdRef.current = e.pointerId;
      activePointersRef.current.set(e.pointerId, { clientX: e.clientX, clientY: e.clientY });
      hasDraggedRef.current = false;
      try {
        e.currentTarget.setPointerCapture(e.pointerId);
      } catch {
        // ignore
      }
      e.stopPropagation();
    } else if (pointerType === "touch" || pointerType === "pen") {
      if (activePointersRef.current.size >= 2) {
        return;
      }
      activePointersRef.current.set(e.pointerId, { clientX: e.clientX, clientY: e.clientY });
      hasDraggedRef.current = false;
      try {
        e.currentTarget.setPointerCapture(e.pointerId);
      } catch {
        // ignore
      }

      if (activePointersRef.current.size === 2) {
        const pts = Array.from(activePointersRef.current.values());
        const p1 = pts[0];
        const p2 = pts[1];
        const d0 = Math.hypot(p2.clientX - p1.clientX, p2.clientY - p1.clientY);
        if (d0 >= 1 && viewportRef.current) {
          const rect = viewportRef.current.getBoundingClientRect();
          const m0x = (p1.clientX + p2.clientX) / 2 - rect.left;
          const m0y = (p1.clientY + p2.clientY) / 2 - rect.top;
          const cur = cameraRef.current;
          pinchBaselineRef.current = {
            s0: cur.scale,
            d0,
            anchorX: (m0x - cur.x) / cur.scale,
            anchorY: (m0y - cur.y) / cur.scale,
          };
        } else {
          pinchBaselineRef.current = null;
        }
      }
      e.stopPropagation();
    }
  }, []);

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!activePointersRef.current.has(e.pointerId)) return;

      const pointerType = e.pointerType || "mouse";

      if (pointerType === "mouse") {
        if (e.buttons !== 1) {
          cancelGesture();
          return;
        }
        const prev = activePointersRef.current.get(e.pointerId)!;
        const dx = e.clientX - prev.clientX;
        const dy = e.clientY - prev.clientY;
        if (dx !== 0 || dy !== 0) {
          hasDraggedRef.current = true;
          activePointersRef.current.set(e.pointerId, { clientX: e.clientX, clientY: e.clientY });
          const cur = cameraRef.current;
          setCamera({ ...cur, x: cur.x + dx, y: cur.y + dy });
        }
        e.stopPropagation();
      } else if (pointerType === "touch" || pointerType === "pen") {
        if (activePointersRef.current.size === 1) {
          const prev = activePointersRef.current.get(e.pointerId)!;
          const dx = e.clientX - prev.clientX;
          const dy = e.clientY - prev.clientY;
          activePointersRef.current.set(e.pointerId, { clientX: e.clientX, clientY: e.clientY });
          if (dx !== 0 || dy !== 0) {
            hasDraggedRef.current = true;
            const cur = cameraRef.current;
            setCamera({ ...cur, x: cur.x + dx, y: cur.y + dy });
          }
        } else if (activePointersRef.current.size === 2) {
          activePointersRef.current.set(e.pointerId, { clientX: e.clientX, clientY: e.clientY });
          hasDraggedRef.current = true;
          const pts = Array.from(activePointersRef.current.values());
          const p1 = pts[0];
          const p2 = pts[1];
          const d1 = Math.hypot(p2.clientX - p1.clientX, p2.clientY - p1.clientY);

          if (!viewportRef.current) return;
          const rect = viewportRef.current.getBoundingClientRect();
          const m1x = (p1.clientX + p2.clientX) / 2 - rect.left;
          const m1y = (p1.clientY + p2.clientY) / 2 - rect.top;

          if (!pinchBaselineRef.current && d1 >= 1) {
            const cur = cameraRef.current;
            pinchBaselineRef.current = {
              s0: cur.scale,
              d0: d1,
              anchorX: (m1x - cur.x) / cur.scale,
              anchorY: (m1y - cur.y) / cur.scale,
            };
          } else if (pinchBaselineRef.current) {
            const base = pinchBaselineRef.current;
            const dims = viewportDimensions ?? lastDimensionsRef.current;
            const currentFitScale =
              dims && dims.width > 0 && dims.height > 0 && contentWidth > 0 && contentHeight > 0
                ? calculateFitCamera(
                    dims.width,
                    dims.height,
                    contentWidth,
                    contentHeight,
                  ).scale
                : 1;
            const minScale = calculateEffectiveMinScale(currentFitScale);
            const reqScale = base.s0 * (d1 / base.d0);
            const sNew = clampScaleWithRecovery(base.s0, reqScale, minScale, MAX_SCALE);

            const t1x = m1x - sNew * base.anchorX;
            const t1y = m1y - sNew * base.anchorY;
            setCamera({ x: t1x, y: t1y, scale: sNew });
          }
        }
        e.stopPropagation();
      }
    },
    [cancelGesture, contentHeight, contentWidth, setCamera, viewportDimensions],
  );

  const handlePointerEnd = useCallback((pointerId: number) => {
    const viewport = viewportRef.current;
    if (viewport) {
      try {
        viewport.releasePointerCapture(pointerId);
      } catch {
        // ignore
      }
    }
    activePointersRef.current.delete(pointerId);
    if (primaryPointerIdRef.current === pointerId) {
      primaryPointerIdRef.current = null;
    }
    if (activePointersRef.current.size === 1) {
      pinchBaselineRef.current = null;
      const [remId, remPt] = Array.from(activePointersRef.current.entries())[0];
      activePointersRef.current.set(remId, { clientX: remPt.clientX, clientY: remPt.clientY });
    } else if (activePointersRef.current.size === 0) {
      pinchBaselineRef.current = null;
    }
  }, []);

  const onPointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      handlePointerEnd(e.pointerId);
    },
    [handlePointerEnd],
  );

  const onPointerCancel = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      handlePointerEnd(e.pointerId);
    },
    [handlePointerEnd],
  );

  const onLostPointerCapture = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      handlePointerEnd(e.pointerId);
    },
    [handlePointerEnd],
  );

  const onClickCapture = useCallback((e: React.MouseEvent) => {
    if (hasDraggedRef.current) {
      e.stopPropagation();
      e.preventDefault();
      hasDraggedRef.current = false;
    }
  }, []);

  // Camera buttons
  const handleZoomIn = useCallback(() => {
    const viewport = viewportRef.current;
    const width = viewportDimensions?.width ?? lastDimensionsRef.current?.width ?? viewport?.clientWidth ?? 1000;
    const height = viewportDimensions?.height ?? lastDimensionsRef.current?.height ?? viewport?.clientHeight ?? 700;
    const center = { x: width / 2, y: height / 2 };
    const minScale = calculateEffectiveMinScale(fitScale);
    const nextCam = calculateZoomAtAnchor(
      cameraRef.current,
      cameraRef.current.scale * 1.2,
      center,
      minScale,
      MAX_SCALE,
    );
    setCamera(nextCam);
  }, [fitScale, setCamera, viewportDimensions]);

  const handleZoomOut = useCallback(() => {
    const viewport = viewportRef.current;
    const width = viewportDimensions?.width ?? lastDimensionsRef.current?.width ?? viewport?.clientWidth ?? 1000;
    const height = viewportDimensions?.height ?? lastDimensionsRef.current?.height ?? viewport?.clientHeight ?? 700;
    const center = { x: width / 2, y: height / 2 };
    const minScale = calculateEffectiveMinScale(fitScale);
    const nextCam = calculateZoomAtAnchor(
      cameraRef.current,
      cameraRef.current.scale / 1.2,
      center,
      minScale,
      MAX_SCALE,
    );
    setCamera(nextCam);
  }, [fitScale, setCamera, viewportDimensions]);

  const handleResetZoom = useCallback(() => {
    const viewport = viewportRef.current;
    const width = viewportDimensions?.width ?? lastDimensionsRef.current?.width ?? viewport?.clientWidth ?? 1000;
    const height = viewportDimensions?.height ?? lastDimensionsRef.current?.height ?? viewport?.clientHeight ?? 700;
    const center = { x: width / 2, y: height / 2 };
    const minScale = calculateEffectiveMinScale(fitScale);
    const nextCam = calculateZoomAtAnchor(
      cameraRef.current,
      1.0,
      center,
      minScale,
      MAX_SCALE,
    );
    setCamera(nextCam);
  }, [fitScale, setCamera, viewportDimensions]);

  const handleFit = useCallback(() => {
    const width = viewportDimensions?.width ?? lastDimensionsRef.current?.width ?? viewportRef.current?.clientWidth ?? 0;
    const height = viewportDimensions?.height ?? lastDimensionsRef.current?.height ?? viewportRef.current?.clientHeight ?? 0;
    if (width > 0 && height > 0 && contentWidth > 0 && contentHeight > 0) {
      const fitCam = calculateFitCamera(width, height, contentWidth, contentHeight);
      setCamera(fitCam);
    }
  }, [contentHeight, contentWidth, setCamera, viewportDimensions]);

  if (!activeRun) {
    return (
      <div
        className="flex h-full w-full items-center justify-center bg-background text-sm text-muted-foreground select-none"
        data-testid="dag-graph-view"
      >
        <span>No dag runs yet</span>
      </div>
    );
  }

  const paneWidth =
    viewportDimensions?.width ?? lastDimensionsRef.current?.width ?? viewportRef.current?.clientWidth ?? 0;
  const isCompact = paneWidth > 0 && paneWidth < 400;

  return (
    <div
      className="relative flex h-full w-full flex-col overflow-hidden bg-background text-foreground select-none"
      data-testid="dag-graph-view"
      data-run-id={activeRun.runId}
    >
      <div
        className={
          isCompact
            ? "flex min-h-9 max-h-[60%] shrink-0 flex-col items-start justify-between gap-x-3 gap-y-1.5 border-b border-border/40 bg-card/60 px-2 py-1.5 text-xs text-muted-foreground backdrop-blur-sm overflow-hidden"
            : "flex min-h-9 shrink-0 flex-row items-center justify-between gap-x-3 gap-y-1 border-b border-border/40 bg-card/60 px-2 sm:px-3 py-1.5 text-xs text-muted-foreground backdrop-blur-sm"
        }
        data-testid="dag-header"
      >
        <div
          className={
            isCompact
              ? "flex min-h-0 min-w-0 max-w-full w-full flex-1 flex-col gap-x-3 gap-y-1 overflow-y-auto overflow-x-hidden scrollbar-none order-2"
              : "flex min-w-0 flex-1 flex-row items-center gap-x-3 gap-y-1 overflow-visible order-1"
          }
        >
          <div className="flex min-w-0 max-w-full flex-wrap items-center gap-x-2 gap-y-0.5 font-medium">
            {showRunName && (
              <span className="flex min-w-0 items-center gap-1.5">
                <span className="truncate text-foreground">{activeRun.name}</span>
                <span className="text-muted-foreground/60">&mdash;</span>
              </span>
            )}
            <span className="inline-flex flex-wrap items-center gap-x-1 font-mono text-muted-foreground">
              <span>{counts.completed}/{counts.total} done, </span>
              <span>{counts.running} running</span>
              {runDurationText && <span>• {runDurationText}</span>}
              {totalStats && <span>• {formatTokenCount(totalStats.totalTokens)} tok</span>}
              {activeRun.amendCount > 0 && (
                <span className="rounded bg-amber-500/15 text-amber-500 px-1 py-0.2 text-[10px] ml-0.5">
                  amend x{activeRun.amendCount}
                </span>
              )}
            </span>
          </div>
          <div
            className={
              isCompact
                ? "flex min-w-0 max-w-full flex-wrap items-center gap-x-2.5 gap-y-0.5 font-mono text-[10px]"
                : "flex min-w-0 max-w-full flex-wrap items-center gap-x-2.5 gap-y-0.5 font-mono text-[10px] ml-auto"
            }
            data-testid="dag-legend"
          >
            <span className="text-indigo-500">▶ running</span>
            <span className="text-foreground/60">✓ done</span>
            <span className="text-muted-foreground">◌ waiting</span>
            <span className="text-rose-500">✗ failed</span>
          </div>
        </div>

        <div
          className={
            isCompact
              ? "flex shrink-0 max-w-full flex-wrap items-center gap-1 rounded border border-border/60 bg-background/50 p-0.5 order-1"
              : "flex shrink-0 max-w-full flex-wrap items-center gap-1 rounded border border-border/60 bg-background/50 p-0.5 order-2"
          }
          data-testid="dag-controls"
        >
            <button
              type="button"
              aria-label="Zoom out"
              aria-disabled={camera.scale <= effectiveMinScale + 1e-6 ? "true" : "false"}
              onClick={handleZoomOut}
              className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring aria-disabled:cursor-default aria-disabled:opacity-35"
            >
              <Minus className="size-3" />
            </button>
            <button
              type="button"
              aria-label="Reset zoom to 100%"
              onClick={handleResetZoom}
              className="flex h-5 items-center justify-center rounded px-1.5 font-mono text-[10px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              {Math.round(camera.scale * 100)}%
            </button>
            <button
              type="button"
              aria-label="Zoom in"
              aria-disabled={camera.scale >= MAX_SCALE - 1e-6 ? "true" : "false"}
              onClick={handleZoomIn}
              className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring aria-disabled:cursor-default aria-disabled:opacity-35"
            >
              <Plus className="size-3" />
            </button>
            <button
              type="button"
              aria-label="Fit graph"
              onClick={handleFit}
              className="flex h-5 items-center justify-center rounded px-1.5 font-mono text-[10px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              Fit
            </button>
          </div>
      </div>

      <div
        ref={setViewportRef}
        className="relative flex-1 min-h-0 min-w-0 overflow-hidden select-none"
        style={{ touchAction: "none", overscrollBehavior: "none" }}
        data-testid="dag-viewport"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
        onLostPointerCapture={onLostPointerCapture}
        onClickCapture={onClickCapture}
      >
        <div
          data-testid="dag-world"
          className="relative"
          style={{
            transformOrigin: "0 0",
            transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.scale})`,
            width: contentWidth,
            height: contentHeight,
          }}
        >
          <DagEdgeLayer
            edges={activeRun.edges}
            nodePositions={nodePositions}
            criticalPath={criticalPath}
            width={contentWidth}
            height={contentHeight}
          />

          {waves.map((wave, colIndex) => (
            <React.Fragment key={wave.index}>
              <div
                data-testid="dag-wave-column"
                data-wave-index={wave.index}
                style={{
                  position: "absolute",
                  left: calculateNodePosition(colIndex, 0).x,
                  top: PAD_Y - WAVE_LABEL_HEIGHT - 6,
                  width: CARD_WIDTH,
                }}
                className="font-mono text-[10px] font-medium text-muted-foreground/70 uppercase tracking-wider select-none truncate"
              >
                wave {colIndex + 1}
              </div>
              {wave.nodeIds.map((nodeId, rowIndex) => {
                const node = nodeMap.get(nodeId);
                if (!node) return null;
                const position = calculateNodePosition(colIndex, rowIndex);
                return (
                  <DagNodeCard
                    key={node.id}
                    node={node}
                    isCriticalPath={criticalPathSet.has(node.id)}
                    blockedCount={bottleneckMap.get(node.id) ?? 0}
                    isSelected={node.id === selectedNodeId}
                    onClick={() => setSelectedNodeId(node.id)}
                    style={{
                      position: "absolute",
                      left: position.x,
                      top: position.y,
                      height: CARD_HEIGHT,
                      width: CARD_WIDTH,
                    }}
                  />
                );
              })}
            </React.Fragment>
          ))}
        </div>
      </div>

      <DagNodeInspector
        node={selectedNode}
        projectPath={projectPath}
        allNodes={activeRun.nodes}
        onClose={() => setSelectedNodeId(null)}
        onSelectNode={(id) => setSelectedNodeId(id)}
      />
    </div>
  );
}
