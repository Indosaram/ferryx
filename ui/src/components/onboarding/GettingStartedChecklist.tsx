import { ChevronRight, FolderPlus } from "lucide-react";

import { Button } from "../ui/button";

export type GettingStartedChecklistProps = {
  permissionsSummary: { granted: number; total: number } | null;
  onAddProject: () => void;
  onOpenWelcome: () => void;
};

const CHOICE_CLASS =
  "group flex w-full items-center gap-3 rounded-md border border-border px-4 py-3 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

export function GettingStartedChecklist({
  permissionsSummary,
  onAddProject,
  onOpenWelcome,
}: GettingStartedChecklistProps): JSX.Element {
  const permissionsLine =
    permissionsSummary !== null && permissionsSummary.total > 0
      ? `${permissionsSummary.granted} of ${permissionsSummary.total} permissions granted`
      : "Permissions, agents, and CLI";

  return (
    <div
      data-testid="no-projects-view"
      className="flex h-full flex-1 flex-col items-center justify-center gap-4 overflow-y-auto bg-background py-6"
    >
      <div className="w-full max-w-md px-4">
        <div>
          <p className="text-[15px] font-semibold tracking-tight text-foreground">
            No projects
          </p>
          <p className="mt-1 text-[13px] text-muted-foreground">
            Add a project to open a terminal workspace.
          </p>
        </div>

        <div className="mt-4 grid gap-2">
          <button
            type="button"
            data-testid="checklist-add-project"
            aria-label="Add Project"
            onClick={onAddProject}
            className={CHOICE_CLASS}
          >
            <div className="grid size-8 shrink-0 place-items-center rounded-md bg-accent/60">
              <FolderPlus className="size-4 text-foreground" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium text-foreground">
                Add Project
              </div>
              <div className="mt-0.5 text-[12px] leading-relaxed text-muted-foreground">
                Pick a folder on this computer. Git repositories unlock
                worktrees.
              </div>
            </div>
            <ChevronRight className="ml-auto size-4 shrink-0 text-muted-foreground" />
          </button>
        </div>

        <div
          data-testid="checklist-review-setup"
          className="mt-4 flex items-center justify-between gap-3 border-t border-border pt-4"
        >
          <span className="text-[12px] text-muted-foreground">
            {permissionsLine}
          </span>
          <Button type="button" variant="ghost" size="sm" onClick={onOpenWelcome}>
            Open Welcome Setup
          </Button>
        </div>
      </div>
    </div>
  );
}
