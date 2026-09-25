import { act, cleanup, createEvent, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteSection } from "./RemoteSection";
import { createRemoteHostStore, remoteHostKey } from "../../state/remoteHostStore";
import { createPairedHostInventory, DEFAULT_RELAY_ORIGIN, type HostView, type PairedHostCommands } from "../../lib/pairedHostInventory";
import { resetSshHostsCache, type SshHost } from "../../lib/sshHosts";

const { isTauriMock, invokeMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("./RemoteAccessSection", () => ({
  RemoteAccessSection: ({ detailsOnly }: { detailsOnly?: boolean }) => (
    <div data-testid={detailsOnly ? "remote-access-details" : "remote-access-section"}>
      Remote Access Mock {detailsOnly ? "(details only)" : ""}
    </div>
  ),
}));

const pairedHostId = remoteHostKey(DEFAULT_RELAY_ORIGIN, "paired-box");
const mockPairedHostView: HostView = {
  hostId: pairedHostId,
  machineId: "paired-box",
  relayOrigin: DEFAULT_RELAY_ORIGIN,
  displayLabel: "Linux Build Box",
  grantScope: "machine",
  generation: "1",
  authStatus: "paired",
  online: true,
};

const mockSshHost: SshHost = {
  id: "ssh-box-1",
  label: "Dev Server",
  hostname: "192.168.1.50",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

function createTestInventory(initialHosts: HostView[] = [mockPairedHostView]) {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = {
    list: vi.fn().mockResolvedValue(initialHosts),
    capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }),
    pair: vi.fn().mockImplementation(async (req: { pin: string }) => {
      if (req.pin === "bad-pin") {
        throw new Error("PAIR_FAILED");
      }
      return {
        hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "new-paired-box"),
        machineId: "new-paired-box",
        relayOrigin: DEFAULT_RELAY_ORIGIN,
        displayLabel: "New Paired Machine",
        grantScope: "machine",
        generation: "1",
        authStatus: "paired",
        online: true,
      };
    }),
    forget: vi.fn().mockResolvedValue(undefined),
    migrate: vi.fn(),
    read: vi.fn(),
  };
  const inventory = createPairedHostInventory(store, commands);
  return { store, inventory, commands };
}

describe("RemoteSection UX Unification & Review Blockers", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    localStorage.clear();
    isTauriMock.mockReturnValue(true);

    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === "cmd_ssh_list_hosts") return [mockSshHost];
      if (command === "cmd_ssh_read_system_config") {
        return {
          path: "/Users/test/.ssh/config",
          exists: true,
          hosts: [{ ...mockSshHost, id: "sys-1", label: "sys-box" }],
          rawText: "Host sys-box\n  HostName 10.0.0.2",
        };
      }
      if (command === "cmd_ssh_test_connection") {
        if (args?.host?.hostname === "unreachable.corp") {
          return { host: args.host, reachable: false, lastError: "Host unreachable" };
        }
        return {
          host: args?.host ?? mockSshHost,
          reachable: true,
          checkedAt: 1,
          environment: { platform: "linux", executor: "bash", version: "5.0", home: "/home/ubuntu", temp: "/tmp", git: true },
        };
      }
      if (command === "cmd_ssh_update_host") {
        return [args?.host ?? mockSshHost];
      }
      if (command === "cmd_ssh_import_config") {
        if (args?.configText?.includes("invalid")) {
          throw new Error("Invalid SSH config syntax");
        }
        return [{ ...mockSshHost, id: "imported-1", label: "imported-host" }];
      }
      return undefined;
    });
  });

  afterEach(cleanup);

  it("authenticates SSH with a separate password before probing and never saves it in the host", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} />); });
    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    fireEvent.click(screen.getByRole("tab", { name: "Connect with SSH" }));
    fireEvent.change(screen.getByLabelText("Authentication method"), { target: { value: "password" } });
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Password server" } });
    fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "password.example" } });
    fireEvent.change(screen.getByLabelText("SSH password"), { target: { value: "fixture-secret" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Connect SSH Machine" })); });
    const calls = invokeMock.mock.calls;
    const authIndex = calls.findIndex(([command]) => command === "cmd_ssh_set_password");
    const probeIndex = calls.findIndex(([command]) => command === "cmd_ssh_test_connection");
    expect(authIndex).toBeGreaterThanOrEqual(0);
    expect(probeIndex).toBeGreaterThan(authIndex);
    expect(calls[authIndex][1]).toMatchObject({ password: "fixture-secret", host: { authMethod: "password" } });
    const saved = calls.find(([command]) => command === "cmd_ssh_update_host");
    expect(saved?.[1].host).not.toHaveProperty("password");
    expect(screen.queryByLabelText("SSH password")).toBeNull();
  });

  it("shows machines without top-level destination navigation", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });
    expect(screen.queryByRole("navigation", { name: "Remote destinations" })).toBeNull();
    expect(screen.getByRole("list", { name: "Remote machines" })).toBeTruthy();
  });

  it("renders ONE mixed machine list without All/Paired/SSH tabs or separate headings", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    expect(screen.getByRole("list", { name: "Remote machines" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Remote destinations" })).toBeNull();

    // MUST NOT have All / Paired / SSH tabs
    expect(screen.queryByRole("button", { name: "All" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Paired" })).toBeNull();
    expect(screen.queryByRole("button", { name: "SSH" })).toBeNull();

    // MUST NOT have separate section headings
    expect(screen.queryByRole("heading", { name: "Paired machines" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "SSH Machines" })).toBeNull();

    // Mixed in ONE list
    expect(screen.getByText("Linux Build Box")).toBeInTheDocument();
    expect(screen.getByText("Dev Server")).toBeInTheDocument();
  });

  it("renders EXACTLY ONE Add Machine button even when the inventory is empty", async () => {
    const { store, inventory } = createTestInventory([]);
    await inventory.refresh();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_ssh_list_hosts") return [];
      return undefined;
    });

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    // Exactly one Add Machine button in the toolbar; none duplicated in empty state
    const addButtons = screen.getAllByRole("button", { name: "Add Machine" });
    expect(addButtons).toHaveLength(1);
  });

  it("renders NO permanent add or PIN forms on the page body", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    expect(screen.queryByLabelText("Machine PIN")).toBeNull();
    expect(screen.queryByRole("button", { name: "Pair machine" })).toBeNull();
  });

  it("common rows display concise status, Add Project, and Details toggle with technical metadata hidden", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    // Paired row
    expect(screen.getByText("Linux Build Box")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Project on Linux Build Box" })).toBeInTheDocument();
    const pairedDetailsBtn = screen.getByRole("button", { name: "Details for Linux Build Box" });
    expect(pairedDetailsBtn).toBeInTheDocument();

    // Status is concise, not paragraph
    expect(screen.getByText("Unchecked")).toBeInTheDocument();
    expect(screen.getByText("Not checked")).toBeInTheDocument();

    // Technical metadata hidden initially
    expect(screen.queryByText(/relayOrigin/i)).toBeNull();
    expect(screen.queryByText("192.168.1.50")).toBeNull();
    expect(screen.queryByText("paired-box")).toBeNull();

    // Expanding Details reveals technical metadata
    fireEvent.click(pairedDetailsBtn);
    expect(screen.getByText(DEFAULT_RELAY_ORIGIN)).toBeInTheDocument();
    expect(screen.getByText("paired-box")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check capabilities for Linux Build Box" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Forget Linux Build Box" })).toBeInTheDocument();
  });

  it("shows the sign-in form with email input when signed out", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    const emailInput = screen.getByLabelText(/Email Address/i);
    expect(emailInput).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Send Magic Link/i })).toBeInTheDocument();
  });

  it("asserts PIN issuance is no longer offered on this surface", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    expect(screen.queryByLabelText(/Machine PIN/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /Pair Machine/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /Generate PIN/i })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    const dialog = screen.getByRole("dialog", { name: "Add Machine" });
    expect(dialog).toBeInTheDocument();

    expect(screen.queryByLabelText(/Machine PIN/i)).toBeNull();
    expect(screen.queryByRole("tab", { name: /Pair with PIN/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /Pair Machine/i })).toBeNull();
    expect(screen.getByRole("tab", { name: "Connect with SSH" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Import SSH Config" })).toBeInTheDocument();
  });

  it("asserts an SSH machine row is still rendered", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    expect(screen.getByText("Dev Server")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Project on Dev Server" })).toBeInTheDocument();
  });

  it("lists one machine from a stubbed account API with a signed-in fixture and enables Add Project only when grant scope is machine", async () => {
    const { store, inventory } = createTestInventory([]);
    await inventory.refresh();
    const onOpenProject = vi.fn();

    const machineHost = {
      machineRecordId: "rec-machine-1",
      machineId: "acc-box-1",
      displayName: "Enrolled Box",
      publicKey: "pub1",
      attachPublicKey: "att1",
      relayOrigin: DEFAULT_RELAY_ORIGIN,
      platform: "linux",
      online: true,
      enrollmentEpoch: "1",
      lastSeenAt: Date.now(),
      grantScope: "machine" as const,
    };

    const fetchMock = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/machines")) {
        return {
          ok: true,
          status: 200,
          json: async () => [machineHost],
        };
      }
      return { ok: false, status: 404, json: async () => ({}) };
    });
    vi.stubGlobal("fetch", fetchMock);

    try {
      const { unmount } = render(
        <RemoteSection
          store={store}
          inventory={inventory}
          onOpenProject={onOpenProject}
          accountSessionToken="test-session-token"
        />
      );

      expect(await screen.findByText("Enrolled Box")).toBeInTheDocument();
      const addProjectBtn = screen.getByRole("button", { name: "Add Project on Enrolled Box" });
      expect(addProjectBtn).toBeInTheDocument();
      expect(addProjectBtn).not.toBeDisabled();

      fireEvent.click(addProjectBtn);
      expect(onOpenProject).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "pairedDaemon",
          hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "acc-box-1"),
        }),
        expect.anything(),
      );

      unmount();

      const mirrorHost = { ...machineHost, grantScope: "mirror" as const };
      fetchMock.mockImplementation(async (url: string) => {
        if (url.includes("/api/account/v1/machines")) {
          return {
            ok: true,
            status: 200,
            json: async () => [mirrorHost],
          };
        }
        return { ok: false, status: 404, json: async () => ({}) };
      });

      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          onOpenProject={onOpenProject}
          accountSessionToken="test-session-token"
        />
      );

      expect(await screen.findByText("Enrolled Box")).toBeInTheDocument();
      const disabledAddBtn = screen.getByRole("button", { name: "Add Project on Enrolled Box" });
      expect(disabledAddBtn).toBeDisabled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("issues an enrollment code and displays the exact CLI command", async () => {
    const { store, inventory } = createTestInventory([]);
    await inventory.refresh();

    const fetchMock = vi.fn().mockImplementation(async (url: string, opts?: RequestInit) => {
      if (url.includes("/api/account/v1/machines")) {
        return {
          ok: true,
          status: 200,
          json: async () => [],
        };
      }
      if (url.includes("/api/account/v1/enrollment-codes") && opts?.method === "POST") {
        return {
          ok: true,
          status: 200,
          json: async () => ({ code: "enr_code_xyz", expiresAt: Date.now() + 600000 }),
        };
      }
      return { ok: false, status: 404, json: async () => ({}) };
    });
    vi.stubGlobal("fetch", fetchMock);

    try {
      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountSessionToken="test-session-token"
          accountOrigin={DEFAULT_RELAY_ORIGIN}
        />
      );

      const issueBtn = await screen.findByRole("button", { name: /Issue Enrollment Code/i });
      expect(issueBtn).toBeInTheDocument();

      await act(async () => {
        fireEvent.click(issueBtn);
      });

      expect(screen.getByText("enr_code_xyz")).toBeInTheDocument();
      const expectedCommand = `ferryx-cli account enroll --code enr_code_xyz --origin ${DEFAULT_RELAY_ORIGIN}`;
      expect(screen.getByText(expectedCommand)).toBeInTheDocument();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("displays empty state copy stating the other machine has to enroll first when signed in with no machines", async () => {
    const { store, inventory } = createTestInventory([]);
    await inventory.refresh();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_ssh_list_hosts") return [];
      return undefined;
    });

    const fetchMock = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/machines")) {
        return {
          ok: true,
          status: 200,
          json: async () => [],
        };
      }
      return { ok: false, status: 404, json: async () => ({}) };
    });
    vi.stubGlobal("fetch", fetchMock);

    try {
      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountSessionToken="test-session-token"
        />
      );

      expect(await screen.findByText(/The other machine has to enroll first/i)).toBeInTheDocument();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("config import separates imported-not-verified result with NO ready/project offer", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    fireEvent.click(screen.getByRole("tab", { name: "Import SSH Config" }));

    // Switch to paste configuration
    fireEvent.click(screen.getByRole("button", { name: "Paste Configuration" }));

    const configArea = screen.getByLabelText("SSH Configuration");
    fireEvent.change(configArea, { target: { value: "Host valid-box\n  HostName 10.0.0.1" } });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Import Configuration" }));
    });

    // Shows imported result clearly separated with NO ready / project offer
    expect(screen.getByText(/Imported 1 machine into inventory/i)).toBeInTheDocument();
    expect(screen.getByText(/Imported machines are recorded as unchecked/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add Project" })).toBeNull();

    // Click Done closes modal and selects row
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog", { name: "Add Machine" })).toBeNull();
  });

  it("restores system SSH config discovery, card, and import options inside Add Machine modal", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    fireEvent.click(screen.getByRole("tab", { name: "Import SSH Config" }));

    // System config detected and card displayed
    expect(await screen.findByText("System SSH Config")).toBeInTheDocument();
    expect(await screen.findByText("1 hosts found")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Import all system hosts" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose custom SSH config file" })).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Import all system hosts" }));
    });

    expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_import_config", {
      configText: "Host sys-box\n  HostName 10.0.0.2",
    });
  });

  it("restores all SSH edit fields including port validation, authMethod, identityFile, and jumpHost", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    // Expand SSH details and start Edit
    fireEvent.click(screen.getByRole("button", { name: "Details for Dev Server" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit Dev Server" }));

    // Invalid port validation
    const portInput = screen.getByLabelText("Port");
    fireEvent.change(portInput, { target: { value: "99999" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent(/Port must be an integer between 1 and 65535/i);

    // Expand advanced options in edit
    fireEvent.click(screen.getByRole("button", { name: /Show Advanced Options/i }));
    expect(screen.getByLabelText("Identity File")).toBeInTheDocument();
    expect(screen.getByLabelText("Jump Host")).toBeInTheDocument();
    expect(screen.getByLabelText("Auth Method")).toBeInTheDocument();

    // Valid update
    fireEvent.change(portInput, { target: { value: "2222" } });
    fireEvent.change(screen.getByLabelText("Identity File"), { target: { value: "~/.ssh/custom_key" } });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    });

    expect(invokeMock).toHaveBeenCalledWith(
      "cmd_ssh_update_host",
      expect.objectContaining({
        host: expect.objectContaining({
          port: 2222,
          identityFile: "~/.ssh/custom_key",
        }),
      }),
    );
  });

  it("restores sameConnection invalidation: clears stale test results when host config changes", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    // Test connection
    fireEvent.click(screen.getByRole("button", { name: "Details for Dev Server" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" }));
    });
    expect(screen.getByTestId("ssh-runtime-ssh-box-1")).toBeInTheDocument();

    // Edit host to change hostname
    fireEvent.click(screen.getByRole("button", { name: "Edit Dev Server" }));
    fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "192.168.1.99" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    });

    // Updating host must clear previous test result
    expect(screen.queryByTestId("ssh-runtime-ssh-box-1")).toBeNull();
  });

  it("disables Paired Add Project without onOpenProject even if onOpenSshProject is provided", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    const negotiate = vi.fn().mockResolvedValue(undefined);
    const onOpenSshProject = vi.fn();

    await act(async () => {
      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          negotiate={negotiate}
          onOpenSshProject={onOpenSshProject}
        />,
      );
    });

    // Mark paired host ready
    fireEvent.click(screen.getByRole("button", { name: "Details for Linux Build Box" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Check capabilities for Linux Build Box" }));
    });

    // Paired Add Project MUST BE DISABLED because onOpenProject is undefined!
    const pairedAddBtn = screen.getByRole("button", { name: "Add Project on Linux Build Box" });
    expect(pairedAddBtn).toBeDisabled();

    // SSH Add Project IS ENABLED because onOpenSshProject is provided
    const sshAddBtn = screen.getByRole("button", { name: "Add Project on Dev Server" });
    expect(sshAddBtn).not.toBeDisabled();
    fireEvent.click(sshAddBtn);
    expect(onOpenSshProject).toHaveBeenCalledWith("ssh-box-1");
  });

  it("namespaces machine row IDs and prevents ID collision between Paired and SSH machines", async () => {
    // Both paired host and ssh host have ID matching collidingPairedId
    const collidingId = remoteHostKey(DEFAULT_RELAY_ORIGIN, "colliding-box");
    const collidingPairedView: HostView = {
      ...mockPairedHostView,
      hostId: collidingId,
      machineId: "colliding-box",
      displayLabel: "Colliding Paired",
    };
    const collidingSshHost: SshHost = {
      ...mockSshHost,
      id: collidingId,
      label: "Colliding SSH",
    };

    const { store, inventory } = createTestInventory([collidingPairedView]);
    await inventory.refresh();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_ssh_list_hosts") return [collidingSshHost];
      return undefined;
    });

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    const pairedEl = screen.getByText("Colliding Paired").closest("[data-machine-id]");
    const sshEl = screen.getByText("Colliding SSH").closest("[data-machine-id]");

    expect(pairedEl).toHaveAttribute("data-machine-id", `paired:${collidingId}`);
    expect(sshEl).toHaveAttribute("data-machine-id", `ssh:${collidingId}`);
  });

  it("handles Escape in Add Machine modal without propagating to parent SettingsDialog", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    const dialog = screen.getByRole("dialog", { name: "Add Machine" });
    expect(dialog).toBeInTheDocument();

    const event = createEvent.keyDown(dialog, { key: "Escape" });
    const stopPropagationSpy = vi.spyOn(event, "stopPropagation");
    fireEvent(dialog, event);

    expect(stopPropagationSpy).toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Add Machine" })).toBeNull();
  });

  it("restores focus to trigger element when Add Machine modal closes", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    const addBtn = screen.getByRole("button", { name: "Add Machine" });
    addBtn.focus();
    expect(document.activeElement).toBe(addBtn);

    fireEvent.click(addBtn);
    expect(screen.getByRole("dialog", { name: "Add Machine" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Add Machine" })).toBeNull();

    // Focus restored to the Add Machine trigger button
    expect(document.activeElement).toBe(addBtn);
  });

  it("resets dismissed state on reopen: open-close-reopen SSH connection succeeds and is not discarded", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    const negotiate = vi.fn().mockResolvedValue(undefined);

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} negotiate={negotiate} />);
    });

    // 1. Open modal and close it via Cancel (sets dismissed)
    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    expect(screen.getByRole("dialog", { name: "Add Machine" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Add Machine" })).toBeNull();

    // 2. Reopen modal
    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    expect(screen.getByRole("dialog", { name: "Add Machine" })).toBeInTheDocument();

    // 3. Connect via SSH and submit
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Reopened Box" } });
    fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "192.168.1.100" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Connect SSH Machine" }));
    });

    // 4. Must successfully verify and offer project (not discarded!)
    expect(screen.getByText(/connected and verified/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Project" })).toBeInTheDocument();
  });
});
