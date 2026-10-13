export type MarkdownHeading = {
  level: 1 | 2 | 3;
  text: string;
  slug: string;
};

export type MarkdownHeadingIndex = {
  readonly headings: MarkdownHeading[];
  /** Slug of each listed heading keyed by the 1-based source line where the heading starts. */
  readonly slugByLine: ReadonlyMap<number, string>;
};

export function extractMarkdownHeadings(markdown: string): MarkdownHeading[] {
  return extractMarkdownHeadingIndex(markdown).headings;
}

/**
 * Top-level ATX and setext headings (levels 1-3) with GitHub-style deduped slugs.
 * The renderer looks slugs up by source line, so a heading's id never depends on
 * render order and always matches its TOC entry.
 */
export function extractMarkdownHeadingIndex(markdown: string): MarkdownHeadingIndex {
  const lines = markdown.split(/\r\n|\r|\n/);
  const headings: MarkdownHeading[] = [];
  const slugByLine = new Map<number, string>();
  const occurrences = new Map<string, number>();
  const assigned = new Set<string>();

  let inCodeBlock = false;
  let fenceChar = "";
  let fenceLength = 0;
  // Open paragraph that a setext underline would turn into a heading.
  let paragraph: { startLine: number; text: string[] } | null = null;

  const push = (level: 1 | 2 | 3, rawText: string, line: number) => {
    const headingText = rawText.trim();
    if (!headingText) return;
    const slug = assignSlug(headingText, occurrences, assigned);
    headings.push({ level, text: headingText, slug });
    slugByLine.set(line, slug);
  };

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
    const line = lines[lineIndex]!;
    const trimmedLeft = line.trimStart();
    const indent = line.length - trimmedLeft.length;

    if (indent <= 3) {
      const fenceMatch = trimmedLeft.match(/^(`{3,}|~{3,})/);
      if (fenceMatch) {
        const matchFence = fenceMatch[1];
        const char = matchFence[0];
        const len = matchFence.length;

        if (!inCodeBlock) {
          inCodeBlock = true;
          fenceChar = char;
          fenceLength = len;
          paragraph = null;
          continue;
        } else if (char === fenceChar && len >= fenceLength && trimmedLeft.slice(len).trim() === "") {
          inCodeBlock = false;
          fenceChar = "";
          fenceLength = 0;
          continue;
        }
      }
    }

    if (inCodeBlock) {
      continue;
    }

    if (trimmedLeft.length === 0) {
      paragraph = null;
      continue;
    }

    const setextMatch = paragraph && indent <= 3 ? trimmedLeft.match(/^(=+|-+)\s*$/) : null;
    if (paragraph && setextMatch) {
      push(setextMatch[1]![0] === "=" ? 1 : 2, paragraph.text.join(" "), paragraph.startLine);
      paragraph = null;
      continue;
    }

    const headingMatch = line.match(/^ {0,3}(#{1,3})\s+(.*)$/);
    if (!headingMatch) {
      const startsOtherBlock =
        /^ {0,3}(#{4,6}\s|>|[-*+]\s|\d+[.)]\s|(\*\s*){3,}$|(-\s*){3,}$|(_\s*){3,}$|\|)/.test(line) ||
        (!paragraph && indent >= 4);
      if (startsOtherBlock) {
        paragraph = null;
      } else if (paragraph) {
        paragraph.text.push(trimmedLeft.trim());
      } else {
        paragraph = { startLine: lineIndex + 1, text: [trimmedLeft.trim()] };
      }
      continue;
    }

    paragraph = null;
    const level = headingMatch[1].length as 1 | 2 | 3;
    let headingText = headingMatch[2].trim();
    headingText = headingText.replace(/\s+#+\s*$/, "").trim();
    push(level, headingText, lineIndex + 1);
  }

  return { headings, slugByLine };
}

function assignSlug(headingText: string, occurrences: Map<string, number>, assigned: Set<string>): string {
  let baseSlug = headingText
    .toLowerCase()
    .replace(/\s+/g, "-")
    .replace(/[^\p{L}0-9\-_]/gu, "");

  if (!baseSlug) {
    baseSlug = "heading";
  }

  if (!assigned.has(baseSlug)) {
    assigned.add(baseSlug);
    occurrences.set(baseSlug, 0);
    return baseSlug;
  }
  let count = (occurrences.get(baseSlug) ?? 0) + 1;
  let candidate = `${baseSlug}-${count}`;
  while (assigned.has(candidate)) {
    count++;
    candidate = `${baseSlug}-${count}`;
  }
  occurrences.set(baseSlug, count);
  assigned.add(candidate);
  return candidate;
}
