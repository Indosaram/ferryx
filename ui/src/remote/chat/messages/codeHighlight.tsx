import React, { useEffect, useState } from "react";

import { cn } from "../../../lib/cn";

/**
 * Syntax highlighting for chat code blocks (t3code MarkdownCodeBlock /
 * HighlightedCodeLines), on the repo's existing highlight.js setup
 * (`lib/previewHighlight.ts`). Loaded lazily; the module itself skips very
 * large blocks by returning null, which falls back to plain code.
 */

type HighlightModule = typeof import("../../../lib/previewHighlight");

let highlightModule: HighlightModule | undefined;
let highlightLoad: Promise<HighlightModule | undefined> | undefined;

export function loadHighlight(): Promise<HighlightModule | undefined> {
  highlightLoad ??= import("../../../lib/previewHighlight").then(
    (loaded) => {
      highlightModule = loaded;
      return loaded;
    },
    () => undefined,
  );
  return highlightLoad;
}

/** Fence language to a highlight.js grammar the preview setup registered. */
const LANGUAGE_ALIASES: ReadonlyMap<string, string> = new Map([
  ["ts", "typescript"],
  ["tsx", "typescript"],
  ["typescript", "typescript"],
  ["js", "javascript"],
  ["jsx", "javascript"],
  ["javascript", "javascript"],
  ["mjs", "javascript"],
  ["cjs", "javascript"],
  ["json", "json"],
  ["jsonc", "json"],
  ["rs", "rust"],
  ["rust", "rust"],
  ["py", "python"],
  ["python", "python"],
  ["go", "go"],
  ["sh", "bash"],
  ["bash", "bash"],
  ["shell", "bash"],
  ["zsh", "bash"],
  ["console", "bash"],
  ["yml", "yaml"],
  ["yaml", "yaml"],
  ["css", "css"],
  ["scss", "css"],
  ["html", "xml"],
  ["xml", "xml"],
  ["svg", "xml"],
  ["sql", "sql"],
  ["java", "java"],
  ["c", "c"],
  ["h", "c"],
  ["cpp", "cpp"],
  ["c++", "cpp"],
  ["cc", "cpp"],
  ["cxx", "cpp"],
  ["hpp", "cpp"],
  ["csharp", "csharp"],
  ["c#", "csharp"],
  ["cs", "csharp"],
  ["ruby", "ruby"],
  ["rb", "ruby"],
  ["php", "php"],
  ["swift", "swift"],
  ["kotlin", "kotlin"],
  ["kt", "kotlin"],
  ["ini", "ini"],
  ["toml", "ini"],
  ["diff", "diff"],
  ["patch", "diff"],
  ["dockerfile", "dockerfile"],
]);

export function highlighterLanguageOf(fenceLanguage: string): string | null {
  const lower = fenceLanguage.toLowerCase();
  return LANGUAGE_ALIASES.get(lower) ?? null;
}

export interface HighlightedCodeResult {
  /** Highlighted HTML per line; null falls back to plain code. */
  readonly lines: readonly string[] | null;
  readonly language: string | null;
}

/** Pure lookup against the already-loaded module; null when it is not loaded yet. */
export function highlightLinesSync(code: string, fenceLanguage: string): HighlightedCodeResult {
  const module = highlightModule;
  const language = highlighterLanguageOf(fenceLanguage);
  if (!module || language === null) return { lines: null, language: null };
  return { lines: module.highlightLines(code, language), language };
}

/**
 * One highlighted code line as HTML. The markup comes from highlight.js over
 * agent prose, with spans rebalanced per line by `highlightLines`; a standalone
 * image, script, or style tag is impossible in its output.
 */
function HighlightedLine({ html }: { html: string }) {
  return <span className="block" dangerouslySetInnerHTML={{ __html: html }} />;
}

/** Plain lines, the fallback whenever highlighting is unavailable or refused. */
function plainLines(code: string): readonly string[] {
  return code.split("\n");
}

function PlainLine({ line }: { line: string }) {
  return <span className="block">{line.length > 0 ? line : " "}</span>;
}

export const HighlightedCodeLines: React.FC<{
  code: string;
  fenceLanguage: string;
  className?: string;
  testId?: string;
}> = ({ code, fenceLanguage, className, testId }) => {
  const [loaded, setLoaded] = useState(() => highlightModule !== undefined);
  // A block too large to highlight never pays for the lazy load.
  const tooLarge = isCodeBlockTooLarge(code);
  useEffect(() => {
    if (loaded || tooLarge) return;
    let active = true;
    void loadHighlight().then((module) => {
      if (active && module) setLoaded(true);
    });
    return () => {
      active = false;
    };
  }, [loaded, tooLarge]);

  const result =
    loaded && !tooLarge
      ? highlightLinesSync(code, fenceLanguage)
      : { lines: null, language: null };
  // Plain code until the grammar arrives: the block never renders empty.
  if (result.lines === null) {
    return (
      <span data-testid={testId ?? "highlighted-code-lines"} data-highlighted="false">
        {plainLines(code).map((line, index) => (
          <PlainLine key={index} line={line} />
        ))}
      </span>
    );
  }
  return (
    <span
      data-testid={testId ?? "highlighted-code-lines"}
      data-highlighted="true"
      role="presentation"
      className={cn("hljs", className)}
    >
      {result.lines.map((html, index) => (
        <HighlightedLine key={index} html={html} />
      ))}
    </span>
  );
};

/** True when a code block is too large to highlight; the plain code renders instead. */
export function isCodeBlockTooLarge(code: string): boolean {
  // Matches previewHighlight's own caps, used to skip the lazy load entirely.
  if (code.length > 512 * 1024) return true;
  let newlines = 0;
  for (let i = 0; i < code.length; i++) {
    if (code.charCodeAt(i) === 10 && ++newlines >= 20000) return true;
  }
  return false;
}
