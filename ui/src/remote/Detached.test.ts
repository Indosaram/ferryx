import { expect, it } from "vitest";
import { parseSessionsPayload } from "./remoteSessionInventory";

it("preserves optional detached inventory state without guessing for older hosts", () => {
  const row = { workspaceId: "workspace", target: { machineId: "machine", sessionId: "cat", daemonEpoch: 1 }, sessionId: "cat", daemonEpoch: 1, running: true, worktree: null };
  for (const detached of [true, false, undefined]) {
    const parsed = parseSessionsPayload("machine", { sessions: [{ ...row, ...(detached === undefined ? {} : { detached }) }], completeness: "complete" });
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) throw new Error("inventory refused");
    expect(parsed.inventory.entries[0].detached).toBe(detached);
    expect(parsed.inventory.entries[0].sessionId).toBe("cat");
  }
});
