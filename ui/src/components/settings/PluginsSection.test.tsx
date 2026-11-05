import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PluginsSection } from "./PluginsSection";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("keeps the reviewed identity when replacement approval is rejected", async () => {
  const manifest = { name: "clock", version: "1", module: "/plugins/clock.wasm", sha256: "a".repeat(64), capabilities: ["render"] };
  let discovered!: () => void;
  const discovery = new Promise<void>((resolve) => { discovered = resolve; });
  bridge.invoke.mockImplementation((_command, args) => {
    if (args?.enabled) return Promise.reject({ code: "PLUGIN_APPROVAL_REQUIRED" });
    discovered();
    return Promise.resolve([{ manifest, enabled: false }]);
  });
  render(<PluginsSection />);
  await act(async () => { await discovery; });
  await act(async () => { fireEvent.click(screen.getByRole("switch")); });
  expect(bridge.invoke).toHaveBeenLastCalledWith("cmd_plugins", { name: "clock", enabled: true, expected: manifest });
  expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  expect(screen.getByRole("alert")).toBeInTheDocument();
});

it("does not enable on discovery and sends explicit consent only on owner action", async () => {
  const manifest = { name: "clock", version: "1", module: "/plugins/clock.wasm", sha256: "a".repeat(64), capabilities: ["render"] };
  let discovered!: () => void;
  const discovery = new Promise<void>((resolve) => { discovered = resolve; });
  bridge.invoke.mockImplementation((_command, args) => {
    discovered();
    return Promise.resolve([{ manifest, enabled: args?.enabled ?? false }]);
  });
  render(<PluginsSection />);
  await act(async () => { await discovery; });
  expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("cmd_plugins");
  const toggle = screen.getByRole("switch");
  expect(toggle).toHaveAttribute("aria-checked", "false");
  await act(async () => { fireEvent.click(toggle); });
  expect(bridge.invoke).toHaveBeenLastCalledWith("cmd_plugins", { name: "clock", enabled: true, expected: manifest });
  expect(toggle).toHaveAttribute("aria-checked", "true");
  await act(async () => { fireEvent.click(toggle); });
  expect(bridge.invoke).toHaveBeenLastCalledWith("cmd_plugins", { name: "clock", enabled: false, expected: null });
});
