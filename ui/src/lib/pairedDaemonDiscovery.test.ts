import { describe, expect, it, vi } from "vitest";
import identities from "../../../docs/evidence/paired-daemon/fixtures/identities.json";
import fixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import { decodeMachineJson, MachineDecodeError } from "./pairedDaemonContracts";
import { createRemoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import { createPairedDaemonProjectAdapter, type PairedHostOperationRequest } from "./pairedDaemonProject";

const hostId = remoteHostKey("https://relay.test", "machine-a");
const target = { kind: "pairedDaemon", hostId };
const registered = { repoRoot: "/r/app", isLinkedWorktree: false, gitCommonDir: "/r/app/.git", branch: "main",
  registeredWorkspaceId: identities[0].workspaceId, remoteWorkspaceId: "project-a", target };
const unregistered = { repoRoot: "/r/wt", isLinkedWorktree: true, gitCommonDir: null, target };
const wire = { path: "/r", repos: [registered, unregistered], truncated: true, elapsedMs: 3 };

describe("repository discovery decoder", () => {
  it("decodes registered and unregistered rows", () => {
    const decoded = decodeMachineJson("repositories", JSON.stringify(wire));
    expect(decoded).toEqual({ path: "/r", truncated: true, elapsedMs: 3, repos: [
      registered, { ...unregistered, branch: null, registeredWorkspaceId: null, remoteWorkspaceId: null }] });
  });
  it.each([
    ["half registration", { ...registered, remoteWorkspaceId: undefined }],
    ["host-local id in desktop slot", { ...registered, registeredWorkspaceId: "project-a" }],
    ["non-paired target", { ...registered, target: { kind: "local" } }],
    ["missing linked flag", { ...unregistered, isLinkedWorktree: undefined }],
  ])("rejects %s", (_name, row) => {
    expect(() => decodeMachineJson("repositories", JSON.stringify({ ...wire, repos: [row] }))).toThrow(MachineDecodeError);
  });
  it("rejects more than the 200 repository cap and bounds bytes like directories", () => {
    expect(() => decodeMachineJson("repositories", JSON.stringify({ ...wire, repos: Array(201).fill(unregistered) }))).toThrow(MachineDecodeError);
    expect(() => decodeMachineJson("repositories", " ".repeat(262145))).toThrow(new MachineDecodeError("PAYLOAD_TOO_LARGE"));
  });
});

describe("discoverRepos adapter", () => {
  function setup(capabilities: string[], data: unknown = wire) {
    const store = createRemoteHostStore(undefined, undefined);
    store.setHosts([{ hostId, machineId: "machine-a", generation: "7", name: "a", address: "https://relay.test", relayOrigin: "https://relay.test",
      transport: "relay", authStatus: "paired", grantScope: "machine", online: true }]);
    store.setState(s => ({ ...s, nativeStatus: "ready" }));
    const invoke = vi.fn(async (_command: "paired_host_operation", { request }: { request: PairedHostOperationRequest }) => ({
      hostId: request.hostId, generation: request.generation, result: { kind: request.operation.kind,
        data: request.operation.kind === "capabilities" ? { ...fixtures.find(r => r.kind === "capabilities")!.value, capabilities } : data } }));
    return { invoke, adapter: createPairedDaemonProjectAdapter({ hostId, generation: "7" }, { store, invoke, isNative: () => true }) };
  }
  it("requires repoDiscoveryV1 and forwards depth and hidden flags", async () => {
    const missing = setup(["directoryBrowseV1"]);
    await missing.adapter.capabilities();
    await expect(missing.adapter.discoverRepos()).rejects.toThrow("UNSUPPORTED_CAPABILITY");
    const s = setup(["repoDiscoveryV1"]);
    await s.adapter.capabilities();
    expect((await s.adapter.discoverRepos("~/src", { depth: 2, includeHidden: true })).repos).toHaveLength(2);
    expect(s.invoke).toHaveBeenLastCalledWith("paired_host_operation", { request: { hostId, generation: "7",
      operation: { kind: "discoverRepos", path: "~/src", depth: 2, includeHidden: true } } });
    await expect(s.adapter.discoverRepos(undefined, { depth: 5 })).rejects.toThrow("INVALID_REQUEST");
  });
  it("refuses rows that name another host", async () => {
    const s = setup(["repoDiscoveryV1"], { ...wire, repos: [{ ...unregistered, target: { kind: "pairedDaemon", hostId: "other" } }] });
    await s.adapter.capabilities();
    await expect(s.adapter.discoverRepos()).rejects.toThrow("CROSS_HOST_RESULT");
  });
});

