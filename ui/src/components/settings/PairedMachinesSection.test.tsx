import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createRemoteHostStore, remoteHostKey } from "../../state/remoteHostStore";
import { createPairedHostInventory, DEFAULT_RELAY_ORIGIN, type HostView, type PairedHostCommands } from "../../lib/pairedHostInventory";
import { PairedMachinesSection } from "./PairedMachinesSection";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
const hostId = remoteHostKey("https://relay.example", "fixture");
const host: HostView = { hostId, machineId: "fixture", relayOrigin: "https://relay.example", displayLabel: "Fixture", generation: "9", authStatus: "paired", grantScope: "machine", online: true };
function fixture(overrides: Partial<HostView> = {}, proxy = true) {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = {
    list: vi.fn().mockResolvedValue([{ ...host, ...overrides }]),
    capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: proxy }),
    pair: vi.fn().mockResolvedValue({ ...host, generation: "10" }),
    forget: vi.fn().mockResolvedValue(undefined), migrate: vi.fn(), read: vi.fn(),
  };
  const inventory = createPairedHostInventory(store, commands, localStorage);
  return { store, commands, inventory };
}
beforeEach(() => { localStorage.clear(); vi.clearAllMocks(); });
afterEach(cleanup);
it("refresh preserves host selection, generation, and saved layout bytes", async () => {
  const { store, inventory } = fixture();
  await inventory.refresh();
  store.setActiveHost(hostId);
  localStorage.setItem("ferryx_workspace_layout", '{"target":{"kind":"daemon","hostId":"fixture"},"session":"original"}');
  const saved = localStorage.getItem("ferryx_workspace_layout");
  render(<PairedMachinesSection store={store} inventory={inventory} />);
  expect(store.getState().machineFeaturesEnabled).toBe(true);
  expect(screen.queryByRole("switch", { name: "Paired daemon projects" })).toBeNull();
  await act(() => inventory.refresh());
  expect(store.getState().machineFeaturesEnabled).toBe(true);
  expect(store.getState().activeHostId).toBe(hostId);
  expect(store.getState().hosts[hostId].generation).toBe("9");
  expect(localStorage.getItem("ferryx_workspace_layout")).toBe(saved);
});
it("requires explicit confirmation before native credential forget, and cancel preserves credentials", async () => {
  const { store, inventory, commands } = fixture(); await inventory.refresh();
  render(<PairedMachinesSection store={store} inventory={inventory} />);
  fireEvent.click(screen.getByRole("button", { name: "Forget Fixture" }));
  expect(commands.forget).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Cancel forget" }));
  expect(store.getState().hosts[hostId]).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Forget Fixture" }));
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Confirm forget Fixture" })); });
  expect(commands.forget).toHaveBeenCalledExactlyOnceWith({ hostId, generation: "9" });
  expect(store.getState().hosts[hostId]).toBeUndefined();
});
it.each([
  [{ grantScope: "mirror" }, true, "MACHINE_GRANT_REQUIRED"],
  [{ authStatus: "revoked" }, true, "MACHINE_GRANT_REQUIRED"],
] as const)("does not classify incompatible hosts as merely offline: %j", async (overrides, proxy, code) => {
  const { store, inventory } = fixture(overrides, proxy); await inventory.refresh();
  render(<PairedMachinesSection store={store} inventory={inventory} />);
  expect(screen.getByTestId("machine-status").getAttribute("data-code")).toBe(code);
  expect(screen.getByRole("button", { name: "Add Project on Fixture" })).toBeDisabled();
  expect(store.getState().hosts[hostId].online).toBe(true);
});
it("pairs and re-pairs only through native inventory and never selects a mirror host", async () => {
  const { store, inventory, commands } = fixture(); await inventory.refresh();
  render(<PairedMachinesSection store={store} inventory={inventory} />);
  fireEvent.click(screen.getByRole("button", { name: "Re-pair Fixture" }));
  fireEvent.change(screen.getByLabelText("Machine PIN"), { target: { value: "123456" } });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Pair machine" })); });
  expect(commands.pair).toHaveBeenCalledExactlyOnceWith({ relayOrigin: host.relayOrigin, displayLabel: "Fixture", pin: "123456" });
  expect(store.getState().activeHostId).toBeNull();
  expect(store.getState().hosts[hostId].generation).toBe("10");
  expect(screen.getByLabelText("Machine PIN")).toHaveValue("");
});
it("pairs with the PIN alone using the built-in relay and default label", async () => {
  const { store, inventory, commands } = fixture(); await inventory.refresh();
  render(<PairedMachinesSection store={store} inventory={inventory} />);
  fireEvent.change(screen.getByLabelText("Machine PIN"), { target: { value: "654321" } });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Pair machine" })); });
  expect(commands.pair).toHaveBeenCalledExactlyOnceWith({ relayOrigin: DEFAULT_RELAY_ORIGIN, displayLabel: "Machine", pin: "654321" });
});
it("fails closed without native inventory and exposes stale generation errors from capability negotiation", async () => {
  const { store, inventory } = fixture(); await inventory.refresh();
  const negotiate = vi.fn().mockRejectedValue(new Error("STALE_HOST_GENERATION"));
  render(<PairedMachinesSection store={store} inventory={inventory} negotiate={negotiate} />);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Check capabilities for Fixture" })); });
  expect(screen.getByTestId("machine-status").getAttribute("data-code")).toBe("STALE_HOST_GENERATION");
  act(() => store.setState(s => ({ ...s, nativeStatus: "unavailable", machineFeaturesEnabled: false })));
  expect(screen.getByTestId("machine-status").getAttribute("data-code")).toBe("NATIVE_CONTEXT_REQUIRED");
});
