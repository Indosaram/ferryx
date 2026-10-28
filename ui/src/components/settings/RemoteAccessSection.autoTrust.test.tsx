import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const getRemoteStatus = vi.fn();
const listRemoteDevices = vi.fn();
const enableRemoteGateway = vi.fn();
const disableRemoteGateway = vi.fn();
const revokeRemoteDevice = vi.fn();
const getDirectAutoTrustStatus = vi.fn();
const setDirectAutoTrustDisabled = vi.fn();

vi.mock("../../lib/tauri", () => ({
  // Agent control has its own suite; this fixture exercises direct trust only.
  getAgentControlSettings: () => Promise.resolve(null),
  getRemoteStatus: () => getRemoteStatus(),
  listRemoteDevices: () => listRemoteDevices(),
  enableRemoteGateway: (req: unknown) => enableRemoteGateway(req),
  disableRemoteGateway: () => disableRemoteGateway(),
  revokeRemoteDevice: (id: string) => revokeRemoteDevice(id),
  getDirectAutoTrustStatus: () => getDirectAutoTrustStatus(),
  setDirectAutoTrustDisabled: (disabled: boolean) => setDirectAutoTrustDisabled(disabled),
}));

const { RemoteAccessSection } = await import("./RemoteAccessSection");

const PINNED = "MCowBQYDK2VwAyEAq1w2e3r4t5y6u7i8o9p0a1s2d3f4g5h6j7k8l9z0x1c=";

const trustOn = { enabled: true, disabledBySetting: false, disabledByEnv: false, pinnedAccountKey: PINNED };
const trustOff = { ...trustOn, enabled: false, disabledBySetting: true };

describe("RemoteAccessSection automatic direct trust", () => {
  beforeEach(() => {
    getRemoteStatus.mockResolvedValue({
      enabled: true,
      mode: "localNetwork",
      port: 43821,
      boundAddress: null,
      localIp: "192.168.0.5",
      relayUrl: null,
    });
    listRemoteDevices.mockResolvedValue([]);
    getDirectAutoTrustStatus.mockResolvedValue(trustOn);
    setDirectAutoTrustDisabled.mockResolvedValue(trustOff);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows the auto-trust state and the pinned account key", async () => {
    render(<RemoteAccessSection />);

    const toggle = await screen.findByRole("switch", { name: "Automatic direct trust" });
    expect(toggle).toBeChecked();
    expect(screen.getByTestId("auto-trust-state")).toHaveTextContent(/^On:/);
    expect(screen.getByTestId("pinned-account-key")).toHaveTextContent(PINNED);
  });

  it("persists the disable choice through the backend and shows the returned state", async () => {
    render(<RemoteAccessSection />);

    const toggle = await screen.findByRole("switch", { name: "Automatic direct trust" });
    await act(async () => {
      fireEvent.click(toggle);
    });

    expect(setDirectAutoTrustDisabled).toHaveBeenCalledTimes(1);
    expect(setDirectAutoTrustDisabled).toHaveBeenCalledWith(true);
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(screen.getByTestId("auto-trust-state")).toHaveTextContent(/^Off:/);
  });

  it("re-enables auto-trust by persisting disabled=false", async () => {
    getDirectAutoTrustStatus.mockResolvedValue(trustOff);
    setDirectAutoTrustDisabled.mockResolvedValue(trustOn);
    render(<RemoteAccessSection />);

    const toggle = await screen.findByRole("switch", { name: "Automatic direct trust" });
    expect(toggle).not.toBeChecked();
    await act(async () => {
      fireEvent.click(toggle);
    });

    expect(setDirectAutoTrustDisabled).toHaveBeenCalledWith(false);
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it("names the environment override and shows no key when none is pinned", async () => {
    getDirectAutoTrustStatus.mockResolvedValue({
      enabled: false,
      disabledBySetting: false,
      disabledByEnv: true,
      pinnedAccountKey: null,
    });
    render(<RemoteAccessSection />);

    await screen.findByRole("switch", { name: "Automatic direct trust" });
    expect(screen.getByTestId("auto-trust-state")).toHaveTextContent("FERRYX_DIRECT_AUTO_TRUST");
    expect(screen.getByTestId("pinned-account-key")).toHaveTextContent("Pinned account key: None");
  });

  it("reports a failed save and keeps the previous state", async () => {
    setDirectAutoTrustDisabled.mockRejectedValue(new Error("settings store unwritable"));
    render(<RemoteAccessSection />);

    const toggle = await screen.findByRole("switch", { name: "Automatic direct trust" });
    await act(async () => {
      fireEvent.click(toggle);
    });

    expect(await screen.findByRole("alert")).toHaveTextContent("settings store unwritable");
    expect(toggle).toBeChecked();
  });
});
