import type { DraggableAttributes, DraggableSyntheticListeners } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical } from "lucide-react";
import { memo, type CSSProperties, type ReactNode } from "react";

import { cn } from "../../lib/cn";

type SidebarDragRowProps = {
  readonly kind: "project" | "worktree";
  readonly children: ReactNode;
  readonly setNodeRef?: (node: HTMLElement | null) => void;
  readonly setActivatorNodeRef?: (node: HTMLElement | null) => void;
  readonly attributes?: DraggableAttributes;
  readonly listeners?: DraggableSyntheticListeners;
  readonly transform?: { x: number; y: number; scaleX: number; scaleY: number } | null;
  readonly transition?: string;
  readonly wrapperStyle?: CSSProperties;
  readonly wrapperClassName?: string;
  readonly dragging?: boolean;
  readonly overlay?: boolean;
};

export const SidebarDragRow = memo(function SidebarDragRow({
  kind,
  children,
  setNodeRef,
  setActivatorNodeRef,
  attributes,
  listeners,
  transform,
  transition,
  wrapperStyle,
  wrapperClassName,
  dragging = false,
  overlay = false,
}: SidebarDragRowProps) {
  const project = kind === "project";
  const style = transform
    ? { ...wrapperStyle, transform: CSS.Transform.toString(transform), transition }
    : transition
      ? { ...wrapperStyle, transition }
      : wrapperStyle;

  return (
    <div
      ref={setNodeRef}
      data-sidebar-dnd-type={kind}
      data-sidebar-dragging={dragging ? "true" : undefined}
      role={kind === "worktree" && !overlay ? "listitem" : undefined}
      style={style}
      className={cn(
        "relative w-full",
        wrapperClassName,
        dragging && !overlay &&
          "z-10 opacity-25 before:pointer-events-none before:absolute before:inset-x-0 before:top-0 before:h-7 before:rounded-md before:bg-worktree-sidebar-accent/35",
        overlay && "rounded-md border border-worktree-sidebar-ring/70 bg-worktree-sidebar shadow-lg",
      )}
    >
      <button
        ref={setActivatorNodeRef}
        type="button"
        aria-label={`Reorder ${kind}`}
        className={cn(
          "absolute left-0 top-0 z-10 flex shrink-0 touch-none cursor-grab items-center justify-center rounded-sm text-muted-foreground/55 transition-colors",
          "hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground active:cursor-grabbing",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-worktree-sidebar-ring",
          project ? "size-5" : "h-7 w-4",
        )}
        {...attributes}
        {...listeners}
      >
        <GripVertical aria-hidden="true" className="size-3" />
      </button>
      <div className={cn("min-w-0", !project && "pl-4")}>{children}</div>
    </div>
  );
});
