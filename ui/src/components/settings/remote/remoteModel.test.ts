import { describe, expect, it } from "vitest";
import { machineRowName, matchesQuery, mergePairedHosts } from "./remoteModel";
import type { RemoteHostState } from "../../../state/remoteHostStore";
import type { AccountMachineView } from "../../../remote/accountSession";

function accountMachine(machineId: string, displayName: string): AccountMachineView {
  return {
    machineRecordId: `rec-${machineId}`,
    machineId,
    displayName,
    publicKey: "",
    attachPublicKey: "",
    relayOrigin: "https://relay.ferryx.dev",
    platform: "linux",
    online: true,
    enrollmentEpoch: 1,
    lastSeenAt: 0,
  };
}

describe("machineRowName", () => {
  it("prefers the local label, then a real hostname, then a short machine id", () => {
    expect(machineRowName("noye-box", "grok-bot-vm-405629251", "4fa4ca2f-c9d9")).toBe("noye-box");
    expect(machineRowName(undefined, "grok-bot-vm-405629251", "4fa4ca2f-c9d9")).toBe("grok-bot-vm-405629251");
    expect(machineRowName(undefined, "(none)", "547d1fd2-d39b")).toBe("Machine 547d1fd2");
    expect(machineRowName("Machine", "(none)", null)).toBe("Machine");
  });
});

describe("mergePairedHosts", () => {
  it("names a placeholder-hostname machine by id, so a search for 'no' does not match it", () => {
    const state = { hosts: {} } as unknown as RemoteHostState;
    const [row] = mergePairedHosts([accountMachine("547d1fd2-d39b", "(none)")], state, "https://relay.ferryx.dev");
    expect(row.name).toBe("Machine 547d1fd2");
    expect(matchesQuery({ kind: "paired", id: row.hostId, name: row.name, host: row }, "no")).toBe(false);
  });
});
