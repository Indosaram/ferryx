import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getRemoteStatus: vi.fn(),
  listRemoteDevices: vi.fn(),
  getAgentControlSettings: vi.fn(),
  setAgentControlSettings: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  getRemoteStatus: () => tauriMocks.getRemoteStatus(),
  listRemoteDevices: () => tauriMocks.listRemoteDevices(),
  enableRemoteGateway: vi.fn(),
  disableRemoteGateway: vi.fn(),
  revokeRemoteDevice: vi.fn(),
  getAgentControlSettings: () => tauriMocks.getAgentControlSettings(),
  setAgentControlSettings: (request: unknown) => tauriMocks.setAgentControlSettings(request),
}));

import { RemoteAccessSection } from "./RemoteAccessSection";

const record = (index: number) => ({
  deviceId: "dev-phone",
  op: "send",
  sessionId: `pane-${index}`,
  requestId: null,
  code: "OK",
  atMs: 1_700_000_000_000 + index,
  text: `echo ${index}`,
});

describe("RemoteAccessSection agent control card", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getRemoteStatus.mockResolvedValue({
      enabled: true,
      mode: "localNetwork",
      port: 43821,
      boundAddress: null,
      localIp: "192.168.0.5",
      relayUrl: null,
    });
    tauriMocks.listRemoteDevices.mockResolvedValue([
      { id: "dev-phone", name: "Phone", permission: "control", createdAt: 0, lastSeenAt: 0, revoked: false },
      { id: "dev-tv", name: "TV", permission: "view", createdAt: 0, lastSeenAt: 0, revoked: false },
    ]);
  });

  afterEach(() => cleanup());

  it("the toggle calls the set command and the card renders the audit list", async () => {
    tauriMocks.getAgentControlSettings.mockResolvedValue({
      enabled: false,
      excludedDeviceIds: [],
      audit: Array.from({ length: 60 }, (_, index) => record(index)),
    });
    tauriMocks.setAgentControlSettings.mockResolvedValue({
      enabled: true,
      excludedDeviceIds: [],
      audit: [record(1)],
    });

    await act(async () => {
      render(<RemoteAccessSection />);
    });

    const card = await screen.findByRole("region", { name: "Agent control" });
    expect(within(card).getByText("Lets paired devices with a Control grant type into your terminals")).toBeInTheDocument();
    const audit = within(card).getByRole("list", { name: "Agent control audit" });
    expect(within(audit).getAllByRole("listitem")).toHaveLength(50);
    expect(audit).toHaveTextContent("pane-0");

    const toggle = within(card).getByRole("switch", { name: "Agent control" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);

    await waitFor(() =>
      expect(tauriMocks.setAgentControlSettings).toHaveBeenCalledWith({ enabled: true, excludedDeviceIds: [] }),
    );
    await waitFor(() => expect(within(card).getByRole("switch", { name: "Agent control" })).toBeChecked());
    const refreshed = within(card).getByRole("list", { name: "Agent control audit" });
    expect(within(refreshed).getAllByRole("listitem")).toHaveLength(1);
    expect(refreshed).toHaveTextContent('"echo 1"');
  });

  it("excludes one Control device and leaves view-only devices out of the list", async () => {
    tauriMocks.getAgentControlSettings.mockResolvedValue({ enabled: true, excludedDeviceIds: [], audit: [] });
    tauriMocks.setAgentControlSettings.mockResolvedValue({ enabled: true, excludedDeviceIds: ["dev-phone"], audit: [] });

    await act(async () => {
      render(<RemoteAccessSection />);
    });

    const card = await screen.findByRole("region", { name: "Agent control" });
    const devices = within(card).getByRole("list", { name: "Agent control devices" });
    expect(devices).not.toHaveTextContent("TV");
    fireEvent.click(within(devices).getByRole("button", { name: "Exclude agent control for Phone" }));

    await waitFor(() =>
      expect(tauriMocks.setAgentControlSettings).toHaveBeenCalledWith({ enabled: true, excludedDeviceIds: ["dev-phone"] }),
    );
    expect(await within(card).findByRole("button", { name: "Allow agent control for Phone" })).toBeInTheDocument();
  });

  it("shows the save error and keeps the switch where it was", async () => {
    tauriMocks.getAgentControlSettings.mockResolvedValue({ enabled: false, excludedDeviceIds: [], audit: [] });
    tauriMocks.setAgentControlSettings.mockRejectedValue(new Error("disk full"));

    await act(async () => {
      render(<RemoteAccessSection />);
    });

    const card = await screen.findByRole("region", { name: "Agent control" });
    fireEvent.click(within(card).getByRole("switch", { name: "Agent control" }));
    expect(await within(card).findByRole("alert")).toHaveTextContent("disk full");
    expect(within(card).getByRole("switch", { name: "Agent control" })).not.toBeChecked();
  });
});
