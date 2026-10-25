import { Eye } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";

import { cn } from "../../lib/cn";
import type { SidebarRowLive, SidebarWorktreeNode } from "./sidebarModel";
import { SidebarStateSlot } from "./SidebarStateSlot";

/** `compact` is the desktop sidebar; `touch` is the phone drawer (44px rows, 14px labels). */
export type SidebarDensity = "compact" | "touch";

export type SidebarWorktreeRowProps = {
  readonly node: Pick<SidebarWorktreeNode, "path" | "label" | "hostLabel" | "stale">;
  readonly live: SidebarRowLive;
  readonly density?: SidebarDensity;
  readonly onSelect?: () => void;
  /** Dims the row and marks it aria-disabled; the select handler still runs so it can explain why. */
  readonly disabled?: boolean;
  readonly title?: string;
  readonly slotTitle?: string;
  readonly shortcutWorkspaceId?: string;
  readonly onContextMenu?: (event: MouseEvent) => void;
  /** Hover actions on the right edge; undefined renders none and reserves no space. */
  readonly trailing?: ReactNode;
};

/** One worktree row of the sidebar tree (ui/DESIGN.md section 2), shared by desktop and remote. */
export function SidebarWorktreeRow({
  node,
  live,
  density = "compact",
  onSelect,
  disabled = false,
  title,
  slotTitle,
  shortcutWorkspaceId,
  onContextMenu,
  trailing,
}: SidebarWorktreeRowProps) {
  const touch = density === "touch";
  return (
    <div
      data-stale={node.stale ? "true" : undefined}
      data-disabled={disabled ? "true" : undefined}
      onContextMenu={onContextMenu}
      className={cn(
        "group/worktree-row relative my-0.5 w-full rounded-md transition-colors",
        disabled && "opacity-60",
        live.active ? "bg-worktree-sidebar-accent text-foreground" : "bg-transparent hover:bg-white/[0.04]",
      )}
    >
      {live.active ? (
        <span
          aria-hidden="true"
          data-testid="worktree-rail-active"
          className="pointer-events-none absolute -left-[9px] top-1 bottom-1 w-px bg-worktree-sidebar-ring"
        />
      ) : null}
      <button
        type="button"
        onClick={onSelect}
        data-shortcut-worktree-path={node.path}
        data-shortcut-workspace-id={shortcutWorkspaceId}
        aria-current={live.active ? "true" : undefined}
        aria-disabled={disabled ? "true" : undefined}
        title={title}
        className={cn(
          "flex w-full items-center gap-1.5 rounded-md px-2 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring aria-disabled:cursor-not-allowed",
          touch ? "min-h-11" : "h-7",
          trailing === undefined ? null : "pr-8",
        )}
      >
        <span
          className={cn(
            "min-w-0 truncate font-semibold leading-tight",
            touch ? "text-[14px]" : "text-[12px]",
            live.active ? "text-foreground" : "text-worktree-sidebar-foreground",
          )}
        >
          {node.label}
        </span>
        {live.supervisorTyping ? (
          <span data-testid="worktree-supervisor-slot" className="ml-auto inline-flex min-w-0 shrink-0 items-center gap-1 truncate text-[10px] leading-none text-foreground">
            <Eye aria-hidden="true" className="size-2.5 shrink-0" />
            <span className="truncate">supervisor typing</span>
          </span>
        ) : (
          <SidebarStateSlot
            state={live.slot}
            host={node.hostLabel ?? undefined}
            title={slotTitle}
            className="ml-auto min-w-0 truncate"
            data-testid={live.slot ? "worktree-status-dot" : "remote-machine-badge"}
          />
        )}
      </button>
      {trailing === undefined ? null : (
        <div
          className={cn(
            "absolute right-1.5 top-1/2 flex -translate-y-1/2 items-center gap-0.5 transition-opacity",
            touch ? null : "opacity-0 focus-within:opacity-100 group-hover/worktree-row:opacity-100",
          )}
        >
          {trailing}
        </div>
      )}
    </div>
  );
}

/** The role=status box a project section shows instead of rows. */
export function SidebarEmptyStatus({ children }: { readonly children: ReactNode }) {
  return (
    <div role="status" className="px-2 py-1.5 text-[11px] leading-relaxed text-muted-foreground">
      {children}
    </div>
  );
}

/** Empty-section hint: says what is going on and offers the one action when there is one. */
export function SidebarWorktreesEmpty({ text, onCreateWorktree }: { readonly text: string; readonly onCreateWorktree?: () => void }) {
  return (
    <span className="flex items-center justify-between gap-2">
      <span>{text}</span>
      {onCreateWorktree ? (
        <button
          type="button"
          onClick={onCreateWorktree}
          className="shrink-0 rounded px-1.5 py-0.5 text-[11px] text-foreground hover:bg-white/[0.06] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          Add worktree
        </button>
      ) : null}
    </span>
  );
}
