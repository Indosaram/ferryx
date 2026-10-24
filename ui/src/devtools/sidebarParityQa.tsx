// Remote half of the sidebar parity QA: the parity fixture published as a desktop (or an
// inventory fallback) model, drawn by the real remote workspace mirror. No network.
import React from "react";

import { buildSidebarModel } from "../components/sidebar/sidebarModel";
import { sidebarParityFixture as fixture } from "../components/sidebar/sidebarParityFixture";
import { remoteSidebarSignals } from "../remote/remoteSidebarLive";
import { RemoteWorkspaceMirror, type RemoteWorkspaceModel } from "../remote/RemoteSessionList";
import type { RemoteSidebarSection } from "../remote/useRemoteSidebarModel";

export const SIDEBAR_PARITY_QA_STATES = ["sidebar-parity", "sidebar-parity-inventory"] as const;

export function sidebarParitySections(source: "desktop" | "inventory"): RemoteSidebarSection[] {
  const model = buildSidebarModel({
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
  return [{
    machineId: "machine-fixture",
    label: "Fixture Mac",
    state: "ok",
    model,
    signals: remoteSidebarSignals(model, null, fixture.active),
    error: null,
  }];
}

export const SidebarParityQaState: React.FC<{ state: string; model: RemoteWorkspaceModel }> = ({ state, model }) => {
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
