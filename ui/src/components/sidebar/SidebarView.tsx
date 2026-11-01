import { CheckCheck, PanelLeftClose, Plus } from "lucide-react";
import { Fragment, type ReactNode } from "react";

import { cn } from "../../lib/cn";
import { AttentionInbox } from "../../features/ferryx/attention/AttentionInbox";
import type { AttentionPrompts } from "../../features/ferryx/attention/attentionPrompts";
import type { AttentionRow } from "../../features/ferryx/attention/attentionModel";
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
};

/** Desktop-only drag-and-drop wrappers; the view renders plain sections when absent. */
export type SidebarReorder = {
  readonly projects: (children: ReactNode) => ReactNode;
  readonly section: (project: SidebarProjectNode, header: ReactNode, body: ReactNode) => ReactNode;
  readonly rows: (project: SidebarProjectNode, rows: readonly SidebarWorktreeNode[], children: ReactNode) => ReactNode;
};

export type SidebarInbox = {
  readonly rows: readonly AttentionRow[];
  readonly onOpen: (row: AttentionRow) => void;
  /** Absent when this surface may not dismiss (the remote showing a desktop's published rows). */
  readonly onDismiss?: (row: AttentionRow) => void;
  readonly onDismissAll?: () => void;
  /** Present where a needs-you row may answer its pane's prompt inline (the desktop). */
  readonly prompts?: AttentionPrompts;
  /** Working panes the Inbox lists only once the daemon reads a prompt on them (needs `prompts`). */
  readonly candidates?: readonly AttentionRow[];
};

export type SidebarViewProps = {
  readonly model: SidebarModel;
  readonly live: SidebarLive;
  /** The workspace whose project header reads as current. */
  readonly activeWorkspaceId?: string;
  readonly density?: SidebarDensity;
  readonly actions: SidebarViewActions;
  readonly filter: { readonly query: string; readonly onChange: (query: string) => void };
  /** Active tab; the tabs render only when an inbox is offered. */
  readonly view?: SidebarViewId;
  readonly inbox?: SidebarInbox;
  /** The Supervisor tab's content; the tab shows when this is set and advanced features are on. */
  readonly supervisor?: { readonly typing: boolean; readonly content: ReactNode };
  /** Leading titlebar content (desktop traffic-light pad). */
  readonly titlebarLeading?: ReactNode;
  readonly titlebarProps?: Readonly<Record<string, string | boolean | undefined>>;
  readonly shortcuts?: { readonly hideSidebar?: string; readonly addProject?: string; readonly settings?: string; readonly inbox?: string };
  /** Container-supplied header extras (menus, slot activation, add worktree). */
  readonly projectChrome?: (
    project: SidebarProjectNode,
    rows: readonly SidebarWorktreeNode[],
  ) => Partial<SidebarProjectHeaderProps> & { readonly onCreateWorktree?: () => void };
  /** Container-supplied row; the default is a plain SidebarWorktreeRow. */
  readonly renderRow?: (project: SidebarProjectNode, node: SidebarWorktreeNode, live: SidebarRowLive) => ReactNode;
  readonly reorder?: SidebarReorder;
};

/** The sidebar body both apps render: titlebar actions, view tabs, filter, project tree, footer. */
export function SidebarView({
  model,
  live,
  activeWorkspaceId,
  density = "compact",
  actions,
  filter,
  view = "worktrees",
  inbox,
  supervisor,
  titlebarLeading,
  titlebarProps,
  shortcuts = {},
  projectChrome,
  renderRow,
  reorder,
}: SidebarViewProps) {
  const touch = density === "touch";
  const showSupervisor = Boolean(supervisor) && model.advancedFeatures;
  const activeView: SidebarViewId = view === "supervisor" && !showSupervisor ? "worktrees" : view;
  const worktreesHidden = activeView !== "worktrees";
  const collapsed = new Set(model.collapsedGroupIds);
  const entries = filterSidebarModel(model, filter.query);
  const showTitlebar = Boolean(titlebarLeading || actions.hideSidebar || actions.addProject || inbox);

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
          {inbox ? (
            <div className="ml-auto flex items-center gap-0.5">
              {activeView === "inbox" && inbox.onDismissAll ? (
                <IconButton label="Mark all read" className="no-drag" size="sm" disabled={inbox.rows.length === 0} onClick={inbox.onDismissAll}>
                  <CheckCheck className="size-3.5" />
                </IconButton>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}

      {inbox && actions.onViewChange ? (
        <SidebarViewTabs
          view={activeView}
          onViewChange={actions.onViewChange}
          inboxCount={inbox.rows.length}
          supervisor={showSupervisor && supervisor ? { typing: supervisor.typing } : null}
          inboxShortcut={shortcuts.inbox}
        />
      ) : null}

      {activeView === "supervisor" && supervisor ? supervisor.content : null}

      {activeView === "inbox" && inbox ? <AttentionInbox rows={inbox.rows} onOpen={inbox.onOpen} {...(inbox.onDismiss ? { onDismiss: inbox.onDismiss } : {})} {...(inbox.prompts ? { prompts: inbox.prompts } : {})} {...(inbox.candidates ? { candidates: inbox.candidates } : {})} /> : null}

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
