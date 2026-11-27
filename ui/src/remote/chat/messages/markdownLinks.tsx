import React, { createContext, useContext } from "react";
import { FileText } from "lucide-react";
import { cn } from "../../../lib/cn";
import { handleExternalLinkClick } from "../../../lib/externalLinks";

/**
 * Markdown links (t3code MarkdownFileLink / MarkdownExternalLink). A file link
 * refers to the environment's machine, so it renders as a button and the pane's
 * onOpenFile callback decides how to open it; a local path never becomes an
 * <a href>. External links go through onOpenExternal.
 */

export interface MarkdownLinkActions {
  /** Open a workspace file, optionally scrolled to `line` (1-based). */
  onOpenFile?: (path: string, line?: number) => void;
  /** Open an external URL in the system browser. */
  onOpenExternal?: (url: string) => void;
}

const MarkdownLinkActionsContext = createContext<MarkdownLinkActions | null>(null);

export function MarkdownLinkActionsProvider({
  actions,
  children,
}: {
  actions: MarkdownLinkActions | null;
  children: React.ReactNode;
}) {
  return (
    <MarkdownLinkActionsContext.Provider value={actions}>
      {children}
    </MarkdownLinkActionsContext.Provider>
  );
}

export function useMarkdownLinkActions(): MarkdownLinkActions | null {
  return useContext(MarkdownLinkActionsContext);
}

const EXTERNAL_PROTOCOL = /^(?:https?:|mailto:|tel:)/i;
/** `path`, `path:42`, and `path#L42` all name a file on the environment's machine. */
const FILE_LINE_PATTERN = /^(.*?)(?::(\d+))?(?:#L(\d+))?$/;

export function isExternalLinkHref(href: string): boolean {
  return EXTERNAL_PROTOCOL.test(href.trim());
}

export interface FileLinkTarget {
  readonly path: string;
  readonly line?: number;
}

export function parseFileLinkHref(href: string): FileLinkTarget | null {
  const trimmed = href.trim();
  if (trimmed.length === 0 || isExternalLinkHref(trimmed)) return null;
  // An in-page anchor, a query-only reference, or a protocol-relative URL names no file on
  // the environment's machine: `#x` scrolls this page, `//host/x` is another origin.
  if (trimmed.startsWith("#") || trimmed.startsWith("?") || trimmed.startsWith("//")) return null;
  const match = FILE_LINE_PATTERN.exec(trimmed);
  if (!match || match[1]!.trim().length === 0) return null;
  const line = Number(match[2] ?? match[3]);
  return {
    path: match[1]!.trim(),
    line: Number.isFinite(line) && line > 0 ? line : undefined,
  };
}

export const MarkdownFileLink: React.FC<{ path: string; line?: number; children?: React.ReactNode }> = ({
  path,
  line,
  children,
}) => {
  const actions = useMarkdownLinkActions();
  return (
    <button
      type="button"
      data-testid="markdown-file-link"
      data-path={path}
      data-line={line ?? undefined}
      onClick={() => actions?.onOpenFile?.(path, line)}
      className={cn(
        "inline-flex items-center gap-1 rounded border border-chat-border bg-chat-surface px-1.5 py-0.5 font-mono text-[0.875em] text-chat-link underline decoration-chat-link/60 underline-offset-2 transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        !actions?.onOpenFile && "cursor-default",
      )}
    >
      <FileText aria-hidden="true" className="size-3 shrink-0" />
      <span className="break-all">{children ?? path}</span>
    </button>
  );
};

/** The `a` override the chat markdown uses: buttons for files, handled external links. */
export const MarkdownLink: React.FC<{ href?: string; children?: React.ReactNode }> = ({
  href,
  children,
}) => {
  const actions = useMarkdownLinkActions();
  if (typeof href === "string" && !isExternalLinkHref(href)) {
    const target = parseFileLinkHref(href);
    if (target) return <MarkdownFileLink path={target.path} line={target.line}>{children}</MarkdownFileLink>;
  }
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      data-testid="markdown-external-link"
      onClick={(event) => {
        if (typeof href === "string" && actions?.onOpenExternal) {
          event.preventDefault();
          actions.onOpenExternal(href);
          return;
        }
        handleExternalLinkClick(event);
      }}
      className="text-chat-link decoration-chat-link/60 hover:decoration-chat-link underline underline-offset-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded"
    >
      {children}
    </a>
  );
};
