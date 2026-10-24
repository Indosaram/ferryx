// The remote's sidebar: each machine's section drawn by the desktop's own SidebarView from the
// model that machine's desktop published. The remote adds only what the desktop cannot know:
// which groups this browser folded and which row is being attached right now.
import { useState, type ReactNode } from "react";

import { sidebarLiveState } from "../components/sidebar/sidebarModel";
import type { SidebarModel, SidebarProjectNode, SidebarWorktreeNode } from "../components/sidebar/sidebarModel";
import { SidebarView, type SidebarInbox } from "../components/sidebar/SidebarView";
import type { SidebarViewId } from "../components/sidebar/SidebarViewTabs";
import { SidebarWorktreeRow, type SidebarDensity } from "../components/sidebar/SidebarWorktreeRow";
import { cn } from "../lib/cn";
import type { RemoteSidebarSection } from "./useRemoteSidebarModel";

export const REMOTE_COLLAPSED_KEY_PREFIX = "ferryx.remote.sidebarCollapsed.";
export const NOT_SYNCED_CAPTION = "Not synced with desktop";

/** This browser's folded groups for one machine; never sent back to the desktop. */
function readCollapsed(machineId: string): string[] | null {
  let raw: string | null;
  try {
    raw = sessionStorage.getItem(REMOTE_COLLAPSED_KEY_PREFIX + machineId);
  } catch (err) {
    // Storage blocked by the browser (private mode): fall back to the desktop's set.
    if (err instanceof DOMException) return null;
    throw err;
  }
  if (raw === null) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) && parsed.every((id) => typeof id === "string") ? parsed : null;
  } catch (err) {
    if (err instanceof SyntaxError) return null;
    throw err;
  }
}

function writeCollapsed(machineId: string, ids: readonly string[]): void {
  try {
    sessionStorage.setItem(REMOTE_COLLAPSED_KEY_PREFIX + machineId, JSON.stringify(ids));
  } catch (err) {
    // A full or blocked store keeps the fold for this page view only.
    if (!(err instanceof DOMException)) throw err;
  }
}

export type RemoteSidebarTarget = {
  readonly machineId: string;
  readonly project: SidebarProjectNode;
  readonly node: SidebarWorktreeNode;
};

/** The worktree being attached; `machineId` null matches the only machine of a direct link. */
export type RemoteSidebarPending = {
  readonly machineId: string | null;
  readonly workspaceId: string;
  readonly worktreeSlug: string | null;
};

export type RemoteSidebarProps = {
  readonly sections: readonly RemoteSidebarSection[];
  readonly density: SidebarDensity;
  readonly pending: RemoteSidebarPending | null;
  readonly onSelect: (target: RemoteSidebarTarget) => void;
  readonly view: SidebarViewId;
  readonly onViewChange?: (view: SidebarViewId) => void;
  readonly inbox?: SidebarInbox;
  readonly supervisor?: { readonly typing: boolean; readonly content: ReactNode };
  /** Account inventory loading/offline rows, shown above the tree. */
  readonly status?: ReactNode;
};

function isPendingRow(pending: RemoteSidebarPending | null, machineId: string, node: SidebarWorktreeNode): boolean {
  return pending !== null &&
    (pending.machineId === null || pending.machineId === machineId) &&
    pending.workspaceId === node.workspaceId &&
    pending.worktreeSlug === node.worktreeSlug;
}

function SectionState({ section }: { readonly section: RemoteSidebarSection }) {
  if (section.state === "loading") {
    return <p role="status" className="px-3 py-2 text-[11px] text-muted-foreground">Loading worktrees…</p>;
  }
  if (!section.error) return null;
  const text = section.state === "offline" ? "Can't reach this computer right now." : `Worktrees unavailable: ${section.error.message}`;
  return (
    <p role="status" className="px-3 py-2 text-[11px] text-muted-foreground">
      {section.model ? `${text} Showing the last list.` : text}
    </p>
  );
}

/**
 * A reachable machine with a list takes the remaining height. An offline machine (even one still
 * showing its last list) or one with nothing to list yet sizes to its content, capped, so it never
 * squeezes the list the user can actually open.
 */
export function sectionFillsHeight(section: Pick<RemoteSidebarSection, "state" | "model">): boolean {
  switch (section.state) {
    case "offline":
    case "loading":
      return false;
    case "ok":
    case "disconnected":
    case "error":
      return section.model !== null;
    default: {
      const unreachable: never = section.state;
      return unreachable;
    }
  }
}

type SectionProps = {
  readonly section: RemoteSidebarSection;
  readonly sidebar: RemoteSidebarProps;
  readonly first: boolean;
  readonly heading: boolean;
};

function MachineSection({ section, sidebar, first, heading }: SectionProps) {
  const [collapsed, setCollapsed] = useState<string[] | null>(() => readCollapsed(section.machineId));
  const [query, setQuery] = useState("");
  const model: SidebarModel | null = section.model
    ? { ...section.model, collapsedGroupIds: collapsed ?? section.model.collapsedGroupIds }
    : null;
  const toggleGroup = (groupId: string) => {
    const current = model?.collapsedGroupIds ?? [];
    const next = current.includes(groupId) ? current.filter((id) => id !== groupId) : [...current, groupId];
    setCollapsed(next);
    writeCollapsed(section.machineId, next);
  };
  const locked = sidebar.pending !== null;
  const select = (project: SidebarProjectNode, node: SidebarWorktreeNode) => {
    if (!locked) sidebar.onSelect({ machineId: section.machineId, project, node });
  };
  const notSynced = section.model?.source === "inventory";
  // The same derivation and filter the desktop Sidebar applies.
  const live = model && section.signals ? sidebarLiveState(model, section.signals, query) : null;
  // The project whose row the live state marks active is the active project, as on the desktop.
  const activeWorkspaceId = model?.projects
    .flatMap((project) => project.worktrees)
    .find((node) => live?.rows[node.key]?.active)?.workspaceId;
  return (
    <section
      data-testid="remote-sidebar-section"
      data-machine-id={section.machineId}
      aria-label={section.label}
      className={cn("flex min-h-0 flex-col", sectionFillsHeight(section) ? "flex-1" : "max-h-[40%] flex-none")}
    >
      {heading || notSynced ? (
        <div className="flex min-h-6 shrink-0 items-center gap-2 px-3 pt-1.5 text-[11px]">
          {heading ? (
            <h2 className="min-w-0 flex-1 truncate font-medium text-worktree-sidebar-foreground/80">{section.label}</h2>
          ) : <span className="flex-1" />}
          {notSynced ? (
            <span data-testid="remote-sidebar-not-synced" className="shrink-0 text-muted-foreground">{NOT_SYNCED_CAPTION}</span>
          ) : null}
        </div>
      ) : null}
      {first ? sidebar.status : null}
      <SectionState section={section} />
      {model && live ? (
        <SidebarView
          model={model}
          live={live}
          {...(activeWorkspaceId === undefined ? {} : { activeWorkspaceId })}
          density={sidebar.density}
          actions={{
            selectWorktree: select,
            toggleGroup,
            ...(first && sidebar.onViewChange ? { onViewChange: sidebar.onViewChange } : {}),
          }}
          filter={{ query, onChange: setQuery }}
          view={first ? sidebar.view : "worktrees"}
          {...(first && sidebar.inbox ? { inbox: sidebar.inbox } : {})}
          {...(first && sidebar.supervisor ? { supervisor: sidebar.supervisor } : {})}
          renderRow={(project, node, live) => (
            <div
              role="listitem"
              data-testid="remote-sidebar-row"
              data-workspace-id={node.workspaceId}
              data-worktree-slug={node.worktreeSlug ?? ""}
              aria-busy={isPendingRow(sidebar.pending, section.machineId, node) || undefined}
            >
              <SidebarWorktreeRow
                node={node}
                live={live}
                density={sidebar.density}
                disabled={locked}
                onSelect={() => select(project, node)}
              />
            </div>
          )}
        />
      ) : null}
    </section>
  );
}

/** One SidebarView per machine; the view tabs, inbox and supervisor ride on the first section. */
export function RemoteSidebar(sidebar: RemoteSidebarProps) {
  const heading = sidebar.sections.length > 1;
  const sections = sidebar.view === "worktrees" ? sidebar.sections : sidebar.sections.slice(0, 1);
  if (sections.length === 0) return <>{sidebar.status}</>;
  return (
    <>
      {sections.map((section, index) => (
        <MachineSection key={section.machineId} section={section} sidebar={sidebar} first={index === 0} heading={heading} />
      ))}
    </>
  );
}
