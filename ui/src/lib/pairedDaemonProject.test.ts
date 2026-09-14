import { describe, expect, it, vi } from "vitest";
import { createRemoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import { createPairedDaemonProjectAdapter, type PairedHostOperationRequest } from "./pairedDaemonProject";
import fixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import lifecycle from "../../../docs/evidence/paired-daemon/fixtures/lifecycle.json";
import identities from "../../../docs/evidence/paired-daemon/fixtures/identities.json";

function fixture(kind: string) {
  const row = [...fixtures, ...lifecycle].find(row => row.kind === kind);
  if (!row) throw new Error("Missing fixture");
  return row.value;
}
function gate<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
const requestId = "3941b9de-b16d-4d9a-ae0a-118f90fd91f4";
function setup() {
  const store = createRemoteHostStore(undefined, undefined);
  const hosts = ["machine-a", "machine-b"].map(machineId => ({
    hostId: remoteHostKey("https://relay.test", machineId), machineId, generation: "7",
    name: machineId, address: "https://relay.test", relayOrigin: "https://relay.test",
    transport: "relay" as const, authStatus: "paired" as const, grantScope: "machine" as const, online: true,
  }));
  store.setHosts(hosts);
  store.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: false }));
  let respond: (request: PairedHostOperationRequest) => Promise<unknown> = async request => {
    const { operation } = request;
    if (operation.kind === "registerProject") return project(request.hostId);
    return fixture(operation.kind);
  };
  function project(hostId: string) {
    return { ...fixture("project"), workspaceId: identities[hostId === hosts[0].hostId ? 0 : 1].workspaceId,
      remoteWorkspaceId: "project-a", target: { kind: "pairedDaemon", hostId } };
  }
  const invoke = vi.fn(async (_command: "paired_host_operation", { request }: { request: PairedHostOperationRequest }) => {
    const data = request.operation.kind === "capabilities"
      ? { ...fixture("capabilities"), machineId: hosts.find(h => h.hostId === request.hostId)?.machineId }
      : await respond(request);
    return { hostId: request.hostId, generation: request.generation, result: { kind: request.operation.kind, data } };
  });
  const adapters = hosts.map(host => createPairedDaemonProjectAdapter(host, { store, invoke, isNative: () => true }));
  return { store, hosts, adapters, invoke, project, respond: (fn: typeof respond) => { respond = fn; } };
}

describe("paired desktop operation boundary", () => {
  it("captures equal-path owners independently of selection and preserves Rust IDs", async () => {
    const s = setup();
    await Promise.all(s.adapters.map(a => a.capabilities()));
    const entered = gate<void>(), release = gate<unknown>();
    s.respond(async request => {
      if (request.hostId === s.hosts[0].hostId) { entered.resolve(); return release.promise; }
      return s.project(request.hostId);
    });
    const pending = s.adapters[0].registerProject({ requestId, repoPath: "/home/test/app" });
    await entered.promise;
    s.store.setActiveHost(s.hosts[1].hostId);
    const second = await s.adapters[1].registerProject({ requestId, repoPath: "/home/test/app" });
    release.resolve(s.project(s.hosts[0].hostId));
    const first = await pending;
    expect(first.repoRoot).toBe(second.repoRoot);
    expect(first.workspaceId).not.toBe(second.workspaceId);
    expect(first.remoteWorkspaceId).toBe("project-a");
    expect(first.target.hostId).toBe(s.hosts[0].hostId);
    expect(s.store.getState().machineFeaturesEnabled).toBe(false);
  }, 2000);

  it.each(["re-pair", "forget", "revoke"])("rejects late registration after %s", async change => {
    const s = setup(); await s.adapters[0].capabilities();
    const entered = gate<void>(), release = gate<unknown>();
    s.respond(async () => { entered.resolve(); return release.promise; });
    const pending = s.adapters[0].registerProject({ requestId, repoPath: "/home/test/app" });
    const rejected = expect(pending).rejects.toMatchObject({ code: change === "revoke" ? "MACHINE_GRANT_REQUIRED" : "STALE_HOST_GENERATION" });
    await entered.promise;
    if (change === "forget") s.store.removeHost(s.hosts[0].hostId);
    else s.store.upsertHost({ ...s.hosts[0], ...(change === "re-pair" ? { generation: "8" } : { authStatus: "revoked" as const }) });
    release.resolve(s.project(s.hosts[0].hostId));
    await rejected;
  }, 2000);

  it("requires native context and negotiated capabilities before dispatch", async () => {
    const s = setup();
    await expect(s.adapters[0].projects()).rejects.toMatchObject({ code: "UNSUPPORTED_CAPABILITY" });
    const browser = createPairedDaemonProjectAdapter(s.hosts[0], { store: s.store, invoke: s.invoke, isNative: () => false });
    await expect(browser.capabilities()).rejects.toMatchObject({ code: "NATIVE_CONTEXT_REQUIRED" });
    expect(s.invoke).not.toHaveBeenCalled();
  });
  it.each(["generation", "hostId", "kind", "machineId", "unknownCapability"])("rejects invalid capability %s", async field => {
    const s = setup();
    const invoke = vi.fn(async () => ({ hostId: field === "hostId" ? s.hosts[1].hostId : s.hosts[0].hostId,
      generation: field === "generation" ? "8" : "7", result: {
        kind: field === "kind" ? "projects" : "capabilities", data: { ...fixture("capabilities"),
          ...(field === "machineId" ? { machineId: "machine-b" } : {}),
          ...(field === "unknownCapability" ? { capabilities: ["futureV9"] } : {}) } } }));
    const a = createPairedDaemonProjectAdapter(s.hosts[0], { store: s.store, invoke, isNative: () => true });
    await expect(a.capabilities()).rejects.toThrow();
    await expect(a.projects()).rejects.toMatchObject({ code: "UNSUPPORTED_CAPABILITY" });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
  it.each(["", "../project", "daemon:" + "a".repeat(64), "project/a", "project\0a"])("rejects invalid remote ID %j before dispatch", async id => {
    const s = setup(); await s.adapters[0].capabilities();
    await expect(s.adapters[0].worktrees(id)).rejects.toMatchObject({ code: "INVALID_REMOTE_WORKSPACE_ID" });
    expect(s.invoke).toHaveBeenCalledTimes(1);
  });
  it("validates project mapping, metadata and desktop unavailable IDs", async () => {
    const s = setup(); await s.adapters[0].capabilities();
    s.respond(async () => ({ revision: "7", completeness: "partial", projects: [s.project(s.hosts[0].hostId)], unavailableWorkspaceIds: [identities[0].workspaceId] }));
    const list = await s.adapters[0].projects();
    expect(list.projects[0].availability).toBe("ready");
    expect(list.unavailableWorkspaceIds).toEqual([identities[0].workspaceId]);
    s.respond(async () => ({ ...s.project(s.hosts[0].hostId), remoteWorkspaceId: "../bad" }));
    await expect(s.adapters[0].registerProject({ requestId, repoPath: "/app" })).rejects.toThrow();
    s.respond(async () => s.project(s.hosts[1].hostId));
    await expect(s.adapters[0].registerProject({ requestId, repoPath: "/app" })).rejects.toMatchObject({ code: "CROSS_HOST_RESULT" });
  });
  it("preserves structured machine failure but drops raw native bodies without retry", async () => {
    const s = setup(); await s.adapters[0].capabilities();
    s.respond(async () => { throw fixture("error"); });
    await expect(s.adapters[0].registerProject({ requestId, repoPath: "/app" })).rejects.toMatchObject({ code: "HOST_UNAVAILABLE", requestId, retryable: true });
    s.respond(async () => { throw "<html>Bearer secret</html>"; });
    await expect(s.adapters[0].projects()).rejects.toMatchObject({ message: "PAIRED_HOST_UNAVAILABLE" });
    expect(s.invoke).toHaveBeenCalledTimes(3);
  });
  it("exposes directory, worktree, session and reconciliation parsers", async () => {
    const s = setup(); await s.adapters[0].capabilities();
    expect(await s.adapters[0].directories("~", true)).toEqual(fixture("directories"));
    expect(await s.adapters[0].worktrees("project-a")).toEqual(fixture("worktrees"));
    expect(await s.adapters[0].worktreeStatus("project-a", { wsId: "project-a", slug: "feature" })).toEqual(fixture("worktreeStatus"));
    expect(await s.adapters[0].sessions("project-a")).toEqual(fixture("sessions"));
    expect(await s.adapters[0].operation(requestId)).toEqual(fixture("operation"));
    s.respond(async () => fixture("sessionDetail"));
    expect(await s.adapters[0].session("pty-1", "9007199254740993")).toEqual(fixture("sessionDetail"));
    s.respond(async () => ({ ...fixture("session"), target: { machineId: "machine-b", daemonEpoch: "7", sessionId: "pty-1" } }));
    await expect(s.adapters[0].createSession({ requestId, workspaceId: "project-a", worktree: null, cols: 80, rows: 24, cwdRelative: null, inheritFromSessionId: null, startup: { kind: "shell" } })).rejects.toMatchObject({ code: "CROSS_HOST_RESULT" });
  });
  it("dispatches typed mutations and preserves native ambiguity for explicit reconciliation", async () => {
    const s = setup(); await s.adapters[0].capabilities();
    const request = { requestId, workspaceId: "project-a", worktree: { wsId: "project-a", slug: "feature" } };
    s.respond(async () => fixture("worktree"));
    expect(await s.adapters[0].createWorktree({ ...request, baseRef: "HEAD" })).toEqual(fixture("worktree"));
    s.respond(async () => null);
    expect(await s.adapters[0].deleteWorktree({ ...request, deleteBranch: false, expectedRevision: "7" })).toBeNull();
    expect(await s.adapters[0].unregisterProject("project-a", { requestId, expectedRevision: "7" })).toBeNull();
    expect(await s.adapters[0].closeSession("pty-1", { requestId, daemonEpoch: "9" })).toBeNull();
    s.respond(async () => { throw { code: "TIMEOUT", machineError: null, requestId, ambiguous: true }; });
    await expect(s.adapters[0].registerProject({ requestId, repoPath: "/app" })).rejects.toMatchObject({ code: "TIMEOUT", requestId, ambiguous: true });
    s.respond(async () => ({ state: "completed", requestId, outcome: { kind: "project", project: fixture("project") } }));
    expect(await s.adapters[0].operation(requestId)).toMatchObject({ outcome: { project: { workspaceId: "project-a" } } });
    const machine = fixture("error");
    s.respond(async () => { throw { code: "HOST_UNAVAILABLE", machineError: "error" in machine ? machine.error : null, requestId, ambiguous: true }; });
    await expect(s.adapters[0].projects()).rejects.toMatchObject({ code: "HOST_UNAVAILABLE", retryable: true, ambiguous: true });
  });

});
