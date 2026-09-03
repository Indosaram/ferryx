export type GridColor = readonly [number, number, number];

export type GridCursorVisualStyle = "bar" | "block" | "underline" | "blockHollow";

export type GridCursor = {
  readonly x: number;
  readonly y: number;
  readonly visible: boolean;
  readonly blinking: boolean;
  readonly wideTail: boolean;
  readonly visualStyle: GridCursorVisualStyle;
};

export type GridRun = {
  readonly text: string;
  readonly fg: GridColor | null;
  readonly bg: GridColor | null;
  readonly attrs: number;
  /** Grid columns this run occupies (wide cells count 2). Absent on legacy frames. */
  readonly cells?: number;
};

export type GridLine = {
  readonly index: number;
  readonly runs: readonly GridRun[];
};

type GridFrameBase = {
  readonly cols: number;
  readonly rows: number;
  readonly cursor: GridCursor;
  readonly lines: readonly GridLine[];
};

export type FullGridFrame = GridFrameBase & { readonly type: "grid" };
export type GridDiffFrame = GridFrameBase & { readonly type: "gridDiff" };
export type GridFrame = FullGridFrame | GridDiffFrame;

export type TerminalGridState = {
  readonly cols: number;
  readonly rows: number;
  readonly cursor: GridCursor;
  readonly lines: readonly GridLine[];
};

export type DecodedGridAttrs = {
  readonly bold: boolean;
  readonly italic: boolean;
  readonly underline: boolean;
  readonly inverse: boolean;
};

const CURSOR_STYLES = new Set<GridCursorVisualStyle>([
  "bar",
  "block",
  "underline",
  "blockHollow",
]);

export function decodeGridAttrs(attrs: number): DecodedGridAttrs {
  return {
    bold: (attrs & 1) !== 0,
    italic: (attrs & 2) !== 0,
    underline: (attrs & 4) !== 0,
    inverse: (attrs & 8) !== 0,
  };
}

/** East Asian Wide / Fullwidth ranges (unicode-width compatible subset). */
const WIDE_RANGES: readonly (readonly [number, number])[] = [
  [0x1100, 0x115f], // Hangul Jamo leading consonants
  [0x2e80, 0x303e], // CJK Radicals .. CJK Symbols and Punctuation
  [0x3041, 0x33ff], // Hiragana .. CJK Compatibility
  [0x3400, 0x4dbf], // CJK Extension A
  [0x4e00, 0x9fff], // CJK Unified Ideographs
  [0xa000, 0xa4cf], // Yi Syllables and Radicals
  [0xac00, 0xd7a3], // Hangul Syllables
  [0xf900, 0xfaff], // CJK Compatibility Ideographs
  [0xfe10, 0xfe19], // Vertical Forms
  [0xfe30, 0xfe6f], // CJK Compatibility Forms
  [0xff00, 0xff60], // Fullwidth Forms
  [0xffe0, 0xffe6], // Fullwidth Signs
  [0x1f300, 0x1f64f], // Emoji (wide subset)
  [0x1f900, 0x1f9ff], // Supplemental Symbols and Pictographs (wide subset)
  [0x20000, 0x2fffd], // CJK Extension B..
  [0x30000, 0x3fffd], // CJK Extension G..
];

/**
 * Fallback cell-width estimate for server frames that predate the per-run
 * `cells` field. The daemon reports exact counts; this only matters across a
 * version skew between the gateway and a cached web client.
 */
export function estimateCellWidth(text: string): number {
  let width = 0;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    let charWidth = 1;
    for (const [start, end] of WIDE_RANGES) {
      if (code >= start && code <= end) {
        charWidth = 2;
        break;
      }
    }
    width += charWidth;
  }
  return width;
}

export function parseGridFrame(text: string): GridFrame | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }

  if (!isRecord(value) || (value.type !== "grid" && value.type !== "gridDiff")) return null;
  if (!isPositiveInteger(value.cols) || !isPositiveInteger(value.rows)) return null;
  const cursor = parseCursor(value.cursor);
  const lines = parseLines(value.lines);
  if (!cursor || !lines) return null;

  return {
    type: value.type,
    cols: value.cols,
    rows: value.rows,
    cursor,
    lines,
  };
}

export function applyGridFrame(state: TerminalGridState | null, frame: GridFrame): TerminalGridState {
  const lines = blankLines(frame.rows);

  if (frame.type === "gridDiff" && state) {
    for (const line of state.lines) {
      if (line.index >= 0 && line.index < frame.rows) lines[line.index] = line;
    }
  }

  for (const line of frame.lines) {
    if (line.index >= 0 && line.index < frame.rows) lines[line.index] = line;
  }

  return {
    cols: frame.cols,
    rows: frame.rows,
    cursor: frame.cursor,
    lines,
  };
}

function blankLines(rows: number): GridLine[] {
  return Array.from({ length: rows }, (_, index) => ({ index, runs: [] }));
}

function parseCursor(value: unknown): GridCursor | null {
  if (!isRecord(value)) return null;
  if (!isNonNegativeInteger(value.x) || !isNonNegativeInteger(value.y)) return null;
  if (typeof value.visible !== "boolean" || typeof value.blinking !== "boolean" || typeof value.wideTail !== "boolean") {
    return null;
  }
  if (typeof value.visualStyle !== "string" || !CURSOR_STYLES.has(value.visualStyle as GridCursorVisualStyle)) {
    return null;
  }
  return {
    x: value.x,
    y: value.y,
    visible: value.visible,
    blinking: value.blinking,
    wideTail: value.wideTail,
    visualStyle: value.visualStyle as GridCursorVisualStyle,
  };
}

function parseLines(value: unknown): GridLine[] | null {
  if (!Array.isArray(value)) return null;
  const lines: GridLine[] = [];
  for (const item of value) {
    if (!isRecord(item) || !isNonNegativeInteger(item.index) || !Array.isArray(item.runs)) return null;
    const runs: GridRun[] = [];
    for (const runValue of item.runs) {
      const run = parseRun(runValue);
      if (!run) return null;
      runs.push(run);
    }
    lines.push({ index: item.index, runs });
  }
  return lines;
}

function parseRun(value: unknown): GridRun | null {
  if (!isRecord(value) || typeof value.text !== "string" || !isNonNegativeInteger(value.attrs)) return null;
  const fg = parseColor(value.fg);
  const bg = parseColor(value.bg);
  if (fg === undefined || bg === undefined) return null;
  const cells = isNonNegativeInteger(value.cells) ? value.cells : undefined;
  return cells === undefined
    ? { text: value.text, fg, bg, attrs: value.attrs }
    : { text: value.text, fg, bg, attrs: value.attrs, cells };
}

function parseColor(value: unknown): GridColor | null | undefined {
  if (value === null) return null;
  if (!Array.isArray(value) || value.length !== 3) return undefined;
  if (!value.every((channel) => Number.isInteger(channel) && channel >= 0 && channel <= 255)) return undefined;
  return [value[0] as number, value[1] as number, value[2] as number];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isPositiveInteger(value: unknown): value is number {
  return Number.isInteger(value) && (value as number) > 0;
}

function isNonNegativeInteger(value: unknown): value is number {
  return Number.isInteger(value) && (value as number) >= 0;
}
