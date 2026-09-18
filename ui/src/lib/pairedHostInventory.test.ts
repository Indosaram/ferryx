import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createRemoteHostStore, remoteHostKey, REMOTE_HOST_STORAGE_KEY } from "../state/remoteHostStore";
import { createPairedHostInventory, nativePairedHostCommands, normalizeRelayOrigin, parsePairingInvite, type HostView, type PairedHostCommands } from "./pairedHostInventory";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
const hostId = remoteHostKey("https://relay.example", "fixture");
const view: HostView = { hostId, relayOrigin: "https://relay.example", machineId: "fixture", displayLabel: "Fixture", generation: "9", grantScope: "machine", authStatus: "paired", online: true };
const token = "private-fixture-bearer";
const tokenKey = `ferryx_remote_token_${hostId}`;
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(r => { resolve = r; }); return { promise, resolve }; }
function fixture() {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = { list: vi.fn().mockResolvedValue([view]), capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }), pair: vi.fn().mockResolvedValue({ ...view, generation: "10" }), forget: vi.fn().mockResolvedValue(undefined), migrate: vi.fn().mockResolvedValue({ hostId, generation: "9" }), read: vi.fn().mockResolvedValue(view) };
  return { store, commands, inventory: createPairedHostInventory(store, commands, localStorage) };
}
function legacy() {
  const original = JSON.stringify({ activeHostId: hostId, hosts: { [hostId]: { machineId: "fixture", relayOrigin: view.relayOrigin, displayName: "Fixture", deviceToken: token }, other: { name: "unrelated", deviceToken: "other-fixture-token" } } });
  localStorage.setItem(REMOTE_HOST_STORAGE_KEY, original);
  localStorage.setItem(tokenKey, token);
  return original;
}
beforeEach(() => { localStorage.clear(); vi.clearAllMocks(); });
it.each(["migrate", "read"] as const)("failed native %s preserves exact legacy bytes and reports pending", async method => {
  const original = legacy(); const { commands, inventory, store } = fixture();
  vi.mocked(commands[method]).mockRejectedValue(new Error(token));
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  await inventory.migrateLegacy();
  expect(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)).toBe(original);
  expect(localStorage.getItem(tokenKey)).toBe(token);
  expect(store.getState().migrationStatus).toBe("pending");
  expect(JSON.stringify(store.getState())).not.toContain(token);
  expect(warn).not.toHaveBeenCalled(); warn.mockRestore();
});
it("verified migration removes only the exact credential and matching key, retaining other records", async () => {
  legacy(); const { inventory, commands, store } = fixture();
  await inventory.migrateLegacy();
  expect(commands.read).toHaveBeenCalledWith({ hostId, generation: "9" });
  const saved = JSON.parse(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)!);
  expect(saved.hosts[hostId].deviceToken).toBeUndefined();
  expect(saved.hosts.other.deviceToken).toBe("other-fixture-token");
  expect(saved.activeHostId).toBe(hostId);
  expect(localStorage.getItem(tokenKey)).toBeNull();
  expect(store.getState().hosts[hostId].authStatus).toBe("paired");
  expect(JSON.stringify(store.getState())).not.toContain(token);
});
it.each([{ hostId: "wrong", generation: "9" }, { hostId, generation: "09" }, { hostId, generation: "18446744073709551616" }])("rejects mismatched or malformed migration receipt %j", async receipt => {
  const original = legacy(); const { inventory, commands, store } = fixture();
  vi.mocked(commands.migrate).mockResolvedValue(receipt);
  await inventory.migrateLegacy();
  expect(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)).toBe(original);
  expect(store.getState().migrationStatus).toBe("pending");
});
it("a mismatched readback cannot authorize cleanup", async () => {
  const original = legacy(); const { inventory, commands } = fixture();
  vi.mocked(commands.read).mockResolvedValue({ ...view, generation: "10" });
  await inventory.migrateLegacy();
  expect(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)).toBe(original);
  expect(localStorage.getItem(tokenKey)).toBe(token);
});
it("never assigns an origin-wide token or ambiguous alias key to a machine", async () => {
  localStorage.setItem(REMOTE_HOST_STORAGE_KEY, JSON.stringify({ machineId: "fixture", relayOrigin: view.relayOrigin, hostId: "alias", displayName: "Fixture" }));
  localStorage.setItem(`ferryx_remote_token_${view.relayOrigin}`, token);
  localStorage.setItem("ferryx_remote_token_alias", token);
  const { inventory, commands } = fixture(); await inventory.migrateLegacy();
  expect(commands.migrate).not.toHaveBeenCalled();
  expect(localStorage.getItem(`ferryx_remote_token_${view.relayOrigin}`)).toBe(token);
});
it.each(["inventory", "token"])("does not overwrite concurrent %s changes during migration", async kind => {
  legacy(); const { inventory, commands, store } = fixture(); const read = deferred<HostView>();
  const entered = deferred<void>();
  vi.mocked(commands.read).mockImplementation(() => { entered.resolve(); return read.promise; });
  const pending = inventory.migrateLegacy(); await entered.promise;
  const key = kind === "inventory" ? REMOTE_HOST_STORAGE_KEY : tokenKey;
  localStorage.setItem(key, kind === "inventory" ? JSON.stringify({ hosts: { newer: { machineId: "newer" } } }) : "newer-fixture-token");
  const updated = localStorage.getItem(key);
  read.resolve(view); await pending;
  expect(localStorage.getItem(key)).toBe(updated);
  expect(store.getState().migrationStatus).toBe("pending");
});
it("storage write failure retains original and scoped key", async () => {
  const original = legacy(); const { inventory, store } = fixture();
  const write = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("fixture quota"); });
  await inventory.migrateLegacy(); write.mockRestore();
  expect(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)).toBe(original);
  expect(localStorage.getItem(tokenKey)).toBe(token);
  expect(store.getState().migrationStatus).toBe("pending");
});
it("conflicting host-scoped credential copies are retained for explicit retry", async () => {
  const original = legacy(); localStorage.setItem(tokenKey, "different-fixture-token");
  const { inventory, commands, store } = fixture(); await inventory.migrateLegacy();
  expect(commands.migrate).not.toHaveBeenCalled();
  expect(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)).toBe(original);
  expect(localStorage.getItem(tokenKey)).toBe("different-fixture-token");
  expect(store.getState().migrationStatus).toBe("pending");
});
it("inventory readiness enables machine features regardless of the legacy proxy flag", async () => {
  const { inventory, commands, store } = fixture();
  vi.mocked(commands.capabilities).mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: false });
  await inventory.refresh();
  expect(store.getState().nativeStatus).toBe("ready");
  expect(store.getState().machineFeaturesEnabled).toBe(true);
  expect(store.getState().hosts[hostId].authStatus).toBe("paired");
});
it("refresh failure retains offline rows and unknown local capabilities disable machine features", async () => {
  const { inventory, store, commands } = fixture(); await inventory.refresh();
  inventory.setProjectsEnabled(true);
  expect(store.getState().machineFeaturesEnabled).toBe(true);
  vi.mocked(commands.capabilities).mockRejectedValue(new Error("unknown request"));
  await inventory.refresh();
  expect(store.getState().hosts[hostId].online).toBe(false);
  expect(store.getState().machineFeaturesEnabled).toBe(false);
  expect(store.getState().nativeStatus).toBe("unavailable");
});
it("out-of-order refresh cannot replace a newer response", async () => {
  const { inventory, store, commands } = fixture(); const old = deferred<HostView[]>();
  vi.mocked(commands.list).mockReturnValueOnce(old.promise);
  const first = inventory.refresh(); await inventory.refresh();
  old.resolve([{ ...view, generation: "8", displayLabel: "stale" }]); await first;
  expect(store.getState().hosts[hostId].generation).toBe("9");
});
it("generation fence cancels in-flight refresh from overwriting newer generation state", async () => {
  const { inventory, store, commands } = fixture();
  await inventory.refresh();
  expect(store.getState().hosts[hostId].generation).toBe("9");

  // In-flight refresh is started which will return generation "9"
  const pendingRefresh = deferred<HostView[]>();
  vi.mocked(commands.list).mockReturnValueOnce(pendingRefresh.promise);
  const refreshPromise = inventory.refresh();

  // Meanwhile, host is updated to newer generation "10" (e.g. via push or pair)
  store.upsertHost({ ...store.getState().hosts[hostId], generation: "10" });
  expect(store.getState().hosts[hostId].generation).toBe("10");

  // In-flight refresh completes with older generation "9"
  pendingRefresh.resolve([view]);
  await refreshPromise;

  // The generation fence must PREVENT older generation "9" from overwriting newer "10"!
  expect(store.getState().hosts[hostId].generation).toBe("10");
});
it("forget fences pending refresh and host callbacks, re-pair creates a new generation", async () => {
  const { inventory, store, commands } = fixture(); await inventory.refresh();
  const current = inventory.capture(hostId); const old = deferred<HostView[]>();
  vi.mocked(commands.list).mockReturnValueOnce(old.promise);
  const pending = inventory.refresh(); expect(await inventory.forget(hostId)).toBe(true);
  old.resolve([view]); await pending;
  expect(store.getState().hosts[hostId]).toBeUndefined(); expect(current()).toBe(false);
  const pairResult = await inventory.pair({ relayOrigin: view.relayOrigin, pin: "123456", displayLabel: "Fixture" });
  expect(pairResult.ok).toBe(true);
  expect(await inventory.pairBoolean({ relayOrigin: view.relayOrigin, pin: "123456", displayLabel: "Fixture" })).toBe(true);
  expect(store.getState().hosts[hostId].generation).toBe("10"); expect(current()).toBe(false);
});
it("re-pair fences a pending migration without deleting original credentials", async () => {
  const original = legacy(); const { inventory, commands } = fixture(); const read = deferred<HostView>(); const entered = deferred<void>();
  vi.mocked(commands.read).mockImplementation(() => { entered.resolve(); return read.promise; });
  const pending = inventory.migrateLegacy(); await entered.promise;
  await inventory.pair({ relayOrigin: view.relayOrigin, pin: "123456", displayLabel: "Fixture" });
  read.resolve(view); await pending;
  expect(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)).toBe(original);
});
it("native pair invokes PIN-only command and errors are sanitized", async () => {
  vi.mocked(invoke).mockResolvedValue(view);
  const request = { relayOrigin: view.relayOrigin, pin: "123456", displayLabel: "Fixture" };
  expect(await nativePairedHostCommands.pair(request)).toEqual(view);
  expect(invoke).toHaveBeenCalledWith("paired_host_pair", { request });
  vi.mocked(invoke).mockRejectedValue(new Error(token));
  await expect(nativePairedHostCommands.migrate({ ...request, machineId: "fixture", deviceToken: token })).rejects.toThrow("PAIRED_HOST_UNAVAILABLE");
});
it("subscribes app-wide to native inventory generation/online/auth change events and atomically updates entries", async () => {
  let eventHandler!: (event: { payload: any }) => void;
  vi.mocked(listen).mockImplementation(async (name, handler) => {
    if (name === "paired_host_inventory_changed") {
      eventHandler = handler as any;
    }
    return () => {};
  });

  const { inventory, store, commands } = fixture();
  await inventory.refresh();
  expect(store.getState().hosts[hostId].generation).toBe("9");
  expect(store.getState().hosts[hostId].online).toBe(true);

  // Subscribe app-wide
  await (inventory as any).subscribeAppWide();
  expect(listen).toHaveBeenCalledWith("paired_host_inventory_changed", expect.any(Function));

  // Native push: host generation advances to 11, goes offline, authStatus revoked
  eventHandler({
    payload: {
      type: "update",
      host: { ...view, generation: "11", online: false, authStatus: "revoked" },
    },
  });

  expect(store.getState().hosts[hostId].generation).toBe("11");
  expect(store.getState().hosts[hostId].online).toBe(false);
  expect(store.getState().hosts[hostId].authStatus).toBe("revoked");

  // In-flight refresh that was initiated before the push must NOT overwrite newer state
  const pending = deferred<HostView[]>();
  vi.mocked(commands.list).mockReturnValueOnce(pending.promise);
  const refreshPromise = inventory.refresh();
  // Old view has generation 9
  pending.resolve([view]);
  await refreshPromise;

  expect(store.getState().hosts[hostId].generation).toBe("11");
  expect(store.getState().hosts[hostId].online).toBe(false);
  expect(store.getState().hosts[hostId].authStatus).toBe("revoked");
});
it("R5-N4: authoritative revoke sets a generation barrier and stale lists cannot resurrect paired state", async () => {
  let eventHandler!: (event: { payload: any }) => void;
  vi.mocked(listen).mockImplementation(async (name, handler) => {
    if (name === "paired_host_inventory_changed") {
      eventHandler = handler as any;
    }
    return () => {};
  });
  const { inventory, store, commands } = fixture();
  await inventory.refresh();
  expect(store.getState().hosts[hostId].authStatus).toBe("paired");

  await (inventory as any).subscribeAppWide();

  // The authoritative daemon revoke carries the post-bump view: generation 10, revoked.
  eventHandler({
    payload: {
      type: "revoke",
      hostId,
      generation: "10",
      host: { ...view, generation: "10", authStatus: "revoked", online: false },
    },
  });
  expect(store.getState().hosts[hostId].authStatus).toBe("revoked");
  expect(store.getState().hosts[hostId].generation).toBe("10");

  // A refresh started after the revoke that still returns the stale cached
  // paired row (generation 9) must not resurrect paired state. Fail-safe is
  // rejection (the entry is dropped or stays revoked), never paired again.
  const staleList = deferred<HostView[]>();
  vi.mocked(commands.list).mockReturnValueOnce(staleList.promise);
  const refreshPromise = inventory.refresh();
  staleList.resolve([{ ...view, authStatus: "paired", online: true }]);
  await refreshPromise;
  const afterStale = store.getState().hosts[hostId];
  expect(afterStale?.authStatus ?? "revoked").toBe("revoked");
  expect(afterStale?.authStatus).not.toBe("paired");

  // A legitimately re-paired newer generation lifts the barrier.
  vi.mocked(commands.list).mockResolvedValueOnce([{ ...view, generation: "11", authStatus: "paired", online: true }]);
  await inventory.refresh();
  expect(store.getState().hosts[hostId].authStatus).toBe("paired");
  expect(store.getState().hosts[hostId].generation).toBe("11");
});
it("preserves structured error {code, message, details, retryable} on pair failure", async () => {
  const { inventory, commands } = fixture();
  const structuredError = {
    code: "PIN_EXPIRED",
    message: "The pairing PIN has expired",
    details: { expiredAt: 12345 },
    retryable: false,
  };
  vi.mocked(commands.pair).mockRejectedValue(structuredError);

  const result = await (inventory as any).pair({
    relayOrigin: view.relayOrigin,
    pin: "123456",
    displayLabel: "Fixture",
  });

  expect(result).toEqual({
    ok: false,
    error: expect.objectContaining({
      code: "PIN_EXPIRED",
      message: expect.stringContaining("expired"),
      retryable: false,
    }),
  });
});

it("pairing stores canonical custom relayOrigin and persists it across refresh and subsequent connections", async () => {
  const store = createRemoteHostStore();
  const customOrigin = "https://custom-relay.example.com";
  const customHostId = remoteHostKey(customOrigin, "fixture");
  const customView: HostView = {
    hostId: customHostId,
    relayOrigin: customOrigin,
    machineId: "fixture",
    displayLabel: "Custom Fixture",
    generation: "1",
    grantScope: "machine",
    authStatus: "paired",
    online: true,
  };
  const commands: PairedHostCommands = {
    list: vi.fn().mockResolvedValue([customView]),
    capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }),
    pair: vi.fn().mockResolvedValue(customView),
    forget: vi.fn().mockResolvedValue(undefined),
    migrate: vi.fn().mockResolvedValue({ hostId: customHostId, generation: "1" }),
    read: vi.fn().mockResolvedValue(customView),
  };
  const inventory = createPairedHostInventory(store, commands, localStorage);

  const pairResult = await inventory.pair({
    relayOrigin: customOrigin,
    pin: "654321",
    displayLabel: "Custom Fixture",
  });

  expect(pairResult.ok).toBe(true);
  expect(commands.pair).toHaveBeenCalledWith({
    relayOrigin: customOrigin,
    pin: "654321",
    displayLabel: "Custom Fixture",
  });

  // Host in store has custom relayOrigin and address
  const host = store.getState().hosts[customHostId];
  expect(host).toBeDefined();
  expect(host.relayOrigin).toBe(customOrigin);
  expect(host.address).toBe(customOrigin);
  expect(host.hostId).toBe(customHostId);

  // Subsequent refresh retains the custom relay origin from native list
  await inventory.refresh();
  const refreshedHost = store.getState().hosts[customHostId];
  expect(refreshedHost.relayOrigin).toBe(customOrigin);
  expect(refreshedHost.address).toBe(customOrigin);
});

it("normalizes relay origin and extracts origin and pin from invite links with #pair=", () => {
  expect(normalizeRelayOrigin("https://relay.checka.cc")).toBe("https://relay.checka.cc");
  expect(normalizeRelayOrigin("https://relay.checka.cc/")).toBe("https://relay.checka.cc");
  expect(normalizeRelayOrigin("http://localhost:43821")).toBe("http://localhost:43821");
  expect(normalizeRelayOrigin("http://127.0.0.1:43821/")).toBe("http://127.0.0.1:43821");
  expect(() => normalizeRelayOrigin("http://insecure-remote.com")).toThrow("INVALID_RELAY_ORIGIN");
  expect(() => normalizeRelayOrigin("https://relay.example.com/subpath")).toThrow("INVALID_RELAY_ORIGIN");

  const invite1 = parsePairingInvite("https://custom-relay.org/#pair=849201");
  expect(invite1).toEqual({ pin: "849201", relayOrigin: "https://custom-relay.org" });

  const invite2 = parsePairingInvite("https://relay.org/#pair=123456&relay=https%3A%2F%2Fother-relay.org");
  expect(invite2).toEqual({ pin: "123456", relayOrigin: "https://other-relay.org" });

  const invite3 = parsePairingInvite("http://localhost:43821/#pair=654321");
  expect(invite3).toEqual({ pin: "654321", relayOrigin: "http://localhost:43821" });

  expect(parsePairingInvite("123456")).toBeNull();
  expect(parsePairingInvite("")).toBeNull();
});

it("R3-N4: ignores stale revoke events with older generation than current store generation", async () => {
  const { inventory, store } = fixture();
  await inventory.refresh();
  store.upsertHost({ ...store.getState().hosts[hostId], generation: "10", authStatus: "paired", online: true });

  // Stale revoke event from generation 9 must be ignored
  await inventory.handleNativeEvent({
    type: "revoke",
    hostId: hostId,
    generation: "9",
  });
  expect(store.getState().hosts[hostId].authStatus).toBe("paired");
  expect(store.getState().hosts[hostId].online).toBe(true);

  // Authoritative revoke event for generation 10 must apply
  await inventory.handleNativeEvent({
    type: "revoke",
    hostId: hostId,
    generation: "10",
  });
  expect(store.getState().hosts[hostId].authStatus).toBe("revoked");
  expect(store.getState().hosts[hostId].online).toBe(false);
});

it("R3-N3: pair rejects revision mismatch if intervening change was a revoke even at matching generation", async () => {
  const { inventory, store, commands } = fixture();
  await inventory.refresh();
  // Simulate command returning paired host with generation 10
  vi.mocked(commands.pair).mockImplementation(async () => {
    // Intervening event changes store to revoked at same generation 10
    store.upsertHost({ ...store.getState().hosts[hostId], generation: "10", authStatus: "revoked", online: false });
    // And increments revision
    await inventory.handleNativeEvent({ type: "revoke", hostId, generation: "10" });
    return { ...view, generation: "10" };
  });

  const res = await inventory.pair({ relayOrigin: view.relayOrigin, pin: "123456", displayLabel: "Fixture" });
  expect(res.ok).toBe(false);
  if (!res.ok) {
    expect(res.error.code).toBe("STALE_HOST_GENERATION");
  }
});

it("R4-N1: migrateLegacy cleans legacy storage when native migrate event arrives during the call", async () => {
  legacy();
  const { inventory, store, commands } = fixture();
  // Simulate commands.migrate triggering native event before returning receipt
  vi.mocked(commands.migrate).mockImplementation(async () => {
    await inventory.handleNativeEvent({
      type: "migrate",
      hostId,
      generation: "9",
    });
    return { hostId, generation: "9" };
  });

  await inventory.migrateLegacy();
  // Legacy tokens cleaned from localStorage even though native event bumped revision
  expect(localStorage.getItem(tokenKey)).toBeNull();
  const raw = localStorage.getItem(REMOTE_HOST_STORAGE_KEY);
  expect(raw).not.toBeNull();
  expect(JSON.parse(raw!).hosts[hostId].deviceToken).toBeUndefined();
  expect(store.getState().hosts[hostId].authStatus).toBe("paired");
});



