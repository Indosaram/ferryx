import { Copy, Search, ChevronDown, ChevronUp, Columns, FileText, Image as ImageIcon, Link2, List, Minus, Plus, WrapText } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import Markdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { cn } from "../lib/cn";
import { copyTextToClipboard } from "../lib/clipboard";
import {
  filePreviewErrorReason,
  type FilePreviewChildAsset,
  type FilePreviewMarkdownCapability,
  type FilePreviewTextProps,
} from "../lib/filePreviewTypes";
import {
  classifyPreviewImage,
  classifyPreviewLink,
  clampPreviewTarget,
  findPreviewMatches,
  previewAnchorId,
  remarkLiteralHtml,
  splitPreviewLines,
  type PreviewCaret,
  type PreviewMatch,
  type PreviewRejectionReason,
} from "../lib/previewMarkdown";
import { highlightLines, languageForPath } from "../lib/previewHighlight";
import { extractMarkdownHeadingIndex, type MarkdownHeadingIndex } from "../lib/markdownToc";
import { useLineWindow } from "../lib/useLineWindow";
import {
  loadFilePreviewWordWrap,
  loadMarkdownFontSize,
  loadMarkdownWide,
  saveFilePreviewWordWrap,
  saveMarkdownFontSize,
  saveMarkdownWide,
} from "../lib/filePreviewReadingSettings";

const NO_MATCHES: readonly PreviewMatch[] = [];
const EMPTY_HEADING_INDEX: MarkdownHeadingIndex = { headings: [], slugByLine: new Map() };

/**
 * Source views with more lines than this are windowed: only the visible lines plus
 * overscan are mounted. Smaller documents keep every line mounted (no behaviour change).
 */
export const SOURCE_WINDOW_THRESHOLD = 2000;
/** Pitch of one unwrapped source line: `text-[12px] leading-5` is a 20px line box. */
const SOURCE_LINE_HEIGHT = 20;

/** A scroll/selection target that waits until its line is mounted by the window. */
type PendingReveal = { index: number; attempted: boolean };

/**
 * Scrolls to a heading id inside one rendered document only; a global
 * `document.getElementById` could land on another preview's heading.
 */
function scrollToFragmentIn(host: HTMLElement | null, id: string): void {
  if (!host || id.length === 0) return;
  const escaped = typeof CSS !== "undefined" && CSS.escape ? CSS.escape(id) : id;
  const anchor = host.querySelector(`#${escaped}`);
  if (anchor instanceof HTMLElement) anchor.scrollIntoView({ block: "start" });
}

/**
 * Read-only text and Markdown renderer for the file-preview modal (plan task 3).
 *
 * Everything here is display-only: no textarea, no `contentEditable`, no editor
 * package. Markdown goes through react-markdown + remark-gfm with raw HTML
 * literalized (see `remarkLiteralHtml`), so author markup is visible but inert.
 * Relative images and documents are resolved exclusively through the capability
 * the modal passes in; remote images and non-http(s) schemes never reach the DOM.
 */
export function FilePreviewText({
  payload,
  generation,
  onFailure,
  target,
  sourceMode,
  onSourceModeChange,
  markdown,
}: FilePreviewTextProps) {
  const text = payload.text ?? "";
  const isMarkdown = payload.kind === "markdown";
  const showSource = !isMarkdown || sourceMode;

  const lines = useMemo(() => splitPreviewLines(text), [text]);
  const caret = useMemo(() => (target ? clampPreviewTarget(target, lines) : null), [target, lines]);

  const highlightLanguage = useMemo(() => {
    if (payload.kind !== "text") return null;
    return languageForPath(payload.displayName);
  }, [payload.kind, payload.displayName]);

  const highlightedLines = useMemo(() => {
    if (!highlightLanguage) return null;
    return highlightLines(text, highlightLanguage);
  }, [text, highlightLanguage]);

  const [query, setQuery] = useState("");
  const [requestedMatch, setRequestedMatch] = useState(0);
  const matches = useMemo(() => findPreviewMatches(lines, query), [lines, query]);
  const activeMatch = matches.length === 0 ? -1 : Math.min(requestedMatch, matches.length - 1);

  const sourceRef = useRef<HTMLDivElement | null>(null);
  const documentRef = useRef<HTMLDivElement | null>(null);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [wordWrap, setWordWrap] = useState<boolean>(() => loadFilePreviewWordWrap());

  const handleWordWrapToggle = useCallback(() => {
    setWordWrap((prev) => {
      const next = !prev;
      saveFilePreviewWordWrap(next);
      return next;
    });
  }, []);

  const [markdownFontSize, setMarkdownFontSize] = useState<number>(() => loadMarkdownFontSize());

  const handleFontSizeChange = useCallback((next: number) => {
    const clamped = Math.max(11, Math.min(22, next));
    setMarkdownFontSize(clamped);
    saveMarkdownFontSize(clamped);
  }, []);

  const [markdownWide, setMarkdownWide] = useState<boolean>(() => loadMarkdownWide());

  const handleWideToggle = useCallback(() => {
    setMarkdownWide((prev) => {
      const next = !prev;
      saveMarkdownWide(next);
      return next;
    });
  }, []);

  const [showToc, setShowToc] = useState<boolean>(false);
  const headingIndex = useMemo(
    () => (isMarkdown ? extractMarkdownHeadingIndex(text) : EMPTY_HEADING_INDEX),
    [isMarkdown, text],
  );
  const headings = headingIndex.headings;

  useEffect(() => {
    setRequestedMatch(0);
  }, [query, text]);

  // Large documents mount only the visible lines. Windowing assumes one visual row per
  // line, so soft wrap is unavailable while it is active (it comes back when printing,
  // because printing mounts every line).
  const lineWindow = useLineWindow({
    count: lines.length,
    enabled: showSource && lines.length > SOURCE_WINDOW_THRESHOLD,
    estimatedRowHeight: SOURCE_LINE_HEIGHT,
  });
  const windowed = lineWindow.active;
  const effectiveWordWrap = wordWrap && !windowed;
  // Only the mounted lines size the content, so give the windowed list the width of the
  // longest line (gutter w-10 + gap-3 = 3.25rem) to keep horizontal scrolling stable.
  const windowedMinWidth = useMemo(() => {
    if (!windowed) return undefined;
    let longest = 0;
    for (const line of lines) if (line.length > longest) longest = line.length;
    return `calc(${longest}ch + 3.25rem)`;
  }, [lines, windowed]);
  const { scrollRef: setWindowScroller, scrollToIndex, start: windowStart, end: windowEnd } = lineWindow;
  const setSourceElement = useCallback(
    (element: HTMLDivElement | null) => {
      sourceRef.current = element;
      setWindowScroller(element);
    },
    [setWindowScroller],
  );

  // Caret and active-match reveals are queued and retried once the window has mounted
  // their line: an unmounted line is first scrolled to by arithmetic (index * line height).
  const pendingCaret = useRef<PendingReveal | null>(null);
  const pendingMatch = useRef<PendingReveal | null>(null);

  useEffect(() => {
    pendingCaret.current = showSource && caret ? { index: caret.line - 1, attempted: false } : null;
  }, [caret, showSource]);

  useEffect(() => {
    pendingMatch.current = showSource && activeMatch >= 0 ? { index: activeMatch, attempted: false } : null;
  }, [activeMatch, matches, showSource]);

  useEffect(() => {
    const host = sourceRef.current;
    if (!host) return;

    const caretReveal = pendingCaret.current;
    if (caretReveal) {
      const line = host.querySelector(`[data-testid="file-preview-line-${caretReveal.index + 1}"]`);
      if (line instanceof HTMLElement) {
        line.scrollIntoView({ block: "center" });
        pendingCaret.current = null;
      } else if (windowed && !caretReveal.attempted) {
        // Retried when the window moves (or the scroller attaches, which renews scrollToIndex).
        caretReveal.attempted = scrollToIndex(caretReveal.index);
      } else {
        pendingCaret.current = null;
      }
    }

    // Selecting the active match is how "search" behaves here: it moves the
    // browser selection over existing text and never rewrites the document.
    const matchReveal = pendingMatch.current;
    if (matchReveal) {
      const mark = host.querySelector(`[data-testid="file-preview-match-${matchReveal.index}"]`);
      const match = matches[matchReveal.index];
      if (mark instanceof HTMLElement) {
        pendingMatch.current = null;
        mark.scrollIntoView({ block: "center" });
        const selection = window.getSelection?.();
        if (selection && mark.firstChild) {
          const range = document.createRange();
          range.selectNodeContents(mark);
          selection.removeAllRanges();
          selection.addRange(range);
        }
      } else if (windowed && match && !matchReveal.attempted) {
        matchReveal.attempted = scrollToIndex(match.line);
      } else {
        pendingMatch.current = null;
      }
    }
  }, [activeMatch, caret, matches, showSource, windowed, windowStart, windowEnd, scrollToIndex]);

  useEffect(
    () => () => {
      if (copyTimer.current) clearTimeout(copyTimer.current);
    },
    [],
  );

  const handleCopy = useCallback(async () => {
    try {
      if (!(await copyTextToClipboard(text))) throw new Error("clipboard unavailable");
      setCopied(true);
      if (copyTimer.current) clearTimeout(copyTimer.current);
      copyTimer.current = setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      onFailure({
        reason: null,
        message: `Could not copy ${payload.displayName}: ${errorMessage(error)}`,
        details: null,
      });
    }
  }, [onFailure, payload.displayName, text]);

  const { byLine: matchesByLine, firstIndexByLine } = useMemo(() => indexMatches(matches), [matches]);

  const stepMatch = useCallback(
    (direction: 1 | -1) =>
      setRequestedMatch((current) => {
        if (matches.length === 0) return current;
        const base = Math.min(current, matches.length - 1);
        return (base + direction + matches.length) % matches.length;
      }),
    [matches.length],
  );

  return (
    <div className="flex h-full min-h-0 flex-col bg-card text-foreground" data-testid="file-preview-text">
      <div
        data-testid="file-preview-text-toolbar"
        data-print-hide=""
        className="flex h-9 shrink-0 items-center gap-2 border-b border-worktree-sidebar-border px-2"
      >
        {showSource ? (
          <div className="flex min-w-0 items-center gap-1">
            <label className="flex h-7 min-w-0 items-center gap-1.5 rounded-md bg-background px-2 focus-within:ring-1 focus-within:ring-ring">
              <Search aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
              <span className="sr-only">Search document</span>
              <input
                type="search"
                data-testid="file-preview-search-input"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
                  event.preventDefault();
                  stepMatch(event.shiftKey ? -1 : 1);
                }}
                placeholder="Search"
                className="h-6 w-32 min-w-0 bg-transparent text-[12px] text-foreground outline-none placeholder:text-muted-foreground"
              />
            </label>
            {query.trim().length > 0 ? (
              <>
                <span
                  data-testid="file-preview-search-status"
                  aria-live="polite"
                  className="whitespace-nowrap text-[11px] tabular-nums text-muted-foreground"
                >
                  {matches.length === 0 ? "No matches" : `${activeMatch + 1} of ${matches.length}`}
                </span>
                <ToolbarButton
                  label="Previous match"
                  testId="file-preview-search-previous"
                  disabled={matches.length === 0}
                  onClick={() => stepMatch(-1)}
                >
                  <ChevronUp aria-hidden className="size-3.5" />
                </ToolbarButton>
                <ToolbarButton
                  label="Next match"
                  testId="file-preview-search-next"
                  disabled={matches.length === 0}
                  onClick={() => stepMatch(1)}
                >
                  <ChevronDown aria-hidden className="size-3.5" />
                </ToolbarButton>
              </>
            ) : null}
          </div>
        ) : (
          <div className="flex min-w-0 items-center gap-1">
            <span className="truncate text-[11px] text-muted-foreground mr-1">Rendered Markdown</span>
            <ToolbarButton
              label="Table of contents"
              testId="file-preview-md-toc-toggle"
              pressed={showToc}
              onClick={() => setShowToc((v) => !v)}
            >
              <List aria-hidden className="size-3.5" />
            </ToolbarButton>
            <ToolbarButton
              label="Decrease font size"
              testId="file-preview-md-font-dec"
              disabled={markdownFontSize <= 11}
              onClick={() => handleFontSizeChange(markdownFontSize - 1)}
            >
              <Minus aria-hidden className="size-3.5" />
            </ToolbarButton>
            <span className="text-[11px] tabular-nums text-muted-foreground">{markdownFontSize}px</span>
            <ToolbarButton
              label="Increase font size"
              testId="file-preview-md-font-inc"
              disabled={markdownFontSize >= 22}
              onClick={() => handleFontSizeChange(markdownFontSize + 1)}
            >
              <Plus aria-hidden className="size-3.5" />
            </ToolbarButton>
            <ToolbarButton
              label={markdownWide ? "Reading width" : "Full width"}
              testId="file-preview-md-width-toggle"
              pressed={markdownWide}
              onClick={handleWideToggle}
            >
              <Columns aria-hidden className="size-3.5" />
            </ToolbarButton>
          </div>
        )}

        <div className="ml-auto flex items-center gap-2">
          {showSource ? (
            <ToolbarButton
              label={windowed ? "Word wrap is unavailable for very long files" : "Toggle word wrap"}
              testId="file-preview-wrap-toggle"
              pressed={effectiveWordWrap}
              disabled={windowed}
              onClick={handleWordWrapToggle}
            >
              <WrapText aria-hidden className="size-3.5" />
            </ToolbarButton>
          ) : null}
          {showSource && caret ? (
            <span
              data-testid="file-preview-location"
              className="whitespace-nowrap text-[11px] tabular-nums text-muted-foreground"
            >
              {`Ln ${caret.line}, Col ${caret.column}`}
            </span>
          ) : null}
          {showSource && caret?.clamped ? (
            <span
              data-testid="file-preview-target-clamped"
              title="The requested position is outside this file; the caret was clamped."
              className="whitespace-nowrap rounded-sm bg-accent px-1.5 py-0.5 text-[10px] text-status-warning"
            >
              clamped
            </span>
          ) : null}
          {isMarkdown && !sourceMode && target ? (
            <ToolbarButton
              label={`Show line ${target.line} in source`}
              testId="file-preview-target-source"
              onClick={() => onSourceModeChange(true)}
              wide
            >
              {`Line ${target.line}`}
            </ToolbarButton>
          ) : null}
          {isMarkdown ? (
            <ToolbarButton
              label={sourceMode ? "Show rendered Markdown" : "Show Markdown source"}
              testId="file-preview-source-toggle"
              onClick={() => onSourceModeChange(!sourceMode)}
              wide
            >
              <FileText aria-hidden className="size-3.5" />
              {sourceMode ? "Rendered" : "Source"}
            </ToolbarButton>
          ) : null}
          <ToolbarButton label="Copy document text" testId="file-preview-copy" onClick={handleCopy} wide>
            <Copy aria-hidden className="size-3.5" />
            {copied ? "Copied" : "Copy"}
          </ToolbarButton>
        </div>
      </div>

      {showSource ? (
        <div
          ref={setSourceElement}
          data-testid="file-preview-source"
          data-windowed={windowed ? "true" : undefined}
          role="document"
          aria-readonly="true"
          aria-label={`${payload.displayName} source`}
          tabIndex={0}
          className="selectable min-h-0 flex-1 overflow-auto px-2 py-2 font-mono text-[12px] leading-5 focus-visible:outline-none"
        >
          {/* Small documents keep every line mounted; large ones mount only the window, with
              spacers keeping the full scroll height. Memoized lines with stable props keep a
              search keystroke from re-rendering lines whose matches did not change. */}
          <div ref={lineWindow.listRef} style={windowedMinWidth ? { minWidth: windowedMinWidth } : undefined}>
            {lineWindow.padTop > 0 ? (
              <div aria-hidden data-testid="file-preview-window-spacer-top" style={{ height: lineWindow.padTop }} />
            ) : null}
            {lines.slice(windowStart, windowEnd).map((line, offset) => {
              const index = windowStart + offset;
              const lineMatches = matchesByLine.get(index) ?? NO_MATCHES;
              const matchOffset = firstIndexByLine.get(index) ?? -1;
              const ownsActiveMatch =
                matchOffset >= 0 && activeMatch >= matchOffset && activeMatch < matchOffset + lineMatches.length;
              return (
                <SourceLine
                  key={index}
                  line={line}
                  highlightedHtml={highlightedLines?.[index]}
                  number={index + 1}
                  caret={caret && caret.line === index + 1 ? caret : null}
                  matches={lineMatches}
                  activeMatch={ownsActiveMatch ? activeMatch : -1}
                  matchOffset={matchOffset}
                  wordWrap={effectiveWordWrap}
                />
              );
            })}
            {lineWindow.padBottom > 0 ? (
              <div
                aria-hidden
                data-testid="file-preview-window-spacer-bottom"
                style={{ height: lineWindow.padBottom }}
              />
            ) : null}
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 overflow-hidden">
          {showToc ? (
            <aside
              data-testid="file-preview-md-toc"
              data-print-hide=""
              className="w-56 shrink-0 overflow-y-auto border-r border-worktree-sidebar-border p-3 text-[12px]"
            >
              <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                Table of Contents
              </div>
              {headings.length === 0 ? (
                <div className="text-[11px] text-muted-foreground">No headings</div>
              ) : (
                <nav className="flex flex-col gap-1">
                  {headings.map((heading, index) => (
                    <button
                      key={`${heading.slug}-${index}`}
                      type="button"
                      data-testid={`file-preview-md-toc-item-${heading.slug}`}
                      onClick={() => {
                        scrollToFragmentIn(documentRef.current, heading.slug);
                      }}
                      className={cn(
                        "text-left text-muted-foreground hover:text-foreground hover:underline transition-colors truncate",
                        heading.level === 1 && "font-medium text-foreground",
                        heading.level === 2 && "pl-3 text-[12px]",
                        heading.level === 3 && "pl-6 text-[11px]",
                      )}
                      title={heading.text}
                    >
                      {heading.text}
                    </button>
                  ))}
                </nav>
              )}
            </aside>
          ) : null}
          <div
            ref={documentRef}
            data-testid="file-preview-markdown"
            style={{ fontSize: `${markdownFontSize}px` }}
            className={cn(
              "selectable min-h-0 flex-1 overflow-auto px-4 py-3 leading-6 [overflow-wrap:anywhere]",
              markdownWide ? "w-full" : "max-w-[72ch] mx-auto",
            )}
          >
            <PreviewMarkdownDocument
              source={text}
              capability={markdown}
              generation={generation}
              containerRef={documentRef}
              slugByLine={headingIndex.slugByLine}
            />
          </div>
        </div>
      )}
    </div>
  );
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return "unknown error";
}

/**
 * R8: Linear document match indexing. Precomputes line buckets and the flat
 * index of the first match per line in a single O(matches) pass, eliminating
 * quadratic findIndex calls during line rendering.
 */
function indexMatches(matches: readonly PreviewMatch[]): {
  byLine: Map<number, PreviewMatch[]>;
  firstIndexByLine: Map<number, number>;
} {
  const byLine = new Map<number, PreviewMatch[]>();
  const firstIndexByLine = new Map<number, number>();
  for (let i = 0; i < matches.length; i++) {
    const match = matches[i]!;
    const bucket = byLine.get(match.line);
    if (bucket) {
      bucket.push(match);
    } else {
      byLine.set(match.line, [match]);
      firstIndexByLine.set(match.line, i);
    }
  }
  return { byLine, firstIndexByLine };
}

type ToolbarButtonProps = {
  label: string;
  testId: string;
  onClick: () => void;
  children: ReactNode;
  disabled?: boolean;
  wide?: boolean;
  /** Set for toggle buttons so assistive tech can read their on/off state. */
  pressed?: boolean;
};

function ToolbarButton({ label, testId, onClick, children, disabled, wide, pressed }: ToolbarButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      data-testid={testId}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "inline-flex h-7 shrink-0 items-center justify-center gap-1 rounded-md text-[11px] text-muted-foreground transition-colors",
        "hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        "disabled:pointer-events-none disabled:opacity-40",
        wide ? "px-2" : "w-7",
      )}
    >
      {children}
    </button>
  );
}

type SourceLineProps = {
  line: string;
  highlightedHtml?: string;
  number: number;
  caret: PreviewCaret | null;
  matches: readonly PreviewMatch[];
  activeMatch: number;
  matchOffset: number;
  wordWrap: boolean;
};

/**
 * One document line: a fixed gutter number plus wrapped literal content. React
 * escapes the content, so `<script>` is text and can never become an element.
 */
const SourceLine = memo(function SourceLine({
  line,
  highlightedHtml,
  number,
  caret,
  matches,
  activeMatch,
  matchOffset,
  wordWrap,
}: SourceLineProps) {
  const contentClass = cn(
    "min-w-0 flex-1",
    wordWrap ? "whitespace-pre-wrap break-words" : "whitespace-pre",
  );

  return (
    <div
      data-testid={`file-preview-line-${number}`}
      data-line-window-row=""
      data-target={caret ? "true" : undefined}
      className={cn("flex items-start gap-3 rounded-sm", !wordWrap && "min-w-fit", caret && "bg-accent/60")}
    >
      <span
        data-testid={`file-preview-line-number-${number}`}
        aria-hidden
        className="w-10 shrink-0 select-none text-right text-muted-foreground/70 tabular-nums"
      >
        {number}
      </span>
      {matches.length > 0 ? (
        // Only lines that contain a match drop the syntax highlight (the marks are spliced
        // into the literal text); every other line keeps its highlightedHtml and props.
        <span className={contentClass}>
          {renderMatchSegments(line, matches, activeMatch, matchOffset, caret)}
        </span>
      ) : caret ? (
        <span className={contentClass}>
          {line.slice(0, caret.offset)}
          <CaretMarker caret={caret} />
          {line.slice(caret.offset)}
        </span>
      ) : highlightedHtml ? (
        // Safe only because highlight.js escapes every source character (<, >, &, quotes)
        // and highlightLines re-inserts nothing but its own <span class> tags; never feed
        // unescaped text into this path.
        <span
          className={cn(contentClass, "hljs-line")}
          dangerouslySetInnerHTML={{ __html: highlightedHtml }}
        />
      ) : (
        <span className={contentClass}>{line}</span>
      )}
    </div>
  );
});

function CaretMarker({ caret }: { caret: PreviewCaret }) {
  return (
    <span
      data-testid="file-preview-caret"
      data-offset={caret.offset}
      data-column={caret.column}
      aria-hidden
      className="inline-block h-4 w-px -translate-y-px align-middle bg-status-working"
    />
  );
}

/** Literal text for [start, end), with the caret marker spliced in when it falls inside. */
function sliceWithCaret(
  line: string,
  start: number,
  end: number,
  caret: PreviewCaret | null,
  includeEnd: boolean,
): ReactNode {
  if (!caret || caret.offset < start || caret.offset > end || (caret.offset === end && !includeEnd)) {
    return line.slice(start, end);
  }
  return (
    <>
      {line.slice(start, caret.offset)}
      <CaretMarker caret={caret} />
      {line.slice(caret.offset, end)}
    </>
  );
}

function renderMatchSegments(
  line: string,
  matches: readonly PreviewMatch[],
  activeMatch: number,
  matchOffset: number,
  caret: PreviewCaret | null,
): ReactNode[] {
  const nodes: ReactNode[] = [];
  let cursor = 0;
  matches.forEach((match, index) => {
    if (match.start > cursor) {
      nodes.push(<span key={`t${cursor}`}>{sliceWithCaret(line, cursor, match.start, caret, false)}</span>);
    }
    const flatIndex = matchOffset + index;
    const isLast = index === matches.length - 1 && match.end >= line.length;
    nodes.push(
      <mark
        key={`m${flatIndex}`}
        data-testid={`file-preview-match-${flatIndex}`}
        className={cn(
          "rounded-sm",
          flatIndex === activeMatch
            ? "bg-status-warning text-background"
            : "bg-status-warning/30 text-foreground",
        )}
      >
        {sliceWithCaret(line, match.start, match.end, caret, isLast)}
      </mark>,
    );
    cursor = match.end;
  });
  if (cursor < line.length) {
    nodes.push(<span key={`t${cursor}`}>{sliceWithCaret(line, cursor, line.length, caret, true)}</span>);
  }
  return nodes;
}

type PreviewMarkdownDocumentProps = {
  source: string;
  capability: FilePreviewMarkdownCapability | null;
  generation: number;
  containerRef: { current: HTMLDivElement | null };
  /** TOC slug per 1-based source line, so each heading's id never depends on render order. */
  slugByLine: ReadonlyMap<number, string>;
};

function PreviewMarkdownDocument({
  source,
  capability,
  generation,
  containerRef,
  slugByLine,
}: PreviewMarkdownDocumentProps) {
  const scrollToFragment = useCallback(
    (id: string) => scrollToFragmentIn(containerRef.current, id),
    [containerRef],
  );

  const components = useMemo<Components>(
    () => ({
      a: ({ href, children }) => (
        <PreviewAnchor
          href={typeof href === "string" ? href : ""}
          capability={capability}
          onFragment={scrollToFragment}
        >
          {children}
        </PreviewAnchor>
      ),
      img: ({ src, alt }) => (
        <PreviewImageNode
          src={typeof src === "string" ? src : ""}
          alt={typeof alt === "string" ? alt : ""}
          capability={capability}
          generation={generation}
        />
      ),
      // GFM task lists would otherwise mount a checkbox; the preview is inert.
      input: ({ checked }) => (
        <span aria-hidden className="mr-1 text-muted-foreground">
          {checked ? "[x]" : "[ ]"}
        </span>
      ),
      h1: ({ node, children }) => (
        <PreviewHeading level={1} slug={slugByLine.get(node?.position?.start.line ?? -1)}>{children}</PreviewHeading>
      ),
      h2: ({ node, children }) => (
        <PreviewHeading level={2} slug={slugByLine.get(node?.position?.start.line ?? -1)}>{children}</PreviewHeading>
      ),
      h3: ({ node, children }) => (
        <PreviewHeading level={3} slug={slugByLine.get(node?.position?.start.line ?? -1)}>{children}</PreviewHeading>
      ),
      h4: (props) => <PreviewHeading level={4} {...props} />,
      h5: (props) => <PreviewHeading level={5} {...props} />,
      h6: (props) => <PreviewHeading level={6} {...props} />,
      table: ({ children }) => (
        <div className="my-3 overflow-x-auto rounded-md border border-worktree-sidebar-border">
          <table className="w-full border-collapse text-[12px]">{children}</table>
        </div>
      ),
      th: ({ children }) => (
        <th className="border-b border-worktree-sidebar-border bg-background/60 px-2 py-1 text-left font-medium">
          {children}
        </th>
      ),
      td: ({ children }) => (
        <td className="border-b border-worktree-sidebar-border/60 px-2 py-1 align-top">{children}</td>
      ),
      pre: ({ children }) => (
        <pre className="my-3 overflow-x-auto rounded-md bg-background p-3 font-mono text-[12px] leading-5">
          {children}
        </pre>
      ),
      code: ({ children, className }) => (
        <code
          className={cn(
            "font-mono text-[12px]",
            className?.includes("language-") ? undefined : "rounded-sm bg-background px-1 py-0.5",
          )}
        >
          {children}
        </code>
      ),
      ul: ({ children }) => <ul className="my-2 list-disc pl-5">{children}</ul>,
      ol: ({ children }) => <ol className="my-2 list-decimal pl-5">{children}</ol>,
      p: ({ children }) => <p className="my-2">{children}</p>,
      blockquote: ({ children }) => (
        <blockquote className="my-3 border-l-2 border-worktree-sidebar-ring pl-3 text-muted-foreground">
          {children}
        </blockquote>
      ),
      hr: () => <hr className="my-4 border-worktree-sidebar-border" />,
    }),
    [capability, generation, scrollToFragment, slugByLine],
  );

  return (
    <Markdown
      components={components}
      remarkPlugins={[remarkGfm, remarkLiteralHtml]}
      // Raw HTML is already literalized; this list is belt-and-braces for any
      // element an upstream plugin change could introduce.
      disallowedElements={["script", "iframe", "object", "embed", "form", "style", "link", "meta", "audio", "video"]}
      unwrapDisallowed
      urlTransform={(url) => url}
    >
      {source}
    </Markdown>
  );
}

const HEADING_CLASS: Record<number, string> = {
  1: "mt-4 mb-2 text-[20px] font-semibold",
  2: "mt-4 mb-2 text-[17px] font-semibold",
  3: "mt-3 mb-1.5 text-[15px] font-semibold",
  4: "mt-3 mb-1.5 text-[13px] font-semibold",
  5: "mt-2 mb-1 text-[12px] font-semibold uppercase tracking-wide text-muted-foreground",
  6: "mt-2 mb-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground",
};

function PreviewHeading({
  level,
  slug,
  children,
}: {
  level: number;
  slug?: string;
  children?: ReactNode;
}) {
  const Tag = `h${level}` as "h1";
  return (
    <Tag id={slug ?? previewAnchorId(nodeText(children))} className={HEADING_CLASS[level]}>
      {children}
    </Tag>
  );
}

function nodeText(node: ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(nodeText).join("");
  if (typeof node === "object" && "props" in node) {
    return nodeText((node as { props?: { children?: ReactNode } }).props?.children);
  }
  return "";
}

const REJECTION_TEXT: Record<PreviewRejectionReason, string> = {
  "unsafe-scheme": "Blocked link scheme",
  remote: "Blocked remote target",
  "absolute-path": "Blocked absolute path",
  traversal: "Blocked path traversal",
  empty: "Empty link target",
};

type PreviewAnchorProps = {
  href: string;
  capability: FilePreviewMarkdownCapability | null;
  onFragment: (id: string) => void;
  children?: ReactNode;
};

/**
 * Anchors are buttons, never `<a href>`: nothing in the preview is navigable,
 * so no middle click, drag or keyboard activation can move the WebView.
 */
function PreviewAnchor({ href, capability, onFragment, children }: PreviewAnchorProps) {
  const link = useMemo(() => classifyPreviewLink(href), [href]);
  const linkClass =
    "rounded-sm underline decoration-worktree-sidebar-ring underline-offset-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

  if (link.kind === "rejected") {
    return (
      <span
        data-testid="file-preview-rejected-link"
        data-reason={link.reason}
        title={`${REJECTION_TEXT[link.reason]} — this link is not openable from a preview.`}
        className="cursor-not-allowed rounded-sm bg-accent/60 px-1 text-muted-foreground line-through"
      >
        {children}
      </span>
    );
  }

  if (link.kind === "fragment") {
    return (
      <button
        type="button"
        data-testid="file-preview-fragment-link"
        onClick={() => onFragment(link.id)}
        className={cn(linkClass, "text-status-working")}
      >
        {children}
      </button>
    );
  }

  if (link.kind === "document") {
    return (
      <button
        type="button"
        data-testid="file-preview-document-link"
        title={`Preview ${link.relativePath}`}
        onClick={() => capability?.requestDocument(link.relativePath)}
        className={cn(linkClass, "text-status-working")}
      >
        <FileText aria-hidden className="mr-0.5 inline size-3 align-[-2px]" />
        {children}
      </button>
    );
  }

  return (
    <button
      type="button"
      data-testid="file-preview-external-link"
      title={`Open ${link.url} in your browser`}
      onClick={() => capability?.openExternalUrl(link.url)}
      className={cn(linkClass, "text-status-working")}
    >
      {children}
      <Link2 aria-hidden className="ml-0.5 inline size-3 align-[-2px]" />
    </button>
  );
}

type PreviewImageNodeProps = {
  src: string;
  alt: string;
  capability: FilePreviewMarkdownCapability | null;
  generation: number;
};

type ImageState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; url: string }
  | { status: "error"; label: string };

/**
 * Images exist only as capability URLs. A rejected source never becomes a
 * `src`, so a hostile document cannot make the preview touch the network.
 */
function PreviewImageNode({ src, alt, capability, generation }: PreviewImageNodeProps) {
  const classified = useMemo(() => classifyPreviewImage(src), [src]);
  const [state, setState] = useState<ImageState>({ status: "idle" });
  // The budget only blocks nodes that never issued a request: once this node is loading or
  // loaded, the handle it holds counts toward the budget, so hitting 0 must not hide it.
  const budgetExhausted =
    classified.kind === "local" && state.status === "idle" && (capability?.remainingChildHandles ?? 0) <= 0;
  // R1: acquire each image at most once per generation. The in-flight request lives in a ref,
  // so a StrictMode effect replay (or a capability identity change) re-subscribes to the same
  // promise instead of opening a second child handle. The capability has no per-handle release;
  // filePreview.ts releases every child handle together with its parent.
  const requestRef = useRef<{
    generation: number;
    path: string;
    promise: Promise<FilePreviewChildAsset>;
  } | null>(null);

  useEffect(() => {
    if (classified.kind !== "local" || !capability) return;
    let request = requestRef.current;
    if (!request || request.generation !== generation || request.path !== classified.relativePath) {
      if (capability.remainingChildHandles <= 0) return;
      request = {
        generation,
        path: classified.relativePath,
        promise: capability.requestImage(classified.relativePath),
      };
      requestRef.current = request;
    }
    let cancelled = false;
    setState({ status: "loading" });
    request.promise.then(
      (asset) => {
        if (!cancelled) setState({ status: "ready", url: asset.mediaUrl });
      },
      (error: unknown) => {
        if (cancelled) return;
        setState({ status: "error", label: filePreviewErrorReason(error) ?? errorMessage(error) });
      },
    );
    return () => {
      cancelled = true;
    };
    // `generation` is a dependency on purpose: a superseded request must not
    // paint, and a reload must re-acquire its child handle.
  }, [capability, classified, generation]);

  if (classified.kind === "rejected" || budgetExhausted) {
    const label = budgetExhausted
      ? "Image limit reached for this document"
      : REJECTION_TEXT[(classified as { reason: PreviewRejectionReason }).reason];
    return (
      <span
        data-testid="file-preview-rejected-image"
        data-reason={budgetExhausted ? "child-handle-limit" : (classified as { reason: string }).reason}
        className="my-1 inline-flex items-center gap-1.5 rounded-md border border-dashed border-worktree-sidebar-border px-2 py-1 text-[11px] text-muted-foreground"
      >
        <ImageIcon aria-hidden className="size-3.5" />
        {label}
        {alt ? <span className="text-muted-foreground/70">{`— ${alt}`}</span> : null}
      </span>
    );
  }

  if (state.status === "error") {
    return (
      <span
        data-testid="file-preview-image-error"
        className="my-1 inline-flex items-center gap-1.5 rounded-md border border-dashed border-status-warning/60 px-2 py-1 text-[11px] text-status-warning"
      >
        <ImageIcon aria-hidden className="size-3.5" />
        {`Image unavailable (${state.label})`}
        {alt ? <span className="text-muted-foreground">{`— ${alt}`}</span> : null}
      </span>
    );
  }

  if (state.status !== "ready") {
    return (
      <span
        data-testid="file-preview-image-loading"
        className="my-1 inline-flex items-center gap-1.5 rounded-md border border-worktree-sidebar-border px-2 py-1 text-[11px] text-muted-foreground"
      >
        <ImageIcon aria-hidden className="size-3.5" />
        {alt || "Loading image"}
      </span>
    );
  }

  return (
    <img
      data-testid="file-preview-image"
      src={state.url}
      alt={alt}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      className="my-2 block h-auto max-w-full rounded-md border border-worktree-sidebar-border"
    />
  );
}
