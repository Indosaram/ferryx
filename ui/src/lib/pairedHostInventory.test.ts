import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { createRemoteHostStore, remoteHostKey, REMOTE_HOST_STORAGE_KEY } from "../state/remoteHostStore";
import { createPairedHostInventory, nativePairedHostCommands, type HostView, type PairedHostCommands } from "./pairedHostInventory";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
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
it("forget fences pending refresh and host callbacks, re-pair creates a new generation", async () => {
  const { inventory, store, commands } = fixture(); await inventory.refresh();
  const current = inventory.capture(hostId); const old = deferred<HostView[]>();
  vi.mocked(commands.list).mockReturnValueOnce(old.promise);
  const pending = inventory.refresh(); expect(await inventory.forget(hostId)).toBe(true);
  old.resolve([view]); await pending;
  expect(store.getState().hosts[hostId]).toBeUndefined(); expect(current()).toBe(false);
  expect(await inventory.pair({ relayOrigin: view.relayOrigin, pin: "123456", displayLabel: "Fixture" })).toBe(true);
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
