import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";

const getRemoteStatus = vi.fn();
const listRemoteDevices = vi.fn();
const getDirectAutoTrustStatus = vi.fn();

vi.mock("../../lib/tauri", () => ({
  getRemoteStatus: () => getRemoteStatus(),
  listRemoteDevices: () => listRemoteDevices(),
  enableRemoteGateway: vi.fn(),
  disableRemoteGateway: vi.fn(),
  revokeRemoteDevice: vi.fn(),
  getDirectAutoTrustStatus: () => getDirectAutoTrustStatus(),
  setDirectAutoTrustDisabled: vi.fn(),
  getAgentControlSettings: () => Promise.resolve(null),
  setAgentControlSettings: vi.fn(),
}));

const { RemoteAccessSection } = await import("./RemoteAccessSection");

const CAVEATS = ["caveat-machine-grant", "caveat-auto-trust", "caveat-lan-http", "caveat-stun"];

describe("RemoteAccessSection security caveats", () => {
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
    getDirectAutoTrustStatus.mockResolvedValue(null);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows the four caveats with the env opt-outs at the point of pairing", async () => {
    render(<RemoteAccessSection />);

    const note = await screen.findByTestId("remote-access-caveats");
    expect(note).toHaveAttribute("role", "note");
    expect(within(note).getAllByRole("listitem")).toHaveLength(4);
    for (const id of CAVEATS) expect(within(note).getByTestId(id)).toBeInTheDocument();
    expect(within(screen.getByTestId("caveat-auto-trust")).getByText("FERRYX_DIRECT_AUTO_TRUST=0").tagName).toBe("CODE");
    expect(within(screen.getByTestId("caveat-stun")).getByText("FERRYX_STUN_SERVERS=off").tagName).toBe("CODE");
  });

  it("keeps the caveats out of the diagnostics-only view", async () => {
    render(<RemoteAccessSection detailsOnly />);

    expect(await screen.findByLabelText("Gateway diagnostics")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-access-caveats")).toBeNull();
  });
});
