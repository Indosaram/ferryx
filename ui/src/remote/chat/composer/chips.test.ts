import { afterEach, describe, expect, it } from "vitest";
import { cleanup } from "@testing-library/react";
import type { ChipItem } from "./chipTypes";
import {
  chipMarkdown,
  chipTriggerAt,
  citationMarkdown,
  isChipTrigger,
  parseCitation,
  staticChipCandidates,
} from "./chips";
import { parseInline } from "./markdown";

afterEach(cleanup);

const models: readonly ChipItem[] = [
  { id: "opus", kind: "model", label: "Opus" },
  { id: "sonnet", kind: "model", label: "Sonnet" },
];
const skills: readonly ChipItem[] = [{ id: "review", kind: "skill", label: "review" }];

describe("chip triggers", () => {
  it("opens on @ # $ / at the caret and carries the query", () => {
    expect(chipTriggerAt("look at @op", 11, "@")).toEqual({ start: 8, end: 11, query: "op", trigger: "@" });
    expect(chipTriggerAt("#12", 3, "#")).toEqual({ start: 0, end: 3, query: "12", trigger: "#" });
    expect(chipTriggerAt("use $rev", 8, "$")).toEqual({ start: 4, end: 8, query: "rev", trigger: "$" });
  });

  it("requires whitespace or line start before the trigger", () => {
    expect(chipTriggerAt("email a@b", 9, "@")).toBeNull();
    expect(chipTriggerAt("hey @", 5, "@")).toEqual({ start: 4, end: 5, query: "", trigger: "@" });
    expect(chipTriggerAt("hello #", 7, "#")).toEqual({ start: 6, end: 7, query: "", trigger: "#" });
  });

  it("a bare slash stays with the provider slash menu, not this menu", () => {
    expect(chipTriggerAt("/", 1, "/")).toBeNull();
    expect(chipTriggerAt("/comp", 5, "/")).toEqual({ start: 0, end: 5, query: "comp", trigger: "/" });
  });

  it("maps static sources to their trigger", () => {
    expect(staticChipCandidates("@", { models, skills })).toEqual(models);
    expect(staticChipCandidates("$", { models, skills })).toEqual(skills);
    expect(staticChipCandidates("#", { models, skills })).toEqual([]);
  });

  it("recognizes the chip triggers", () => {
    expect(isChipTrigger("@")).toBe(true);
    expect(isChipTrigger("#")).toBe(true);
    expect(isChipTrigger("$")).toBe(true);
    expect(isChipTrigger("x")).toBe(false);
  });
});

describe("chip serialization", () => {
  it("serializes a chip as a readable markdown link", () => {
    expect(chipMarkdown({ id: "PR 12", kind: "pr", label: "#12" })).toBe("[#12](ferryx://chip/pr/PR%2012)");
  });

  it("serializes a citation with its comment in the link title", () => {
    const md = citationMarkdown({ messageId: "m1", text: "the quoted words" }, "why it matters");
    expect(md).toBe('[the quoted words](ferryx://cite/m1 "why it matters")');
    expect(parseCitation("ferryx://cite/m1", "why it matters")).toEqual({ messageId: "m1", comment: "why it matters" });
  });

  it("reads a citation without a comment and ignores other links", () => {
    expect(parseCitation("ferryx://cite/m2", null)).toEqual({ messageId: "m2", comment: "" });
    expect(parseCitation("https://example.com", null)).toBeNull();
  });

  it("keeps the whole quote in the label so the agent receives the quoted words", () => {
    // composer.md:156-157 "sending tells the agent which words were quoted"; t3code carries the
    // full quote (assistantCitations.ts renderAssistantCitationsAsText) and counts it toward the
    // limit, so the label is the quote, not a preview.
    const quoted = "x".repeat(200);
    const md = citationMarkdown({ messageId: "m", text: quoted }, "");
    expect(md).toContain(quoted);
    expect(md).not.toContain("…");
  });

  it("keeps an 80-character quote whole", () => {
    const quoted = "z".repeat(80);
    expect(citationMarkdown({ messageId: "m1", text: quoted }, "")).toContain(quoted);
  });

  it("keeps a quote that spans lines readable and parseable", () => {
    const md = citationMarkdown({ messageId: "m1", text: "line one\nline two" }, "note");
    expect(md).toBe('[line one\\nline two](ferryx://cite/m1 "note")');
    const link = parseInline(md).find((token) => token.type === "link");
    expect(link?.text).toBe("line one\nline two");
  });

  it("escapes brackets in a chip label so the link survives", () => {
    const md = chipMarkdown({ kind: "pr", id: "42", label: "[WIP] fix" });
    expect(md).toBe("[\\[WIP\\] fix](ferryx://chip/pr/42)");
    const link = parseInline(md).find((token) => token.type === "link");
    expect(link?.text).toBe("[WIP] fix");
  });
});
