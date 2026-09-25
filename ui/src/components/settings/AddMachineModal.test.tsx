import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { AddMachineModal, getModalErrorMessage } from "./AddMachineModal";
import { createRemoteHostStore } from "../../state/remoteHostStore";
import { parsePairingInvite, type PairedHostError } from "../../lib/pairedHostInventory";

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
  it("asserts PIN is not reachable at all (no PIN tab, no Machine PIN field, regardless of initialTab)", () => {
    const store = createRemoteHostStore();
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        store={store}
        onSuccess={() => {}}
        initialTab={"pin" as any}
      />,
    );
    expect(screen.getByRole("tab", { name: "Connect with SSH" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Import SSH Config" })).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /Pair with PIN/i })).toBeNull();
    expect(screen.queryByLabelText(/Machine PIN/i)).toBeNull();
    expect(screen.queryByPlaceholderText(/Enter 6-digit PIN/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /Pair Machine/i })).toBeNull();
  });

  it("getModalErrorMessage maps each specific error code to actionable UX guidance", () => {
    expect(getModalErrorMessage("PIN_EXPIRED")).toContain("Code expired. Sign in to your Ferryx account");
    expect(getModalErrorMessage("EXPIRED_PIN")).toContain("Code expired. Sign in to your Ferryx account");
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

  it("maps PIN_EXPIRED to actionable guidance", () => {
    const msg = getModalErrorMessage("PIN_EXPIRED");
    expect(msg).toMatch(/expired/i);
    expect(msg).toMatch(/sign in/i);
  });

  it("maps WRONG_RELAY to relay mismatch advice", () => {
    const msg = getModalErrorMessage("WRONG_RELAY");
    expect(msg).toMatch(/Relay mismatch/i);
  });

  it("maps DAEMON_UNAVAILABLE to start-daemon advice", () => {
    const msg = getModalErrorMessage("DAEMON_UNAVAILABLE");
    expect(msg).toMatch(/Remote daemon unavailable/i);
  });

  it("maps STALE_HOST_GENERATION to inventory refresh advice", () => {
    const msg = getModalErrorMessage("STALE_HOST_GENERATION");
    expect(msg).toMatch(/Credentials changed during this request/i);
  });
});

describe("AddMachineModal - P04 custom relay origin support", () => {
  it("pairing without a custom relay uses the default origin (DEFAULT_RELAY_ORIGIN)", () => {
    const invite = parsePairingInvite("123456");
    expect(invite).toBeNull();
  });

  it("entering a custom relay origin sends the pair request to THAT origin", () => {
    const invite = parsePairingInvite("https://my-custom-relay.internal/#pair=654321");
    expect(invite?.relayOrigin).toBe("https://my-custom-relay.internal");
    expect(invite?.pin).toBe("654321");
  });

  it("pasting an invite link with #pair= into the PIN field extracts PIN and sends pair request to THAT origin", () => {
    const invite = parsePairingInvite("https://invite-relay.example.com/#pair=998877");
    expect(invite?.relayOrigin).toBe("https://invite-relay.example.com");
    expect(invite?.pin).toBe("998877");
  });
});

describe("AddMachineModal - SSH connection and import", () => {
  it("renders SSH connection form by default and allows SSH connection test", () => {
    const store = createRemoteHostStore();
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        store={store}
        onSuccess={() => {}}
      />,
    );
    expect(screen.getByLabelText(/Label/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/Hostname/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Connect SSH Machine/i })).toBeInTheDocument();
  });
});
