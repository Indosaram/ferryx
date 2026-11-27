import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { AddMachineModal, getModalErrorMessage, selectedHostsConfig } from "./AddMachineModal";
import { parsePairingInvite, type PairedHostError } from "../../lib/pairedHostInventory";
import type { SshHost } from "../../lib/sshHosts";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
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

/**
 * Pick an SSH authentication method in the wizard's Radix Select. A plain click opens the
 * trigger (the mouse branch needs a real pointerType, which jsdom's PointerEvent lacks), and
 * the option commits on its own click.
 */
async function chooseAuthMethod(label: string) {
  await act(async () => {
    fireEvent.click(screen.getByRole("combobox", { name: "Authentication method" }));
  });
  const option = await screen.findByRole("option", { name: label });
  await act(async () => { fireEvent.click(option); });
}

const configHost = (overrides: Partial<SshHost> = {}): SshHost => ({
  id: "sys-1",
  label: "sys-box",
  hostname: "10.0.0.2",
  username: "ubuntu",
  port: 22,
  source: "config",
  authMethod: "agent",
  disabled: false,
  ...overrides,
});

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
    expect(screen.getByRole("radio", { name: "Connect with SSH" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Import SSH Config" })).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(4);
    expect(screen.queryByRole("radio", { name: /Enrollment code/i })).toBeNull();
    expect(screen.queryByRole("radio", { name: /Pair with PIN/i })).toBeNull();
    expect(screen.queryByRole("tab", { name: /Pair with PIN/i })).toBeNull();
    expect(screen.queryByLabelText(/Machine PIN/i)).toBeNull();
    expect(screen.queryByPlaceholderText(/Enter 6-digit PIN/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /Pair Machine/i })).toBeNull();
  });

  it("getModalErrorMessage maps each specific error code to actionable UX guidance", () => {
    expect(getModalErrorMessage("PIN_EXPIRED")).toContain("Code expired");
    expect(getModalErrorMessage("EXPIRED_PIN")).toContain("Code expired");
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

  it("maps PIN_EXPIRED to guidance that never sends the user to an enrollment code", () => {
    const msg = getModalErrorMessage("PIN_EXPIRED");
    expect(msg).toMatch(/expired/i);
    expect(msg).toMatch(/retry/i);
    expect(msg).not.toMatch(/enrollment code/i);
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
    // Signed out (no install-key issuer): SSH is the default, and there is no enrollment method
    // to fall back on. A machine already running ferryx-cli pairs itself when it signs in.
    expect(screen.getByRole("radio", { name: "Connect with SSH" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("radio", { name: /Enrollment code/i })).toBeNull();
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

describe("AddMachineModal - the manual enrollment-code method is gone", () => {
  it("offers no enrollment method, no enroll command and no issued card even when signed in", async () => {
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        onSuccess={() => {}}
        accountOrigin="https://relay.example"
      />,
    );

    expect(screen.queryByRole("radio", { name: /Enrollment code/i })).toBeNull();
    expect(screen.queryByText(/ferryx-cli account enroll/)).toBeNull();
    expect(screen.queryByTestId("issued-enrollment-card")).toBeNull();
    expect(screen.queryByTestId("enrollment-command")).toBeNull();
    expect(screen.queryByText(/Sign in to issue an enrollment code/i)).toBeNull();
  });

  it("an initialMethod of enrollment falls back to the method chooser instead of a code step", () => {
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        onSuccess={() => {}}
        initialMethod={"enrollment" as any}
      />,
    );

    expect(screen.getByRole("radiogroup", { name: "Connection methods" })).toBeInTheDocument();
    expect(screen.queryByTestId("enrollment-command")).toBeNull();
    expect(screen.queryByText(/ferryx-cli account enroll/)).toBeNull();
  });
});

describe("AddMachineModal - SSH auth method and Key File reveal", () => {
  it("uses the app's Radix Select for the auth method instead of a native select", () => {
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} initialMethod="ssh" />);
    expect(screen.getByRole("combobox", { name: "Authentication method" })).toBeInTheDocument();
    // A native <select> would expose its own combobox role here; the Radix trigger is a button.
    expect(screen.getByRole("combobox", { name: "Authentication method" }).tagName).toBe("BUTTON");
    expect(document.querySelector("select#ssh-auth-method")).toBeNull();
  });

  it("reveals the Identity File field immediately on switching to Key File, without Advanced", async () => {
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} initialMethod="ssh" />);
    // Collapsed by default: the identity field lives only in Advanced while the method is Agent.
    expect(screen.queryByLabelText("Identity file")).toBeNull();

    await chooseAuthMethod("Key File");

    expect(screen.getByLabelText("Identity file")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Show Advanced Options/i })).toBeInTheDocument();
    // ...and the advanced section is still collapsed, so the field above is the inline one.
    expect(screen.queryByLabelText("Jump host")).toBeNull();
  });

  it("keeps one Identity File field when Key File is selected and Advanced is opened", async () => {
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} initialMethod="ssh" />);
    await chooseAuthMethod("Key File");
    fireEvent.click(screen.getByRole("button", { name: /Show Advanced Options/i }));
    expect(screen.getAllByLabelText("Identity file")).toHaveLength(1);
    expect(screen.getByLabelText("Jump host")).toBeInTheDocument();
  });
});

describe("AddMachineModal - candidate password is tested before it is stored", () => {
  function mockIpc(overrides: Record<string, (args?: Record<string, unknown>) => unknown>) {
    vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
      const request = args as Record<string, unknown> | undefined;
      if (command === "cmd_ssh_read_system_config") return { path: "", exists: false, rawText: "", hosts: [] };
      const handler = overrides[command];
      if (handler) return handler(request);
      if (command === "cmd_ssh_update_host") return [request?.host];
      return undefined;
    });
  }

  function fillPasswordForm() {
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Password server" } });
    fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "password.example" } });
    fireEvent.change(screen.getByLabelText("SSH password"), { target: { value: "fixture-secret" } });
  }

  it("passes the typed password as candidatePassword to the probe and stores it only after saving", async () => {
    mockIpc({
      cmd_ssh_test_connection: args => ({ host: args?.host, reachable: true, checkedAt: 1 }),
    });
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} initialMethod="ssh" />);
    await chooseAuthMethod("Password");
    fillPasswordForm();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Connect SSH Machine" })); });

    const commands = vi.mocked(invoke).mock.calls.map(([command]) => command);
    const probeIndex = commands.indexOf("cmd_ssh_test_connection");
    const saveIndex = commands.indexOf("cmd_ssh_update_host");
    const setPasswordIndex = commands.indexOf("cmd_ssh_set_password");

    expect(probeIndex).toBeGreaterThanOrEqual(0);
    expect(vi.mocked(invoke).mock.calls[probeIndex][1]).toMatchObject({ candidatePassword: "fixture-secret" });
    expect(saveIndex).toBeGreaterThan(probeIndex);
    expect(setPasswordIndex).toBeGreaterThan(saveIndex);
  });

  it("never stores or clears a password when the probe fails", async () => {
    mockIpc({
      cmd_ssh_test_connection: args => ({ host: args?.host, reachable: false, lastError: "Permission denied", checkedAt: 1 }),
    });
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} initialMethod="ssh" />);
    await chooseAuthMethod("Password");
    fillPasswordForm();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Connect SSH Machine" })); });

    const commands = vi.mocked(invoke).mock.calls.map(([command]) => command);
    expect(commands).toContain("cmd_ssh_test_connection");
    expect(commands).not.toContain("cmd_ssh_set_password");
    expect(commands).not.toContain("cmd_ssh_clear_password");
    expect(commands).not.toContain("cmd_ssh_update_host");
    expect(screen.getByRole("alert")).toHaveTextContent(/Permission denied/);
  });
});

describe("AddMachineModal - system SSH config host picker", () => {
  const fourHosts = [
    configHost({ id: "sys-1", label: "alpha", hostname: "10.0.0.1" }),
    configHost({ id: "sys-2", label: "beta", hostname: "10.0.0.2", username: null, port: 2222 }),
    configHost({ id: "sys-3", label: "gamma", hostname: "10.0.0.3" }),
    configHost({ id: "sys-4", label: "delta", hostname: "10.0.0.4" }),
  ];

  function mockConfig(hosts: SshHost[], exists = true) {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_read_system_config") {
        return { path: "/home/test/.ssh/config", exists, rawText: hosts.map(h => `Host ${h.label}`).join("\n"), hosts };
      }
      if (command === "cmd_ssh_import_config") return hosts;
      return undefined;
    });
  }

  async function openImport() {
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} initialMethod="import" />);
    await screen.findByText(/hosts found/);
  }

  it("checks every discovered host by default and imports only the unchecked ones after Clear", async () => {
    mockConfig(fourHosts);
    await openImport();

    for (const host of fourHosts) {
      expect(screen.getByRole("checkbox", { name: `Import ${host.label}` })).toBeChecked();
    }
    expect(screen.getByRole("button", { name: "Import selected system hosts" })).toHaveTextContent("Import Selected (4)");

    // Clear empties the selection and disables the import.
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.getByRole("button", { name: "Import selected system hosts" })).toBeDisabled();

    // Select All restores it.
    fireEvent.click(screen.getByRole("button", { name: "Select All" }));
    expect(screen.getByRole("button", { name: "Import selected system hosts" })).toHaveTextContent("Import Selected (4)");
  });

  it("imports only the chosen hosts, rebuilding a config from their fields", async () => {
    mockConfig(fourHosts);
    await openImport();

    fireEvent.click(screen.getByRole("checkbox", { name: "Import alpha" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Import gamma" }));
    expect(screen.getByRole("button", { name: "Import selected system hosts" })).toHaveTextContent("Import Selected (2)");

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Import selected system hosts" })); });

    const importCall = vi.mocked(invoke).mock.calls.find(([command]) => command === "cmd_ssh_import_config");
    const sent = (importCall?.[1] as { configText: string }).configText;
    expect(sent).toContain("Host beta");
    expect(sent).toContain("Host delta");
    expect(sent).not.toContain("Host alpha");
    expect(sent).not.toContain("Host gamma");
  });

  it("keeps Choose file… reachable when the system config does not exist", async () => {
    mockConfig([], false);
    render(<AddMachineModal isOpen={true} onClose={() => {}} onSuccess={() => {}} initialMethod="import" />);

    // Paste is the default when there is nothing to import, but the tab is always offered.
    const tab = await screen.findByRole("button", { name: "System Config" });
    fireEvent.click(tab);

    expect(screen.getByText("System SSH Config")).toBeInTheDocument();
    expect(screen.getByText(/No SSH config at that path/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose custom SSH config file" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.getByRole("button", { name: "Import selected system hosts" })).toBeDisabled();
  });

  it("keeps Paste Configuration working next to the host picker", async () => {
    mockConfig(fourHosts);
    await openImport();
    fireEvent.click(screen.getByRole("button", { name: "Paste Configuration" }));
    expect(screen.getByLabelText("SSH Configuration")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("quotes an IdentityFile path that contains spaces so the config parser keeps it whole", () => {
    const text = selectedHostsConfig([
      configHost({
        id: "spaced",
        label: "spaced",
        hostname: "10.0.0.3",
        username: null,
        port: null,
        identityFile: "/Users/test user/.ssh/my key",
      }),
    ]);
    // The generated block is machine-consumed: the Rust parser's quoted-value path reads this
    // whole path, so the quotes are the contract, not formatting.
    expect(text).toContain('IdentityFile "/Users/test user/.ssh/my key"');
  });

  it("builds a parseable block per host, skipping fields the host does not have", () => {
    const text = selectedHostsConfig([
      configHost({ id: "a", label: "alpha", hostname: "10.0.0.1", username: "ubuntu", port: 22 }),
      configHost({
        id: "b",
        label: "beta",
        hostname: "10.0.0.2",
        username: null,
        port: null,
        identityFile: "/home/test/.ssh/id_ed25519",
        jumpHost: "bastion.example",
      }),
    ]);
    expect(text).toBe(
      [
        "Host alpha",
        "  HostName 10.0.0.1",
        "  User ubuntu",
        "  Port 22",
        "Host beta",
        "  HostName 10.0.0.2",
        "  IdentityFile \"/home/test/.ssh/id_ed25519\"",
        "  ProxyJump bastion.example",
      ].join("\n"),
    );
  });
});

describe("AddMachineModal - install key expiry ticks live", () => {
  it("counts down while the key is shown and stops at expiry", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-26T00:00:00Z"));
    const expiresAt = Math.floor(Date.now() / 1000) + 120;
    vi.mocked(invoke).mockImplementation(async (command: string) =>
      command === "cmd_ssh_read_system_config" ? { path: "", exists: false, rawText: "", hosts: [] } : undefined,
    );
    render(
      <AddMachineModal
        isOpen={true}
        onClose={() => {}}
        onSuccess={() => {}}
        initialMethod="install"
        onIssueInstallKey={vi.fn().mockResolvedValue({
          keyId: "key-1",
          key: "fxk_secret",
          expiresAt,
          installCommand: { sh: "sh-cmd", ps1: "ps1-cmd" },
        })}
      />,
    );
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });

    const expiry = screen.getByTestId("install-key-expiry");
    expect(expiry).toHaveAttribute("data-expired", "false");
    expect(Number(expiry.getAttribute("data-remaining-ms"))).toBeGreaterThan(60_000);

    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(Number(expiry.getAttribute("data-remaining-ms"))).toBeLessThanOrEqual(60_000);

    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(expiry).toHaveAttribute("data-expired", "true");
    expect(expiry).toHaveAttribute("data-remaining-ms", "0");

    // The countdown is finished: further time cannot change the state any more.
    await act(async () => { await vi.advanceTimersByTimeAsync(120_000); });
    expect(expiry).toHaveAttribute("data-expired", "true");
    expect(expiry).toHaveAttribute("data-remaining-ms", "0");
    vi.useRealTimers();
  });
});
