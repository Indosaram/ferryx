import { SegmentedControl } from "../ui/SegmentedControl";

export type SidebarViewId = "supervisor" | "worktrees";

export type SidebarViewTabsProps = {
  readonly view: SidebarViewId;
  readonly onViewChange: (view: SidebarViewId) => void;
};

/** Supervisor / Worktrees switch at the top of the sidebar: the panel first, the project tree second. */
export function SidebarViewTabs({ view, onViewChange }: SidebarViewTabsProps) {
  return (
    <div className="shrink-0 px-2 pb-1">
      <SegmentedControl
        label="Sidebar view"
        value={view}
        onChange={onViewChange}
        options={[
          { value: "supervisor", label: "Supervisor" },
          { value: "worktrees", label: "Worktrees" },
        ]}
      />
    </div>
  );
}
