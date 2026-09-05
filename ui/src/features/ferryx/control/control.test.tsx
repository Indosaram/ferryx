import { describe, it, expect } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
import { AttentionInbox } from "./AttentionInbox";
import { InventoryClientState, targetKey, type Agent } from "./client";
afterEach(cleanup);
const agent = (hostId: string): Agent => ({target: {hostId, ownerId: "owner", epoch: "1", backendSessionId: "same"}, workspaceId: hostId, label: hostId, state: "waiting", revision: 1, source: {kind: "lifecycle"}});
describe("global inventory", () => {
  it("rejects stale deltas and requires reset on gaps", () => {
    const s = new InventoryClientState();
    s.bootstrap({revision: 4, items: [agent("a")], completeness: "complete", unavailableHosts: []});
    expect(s.delta({sequence: 3, revision: 3, target: agent("a").target, type: "working", data: {...agent("a"), state: "working"}})).toBe(false);
    expect(s.snapshot.items[0].state).toBe("waiting");
    expect(s.delta({sequence: 6, revision: 6, target: agent("a").target, type: "working", data: agent("a")})).toBe(false);
  });
  it("renders both hosts and selects full identity without changing another client", () => {
    const a = new InventoryClientState(), b = new InventoryClientState();
    a.bootstrap({revision: 1, items: [agent("a"), agent("b")], completeness: "partial", unavailableHosts: ["offline"]});
    render(<AttentionInbox snapshot={a.snapshot} isUnread={row => a.unread(row)} onSelect={t => { a.selected = t; a.acknowledge(t); }} />);
    const rows = screen.getAllByTestId("waiting-target");
    expect(rows).toHaveLength(2);
    fireEvent.click(rows[1]);
    expect(targetKey(a.selected!)).toBe(targetKey(agent("b").target));
    expect(b.selected).toBeNull();
    expect(a.unread(agent("a"))).toBe(true);
    expect(a.unread(agent("b"))).toBe(false);
  });
});
