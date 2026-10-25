import { Eye } from "lucide-react";

import { SegmentedControl } from "../ui/SegmentedControl";

export type SidebarViewId = "worktrees" | "inbox" | "supervisor";

export type SidebarViewTabsProps = {
  readonly view: SidebarViewId;
  readonly onViewChange: (view: SidebarViewId) => void;
  readonly inboxCount: number;
  /** Null hides the Supervisor tab (advanced features off or no supervisor view). */
  readonly supervisor: { readonly typing: boolean } | null;
  readonly inboxShortcut?: string;
};

/** Worktrees / Inbox / Supervisor switch at the top of the sidebar. */
export function SidebarViewTabs({ view, onViewChange, inboxCount, supervisor, inboxShortcut }: SidebarViewTabsProps) {
  return (
    <div className="shrink-0 px-2 pb-1">
      <SegmentedControl
        label="Sidebar view"
        value={view}
        onChange={onViewChange}
        options={[
          { value: "worktrees", label: "Worktrees" },
          {
            value: "inbox",
            label: "Inbox",
            ariaLabel: inboxCount > 0 ? `Inbox (${inboxCount})` : "Inbox",
            count: inboxCount > 0 ? (inboxCount > 9 ? "9+" : inboxCount) : undefined,
            countTestId: "sidebar-attention-badge",
            "data-shortcut": inboxShortcut,
          },
          ...(supervisor
            ? [{
                value: "supervisor" as const,
                label: "Supervisor",
                ariaLabel: supervisor.typing ? "Supervisor (typing)" : "Supervisor",
                count: supervisor.typing ? <Eye aria-hidden="true" className="size-3 text-foreground" /> : undefined,
                countTestId: "sidebar-supervisor-typing",
              }]
            : []),
        ]}
      />
    </div>
  );
}
