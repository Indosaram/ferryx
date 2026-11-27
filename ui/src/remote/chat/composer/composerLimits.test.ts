import { afterEach, describe, expect, it } from "vitest";
import { cleanup } from "@testing-library/react";
import {
  LARGE_PASTE_BYTES,
  MAX_PROMPT_LENGTH,
  decideTextPaste,
  isImageFile,
  isPromptTooLong,
  pasteTextFile,
  promptLengthMessage,
} from "./composerLimits";

afterEach(cleanup);

describe("composer limits", () => {
  it("flags drafts over the 120,000 character limit and keeps them in the box", () => {
    expect(isPromptTooLong("a".repeat(MAX_PROMPT_LENGTH))).toBe(false);
    expect(isPromptTooLong("a".repeat(MAX_PROMPT_LENGTH + 1))).toBe(true);
  });

  it("names the count and the amount to cut in the visible message", () => {
    const message = promptLengthMessage("a".repeat(MAX_PROMPT_LENGTH + 12));
    expect(message).toContain("120,012");
    expect(message).toContain("12");
  });

  it("makes a 32 KiB or larger paste an attachment", () => {
    const large = "x".repeat(LARGE_PASTE_BYTES);
    expect(decideTextPaste(large, 0, 0, false).attach).toBe(true);
    expect(decideTextPaste(large, 0, 0, true).attach).toBe(false);
  });

  it("keeps a small paste inline unless it would exceed the message limit", () => {
    expect(decideTextPaste("hello", 0, 0, false).attach).toBe(false);
    const almostFull = "a".repeat(MAX_PROMPT_LENGTH - 2);
    expect(decideTextPaste("three", almostFull.length, almostFull.length, true).attach).toBe(true);
  });

  it("keeps a paste one byte under 32 KiB inline", () => {
    expect(decideTextPaste("x".repeat(LARGE_PASTE_BYTES - 1), 0, 0, false).attach).toBe(false);
  });

  it("keeps a paste that exactly reaches the message limit inline", () => {
    const draft = "a".repeat(MAX_PROMPT_LENGTH - 2);
    expect(decideTextPaste("ab", draft.length, draft.length, false).attach).toBe(false);
  });

  it("counts the paste in bytes, not characters, against the 32 KiB threshold", () => {
    // 16,400 two-byte characters: under 32 KiB as text length, over in bytes.
    const multibyte = "\u00e9".repeat(16400);
    expect(new TextEncoder().encode(multibyte).length).toBeGreaterThanOrEqual(LARGE_PASTE_BYTES);
    expect(decideTextPaste(multibyte, 0, 0, false).attach).toBe(true);
  });

  it("measures a paste that replaces a selection against what the draft becomes", () => {
    // composer.md:8-13: the attachment exists for a paste that would not fit inline. A paste over
    // a selection replaces that text, so a draft already near the limit can take a long paste
    // inline when the selection it overwrites is longer still.
    const nearFull = "a".repeat(MAX_PROMPT_LENGTH - 10);
    const pasted = "b".repeat(500);
    // Ignoring the selection, the draft would overflow and the paste would be forced out.
    expect(decideTextPaste(pasted, nearFull.length, 0, true).attach).toBe(true);
    // Replacing 600 characters leaves a shorter draft, so the paste stays editable.
    expect(decideTextPaste(pasted, nearFull.length, 0, true, 600).attach).toBe(false);
  });

  it("never lets a selection make a 32 KiB paste editable without the keep shortcut", () => {
    // The size rule is about the pasted fragment, not the draft it lands in.
    const large = "x".repeat(LARGE_PASTE_BYTES);
    expect(decideTextPaste(large, 0, 0, false, 40_000).attach).toBe(true);
  });

  it("builds a text file a large paste becomes", () => {
    const file = pasteTextFile("payload");
    expect(file.name).toContain("pasted");
    expect(file.type).toBe("text/plain");
  });

  it("treats images as image files and nothing else", () => {
    expect(isImageFile(new File([], "a.png", { type: "image/png" }))).toBe(true);
    expect(isImageFile(new File([], "a.txt", { type: "text/plain" }))).toBe(false);
  });
});
