import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import type { Components } from "react-markdown";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { handleExternalLinkClick } from "../../lib/externalLinks";

import { Button } from "../ui/button";
import { IconButton } from "../ui/IconButton";

const HEADING_ID = "whats-new-heading";

const markdownComponents: Components = {
  h1: ({ children }) => (
    <h1 className="mt-4 text-[13px] font-semibold text-foreground first:mt-0">
      {children}
    </h1>
  ),
  h2: ({ children }) => (
    <h2 className="mt-4 text-[13px] font-semibold text-foreground first:mt-0">
      {children}
    </h2>
  ),
  h3: ({ children }) => (
    <h3 className="mt-3 text-[13px] font-semibold text-foreground">{children}</h3>
  ),
  p: ({ children }) => (
    <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
      {children}
    </p>
  ),
  ul: ({ children }) => (
    <ul className="mt-2 list-disc space-y-1 pl-5 text-[13px] leading-relaxed text-muted-foreground">
      {children}
    </ul>
  ),
  ol: ({ children }) => (
    <ol className="mt-2 list-decimal space-y-1 pl-5 text-[13px] leading-relaxed text-muted-foreground">
      {children}
    </ol>
  ),
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  a: ({ href, children }) => (
    <a
      href={href}
      target="_blank"
      onClick={handleExternalLinkClick}
      rel="noreferrer"
      className="text-foreground underline decoration-border underline-offset-2 hover:decoration-foreground"
    >
      {children}
    </a>
  ),
  strong: ({ children }) => (
    <strong className="font-semibold text-foreground">{children}</strong>
  ),
};

export function WhatsNewDialog(props: {
  version: string;
  notes: string;
  onClose: () => void;
}): JSX.Element {
  const { version, notes, onClose } = props;
  const closeButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeButtonRef.current?.focus({ preventScroll: true });
  }, []);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={HEADING_ID}
        onKeyDown={handleKeyDown}
        className="flex max-h-[min(36rem,90vh)] w-full max-w-lg flex-col overflow-hidden rounded-lg border border-border bg-card shadow-lg"
      >
        <div className="flex items-start justify-between gap-4 px-6 pt-5">
          <div className="min-w-0 flex-1">
            <div className="text-[11px] font-medium text-muted-foreground">
              Updated
            </div>
            <h2
              id={HEADING_ID}
              className="mt-1 text-[17px] font-semibold tracking-tight text-foreground"
            >
              {`What's new in Ferryx ${version}`}
            </h2>
          </div>
          <IconButton label="Close" size="sm" onClick={onClose}>
            <X className="size-4" />
          </IconButton>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          <Markdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
            {notes}
          </Markdown>
        </div>

        <div className="flex justify-end border-t border-border px-6 py-3">
          <Button
            ref={closeButtonRef}
            type="button"
            size="sm"
            className="min-w-24"
            data-testid="whats-new-close"
            onClick={onClose}
          >
            Got it
          </Button>
        </div>
      </div>
    </div>
  );
}
