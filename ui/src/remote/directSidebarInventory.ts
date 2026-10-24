// A direct pairing has no machine session inventory; its gateway state declares activity per
// worktree option and per pane instead. This shapes those declarations as inventory rows so the
// sidebar derives live state through the same shared path as an account machine.
import type { RemoteContextOption, RemoteWorkspaceModel } from "./RemoteSessionList";
import type { RemoteSessionInventory, RemoteSessionInventoryEntry } from "./remoteSessionInventory";

function entry(fields: {
  workspaceId: string;
  worktreeSlug: string | null;
  sessionId: string;
  title: string | null;
  agentType: string | null;
  activityState: "working" | "waiting" | "done";
}): RemoteSessionInventoryEntry {
  return {
    machineId: "direct",
    worktreeLabel: null,
    daemonEpoch: null,
    running: true,
    providerSessionId: null,
    ...fields,
  };
}

function optionActivity(option: RemoteContextOption) {
  return option.attention ?? option.activityState ?? null;
}

export function directSidebarInventory(model: RemoteWorkspaceModel): RemoteSessionInventory {
  const entries: RemoteSessionInventoryEntry[] = [];
  model.options.forEach((option, index) => {
    const activityState = optionActivity(option);
    if (!activityState) return;
    entries.push(entry({
      workspaceId: option.workspaceId,
      worktreeSlug: option.worktreeSlug,
      sessionId: option.sessionId ?? `option-${index}`,
      title: option.sessionLabel ?? null,
      agentType: option.agentType ?? null,
      activityState,
    }));
  });
  for (const tab of model.context.terminalTabs ?? []) {
    const workspaceId = tab.workspaceId ?? model.context.workspaceId;
    if (!tab.activityState || !workspaceId) continue;
    entries.push(entry({
      workspaceId,
      worktreeSlug: tab.worktreeSlug !== undefined ? tab.worktreeSlug : model.context.worktreeSlug,
      sessionId: tab.sessionId ?? `tab-${tab.id}`,
      title: tab.label,
      agentType: tab.agentType ?? null,
      activityState: tab.activityState,
    }));
  }
  // The revision names the declared activity so the sidebar recomputes exactly when it changes.
  const revision = entries.map((e) => `${e.workspaceId}\u0000${e.worktreeSlug ?? ""}\u0000${e.sessionId}\u0000${e.activityState}`).join("\u0001");
  return { machineId: "direct", revision, completeness: "complete", entries, unavailableWorkspaceIds: [], fetchedAt: 0 };
}
