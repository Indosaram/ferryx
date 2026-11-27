/**
 * Markdown text helpers for the plain-text composer (t3code composer.md "Context in your
 * message" / "Quote an assistant response").
 *
 * The composer is a plain `<textarea>`, so there is no document round trip to pin here: the
 * draft is already the markdown the agent receives. What these tests pin is the escaping that
 * keeps a chip label or a citation quote a single link, which is what the chip parser and the
 * citation chip row depend on.
 */
import { describe, expect, it } from "vitest";
import { escapeLinkLabel, escapeMarkdownText, parseInline, unescapeMarkdownText } from "./markdown";

describe("composer markdown label escaping", () => {
  it("escapes and reads back a label holding brackets", () => {
    const label = "[WIP] fix";
    expect(escapeLinkLabel(label)).toBe("\\[WIP\\] fix");
    expect(unescapeMarkdownText(escapeLinkLabel(label))).toBe(label);
    const tokens = parseInline(`[${escapeLinkLabel(label)}](ferryx://chip/pr/42)`);
    expect(tokens.find((token) => token.type === "link")?.text).toBe(label);
  });

  it("encodes a newline in a label so a multi-line quote stays one link", () => {
    const label = "line one\nline two";
    expect(escapeLinkLabel(label)).toBe("line one\\nline two");
    expect(unescapeMarkdownText(escapeLinkLabel(label))).toBe(label);
    expect(parseInline(`[${escapeLinkLabel(label)}](ferryx://cite/m1)`).find((token) => token.type === "link")?.text).toBe(label);
  });

  it("leaves ordinary punctuation alone, so the words the user typed are the words sent", () => {
    // composer.md:25 "What you typed is what the agent receives, markers and numbering included".
    const label = "#12 - fix *this* `now`";
    expect(escapeMarkdownText(label)).toBe(label);
    expect(escapeLinkLabel(label)).toBe(label);
  });

  it("escapes a backslash so the escape itself survives the round trip", () => {
    const label = "C:\\tmp\\new";
    expect(unescapeMarkdownText(escapeLinkLabel(label))).toBe(label);
  });
});

describe("composer markdown inline parsing", () => {
  it("reports the source range of a bold token", () => {
    const tokens = parseInline("a **bold** b");
    const bold = tokens.find((token) => token.type === "bold");
    expect(bold).toBeDefined();
    expect(bold?.start).toBe(2);
    expect(bold?.end).toBe(10);
    expect("a **bold** b".slice(bold!.start, bold!.end)).toBe("**bold**");
  });

  it("reads a link's href and its title", () => {
    const link = parseInline('[quote](ferryx://cite/m1 "note")').find((token) => token.type === "link");
    expect(link?.text).toBe("quote");
    expect(link?.href).toBe('ferryx://cite/m1 "note"');
  });

  it("keeps plain text as one token, not one token per character", () => {
    const tokens = parseInline("plain words");
    expect(tokens).toHaveLength(1);
    expect(tokens[0]).toMatchObject({ type: "text", text: "plain words" });
  });
});
