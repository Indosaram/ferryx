import { Eye, LoaderCircle } from "lucide-react";
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
  /** Touch only: a visible reason shown in the slot (e.g. "Opening…", "Out of date"). */
  readonly statusText?: string;
  /** Touch only: the row is the one being opened; it shows a spinner and stays undimmed. */
  readonly busy?: boolean;
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
  statusText,
  busy = false,
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
        disabled && !(touch && (busy || statusText)) && "opacity-60",
        touch
          ? live.active
            ? "bg-chat-surface-raised text-chat-foreground"
            : "bg-transparent hover:bg-chat-row-hover"
          : live.active
            ? "bg-worktree-sidebar-accent text-foreground"
            : "bg-transparent hover:bg-white/[0.04]",
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
            touch ? "text-sm" : "text-[12px]",
            touch ? "text-chat-foreground" : live.active ? "text-foreground" : "text-worktree-sidebar-foreground",
          )}
        >
          {node.label}
        </span>
        {touch && (busy || statusText) ? (
          <span data-testid="worktree-row-reason" className="ml-auto inline-flex min-w-0 shrink-0 items-center gap-1 truncate text-xs text-chat-foreground-secondary">
            {busy ? <LoaderCircle aria-hidden="true" className="size-3.5 shrink-0 animate-spin motion-reduce:animate-none" /> : null}
            <span className="truncate">{busy ? "Opening…" : statusText}</span>
          </span>
        ) : live.supervisorTyping ? (
          <span
            data-testid="worktree-supervisor-slot"
            className={cn("ml-auto inline-flex min-w-0 shrink-0 items-center gap-1 truncate leading-none text-foreground", touch ? "text-xs" : "text-[10px]")}
          >
            <Eye aria-hidden="true" className={cn(touch ? "size-3.5" : "size-2.5", "shrink-0")} />
            <span className="truncate">{touch ? "Supervisor typing" : "supervisor typing"}</span>
          </span>
        ) : (
          <SidebarStateSlot
            state={live.slot}
            host={node.hostLabel ?? undefined}
            title={slotTitle}
            className="ml-auto min-w-0 truncate"
            density={density}
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
export function SidebarEmptyStatus({ children, density = "compact" }: { readonly children: ReactNode; readonly density?: SidebarDensity }) {
  return (
    <div role="status" className={cn("px-2 py-1.5 leading-relaxed text-muted-foreground", density === "touch" ? "text-sm" : "text-[11px]")}>
      {children}
    </div>
  );
}

/** Empty-section hint: says what is going on and offers the one action when there is one. */
export function SidebarWorktreesEmpty({
  text,
  onCreateWorktree,
  density = "compact",
}: {
  readonly text: string;
  readonly onCreateWorktree?: () => void;
  readonly density?: SidebarDensity;
}) {
  const touch = density === "touch";
  return (
    <span className="flex items-center justify-between gap-2">
      <span>{text}</span>
      {onCreateWorktree ? (
        <button
          type="button"
          onClick={onCreateWorktree}
          className={cn(
            "shrink-0 rounded text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            touch ? "min-h-11 px-3 text-sm hover:bg-chat-row-hover" : "px-1.5 py-0.5 text-[11px] hover:bg-white/[0.06]",
          )}
        >
          Add worktree
        </button>
      ) : null}
    </span>
  );
}
