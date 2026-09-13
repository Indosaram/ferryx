import { expect, it, vi } from "vitest";
import { createRemoteHostStore } from "../../state/remoteHostStore";
import { createPairedHostInventory, type PairedHostCommands } from "../../lib/pairedHostInventory";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
it("pairedDaemonProjectsV1 defaults off even when the proxy is advertised", async () => {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = { list: vi.fn().mockResolvedValue([]), capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }), pair: vi.fn(), forget: vi.fn(), migrate: vi.fn(), read: vi.fn() };
  const inventory = createPairedHostInventory(store, commands);
  await inventory.refresh();
  expect(store.getState().machineFeaturesEnabled).toBe(false);
});
it("persisted rollout intent cannot bypass absent proxy support and rollback survives inventory recreation", async () => {
  localStorage.clear();
  const commands: PairedHostCommands = { list: vi.fn().mockResolvedValue([]), capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: false }), pair: vi.fn(), forget: vi.fn(), migrate: vi.fn(), read: vi.fn() };
  const store = createRemoteHostStore();
  const inventory = createPairedHostInventory(store, commands, localStorage);
  await inventory.refresh(); inventory.setProjectsEnabled(true);
  expect(store.getState().machineFeaturesEnabled).toBe(false);
  expect(localStorage.getItem("pairedDaemonProjectsV1")).toBe("true");
  inventory.setProjectsEnabled(false);
  vi.mocked(commands.capabilities).mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true });
  const restarted = createPairedHostInventory(store, commands, localStorage);
  await restarted.refresh();
  expect(restarted.getProjectsEnabled()).toBe(false);
  expect(store.getState().machineFeaturesEnabled).toBe(false);
  localStorage.clear();
});
