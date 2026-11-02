import { ChevronRight, Folder, MoreHorizontal, Plus } from "lucide-react";
import type { MouseEvent } from "react";

import { cn } from "../../lib/cn";
import { IconButton } from "../ui/IconButton";
import type { ProjectSlot } from "./sidebarSlots";
import { SidebarStateSlot } from "./SidebarStateSlot";
import type { SidebarDensity } from "./SidebarWorktreeRow";

export type SidebarProjectHeaderProps = {
  readonly label: string;
  readonly title?: string;
  readonly expanded: boolean;
  readonly active: boolean;
  readonly slot: ProjectSlot;
  readonly density?: SidebarDensity;
  /** Drag-overlay copy: every control renders disabled. */
  readonly inert?: boolean;
  readonly onToggle?: () => void;
  readonly onSelect?: () => void;
  readonly addWorktreeLabel?: string;
  readonly onAddWorktree?: () => void;
  readonly onMoreActions?: (event: MouseEvent<HTMLButtonElement>) => void;
  readonly onContextMenu?: (event: MouseEvent) => void;
  /** Makes the state slot its own button (paired machine offline: open remote settings). */
  readonly onActivateSlot?: () => void;
  readonly slotAriaLabel?: string;
};

const stopPointer = (event: { stopPropagation: () => void }) => event.stopPropagation();

/** A project row of the sidebar tree: chevron, folder label, one state slot, hover actions. */
export function SidebarProjectHeader({
  label,
  title,
  expanded,
  active,
  slot,
  density = "compact",
  inert = false,
  onToggle,
  onSelect,
  addWorktreeLabel = `Add worktree to ${label}`,
  onAddWorktree,
  onMoreActions,
  onContextMenu,
  onActivateSlot,
  slotAriaLabel,
}: SidebarProjectHeaderProps) {
  const touch = density === "touch";
  const hoverAction = cn(touch ? "size-11" : "size-5", touch ? null : "opacity-0 transition-opacity focus-visible:opacity-100 group-hover/project:opacity-100");
  const slotElement = slot.kind === "none" ? null : (
    <SidebarStateSlot
      state={slot.state}
      label={slot.label}
      title={slot.title}
      className={slot.kind === "failure" ? "text-destructive" : undefined}
      data-testid={slot.testId}
      data-attention-state={slot.kind === "attention" ? slot.state ?? undefined : undefined}
      onActivate={onActivateSlot}
      ariaLabel={slotAriaLabel}
      density={density}
    />
  );
  return (
    <div
      onContextMenu={onContextMenu}
      className={cn(
        "group/project flex w-full items-center gap-0.5 rounded-md pr-1 transition-colors",
        touch
          ? "min-h-11 text-chat-foreground hover:bg-chat-row-hover"
          : "h-7 text-worktree-sidebar-foreground/65 hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground",
      )}
    >
      <button
        type="button"
        disabled={inert}
        onClick={onToggle}
        onPointerDown={stopPointer}
        aria-expanded={expanded}
        aria-label={`${expanded ? "Collapse" : "Expand"} ${label}`}
        className={cn(
          "flex shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none",
          touch ? "size-11" : "size-5",
        )}
      >
        <ChevronRight aria-hidden="true" className={cn(touch ? "size-4" : "size-3", "shrink-0 transition-transform", expanded && "rotate-90")} />
      </button>
      <button
        type="button"
        disabled={inert}
        onClick={onSelect}
        aria-current={active ? "true" : undefined}
        aria-label={label}
        title={title}
        className={cn(
          "flex min-w-0 flex-1 items-center gap-1.5 rounded-sm py-1 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none",
          touch ? "min-h-11 text-sm" : "text-[12px]",
        )}
      >
        <Folder className={cn(touch ? "size-4" : "size-3.5", "shrink-0")} />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {onActivateSlot ? null : slotElement}
      </button>
      {onActivateSlot ? slotElement : null}
      {onAddWorktree ? (
        <IconButton label={addWorktreeLabel} size="sm" disabled={inert} className={hoverAction} onClick={onAddWorktree} onPointerDown={stopPointer}>
          <Plus className="size-3.5" />
        </IconButton>
      ) : null}
      {onMoreActions ? (
        <IconButton label={`More actions for ${label}`} size="sm" disabled={inert} className={hoverAction} onClick={onMoreActions} onPointerDown={stopPointer}>
          <MoreHorizontal className="size-3.5" />
        </IconButton>
      ) : null}
    </div>
  );
}
