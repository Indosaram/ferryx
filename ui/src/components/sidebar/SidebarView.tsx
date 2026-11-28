import { Inbox, PanelLeftClose, Plus } from "lucide-react";
import { createContext, Fragment, type ReactNode } from "react";

import { cn } from "../../lib/cn";
import { IconButton } from "../ui/IconButton";
import { SectionHeader } from "../ui/SectionHeader";
import { SIDEBAR_EMPTY_TEXT } from "./sidebarLabels";
import { filterSidebarModel, type SidebarLive, type SidebarModel, type SidebarProjectNode, type SidebarRowLive, type SidebarWorktreeNode } from "./sidebarModel";
import { SidebarFilter } from "./SidebarFilter";
import { SidebarFooter } from "./SidebarFooter";
import { SidebarProjectHeader, type SidebarProjectHeaderProps } from "./SidebarProjectHeader";
import { SidebarViewTabs, type SidebarViewId } from "./SidebarViewTabs";
import { SidebarEmptyStatus, SidebarWorktreeRow, SidebarWorktreesEmpty, type SidebarDensity } from "./SidebarWorktreeRow";

const IDLE_ROW: SidebarRowLive = { active: false, slot: null, supervisorTyping: false };

/**
 * Lets a mounted supervisor panel inside `SidebarView` know whether the Supervisor tab is active
 * without coupling `SidebarView` to desktop-only host components. Defaults to `true` so standalone
 * renders (tests, mobile home) behave as visible.
 */
export const SidebarSupervisorVisibilityContext = createContext(true);

/** Every action is optional except selection and collapse; an undefined action hides its control. */
export type SidebarViewActions = {
  readonly selectWorktree: (project: SidebarProjectNode, node: SidebarWorktreeNode) => void;
  readonly toggleGroup: (groupId: string) => void;
  readonly selectProject?: (project: SidebarProjectNode) => void;
  readonly addProject?: () => void;
  readonly hideSidebar?: () => void;
  readonly openSettings?: () => void;
  readonly openResources?: () => void;
  readonly onViewChange?: (view: SidebarViewId) => void;
  /** Opens the Inbox popup: the decisions waiting on the owner, at the titlebar's right end. */
  readonly openInbox?: () => void;
  /** Decisions waiting on the owner; shown on the button so the popup is worth opening. */
  readonly inboxCount?: number;
};

/** Desktop-only drag-and-drop wrappers; the view renders plain sections when absent. */
export type SidebarReorder = {
  readonly projects: (children: ReactNode) => ReactNode;
  readonly section: (project: SidebarProjectNode, header: ReactNode, body: ReactNode) => ReactNode;
  readonly rows: (project: SidebarProjectNode, rows: readonly SidebarWorktreeNode[], children: ReactNode) => ReactNode;
};

export type SidebarViewProps = {
  readonly model: SidebarModel;
  readonly live: SidebarLive;
  /** The workspace whose project header reads as current. */
  readonly activeWorkspaceId?: string;
  readonly density?: SidebarDensity;
  readonly actions: SidebarViewActions;
  readonly filter: { readonly query: string; readonly onChange: (query: string) => void };
  /** Active tab; the tabs render only when the Supervisor panel is offered. */
  readonly view?: SidebarViewId;
  /** The Supervisor panel; the Supervisor tab shows whenever this is set. */
  readonly supervisor?: { readonly content: ReactNode };
  /** Leading titlebar content (desktop traffic-light pad). */
  readonly titlebarLeading?: ReactNode;
  readonly titlebarProps?: Readonly<Record<string, string | boolean | undefined>>;
  readonly shortcuts?: { readonly hideSidebar?: string; readonly addProject?: string; readonly settings?: string };
  /** True while the Inbox popup is open, so the button reads as expanded. */
  readonly inboxOpen?: boolean;
  /** Container-supplied header extras (menus, slot activation, add worktree). */
  readonly projectChrome?: (
    project: SidebarProjectNode,
    rows: readonly SidebarWorktreeNode[],
  ) => Partial<SidebarProjectHeaderProps> & { readonly onCreateWorktree?: () => void };
  /** Container-supplied row; the default is a plain SidebarWorktreeRow. */
  readonly renderRow?: (project: SidebarProjectNode, node: SidebarWorktreeNode, live: SidebarRowLive) => ReactNode;
  readonly reorder?: SidebarReorder;
};

/** The sidebar body both apps render: titlebar actions, view tabs, panel, filter, project tree, footer. */
export function SidebarView({
  model,
  live,
  activeWorkspaceId,
  density = "compact",
  actions,
  filter,
  view = "worktrees",
  supervisor,
  titlebarLeading,
  titlebarProps,
  shortcuts = {},
  inboxOpen = false,
  projectChrome,
  renderRow,
  reorder,
}: SidebarViewProps) {
  const touch = density === "touch";
  const activeView: SidebarViewId = view === "supervisor" && !supervisor ? "worktrees" : view;
  const worktreesHidden = activeView !== "worktrees";
  const collapsed = new Set(model.collapsedGroupIds);
  const entries = filterSidebarModel(model, filter.query);
  const showTitlebar = Boolean(titlebarLeading || actions.hideSidebar || actions.addProject || actions.openInbox);
  const inboxCount = actions.inboxCount ?? 0;

  const sections = entries.map(({ group: project, rows, forceExpanded }) => {
    const expanded = forceExpanded || !collapsed.has(project.groupId);
    const { onCreateWorktree, ...chrome } = projectChrome?.(project, rows) ?? {};
    const header = (
      <SidebarProjectHeader
        label={project.label}
        expanded={expanded}
        active={project.members.some((member) => member.workspaceId === activeWorkspaceId)}
        slot={live.projects[project.groupId] ?? { kind: "none" }}
        density={density}
        onToggle={() => actions.toggleGroup(project.groupId)}
        onSelect={() => {
          actions.selectProject?.(project);
          if (!expanded) actions.toggleGroup(project.groupId);
        }}
        {...chrome}
      />
    );
    const rowElements = rows.map((node) => {
      const rowLive = live.rows[node.key] ?? IDLE_ROW;
      return (
        <Fragment key={node.key}>
          {renderRow ? renderRow(project, node, rowLive) : (
            <div role="listitem">
              <SidebarWorktreeRow node={node} live={rowLive} density={density} onSelect={() => actions.selectWorktree(project, node)} />
            </div>
          )}
        </Fragment>
      );
    });
    const list = rows.length === 0 ? (
      <SidebarEmptyStatus density={density}>
        <SidebarWorktreesEmpty text={project.worktreesEmptyHint ?? SIDEBAR_EMPTY_TEXT.noWorktrees} onCreateWorktree={onCreateWorktree} density={density} />
      </SidebarEmptyStatus>
    ) : (
      <div role="list" aria-label={`${project.groupId} worktrees`} className="m-0 p-0">{rowElements}</div>
    );
    const body = expanded ? (
      <div
        className="ml-3 border-l border-worktree-sidebar-border pl-2 pr-0.5 pt-0.5"
        onPointerDown={(event) => event.stopPropagation()}
      >
        {reorder ? reorder.rows(project, rows, list) : list}
      </div>
    ) : null;
    return reorder ? (
      <Fragment key={project.groupId}>{reorder.section(project, header, body)}</Fragment>
    ) : (
      <div key={project.groupId} className="pb-0.5">{header}{body}</div>
    );
  });

  return (
    <>
      {showTitlebar ? (
        <div {...titlebarProps} className="flex h-titlebar shrink-0 items-center px-2 gap-0.5">
          {titlebarLeading}
          {actions.hideSidebar ? (
            <IconButton label="Hide sidebar" data-shortcut={shortcuts.hideSidebar} className="no-drag" size="sm" onClick={actions.hideSidebar}>
              <PanelLeftClose className="size-3.5" />
            </IconButton>
          ) : null}
          {actions.addProject ? (
            <IconButton data-shortcut={shortcuts.addProject} label="Add project" className="no-drag" size="sm" onClick={actions.addProject}>
              <Plus className="size-3.5" />
            </IconButton>
          ) : null}
          {actions.openInbox ? (
            <IconButton
              data-testid="inbox-open-button"
              label={inboxCount > 0 ? `Open inbox (${inboxCount})` : "Open inbox"}
              aria-haspopup="dialog"
              aria-expanded={inboxOpen || undefined}
              className="no-drag relative ml-auto"
              size="sm"
              onClick={actions.openInbox}
            >
              <Inbox className="size-3.5" />
              {inboxCount > 0 ? (
                <span
                  data-testid="inbox-badge"
                  className="absolute -right-0.5 -top-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-status-warning px-1 text-[9px] font-semibold leading-none text-black"
                >
                  {inboxCount > 9 ? "9+" : inboxCount}
                </span>
              ) : null}
            </IconButton>
          ) : null}
        </div>
      ) : null}

      {supervisor && actions.onViewChange ? <SidebarViewTabs view={activeView} onViewChange={actions.onViewChange} /> : null}

      {/* Kept mounted while Worktrees shows, so switching back is instant: no reload, no "Loading" flash. */}
      {supervisor ? (
        <div
          data-testid="supervisor-region"
          hidden={activeView !== "supervisor" || undefined}
          className={cn("min-h-0 flex-1 flex-col", activeView === "supervisor" ? "flex" : "hidden")}
        >
          <SidebarSupervisorVisibilityContext.Provider value={activeView === "supervisor"}>
            {supervisor.content}
          </SidebarSupervisorVisibilityContext.Provider>
        </div>
      ) : null}

      <div
        data-testid="worktree-region"
        hidden={worktreesHidden || undefined}
        className={cn("min-h-0 flex-1 flex-col", worktreesHidden ? "hidden" : "flex")}
      >
        {touch ? null : <SidebarFilter value={filter.query} onChange={filter.onChange} density={density} />}
        {touch ? (
          <h3 className="flex min-h-8 shrink-0 items-center gap-1.5 px-4 text-sm font-semibold text-chat-foreground">
            Projects
            <span className="text-xs font-normal tabular-nums text-chat-foreground-secondary">{model.projects.length}</span>
          </h3>
        ) : (
          <SectionHeader title="Projects" count={model.projects.length} className="shrink-0" />
        )}
        <div className={cn("min-h-0 flex-1 overflow-x-hidden px-2 pb-2", touch ? null : "overflow-y-auto scrollbar-sleek")}>
          {model.emptyState === "noProjects" ? (
            <p className={cn("px-2 py-3 leading-relaxed text-muted-foreground", touch ? "text-sm" : "text-[11px]")}>{SIDEBAR_EMPTY_TEXT.noProjects}</p>
          ) : null}
          {reorder ? reorder.projects(sections) : sections}
        </div>
      </div>

      <SidebarFooter
        density={density}
        summary={live.footer}
        onOpenResources={actions.openResources}
        onOpenSettings={actions.openSettings}
        settingsShortcut={shortcuts.settings}
      />
    </>
  );
}
