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
  return { text: value.text, fg, bg, attrs: value.attrs };
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
