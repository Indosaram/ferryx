import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createRemoteHostStore, remoteHostKey } from "../../state/remoteHostStore";
import { createPairedHostInventory, type HostView, type PairedHostCommands } from "../../lib/pairedHostInventory";
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
it("rollback disables the existing flag and preserves host selection and saved layout bytes across refresh", async () => {
  const { store, inventory } = fixture();
  await inventory.refresh();
  inventory.setProjectsEnabled(true);
  store.setActiveHost(hostId);
  localStorage.setItem("ferryx_workspace_layout", '{"target":{"kind":"daemon","hostId":"fixture"},"session":"original"}');
  const saved = localStorage.getItem("ferryx_workspace_layout");
  render(<PairedMachinesSection store={store} inventory={inventory} />);
  expect(store.getState().machineFeaturesEnabled).toBe(true);
  fireEvent.click(screen.getByRole("switch", { name: "Paired daemon projects" }));
  await act(() => inventory.refresh());
  expect(store.getState().machineFeaturesEnabled).toBe(false);
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
  [{}, false, "UNSUPPORTED_CAPABILITY"],
] as const)("does not classify incompatible hosts as merely offline: %j", async (overrides, proxy, code) => {
  const { store, inventory } = fixture(overrides, proxy); await inventory.refresh(); inventory.setProjectsEnabled(true);
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
it("fails closed without native inventory and exposes stale generation errors from capability negotiation", async () => {
  const { store, inventory } = fixture(); await inventory.refresh(); inventory.setProjectsEnabled(true);
  const negotiate = vi.fn().mockRejectedValue(new Error("STALE_HOST_GENERATION"));
  render(<PairedMachinesSection store={store} inventory={inventory} negotiate={negotiate} />);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Check capabilities for Fixture" })); });
  expect(screen.getByTestId("machine-status").getAttribute("data-code")).toBe("STALE_HOST_GENERATION");
  act(() => store.setState(s => ({ ...s, nativeStatus: "unavailable", machineFeaturesEnabled: false })));
  expect(screen.getByTestId("machine-status").getAttribute("data-code")).toBe("NATIVE_CONTEXT_REQUIRED");
});
