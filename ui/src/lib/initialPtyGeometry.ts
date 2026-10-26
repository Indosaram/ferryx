import { invoke, isTauri } from "@tauri-apps/api/core";

/** Initial PTY grid measured from a real pane rect and native font cell metrics. */
export type InitialPtyGrid = { readonly cols: number; readonly rows: number };

/** Physical-pixel cell size reported by the native font manager at a given display scale. */
export type NativeCellMetrics = { readonly widthPx: number; readonly heightPx: number };

/**
 * Where the PTY that is about to be spawned will be presented.
 * - `session`: the pane for this frontend session is already mounted (split after commit, restart).
 * - `newTab`: a new single-leaf tab will fill this tab group's body.
 */
export type InitialGeometryTarget =
  | { readonly kind: "session"; readonly sessionId: string }
  | { readonly kind: "newTab"; readonly groupId: string | null };

const MAX_SPAWN_DIMENSION = 1000;

type Rect = { readonly x: number; readonly y: number; readonly width: number; readonly height: number };

/**
 * Mirrors the native layout path: `snapBoundsToDevicePixels` in NativeTerminalPane followed by
 * `SurfaceCompositionLayout::compute` (round to physical pixels, floor by cell size). Returns
 * null when any input is unusable, so callers fall back to the daemon defaults instead of
 * sending a guessed grid.
 */
export function computeInitialPtyGrid(rect: Rect, scaleFactor: number, cell: NativeCellMetrics): InitialPtyGrid | null {
  const inputs = [rect.x, rect.y, rect.width, rect.height, scaleFactor, cell.widthPx, cell.heightPx];
  if (!inputs.every(Number.isFinite)) return null;
  if (scaleFactor <= 0 || cell.widthPx <= 0 || cell.heightPx <= 0) return null;
  const snap = (value: number) => Math.round(value * scaleFactor) / scaleFactor;
  const left = Math.max(0, snap(rect.x));
  const top = Math.max(0, snap(rect.y));
  const right = Math.max(left, snap(rect.x + rect.width));
  const bottom = Math.max(top, snap(rect.y + rect.height));
  const physicalWidth = Math.round((right - left) * scaleFactor);
  const physicalHeight = Math.round((bottom - top) * scaleFactor);
  const cols = Math.floor(physicalWidth / cell.widthPx);
  const rows = Math.floor(physicalHeight / cell.heightPx);
  if (cols < 1 || rows < 1 || cols > MAX_SPAWN_DIMENSION || rows > MAX_SPAWN_DIMENSION) return null;
  return { cols, rows };
}

export async function fetchNativeCellMetrics(scaleFactor: number): Promise<NativeCellMetrics | null> {
  if (!isTauri()) return null;
  try {
    const metrics = await invoke<NativeCellMetrics>("cmd_terminal_cell_metrics", { scaleFactor });
    return metrics && metrics.widthPx > 0 && metrics.heightPx > 0 ? metrics : null;
  } catch {
    return null;
  }
}

function currentScaleFactor(): number {
  return typeof window !== "undefined" && typeof window.devicePixelRatio === "number" && window.devicePixelRatio > 0
    ? window.devicePixelRatio
    : 1;
}

function escapeAttribute(value: string): string {
  return typeof CSS !== "undefined" && typeof CSS.escape === "function" ? CSS.escape(value) : value.replace(/["\\]/g, "\\$&");
}

export function initialGeometryElement(target: InitialGeometryTarget): Element | null {
  if (typeof document === "undefined") return null;
  if (target.kind === "session") {
    return document.querySelector(
      `[data-testid="native-terminal-viewport"][data-frontend-session-id="${escapeAttribute(target.sessionId)}"]`,
    );
  }
  if (!target.groupId) return null;
  return document.querySelector(`[data-tab-group-body-id="${escapeAttribute(target.groupId)}"]`);
}

/** The rect is read synchronously, before the metrics await, so it reflects the commit just made. */
export async function measureInitialPtyGrid(
  element: Element | null,
  fetchMetrics: (scaleFactor: number) => Promise<NativeCellMetrics | null> = fetchNativeCellMetrics,
): Promise<InitialPtyGrid | null> {
  if (!element) return null;
  const rect = element.getBoundingClientRect();
  if (!(rect.width > 0 && rect.height > 0)) return null;
  const scaleFactor = currentScaleFactor();
  const cell = await fetchMetrics(scaleFactor);
  return cell ? computeInitialPtyGrid(rect, scaleFactor, cell) : null;
}

export function measureInitialGeometry(target: InitialGeometryTarget): Promise<InitialPtyGrid | null> {
  return measureInitialPtyGrid(initialGeometryElement(target));
}

/** Spread into a spawn request: measured fields only, nothing when geometry is unavailable. */
export function initialGridFields(grid: InitialPtyGrid | null | undefined): { cols?: number; rows?: number } {
  return grid ? { cols: grid.cols, rows: grid.rows } : {};
}
