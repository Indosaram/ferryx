import "@testing-library/jest-dom/vitest";
import { act, cleanup, createEvent, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteSection } from "./RemoteSection";
import { createRemoteHostStore, remoteHostKey } from "../../state/remoteHostStore";
import { createPairedHostInventory, DEFAULT_RELAY_ORIGIN, type HostView, type PairedHostCommands } from "../../lib/pairedHostInventory";
import { resetSshHostsCache, type SshHost } from "../../lib/sshHosts";
import type { AccountMachineView } from "../../remote/accountSession";
import { resetTailscaleDiscoveryForTests, syncTailscaleMachines } from "../../lib/tailscaleDiscovery";

const grantMocks = vi.hoisted(() => ({
  attachKey: vi.fn(),
  requestGrant: vi.fn(),
}));

// Reachability defaults to an older backend without the list command, so rows use the legacy
// online flag; individual tests opt into reported states.
const reachabilityMock = vi.hoisted(() => vi.fn((_hostId: string) => ({ supported: false, entry: null as unknown })));
vi.mock("../../lib/pairedHostReachability", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/pairedHostReachability")>()),
  usePairedHostReachability: (hostId: string) => reachabilityMock(hostId),
}));

/** Closes the open details drawer the way a keyboard user does. */
function pressEscape() {
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
}

/** Add Machine wizard step 1: pick a method by its option name, then continue to its step. */
function chooseAddMachineMethod(name: string) {
  fireEvent.click(screen.getByRole("radio", { name }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}

vi.mock("../../remote/accountAttach", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../remote/accountAttach")>()),
  getOrCreateAttachKey: grantMocks.attachKey,
}));

vi.mock("../../remote/accountSession", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../remote/accountSession")>()),
  requestGrant: grantMocks.requestGrant,
}));

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
    resetTailscaleDiscoveryForTests();
    localStorage.clear();
    isTauriMock.mockReturnValue(true);
    reachabilityMock.mockImplementation(() => ({ supported: false, entry: null }));

    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === "cmd_ssh_list_hosts") return [mockSshHost];
      if (command === "cmd_agent_integration_get") return { enabled: true };
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

  afterEach(() => {
    cleanup();
    resetTailscaleDiscoveryForTests();
  });

  it("automatically enables a provisioned Tailnet machine without opening its project", async () => {
    const tailnet: SshHost = { ...mockSshHost, id: "tailscale:ready", source: "tailscale" };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_sync_tailscale") return { hosts: [tailnet], targets: [{ host: tailnet, reachable: true, helper: "installed", checkedAt: 1 }] };
      if (command === "cmd_ssh_list_hosts") return [tailnet];
      return undefined;
    });
    await syncTailscaleMachines();
    const { store, inventory } = createTestInventory([]);
    const openProject = vi.fn();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} onOpenSshProject={openProject} />); });
    expect(screen.getByTestId("machine-status")).toHaveAttribute("data-code", "READY");
    expect(screen.getByRole("button", { name: `Add Project on ${tailnet.label}` })).toBeEnabled();
    expect(openProject).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: `Add Project on ${tailnet.label}` }));
    expect(openProject).toHaveBeenCalledWith(tailnet.id);
  });

  it("keeps a reachable Tailnet machine unavailable (under Discovered) when helper provisioning fails", async () => {
    const tailnet: SshHost = { ...mockSshHost, id: "tailscale:failed", source: "tailscale" };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_sync_tailscale") return { hosts: [tailnet], targets: [{ host: tailnet, reachable: true, helper: "unknown", checkedAt: 1, diagnostic: { code: "CLI_EXECUTABLE_NOT_FOUND", message: "Fixture helper unavailable", details: { stage: "provisioning" } } }] };
      if (command === "cmd_ssh_list_hosts") return [tailnet];
      return undefined;
    });
    await syncTailscaleMachines();
    const { store, inventory } = createTestInventory([]);
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} onOpenSshProject={vi.fn()} />); });
    expect(screen.getByTestId("machine-status")).toHaveAttribute("data-code", "CLI_EXECUTABLE_NOT_FOUND");
    // Not usable yet: it is listed under Discovered, which offers setup but no Add Project.
    expect(screen.queryByRole("button", { name: `Add Project on ${tailnet.label}` })).toBeNull();
    expect(within(screen.getByRole("list", { name: "Discovered machines" })).getByText(tailnet.label)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Set up ${tailnet.label}` })).toBeEnabled();
    // Section-level retry is a different control and keeps its own name.
    expect(screen.queryByRole("button", { name: "Retry Tailscale" })).toBeNull();
  });

  it("opens the machine drawer from a saved Discovered host so it can be edited, disabled or deleted", async () => {
    const tailnet: SshHost = { ...mockSshHost, id: "tailscale:stuck", source: "tailscale" };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_sync_tailscale") return { hosts: [tailnet], targets: [{ host: tailnet, reachable: true, helper: "unknown", checkedAt: 1, diagnostic: { code: "CLI_EXECUTABLE_NOT_FOUND", message: "Fixture helper unavailable", details: { stage: "provisioning" } } }] };
      if (command === "cmd_ssh_list_hosts") return [tailnet];
      if (command === "cmd_agent_integration_get") return { enabled: true };
      return undefined;
    });
    await syncTailscaleMachines();
    const { store, inventory } = createTestInventory([]);
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} onOpenSshProject={vi.fn()} />); });

    const discovered = screen.getByRole("list", { name: "Discovered machines" });
    expect(within(discovered).getByRole("button", { name: `Set up ${tailnet.label}` })).toBeInTheDocument();
    const details = within(discovered).getByRole("button", { name: `Details for ${tailnet.label}` });
    details.focus();
    fireEvent.click(details);

    const drawer = screen.getByRole("dialog");
    expect(within(drawer).getByRole("button", { name: `Edit ${tailnet.label}` })).toBeEnabled();
    expect(within(drawer).getByRole("button", { name: `Toggle disable ${tailnet.label}` })).toHaveTextContent("Disable");
    expect(within(drawer).getByRole("button", { name: `Delete ${tailnet.label}` })).toBeEnabled();

    pressEscape();
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: `Details for ${tailnet.label}` })));
  });

  it("orders the sections My machines, Discovered, This machine, Advanced", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} />); });
    const order = [
      screen.getByRole("heading", { name: "My machines" }),
      screen.getByRole("heading", { name: "Discovered" }),
      screen.getByRole("heading", { name: "This machine" }),
      screen.getByText("Advanced", { selector: "summary" }),
    ];
    for (let i = 1; i < order.length; i += 1) {
      expect(order[i - 1].compareDocumentPosition(order[i]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
  });

  it("lists an unprovisioned Tailscale host under Discovered and not in Remote machines", async () => {
    const tailnet: SshHost = { ...mockSshHost, id: "tailscale:new", label: "Tailnet Peer", hostname: "100.64.0.9", source: "tailscale" };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_sync_tailscale") return { hosts: [tailnet], targets: [{ host: tailnet, reachable: true, helper: "missing", checkedAt: 1 }] };
      if (command === "cmd_ssh_list_hosts") return [mockSshHost, tailnet];
      return undefined;
    });
    await syncTailscaleMachines();
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} onOpenSshProject={vi.fn()} />); });

    expect(within(screen.getByRole("list", { name: "Discovered machines" })).getByText("Tailnet Peer")).toBeInTheDocument();
    const machines = screen.getByRole("list", { name: "Remote machines" });
    expect(within(machines).queryByText("Tailnet Peer")).toBeNull();
    expect(within(machines).getByText("Linux Build Box")).toBeInTheDocument();
  });

  it("keeps a disabled Tailscale host under My machines so its menu can enable it again", async () => {
    const tailnet: SshHost = { ...mockSshHost, id: "tailscale:off", label: "Disabled Peer", hostname: "100.64.0.10", source: "tailscale", disabled: true };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_sync_tailscale") return { hosts: [tailnet], targets: [] };
      if (command === "cmd_ssh_list_hosts") return [mockSshHost, tailnet];
      return undefined;
    });
    await syncTailscaleMachines();
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} onOpenSshProject={vi.fn()} />); });

    expect(within(screen.getByRole("list", { name: "Remote machines" })).getByText("Disabled Peer")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Discovered machines" }) && within(screen.getByRole("list", { name: "Discovered machines" })).queryByText("Disabled Peer")).toBeFalsy();
  });

  it("shows OFFLINE and CHECKING from paired-host reachability, with a visible reason on the disabled primary", async () => {
    const offlineId = remoteHostKey(DEFAULT_RELAY_ORIGIN, "offline-box");
    const checkingId = remoteHostKey(DEFAULT_RELAY_ORIGIN, "checking-box");
    reachabilityMock.mockImplementation((hostId: string) => ({
      supported: true,
      entry: hostId === offlineId
        ? { hostId, generation: "1", state: "offline", checkedAtMs: 1, lastError: "relay has no route" }
        : hostId === checkingId
        ? { hostId, generation: "1", state: "checking", checkedAtMs: 1, lastError: null }
        : null,
    }));
    const { store, inventory } = createTestInventory([
      { ...mockPairedHostView, hostId: offlineId, machineId: "offline-box", displayLabel: "Offline Box" },
      { ...mockPairedHostView, hostId: checkingId, machineId: "checking-box", displayLabel: "Checking Box" },
    ]);
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} onOpenProject={vi.fn()} />); });

    const offlineRow = screen.getByText("Offline Box").closest("[data-machine-id]") as HTMLElement;
    expect(within(offlineRow).getByTestId("machine-status")).toHaveAttribute("data-code", "OFFLINE");
    expect(within(offlineRow).getByRole("button", { name: "Add Project on Offline Box" })).toBeDisabled();
    expect(within(offlineRow).getByTestId("machine-disabled-reason")).toHaveTextContent("relay has no route");

    const checkingRow = screen.getByText("Checking Box").closest("[data-machine-id]") as HTMLElement;
    expect(within(checkingRow).getByTestId("machine-status")).toHaveAttribute("data-code", "CHECKING");
    expect(within(checkingRow).getByTestId("machine-disabled-reason").textContent).not.toBe("");
  });

  it("opens the details drawer, closes it with Escape, and returns focus to its Details button", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} />); });

    const details = screen.getByRole("button", { name: "Details for Linux Build Box" });
    details.focus();
    fireEvent.click(details);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check capabilities for Linux Build Box" })).toBeInTheDocument();

    pressEscape();
    expect(screen.queryByRole("dialog")).toBeNull();
    // Radix restores focus on its close-auto-focus event, dispatched after unmount.
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Details for Linux Build Box" })));
  });

  it("returns focus to the row's More actions trigger when a drawer opened from that menu closes", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} />); });

    const trigger = await screen.findByRole("button", { name: "More actions for Dev Server" });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "Enter" });
    const edit = await screen.findByRole("menuitem", { name: "Edit…" });
    await act(async () => { fireEvent.click(edit); });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save Changes" })).toBeInTheDocument();

    pressEscape();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "More actions for Dev Server" })));
  });

  it("authenticates SSH with a separate password before probing and never saves it in the host", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} />); });
    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    chooseAddMachineMethod("Connect with SSH");
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

  it("shows the sign-in form with email input when signed out and asserts Generate QR is absent", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    const emailInput = screen.getByLabelText(/Email Address/i);
    expect(emailInput).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Send Magic Link/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /generate qr|regenerate/i })).toBeNull();
    expect(screen.queryByText(/PIN:/i)).toBeNull();
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
    expect(screen.queryByRole("button", { name: /generate qr|regenerate/i })).toBeNull();
    expect(screen.queryByText(/PIN:/i)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    const dialog = screen.getByRole("dialog", { name: "Add Machine" });
    expect(dialog).toBeInTheDocument();

    expect(screen.queryByLabelText(/Machine PIN/i)).toBeNull();
    expect(screen.queryByRole("tab", { name: /Pair with PIN/i })).toBeNull();
    expect(screen.queryByRole("radio", { name: /Pair with PIN/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /Pair Machine/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /generate qr|regenerate/i })).toBeNull();
    expect(screen.getByRole("radio", { name: "Enrollment code (manual)" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Connect with SSH" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Import SSH Config" })).toBeInTheDocument();
  });

  it("asserts Generate QR is absent and the SSH method is offered in Add Machine", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} />);
    });

    // Remote Access UI must not render a Generate QR button
    expect(screen.queryByRole("button", { name: /generate qr|regenerate/i })).toBeNull();
    expect(screen.queryByText(/PIN:/i)).toBeNull();

    // SSH methods are offered in the Add Machine wizard's method step
    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    expect(screen.getByRole("radiogroup", { name: "Connection methods" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Connect with SSH" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Import SSH Config" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /generate qr|regenerate/i })).toBeNull();
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
    const grantedHostView: HostView = {
      hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "acc-box-1"),
      machineId: "acc-box-1",
      relayOrigin: DEFAULT_RELAY_ORIGIN,
      displayLabel: "Enrolled Box",
      grantScope: "machine",
      generation: "1",
      authStatus: "paired",
      online: true,
    };
    const { store, inventory } = createTestInventory([grantedHostView]);
    await inventory.refresh();
    const onOpenProject = vi.fn();

    const machineHost: AccountMachineView = {
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
          accountOrigin={DEFAULT_RELAY_ORIGIN}
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

      // Now create inventory where the host has "mirror" scope (not "machine")
      const mirrorHostView: HostView = {
        ...grantedHostView,
        grantScope: "mirror",
      };
      const { store: mirrorStore, inventory: mirrorInventory } = createTestInventory([mirrorHostView]);
      await mirrorInventory.refresh();

      render(
        <RemoteSection
          store={mirrorStore}
          inventory={mirrorInventory}
          onOpenProject={onOpenProject}
          accountSessionToken="test-session-token"
          accountOrigin={DEFAULT_RELAY_ORIGIN}
        />
      );

      expect(await screen.findByText("Enrolled Box")).toBeInTheDocument();
      // Mirror scope cannot open projects: the row's single primary is Connect, never Add Project.
      expect(screen.queryByRole("button", { name: "Add Project on Enrolled Box" })).toBeNull();
      expect(screen.getByRole("button", { name: "Connect Enrolled Box" })).toBeInTheDocument();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("issues an enrollment code from the account bar into the Add Machine wizard and displays the exact CLI command", async () => {
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

      // The account bar opens the wizard directly on its enrollment-code step.
      expect(screen.getByRole("dialog", { name: "Add Machine" })).toBeInTheDocument();
      expect(await screen.findByText("enr_code_xyz")).toBeInTheDocument();
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
    chooseAddMachineMethod("Import SSH Config");

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
    chooseAddMachineMethod("Import SSH Config");

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

    // Open the SSH details drawer and start Edit
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
    // Close the drawer to get back to the rows.
    pressEscape();

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

    // 3. Choose SSH, then connect and submit
    chooseAddMachineMethod("Connect with SSH");
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Reopened Box" } });
    fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "192.168.1.100" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Connect SSH Machine" }));
    });

    // 4. Must successfully verify and offer project (not discarded!)
    expect(screen.getByText(/connected and verified/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Project" })).toBeInTheDocument();
  });

  it("passes supplied non-default account origin to listMachines and issueEnrollmentCode", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    const customOrigin = "https://custom.relay.company.corp";

    const fetchMock = vi.fn().mockImplementation(async (url: string, opts?: RequestInit) => {
      if (url.includes("/api/account/v1/enrollment-codes") && opts?.method === "POST") {
        return {
          ok: true,
          status: 200,
          json: async () => ({ code: "ENROLL-123456", expiresAt: Date.now() + 600000 }),
        };
      }
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
      await act(async () => {
        render(
          <RemoteSection
            store={store}
            inventory={inventory}
            accountOrigin={customOrigin}
            accountSessionToken="test-session-token"
          />,
        );
      });

      expect(fetchMock).toHaveBeenCalledWith(
        `${customOrigin}/api/account/v1/machines`,
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: "Bearer test-session-token",
          }),
        }),
      );

      // Click "Issue Enrollment Code" button
      const issueBtn = screen.getByRole("button", { name: "Issue Enrollment Code" });
      await act(async () => {
        fireEvent.click(issueBtn);
      });

      expect(fetchMock).toHaveBeenCalledWith(
        `${customOrigin}/api/account/v1/enrollment-codes`,
        expect.objectContaining({
          method: "POST",
          headers: expect.objectContaining({
            Authorization: "Bearer test-session-token",
          }),
        }),
      );
      expect(await screen.findByTestId("issued-enrollment-card")).toBeInTheDocument();
      expect(screen.getByTestId("enrollment-command")).toHaveTextContent(
        `ferryx-cli account enroll --code ENROLL-123456 --origin ${customOrigin}`,
      );
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("switches account authority without sending the previous session token to the new server", async () => {
    const { store, inventory } = createTestInventory();
    await inventory.refresh();
    const calls: Array<{ url: string; authorization: string | undefined }> = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, authorization: new Headers(init?.headers).get("Authorization") ?? undefined });
      return { ok: true, status: 200, json: async () => [] };
    }));
    try {
      const view = render(<RemoteSection store={store} inventory={inventory} accountSessionToken="old-account-token" />);
      // The account server card lives in the collapsed Advanced section.
      fireEvent.click(screen.getByText("Advanced", { selector: "summary" }));
      const field = await screen.findByRole("textbox", { name: "Account server URL" });
      fireEvent.change(field, { target: { value: "https://account.selfhost.example" } });
      fireEvent.click(screen.getByRole("button", { name: "Use account server" }));

      expect(screen.getByRole("textbox", { name: "Account server URL" })).toHaveValue("https://account.selfhost.example");
      expect(calls.filter(({ url }) => url.startsWith("https://account.selfhost.example"))).toEqual([]);
      expect(screen.queryByText("Account Active")).toBeNull();
      view.unmount();
      render(<RemoteSection store={store} inventory={inventory} />);
      fireEvent.click(screen.getByText("Advanced", { selector: "summary" }));
      expect(screen.getByRole("textbox", { name: "Account server URL" })).toHaveValue("https://account.selfhost.example");
      expect(screen.queryByText("Account Active")).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("treats account machines with absent grantScope as ungranted and disables Add Project", async () => {
    const absentScopeMachine: AccountMachineView = {
      machineRecordId: "rec-absent",
      machineId: "mach-absent-scope",
      displayName: "Ungranted Account Box",
      publicKey: "pk-absent",
      attachPublicKey: "apk-absent",
      relayOrigin: DEFAULT_RELAY_ORIGIN,
      platform: "linux",
      online: true,
      enrollmentEpoch: 1,
      lastSeenAt: Date.now(),
      // grantScope is omitted (undefined)
    };

    const machineScopedMachine: AccountMachineView = {
      machineRecordId: "rec-machine",
      machineId: "mach-with-machine-scope",
      displayName: "Granted Account Box",
      publicKey: "pk-machine",
      attachPublicKey: "apk-machine",
      relayOrigin: DEFAULT_RELAY_ORIGIN,
      platform: "linux",
      online: true,
      enrollmentEpoch: 1,
      lastSeenAt: Date.now(),
    };

    const fetchMock = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/machines")) {
        return {
          ok: true,
          status: 200,
          json: async () => [absentScopeMachine, machineScopedMachine],
        };
      }
      return { ok: false, status: 404, json: async () => ({}) };
    });
    vi.stubGlobal("fetch", fetchMock);

    try {
      const grantedHostView: HostView = {
        hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "mach-with-machine-scope"),
        machineId: "mach-with-machine-scope",
        relayOrigin: DEFAULT_RELAY_ORIGIN,
        displayLabel: "Granted Account Box",
        grantScope: "machine",
        generation: "1",
        authStatus: "paired",
        online: true,
      };
      const { store, inventory } = createTestInventory([grantedHostView]);
      await inventory.refresh();
      const onOpenProject = vi.fn();

      await act(async () => {
        render(
          <RemoteSection
            store={store}
            inventory={inventory}
            accountOrigin={DEFAULT_RELAY_ORIGIN}
            accountSessionToken="test-token"
            onOpenProject={onOpenProject}
          />,
        );
      });

      // Verify absent-scope machine
      expect(await screen.findByText("Ungranted Account Box")).toBeInTheDocument();
      const absentRow = screen.getByText("Ungranted Account Box").closest("[data-machine-id]");
      expect(absentRow).toBeInTheDocument();
      const absentStatus = absentRow!.querySelector('[data-testid="machine-status"]');
      expect(absentStatus).toHaveAttribute("data-code", "MACHINE_GRANT_REQUIRED");
      expect(absentStatus).toHaveTextContent("Needs access");

      // Projects cannot be added without the grant: the row offers Connect instead of Add Project.
      expect(screen.queryByRole("button", { name: "Add Project on Ungranted Account Box" })).toBeNull();

      // An enrolled machine whose grant was never redeemed on this desktop used to be a dead end:
      // every action disabled, with no way to obtain the grant from here. Offer the way forward.
      const connectBtn = screen.getByRole("button", { name: "Connect Ungranted Account Box" });
      expect(connectBtn).not.toBeDisabled();
      // The granted machine is already usable, so it must NOT show a redundant Connect action.
      expect(screen.queryByRole("button", { name: "Connect Granted Account Box" })).toBeNull();

      // Verify machine-scoped machine
      expect(await screen.findByText("Granted Account Box")).toBeInTheDocument();
      const grantedRow = screen.getByText("Granted Account Box").closest("[data-machine-id]");
      expect(grantedRow).toBeInTheDocument();
      const grantedStatus = grantedRow!.querySelector('[data-testid="machine-status"]');
      expect(grantedStatus).toHaveAttribute("data-code", "READY");
      expect(grantedStatus).toHaveTextContent("Connected");

      const grantedAddBtn = screen.getByRole("button", { name: "Add Project on Granted Account Box" });
      expect(grantedAddBtn).not.toBeDisabled();

      // Open the details drawer on the ungranted machine: metadata shows unknown grant and needsMachineGrant authStatus
      fireEvent.click(screen.getByRole("button", { name: "Details for Ungranted Account Box" }));
      expect(screen.getByText("unknown")).toBeInTheDocument();
      expect(screen.getByText("needsMachineGrant")).toBeInTheDocument();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("re-pair explains the account enrollment path instead of a retired PIN", async () => {
    const unassociatedHostView: HostView = {
      hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "legacy-unassociated-box"),
      machineId: "legacy-unassociated-box",
      relayOrigin: DEFAULT_RELAY_ORIGIN,
      displayLabel: "Legacy Box",
      grantScope: "machine",
      generation: "1",
      authStatus: "paired",
      online: true,
    };
    const { store, inventory } = createTestInventory([unassociatedHostView]);
    await inventory.refresh();

    await act(async () => {
      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={DEFAULT_RELAY_ORIGIN}
          accountSessionToken="test-token"
        />,
      );
    });

    expect(await screen.findByText("Legacy Box")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Details for Legacy Box" }));

    const repairButton = screen.getByRole("button", { name: "Re-pair Legacy Box" });
    fireEvent.click(repairButton);

    const statusEl = screen
      .getAllByRole("status")
      .find((el) => el.getAttribute("data-code") === "REPAIR_REQUIRES_ENROLLMENT");
    expect(statusEl).toBeDefined();
    if (!statusEl) return;
    expect(statusEl.textContent).toContain("enrollment code");
    expect(statusEl.textContent).toContain("ferryx-cli account enroll");
    expect(screen.queryByText(/machine-access PIN/)).toBeNull();
  });

  it("re-pair on an enrolled account machine re-issues its machine grant", async () => {
    grantMocks.attachKey.mockResolvedValue({ publicKey: "test-attach-public-key" });
    grantMocks.requestGrant.mockRejectedValue(new Error("GRANT_STOP_FOR_TEST"));

    const machineScopedMachine: AccountMachineView = {
      machineRecordId: "rec-machine",
      machineId: "mach-with-machine-scope",
      displayName: "Granted Account Box",
      publicKey: "pk-machine",
      attachPublicKey: "apk-machine",
      relayOrigin: DEFAULT_RELAY_ORIGIN,
      platform: "linux",
      online: true,
      enrollmentEpoch: 1,
      lastSeenAt: Date.now(),
    };

    const fetchMock = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/machines")) {
        return {
          ok: true,
          status: 200,
          json: async () => [machineScopedMachine],
        };
      }
      return { ok: false, status: 404, json: async () => ({}) };
    });
    vi.stubGlobal("fetch", fetchMock);

    try {
      const grantedHostView: HostView = {
        hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "mach-with-machine-scope"),
        machineId: "mach-with-machine-scope",
        relayOrigin: DEFAULT_RELAY_ORIGIN,
        displayLabel: "Granted Account Box",
        grantScope: "machine",
        generation: "1",
        authStatus: "paired",
        online: true,
      };
      const { store, inventory } = createTestInventory([grantedHostView]);
      await inventory.refresh();

      await act(async () => {
        render(
          <RemoteSection
            store={store}
            inventory={inventory}
            accountOrigin={DEFAULT_RELAY_ORIGIN}
            accountSessionToken="test-token"
          />,
        );
      });

      expect(await screen.findByText("Granted Account Box")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Details for Granted Account Box" }));

      const repairButton = screen.getByRole("button", { name: "Re-pair Granted Account Box" });
      await act(async () => {
        fireEvent.click(repairButton);
      });

      expect(grantMocks.requestGrant).toHaveBeenCalledTimes(1);
      expect(grantMocks.requestGrant).toHaveBeenCalledWith(
        expect.anything(),
        expect.anything(),
        expect.anything(),
        expect.anything(),
        expect.objectContaining({ grantScope: "machine" }),
      );
      expect(screen.queryByRole("status", { name: /REPAIR_REQUIRES_ENROLLMENT/ })).toBeNull();
      const statusElements = screen.queryAllByRole("status");
      for (const el of statusElements) {
        expect(el).not.toHaveAttribute("data-code", "REPAIR_REQUIRES_ENROLLMENT");
      }
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("reports account session changes to its parent for the resolved origin", async () => {
    const { store, inventory } = createTestInventory([]);
    await inventory.refresh();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        if (url.includes("/api/account/v1/machines")) {
          return { ok: true, status: 200, json: async () => [] };
        }
        return { ok: false, status: 404, json: async () => ({}) };
      }),
    );
    const onAccountSessionChange = vi.fn();

    try {
      const { rerender } = render(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={DEFAULT_RELAY_ORIGIN}
          accountSessionToken={null}
          onAccountSessionChange={onAccountSessionChange}
        />,
      );

      expect(onAccountSessionChange).toHaveBeenCalledWith(null, DEFAULT_RELAY_ORIGIN);

      rerender(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={DEFAULT_RELAY_ORIGIN}
          accountSessionToken="session-token-1"
          onAccountSessionChange={onAccountSessionChange}
        />,
      );

      expect(onAccountSessionChange).toHaveBeenCalledWith("session-token-1", DEFAULT_RELAY_ORIGIN);

      rerender(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={DEFAULT_RELAY_ORIGIN}
          accountSessionToken={null}
          onAccountSessionChange={onAccountSessionChange}
        />,
      );

      const calls = onAccountSessionChange.mock.calls;
      expect(calls[calls.length - 1]).toEqual([null, DEFAULT_RELAY_ORIGIN]);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
