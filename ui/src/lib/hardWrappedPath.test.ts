import { describe, expect, it } from "vitest";
import { terminalWrapsInto, tuiWrapIndent, type WrappedRow } from "./hardWrappedPath";

describe("terminalWrapsInto", () => {
  it("trusts the terminal's soft-wrap flag over row shape", () => {
    const full = "a".repeat(20);
    expect(terminalWrapsInto({ text: full, wrapped: false }, { text: "bcd" }, 20)).toBe(false);
    expect(terminalWrapsInto({ text: "short ", wrapped: true }, { text: " x" }, 20)).toBe(true);
  });

  it("falls back to a full row with no blank at the boundary when the flag is absent", () => {
    expect(terminalWrapsInto({ text: "a".repeat(20) }, { text: "bcd" }, 20)).toBe(true);
    expect(terminalWrapsInto({ text: `${"a".repeat(19)} ` }, { text: "bcd" }, 20)).toBe(false);
    expect(terminalWrapsInto({ text: "a".repeat(20) }, { text: " bcd" }, 20)).toBe(false);
    expect(terminalWrapsInto({ text: "src/short/" }, { text: "index.ts" }, 80)).toBe(false);
  });
});

describe("tuiWrapIndent", () => {
  it("joins a path a TUI word-wrapped after a separator short of the terminal width", () => {
    const from: WrappedRow = { text: "/Users/indo/.aside/u/1/sessions/2026-11-02_sIAW3lPLLPzn4OMC/" };
    expect(tuiWrapIndent(from, { text: "qr-fresh.png" }, 66)).toBe(0);
  });

  it("drops the continuation row's margin", () => {
    expect(tuiWrapIndent({ text: "  /very/long/directory/name/" }, { text: "  file.ts" }, 34)).toBe(2);
  });

  it("joins a token the TUI had to break because it fills the row", () => {
    const from: WrappedRow = { text: "  https://example.com/a-very-long-path-segment-that-did-not-fit-on-one-ro" };
    expect(tuiWrapIndent(from, { text: "  w?query=1 and more" }, 80)).toBe(2);
  });

  it("does not join when the next word would have fit beside the row", () => {
    expect(tuiWrapIndent({ text: "see src/" }, { text: "next.ts and more" }, 80)).toBeNull();
    expect(tuiWrapIndent({ text: "ends with a word" }, { text: "next line" }, 80)).toBeNull();
  });

  it("does not join a short complete token followed by a long next row", () => {
    const from: WrappedRow = { text: "Open https://ferryx.dev/docs" };
    expect(tuiWrapIndent(from, { text: "a".repeat(70) }, 80)).toBeNull();
  });

  it("never joins a row that starts a new URL", () => {
    const from: WrappedRow = { text: `${"x".repeat(70)}/` };
    expect(tuiWrapIndent(from, { text: "https://ferryx.dev" }, 80)).toBeNull();
  });

  it("handles empty rows without joining", () => {
    expect(tuiWrapIndent({ text: "" }, { text: "valid_path.ts" }, 80)).toBeNull();
    expect(tuiWrapIndent({ text: "dir/" }, { text: "" }, 80)).toBeNull();
  });
});
