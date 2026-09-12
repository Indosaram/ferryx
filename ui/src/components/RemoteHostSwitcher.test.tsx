import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pairRemoteMachine } from "../lib/pairClient";
import { remoteHostKey, remoteHostStore } from "../state/remoteHostStore";
import { RemoteHostSwitcher } from "./RemoteHostSwitcher";
import { PairMachineModal } from "./PairMachineModal";

describe("RemoteHostSwitcher", () => {
  beforeEach(() => {
    localStorage.clear();
    remoteHostStore.setHosts([]);
    remoteHostStore.setActiveHost(null);
    vi.restoreAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders trigger with 'Local Machine' as default active host", () => {
    render(<RemoteHostSwitcher />);

    const trigger = screen.getByTestId("remote-host-switcher-trigger");
    expect(trigger).toHaveTextContent("Local Machine");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("opens menu when trigger is clicked, showing Local Machine and Pair button", () => {
    render(<RemoteHostSwitcher />);

    const trigger = screen.getByTestId("remote-host-switcher-trigger");
    fireEvent.click(trigger);

    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("remote-host-switcher-menu")).toBeInTheDocument();
    expect(screen.getByTestId("remote-host-option-local")).toBeInTheDocument();
    expect(screen.getByText("No remote hosts discovered yet.")).toBeInTheDocument();
    expect(screen.getByTestId("pair-remote-machine-button")).toBeInTheDocument();
  });

  it("displays hosts from remoteHostStore and switches active host on click", () => {
    remoteHostStore.upsertHost({
      hostId: "https://relay.checka.cc/host/worker-1",
      machineId: "worker-1",
      name: "Worker 1",
      address: "https://relay.checka.cc",
      relayOrigin: "https://relay.checka.cc",
      transport: "relay",
      authStatus: "paired",
      online: true,
      lastSeenAt: Date.now(),
      directHints: [],
    });

    render(<RemoteHostSwitcher />);

    const trigger = screen.getByTestId("remote-host-switcher-trigger");
    fireEvent.click(trigger);

    const option = screen.getByTestId("remote-host-option-https://relay.checka.cc/host/worker-1");
    expect(option).toBeInTheDocument();
    expect(option).toHaveTextContent("Worker 1");

    fireEvent.click(option);

    expect(remoteHostStore.getState().activeHostId).toBe("https://relay.checka.cc/host/worker-1");
    expect(screen.queryByTestId("remote-host-switcher-menu")).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-host-switcher-trigger")).toHaveTextContent("Worker 1");
  });

  it("can switch back to Local Machine", () => {
    remoteHostStore.upsertHost({
      hostId: "https://relay.checka.cc/host/worker-1",
      machineId: "worker-1",
      name: "Worker 1",
      address: "https://relay.checka.cc",
      relayOrigin: "https://relay.checka.cc",
      transport: "relay",
      authStatus: "paired",
      online: true,
      lastSeenAt: Date.now(),
      directHints: [],
    });
    remoteHostStore.setActiveHost("https://relay.checka.cc/host/worker-1");

    render(<RemoteHostSwitcher />);

    expect(screen.getByTestId("remote-host-switcher-trigger")).toHaveTextContent("Worker 1");

    fireEvent.click(screen.getByTestId("remote-host-switcher-trigger"));
    fireEvent.click(screen.getByTestId("remote-host-option-local"));

    expect(remoteHostStore.getState().activeHostId).toBeNull();
    expect(screen.getByTestId("remote-host-switcher-trigger")).toHaveTextContent("Local Machine");
  });

  it("opens PairMachineModal when 'Pair Remote Machine...' button is clicked", () => {
    render(<RemoteHostSwitcher />);

    // Open switcher popover
    fireEvent.click(screen.getByTestId("remote-host-switcher-trigger"));

    // Click Pair Remote Machine button
    const pairBtn = screen.getByTestId("pair-remote-machine-button");
    fireEvent.click(pairBtn);

    // Switcher popover should close and modal should be open
    expect(screen.queryByTestId("remote-host-switcher-menu")).not.toBeInTheDocument();
    expect(screen.getByTestId("pair-machine-modal")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Pair Remote Machine" })).toBeInTheDocument();
    expect(screen.getByText(/ferryx pair generate/)).toBeInTheDocument();
  });
});

describe("PairMachineModal", () => {
  beforeEach(() => {
    localStorage.clear();
    remoteHostStore.setHosts([]);
    remoteHostStore.setActiveHost(null);
    vi.restoreAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("does not render when open is false", () => {
    render(<PairMachineModal open={false} onClose={vi.fn()} />);
    expect(screen.queryByTestId("pair-machine-modal")).not.toBeInTheDocument();
  });

  it("closes when Cancel button is clicked", () => {
    const onClose = vi.fn();
    render(<PairMachineModal open={true} onClose={onClose} />);

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when Escape key is pressed", () => {
    const onClose = vi.fn();
    render(<PairMachineModal open={true} onClose={onClose} />);

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("displays error message if pairing fails", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(JSON.stringify({ message: "Invalid or expired PIN" }), {
        status: 400,
        headers: { "Content-Type": "application/json" },
      }),
    );

    const onClose = vi.fn();
    render(<PairMachineModal open={true} onClose={onClose} />);

    const input = screen.getByTestId("pair-machine-input");
    fireEvent.change(input, { target: { value: "999999" } });

    const submitBtn = screen.getByTestId("pair-machine-submit");
    await act(async () => {
      fireEvent.click(submitBtn);
    });

    expect(fetchSpy).toHaveBeenCalledWith(
      "https://relay.checka.cc/api/v1/pair/exchange",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ code: "999999", deviceName: "Ferryx Desktop" }),
      }),
    );

    expect(screen.getByRole("alert")).toHaveTextContent("Invalid or expired PIN");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("pairs successfully with PIN, updates store, and closes modal", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          token: "secret-token-123",
          machineId: "mach-abc",
          displayName: "Ubuntu Server",
        }),
        {
          status: 200,
          headers: { "Content-Type": "application/json" },
        },
      ),
    );

    const onClose = vi.fn();
    render(<PairMachineModal open={true} onClose={onClose} />);

    const input = screen.getByTestId("pair-machine-input");
    fireEvent.change(input, { target: { value: "123456" } });

    const submitBtn = screen.getByTestId("pair-machine-submit");
    await act(async () => {
      fireEvent.click(submitBtn);
    });

    expect(fetchSpy).toHaveBeenCalledWith(
      "https://relay.checka.cc/api/v1/pair/exchange",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ code: "123456", deviceName: "Ferryx Desktop" }),
      }),
    );

    const expectedHostKey = remoteHostKey("https://relay.checka.cc", "mach-abc");
    const storedHost = remoteHostStore.getState().hosts[expectedHostKey];
    expect(storedHost).toBeDefined();
    expect(storedHost?.name).toBe("Ubuntu Server");
    expect(storedHost?.deviceToken).toBe("secret-token-123");
    expect(storedHost?.authStatus).toBe("paired");
    expect(remoteHostStore.getState().activeHostId).toBe(expectedHostKey);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("pairRemoteMachine", () => {
  beforeEach(() => {
    remoteHostStore.setHosts([]);
    remoteHostStore.setActiveHost(null);
    vi.restoreAllMocks();
  });

  it("parses full URL with fragment and exchanges with extracted relay origin and token", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          token: "token-url-test",
          machineId: "machine-url-test",
          displayName: "Cloud VPS",
        }),
        {
          status: 200,
          headers: { "Content-Type": "application/json" },
        },
      ),
    );

    const result = await pairRemoteMachine({
      codeOrUrl: "https://customrelay.example.com/#pair=tok123&hints=lan",
    });

    expect(fetchSpy).toHaveBeenCalledWith(
      "https://customrelay.example.com/api/v1/pair/exchange",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ code: "tok123", deviceName: "Ferryx Desktop" }),
      }),
    );

    expect(result.displayName).toBe("Cloud VPS");
    expect(result.machineId).toBe("machine-url-test");
    const expectedKey = remoteHostKey("https://customrelay.example.com", "machine-url-test");
    expect(result.hostId).toBe(expectedKey);
    expect(remoteHostStore.getState().activeHostId).toBe(expectedKey);
  });

  it("throws when response is missing token or machineId", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(JSON.stringify({ token: "ok-token" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );

    await expect(pairRemoteMachine({ codeOrUrl: "123456" })).rejects.toThrow(
      "Invalid response from pairing server",
    );
  });
});
