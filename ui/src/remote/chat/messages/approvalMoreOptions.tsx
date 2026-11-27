import React, { useEffect, useId, useRef, useState } from "react";
import { Ellipsis, TriangleAlert } from "lucide-react";
import { cn } from "../../../lib/cn";

/**
 * "More approval options" (t3code ComposerPendingApprovalActions). Approve and
 * decline stay the two primary buttons; every wider decision a provider offers
 * (Cancel, Always allow this session, ...) lives behind one menu. An option that
 * carries a warning shows the warning icon and its text on hover and to the
 * screen reader, the way the reference pairs the trigger with a tooltip.
 */

export interface ApprovalExtraOption {
  readonly decision: string;
  readonly label: string;
  /** Why this choice is risky; also what the reader hears for the item. */
  readonly warning?: string;
}

/** The built-in wider choices, in the reference's own order. */
export const DEFAULT_APPROVAL_EXTRA_OPTIONS: readonly ApprovalExtraOption[] = [
  { decision: "cancel", label: "Cancel" },
  {
    decision: "acceptForSession",
    label: "Always allow this session",
    warning: "This lets the agent run similar commands without asking again until the session ends.",
  },
];

export const ApprovalMoreOptionsMenu: React.FC<{
  options?: readonly ApprovalExtraOption[];
  disabled?: boolean;
  onChoose: (decision: string, event: React.MouseEvent<HTMLButtonElement>) => void;
  className?: string;
}> = ({ options = DEFAULT_APPROVAL_EXTRA_OPTIONS, disabled = false, onChoose, className }) => {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const menuId = useId();

  // A menu that cannot be dismissed is a trap: Escape and a tap outside close it.
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open]);

  if (options.length === 0) return null;

  return (
    <div ref={containerRef} className={cn("relative", className)}>
      <button
        type="button"
        data-testid="approval-more-options"
        aria-label="More approval options"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        disabled={disabled}
        onClick={() => setOpen((prev) => !prev)}
        className="flex min-h-11 w-full items-center justify-center gap-1.5 rounded-lg px-4 text-sm text-chat-foreground-secondary transition-colors hover:bg-chat-row-hover hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <Ellipsis aria-hidden="true" className="size-4 shrink-0" />
        <span>More approval options</span>
      </button>
      {open && (
        <div
          id={menuId}
          role="menu"
          data-testid="approval-more-options-menu"
          className="absolute inset-x-0 bottom-full z-30 mb-1 flex flex-col overflow-hidden rounded-lg border border-chat-border bg-chat-surface shadow-xl"
        >
          {options.map((option) => (
            <button
              key={option.decision}
              type="button"
              role="menuitem"
              data-testid={`approval-option-${option.decision}`}
              data-decision={option.decision}
              aria-description={option.warning}
              title={option.warning}
              disabled={disabled}
              onClick={(event) => {
                setOpen(false);
                onChoose(option.decision, event);
              }}
              className="flex min-h-11 items-center gap-2 px-4 text-left text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              {option.warning && (
                <TriangleAlert aria-hidden="true" className="size-3 shrink-0 text-status-warning" />
              )}
              <span className="min-w-0 whitespace-normal break-words">{option.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
