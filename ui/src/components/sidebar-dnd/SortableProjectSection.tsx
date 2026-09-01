import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { memo, type ReactNode } from "react";

import { cn } from "../../lib/cn";
import { SidebarDragRow } from "./SidebarDragRow";

type SortableProjectSectionProps = {
  readonly workspaceId: string;
  readonly header: ReactNode;
  readonly children?: ReactNode;
};

export const SortableProjectSection = memo(function SortableProjectSection({
  workspaceId,
  header,
  children,
}: SortableProjectSectionProps) {
  const sortable = useSortable({
    id: projectSortableId(workspaceId),
    data: { type: "sidebar-project", workspaceId },
  });

  return (
    <SidebarDragRow
      kind="project"
      setNodeRef={sortable.setNodeRef}
      setActivatorNodeRef={sortable.setActivatorNodeRef}
      attributes={sortable.attributes}
      listeners={sortable.listeners}
      dragging={sortable.isDragging}
      wrapperStyle={{
        transform: CSS.Transform.toString(sortable.transform),
        transition: sortable.transition,
      }}
      wrapperClassName={cn("pb-0.5", sortable.isDragging && "z-10 opacity-25")}
    >
      {header}
      {children}
    </SidebarDragRow>
  );
});

export function projectSortableId(workspaceId: string) {
  return `sidebar-project:${workspaceId}`;
}
