import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteAccessSection } from "./RemoteAccessSection";

const getRemoteStatus = vi.fn();
const listRemoteDevices = vi.fn();
const getTailscaleStatus = vi.fn();
const createPairingCode = vi.fn();
const enableRemoteGateway = vi.fn();
const disableRemoteGateway = vi.fn();
const revokeRemoteDevice = vi.fn();

vi.mock("../../lib/tauri", () => ({
  getRemoteStatus: () => getRemoteStatus(),
  listRemoteDevices: () => listRemoteDevices(),
  getTailscaleStatus: () => getTailscaleStatus(),
  createPairingCode: (perm?: "view" | "control") => createPairingCode(perm),
  enableRemoteGateway: (req: unknown) => enableRemoteGateway(req),
  disableRemoteGateway: () => disableRemoteGateway(),
  revokeRemoteDevice: (id: string) => revokeRemoteDevice(id),
}));

const enabledStatus = {
  enabled: true,
  port: 43821,
  localIp: "192.168.0.5",
};

const disabledStatus = {
  enabled: false,
  port: 43821,
  localIp: "192.168.0.5",
};

describe("RemoteAccessSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getRemoteStatus.mockResolvedValue(enabledStatus);
    listRemoteDevices.mockResolvedValue([]);
    getTailscaleStatus.mockResolvedValue({
      running: false,
      installed: false,
      selfDns: null,
      tailnetName: null,
    });
    createPairingCode.mockResolvedValue({ code: "123456", expiresInSeconds: 300 });
    enableRemoteGateway.mockResolvedValue(enabledStatus);
    disableRemoteGateway.mockResolvedValue(disabledStatus);
    revokeRemoteDevice.mockResolvedValue(true);
  });

  afterEach(cleanup);

  it("does not probe for or display Tailscale detection", async () => {
    render(<RemoteAccessSection />);

    await screen.findByRole("button", { name: "Generate QR Code" });

    expect(getTailscaleStatus).not.toHaveBeenCalled();
    expect(screen.queryByText("Tailscale Status")).toBeNull();
    expect(screen.queryByText(/not installed/i)).toBeNull();
  });

  // Test A (C7): render the section, await the status settle, and assert createPairingCode was NOT called.
  it("does not automatically mint a pairing code when mounting with remote access enabled", async () => {
    render(<RemoteAccessSection />);

    const button = await screen.findByRole("button", { name: "Generate QR Code" });
    expect(button).toBeInTheDocument();
    expect(getRemoteStatus).toHaveBeenCalledTimes(1);
    expect(createPairingCode).not.toHaveBeenCalled();
  });

  // Test B (C7): click the "Generate QR Code" button and assert createPairingCode WAS called.
  it("creates a pairing code when the user explicitly clicks Generate QR Code", async () => {
    render(<RemoteAccessSection />);

    const button = await screen.findByRole("button", { name: "Generate QR Code" });
    fireEvent.click(button);

    await waitFor(() => {
      expect(createPairingCode).toHaveBeenCalledWith("control");
    });
  });

  // Test C (C8a): make enableRemoteGateway reject with new Error("gateway boom"), start from a disabled status, toggle the Remote Access switch on, and assert a node with role="alert" containing "gateway boom" appears.
  it("displays an alert when enabling the remote gateway fails", async () => {
    getRemoteStatus.mockResolvedValue(disabledStatus);
    enableRemoteGateway.mockRejectedValue(new Error("gateway boom"));

    render(<RemoteAccessSection />);

    const toggle = await screen.findByRole("switch", { name: "Remote Access" });
    expect(toggle).not.toBeChecked();

    fireEvent.click(toggle);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("gateway boom");
  });

  it("displays an alert when revoking a device fails", async () => {
    listRemoteDevices.mockResolvedValue([
      {
        id: "dev-test-1",
        name: "Test Phone",
        permission: "control",
        createdAt: Date.now(),
        lastSeenAt: Date.now(),
        revoked: false,
      },
    ]);
    revokeRemoteDevice.mockRejectedValue(new Error("revoke boom"));

    render(<RemoteAccessSection />);

    const revokeBtn = await screen.findByRole("button", { name: "Revoke device Test Phone" });
    fireEvent.click(revokeBtn);

    const confirmBtn = await screen.findByRole("button", { name: "Confirm revoke Test Phone" });
    fireEvent.click(confirmBtn);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("revoke boom");
  });

  it("does not automatically mint a pairing code when toggling remote access on", async () => {
    getRemoteStatus.mockResolvedValueOnce(disabledStatus).mockResolvedValue(enabledStatus);
    enableRemoteGateway.mockResolvedValue(enabledStatus);

    render(<RemoteAccessSection />);

    const toggle = await screen.findByRole("switch", { name: "Remote Access" });
    expect(toggle).not.toBeChecked();

    fireEvent.click(toggle);

    await screen.findByRole("button", { name: "Generate QR Code" });
    expect(enableRemoteGateway).toHaveBeenCalled();
    expect(createPairingCode).not.toHaveBeenCalled();
  });
});
