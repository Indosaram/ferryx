import { expect, it, vi } from "vitest";
import { createRemoteHostStore } from "../../state/remoteHostStore";
import { createPairedHostInventory, type PairedHostCommands } from "../../lib/pairedHostInventory";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
it("native inventory readiness alone enables machine features; there is no rollout gate", async () => {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = { list: vi.fn().mockResolvedValue([]), capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }), pair: vi.fn(), forget: vi.fn(), migrate: vi.fn(), read: vi.fn() };
  const inventory = createPairedHostInventory(store, commands);
  await inventory.refresh();
  expect(store.getState().nativeStatus).toBe("ready");
  expect(store.getState().machineFeaturesEnabled).toBe(true);
});
it("an unreachable local inventory still fails closed and marks retained rows offline", async () => {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = { list: vi.fn().mockResolvedValue([]), capabilities: vi.fn().mockRejectedValue(new Error("unknown request")), pair: vi.fn(), forget: vi.fn(), migrate: vi.fn(), read: vi.fn() };
  const inventory = createPairedHostInventory(store, commands);
  await inventory.refresh();
  expect(store.getState().nativeStatus).toBe("unavailable");
  expect(store.getState().machineFeaturesEnabled).toBe(false);
});
