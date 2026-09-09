import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The vitest config supplies a jsdom environment; `bun test` does not. Install one here so a
// single test file is runnable by either runner instead of silently failing on `document`.
if (typeof globalThis.document === "undefined") {
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
  const globals = globalThis as unknown as Record<string, unknown>;
  const domWindow = dom.window as unknown as Record<string, unknown>;
  globals.window = dom.window;
  globals.document = dom.window.document;
  for (const key of Object.getOwnPropertyNames(dom.window)) {
    if (key in globals) continue;
    const value = domWindow[key];
    globals[key] = typeof value === "function" ? value.bind(dom.window) : value;
  }
  globals.navigator = dom.window.navigator;
  globals.PointerEvent = dom.window.MouseEvent;
  globals.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

const { cleanup, fireEvent, render, screen, waitFor } = await import("@testing-library/react");
await import("@testing-library/jest-dom/vitest");

const getRemoteStatus = vi.fn();
const listRemoteDevices = vi.fn();
const getTailscaleStatus = vi.fn();
const createPairingCode = vi.fn();
const enableRemoteGateway = vi.fn();
const disableRemoteGateway = vi.fn();
const revokeRemoteDevice = vi.fn();
const toDataURL = vi.fn();

vi.mock("../../lib/tauri", () => ({
  getRemoteStatus: () => getRemoteStatus(),
  listRemoteDevices: () => listRemoteDevices(),
  getTailscaleStatus: () => getTailscaleStatus(),
  createPairingCode: (perm?: "view" | "control") => createPairingCode(perm),
  enableRemoteGateway: (req: unknown) => enableRemoteGateway(req),
  disableRemoteGateway: () => disableRemoteGateway(),
  revokeRemoteDevice: (id: string) => revokeRemoteDevice(id),
}));

vi.mock("qrcode", () => ({
  default: { toDataURL: (url: string) => toDataURL(url) },
}));

const { RemoteAccessSection, buildPairingUrl } = await import("./RemoteAccessSection");

const enabledStatus = {
  enabled: true,
  mode: "localNetwork" as const,
  port: 43821,
  boundAddress: null,
  localIp: "192.168.0.5",
  relayUrl: null,
};

const disabledStatus = { ...enabledStatus, enabled: false, mode: "off" as const };

const relayStatus = {
  ...enabledStatus,
  mode: "relay" as const,
  relayUrl: "https://relay.ferryx.dev",
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
    toDataURL.mockResolvedValue("data:image/png;base64,QR");
  });

  afterEach(cleanup);

  it("does not probe for or display Tailscale detection", async () => {
    render(<RemoteAccessSection />);

    await screen.findByRole("button", { name: "Generate QR Code" });

    expect(getTailscaleStatus).not.toHaveBeenCalled();
    expect(screen.queryByText("Tailscale Status")).toBeNull();
    expect(screen.queryByText(/not installed/i)).toBeNull();
  });

  it("exposes a single Remote Access master switch with no per-mode selector", async () => {
    render(<RemoteAccessSection />);

    const switches = await screen.findAllByRole("switch");
    expect(switches).toHaveLength(1);
    expect(switches[0]).toHaveAccessibleName("Remote Access");
    expect(screen.queryByRole("radiogroup")).toBeNull();
    expect(screen.queryByRole("radio", { name: "Tailscale" })).toBeNull();
  });

  it("renders a relay / signaling server URL field with the default relay placeholder", async () => {
    render(<RemoteAccessSection />);

    const field = await screen.findByLabelText("Relay / Signaling Server URL");
    expect(field).toHaveAttribute("placeholder", "https://relay.ferryx.dev");

    fireEvent.change(field, { target: { value: "https://relay.example.com" } });
    expect(field).toHaveValue("https://relay.example.com");
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
    await screen.findByAltText("Pairing QR Code");
  });

  it("encodes the relay URL, PIN and direct hints into one universal QR link", async () => {
    getRemoteStatus.mockResolvedValue(relayStatus);

    render(<RemoteAccessSection />);

    fireEvent.click(await screen.findByRole("button", { name: "Generate QR Code" }));

    await waitFor(() => expect(toDataURL).toHaveBeenCalled());
    const expected = `https://relay.ferryx.dev/#pair=123456&hints=${encodeURIComponent(
      "http://192.168.0.5:43821",
    )}`;
    expect(toDataURL).toHaveBeenCalledWith(expected);
    expect(await screen.findByTestId("pairing-url")).toHaveTextContent(expected);
  });

  it("falls back to the host address link when no relay is configured", async () => {
    render(<RemoteAccessSection />);

    fireEvent.click(await screen.findByRole("button", { name: "Generate QR Code" }));

    await waitFor(() => expect(toDataURL).toHaveBeenCalled());
    expect(toDataURL).toHaveBeenCalledWith("http://192.168.0.5:43821/#pair=123456");
  });

  it("builds relay and fallback pairing URLs from status and typed relay URL", () => {
    expect(buildPairingUrl(enabledStatus, "", "111222")).toBe(
      "http://192.168.0.5:43821/#pair=111222",
    );
    expect(buildPairingUrl(null, "", "111222")).toBe("http://localhost:43821/#pair=111222");
    expect(buildPairingUrl(enabledStatus, "https://relay.example.com/", "111222")).toBe(
      `https://relay.example.com/#pair=111222&hints=${encodeURIComponent(
        "http://192.168.0.5:43821",
      )}`,
    );
    expect(buildPairingUrl(relayStatus, "https://ignored.example.com", "111222")).toContain(
      "https://relay.ferryx.dev/#pair=111222",
    );
  });

  it("sends the typed relay URL when enabling remote access", async () => {
    getRemoteStatus.mockResolvedValue(disabledStatus);

    render(<RemoteAccessSection />);

    const field = await screen.findByLabelText("Relay / Signaling Server URL");
    fireEvent.change(field, { target: { value: "https://relay.example.com" } });
    fireEvent.click(screen.getByRole("switch", { name: "Remote Access" }));

    await waitFor(() => {
      expect(enableRemoteGateway).toHaveBeenCalledWith({
        mode: "relay",
        relayUrl: "https://relay.example.com",
      });
    });
  });

  it("enables local-network mode when the relay URL is left empty", async () => {
    getRemoteStatus.mockResolvedValue(disabledStatus);

    render(<RemoteAccessSection />);

    fireEvent.click(await screen.findByRole("switch", { name: "Remote Access" }));

    await waitFor(() => {
      expect(enableRemoteGateway).toHaveBeenCalledWith({
        mode: "localNetwork",
        relayUrl: undefined,
      });
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

  it("revokes a paired device after confirmation", async () => {
    listRemoteDevices.mockResolvedValue([
      {
        id: "dev-test-2",
        name: "Kitchen iPad",
        permission: "view",
        createdAt: Date.now(),
        lastSeenAt: 0,
        revoked: false,
      },
    ]);

    render(<RemoteAccessSection />);

    fireEvent.click(await screen.findByRole("button", { name: "Revoke device Kitchen iPad" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm revoke Kitchen iPad" }));

    await waitFor(() => expect(revokeRemoteDevice).toHaveBeenCalledWith("dev-test-2"));
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
