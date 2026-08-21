import { FolderPlus, LayoutDashboard, PanelLeftClose, Search, Settings2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { shortcutLabel } from "../lib/shortcuts";
import type { ActiveAgent, DirtyState, Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { SectionHeader } from "./ui/SectionHeader";
import { WorktreeList } from "./WorktreeList";

export const SIDEBAR_WIDTH_STORAGE_KEY = "orca.sidebar.width";
const DEFAULT_SIDEBAR_WIDTH = 264;
const MIN_SIDEBAR_WIDTH = 220;
const MAX_SIDEBAR_WIDTH = 420;

type SidebarProps = {
  worktrees: Worktree[];
  agents: ActiveAgent[];
  activePath: string;
  statuses?: Record<string, DirtyState | undefined>;
  onSelectWorktree: (worktree: Worktree) => void;
  onCreateWorktree: () => void;
  onRefreshWorktreeStatus?: (worktree: Worktree) => void;
  onDeleteWorktree?: (worktree: Worktree) => void;
  onOpenCommandPalette: () => void;
  onOpenSettings?: () => void;
};

export function Sidebar({
  worktrees,
  agents,
  activePath,
  statuses = {},
  onSelectWorktree,
  onCreateWorktree,
  onRefreshWorktreeStatus = () => undefined,
  onDeleteWorktree = () => undefined,
  onOpenCommandPalette,
  onOpenSettings,
}: SidebarProps) {
  const worktreeRegionRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [width, setWidth] = useState(loadSidebarWidth);
  const widthRef = useRef(width);
  widthRef.current = width;

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const nextWidth = clampSidebarWidth(drag.startWidth + event.clientX - drag.startX);
      widthRef.current = nextWidth;
      setWidth(nextWidth);
    };

    const finishDrag = () => {
      if (!dragRef.current) return;
      dragRef.current = null;
      persistSidebarWidth(widthRef.current);
      document.body.style.cursor = "";
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", finishDrag);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", finishDrag);
      window.removeEventListener("pointercancel", finishDrag);
      document.body.style.cursor = "";
    };
  }, []);

  const focusWorktrees = () => {
    worktreeRegionRef.current?.focus();
    worktreeRegionRef.current?.scrollIntoView?.({ block: "start" });
  };

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragRef.current = { startX: event.clientX, startWidth: widthRef.current };
    document.body.style.cursor = "col-resize";
  };

  return (
    <aside
      className="relative flex h-full shrink-0 flex-col overflow-hidden bg-worktree-sidebar text-worktree-sidebar-foreground"
      style={{ width: `${width}px`, minWidth: `${MIN_SIDEBAR_WIDTH}px`, maxWidth: `${MAX_SIDEBAR_WIDTH}px` }}
    >
      <div className="drag-region flex h-titlebar shrink-0 items-center justify-end px-2 pt-1">
        <IconButton label="Hide sidebar" className="no-drag" size="sm">
          <PanelLeftClose className="size-3.5" />
        </IconButton>
      </div>

      <div className="space-y-1 px-2 pb-1">
        <button
          type="button"
          onClick={focusWorktrees}
          className="flex w-full items-center gap-2 rounded-md bg-worktree-sidebar-accent px-2 py-1.5 text-left text-[13px] font-medium tracking-tight text-worktree-sidebar-accent-foreground"
        >
          <LayoutDashboard className="size-4" />
          <span className="flex-1">Workspace</span>
        </button>
        <button
          type="button"
          onClick={onOpenCommandPalette}
          className="group relative flex h-7 w-full items-center rounded-md border border-worktree-sidebar-border/70 bg-worktree-sidebar-foreground/5 pl-7 pr-1.5 text-left text-[12px] font-medium tracking-tight text-worktree-sidebar-foreground/45 transition-colors hover:border-worktree-sidebar-border hover:bg-worktree-sidebar-foreground/8"
        >
          <Search className="pointer-events-none absolute left-2 top-1/2 size-3 -translate-y-1/2 text-worktree-sidebar-foreground/30" />
          Search workspaces
          <kbd className="ml-auto rounded border border-worktree-sidebar-border/80 bg-worktree-sidebar-foreground/8 px-1 py-px text-[9px] text-worktree-sidebar-foreground/55">
            {shortcutLabel("commandPalette.open")}
          </kbd>
        </button>
      </div>

      <div ref={worktreeRegionRef} tabIndex={-1} data-testid="worktree-region" className="flex min-h-0 flex-1 flex-col outline-none">
        <SectionHeader
          title="Worktrees"
          count={worktrees.length}
          actions={
            <IconButton label="Add worktree" size="sm" onClick={onCreateWorktree}>
              <FolderPlus className="size-3.5" />
            </IconButton>
          }
        />
        <WorktreeList
          worktrees={worktrees}
          agents={agents}
          activePath={activePath}
          statuses={statuses}
          onSelect={onSelectWorktree}
          onCreate={onCreateWorktree}
          onRefreshStatus={onRefreshWorktreeStatus}
          onDelete={onDeleteWorktree}
        />
      </div>

      <div className="flex shrink-0 items-center justify-between border-t border-worktree-sidebar-border px-2 py-1.5">
        <div className="flex items-center gap-2 px-1 text-[11px] text-muted-foreground">
          <span className="size-1.5 rounded-full bg-status-success" />
          Local runtime
        </div>
        <IconButton label="Settings" size="sm" onClick={onOpenSettings}>
          <Settings2 className="size-3.5" />
        </IconButton>
      </div>

      <div
        role="separator"
        aria-label="Resize sidebar"
        aria-orientation="vertical"
        aria-valuemin={MIN_SIDEBAR_WIDTH}
        aria-valuemax={MAX_SIDEBAR_WIDTH}
        aria-valuenow={width}
        onPointerDown={startResize}
        className="absolute inset-y-0 right-0 z-30 w-1.5 cursor-col-resize touch-none transition-colors hover:bg-ring/45"
      />
    </aside>
  );
}

function loadSidebarWidth() {
  try {
    const stored = Number(window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY));
    return Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored) : DEFAULT_SIDEBAR_WIDTH;
  } catch {
    return DEFAULT_SIDEBAR_WIDTH;
  }
}

function persistSidebarWidth(width: number) {
  try {
    window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(clampSidebarWidth(width)));
  } catch {
    // A storage failure should not break pointer resizing for this session.
  }
}

function clampSidebarWidth(width: number) {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, Math.round(width)));
}
