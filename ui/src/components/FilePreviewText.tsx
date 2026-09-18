import { Copy, Search, ChevronDown, ChevronUp, FileText, Image as ImageIcon, Link2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import Markdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { cn } from "../lib/cn";
import {
  filePreviewErrorReason,
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

  const [query, setQuery] = useState("");
  const [requestedMatch, setRequestedMatch] = useState(0);
  const matches = useMemo(() => findPreviewMatches(lines, query), [lines, query]);
  const activeMatch = matches.length === 0 ? -1 : Math.min(requestedMatch, matches.length - 1);

  const sourceRef = useRef<HTMLDivElement | null>(null);
  const documentRef = useRef<HTMLDivElement | null>(null);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    setRequestedMatch(0);
  }, [query, text]);

  // Selecting the active match is how "search" behaves here: it moves the
  // browser selection over existing text and never rewrites the document.
  useEffect(() => {
    if (!showSource || activeMatch < 0) return;
    const host = sourceRef.current?.querySelector(`[data-testid="file-preview-match-${activeMatch}"]`);
    if (!(host instanceof HTMLElement)) return;
    host.scrollIntoView({ block: "center" });
    const selection = window.getSelection?.();
    const node = host.firstChild;
    if (!selection || !node) return;
    const range = document.createRange();
    range.selectNodeContents(host);
    selection.removeAllRanges();
    selection.addRange(range);
  }, [activeMatch, matches, showSource]);

  useEffect(() => {
    if (!showSource || !caret) return;
    const line = sourceRef.current?.querySelector(`[data-testid="file-preview-line-${caret.line}"]`);
    if (line instanceof HTMLElement) line.scrollIntoView({ block: "center" });
  }, [caret, showSource]);

  useEffect(
    () => () => {
      if (copyTimer.current) clearTimeout(copyTimer.current);
    },
    [],
  );

  const handleCopy = useCallback(async () => {
    try {
      const clipboard = navigator.clipboard;
      if (!clipboard?.writeText) throw new Error("clipboard unavailable");
      await clipboard.writeText(text);
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

  return (
    <div className="flex h-full min-h-0 flex-col bg-card text-foreground" data-testid="file-preview-text">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-worktree-sidebar-border px-2">
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
                placeholder="Search"
                className="h-6 w-32 min-w-0 bg-transparent text-[12px] text-foreground outline-none placeholder:text-muted-foreground"
              />
            </label>
            {query.trim().length > 0 ? (
              <>
                <span
                  data-testid="file-preview-search-status"
                  className="whitespace-nowrap text-[11px] tabular-nums text-muted-foreground"
                >
                  {matches.length === 0 ? "No matches" : `${activeMatch + 1} of ${matches.length}`}
                </span>
                <ToolbarButton
                  label="Previous match"
                  testId="file-preview-search-previous"
                  disabled={matches.length === 0}
                  onClick={() =>
                    setRequestedMatch((current) => {
                      const base = Math.min(current, matches.length - 1);
                      return (base - 1 + matches.length) % matches.length;
                    })
                  }
                >
                  <ChevronUp aria-hidden className="size-3.5" />
                </ToolbarButton>
                <ToolbarButton
                  label="Next match"
                  testId="file-preview-search-next"
                  disabled={matches.length === 0}
                  onClick={() =>
                    setRequestedMatch((current) => {
                      const base = Math.min(current, matches.length - 1);
                      return (base + 1) % matches.length;
                    })
                  }
                >
                  <ChevronDown aria-hidden className="size-3.5" />
                </ToolbarButton>
              </>
            ) : null}
          </div>
        ) : (
          <span className="truncate text-[11px] text-muted-foreground">Rendered Markdown</span>
        )}

        <div className="ml-auto flex items-center gap-2">
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
          ref={sourceRef}
          data-testid="file-preview-source"
          role="document"
          aria-readonly="true"
          aria-label={`${payload.displayName} source`}
          tabIndex={0}
          className="min-h-0 flex-1 overflow-auto px-2 py-2 font-mono text-[12px] leading-5 focus-visible:outline-none"
        >
          {lines.map((line, index) => (
            <SourceLine
              key={index}
              line={line}
              number={index + 1}
              caret={caret && caret.line === index + 1 ? caret : null}
              matches={matchesByLine.get(index) ?? []}
              activeMatch={activeMatch}
              matchOffset={firstIndexByLine.get(index) ?? -1}
            />
          ))}
        </div>
      ) : (
        <div
          ref={documentRef}
          data-testid="file-preview-markdown"
          className="min-h-0 flex-1 overflow-auto px-4 py-3 text-[13px] leading-6 [overflow-wrap:anywhere]"
        >
          <PreviewMarkdownDocument
            source={text}
            capability={markdown}
            generation={generation}
            containerRef={documentRef}
          />
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
};

function ToolbarButton({ label, testId, onClick, children, disabled, wide }: ToolbarButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
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
  number: number;
  caret: PreviewCaret | null;
  matches: readonly PreviewMatch[];
  activeMatch: number;
  matchOffset: number;
};

/**
 * One document line: a fixed gutter number plus wrapped literal content. React
 * escapes the content, so `<script>` is text and can never become an element.
 */
function SourceLine({ line, number, caret, matches, activeMatch, matchOffset }: SourceLineProps) {
  return (
    <div
      data-testid={`file-preview-line-${number}`}
      data-target={caret ? "true" : undefined}
      className={cn("flex items-start gap-3 rounded-sm", caret && "bg-accent/60")}
    >
      <span
        data-testid={`file-preview-line-number-${number}`}
        aria-hidden
        className="w-10 shrink-0 select-none text-right text-muted-foreground/70 tabular-nums"
      >
        {number}
      </span>
      {matches.length > 0 ? (
        <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">
          {renderMatchSegments(line, matches, activeMatch, matchOffset)}
        </span>
      ) : caret ? (
        <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">
          {line.slice(0, caret.offset)}
          <span
            data-testid="file-preview-caret"
            data-offset={caret.offset}
            data-column={caret.column}
            aria-hidden
            className="inline-block h-4 w-px -translate-y-px align-middle bg-status-working"
          />
          {line.slice(caret.offset)}
        </span>
      ) : (
        <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">{line}</span>
      )}
    </div>
  );
}

function renderMatchSegments(
  line: string,
  matches: readonly PreviewMatch[],
  activeMatch: number,
  matchOffset: number,
): ReactNode[] {
  const nodes: ReactNode[] = [];
  let cursor = 0;
  matches.forEach((match, index) => {
    if (match.start > cursor) nodes.push(line.slice(cursor, match.start));
    const flatIndex = matchOffset + index;
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
        {line.slice(match.start, match.end)}
      </mark>,
    );
    cursor = match.end;
  });
  if (cursor < line.length) nodes.push(line.slice(cursor));
  return nodes;
}

type PreviewMarkdownDocumentProps = {
  source: string;
  capability: FilePreviewMarkdownCapability | null;
  generation: number;
  containerRef: { current: HTMLDivElement | null };
};

function PreviewMarkdownDocument({
  source,
  capability,
  generation,
  containerRef,
}: PreviewMarkdownDocumentProps) {
  const scrollToFragment = useCallback(
    (id: string) => {
      const host = containerRef.current;
      if (!host || id.length === 0) return;
      const escaped = typeof CSS !== "undefined" && CSS.escape ? CSS.escape(id) : id;
      const anchor = host.querySelector(`#${escaped}`);
      if (anchor instanceof HTMLElement) anchor.scrollIntoView({ block: "start" });
    },
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
      h1: (props) => <PreviewHeading level={1} {...props} />,
      h2: (props) => <PreviewHeading level={2} {...props} />,
      h3: (props) => <PreviewHeading level={3} {...props} />,
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
    [capability, generation, scrollToFragment],
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

function PreviewHeading({ level, children }: { level: number; children?: ReactNode }) {
  const Tag = `h${level}` as "h1";
  return (
    <Tag id={previewAnchorId(nodeText(children))} className={HEADING_CLASS[level]}>
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
  const budgetExhausted = classified.kind === "local" && (capability?.remainingChildHandles ?? 0) <= 0;
  const [state, setState] = useState<ImageState>({ status: "idle" });
  // R1: acquire each image at most once per generation, avoiding re-acquisition loops
  const requestedRef = useRef(false);

  useEffect(() => {
    requestedRef.current = false;
  }, [generation]);

  useEffect(() => {
    if (classified.kind !== "local" || !capability || budgetExhausted) return;
    if (requestedRef.current) return;
    requestedRef.current = true;
    let cancelled = false;
    setState({ status: "loading" });
    capability.requestImage(classified.relativePath).then(
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
  }, [budgetExhausted, capability, classified, generation]);

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
