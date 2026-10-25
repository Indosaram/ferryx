import { Server } from "lucide-react";

import { cn } from "../../lib/cn";
import { StatusDot } from "../ui/StatusDot";

export type SidebarSlotState = "waiting" | "working" | "unread" | "done" | null;

export type SidebarStateSlotProps = {
  state: SidebarSlotState;
  /** Overrides "needs input" / "running"; for an idle slot it is the whole text (e.g. machine status). */
  label?: string;
  /** Remote host name, shown with a server icon when the slot carries no activity state. */
  host?: string;
  /** Makes the slot a button. Without it the slot is a non-interactive span. */
  onActivate?: () => void;
  title?: string;
  ariaLabel?: string;
  className?: string;
  "data-testid"?: string;
  "data-attention-state"?: string;
};

const ACTIVITY_LABELS = { waiting: "needs input", working: "running" } as const;

/** The single trailing status element of a sidebar row (ui/DESIGN.md section 2 and section 4 rule 7). */
export function SidebarStateSlot({ state, label, host, onActivate, title, ariaLabel, className, ...data }: SidebarStateSlotProps) {
  const showHost = state === null && Boolean(host);
  const text = state === "waiting" || state === "working"
    ? (label ?? ACTIVITY_LABELS[state])
    : state === null && !showHost ? label : undefined;
  if (state === null && !showHost && !text) return null;

  const content = (
    <>
      {showHost ? <Server aria-hidden="true" className="size-2.5 shrink-0" /> : null}
      {showHost ? <span className="max-w-[88px] truncate">{host}</span> : null}
      {text ? <span className="truncate">{text}</span> : null}
      {state ? <StatusDot state={state} /> : null}
    </>
  );
  // The slot's state as data, so tests compare dots whose colour is their only difference.
  const stateData = state ? { "data-slot-state": state } : {};
  const classes = cn("inline-flex shrink-0 items-center gap-1 text-[10px] leading-none text-muted-foreground", className);

  if (onActivate) {
    return (
      <button
        type="button"
        title={title}
        aria-label={ariaLabel}
        className={cn(classes, "rounded-sm hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring")}
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          event.stopPropagation();
          onActivate();
        }}
        {...stateData}
        {...data}
      >
        {content}
      </button>
    );
  }
  return (
    <span title={title} aria-label={ariaLabel} className={classes} {...stateData} {...data}>
      {content}
    </span>
  );
}
