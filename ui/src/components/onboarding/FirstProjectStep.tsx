import { AlertCircle, ChevronRight, FolderPlus, Monitor } from "lucide-react";

import type { GitUnavailableReason } from "../../lib/tauri";

/** What the user should install or run so Git-backed worktrees work; null when nothing. */
export function gitUnavailableMessage(reason: GitUnavailableReason | null | undefined): string | null {
  switch (reason?.kind) {
    case undefined:
    case "notARepository":
      return null;
    case "gitMissing":
      return "Git was not found. Install Git to use worktrees; folders still open as plain folders.";
    case "xcodeCommandLineToolsMissing":
      return "Git needs the Xcode Command Line Tools. Run `xcode-select --install`, then reopen the folder.";
    case "dubiousOwnership":
      return `Git refuses this folder because another user owns it. If you trust it, run \`git config --global --add safe.directory ${reason.path || "<folder>"}\`.`;
    case "gitFailed":
      return `Git failed: ${reason.message}`;
    case "nonUtf8Output":
      return "Git reported a file or folder name that is not UTF-8, so this folder opens without worktrees. Rename it to UTF-8 to use worktrees.";
  }
}

export function FirstProjectStep(props: {
  onAddProject: () => void;
  onConnectMachine: () => void;
  gitUnavailable?: GitUnavailableReason | null;
}): JSX.Element {
  const { onAddProject, onConnectMachine, gitUnavailable } = props;
  const gitMessage = gitUnavailableMessage(gitUnavailable);

  return (
    <div className="grid gap-2">
      {gitMessage ? (
        <div
          role="status"
          data-testid="onboarding-git-unavailable"
          className="flex items-start gap-2 rounded-md border border-border px-3 py-2 text-[12px] leading-relaxed text-status-warning"
        >
          <AlertCircle className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span>{gitMessage}</span>
        </div>
      ) : null}
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

      <button
        type="button"
        data-testid="onboarding-connect-machine"
        onClick={onConnectMachine}
        className="group flex w-full items-center gap-3 rounded-md border border-border px-4 py-3 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <div className="size-8 rounded-md bg-accent/60 grid place-items-center shrink-0">
          <Monitor className="size-4 text-foreground" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-foreground">
            Connect a machine
          </div>
          <div className="mt-0.5 text-[12px] text-muted-foreground leading-relaxed">
            Pair another computer through your Ferryx account.
          </div>
        </div>
        <ChevronRight className="size-4 text-muted-foreground ml-auto shrink-0" />
      </button>
    </div>
  );
}
