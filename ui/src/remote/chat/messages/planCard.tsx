import React, { useRef, useState } from "react";
import { Check, ChevronDown, Copy, ListChecks } from "lucide-react";
import { copyTextToClipboard } from "../../../lib/clipboard";
import { cn } from "../../../lib/cn";

/**
 * Proposed plan card (t3code ProposedPlanCard). A `plan` part renders as a
 * badged card with the plan's own first heading as its title, collapsed to a
 * preview when long, with the plan markdown copyable.
 *
 * The body renderer is a prop rather than an import: the chat markdown module
 * renders this card, so importing it back would make the two modules cyclic.
 */

export const PLAN_COLLAPSE_CHARS = 900;
export const PLAN_COLLAPSE_LINES = 20;
export const PLAN_PREVIEW_LINES = 10;

/** The plan's first markdown heading, else null; `stripDisplayedPlanMarkdown` drops it. */
export function proposedPlanTitle(planMarkdown: string): string | null {
  for (const line of planMarkdown.split("\n")) {
    const heading = /^#{1,6}\s+(.+)$/.exec(line.trim());
    if (heading) return heading[1]!.trim();
  }
  return null;
}

/**
 * The plan body without the heading the card already draws as its title (t3code
 * `stripDisplayedPlanMarkdown`): otherwise the plan says its own name twice.
 */
export function stripDisplayedPlanMarkdown(planMarkdown: string): string {
  const lines = planMarkdown.split("\n");
  const headingIndex = lines.findIndex((line) => /^#{1,6}\s+(.+)$/.test(line.trim()));
  if (headingIndex === -1) return planMarkdown;
  const body = lines.slice(headingIndex + 1);
  while (body.length > 0 && body[0]!.trim().length === 0) body.shift();
  return body.join("\n");
}

export function planIsLong(planMarkdown: string): boolean {
  return planMarkdown.length > PLAN_COLLAPSE_CHARS || planMarkdown.split("\n").length > PLAN_COLLAPSE_LINES;
}

export function buildPlanPreviewMarkdown(planMarkdown: string): string {
  return (
    stripDisplayedPlanMarkdown(planMarkdown)
      .split("\n")
      .slice(0, PLAN_PREVIEW_LINES)
      .join("\n") + "\n…"
  );
}

export const ProposedPlanCard: React.FC<{
  planMarkdown: string;
  className?: string;
  /** Draws the plan body; the chat's markdown pipeline is passed in by the caller. */
  renderMarkdown?: (markdown: string) => React.ReactNode;
}> = ({ planMarkdown, className, renderMarkdown }) => {
  const long = planIsLong(planMarkdown);
  const [expanded, setExpanded] = useState(!long);
  const [copied, setCopied] = useState(false);
  const title = proposedPlanTitle(planMarkdown) ?? "Proposed plan";
  // The heading became the card's title, so the body is drawn without it.
  const planBody = stripDisplayedPlanMarkdown(planMarkdown);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  React.useEffect(() => () => {
    if (timerRef.current) clearTimeout(timerRef.current);
  }, []);

  const handleCopy = async () => {
    const ok = await copyTextToClipboard(planMarkdown);
    setCopied(ok);
    timerRef.current = setTimeout(() => setCopied(false), 2000);
  };

  return (
    <section
      data-testid="proposed-plan-card"
      className={cn(
        "my-2 overflow-hidden rounded-xl border border-chat-border bg-chat-surface",
        className,
      )}
    >
      <div className="flex items-center gap-2 px-3 py-2">
        <span className="inline-flex items-center gap-1 rounded bg-chat-surface-raised px-1.5 py-0.5 text-xs font-medium text-chat-foreground-secondary">
          <ListChecks aria-hidden="true" className="size-3" />
          Plan
        </span>
        <h3 className="min-w-0 flex-1 truncate text-sm font-medium text-chat-foreground">{title}</h3>
        <button
          type="button"
          data-testid="plan-copy-button"
          aria-label={copied ? "Copied" : "Copy plan"}
          onClick={handleCopy}
          className="flex min-h-9 items-center gap-1 px-2 text-xs text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {copied ? (
            <Check aria-hidden="true" className="size-3.5 text-status-success" />
          ) : (
            <Copy aria-hidden="true" className="size-3.5" />
          )}
          {copied ? "Copied" : "Copy"}
        </button>
        {long && (
          <button
            type="button"
            data-testid="plan-expand-toggle"
            aria-expanded={expanded}
            onClick={() => setExpanded((prev) => !prev)}
            className="flex min-h-9 items-center gap-1 px-2 text-xs text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <ChevronDown
              aria-hidden="true"
              className={cn(
                "size-3.5 transition-transform duration-200 ease-out motion-reduce:transition-none",
                expanded && "rotate-180",
              )}
            />
            {expanded ? "Collapse" : "Expand"}
          </button>
        )}
      </div>
      <div className="px-3 pb-3 text-sm text-chat-foreground">
        {renderMarkdown ? (
          renderMarkdown(expanded ? planBody : buildPlanPreviewMarkdown(planMarkdown))
        ) : (
          <p className="whitespace-pre-wrap break-words">
            {expanded ? planBody : buildPlanPreviewMarkdown(planMarkdown)}
          </p>
        )}
        {!expanded && (
          <button
            type="button"
            onClick={() => setExpanded(true)}
            className="mt-1 text-xs text-chat-link underline underline-offset-2"
          >
            Show full plan
          </button>
        )}
      </div>
    </section>
  );
};
