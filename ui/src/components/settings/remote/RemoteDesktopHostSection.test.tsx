import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke, listeners } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true, Channel: class {} }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return () => listeners.delete(name);
  }),
}));

import { __resetRdSupportedForTests, type RdHostState } from "../../../lib/remoteDesktop";
import { RemoteDesktopConsentDialog, RemoteDesktopHostSection } from "./RemoteDesktopHostSection";

const baseState: RdHostState = {
  enabled: false, running: false, allowInput: false, allowClipboard: false, sessions: [], pending: [], error: null,
};

afterEach(() => {
  cleanup();
  invoke.mockReset();
  listeners.clear();
  __resetRdSupportedForTests();
});

describe("RemoteDesktopHostSection", () => {
  it("renders nothing when the build does not support remote desktop", async () => {
    invoke.mockImplementation(async (command: string) => (command === "cmd_rd_supported" ? false : baseState));
    render(<RemoteDesktopHostSection />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_supported"));
    expect(screen.queryByTestId("remote-desktop-host-section")).toBeNull();
  });

  it("shows toggles and sessions, and sends the full settings on change", async () => {
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === "cmd_rd_supported") return true;
      if (command === "cmd_rd_host_state") return { ...baseState, sessions: [{ sessionId: "s1", clientName: "Laptop" }] };
      if (command === "cmd_rd_host_set") return { ...baseState, ...args, running: true };
      return undefined;
    });
    render(<RemoteDesktopHostSection />);
    expect(await screen.findByText("Laptop")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Allow input" })).toBeDisabled();
    fireEvent.click(screen.getByRole("switch", { name: "Allow remote desktop" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_host_set", { enabled: true, allowInput: false, allowClipboard: false }));
    expect(await screen.findByText("Hosting is running.")).toBeInTheDocument();
  });
});

describe("RemoteDesktopConsentDialog", () => {
  it("answers a consent prompt with cmd_rd_host_respond", async () => {
    invoke.mockImplementation(async (command: string) => (command === "cmd_rd_supported" ? true : undefined));
    render(<RemoteDesktopConsentDialog />);
    await waitFor(() => expect(listeners.has("rd-host-consent")).toBe(true));
    act(() => listeners.get("rd-host-consent")?.({ payload: { requestId: "req-1", clientName: "Phone" } }));
    expect(await screen.findByText(/Phone wants to view this machine/)).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Allow" })); });
    expect(invoke).toHaveBeenCalledWith("cmd_rd_host_respond", { requestId: "req-1", approve: true });
    await waitFor(() => expect(screen.queryByText(/Phone wants/)).toBeNull());
  });
});
