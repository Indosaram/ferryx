// Remote half of the sidebar parity QA: the parity fixture published as a desktop (or an
// inventory fallback) model, drawn by the real remote workspace mirror. No network. A desktop
// snapshot carries the fixture's live signals exactly as the desktop publisher reduces them, so the
// captures show the same state dots, slots, footer and Inbox count as the desktop harness.
import React from "react";

import { buildSidebarModel } from "../components/sidebar/sidebarModel";
import { sidebarParityFixture as fixture } from "../components/sidebar/sidebarParityFixture";
import { sidebarSessionSignals } from "../components/sidebar/sidebarSessionSignals";
import { agentStatesByPath } from "../components/sidebar/useSidebarModel";
import { remoteSidebarSignals } from "../remote/remoteSidebarLive";
import { RemoteWorkspaceMirror, type RemoteWorkspaceModel } from "../remote/RemoteSessionList";
import type { RemoteSidebarSection } from "../remote/useRemoteSidebarModel";

export const SIDEBAR_PARITY_QA_STATES = ["sidebar-parity", "sidebar-parity-inventory"] as const;

/** A second account machine that is offline and never loaded a list. */
const OFFLINE_MACHINE: RemoteSidebarSection = {
  machineId: "machine-offline",
  label: "Studio Mac",
  state: "offline",
  model: null,
  signals: null,
  error: { code: "OFFLINE", message: "This computer is not reachable" },
};

export function sidebarParitySections(
  source: "desktop" | "inventory",
  options: { readonly offlineMachine?: boolean } = {},
): RemoteSidebarSection[] {
  const built = buildSidebarModel({
    source,
    generatedAt: "2026-10-24T00:00:00.000Z",
    revision: 1,
    projects: fixture.projects,
    activeProjectId: fixture.activeProjectId,
    worktrees: fixture.worktrees,
    inactiveProjectWorktrees: fixture.inactiveProjectWorktrees,
    hosts: fixture.sshHosts,
    remoteState: { hosts: fixture.pairedHosts, activeHostId: null, discovering: false, nativeStatus: "ready", machineFeaturesEnabled: true },
    reachability: () => ({ supported: false, entry: null }),
    worktreeOrder: fixture.worktreeOrder,
    collapsedGroupIds: new Set(fixture.collapsedGroupIds),
    advancedFeatures: false,
    listingFailures: fixture.listingFailures,
  });
  // Only a desktop publishes signals; the inventory fallback has none.
  const model = source === "desktop"
    ? {
        ...built,
        signals: sidebarSessionSignals(built, {
          activityByWorktreePath: fixture.desktopActivity,
          unreadWorktreePaths: fixture.desktopUnread,
          agentStateByPath: agentStatesByPath(fixture.desktopAgents.map((agent) => [agent.worktreePath, agent] as const)),
          attentionRows: fixture.desktopAttention,
        }),
      }
    : built;
  const connected: RemoteSidebarSection = {
    machineId: "machine-fixture",
    label: "Fixture Mac",
    state: "ok",
    model,
    signals: remoteSidebarSignals(model, null, fixture.active),
    error: null,
  };
  return options.offlineMachine ? [connected, OFFLINE_MACHINE] : [connected];
}

/**
 * One more finished pane than the seeded model has, so the remote's own pane-derived Inbox (3 rows)
 * never matches the desktop's published Inbox (2 rows) in size: a remote that ignores the
 * published rows shows a different Inbox count and the QA gate fails.
 */
const EXTRA_PANE = { id: "tab-parity-extra", label: "parity extra", activityState: "done" as const };

export const SidebarParityQaState: React.FC<{ state: string; model: RemoteWorkspaceModel }> = ({ state, model: seeded }) => {
  const model = React.useMemo<RemoteWorkspaceModel>(
    () => ({ ...seeded, context: { ...seeded.context, terminalTabs: [...(seeded.context.terminalTabs ?? []), EXTRA_PANE] } }),
    [seeded],
  );
  const [open, setOpen] = React.useState(true);
  const sections = React.useMemo(() => sidebarParitySections(state === "sidebar-parity-inventory" ? "inventory" : "desktop"), [state]);
  return (
    <div className="remote-app-root flex h-[100dvh] w-full flex-col bg-chat-screen">
      <RemoteWorkspaceMirror
        model={model}
        pending={null}
        selectorOpen={open}
        onSelectorOpenChange={setOpen}
        onSelect={() => undefined}
        onCreateTerminal={() => undefined}
        onCreateWorktree={() => undefined}
        sidebarSections={sections}
      />
    </div>
  );
};
