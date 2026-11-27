/**
 * Markdown text helpers for the plain-text composer (t3code composer.md "Formatting").
 *
 * The composer is a plain `<textarea>`: the draft IS the markdown the agent receives, so there
 * is no document to parse it into and no document to serialize back. What survives here is the
 * part the chips and citations need: escaping a label so a chip or a quote stays one link, and
 * reading that escape back. A label holding `[` or `]`, or a quote spanning lines, still parses
 * as one link because of it.
 */

export type InlineTokenType = "text" | "bold" | "italic" | "strike" | "code" | "link" | "hardBreak";

export interface InlineToken {
  readonly type: InlineTokenType;
  /** Plain text for text/code/link-label tokens. */
  readonly text: string;
  /** Link URL for link tokens. */
  readonly href?: string;
  readonly children?: readonly InlineToken[];
  /** Source range in the original markdown text. */
  readonly start: number;
  readonly end: number;
}

// ---------------------------------------------------------------------------
// Markdown text escaping (t3code escapeMarkdownText, assistantCitations.ts:176-182)
// ---------------------------------------------------------------------------

/**
 * The characters that are structural inside a markdown link label: a closing bracket ends the
 * label, an opening one starts a nested one, and a backslash starts an escape. Everything else
 * (emphasis, `#`, `-`, punctuation) stays literal in a label, so escaping it would change the
 * words the user typed. t3code escapes chip text the same way before writing link syntax
 * (`escapeMarkdownText`, assistantCitations.ts:176-182).
 */
const LINK_LABEL_ESCAPES = /[\\[\]]/g;

/** Escape text so it survives as one link label. */
export function escapeMarkdownText(text: string): string {
  return text.replace(LINK_LABEL_ESCAPES, "\\$&");
}

/**
 * Escape a whole link label: the characters above, and a real newline as the two characters
 * `\n`, because a label is one line while a quote can span several.
 */
export function escapeLinkLabel(text: string): string {
  return escapeMarkdownText(text).replace(/\n/g, "\\n");
}

/**
 * Read escaped text back, the inverse of `escapeLinkLabel`: `\n` is the newline a citation
 * label carries, and a backslash before any other character is that character.
 */
export function unescapeMarkdownText(text: string): string {
  let out = "";
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i];
    if (char !== "\\" || i + 1 >= text.length) {
      out += char;
      continue;
    }
    const next = text[i + 1];
    out += next === "n" ? "\n" : next;
    i += 1;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Inline parsing
// ---------------------------------------------------------------------------

const LINK_PATTERN = /\[((?:\\.|[^\\\]])*)\]\(([^)\s]*)(?:\s+"((?:[^"\\]|\\.)*)")?\)/y;

/** Parse one line of inline markdown; every token carries its source range. */
export function parseInline(text: string, base = 0): readonly InlineToken[] {
  const tokens: InlineToken[] = [];
  let i = 0;
  const push = (token: InlineToken) => {
    if (token.type === "text" && token.text.length === 0) return;
    tokens.push(token);
  };
  while (i < text.length) {
    const char = text[i];
    if (char === "`") {
      const close = text.indexOf("`", i + 1);
      if (close > i) {
        push({ type: "code", text: text.slice(i + 1, close), start: base + i, end: base + close + 1 });
        i = close + 1;
        continue;
      }
    }
    if (char === "*" && text[i + 1] === "*") {
      const close = text.indexOf("**", i + 2);
      if (close > i) {
        push({
          type: "bold",
          text: text.slice(i + 2, close),
          children: parseInline(text.slice(i + 2, close), base + i + 2),
          start: base + i,
          end: base + close + 2,
        });
        i = close + 2;
        continue;
      }
    }
    if (char === "~" && text[i + 1] === "~") {
      const close = text.indexOf("~~", i + 2);
      if (close > i) {
        push({ type: "strike", text: text.slice(i + 2, close), start: base + i, end: base + close + 2 });
        i = close + 2;
        continue;
      }
    }
    if (char === "*" || char === "_") {
      const close = findEmphasisClose(text, i + 1, char);
      if (close > i) {
        push({
          type: "italic",
          text: text.slice(i + 1, close),
          children: parseInline(text.slice(i + 1, close), base + i + 1),
          start: base + i,
          end: base + close + 1,
        });
        i = close + 1;
        continue;
      }
    }
    if (char === "[") {
      LINK_PATTERN.lastIndex = i;
      const match = LINK_PATTERN.exec(text);
      if (match) {
        push({
          type: "link",
          // The label is stored readable; serialization escapes it again.
          text: unescapeMarkdownText(match[1]),
          href: match[2] + (match[3] ? ` "${match[3]}"` : ""),
          start: base + i,
          end: base + i + match[0].length,
        });
        i += match[0].length;
        continue;
      }
    }
    // Consecutive plain characters join into one text token so a styled run stays a single
    // token instead of one token per character.
    const last = tokens.at(-1);
    if (last !== undefined && last.type === "text" && last.end === base + i) {
      tokens[tokens.length - 1] = { type: "text", text: last.text + char, start: last.start, end: base + i + 1 };
    } else {
      push({ type: "text", text: char, start: base + i, end: base + i + 1 });
    }
    i += 1;
  }
  return tokens;
}

function findEmphasisClose(text: string, from: number, marker: string): number {
  let index = from;
  while (index < text.length) {
    const found = text.indexOf(marker, index);
    if (found < 0) return -1;
    const prev = text[found - 1];
    const next = text[found + 1];
    // closing emphasis: not preceded by whitespace, and not doubled (** would be bold)
    if (prev !== undefined && prev !== " " && prev !== marker) return found;
    if (prev === " " && next !== undefined && next !== " ") {
      index = found + 1;
      continue;
    }
    index = found + 1;
  }
  return -1;
}
