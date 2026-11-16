import { ChevronRight, FolderPlus } from "lucide-react";

export function FirstProjectStep(props: { onAddProject: () => void }): JSX.Element {
  const { onAddProject } = props;

  return (
    <div className="grid gap-2">
      <button
        type="button"
        data-testid="onboarding-add-project"
        onClick={onAddProject}
        className="group flex w-full items-center gap-3 rounded-md border border-border px-4 py-3 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <div className="size-8 rounded-md bg-accent/60 grid place-items-center shrink-0">
          <FolderPlus className="size-4 text-foreground" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-foreground">
            Add a local folder
          </div>
          <div className="mt-0.5 text-[12px] text-muted-foreground leading-relaxed">
            Pick a folder on this computer. Git repositories unlock worktrees.
          </div>
        </div>
        <ChevronRight className="size-4 text-muted-foreground ml-auto shrink-0" />
      </button>
    </div>
  );
}
