import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import { AddMachineModal, getModalErrorMessage } from "./AddMachineModal";
import { createRemoteHostStore } from "../../state/remoteHostStore";
import { DEFAULT_RELAY_ORIGIN, DEFAULT_MACHINE_LABEL, type PairedHostError, type PairResult } from "../../lib/pairedHostInventory";

afterEach(() => {
  cleanup();
});

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

describe("AddMachineModal - P05 structured per-status error UX", () => {
  it("getModalErrorMessage maps each specific error code to actionable UX guidance", () => {
    expect(getModalErrorMessage("PIN_EXPIRED")).toContain("PIN expired. Obtain a fresh machine PIN");
    expect(getModalErrorMessage("EXPIRED_PIN")).toContain("PIN expired. Obtain a fresh machine PIN");
    expect(getModalErrorMessage("WRONG_RELAY")).toContain("Relay mismatch");
    expect(getModalErrorMessage("INVALID_RELAY_ORIGIN")).toContain("Relay mismatch");
    expect(getModalErrorMessage("DAEMON_UNAVAILABLE")).toContain("Remote daemon unavailable");
    expect(getModalErrorMessage("HOST_UNAVAILABLE")).toContain("Remote daemon unavailable");
    expect(getModalErrorMessage("STALE_HOST_GENERATION")).toContain("Credentials changed during this request");
    expect(getModalErrorMessage("MACHINE_GRANT_REQUIRED")).toContain("Needs machine access");

    const customErr: PairedHostError = {
      code: "CUSTOM_FAILURE",
      message: "Custom detailed reason from daemon",
      retryable: true,
    };
    expect(getModalErrorMessage(customErr)).toBe("Custom detailed reason from daemon");
  });

  it("displays actionable per-status message when pairing fails with PIN_EXPIRED", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: {
          code: "PIN_EXPIRED",
          message: "The provided PIN has expired",
          retryable: false,
        },
      } as PairResult),
    };

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
      />
    );

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, { target: { value: "123456" } });

    const submitBtn = screen.getByRole("button", { name: /pair machine/i });
    fireEvent.click(submitBtn);

    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/PIN expired/i);
    expect(alert).toHaveTextContent(/ferryx-cli pair generate --access machine/i);
  });

  it("displays actionable per-status message when pairing fails with WRONG_RELAY", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: {
          code: "WRONG_RELAY",
          message: "Relay mismatch",
          retryable: false,
        },
      } as PairResult),
    };

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
      />
    );

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: /pair machine/i }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/Relay mismatch/i);
  });

  it("displays actionable per-status message and start-daemon advice on DAEMON_UNAVAILABLE", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: {
          code: "DAEMON_UNAVAILABLE",
          message: "Failed to connect to daemon",
          retryable: true,
        },
      } as PairResult),
    };

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
      />
    );

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: /pair machine/i }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/Remote daemon unavailable/i);
    expect(alert).toHaveTextContent(/ferryx-cli --daemon/i);
  });

  it("displays Refresh inventory action button on STALE_HOST_GENERATION", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: {
          code: "STALE_HOST_GENERATION",
          message: "Stale generation",
          retryable: true,
        },
      } as PairResult),
    };

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
      />
    );

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: /pair machine/i }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });

    const refreshBtn = screen.getByRole("button", { name: /refresh inventory/i });
    expect(refreshBtn).toBeInTheDocument();
    fireEvent.click(refreshBtn);
    expect(mockInventory.refresh).toHaveBeenCalled();
  });
});

describe("AddMachineModal - P04 custom relay origin support", () => {
  it("pairing without a custom relay uses the default origin (DEFAULT_RELAY_ORIGIN)", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: { code: "PAIR_FAILED", message: "Failed", retryable: false },
      } as PairResult),
    };

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
      />
    );

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: /pair machine/i }));

    await waitFor(() => {
      expect(mockInventory.pair).toHaveBeenCalledWith(
        expect.objectContaining({
          relayOrigin: DEFAULT_RELAY_ORIGIN,
          pin: "123456",
          displayLabel: DEFAULT_MACHINE_LABEL,
        }),
        expect.any(Function),
      );
    });
  });

  it("entering a custom relay origin sends the pair request to THAT origin", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: { code: "PAIR_FAILED", message: "Failed", retryable: false },
      } as PairResult),
    };

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
      />
    );

    // Relay origin input should be available
    const relayInput = screen.getByLabelText(/relay origin/i);
    expect(relayInput).toBeInTheDocument();
    fireEvent.change(relayInput, { target: { value: "https://my-custom-relay.internal" } });

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, { target: { value: "654321" } });
    fireEvent.click(screen.getByRole("button", { name: /pair machine/i }));

    await waitFor(() => {
      expect(mockInventory.pair).toHaveBeenCalledWith(
        expect.objectContaining({
          relayOrigin: "https://my-custom-relay.internal",
          pin: "654321",
        }),
        expect.any(Function),
      );
    });
  });

  it("pasting an invite link with #pair= into the PIN field extracts PIN and sends pair request to THAT origin", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: { code: "PAIR_FAILED", message: "Failed", retryable: false },
      } as PairResult),
    };

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
      />
    );

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, {
      target: { value: "https://invite-relay.example.com/#pair=998877" },
    });
    fireEvent.click(screen.getByRole("button", { name: /pair machine/i }));

    await waitFor(() => {
      expect(mockInventory.pair).toHaveBeenCalledWith(
        expect.objectContaining({
          relayOrigin: "https://invite-relay.example.com",
          pin: "998877",
        }),
        expect.any(Function),
      );
    });
  });

  it("prefills custom relay from stored remote gateway status and uses it for pairing", async () => {
    const store = createRemoteHostStore();
    const mockInventory = {
      refresh: vi.fn().mockResolvedValue(undefined),
      pair: vi.fn().mockResolvedValue({
        ok: false,
        error: { code: "PAIR_FAILED", message: "Failed", retryable: false },
      } as PairResult),
    };

    const getStoredRelayOrigin = vi.fn().mockResolvedValue("https://prefilled-relay.corp");

    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        inventory={mockInventory as any}
        store={store}
        onSuccess={() => {}}
        getStoredRelayOrigin={getStoredRelayOrigin}
      />
    );

    // Wait for prefilled relay origin to be populated
    await waitFor(() => {
      const relayInput = screen.getByLabelText(/relay origin/i) as HTMLInputElement;
      expect(relayInput.value).toBe("https://prefilled-relay.corp");
    });

    const pinInput = screen.getByPlaceholderText("Enter 6-digit PIN");
    fireEvent.change(pinInput, { target: { value: "112233" } });
    fireEvent.click(screen.getByRole("button", { name: /pair machine/i }));

    await waitFor(() => {
      expect(mockInventory.pair).toHaveBeenCalledWith(
        expect.objectContaining({
          relayOrigin: "https://prefilled-relay.corp",
          pin: "112233",
        }),
        expect.any(Function),
      );
    });
  });
});

