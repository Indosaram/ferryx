import { Check, GitBranch, LoaderCircle, Terminal as TerminalIcon } from "lucide-react";
import React from "react";
import { resolveAgentLogo, isMonochromeAgentLogo } from "../lib/agentIcon";
import { cn } from "../lib/cn";
import { StatusDot } from "../components/ui/StatusDot";
import type { RemoteInventoryActivityState } from "./remoteInventoryView";

type ActivityGlyphProps = {
  state: RemoteInventoryActivityState | null;
  testId: string;
  className?: string;
};

export function RemoteInventoryActivityGlyph({ state, testId, className }: ActivityGlyphProps) {
  if (state === null) return null;
  return (
    <span data-testid={testId} className={cn("flex shrink-0 items-center", className)}>
      <StatusDot state={state} />
    </span>
  );
}

export type RemoteInventoryProjectRowProps = {
  label: string;
  activity: RemoteInventoryActivityState | null;
};

export function RemoteInventoryProjectRow({ label, activity }: RemoteInventoryProjectRowProps) {
  return (
    <div className="flex h-7 items-center gap-1.5 rounded-md px-2 text-[11px] font-medium text-worktree-sidebar-foreground/65">
      <h3 className="min-w-0 flex-1 truncate" title={label}>
        {label}
      </h3>
      <RemoteInventoryActivityGlyph
        state={activity}
        testId={`project-${activity ?? "idle"}-indicator`}
      />
    </div>
  );
}

export type RemoteInventoryWorktreeRowProps = {
  /** Accessible name: the full project/worktree identity the picker has always exposed. */
  label: string;
  /** Visible row text: the worktree name alone, since the project header owns the project name. */
  visibleLabel?: string;
  isRootWorktree: boolean;
  active: boolean;
  disabled: boolean;
  busy: boolean;
  activity: RemoteInventoryActivityState | null;
  hasSessions: boolean;
  onSelect: () => void;
};

export const RemoteInventoryWorktreeRow: React.FC<RemoteInventoryWorktreeRowProps> = ({
  label,
  visibleLabel,
  isRootWorktree,
  active,
  disabled,
  busy,
  activity,
  hasSessions,
  onSelect,
}) => (
  <button
    type="button"
    aria-current={active ? "true" : undefined}
    aria-label={label}
    disabled={disabled}
    onClick={onSelect}
    className={cn(
      "flex h-7 w-full items-center gap-1.5 rounded-md py-1 pl-3 pr-2 text-left transition-colors",
      "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60",
      active
        ? "bg-worktree-sidebar-accent text-foreground"
        : "text-worktree-sidebar-foreground hover:bg-white/[0.04]",
    )}
  >
    {busy ? (
      <LoaderCircle className="size-3 shrink-0 animate-spin motion-reduce:animate-none" aria-hidden="true" />
    ) : (
      <RemoteInventoryActivityGlyph
        state={activity}
        testId={`worktree-${activity ?? "idle"}-indicator`}
        className="size-3 justify-center"
      />
    )}
    {activity === null && !busy ? (
      hasSessions ? (
        <TerminalIcon className="size-3 shrink-0 opacity-70" aria-hidden="true" />
      ) : (
        <GitBranch className="size-3 shrink-0 opacity-70" aria-hidden="true" />
      )
    ) : null}
    <span
      data-testid="worktree-label-text"
      className="min-w-0 flex-1 truncate text-[12px] font-semibold leading-tight"
    >
      {visibleLabel ?? label}
    </span>
    {isRootWorktree ? (
      <span
        data-testid="worktree-primary-badge"
        className="shrink-0 rounded-sm border border-worktree-sidebar-border px-1 text-[9px] font-medium uppercase tracking-wide text-worktree-sidebar-foreground/65"
      >
        primary
      </span>
    ) : null}
    {active ? <Check className="size-3 shrink-0" aria-label="Active" /> : null}
  </button>
);

export type RemoteInventorySessionRowProps = {
  label: string;
  sessionId: string | null | undefined;
  activity: RemoteInventoryActivityState | null;
  agentType?: string | null;
  active: boolean;
  disabled: boolean;
  busy: boolean;
  testId: string;
  onSelect: () => void;
};

export const RemoteInventorySessionRow: React.FC<RemoteInventorySessionRowProps> = ({
  label,
  sessionId,
  activity,
  agentType,
  active,
  disabled,
  busy,
  testId,
  onSelect,
}) => {
  const logo = agentType ? resolveAgentLogo(agentType) : null;
  const monochrome = agentType ? isMonochromeAgentLogo(agentType) : false;
  return (
    <button
      type="button"
      data-testid={testId}
      data-session-id={sessionId ?? undefined}
      aria-current={active ? "true" : undefined}
      aria-label={activity ? `${label} (${activity})` : label}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        "flex min-h-[24px] w-full items-center gap-1.5 rounded-md py-0.5 pl-6 pr-2 text-left transition-colors",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60",
        active ? "bg-white/[0.06] text-foreground" : "text-worktree-sidebar-foreground/85 hover:bg-white/[0.04]",
      )}
    >
      {busy ? (
        <LoaderCircle className="size-3 shrink-0 animate-spin motion-reduce:animate-none" aria-hidden="true" />
      ) : logo ? (
        <img
          src={logo}
          alt=""
          data-testid="tab-agent-icon"
          data-agent-type={agentType ?? undefined}
          className={cn("size-3 shrink-0", monochrome && "agent-tab-logo--monochrome opacity-80")}
        />
      ) : (
        <TerminalIcon
          data-testid="tab-terminal-icon"
          className="size-3 shrink-0 opacity-70"
          aria-hidden="true"
        />
      )}
      <span className="min-w-0 flex-1 truncate text-[11px] leading-tight">{label}</span>
      <RemoteInventoryActivityGlyph
        state={activity}
        testId={`${testId}-${activity}-indicator`}
        className="justify-center"
      />
    </button>
  );
};
