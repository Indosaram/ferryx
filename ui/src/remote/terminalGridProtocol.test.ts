import { describe, expect, it } from "vitest";

import {
  applyGridFrame,
  decodeGridAttrs,
  parseGridFrame,
  type FullGridFrame,
  type TerminalGridState,
} from "./terminalGridProtocol";

const cursor = {
  x: 1,
  y: 0,
  visible: true,
  blinking: false,
  wideTail: false,
  visualStyle: "block" as const,
};

describe("terminal grid protocol", () => {
  it("parses a full grid frame and ignores arbitrary non-grid text payloads", () => {
    const frame = parseGridFrame(JSON.stringify({
      type: "grid",
      cols: 8,
      rows: 2,
      cursor,
      lines: [
        { index: 0, runs: [{ text: "hello", fg: [255, 0, 0], bg: null, attrs: 1 }] },
        { index: 1, runs: [] },
      ],
    }));

    expect(frame).toEqual({
      type: "grid",
      cols: 8,
      rows: 2,
      cursor,
      lines: [
        { index: 0, runs: [{ text: "hello", fg: [255, 0, 0], bg: null, attrs: 1 }] },
        { index: 1, runs: [] },
      ],
    });
    expect(parseGridFrame("ready> ")).toBeNull();
    expect(parseGridFrame('{"type":"notice","message":"connected"}')).toBeNull();
  });

  it("patches only named lines in a diff frame and always adopts its cursor", () => {
    const full: FullGridFrame = {
      type: "grid",
      cols: 6,
      rows: 2,
      cursor,
      lines: [
        { index: 0, runs: [{ text: "first", fg: null, bg: null, attrs: 0 }] },
        { index: 1, runs: [{ text: "second", fg: null, bg: null, attrs: 0 }] },
      ],
    };
    const initial: TerminalGridState = applyGridFrame(null, full);
    const nextCursor = { ...cursor, x: 3, y: 1, visualStyle: "bar" as const };
    const next = applyGridFrame(initial, {
      type: "gridDiff",
      cols: 6,
      rows: 2,
      cursor: nextCursor,
      lines: [{ index: 1, runs: [{ text: "changed", fg: null, bg: null, attrs: 0 }] }],
    });

    expect(next.lines[0]).toEqual(initial.lines[0]);
    expect(next.lines[1]).toEqual({ index: 1, runs: [{ text: "changed", fg: null, bg: null, attrs: 0 }] });
    expect(next.cursor).toEqual(nextCursor);
  });

  it("decodes the attrs bitmask", () => {
    expect(decodeGridAttrs(0)).toEqual({ bold: false, italic: false, underline: false, inverse: false });
    expect(decodeGridAttrs(15)).toEqual({ bold: true, italic: true, underline: true, inverse: true });
  });

  it("preserves wide CJK run text unchanged", () => {
    const frame = parseGridFrame(JSON.stringify({
      type: "grid",
      cols: 4,
      rows: 1,
      cursor,
      lines: [{ index: 0, runs: [{ text: "한글", fg: null, bg: null, attrs: 0 }] }],
    }));

    expect(frame?.lines[0]?.runs[0]?.text).toBe("한글");
  });
});
