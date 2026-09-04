import type { DraggableAttributes, DraggableSyntheticListeners } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
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
        overlay && "rounded-md border border-worktree-sidebar-ring/70 bg-worktree-sidebar shadow-lg cursor-grabbing",
      )}
    >
      <div
        ref={setActivatorNodeRef}
        {...attributes}
        {...listeners}
        role={undefined}
        tabIndex={undefined}
        className="min-w-0 select-none"
      >
        {children}
      </div>
    </div>
  );
});
