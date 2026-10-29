import "@testing-library/jest-dom/vitest";
import { fireEvent, render, renderHook, screen, cleanup, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AttentionInbox } from "../../features/ferryx/attention/AttentionInbox";
import { remoteInboxEntries, useRemoteInbox, type RemoteInboxTransport } from "../remoteInbox";
import type { RemoteWorkspaceModel } from "../RemoteSessionList";
import { InboxHome } from "./InboxHome";
import { fixtureInventory, fixtureLane, fixtureOption } from "./phoneHomeFixtures";
import { buildInboxRows } from "./phoneHomeModel";

afterEach(cleanup);

const transport: RemoteInboxTransport = { read: vi.fn(async () => null), answer: vi.fn(async () => ({ ok: true as const })) };

// The desktop's active selection is "main"; the waiting sessions live in two other worktrees.
const model: RemoteWorkspaceModel = {
  context: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTerminal: { sessionId: "shell-1" },
    terminalTabs: [{ id: "tab-shell", sessionId: "shell-1", label: "shell", activityState: "working" }],
  },
  options: [],
};

function hostRows() {
  const options = [
    fixtureOption({ machineId: "laptop", sessionId: "wait-a", worktreeSlug: "feat-a", worktreeLabel: "feat-a", inventoryActivityState: "waiting" }),
    fixtureOption({ machineId: "laptop", sessionId: "wait-b", worktreeSlug: "feat-b", worktreeLabel: "feat-b", workspaceId: "other", workspaceLabel: "other", inventoryActivityState: "waiting" }),
    fixtureOption({ machineId: "laptop", sessionId: "done-1", inventoryActivityState: "done" }),
    fixtureOption({ machineId: "studio", sessionId: "held-1", inventoryActivityState: "waiting" }),
  ];
  const inventories = {
    laptop: fixtureInventory("laptop", [
      { sessionId: "wait-a", activityObservedAt: 300, title: "Fix the flaky test" },
      { sessionId: "wait-b", activityObservedAt: 200, title: "Migrate auth" },
      { sessionId: "done-1", activityObservedAt: 900, title: "Release notes" },
    ]),
    studio: fixtureInventory("studio", [{ sessionId: "held-1", activityObservedAt: 100, title: "Ship docs" }]),
  };
  const lanes = [{ ...fixtureLane({ key: "l", sessionId: "held-1", paused: true }), machineId: "studio" }];
  const entries = remoteInboxEntries({ model, machineId: "laptop", accountRows: buildInboxRows({ options, inventories, lanes }) });
  return renderHook(() => useRemoteInbox({ entries, transport })).result.current;
}

describe("InboxHome", () => {
  it("lists waiting sessions of every worktree on the host, not only the desktop's selection", () => {
    const inbox = hostRows();
    expect(inbox.rows.map((row) => [row.sessionId, row.state])).toEqual([
      ["wait-a", "needs-you"],
      ["wait-b", "needs-you"],
      ["held-1", "needs-you"],
      ["done-1", "done"],
    ]);
    expect(inbox.needsYou).toBe(3);
    expect(inbox.rows[0].location).toBe("laptop · ferryx / feat-a");
    expect(inbox.rows[1].location).toBe("laptop · other / feat-b");
    expect(inbox.rows[2].supervisorLabel).toBe("Supervisor paused this lane");
  });

  it("renders exactly the list the session sheet renders", () => {
    const inbox = hostRows();
    render(<InboxHome rows={inbox.rows} prompts={inbox.prompts} now={1000} loading={false} onOpen={vi.fn()} />);
    const home = within(screen.getByTestId("inbox-home")).getAllByTestId("attention-row").map((row) => row.textContent);
    cleanup();
    render(<AttentionInbox compact rows={inbox.rows} prompts={inbox.prompts} now={1000} onOpen={vi.fn()} />);
    const sheet = screen.getAllByTestId("attention-row").map((row) => row.textContent);
    expect(home).toHaveLength(4);
    expect(home).toEqual(sheet);
  });

  it("opens the tapped row", () => {
    const inbox = hostRows();
    const onOpen = vi.fn();
    render(<InboxHome rows={inbox.rows} now={1000} loading={false} onOpen={onOpen} />);
    fireEvent.click(screen.getAllByTestId("attention-row")[1]);
    expect(onOpen).toHaveBeenCalledWith(inbox.rows[1]);
  });

  it("says nothing needs the person once loading has settled", () => {
    render(<InboxHome rows={[]} now={0} loading={false} onOpen={vi.fn()} />);
    expect(screen.getByTestId("inbox-home")).toHaveTextContent("Nothing needs you");
  });
});
