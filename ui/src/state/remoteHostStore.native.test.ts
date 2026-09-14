import { beforeEach, expect, it, vi } from "vitest";
import { createRemoteHostStore, remoteHostKey, REMOTE_HOST_STORAGE_KEY } from "./remoteHostStore";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
const hostId = remoteHostKey("https://relay.example", "fixture-machine");
const legacy = { hostId, machineId: "fixture-machine", relayOrigin: "https://relay.example", name: "Fixture", displayName: "Fixture", address: "https://relay.example", transport: "relay" as const, authStatus: "unknown" as const, online: false, deviceToken: "private-fixture-bearer" };
beforeEach(() => localStorage.clear());
it("desktop never restores or serializes a bearer or treats it as authority", () => {
  localStorage.setItem(REMOTE_HOST_STORAGE_KEY, JSON.stringify({ hosts: { [hostId]: legacy } }));
  const store = createRemoteHostStore();
  expect(JSON.stringify(store.getState())).not.toContain(legacy.deviceToken);
  store.upsertHost(legacy);
  expect(store.getState().hosts[hostId]?.authStatus).not.toBe("paired");
  expect(JSON.stringify(store.getState())).not.toContain(legacy.deviceToken);
});
it("desktop token presence is not authentication authority", () => {
  const store = createRemoteHostStore();
  store.upsertHost(legacy);
  expect(store.getState().hosts[hostId]?.authStatus).toBe("unknown");
});
it("desktop ordinary changes preserve legacy migration input until native confirmation", () => {
  const original = JSON.stringify({ hosts: { [hostId]: legacy } });
  localStorage.setItem(REMOTE_HOST_STORAGE_KEY, original);
  const store = createRemoteHostStore();
  store.setDiscovering(true);
  expect(localStorage.getItem(REMOTE_HOST_STORAGE_KEY)).toBe(original);
});
it("desktop rejects an older host generation after re-pair", () => {
  const store = createRemoteHostStore();
  store.upsertHost({ ...legacy, deviceToken: null, generation: "9" });
  store.upsertHost({ ...legacy, deviceToken: null, generation: "8", name: "stale", displayName: "stale" });
  expect(store.getState().hosts[hostId]?.name).toBe("Fixture");
});
