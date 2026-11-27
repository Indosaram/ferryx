import React, { useEffect, useRef, useState } from "react";
import { MessageSquareQuote } from "lucide-react";
import { cn } from "../../../lib/cn";

/**
 * Assistant text selection toolbar (t3code AssistantSelectionToolbar). While a
 * selection lives inside the assistant body, a small "Cite in composer" button
 * appears and hands {messageId, text} to the pane, which routes it to the
 * composer's frozen insertCitation handle.
 */

export interface CiteSelection {
  readonly messageId: string;
  readonly text: string;
}

/**
 * The selection's text when it is anchored inside `root`, else null. Collapsed
 * selections and whitespace-only text produce nothing to cite.
 */
export function selectionWithin(root: HTMLElement | null): string | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  if (!root) return null;
  const range = selection.getRangeAt(0);
  if (!root.contains(range.commonAncestorContainer)) return null;
  const text = selection.toString();
  return text.trim().length > 0 ? text : null;
}

export const AssistantSelectionToolbar: React.FC<{
  messageId: string;
  anchorRef: React.RefObject<HTMLElement | null>;
  onCiteSelection?: (citation: CiteSelection) => void;
  className?: string;
}> = ({ messageId, anchorRef, onCiteSelection, className }) => {
  const [text, setText] = useState<string | null>(null);
  const visible = onCiteSelection !== undefined && text !== null;
  const textRef = useRef<string | null>(null);
  textRef.current = text;

  useEffect(() => {
    const readSelection = () => {
      const selected = selectionWithin(anchorRef.current);
      setText((prev) => (prev === selected ? prev : selected));
    };
    document.addEventListener("selectionchange", readSelection);
    return () => document.removeEventListener("selectionchange", readSelection);
  }, [anchorRef]);

  if (!visible) return null;
  return (
    <button
      type="button"
      data-testid="cite-selection-button"
      onClick={() => {
        const selected = textRef.current;
        if (selected) onCiteSelection?.({ messageId, text: selected });
      }}
      className={cn(
        "sticky bottom-2 z-10 ml-auto flex min-h-9 items-center gap-1.5 rounded-full border border-chat-border bg-chat-surface px-3 text-xs font-medium text-chat-foreground shadow-md transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        className,
      )}
    >
      <MessageSquareQuote aria-hidden="true" className="size-3.5" />
      Cite in composer
    </button>
  );
};
