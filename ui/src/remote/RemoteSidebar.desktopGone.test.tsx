// When the desktop that published the sidebar goes away, the daemon drops the snapshot's live
// signals and announces sidebarChanged. The remote must then stop drawing the desktop's frozen
// activity: rows follow this machine's own session inventory and the section says it is not synced
// with the desktop.
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { ActivitySummary } from "../lib/activity";
import type { SidebarModel } from "../components/sidebar/sidebarModel";
import { sidebarSessionSignals } from "../components/sidebar/sidebarSessionSignals";
import { RemoteSidebar } from "./RemoteSidebar";
import type { RemoteSessionInventory } from "./remoteSessionInventory";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";
import { useRemoteSidebarModel, type RemoteSidebarMachine } from "./useRemoteSidebarModel";

const built = sidebarModelFixture([{ workspaceId: "ws", slug: null, label: "main", projectLabel: "ferryx" }], { withoutSignals: true });
const path = built.projects[0]!.worktrees[0]!.path;
const working: ActivitySummary = {
  workingCount: 1, waitingCount: 0, doneCount: 0, runningCount: 1,
  hasWorking: true, hasWaiting: false, hasDone: false, hasUnread: false,
};
/** What the desktop published while it ran: one agent working. */
const published: SidebarModel = {
  ...built,
  signals: sidebarSessionSignals(built, { activityByWorktreePath: { [path]: working } }),
};
/** The same snapshot after the daemon dropped its live part (the desktop disconnected). */
const dropped: SidebarModel = { ...built, revision: 2 };

/** The machine's own session inventory: the agent is now waiting for input. */
const inventory: RemoteSessionInventory = {
  machineId: "m1", revision: "7", completeness: "complete", unavailableWorkspaceIds: [], fetchedAt: 1,
  entries: [{
    machineId: "m1", workspaceId: "ws", worktreeSlug: null, worktreeLabel: null, sessionId: "pty-1", daemonEpoch: null,
    title: null, running: true, agentType: "claude", providerSessionId: null, activityState: "waiting",
  }],
};

function Harness({ machine }: { readonly machine: RemoteSidebarMachine }) {
  const sections = useRemoteSidebarModel({ machines: [machine] });
  return (
    <RemoteSidebar
      sections={sections}
      density="compact"
      pending={null}
      onSelect={() => undefined}
      view="worktrees"
    />
  );
}

afterEach(cleanup);

describe("remote sidebar after the publishing desktop disconnects", () => {
  it("falls back to its own inventory and says it is not synced", async () => {
    let served = published;
    const handlers = new Set<(frame: unknown) => void>();
    const machine: RemoteSidebarMachine = {
      machineId: "m1", label: "Studio", inventory, selection: null,
      fetchLike: (url) => Promise.resolve({ status: 200, body: JSON.stringify(url === "/api/v1/workspace/sidebar" ? { model: served } : {}) }),
      subscribe: (handler) => {
        handlers.add(handler);
        return () => handlers.delete(handler);
      },
    };
    render(<Harness machine={machine} />);
    const row = () => screen.getByTestId("remote-sidebar-row");
    const slot = () => row().querySelector("[data-slot-state]")?.getAttribute("data-slot-state") ?? null;

    // While the desktop publishes, the remote draws the desktop's own state, not the inventory's.
    await waitFor(() => expect(slot()).toBe("working"));
    expect(screen.queryByTestId("remote-sidebar-not-synced")).toBeNull();

    // The desktop quits: the daemon serves the model without its live part and says so.
    served = dropped;
    act(() => handlers.forEach((handler) => handler({ type: "sidebarChanged", payload: { revision: 2 } })));

    await waitFor(() => expect(slot()).toBe("waiting"));
    expect(within(screen.getByTestId("remote-sidebar-section")).getByTestId("remote-sidebar-not-synced"))
      .toHaveTextContent("Not synced with desktop");
  });
});
