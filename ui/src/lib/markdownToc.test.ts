import { describe, expect, it } from "vitest";
import { extractMarkdownHeadingIndex, extractMarkdownHeadings } from "./markdownToc";

describe("extractMarkdownHeadings", () => {
  it("extracts levels 1 through 3 and ignores deeper headings", () => {
    const md = [
      "# Level 1",
      "Some text here",
      "## Level 2",
      "### Level 3",
      "#### Level 4 should be ignored",
      "##### Level 5 should be ignored",
      "###### Level 6 should be ignored",
    ].join("\n");

    const result = extractMarkdownHeadings(md);

    expect(result).toEqual([
      { level: 1, text: "Level 1", slug: "level-1" },
      { level: 2, text: "Level 2", slug: "level-2" },
      { level: 3, text: "Level 3", slug: "level-3" },
    ]);
  });

  it("ignores headings inside backtick and tilde fenced code blocks", () => {
    const md = [
      "# First Real Heading",
      "```markdown",
      "# Fake Heading Inside Backticks",
      "## Another Fake Inside Backticks",
      "```",
      "## Second Real Heading",
      "~~~",
      "### Fake Heading Inside Tildes",
      "~~~",
      "### Third Real Heading",
    ].join("\n");

    const result = extractMarkdownHeadings(md);

    expect(result).toEqual([
      { level: 1, text: "First Real Heading", slug: "first-real-heading" },
      { level: 2, text: "Second Real Heading", slug: "second-real-heading" },
      { level: 3, text: "Third Real Heading", slug: "third-real-heading" },
    ]);
  });

  it("de-duplicates slugs with -1 and -2 suffixes like GitHub", () => {
    const md = [
      "# Overview",
      "## Overview",
      "### Overview",
      "# Another Section",
      "## Overview",
    ].join("\n");

    const result = extractMarkdownHeadings(md);

    expect(result).toEqual([
      { level: 1, text: "Overview", slug: "overview" },
      { level: 2, text: "Overview", slug: "overview-1" },
      { level: 3, text: "Overview", slug: "overview-2" },
      { level: 1, text: "Another Section", slug: "another-section" },
      { level: 2, text: "Overview", slug: "overview-3" },
    ]);
  });

  it("preserves Korean letters in heading slugs", () => {
    const md = [
      "# 개요 및 시작하기",
      "## 설치 가이드",
      "### 환경 설정: 1단계",
    ].join("\n");

    const result = extractMarkdownHeadings(md);

    expect(result).toEqual([
      { level: 1, text: "개요 및 시작하기", slug: "개요-및-시작하기" },
      { level: 2, text: "설치 가이드", slug: "설치-가이드" },
      { level: 3, text: "환경 설정: 1단계", slug: "환경-설정-1단계" },
    ]);
  });

  it("handles heading closing hashes and surrounding whitespace", () => {
    const md = [
      "  #   Heading With Hashes ###  ",
      "## Multi   Space   Heading",
    ].join("\n");

    const result = extractMarkdownHeadings(md);

    expect(result).toEqual([
      { level: 1, text: "Heading With Hashes", slug: "heading-with-hashes" },
      { level: 2, text: "Multi   Space   Heading", slug: "multi-space-heading" },
    ]);
  });

  it("lists setext headings and indexes every slug by its source line", () => {
    const md = ["Title", "=====", "", "Two line", "sub title", "---", "", "---", "", "## Next"].join("\r\n");

    const index = extractMarkdownHeadingIndex(md);

    expect(index.headings).toEqual([
      { level: 1, text: "Title", slug: "title" },
      { level: 2, text: "Two line sub title", slug: "two-line-sub-title" },
      { level: 2, text: "Next", slug: "next" },
    ]);
    expect([...index.slugByLine]).toEqual([
      [1, "title"],
      [4, "two-line-sub-title"],
      [10, "next"],
    ]);
  });
});
