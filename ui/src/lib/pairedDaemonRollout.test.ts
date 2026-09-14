import { expect, it, vi } from "vitest";
import { createRemoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import { createPairedDaemonProjectAdapter, type PairedHostOperationRequest } from "./pairedDaemonProject";

function fixture() {
  const host = { hostId: remoteHostKey("https://relay.test", "machine-a"), machineId: "machine-a", generation: "7", name: "A24", address: "https://relay.test", relayOrigin: "https://relay.test", transport: "relay" as const, authStatus: "paired" as const, grantScope: "machine" as const, online: true };
  const store = createRemoteHostStore(undefined, undefined);
  store.setHosts([host]);
  store.setState(s => ({ ...s, nativeStatus: "ready" }));
  let capabilities: Record<string, unknown> = { apiVersion: 1, machineId: host.machineId, daemonEpoch: "9", platform: "linux", accessScope: "machine", permission: "control", capabilities: ["directoryBrowseV1"], limits: { directoryEntries: 100, terminalSessions: 10 } };
  const invoke = vi.fn(async (_command: "paired_host_operation", { request }: { request: PairedHostOperationRequest }) => ({ hostId: request.hostId, generation: request.generation, result: { kind: request.operation.kind, data: request.operation.kind === "capabilities" ? capabilities : { path: "/", parentPath: null, homePath: "/", entries: [], truncated: false } } }));
  const adapter = createPairedDaemonProjectAdapter(host, { store, invoke, isNative: () => true });
  return { adapter, invoke, host, store, peer: (patch: Record<string, unknown>) => { capabilities = { ...capabilities, ...patch }; } };
}

it("new local accepts an older epoch with fewer capabilities but never dispatches unsupported operations", async () => {
  const f = fixture(); f.peer({ daemonEpoch: "1", capabilities: [] });
  expect((await f.adapter.capabilities()).daemonEpoch).toBe("1");
  await expect(f.adapter.directories()).rejects.toMatchObject({ code: "UNSUPPORTED_CAPABILITY" });
  expect(f.invoke).toHaveBeenCalledTimes(1);
  expect(f.adapter.context).toEqual({ hostId: f.host.hostId, generation: "7" });
  expect(f.store.getState().hosts[f.host.hostId].grantScope).toBe("machine");
});

it.each([
  [{ apiVersion: 0, daemonEpoch: "1", capabilities: [] }, "INVALID_REQUEST"],
  [{ capabilities: ["directoryBrowseV1", "futureV9"] }, "UNSUPPORTED_CAPABILITY"],
  [{ accessScope: "mirror" }, "UNSUPPORTED_CAPABILITY"],
  [{ machineId: "other-machine" }, "CROSS_HOST_RESULT"],
] as const)("failed renegotiation revokes prior admission: %j", async (peer, code) => {
  const f = fixture();
  await f.adapter.capabilities();
  await expect(f.adapter.directories()).resolves.toMatchObject({ path: "/" });
  f.peer(peer);
  await expect(f.adapter.capabilities()).rejects.toMatchObject({ code });
  const count = f.invoke.mock.calls.length;
  await expect(f.adapter.directories()).rejects.toMatchObject({ code: "UNSUPPORTED_CAPABILITY" });
  expect(f.invoke).toHaveBeenCalledTimes(count);
  expect(f.adapter.context.hostId).toBe(f.host.hostId);
  expect(f.store.getState().hosts[f.host.hostId].grantScope).toBe("machine");
});
