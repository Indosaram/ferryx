import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import { AddMachineModal, getModalErrorMessage } from "./AddMachineModal";
import { createRemoteHostStore } from "../../state/remoteHostStore";
import type { PairedHostError, PairResult } from "../../lib/pairedHostInventory";

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
