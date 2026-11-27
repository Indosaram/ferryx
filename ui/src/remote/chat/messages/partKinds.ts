import type { ReferencePart } from "../referenceTypes";

/**
 * Part kinds the reference part model may grow into (`plan`, `htmlRender`).
 * The frozen union does not name them yet, so these are runtime guards over the
 * wire shape: an unknown kind still renders nothing.
 *
 * The guards return plain booleans and the readers take `unknown` on purpose: a
 * type predicate would intersect the new kind with the frozen union, and that
 * intersection is `never`.
 */

function partField(part: unknown, key: string): unknown {
  if (typeof part !== "object" || part === null) return undefined;
  return (part as Record<string, unknown>)[key];
}

export function isPlanPart(part: ReferencePart): boolean {
  return partField(part, "kind") === "plan";
}

export function isHtmlRenderPart(part: ReferencePart): boolean {
  return partField(part, "kind") === "htmlRender";
}

export function planPartMarkdown(part: unknown): string {
  const value = partField(part, "markdown") ?? partField(part, "text");
  return typeof value === "string" ? value : "";
}

export function htmlRenderPartHtml(part: unknown): string {
  const value = partField(part, "html");
  return typeof value === "string" ? value : "";
}

export function htmlRenderPartTitle(part: unknown): string | undefined {
  const value = partField(part, "title");
  return typeof value === "string" && value.trim().length > 0 ? value : undefined;
}

/**
 * A tool part's exit code, when the transcript recorded one. The frozen wire
 * type does not name it; the guard keeps a bad payload from rendering "exit NaN".
 */
export function partExitCode(part: ReferencePart): number | null {
  if (part.kind !== "tool") return null;
  const value = (part as ReferencePart & { exitCode?: unknown }).exitCode;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * Live tail for a running command (activity-log.md): long output shows only its
 * most recent part. A finished call keeps its whole output.
 */
export const LIVE_TAIL_LINES = 40;

export function tailLines(text: string, max = LIVE_TAIL_LINES): string {
  if (!text) return "";
  const lines = text.split("\n");
  if (lines.length <= max) return text;
  return lines.slice(-max).join("\n");
}
