import { describe, expect, it } from "vitest";

import {
  classifyPreviewImage,
  classifyPreviewLink,
  clampPreviewTarget,
  findPreviewMatches,
  literalizeHtmlNodes,
  previewAnchorId,
  splitPreviewLines,
} from "./previewMarkdown";

describe("classifyPreviewLink", () => {
  it("accepts http(s) anchors regardless of scheme case", () => {
    expect(classifyPreviewLink("https://example.com/docs")).toEqual({
      kind: "external",
      url: "https://example.com/docs",
    });
    expect(classifyPreviewLink("HTTP://Example.com/a")).toEqual({
      kind: "external",
      url: "HTTP://Example.com/a",
    });
  });

  it("rejects javascript, data, file and custom schemes", () => {
    for (const href of [
      "javascript:alert(1)",
      "JaVaScRiPt:alert(1)",
      "  javascript:alert(1)",
      "java\tscript:alert(1)",
      "java\u0000script:alert(1)",
      "data:text/html,<script>alert(1)</script>",
      "file:///etc/passwd",
      "vscode://file/etc/passwd",
      "mailto:someone@example.com",
    ]) {
      expect(classifyPreviewLink(href), href).toEqual({
        kind: "rejected",
        reason: "unsafe-scheme",
        raw: href,
      });
    }
  });

  it("rejects protocol-relative and absolute filesystem targets", () => {
    expect(classifyPreviewLink("//evil.example/x")).toEqual({
      kind: "rejected",
      reason: "remote",
      raw: "//evil.example/x",
    });
    expect(classifyPreviewLink("/etc/passwd")).toEqual({
      kind: "rejected",
      reason: "absolute-path",
      raw: "/etc/passwd",
    });
    expect(classifyPreviewLink("C:\\Windows\\win.ini")).toEqual({
      kind: "rejected",
      reason: "absolute-path",
      raw: "C:\\Windows\\win.ini",
    });
  });

  it("rejects traversal even when percent-encoded", () => {
    expect(classifyPreviewLink("../secret.md")).toEqual({
      kind: "rejected",
      reason: "traversal",
      raw: "../secret.md",
    });
    expect(classifyPreviewLink("docs/%2e%2e/%2e%2e/secret.md")).toEqual({
      kind: "rejected",
      reason: "traversal",
      raw: "docs/%2e%2e/%2e%2e/secret.md",
    });
  });

  it("rejects empty targets", () => {
    expect(classifyPreviewLink("   ")).toEqual({ kind: "rejected", reason: "empty", raw: "   " });
  });

  it("routes fragments and relative documents", () => {
    expect(classifyPreviewLink("#Hello World")).toEqual({ kind: "fragment", id: "hello-world" });
    expect(classifyPreviewLink("./notes.md")).toEqual({ kind: "document", relativePath: "./notes.md" });
    expect(classifyPreviewLink("docs/notes.md")).toEqual({
      kind: "document",
      relativePath: "docs/notes.md",
    });
  });
});

describe("classifyPreviewImage", () => {
  it("accepts only relative local paths", () => {
    expect(classifyPreviewImage("./img/logo.png")).toEqual({
      kind: "local",
      relativePath: "./img/logo.png",
    });
    expect(classifyPreviewImage("img/logo.png")).toEqual({
      kind: "local",
      relativePath: "img/logo.png",
    });
  });

  it("rejects remote images, data URLs and escapes", () => {
    expect(classifyPreviewImage("https://tracker.example/p.png")).toEqual({
      kind: "rejected",
      reason: "remote",
      raw: "https://tracker.example/p.png",
    });
    expect(classifyPreviewImage("//tracker.example/p.png")).toEqual({
      kind: "rejected",
      reason: "remote",
      raw: "//tracker.example/p.png",
    });
    expect(classifyPreviewImage("data:image/png;base64,AAAA")).toEqual({
      kind: "rejected",
      reason: "unsafe-scheme",
      raw: "data:image/png;base64,AAAA",
    });
    expect(classifyPreviewImage("/etc/shadow.png")).toEqual({
      kind: "rejected",
      reason: "absolute-path",
      raw: "/etc/shadow.png",
    });
    expect(classifyPreviewImage("../../secret.png")).toEqual({
      kind: "rejected",
      reason: "traversal",
      raw: "../../secret.png",
    });
  });
});

describe("splitPreviewLines", () => {
  it("splits LF, CRLF and lone CR without dropping empty trailing lines", () => {
    expect(splitPreviewLines("a\r\nb\rc\nd")).toEqual(["a", "b", "c", "d"]);
    expect(splitPreviewLines("a\n")).toEqual(["a", ""]);
    expect(splitPreviewLines("")).toEqual([""]);
  });
});

describe("clampPreviewTarget", () => {
  const lines = ["안녕하세요 world", "👩x", "last"];

  it("counts columns in Unicode scalars, not UTF-16 code units", () => {
    expect(clampPreviewTarget({ line: 2, col: 2 }, lines)).toEqual({
      line: 2,
      column: 2,
      offset: 2,
      clamped: false,
    });
    expect(clampPreviewTarget({ line: 1, col: 7 }, lines)).toEqual({
      line: 1,
      column: 7,
      offset: 6,
      clamped: false,
    });
  });

  it("clamps out-of-range lines and columns and reports the clamp", () => {
    expect(clampPreviewTarget({ line: 999, col: 999 }, lines)).toEqual({
      line: 3,
      column: 5,
      offset: 4,
      clamped: true,
    });
    expect(clampPreviewTarget({ line: 0, col: null }, lines)).toEqual({
      line: 1,
      column: 1,
      offset: 0,
      clamped: true,
    });
    expect(clampPreviewTarget({ line: 2, col: 9 }, lines)).toEqual({
      line: 2,
      column: 3,
      offset: 3,
      clamped: true,
    });
  });
});

describe("findPreviewMatches", () => {
  const lines = ["Hello hello", "안녕 Hello", "a.b"];

  it("matches case-insensitively and reports UTF-16 ranges per line", () => {
    expect(findPreviewMatches(lines, "hello")).toEqual([
      { line: 0, start: 0, end: 5 },
      { line: 0, start: 6, end: 11 },
      { line: 1, start: 3, end: 8 },
    ]);
  });

  it("treats the query literally, never as a regular expression", () => {
    expect(findPreviewMatches(["axb"], "a.b")).toEqual([]);
    expect(findPreviewMatches(lines, "a.b")).toEqual([{ line: 2, start: 0, end: 3 }]);
  });

  it("returns nothing for an empty query", () => {
    expect(findPreviewMatches(lines, "")).toEqual([]);
    expect(findPreviewMatches(lines, "   \t")).toEqual([]);
  });
});

describe("literalizeHtmlNodes", () => {
  it("turns raw HTML nodes into literal text so nothing is ever mounted", () => {
    const tree = {
      type: "root",
      children: [
        { type: "html", value: "<script>window.__pwned = 1</script>" },
        {
          type: "paragraph",
          children: [
            { type: "text", value: "a" },
            { type: "html", value: "<b>" },
            { type: "text", value: "b" },
          ],
        },
      ],
    };

    literalizeHtmlNodes(tree);

    expect(tree.children[0]).toEqual({
      type: "paragraph",
      children: [{ type: "text", value: "<script>window.__pwned = 1</script>" }],
    });
    expect(tree.children[1]).toEqual({
      type: "paragraph",
      children: [
        { type: "text", value: "a" },
        { type: "text", value: "<b>" },
        { type: "text", value: "b" },
      ],
    });
    expect(JSON.stringify(tree)).not.toContain('"html"');
  });
});

describe("previewAnchorId", () => {
  it("slugifies heading text while preserving non-latin scripts", () => {
    expect(previewAnchorId("Hello World!")).toBe("hello-world");
    expect(previewAnchorId("  안녕 하세요  ")).toBe("안녕-하세요");
    expect(previewAnchorId("a/b c")).toBe("ab-c");
  });
});
