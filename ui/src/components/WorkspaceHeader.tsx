import { Bot, GitBranch, PanelRight, Radio, SplitSquareHorizontal, SplitSquareVertical } from "lucide-react";

import type { ActiveAgent, Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { StatusDot } from "./ui/StatusDot";

type WorkspaceHeaderProps = {
  worktree: Worktree;
  agent?: ActiveAgent;
  onSplit?: () => void;
  splitState?: "none" | "horizontal" | "vertical";
};

function branchName(worktree: Worktree) {
  return worktree.branch?.replace(/^refs\/heads\//, "") ?? "detached HEAD";
}

function basename(path: string) {
  if (path === ".") return "main";
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function WorkspaceHeader({ worktree, agent, onSplit, splitState = "none" }: WorkspaceHeaderProps) {
  return (
    <header className="drag-region flex h-titlebar shrink-0 items-center border-b border-border bg-sidebar pl-3 pr-2">
      <div className="flex min-w-0 flex-1 items-center gap-2">
        <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-accent text-muted-foreground">
          <GitBranch className="size-3.5" />
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <span className="truncate text-[13px] font-semibold text-foreground">{basename(worktree.path)}</span>
            {agent ? <StatusDot state={agent.state} /> : null}
          </div>
          <div className="truncate font-mono text-[10px] text-muted-foreground">{branchName(worktree)}</div>
        </div>
      </div>

      {agent ? (
        <div className="no-drag mr-2 hidden items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11px] text-muted-foreground md:flex">
          <Bot className="size-3" />
          <span className="max-w-agent truncate">{agent.name}</span>
          <Radio className="size-2.5 text-status-working" />
        </div>
      ) : null}

      <div className="no-drag flex items-center gap-0.5">
        <IconButton
          label={`Split terminal (${splitState})`}
          size="sm"
          onClick={onSplit}
          className={splitState !== "none" ? "bg-accent text-foreground" : ""}
        >
          {splitState === "vertical" ? (
            <SplitSquareVertical className="size-3.5" />
          ) : (
            <SplitSquareHorizontal className="size-3.5" />
          )}
        </IconButton>
        <IconButton label="Toggle details" size="sm">
          <PanelRight className="size-3.5" />
        </IconButton>
      </div>
    </header>
  );
}
