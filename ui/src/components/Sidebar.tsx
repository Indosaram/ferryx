import {
  ChevronRight,
  FolderGit2,
  FolderPlus,
  PanelLeftClose,
  Plus,
  Settings2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { combineActivitySummaries, type ActivitySummary } from "../lib/activity";
import { cn } from "../lib/cn";
import { resolveWorktreeOwnerId } from "../lib/worktreeOwnership";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import {
  getMigratedItem,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
} from "../lib/storageKeys";
import type { RegisteredProject } from "../lib/tauri";
import { type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { StatusDot } from "./ui/StatusDot";
import { WorktreeList } from "./WorktreeList";

export { SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, SIDEBAR_WIDTH_STORAGE_KEY };
const DEFAULT_SIDEBAR_WIDTH = 236;
const MIN_SIDEBAR_WIDTH = 220;
const MAX_SIDEBAR_WIDTH = 420;

type SidebarProps = {
  open?: boolean;
  isMac?: boolean;
  projects?: RegisteredProject[];
  activeProjectId?: string;
  worktrees: Worktree[];
  inactiveProjectWorktrees?: Record<string, Worktree[]>;
  agents: ActiveAgent[];
  activePath: string;
  statuses?: Record<string, DirtyState | undefined>;
  unreadWorktreePaths?: Record<string, boolean>;
  activityByWorktreePath?: Record<string, ActivitySummary | undefined>;
  onSelectProject?: (project: RegisteredProject) => void;
  onAddProject?: () => void;
  onSelectWorktree: (worktree: Worktree) => void;
  onCreateWorktree: (project?: RegisteredProject) => void;
  onDeleteWorktree?: (worktree: Worktree) => void;
  onOpenCommandPalette?: () => void;
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
  inactiveProjectWorktrees,
  agents,
  activePath,
  statuses = {},
  unreadWorktreePaths,
  activityByWorktreePath,
  onSelectProject = () => undefined,
  onAddProject = () => undefined,
  onSelectWorktree,
  onCreateWorktree,
  onDeleteWorktree = () => undefined,
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
  const knownProjectsRef = useRef<Set<string> | null>(null);
  if (knownProjectsRef.current === null) {
    knownProjectsRef.current = new Set(projects.map((project) => project.workspaceId));
  }

  useEffect(() => {
    if (knownProjectsRef.current === null) {
      knownProjectsRef.current = new Set(projects.map((project) => project.workspaceId));
    }
    // Projects registered after mount inherit the same default as the initial seed.
    const known = knownProjectsRef.current;
    const unseen = projects.filter((project) => !known.has(project.workspaceId));
    if (unseen.length === 0) return;
    for (const project of unseen) known.add(project.workspaceId);
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
    () => groupWorktreesByProject(worktrees, projects, activeProjectId, inactiveProjectWorktrees),
    [activeProjectId, inactiveProjectWorktrees, projects, worktrees],
  );

  // The active row is highlighted in whichever project's group actually renders
  // it, so a nested project sharing the path prefix does not light up too.
  // Only the active project can own the active row. Matching by path alone lit
  // up an inactive project whose list still contained that path.
  const activeWorktreeOwnerId = useMemo(() => {
    if (!activePath || !activeProjectId) return undefined;
    const activeRows = worktreesByProject.get(activeProjectId);
    return activeRows?.some((row) => row.path === activePath) ? activeProjectId : undefined;
  }, [activePath, activeProjectId, worktreesByProject]);

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
        <IconButton label="Add project" className="no-drag" size="sm" onClick={onAddProject}>
          <Plus className="size-3.5" />
        </IconButton>
      </div>

      <div ref={worktreeRegionRef} tabIndex={-1} data-testid="worktree-region" className="flex min-h-0 flex-1 flex-col outline-none">
        <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-2 pt-2 pb-2 scrollbar-sleek">
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
                  className="group/project flex h-7 w-full items-center gap-0.5 rounded-md pr-1 text-worktree-sidebar-foreground/65 transition-colors hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground"
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
                    onClick={() => {
                      onSelectProject(project);
                      toggleProject(project.workspaceId);
                    }}
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
                  {project.gitRoot !== null ? (
                    <IconButton
                      label={`Add worktree to ${project.workspaceId}`}
                      size="sm"
                      className="size-5 opacity-55 transition-opacity focus-visible:opacity-100 group-hover/project:opacity-100"
                      onClick={() => onCreateWorktree(project)}
                    >
                      <FolderPlus className="size-3" />
                    </IconButton>
                  ) : null}
                </div>

                {expanded ? (
                  <div className="pl-3 pr-0.5 pt-0.5">
                    <WorktreeList
                      worktrees={projectWorktrees}
                      agents={agents}
                      activePath={activeWorktreeOwnerId === project.workspaceId ? activePath : ""}
                      statuses={statuses}
                      unreadWorktreePaths={unreadWorktreePaths}
                      activityByWorktreePath={activityByWorktreePath}
                      onSelect={onSelectWorktree}
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

      <div className="flex shrink-0 items-center justify-end border-t border-worktree-sidebar-border px-2 py-1.5">
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
 * without that identity is attributed by path — a project root, or a path nested under one — so a
 * plain-folder project's root row never lands under whichever project happens to be active.
 */
function groupWorktreesByProject(
  worktrees: Worktree[],
  projects: RegisteredProject[],
  activeProjectId: string | undefined,
  inactiveProjectWorktrees?: Record<string, Worktree[]>,
) {
  const grouped = new Map<string, Worktree[]>();
  for (const project of projects) {
    const listed = project.workspaceId === activeProjectId ? [] : inactiveProjectWorktrees?.[project.workspaceId];
    grouped.set(project.workspaceId, listed ? [...listed] : []);
  }

  for (const worktree of worktrees) {
    const owner = resolveWorktreeOwnerId(worktree, projects, activeProjectId);
    if (!owner) continue;
    const bucket = grouped.get(owner);
    if (!bucket || bucket.some((candidate) => candidate.path === worktree.path)) continue;
    bucket.push(worktree);
  }

  if (activeProjectId) {
    const activeBucket = grouped.get(activeProjectId);
    if (activeBucket && activeBucket.length === 0) {
      const cached = inactiveProjectWorktrees?.[activeProjectId];
      if (cached && cached.length > 0) {
        for (const row of cached) {
          if (!activeBucket.some((candidate) => candidate.path === row.path)) {
            activeBucket.push(row);
          }
        }
      }
    }
  }

  // A non-Git project has no git worktrees at all, so without a synthesized
  // folder row its group would render empty and the folder would be
  // unselectable - including while it is the active project.
  for (const project of projects) {
    if (project.gitRoot !== null) continue;
    const bucket = grouped.get(project.workspaceId);
    if (!bucket || bucket.length > 0) continue;
    bucket.push({
      path: project.repoRoot,
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    });
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
    const raw = getMigratedItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY);
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
    const raw = getMigratedItem(SIDEBAR_WIDTH_STORAGE_KEY);
    const stored = Number(raw);
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
