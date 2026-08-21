import {
  ChevronRight,
  FolderGit2,
  FolderPlus,
  LayoutDashboard,
  PanelLeftClose,
  Plus,
  Search,
  Settings2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { combineActivitySummaries, type ActivitySummary } from "../lib/activity";
import { cn } from "../lib/cn";
import { isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";
import type { RegisteredProject } from "../lib/tauri";
import { worktreeIdentity, type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { SectionHeader } from "./ui/SectionHeader";
import { StatusDot } from "./ui/StatusDot";
import { WorktreeList } from "./WorktreeList";

export const SIDEBAR_WIDTH_STORAGE_KEY = "orca.sidebar.width";
export const SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY = "rorca.sidebar.collapsedProjects";
const DEFAULT_SIDEBAR_WIDTH = 236;
const MIN_SIDEBAR_WIDTH = 220;
const MAX_SIDEBAR_WIDTH = 420;

type SidebarProps = {
  open?: boolean;
  isMac?: boolean;
  projects?: RegisteredProject[];
  activeProjectId?: string;
  worktrees: Worktree[];
  agents: ActiveAgent[];
  activePath: string;
  statuses?: Record<string, DirtyState | undefined>;
  unreadWorktreePaths?: Record<string, boolean>;
  activityByWorktreePath?: Record<string, ActivitySummary | undefined>;
  onSelectProject?: (project: RegisteredProject) => void;
  onAddProject?: () => void;
  onSelectWorktree: (worktree: Worktree) => void;
  onCreateWorktree: () => void;
  onRefreshWorktreeStatus?: (worktree: Worktree) => void;
  onDeleteWorktree?: (worktree: Worktree) => void;
  onOpenCommandPalette: () => void;
  onOpenSettings?: () => void;
  onToggle?: () => void;
  onHide?: () => void;
};

export function Sidebar({
  open = true,
  isMac = isMacShortcutPlatform(),
  projects = [],
  activeProjectId,
  worktrees,
  agents,
  activePath,
  statuses = {},
  unreadWorktreePaths,
  activityByWorktreePath,
  onSelectProject = () => undefined,
  onAddProject = () => undefined,
  onSelectWorktree,
  onCreateWorktree,
  onRefreshWorktreeStatus = () => undefined,
  onDeleteWorktree = () => undefined,
  onOpenCommandPalette,
  onOpenSettings,
  onToggle,
  onHide,
}: SidebarProps) {
  const worktreeRegionRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [width, setWidth] = useState(loadSidebarWidth);
  const widthRef = useRef(width);
  widthRef.current = width;

  // Accordion state is stored as the set of *collapsed* projects, so each project keeps its own
  // open/closed state and toggling one never disturbs another. Projects the user has never
  // toggled default to collapsed unless they are the active project.
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => {
    const stored = loadCollapsedProjects();
    // Once the user has toggled anything the persisted set is authoritative; only a first run
    // (nothing persisted) falls back to "everything but the active project starts collapsed".
    return stored ?? seedCollapsedProjects(new Set<string>(), projects, activeProjectId);
  });
  const knownProjectsRef = useRef<Set<string>>(new Set(projects.map((project) => project.workspaceId)));

  useEffect(() => {
    // Projects registered after mount inherit the same default as the initial seed.
    const unseen = projects.filter((project) => !knownProjectsRef.current.has(project.workspaceId));
    if (unseen.length === 0) return;
    for (const project of unseen) knownProjectsRef.current.add(project.workspaceId);
    setCollapsedProjects((current) => seedCollapsedProjects(current, unseen, activeProjectId));
  }, [activeProjectId, projects]);

  const toggleProject = useCallback((workspaceId: string) => {
    setCollapsedProjects((current) => {
      const next = new Set(current);
      if (next.has(workspaceId)) next.delete(workspaceId);
      else next.add(workspaceId);
      persistCollapsedProjects(next);
      return next;
    });
  }, []);

  const worktreesByProject = useMemo(
    () => groupWorktreesByProject(worktrees, projects, activeProjectId),
    [activeProjectId, projects, worktrees],
  );

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

  if (!open) {
    return null;
  }

  return (
    <aside
      className="relative flex h-full shrink-0 flex-col overflow-hidden bg-worktree-sidebar text-worktree-sidebar-foreground"
      style={{ width: `${width}px`, minWidth: `${MIN_SIDEBAR_WIDTH}px`, maxWidth: `${MAX_SIDEBAR_WIDTH}px` }}
    >
      <div
        data-testid="sidebar-drag-region"
        data-tauri-drag-region
        className="drag-region flex h-titlebar shrink-0 items-center px-2"
      >
        {isMac ? <div data-testid="titlebar-traffic-light-pad" className="w-[72px] shrink-0" aria-hidden="true" /> : null}
        <IconButton
          label="Hide sidebar"
          className="no-drag"
          size="sm"
          onClick={onToggle ?? onHide}
        >
          <PanelLeftClose className="size-3.5" />
        </IconButton>
      </div>

      <div className="space-y-1 px-2 pb-1">
        <button
          type="button"
          onClick={focusWorktrees}
          className="flex h-7 w-full items-center gap-2 rounded-md bg-worktree-sidebar-accent px-2 text-left text-[13px] font-medium tracking-tight text-worktree-sidebar-accent-foreground"
        >
          <LayoutDashboard className="size-3.5" />
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
          title="Projects"
          count={projects.length}
          actions={
            <>
              <IconButton label="Add worktree" size="sm" onClick={onCreateWorktree}>
                <FolderPlus className="size-3.5" />
              </IconButton>
              <IconButton label="Add project" size="sm" onClick={onAddProject}>
                <Plus className="size-3.5" />
              </IconButton>
            </>
          }
        />

        <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-2 pb-2 scrollbar-sleek">
          {projects.length === 0 ? (
            <p className="px-2 py-3 text-[11px] leading-relaxed text-muted-foreground">
              No projects registered yet.
            </p>
          ) : null}

          {projects.map((project) => {
            const active = project.workspaceId === activeProjectId;
            const expanded = !collapsedProjects.has(project.workspaceId);
            const projectWorktrees = worktreesByProject.get(project.workspaceId) ?? [];
            const projectActivity = summarizeProjectActivity(
              projectWorktrees,
              activityByWorktreePath,
              unreadWorktreePaths,
            );
            const attentionState = projectActivity.hasWaiting
              ? "waiting"
              : projectActivity.hasUnread
                ? "unread"
                : null;

            return (
              <div key={project.workspaceId} className="pb-0.5">
                <div
                  className={cn(
                    "flex h-7 w-full items-center gap-0.5 rounded-md pr-1.5 transition-colors",
                    active
                      ? "bg-worktree-sidebar-accent font-medium text-worktree-sidebar-accent-foreground"
                      : "text-worktree-sidebar-foreground/65 hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground",
                  )}
                >
                  <button
                    type="button"
                    onClick={() => toggleProject(project.workspaceId)}
                    aria-expanded={expanded}
                    aria-label={`${expanded ? "Collapse" : "Expand"} ${project.workspaceId}`}
                    className="flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <ChevronRight
                      aria-hidden="true"
                      className={cn("size-3 shrink-0 transition-transform", expanded && "rotate-90")}
                    />
                  </button>
                  <button
                    type="button"
                    onClick={() => onSelectProject(project)}
                    aria-current={active ? "true" : undefined}
                    aria-label={project.workspaceId}
                    className="flex min-w-0 flex-1 items-center gap-1.5 rounded-sm py-1 text-left text-[12px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <FolderGit2 className="size-3.5 shrink-0" />
                    <span className="min-w-0 flex-1 truncate">{project.workspaceId}</span>
                    {projectActivity.runningCount > 0 ? (
                      <span
                        data-testid="project-running-badge"
                        className="shrink-0 rounded-full bg-status-working/12 px-1.5 py-px text-[9px] font-medium leading-none text-status-working"
                      >
                        {projectActivity.runningCount} running
                      </span>
                    ) : null}
                    {attentionState ? (
                      <span
                        data-testid="project-attention-indicator"
                        data-attention-state={attentionState}
                        title={attentionState === "waiting" ? "Agent needs attention" : "Unread activity"}
                        className="inline-flex size-3 shrink-0 items-center justify-center"
                      >
                        <StatusDot state={attentionState} />
                      </span>
                    ) : null}
                  </button>
                </div>

                {expanded ? (
                  <div className="pl-3 pr-0.5 pt-0.5">
                    <WorktreeList
                      worktrees={projectWorktrees}
                      agents={agents}
                      activePath={active ? activePath : ""}
                      statuses={statuses}
                      unreadWorktreePaths={unreadWorktreePaths}
                      activityByWorktreePath={activityByWorktreePath}
                      onSelect={onSelectWorktree}
                      onCreate={onCreateWorktree}
                      onRefreshStatus={onRefreshWorktreeStatus}
                      onDelete={onDeleteWorktree}
                      label={`${project.workspaceId} worktrees`}
                    />
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>

      <div className="flex shrink-0 items-center justify-between border-t border-worktree-sidebar-border px-2 py-1.5">
        <div className="flex items-center gap-2 px-1 text-[10px] text-muted-foreground">
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
        className="no-drag absolute inset-y-0 right-0 z-30 w-1.5 cursor-col-resize touch-none"
      >
        <span className="pointer-events-none absolute inset-y-0 right-0 w-px bg-worktree-sidebar-border transition-colors group-hover:bg-ring/45" />
      </div>
    </aside>
  );
}

function summarizeProjectActivity(
  worktrees: Worktree[],
  activityByWorktreePath: Record<string, ActivitySummary | undefined> | undefined,
  unreadWorktreePaths: Record<string, boolean> | undefined,
): ActivitySummary {
  const summaries = worktrees
    .map((worktree) => activityByWorktreePath?.[worktree.path])
    .filter((summary): summary is ActivitySummary => Boolean(summary));
  const hasUnread = worktrees.some((worktree) => Boolean(unreadWorktreePaths?.[worktree.path]));
  return combineActivitySummaries(summaries, hasUnread);
}

/**
 * Worktrees carry their owning project inside their `orca/<wsId>/<slug>` branch name. Anything
 * without a matching registered project belongs to the active project, which is the only project
 * whose worktrees the workspace store actually holds.
 */
function groupWorktreesByProject(
  worktrees: Worktree[],
  projects: RegisteredProject[],
  activeProjectId: string | undefined,
) {
  const grouped = new Map<string, Worktree[]>();
  for (const project of projects) grouped.set(project.workspaceId, []);

  for (const worktree of worktrees) {
    const ownerId = worktreeIdentity(worktree)?.wsId;
    const owner = ownerId && grouped.has(ownerId) ? ownerId : activeProjectId;
    if (!owner) continue;
    const bucket = grouped.get(owner);
    if (bucket) bucket.push(worktree);
  }

  return grouped;
}

function seedCollapsedProjects(
  collapsed: Set<string>,
  projects: RegisteredProject[],
  activeProjectId: string | undefined,
) {
  const seeded = projects.filter(
    (project) => project.workspaceId !== activeProjectId && !collapsed.has(project.workspaceId),
  );
  if (seeded.length === 0) return collapsed;
  const next = new Set(collapsed);
  for (const project of seeded) next.add(project.workspaceId);
  return next;
}

/** Returns `null` when the accordion has never been persisted, so callers can apply defaults. */
function loadCollapsedProjects(): Set<string> | null {
  try {
    const raw = window.localStorage.getItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return null;
    return new Set(parsed.filter((id): id is string => typeof id === "string"));
  } catch {
    return null;
  }
}

function persistCollapsedProjects(collapsed: Set<string>) {
  try {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([...collapsed]));
  } catch {
    // A storage failure should not break accordion toggling for this session.
  }
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
