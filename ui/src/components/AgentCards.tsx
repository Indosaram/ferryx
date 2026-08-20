import { Bot, ChevronRight } from "lucide-react";

import { cn } from "../lib/cn";
import type { ActiveAgent } from "../lib/types";
import { StatusDot } from "./ui/StatusDot";

type AgentCardsProps = {
  agents: ActiveAgent[];
  activeWorktreePath: string;
  onSelect: (worktreePath: string) => void;
};

const stateLabel: Record<ActiveAgent["state"], string> = {
  starting: "Starting",
  working: "Working",
  waiting: "Needs input",
  exited: "Exited",
  failed: "Failed",
};

export function AgentCards({ agents, activeWorktreePath, onSelect }: AgentCardsProps) {
  return (
    <div className="space-y-1 px-2 pb-2">
      {agents.map((agent, index) => {
        const active = agent.worktreePath === activeWorktreePath;
        return (
          <button
            key={agent.id}
            type="button"
            onClick={() => onSelect(agent.worktreePath)}
            className={cn(
              "group flex w-full animate-enter items-center gap-2 rounded-md border px-2 py-2 text-left transition-colors",
              active
                ? "border-sidebar-ring/35 bg-sidebar-accent/70 ring-1 ring-sidebar-ring/30"
                : "border-transparent hover:bg-sidebar-accent/60",
            )}
            style={{ animationDelay: `${index * 45}ms` }}
          >
            <div className="relative flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
              <Bot className="size-3.5" />
              <StatusDot state={agent.state} className="absolute -bottom-0.5 -right-0.5 ring-2 ring-sidebar" />
            </div>
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-1.5">
                <span className="truncate text-[13px] font-medium text-sidebar-foreground">{agent.name}</span>
                <span className="shrink-0 text-[10px] text-muted-foreground">{stateLabel[agent.state]}</span>
              </span>
              <span className="block truncate text-[11px] text-muted-foreground">{agent.task}</span>
            </span>
            <ChevronRight className="size-3.5 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
          </button>
        );
      })}
    </div>
  );
}
