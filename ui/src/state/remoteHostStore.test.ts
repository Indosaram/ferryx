import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createRemoteHostStore,
  selectActiveHost,
  selectHostList,
  type HostEndpoint,
  type RemoteHostStore,
} from "./remoteHostStore";

function makeHost(overrides: Partial<HostEndpoint> = {}): HostEndpoint {
  return {
    hostId: "host-1",
    name: "Studio Mac",
    address: "100.64.0.12",
    transport: "tailscale",
    authStatus: "paired",
    online: true,
    ...overrides,
  };
}

describe("remoteHostStore", () => {
  let store: RemoteHostStore;

  beforeEach(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.clear();
    }
    store = createRemoteHostStore();
  });

  it("starts with no hosts, no active host, and discovery off", () => {
    // Then: the store begins empty and pointed at local
    const state = store.getState();
    expect(state.hosts).toEqual({});
    expect(state.activeHostId).toBeNull();
    expect(state.discovering).toBe(false);
  });

  it("sets the full host list, replacing any prior hosts", () => {
    // Given: an initial host set
    store.setHosts([makeHost({ hostId: "a" }), makeHost({ hostId: "b" })]);
    expect(Object.keys(store.getState().hosts).sort()).toEqual(["a", "b"]);

    // When: setHosts is called again with a different set
    store.setHosts([makeHost({ hostId: "c" })]);

    // Then: only the new set remains
    expect(Object.keys(store.getState().hosts)).toEqual(["c"]);
  });

  it("upserts a host, adding new ones and updating existing ones by hostId", () => {
    // Given: an existing host
    store.upsertHost(makeHost({ hostId: "host-1", name: "Studio Mac" }));

    // When: upserting a host with the same id but different fields
    store.upsertHost(makeHost({ hostId: "host-1", name: "Studio Mac (renamed)", latencyMs: 42 }));

    // Then: exactly one entry exists with the updated content
    const hosts = store.getState().hosts;
    expect(Object.keys(hosts)).toEqual(["host-1"]);
    expect(hosts["host-1"].name).toBe("Studio Mac (renamed)");
    expect(hosts["host-1"].latencyMs).toBe(42);

    // When: upserting a distinct host id
    store.upsertHost(makeHost({ hostId: "host-2", name: "Laptop" }));

    // Then: both hosts are present
    expect(Object.keys(store.getState().hosts).sort()).toEqual(["host-1", "host-2"]);
  });

  it("removes a host by id and leaves others untouched", () => {
    // Given: two discovered hosts
    store.setHosts([makeHost({ hostId: "a" }), makeHost({ hostId: "b" })]);

    // When: removing one
    store.removeHost("a");

    // Then: only the remaining host is present
    expect(Object.keys(store.getState().hosts)).toEqual(["b"]);
  });

  it("falls back to local when the active remote host is removed", () => {
    // Given: an active remote host
    store.setHosts([makeHost({ hostId: "a" })]);
    store.setActiveHost("a");
    expect(store.getState().activeHostId).toBe("a");

    // When: that host is removed
    store.removeHost("a");

    // Then: the active host reverts to local (null)
    expect(store.getState().activeHostId).toBeNull();
  });

  it("switches activeHostId to a remote host and back to local (null)", () => {
    // Given: a paired remote host
    store.setHosts([makeHost({ hostId: "remote-1" })]);
    expect(store.getState().activeHostId).toBeNull();

    // When: switching to the remote host
    store.setActiveHost("remote-1");

    // Then: the active host is the remote one, and it stays in the registry
    expect(store.getState().activeHostId).toBe("remote-1");
    expect(store.getState().hosts["remote-1"]).toBeDefined();

    // When: switching back to local
    store.setActiveHost(null);

    // Then: activeHostId is null and the host registry is unaffected (nothing dropped)
    expect(store.getState().activeHostId).toBeNull();
    expect(store.getState().hosts["remote-1"]).toBeDefined();
    expect(Object.keys(store.getState().hosts)).toEqual(["remote-1"]);
  });

  it("does not touch the hosts map when only switching the active host", () => {
    // Given: two hosts registered
    store.setHosts([makeHost({ hostId: "a" }), makeHost({ hostId: "b" })]);
    const hostsBefore = store.getState().hosts;

    // When: switching the active host back and forth (including through local)
    store.setActiveHost("a");
    store.setActiveHost("b");
    store.setActiveHost(null);

    // Then: the hosts map reference/content is unchanged by active-host switching,
    // i.e. switching hosts never drops any locally-registered host/session state.
    expect(store.getState().hosts).toBe(hostsBefore);
  });

  it("tracks latency and auth/online status per host", () => {
    // Given: a host with explicit latency and auth status
    const host = makeHost({
      hostId: "remote-2",
      latencyMs: 18,
      authStatus: "unpaired",
      online: false,
    });
    store.upsertHost(host);

    // Then: the stored host reflects those properties exactly
    const stored = store.getState().hosts["remote-2"];
    expect(stored.latencyMs).toBe(18);
    expect(stored.authStatus).toBe("unpaired");
    expect(stored.online).toBe(false);

    // When: latency/status change (e.g. a fresh discovery ping)
    store.upsertHost({ ...host, latencyMs: 5, authStatus: "paired", online: true });

    // Then: the update is reflected
    const updated = store.getState().hosts["remote-2"];
    expect(updated.latencyMs).toBe(5);
    expect(updated.authStatus).toBe("paired");
    expect(updated.online).toBe(true);
  });

  it("selectActiveHost resolves the active host endpoint or null for local", () => {
    // Given: two hosts, none active
    store.setHosts([makeHost({ hostId: "a" }), makeHost({ hostId: "b", name: "Zeta" })]);
    expect(selectActiveHost(store.getState())).toBeNull();

    // When: activating one
    store.setActiveHost("a");

    // Then: selector resolves the matching endpoint
    expect(selectActiveHost(store.getState())?.hostId).toBe("a");

    // When: switching back to local
    store.setActiveHost(null);

    // Then: selector resolves to null again
    expect(selectActiveHost(store.getState())).toBeNull();
  });

  it("selectHostList returns hosts sorted by name", () => {
    // Given: hosts registered out of alphabetical order
    store.setHosts([
      makeHost({ hostId: "b", name: "Zeta" }),
      makeHost({ hostId: "a", name: "Alpha" }),
    ]);

    // Then: selectHostList sorts them by name
    expect(selectHostList(store.getState()).map((h) => h.name)).toEqual(["Alpha", "Zeta"]);
  });

  it("toggles discovering state", () => {
    // Given: discovery is initially off
    expect(store.getState().discovering).toBe(false);

    // When: discovery starts
    store.setDiscovering(true);
    expect(store.getState().discovering).toBe(true);

    // When: discovery ends
    store.setDiscovering(false);
    expect(store.getState().discovering).toBe(false);
  });

  it("notifies subscribers on change and stops after unsubscribe", () => {
    // Given: a subscribed listener
    const listener = vi.fn();
    const unsubscribe = store.subscribe(listener);

    // When: switching the active host
    store.setActiveHost("remote-1");
    expect(listener).toHaveBeenCalledTimes(1);

    // When: unsubscribing and changing state again
    unsubscribe();
    store.setActiveHost(null);

    // Then: subscriber received no further notifications
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("reset restores initial state (empty hosts, local active, discovery off)", () => {
    // Given: a mutated store
    store.setHosts([makeHost({ hostId: "a" })]);
    store.setActiveHost("a");
    store.setDiscovering(true);

    // When: reset
    store.reset();

    // Then: state matches the initial defaults
    expect(store.getState()).toEqual({ hosts: {}, activeHostId: null, discovering: false });
  });
});
