// Test fixtures: the sidebar model a desktop would publish for a list of worktrees.
import { screen, waitFor, within } from "@testing-library/react";

import type { SidebarModel, SidebarProjectNode } from "../components/sidebar/sidebarModel";

export type FixtureWorktree = {
  readonly workspaceId: string;
  /** Null is the project's root worktree. */
  readonly slug: string | null;
  readonly label?: string;
  readonly projectLabel?: string;
};

export function sidebarModelFixture(
  worktrees: readonly FixtureWorktree[],
  overrides: Partial<Pick<SidebarModel, "source" | "collapsedGroupIds" | "advancedFeatures" | "revision">> & {
    /** A desktop snapshot whose live signals the daemon dropped (its desktop disconnected). */
    readonly withoutSignals?: boolean;
  } = {},
): SidebarModel {
  const projects = new Map<string, SidebarProjectNode>();
  for (const row of worktrees) {
    const project = projects.get(row.workspaceId) ?? {
      groupId: row.workspaceId,
      label: row.projectLabel ?? row.workspaceId,
      hostLabel: null,
      members: [{ workspaceId: row.workspaceId, kind: "local" as const, machineId: null }],
      primaryWorkspaceId: row.workspaceId,
      status: null,
      filterLabels: [row.projectLabel ?? row.workspaceId],
      worktrees: [],
      worktreesEmptyHint: null,
    };
    const path = `/fixture/${row.workspaceId}/${row.slug ?? "root"}`;
    project.worktrees.push({
      key: `sidebar-worktree:${row.workspaceId}:${path}`,
      workspaceId: row.workspaceId,
      path,
      worktreeSlug: row.slug,
      label: row.label ?? row.slug ?? row.projectLabel ?? row.workspaceId,
      hostLabel: null,
      stale: false,
      listingFailure: null,
    });
    projects.set(row.workspaceId, project);
  }
  const source = overrides.source ?? "desktop";
  return {
    schemaVersion: 1,
    revision: overrides.revision ?? 1,
    generatedAt: "2026-10-24T00:00:00.000Z",
    source,
    projects: [...projects.values()],
    collapsedGroupIds: overrides.collapsedGroupIds ?? [],
    advancedFeatures: overrides.advancedFeatures ?? false,
    emptyState: projects.size === 0 ? "noProjects" : null,
    // A connected desktop always publishes its live signals; these carry no activity or Inbox.
    ...(source === "desktop" && !overrides.withoutSignals ? { signals: { unreadWorktreePaths: {}, agentStateByPath: {} } } : {}),
  };
}

/** The body GET /api/v1/workspace/sidebar answers with. */
export function sidebarBody(worktrees: readonly FixtureWorktree[]): string {
  return JSON.stringify({ model: sidebarModelFixture(worktrees) });
}

export const SIDEBAR_PATH = "/api/v1/workspace/sidebar";

/**
 * The sidebar model a test's desktop publishes, answered by URL so a fetch mock that answers
 * by call order keeps the sequence its test wrote. Tests that pick a worktree set the rows.
 */
let publishedRows: readonly FixtureWorktree[] = [];

export function publishSidebarRows(rows: readonly FixtureWorktree[]): void {
  publishedRows = rows;
}

export function resetSidebarRows(): void {
  publishedRows = [];
}

export function sidebarFixtureResponse(url: string): Response | null {
  return url.includes(SIDEBAR_PATH) ? new Response(sidebarBody(publishedRows), { status: 200 }) : null;
}

function rowSelector(workspaceId: string, slug: string | null): string {
  return `[data-testid="remote-sidebar-row"][data-workspace-id="${CSS.escape(workspaceId)}"][data-worktree-slug="${CSS.escape(slug ?? "")}"]`;
}

/** The select button of one sidebar row, or null when it is not rendered. */
export function querySidebarRow(workspaceId: string, slug: string | null, root: ParentNode = document): HTMLElement | null {
  const row = root.querySelector<HTMLElement>(rowSelector(workspaceId, slug));
  return row ? within(row).getByRole("button") : null;
}

/** Waits for one sidebar row and returns its select button. */
/** The select button of one sidebar row; throws while it is not rendered (use inside waitFor). */
export function requireSidebarRow(workspaceId: string, slug: string | null, root: ParentNode = document): HTMLElement {
  const row = querySidebarRow(workspaceId, slug, root);
  if (!row) throw new Error(`sidebar row ${workspaceId} / ${slug ?? "(root)"} not rendered`);
  return row;
}

/** Waits for one sidebar row and returns its select button. */
export function findSidebarRow(workspaceId: string, slug: string | null): Promise<HTMLElement> {
  return waitFor(() => requireSidebarRow(workspaceId, slug));
}

/** The terminal tab strip above the mirrored terminal. */
export function terminalStrip(): HTMLElement {
  return screen.getByRole("tablist", { name: "Terminal tabs" });
}
