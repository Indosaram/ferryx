/**
 * Read-only preview document helpers (plan task 3).
 *
 * Pure functions only: URL trust decisions, Unicode-scalar caret math, literal
 * search and the mdast transform that turns raw HTML into inert literal text.
 * `FilePreviewText.tsx` is the only renderer; nothing here touches the DOM,
 * React or Tauri, so every rule below is directly testable.
 *
 * Trust model: the renderer never fabricates a filesystem URL. Relative targets
 * are handed to the parent capability (which re-validates containment in the
 * backend); everything else is refused here, before it can reach the DOM.
 */

export type PreviewRejectionReason =
  | "unsafe-scheme"
  | "remote"
  | "absolute-path"
  | "traversal"
  | "empty";

export type PreviewLink =
  | { kind: "external"; url: string }
  | { kind: "document"; relativePath: string }
  | { kind: "fragment"; id: string }
  | { kind: "rejected"; reason: PreviewRejectionReason; raw: string };

export type PreviewImage =
  | { kind: "local"; relativePath: string }
  | { kind: "rejected"; reason: PreviewRejectionReason; raw: string };

/**
 * Whitespace and C0/C1 controls are stripped before scheme sniffing because
 * WebViews ignore them too: `java\tscript:` navigates, so it must be refused.
 */
function schemeCandidate(value: string): string {
  // eslint-disable-next-line no-control-regex
  return value.replace(/[\u0000-\u0020\u007f-\u009f]/g, "");
}

const SCHEME_PATTERN = /^([a-z][a-z0-9+.-]*):/i;
const WINDOWS_DRIVE_PATTERN = /^[a-z]:[\\/]/i;

/** Percent-decoding is tolerant: an invalid sequence keeps its literal form. */
function decodeLoosely(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function hasTraversalSegment(value: string): boolean {
  const decoded = decodeLoosely(value);
  return decoded
    .split(/[\\/]/)
    .some((segment) => segment === ".." || segment === "..." || segment.trim() === "..");
}

type RelativeVerdict =
  | { ok: true; relativePath: string }
  | { ok: false; reason: PreviewRejectionReason };

/** Shared refusal ladder for anchors and images; order is significant. */
function classifyRelativeTarget(raw: string, remoteSchemeIsRemote: boolean): RelativeVerdict | "external" {
  const cleaned = schemeCandidate(raw);
  if (cleaned.length === 0) return { ok: false, reason: "empty" };
  if (WINDOWS_DRIVE_PATTERN.test(cleaned)) return { ok: false, reason: "absolute-path" };

  const scheme = SCHEME_PATTERN.exec(cleaned);
  if (scheme) {
    const protocol = scheme[1].toLowerCase();
    if (protocol === "http" || protocol === "https") {
      return remoteSchemeIsRemote ? { ok: false, reason: "remote" } : "external";
    }
    return { ok: false, reason: "unsafe-scheme" };
  }

  if (cleaned.startsWith("//")) return { ok: false, reason: "remote" };
  if (cleaned.startsWith("/") || cleaned.startsWith("\\")) return { ok: false, reason: "absolute-path" };
  if (hasTraversalSegment(cleaned)) return { ok: false, reason: "traversal" };
  return { ok: true, relativePath: raw.trim() };
}

/**
 * Decides what a Markdown anchor may do. http(s) opens externally on explicit
 * click, `#fragment` scrolls inside the rendered document, a relative path
 * becomes a new preview request, and everything else is refused.
 */
export function classifyPreviewLink(href: string): PreviewLink {
  const trimmed = href.trim();
  if (trimmed.startsWith("#")) {
    // Keep spaces here: only the slug rules may collapse them.
    return { kind: "fragment", id: previewAnchorId(decodeLoosely(trimmed.slice(1))) };
  }

  const verdict = classifyRelativeTarget(href, false);
  if (verdict === "external") return { kind: "external", url: href.trim() };
  if (!verdict.ok) return { kind: "rejected", reason: verdict.reason, raw: href };
  return { kind: "document", relativePath: verdict.relativePath };
}

/** Images may only come from the parent capability: remote loads are disabled. */
export function classifyPreviewImage(src: string): PreviewImage {
  const verdict = classifyRelativeTarget(src, true);
  if (verdict === "external") return { kind: "rejected", reason: "remote", raw: src };
  if (!verdict.ok) return { kind: "rejected", reason: verdict.reason, raw: src };
  return { kind: "local", relativePath: verdict.relativePath };
}

/** Splits on LF, CRLF and lone CR; a trailing break yields a final empty line. */
export function splitPreviewLines(text: string): string[] {
  return text.split(/\r\n|\r|\n/);
}

export type PreviewCaret = {
  readonly line: number;
  readonly column: number;
  /** UTF-16 offset of the column inside its line, for DOM range math. */
  readonly offset: number;
  readonly clamped: boolean;
};

/**
 * Clamps a 1-based line/column target onto the document. Columns count Unicode
 * scalars (so an astral emoji is one column), and any clamp is reported so the
 * renderer can show it instead of silently lying about the location.
 */
export function clampPreviewTarget(
  target: { readonly line: number; readonly col: number | null },
  lines: readonly string[],
): PreviewCaret {
  const lineCount = Math.max(lines.length, 1);
  let clamped = false;

  let line = Number.isFinite(target.line) ? Math.trunc(target.line) : 1;
  if (line < 1) {
    line = 1;
    clamped = true;
  }
  if (line > lineCount) {
    line = lineCount;
    clamped = true;
  }

  const scalars = Array.from(lines[line - 1] ?? "");
  const maxColumn = scalars.length + 1;
  let column = 1;
  if (target.col != null) {
    column = Number.isFinite(target.col) ? Math.trunc(target.col) : 1;
    if (column < 1) {
      column = 1;
      clamped = true;
    }
    if (column > maxColumn) {
      column = maxColumn;
      clamped = true;
    }
  }

  const offset = scalars.slice(0, column - 1).join("").length;
  return { line, column, offset, clamped };
}

export type PreviewMatch = { readonly line: number; readonly start: number; readonly end: number };

/**
 * Literal, case-insensitive, non-overlapping search. The query is never
 * compiled as a regular expression, and case folding is skipped for any line
 * whose lowercase form changes length so reported offsets stay exact.
 */
export function findPreviewMatches(lines: readonly string[], query: string): PreviewMatch[] {
  if (query.trim().length === 0) return [];
  const matches: PreviewMatch[] = [];
  const loweredQuery = query.toLowerCase();

  lines.forEach((line, index) => {
    const lowered = line.toLowerCase();
    const haystack = lowered.length === line.length ? lowered : line;
    const needle = lowered.length === line.length ? loweredQuery : query;
    if (needle.length === 0) return;
    let from = 0;
    for (;;) {
      const start = haystack.indexOf(needle, from);
      if (start < 0) break;
      matches.push({ line: index, start, end: start + needle.length });
      from = start + needle.length;
    }
  });

  return matches;
}

export type MdastNode = { type: string; value?: string; children?: MdastNode[] };

const BLOCK_PARENTS = new Set(["root", "blockquote", "listItem", "footnoteDefinition"]);

/**
 * remark plugin body: rewrites every `html` node into literal text so raw HTML
 * is shown, never mounted. Combined with react-markdown's default (no
 * `rehype-raw`), no author-supplied element or script can ever reach the DOM.
 */
export function literalizeHtmlNodes(tree: unknown): void {
  const node = tree as MdastNode | null;
  if (!node || typeof node !== "object" || !Array.isArray(node.children)) return;

  const inBlockContext = BLOCK_PARENTS.has(node.type);
  node.children = node.children.map((child) => {
    if (child && child.type === "html") {
      const text: MdastNode = { type: "text", value: child.value ?? "" };
      return inBlockContext ? { type: "paragraph", children: [text] } : text;
    }
    literalizeHtmlNodes(child);
    return child;
  });
}

/** remark plugin: `remarkPlugins={[remarkGfm, remarkLiteralHtml]}`. */
export function remarkLiteralHtml() {
  return (tree: unknown) => {
    literalizeHtmlNodes(tree);
  };
}

/**
 * Heading slug used for in-document fragment links. Letters and numbers of any
 * script survive, so Hangul or CJK headings stay linkable.
 */
export function previewAnchorId(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s+/g, "-");
}
