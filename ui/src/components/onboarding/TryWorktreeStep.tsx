import { useRef, useState } from "react";
import { GitBranch } from "lucide-react";
import { toast } from "sonner";

import type { ResolvedAgent } from "../../lib/agentsSettings";
import { getHostPlatform } from "../../lib/platform";
import { createWorktree, toIpcError, type RegisteredProject } from "../../lib/tauri";
import type { Worktree } from "../../lib/types";
import type { WorktreeAgentLaunch } from "../ProjectDialogs";
import { Button } from "../ui/button";
import { getAgentInstallCommand } from "./agentInstallCommands";

/** Slug of the worktree the first-run step creates; the backend derives its branch and path. */
export const TRY_WORKTREE_SLUG = "try-ferryx";
/** Slugs tried when an earlier run already left try-ferryx behind: try-ferryx-2 ... try-ferryx-9. */
const MAX_TRY_SLUG_ATTEMPTS = 9;

/**
 * The agent a new agent tab starts with, mirroring the TabBar new-tab menu
 * (TabBar.tsx, the `launchable` list): enabled and available agents, the default
 * first when set (and not "none"), otherwise the first in detection order.
 */
export function firstRunAgent(
  agents: ReadonlyArray<ResolvedAgent>,
  defaultAgentId: string | null,
): ResolvedAgent | null {
  const launchable = agents.filter((agent) => agent.enabled !== false && agent.available !== false);
  const preferred =
    defaultAgentId && defaultAgentId !== "none"
      ? launchable.find((agent) => agent.name === defaultAgentId)
      : undefined;
  return preferred ?? launchable[0] ?? null;
}

export type TryWorktreeStepProps = {
  /** Null when setup has no local project yet; the step then only explains how to get one. */
  project: RegisteredProject | null;
  agents: ReadonlyArray<ResolvedAgent>;
  defaultAgentId: string | null;
  /** The Add Worktree dialog's post-create path: open the tab, then start the agent. */
  onCreated: (worktree: Worktree, launch?: WorktreeAgentLaunch) => void | Promise<void>;
  onFinish: () => void;
  /** Reports an in-flight create so the wizard footer (whose Continue is this step's Skip) waits. */
  onBusyChange?: (busy: boolean) => void;
};

function agentLabel(name: string): string {
  return name.charAt(0).toUpperCase() + name.slice(1);
}

function errorMessage(cause: unknown): string {
  const { message } = toIpcError(cause);
  return message && message !== "Unknown IPC error" ? message : "Could not create the worktree.";
}

export function TryWorktreeStep(props: TryWorktreeStepProps): JSX.Element {
  const { project, agents, defaultAgentId, onCreated, onFinish, onBusyChange } = props;
  const [creating, setCreatingState] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notGit, setNotGit] = useState(project?.gitRoot === null);
  const setCreating = (busy: boolean) => {
    setCreatingState(busy);
    onBusyChange?.(busy);
  };
  const inFlight = useRef(false);

  const agent = firstRunAgent(agents, defaultAgentId);
  const hostPlatform = getHostPlatform();
  const installHelp = agent
    ? []
    : agents.flatMap((candidate) => {
        if (candidate.custom) return [];
        const command = getAgentInstallCommand(candidate.name, hostPlatform);
        return command ? [{ name: candidate.name, command }] : [];
      });

  if (!project) {
    return (
      <p data-testid="onboarding-try-no-project" className="text-[12px] leading-relaxed text-muted-foreground">
        Add a local Git project in the previous step to try a worktree here, or skip this step. You can create
        worktrees any time from the sidebar.
      </p>
    );
  }

  const handleTry = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setCreating(true);
    setError(null);
    let worktree: Worktree | null = null;
    try {
      // A re-run of setup (or an earlier try) may have left try-ferryx behind; the backend
      // reports a taken path or a leftover branch as WORKTREE_ALREADY_EXISTS, so take the next free slug.
      for (let attempt = 1; worktree === null; attempt += 1) {
        const slug = attempt === 1 ? TRY_WORKTREE_SLUG : `${TRY_WORKTREE_SLUG}-${attempt}`;
        try {
          worktree = await createWorktree({
            workspaceId: project.workspaceId,
            worktree: { wsId: project.workspaceId, slug },
            baseRef: "HEAD",
          });
        } catch (cause) {
          if (toIpcError(cause).code !== "WORKTREE_ALREADY_EXISTS" || attempt >= MAX_TRY_SLUG_ATTEMPTS) throw cause;
        }
      }
    } catch (cause) {
      // WorktreeError::NotAGitRepository reaches the UI as INVALID_REPO_ROOT (ipc/error.rs).
      if (toIpcError(cause).code === "INVALID_REPO_ROOT") setNotGit(true);
      else setError(errorMessage(cause));
      inFlight.current = false;
      setCreating(false);
      return;
    }
    // The worktree exists now: a failure to open its tab or start the agent is reported,
    // but setup still finishes so a retry cannot trip over the worktree just created.
    try {
      await onCreated(worktree, agent ? { agentId: agent.name } : undefined);
    } catch (cause) {
      toast.error(`The worktree was created, but the agent did not start: ${errorMessage(cause)}`);
    }
    inFlight.current = false;
    onBusyChange?.(false);
    onFinish();
  };

  return (
    <div className="grid gap-3">
      <div className="flex items-center gap-3 rounded-md border border-border px-4 py-3">
        <div className="size-8 rounded-md bg-accent/60 grid place-items-center shrink-0">
          <GitBranch className="size-4 text-foreground" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-medium text-foreground">{project.workspaceId}</div>
          <div className="mt-0.5 truncate text-[12px] text-muted-foreground">{project.repoRoot}</div>
        </div>
      </div>

      {notGit ? (
        <p data-testid="onboarding-try-not-git" className="text-[12px] leading-relaxed text-muted-foreground">
          This folder is not a Git repository, so Ferryx cannot create a worktree in it. Skip this step; terminals
          still work in the folder.
        </p>
      ) : (
        <>
          <p className="text-[12px] leading-relaxed text-muted-foreground">
            {agent
              ? `Creates a worktree named ${TRY_WORKTREE_SLUG} (or the next free name) from HEAD and starts ${agentLabel(agent.name)} in a new tab.`
              : `Creates a worktree named ${TRY_WORKTREE_SLUG} (or the next free name) from HEAD with a shell. No coding agent is installed yet.`}
          </p>
          {installHelp.length > 0 ? (
            <div data-testid="onboarding-try-install-help" className="grid gap-1.5 rounded-md border border-border bg-background/40 p-3">
              {installHelp.map((entry) => (
                <div key={entry.name} className="flex items-center gap-2">
                  <span className="w-20 shrink-0 truncate text-[12px] font-medium text-foreground">{entry.name}</span>
                  <code className="selectable min-w-0 flex-1 truncate rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground" title={entry.command}>
                    {entry.command}
                  </code>
                </div>
              ))}
            </div>
          ) : null}
        </>
      )}

      {error ? (
        <p data-testid="onboarding-try-error" className="text-[12px] text-destructive">
          {error}
        </p>
      ) : null}

      {notGit ? null : (
        <div className="flex items-center gap-2">
          <Button size="sm" data-testid="onboarding-try-create" disabled={creating} onClick={() => void handleTry()}>
            {agent ? `Create a worktree and start ${agentLabel(agent.name)}` : "Create a worktree"}
          </Button>
        </div>
      )}
    </div>
  );
}
