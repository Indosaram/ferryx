import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { SkillsStep } from "./SkillsStep";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));
afterEach(() => { cleanup(); ipc.invoke.mockReset(); });

it("installs only detected built-in agents through integration IPC before completing", async () => {
  const installed = vi.fn();
  const base = { available: true, enabled: true, command: "claude", args: "", custom: false };
  ipc.invoke.mockResolvedValue({ enabled: true });
  render(<SkillsStep agents={[
    { ...base, name: "claude" },
    { ...base, name: "codex", available: false },
    { ...base, name: "custom", custom: true },
  ]} onInstalled={installed} />);
  await act(async () => { fireEvent.click(screen.getByRole("button")); });
  expect(ipc.invoke).toHaveBeenCalledWith("cmd_agent_integration_set", { enabled: true, agents: ["claude"] });
  expect(installed).toHaveBeenCalledOnce();
});

it("does not complete when integration installation fails", async () => {
  const installed = vi.fn();
  ipc.invoke.mockRejectedValue(new Error("disk full"));
  render(<SkillsStep agents={[{ name: "claude", available: true, enabled: true, command: "claude", args: "", custom: false }]} onInstalled={installed} />);
  await act(async () => { fireEvent.click(screen.getByRole("button")); });
  expect(installed).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toBe("disk full");
});
