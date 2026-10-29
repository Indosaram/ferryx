// The Inbox row identity is machine plus session: two machines that share a session id stay two rows.
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { remoteInboxEntries, useRemoteInbox, type RemoteInboxTransport } from "./remoteInbox";
import type { RemoteWorkspaceModel } from "./RemoteSessionList";
import { fixtureInventory, fixtureOption } from "./home/phoneHomeFixtures";
import { buildInboxRows } from "./home/phoneHomeModel";

afterEach(cleanup);

const transport: RemoteInboxTransport = { read: vi.fn(async () => null), answer: vi.fn(async () => ({ ok: true as const })) };
const base: RemoteWorkspaceModel = {
  context: { workspaceId: "ferryx", worktreeSlug: null, worktreeLabel: null, activeTerminal: null, terminalTabs: [] },
  options: [],
};

function accountRows() {
  const options = [
    fixtureOption({ machineId: "laptop", sessionId: "pty-1", inventoryActivityState: "waiting" }),
    fixtureOption({ machineId: "studio", sessionId: "pty-1", inventoryActivityState: "waiting" }),
  ];
  const inventories = {
    laptop: fixtureInventory("laptop", [{ sessionId: "pty-1" }]),
    studio: fixtureInventory("studio", [{ sessionId: "pty-1" }]),
  };
  return buildInboxRows({ options, inventories, lanes: [] });
}

describe("remote Inbox: one row per machine and session", () => {
  it("keeps two machines' same session id as two rows", () => {
    const entries = remoteInboxEntries({ model: base, machineId: "laptop", accountRows: accountRows() });
    expect(entries.map((entry) => `${entry.machineId}/${entry.sessionId}`).sort()).toEqual(["laptop/pty-1", "studio/pty-1"]);
    const { result } = renderHook(() => useRemoteInbox({ entries, transport }));
    expect(result.current.rows).toHaveLength(2);
    expect(new Set(result.current.rows.map((row) => row.id)).size).toBe(2);
    expect(result.current.needsYou).toBe(2);
  });

  it("a mirrored host's sessionAttention row and another machine's same id stay distinct", () => {
    const model: RemoteWorkspaceModel = {
      ...base,
      sessionAttention: [{ sessionId: "pty-1", workspaceId: "ferryx", worktreeSlug: null, state: "waiting", agentType: "claude" }],
    };
    const studioOnly = accountRows().filter((row) => row.machineId === "studio");
    const entries = remoteInboxEntries({ model, machineId: "laptop", accountRows: studioOnly });
    expect(entries.map((entry) => [entry.machineId, entry.sessionId, entry.readable])).toEqual([
      ["laptop", "pty-1", true],
      ["studio", "pty-1", false],
    ]);
  });
});
