import type { Epoch } from "../../../lib/scopedContracts";

export interface DesignIdentity {
  browserId: string;
  webviewLabel: string;
  generation: Epoch;
  operationId: string;
  viewportRevision: number;
}
export interface Rect { x: number; y: number; width: number; height: number }
export interface Viewport { width: number; height: number; dpr: number; zoom: number }
export interface Selection {
  identity: DesignIdentity;
  mode: "element" | "rectangle";
  rect: Rect;
  viewport: Viewport;
  url: string;
  title: string;
  element?: { tag: string; id: string; classes: string[]; selector: string; ancestry: string[]; css: Record<string, string>; contextUnavailable?: string };
}
export type GuestEvent = { type: "selected"; selection: Selection } | { type: "cancelled" | "invalidated"; identity: DesignIdentity };
export function sameIdentity(a: DesignIdentity, b: DesignIdentity): boolean {
  return a.browserId === b.browserId && a.webviewLabel === b.webviewLabel && a.generation === b.generation && a.operationId === b.operationId && a.viewportRevision === b.viewportRevision;
}
export function normalizeRect(x1: number, y1: number, x2: number, y2: number, width: number, height: number): Rect {
  if (![x1, y1, x2, y2, width, height].every(Number.isFinite) || width <= 0 || height <= 0) throw new Error("INVALID_GEOMETRY");
  const x = Math.max(0, Math.min(x1, x2)), y = Math.max(0, Math.min(y1, y2));
  const right = Math.min(width, Math.max(x1, x2)), bottom = Math.min(height, Math.max(y1, y2));
  if (right <= x || bottom <= y) throw new Error("EMPTY_SELECTION");
  return { x, y, width: right - x, height: bottom - y };
}
