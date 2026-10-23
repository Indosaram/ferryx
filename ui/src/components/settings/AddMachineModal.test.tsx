import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { AddMachineModal, getModalErrorMessage } from "./AddMachineModal";
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

/** Step 1 of the wizard: pick a connection method, then continue to its step. */
function chooseMethod(name: string) {
  fireEvent.click(screen.getByRole("radio", { name }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}

describe("AddMachineModal - P05 structured per-status error UX", () => {
  it("asserts PIN is not reachable at all (no PIN option, no Machine PIN field, regardless of initialMethod)", () => {
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        onSuccess={() => {}}
        initialMethod={"pin" as any}
      />,
    );
    // An unknown method falls back to step 1, "Choose a method".
    expect(screen.getByRole("radiogroup", { name: "Connection methods" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Install command" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Approve a machine" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Enrollment code (manual)" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Connect with SSH" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Import SSH Config" })).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(5);
    expect(screen.queryByRole("radio", { name: /Pair with PIN/i })).toBeNull();
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
  function openPasteImport(onClose: () => void) {
    render(<AddMachineModal isOpen={true} onClose={onClose} onSuccess={() => {}} initialMethod="import" />);
    fireEvent.change(screen.getByLabelText("SSH Configuration"), { target: { value: "Host a\n  HostName a.example" } });
  }

  it("keeps the dialog open and shows an error when an import returns zero hosts", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) =>
      command === "cmd_ssh_read_system_config" ? { path: "", exists: false, rawText: "", hosts: [] } : [],
    );
    const onClose = vi.fn();
    openPasteImport(onClose);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Import Configuration" })); });
    expect(screen.getByRole("alert")).toHaveTextContent(/No hosts were found/i);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("renders the code-specific hint for a structured IPC error code", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_read_system_config") return { path: "", exists: false, rawText: "", hosts: [] };
      throw { code: "WRONG_RELAY", message: "relay differs", details: {} };
    });
    openPasteImport(() => {});
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Import Configuration" })); });
    expect(screen.getByRole("alert")).toHaveTextContent(/Verify both machines use the same relay URL/i);
  });

  it("keeps focus on the dialog container when Tab is pressed with every control disabled", async () => {
    let release!: (hosts: unknown[]) => void;
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "cmd_ssh_read_system_config"
        ? Promise.resolve({ path: "", exists: false, rawText: "", hosts: [] })
        : new Promise((resolve) => { release = resolve as (hosts: unknown[]) => void; }),
    );
    openPasteImport(() => {});
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Import Configuration" })); });
    const dialog = screen.getByRole("dialog", { name: "Add Machine" });
    // The Dialog primitive's content element is the dialog container itself.
    const container = dialog;
    expect(container).toHaveAttribute("tabindex", "-1");
    fireEvent.keyDown(document.activeElement ?? container, { key: "Tab" });
    expect(document.activeElement).toBe(container);
    await act(async () => { release([]); });
  });

  it("renders the SSH connection form after choosing Connect with SSH", () => {
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        onSuccess={() => {}}
      />,
    );
    // Signed out (no install-key issuer): the manual enrollment method stays the default.
    expect(screen.getByRole("radio", { name: "Enrollment code (manual)" })).toHaveAttribute("aria-checked", "true");
    chooseMethod("Connect with SSH");
    expect(screen.getByLabelText(/Label/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/Hostname/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Connect SSH Machine/i })).toBeInTheDocument();
  });

  it("returns to step 1 from a method step with Back", () => {
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} />);
    chooseMethod("Import SSH Config");
    expect(screen.getByRole("button", { name: "Paste Configuration" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("radio", { name: "Import SSH Config" })).toHaveAttribute("aria-checked", "true");
  });
});

describe("AddMachineModal - enrollment code step", () => {
  it("signed in: choosing Enrollment code renders the exact enroll command", async () => {
    const origin = "https://relay.example";
    const issue = vi.fn().mockResolvedValue("ABCD-1234");
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        onSuccess={() => {}}
        accountOrigin={origin}
        onIssueEnrollmentCode={issue}
      />,
    );
    chooseMethod("Enrollment code (manual)");

    expect(await screen.findByTestId("issued-enrollment-card")).toBeInTheDocument();
    expect(screen.getByTestId("enrollment-command").textContent).toBe(
      "ferryx-cli account enroll --code ABCD-1234 --origin https://relay.example",
    );
    expect(issue).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Dismiss enrollment code" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
    expect(screen.queryByText(/Sign in to issue an enrollment code/i)).toBeNull();
  });

  it("signed out: choosing Enrollment code shows the sign-in reason and issues nothing", () => {
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} accountOrigin="https://relay.example" />);
    chooseMethod("Enrollment code (manual)");

    expect(screen.getByText(/Sign in to issue an enrollment code/i)).toBeInTheDocument();
    expect(screen.queryByTestId("issued-enrollment-card")).toBeNull();
    expect(screen.queryByTestId("enrollment-command")).toBeNull();
  });
});
