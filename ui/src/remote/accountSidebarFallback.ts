// The list an account machine shows while no connection to it is open: the worktrees its last
// discovery listed, drawn through the shared builder exactly like the inventory fallback.
import { buildSidebarModel, type SidebarModel } from "../components/sidebar/sidebarModel";
import type { RegisteredProject } from "../lib/tauri";
import type { Worktree } from "../lib/types";
import type { MachineDiscoveryStatus } from "./useAccountWorktrees";

/** Discovery marks a project whose listing failed with this placeholder option. */
const UNAVAILABLE_LABEL = "Unavailable";

/** Null until discovery has listed the machine once; an empty listing is an empty sidebar. */
export function discoverySidebarModel(status: MachineDiscoveryStatus | undefined): SidebarModel | null {
  if (!status || (status.status !== "ready" && status.options.length === 0)) return null;
  const projects = new Map<string, RegisteredProject>();
  const rows: Record<string, Worktree[]> = {};
  const listingFailures: Record<string, string> = {};
  for (const option of status.options) {
    if (option.sessionId) continue;
    const workspaceId = option.workspaceId;
    const folder = status.projectLabels?.[workspaceId] ?? option.projectLabel ?? workspaceId;
    const repoRoot = `/${folder.replace(/[\\/]/g, "_")}`;
    if (!projects.has(workspaceId)) projects.set(workspaceId, { workspaceId, repoRoot });
    if (option.worktreeSlug === null && option.worktreeLabel === UNAVAILABLE_LABEL) {
      listingFailures[workspaceId] = "Project unavailable";
      continue;
    }
    const slug = option.worktreeSlug;
    (rows[workspaceId] ??= []).push({
      path: slug ? `${repoRoot}/${slug}` : repoRoot,
      head: "",
      branch: option.worktreeLabel ? `refs/heads/${option.worktreeLabel}` : null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      identity: slug ? { wsId: workspaceId, slug } : null,
    });
  }
  return buildSidebarModel({
    source: "inventory",
    generatedAt: new Date(0).toISOString(),
    projects: [...projects.values()],
    activeProjectId: undefined,
    worktrees: [],
    inactiveProjectWorktrees: rows,
    hosts: [],
    remoteState: { hosts: {}, activeHostId: null, discovering: false },
    reachability: () => ({ supported: false, entry: null }),
    worktreeOrder: {},
    collapsedGroupIds: new Set(),
    // No desktop published its preference, so nothing asks to hide the Supervisor entry: an
    // account-mode remote shows it, as the phone tab bar always does.
    advancedFeatures: true,
    listingFailures,
  });
}
