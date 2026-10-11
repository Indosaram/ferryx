import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(() => true) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => mocks.invoke(...args), isTauri: () => mocks.isTauri() }));

import { mergeTailscaleSshHosts, resetSshHostsCache, subscribeSshHosts, type SshHost } from "./sshHosts";
import { getTailscaleDiscoverySnapshot, isTailscaleProvisioned, resetTailscaleDiscoveryForTests, startTailscaleDiscovery, subscribeTailscaleDiscovery, syncTailscaleMachines } from "./tailscaleDiscovery";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function host(id: string, hostname = `${id}.tailnet`): SshHost {
  return { id, label: id, hostname, source: "tailscale", authMethod: "agent" };
}

describe("Tailscale discovery lifecycle", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    resetTailscaleDiscoveryForTests();
    mocks.isTauri.mockReturnValue(true);
  });
  afterEach(() => resetTailscaleDiscoveryForTests());

  it("starts one nonblocking desktop sync and coalesces concurrent refreshes", async () => {
    const response = deferred<{ hosts: SshHost[]; targets: [] }>();
    mocks.invoke.mockReturnValue(response.promise);
    const stop = startTailscaleDiscovery();
    const refresh = syncTailscaleMachines();
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).toHaveBeenCalledWith("cmd_ssh_sync_tailscale");
    response.resolve({ hosts: [host("box")], targets: [] });
    await Promise.all([refresh]);
    expect(getTailscaleDiscoverySnapshot().hosts).toEqual([host("box")]);
    stop();
  });

  it("runs exactly one follow-up for focus and online received during a pending sync", async () => {
    const first = deferred<{ hosts: SshHost[]; targets: [] }>();
    const second = deferred<{ hosts: SshHost[]; targets: [] }>();
    const secondCalled = deferred<void>();
    mocks.invoke.mockReturnValueOnce(first.promise).mockImplementationOnce(() => {
      secondCalled.resolve();
      return second.promise;
    });
    const stop = startTailscaleDiscovery();
    window.dispatchEvent(new Event("focus"));
    window.dispatchEvent(new Event("online"));
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    first.resolve({ hosts: [], targets: [] });
    await secondCalled.promise;
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
    const done = new Promise<void>((resolve) => {
      const unsubscribe = subscribeTailscaleDiscovery(() => {
        if (getTailscaleDiscoverySnapshot().hosts.some((item) => item.id === "late-peer")) { unsubscribe(); resolve(); }
      });
    });
    second.resolve({ hosts: [host("late-peer")], targets: [] });
    await done;
    stop();
  });

  it("rejects stale sync inventory after a user host edit or deletion", async () => {
    const response = deferred<{ hosts: SshHost[]; targets: [] }>();
    mocks.invoke.mockReturnValueOnce(response.promise).mockResolvedValue({ hosts: [], targets: [] });
    const stop = startTailscaleDiscovery();
    const current: SshHost = { id: "manual", label: "Edited", hostname: "edited", source: "manual", authMethod: "agent" };
    const oldTailnet = host("old-peer");
    mergeTailscaleSshHosts([oldTailnet]);
    const changed = new Promise<void>((resolve) => {
      const unsubscribe = subscribeSshHosts((hosts) => {
        if (hosts.some((item) => item.id === "manual")) { unsubscribe(); resolve(); }
      });
      mergeTailscaleSshHosts([current]);
    });
    await changed;
    response.resolve({ hosts: [host("old-peer", "resurrected.tailnet"), host("manual", "resurrected.tailnet")], targets: [] });
    await syncTailscaleMachines();
    expect(getTailscaleDiscoverySnapshot().hosts).toEqual([]);
    stop();
  });

  it("never invokes native discovery in a browser runtime or when only the hook is used", async () => {
    mocks.isTauri.mockReturnValue(false);
    expect(getTailscaleDiscoverySnapshot().status).toBe("idle");
    const stop = startTailscaleDiscovery();
    await syncTailscaleMachines();
    expect(mocks.invoke).not.toHaveBeenCalled();
    stop();
  });

  it("allows the subscription hook consumer to observe transitions without starting discovery", () => {
    const states: string[] = [];
    const unsubscribe = subscribeTailscaleDiscovery(() => states.push(getTailscaleDiscoverySnapshot().status));
    expect(mocks.invoke).not.toHaveBeenCalled();
    unsubscribe();
    expect(states).toEqual([]);
  });

  it("publishes typed discovery failures", async () => {
    mocks.invoke.mockResolvedValue({ hosts: [], targets: [], discoveryError: { code: "TAILSCALE_OFFLINE", message: "Tailscale is offline" } });
    await syncTailscaleMachines();
    expect(getTailscaleDiscoverySnapshot()).toMatchObject({ status: "error", discoveryError: { code: "TAILSCALE_OFFLINE" } });
  });

  it("retains readiness for a matched manual host and invalidates it after editing credentials", async () => {
    const manual: SshHost = { ...host("saved"), source: "manual", username: "developer" };
    mocks.invoke.mockResolvedValue({ hosts: [manual], targets: [{ host: manual, reachable: true, helper: "installed", checkedAt: 1 }] });
    await syncTailscaleMachines();
    expect(isTailscaleProvisioned(getTailscaleDiscoverySnapshot().targetsByHost.get(manual.id))).toBe(true);
    mergeTailscaleSshHosts([{ ...manual, username: "another-user" }]);
    expect(getTailscaleDiscoverySnapshot().targetsByHost.has(manual.id)).toBe(false);
  });

  it("accepts discovery after an identical inventory read during the pending request", async () => {
    const current = host("existing");
    mergeTailscaleSshHosts([current]);
    const response = deferred<{ hosts: SshHost[]; targets: [] }>();
    mocks.invoke.mockReturnValue(response.promise);
    const sync = syncTailscaleMachines();
    mergeTailscaleSshHosts([current]);
    response.resolve({ hosts: [current, host("new")], targets: [] });
    await sync;
    expect(getTailscaleDiscoverySnapshot().hosts.map((item) => item.id)).toEqual(["existing", "new"]);
  });

  it("marks a host provisioned only when SSH succeeds and helper is installed", () => {
    const target = { host: host("ready"), reachable: true, helper: "installed" as const, checkedAt: 1 };
    expect(isTailscaleProvisioned(target)).toBe(true);
    expect(isTailscaleProvisioned({ ...target, reachable: false })).toBe(false);
    expect(isTailscaleProvisioned({ ...target, helper: "missing" })).toBe(false);
    expect(isTailscaleProvisioned({ ...target, diagnostic: { code: "AUTH_REQUIRED", message: "Key required" } })).toBe(false);
  });
});
