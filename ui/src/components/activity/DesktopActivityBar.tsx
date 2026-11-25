import { Activity, Bot, FolderGit2, Globe, Monitor, PanelLeft, PanelLeftClose, Settings2, type LucideIcon } from "lucide-react";
import { useRef, type KeyboardEvent } from "react";

import { cn } from "../../lib/cn";
import type { ActivityMode } from "../../lib/sidebarPersistence";

/**
 * An attention badge on one rail mode: a count (supervisor decisions, agent-controlled tabs,
 * connected machines) or a bare dot when the mode only needs to say "something happened".
 */
export type ActivityBadge = number | "dot" | null | undefined;

export type DesktopActivityBarProps = {
  /** The selected mode. The rail never hides: it is the shell's top-level navigation. */
  mode: ActivityMode;
  onModeChange: (mode: ActivityMode) => void;
  /** Supervisor decisions waiting on the owner (`supervisorDecisionCount` in the shell). */
  supervisorBadge?: ActivityBadge;
  /** Lan worktrees whose agent finished or needs input. */
  worktreesBadge?: ActivityBadge;
  /** Browser tabs an agent controls or handed back. */
  browserBadge?: ActivityBadge;
  /** Remote Desktop sessions currently connected. */
  remoteDesktopBadge?: ActivityBadge;
  /** Drawer state, so the rail's toggle names the action it performs. */
  sidebarOpen?: boolean;
  onToggleSidebar?: () => void;
  onOpenResources?: () => void;
  onOpenSettings?: () => void;
  /** macOS draws its traffic lights inside the top-left 72px square; the rail is narrower than
   * that square, so it yields the whole band instead of sliding buttons underneath the lights.
   */
  isMac?: boolean;
  /**
   * Test id of the Supervisor mode's badge. The shell keeps `collapsed-attention-badge` so the
   * attention count reads the same whether the drawer is open or collapsed.
   */
  supervisorBadgeTestId?: string;
  className?: string;
};

/** The rail order. Fixed, because the modes are physical positions a user learns. */
export const ACTIVITY_BAR_MODES: readonly ActivityMode[] = ["supervisor", "worktrees", "browser", "remoteDesktop"];

/** macOS traffic lights occupy the top-left 72px of the window; nothing interactive may go there. */
export const MAC_TRAFFIC_LIGHT_CLEARANCE = 72;

export const ACTIVITY_BAR_WIDTH = 48;

type ModeMeta = { readonly label: string; readonly icon: LucideIcon };

export const ACTIVITY_MODE_META: Record<ActivityMode, ModeMeta> = {
  supervisor: { label: "Supervisor", icon: Bot },
  worktrees: { label: "Worktrees", icon: FolderGit2 },
  browser: { label: "Browser", icon: Globe },
  remoteDesktop: { label: "Remote Desktop", icon: Monitor },
};

/** The digits a count badge shows, or null when the badge is absent. */
function badgeText(badge: ActivityBadge): string | null {
  if (badge === "dot") return null;
  if (typeof badge !== "number" || !Number.isFinite(badge) || badge <= 0) return null;
  const count = Math.floor(badge);
  return count > 99 ? "99+" : String(count);
}

function hasDot(badge: ActivityBadge): boolean {
  return badge === "dot";
}

/** What a badge means, announced through `aria-describedby` so the mode name stays exact. */
function badgeDescription(badge: ActivityBadge): string | null {
  const count = badgeText(badge);
  if (count) return `${count} waiting`;
  return hasDot(badge) ? "Attention needed" : null;
}

function RailBadge({ badge, testId }: { readonly badge: ActivityBadge; readonly testId?: string }) {
  const count = badgeText(badge);
  if (count === null && !hasDot(badge)) return null;
  return (
    <span
      {...(testId ? { "data-testid": testId } : {})}
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute flex items-center justify-center rounded-full bg-status-warning font-semibold leading-none text-black",
        count === null ? "right-1 top-1 size-1.5" : "right-0.5 top-0.5 h-3.5 min-w-3.5 px-1 text-[9px]",
      )}
    >
      {count}
    </span>
  );
}

/**
 * The desktop shell's fixed left rail: four activity modes, then Resources, Settings and the
 * drawer toggle. It sits OUTSIDE `SidebarView` so the remote/mobile sidebar keeps its own tree,
 * and it stays visible when `Cmd+B` hides the drawer next to it.
 */
export function DesktopActivityBar({
  mode,
  onModeChange,
  supervisorBadge,
  worktreesBadge,
  browserBadge,
  remoteDesktopBadge,
  sidebarOpen = true,
  onToggleSidebar,
  onOpenResources,
  onOpenSettings,
  isMac = false,
  supervisorBadgeTestId,
  className,
}: DesktopActivityBarProps) {
  const tabRefs = useRef(new Map<ActivityMode, HTMLButtonElement>());

  const badges: Record<ActivityMode, ActivityBadge> = {
    supervisor: supervisorBadge,
    worktrees: worktreesBadge,
    browser: browserBadge,
    remoteDesktop: remoteDesktopBadge,
  };

  // Automatic activation: an arrow key both moves focus and selects, which is the WAI-ARIA
  // pattern for a tablist whose panels are always mounted.
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>, index: number) => {
    let nextIndex: number | null = null;
    if (event.key === "ArrowDown" || event.key === "ArrowRight") nextIndex = (index + 1) % ACTIVITY_BAR_MODES.length;
    else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
      nextIndex = (index - 1 + ACTIVITY_BAR_MODES.length) % ACTIVITY_BAR_MODES.length;
    } else if (event.key === "Home") nextIndex = 0;
    else if (event.key === "End") nextIndex = ACTIVITY_BAR_MODES.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    const next = ACTIVITY_BAR_MODES[nextIndex]!;
    onModeChange(next);
    tabRefs.current.get(next)?.focus();
  };

  const activate = (next: ActivityMode) => {
    // Always reported: the shell reopens a collapsed drawer when the already-active mode is
    // clicked, which is how the Supervisor badge reveals its panel.
    onModeChange(next);
  };

  return (
    <aside
      data-testid="desktop-activity-bar"
      data-tauri-drag-region
      className={cn(
        "relative flex h-full shrink-0 flex-col items-center border-r border-border bg-card text-muted-foreground",
        className,
      )}
      style={{ width: `${ACTIVITY_BAR_WIDTH}px`, minWidth: `${ACTIVITY_BAR_WIDTH}px` }}
    >
      {isMac ? (
        // The traffic lights sit above the web view; the pad reserves their band and keeps the
        // window draggable instead of putting a mode button under the close button.
        <div
          data-testid="titlebar-traffic-light-pad"
          data-tauri-drag-region
          aria-hidden="true"
          className="w-full shrink-0"
          style={{ height: `${MAC_TRAFFIC_LIGHT_CLEARANCE}px` }}
        />
      ) : null}

      <div
        role="tablist"
        aria-label="Activity"
        aria-orientation="vertical"
        className="flex w-full flex-col items-center gap-1 px-1.5 py-1"
      >
        {ACTIVITY_BAR_MODES.map((entry, index) => {
          const { label, icon: Icon } = ACTIVITY_MODE_META[entry];
          const selected = entry === mode;
          const badge = badges[entry];
          const description = badgeDescription(badge);
          return (
            <button
              key={entry}
              ref={(element) => {
                if (element) tabRefs.current.set(entry, element);
                else tabRefs.current.delete(entry);
              }}
              type="button"
              role="tab"
              id={`activity-tab-${entry}`}
              data-testid={`activity-mode-${entry}`}
              data-mode={entry}
              aria-selected={selected}
              aria-label={label}
              aria-describedby={description ? `activity-badge-${entry}` : undefined}
              title={description ? `${label} - ${description}` : label}
              tabIndex={selected ? 0 : -1}
              onClick={() => activate(entry)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  activate(entry);
                  return;
                }
                handleKeyDown(event, index);
              }}
              className={cn(
                "no-drag relative inline-flex size-9 items-center justify-center rounded-md transition-colors",
                "hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                selected ? "bg-accent text-foreground" : "text-muted-foreground",
              )}
            >
              <Icon className="size-4" aria-hidden="true" />
              <RailBadge
                badge={badge}
                testId={entry === "supervisor" ? supervisorBadgeTestId ?? `activity-mode-${entry}-badge` : `activity-mode-${entry}-badge`}
              />
              {description ? (
                <span id={`activity-badge-${entry}`} className="sr-only">{description}</span>
              ) : null}
            </button>
          );
        })}
      </div>

      <div className="mt-auto flex w-full flex-col items-center gap-1 px-1.5 pb-2 pt-1">
        {onOpenResources ? (
          <button
            type="button"
            aria-label="System resources"
            title="System resources"
            data-testid="activity-resources"
            onClick={onOpenResources}
            className="no-drag inline-flex size-9 items-center justify-center rounded-md transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <Activity className="size-4" aria-hidden="true" />
          </button>
        ) : null}
        {onOpenSettings ? (
          <button
            type="button"
            aria-label="Settings"
            title="Settings"
            data-testid="activity-settings"
            onClick={onOpenSettings}
            className="no-drag inline-flex size-9 items-center justify-center rounded-md transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <Settings2 className="size-4" aria-hidden="true" />
          </button>
        ) : null}
        {onToggleSidebar ? (
          <button
            type="button"
            data-shortcut="sidebar.left.toggle"
            aria-label={sidebarOpen ? "Collapse sidebar" : "Show sidebar"}
            title={sidebarOpen ? "Collapse sidebar" : "Show sidebar"}
            aria-expanded={sidebarOpen}
            data-testid="activity-drawer-toggle"
            onClick={onToggleSidebar}
            className="no-drag inline-flex size-9 items-center justify-center rounded-md transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            {sidebarOpen ? <PanelLeftClose className="size-4" aria-hidden="true" /> : <PanelLeft className="size-4" aria-hidden="true" />}
          </button>
        ) : null}
      </div>
    </aside>
  );
}
