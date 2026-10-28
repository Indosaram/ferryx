import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteSection } from "./RemoteSection";
import { createRemoteHostStore, remoteHostKey } from "../../state/remoteHostStore";
import { createPairedHostInventory, DEFAULT_RELAY_ORIGIN, type HostView, type PairedHostCommands } from "../../lib/pairedHostInventory";
import { resetSshHostsCache, updateSshHost, type SshHost, type SshRemoteEnvironment, type SshTargetSummary } from "../../lib/sshHosts";
import { SSH_CONFIG_PATH_STORAGE_KEY } from "../../lib/storageKeys";
import { resetTailscaleDiscoveryForTests } from "../../lib/tailscaleDiscovery";

// SSH machine actions of the live Remote section: row menu (remote/MachineRow.tsx), details drawer
// (remote/MachineDrawer.tsx) and the Add Machine wizard. Ported from the retired SSH settings panel tests.

const { isTauriMock, invokeMock, openDialogMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(),
  openDialogMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openDialogMock(...args),
}));

vi.mock("./RemoteAccessSection", () => ({
  RemoteAccessSection: () => <div data-testid="remote-access-section" />,
}));

vi.mock("../../lib/pairedHostReachability", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/pairedHostReachability")>()),
  usePairedHostReachability: () => ({ supported: false, entry: null }),
}));

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const devServer: SshHost = {
  id: "host-1",
  label: "Dev Server",
  hostname: "192.168.1.100",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

const bastion: SshHost = {
  id: "host-2",
  label: "Bastion Box",
  hostname: "bastion.corp.net",
  username: "ec2-user",
  port: 2222,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

const posixEnvironment: SshRemoteEnvironment = { platform: "posix", executor: "sh", version: "test", home: "/home/test", temp: "/tmp", git: true };

/** IPC args as the fixture reads them; every field is optional because each command sends a different subset. */
interface CommandArgs {
  host?: SshHost;
  id?: string;
  configPath?: string | null;
  [key: string]: unknown;
}

type CommandHandler = (args: CommandArgs) => unknown;

/** Installs the IPC fixture: `hosts` is the saved SSH inventory; handlers override single commands. */
function mockIpc(hosts: SshHost[], handlers: Record<string, CommandHandler> = {}) {
  let current = hosts;
  invokeMock.mockImplementation(async (command: string, rawArgs?: Record<string, unknown>) => {
    const args = (rawArgs ?? {}) as CommandArgs;
    if (handlers[command]) return handlers[command](args);
    switch (command) {
      case "cmd_ssh_list_hosts": return current;
      case "cmd_agent_integration_get": return { enabled: true };
      case "cmd_ssh_read_system_config": return { path: "", exists: false, rawText: "", hosts: [] };
      case "cmd_ssh_test_connection":
        return { host: args.host, reachable: true, checkedAt: 1, environment: posixEnvironment };
      case "cmd_ssh_update_host": {
        const host = args.host as SshHost;
        current = current.some(h => h.id === host.id)
          ? current.map(h => (h.id === host.id ? host : h))
          : [...current, host];
        return current;
      }
      case "cmd_ssh_delete_host":
        current = current.filter(h => h.id !== args.id);
        return current;
      default: return undefined;
    }
  });
}

function createInventory(pairedHosts: HostView[] = []) {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = {
    list: vi.fn().mockResolvedValue(pairedHosts),
    capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }),
    pair: vi.fn(),
    forget: vi.fn().mockResolvedValue(undefined),
    migrate: vi.fn(),
    read: vi.fn(),
  };
  return { store, commands, inventory: createPairedHostInventory(store, commands) };
}

async function renderSection(props: { onOpenSshProject?: (hostId: string) => void } = {}) {
  const { store, inventory } = createInventory();
  await inventory.refresh();
  await act(async () => {
    render(<RemoteSection store={store} inventory={inventory} onOpenSshProject={props.onOpenSshProject ?? vi.fn()} />);
  });
}

function openDetails(label: string) {
  fireEvent.click(screen.getByRole("button", { name: `Details for ${label}` }));
}

function closeDrawer() {
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
}

async function chooseRowMenuItem(label: string, item: string) {
  const trigger = screen.getByRole("button", { name: `More actions for ${label}` });
  trigger.focus();
  fireEvent.keyDown(trigger, { key: "Enter" });
  const menuItem = await screen.findByRole("menuitem", { name: item });
  await act(async () => { fireEvent.click(menuItem); });
}

function drawerStatusCode() {
  return screen.getByTestId("machine-drawer-status").getAttribute("data-code");
}

function chooseAddMachineMethod(name: string) {
  fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
  fireEvent.click(screen.getByRole("radio", { name }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}

/** Opens the wizard on a method whose step loads data on mount, then flushes that load. */
async function chooseAddMachineMethodAndSettle(name: string) {
  chooseAddMachineMethod(name);
  await act(async () => { await Promise.resolve(); });
}

describe("RemoteSection SSH machine actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    resetTailscaleDiscoveryForTests();
    localStorage.clear();
    isTauriMock.mockReturnValue(true);
    mockIpc([devServer]);
  });

  afterEach(() => {
    cleanup();
    resetTailscaleDiscoveryForTests();
  });

  describe("agent integration", () => {
    it("hides Prepare while agent integration is off but keeps Remove available", async () => {
      mockIpc([devServer], { cmd_agent_integration_get: () => ({ enabled: false }) });
      await renderSection();
      openDetails("Dev Server");
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });

      expect(screen.queryByRole("button", { name: "Prepare agent integration on Dev Server" })).toBeNull();
      expect(screen.getByRole("button", { name: "Remove agent integration on Dev Server" })).toBeEnabled();
    });

    it("shows the detected runtime and prepares integration only on explicit action", async () => {
      const environment = { platform: "windows", executor: "powershell", version: "5.1", home: "C:\\Users\\qa", temp: "C:\\Temp", git: false };
      mockIpc([devServer], {
        cmd_ssh_test_connection: ({ host }) => ({ host, reachable: true, checkedAt: 1, environment }),
      });
      await renderSection();
      openDetails("Dev Server");
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });

      expect(screen.getByTestId("ssh-runtime-host-1")).toHaveAttribute("data-platform", "windows");
      expect(screen.getByTestId("ssh-runtime-host-1")).toHaveAttribute("data-git", "false");
      expect(drawerStatusCode()).toBe("REACHABLE");
      expect(invokeMock).not.toHaveBeenCalledWith("cmd_ssh_prepare_integration", expect.anything());

      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Prepare agent integration on Dev Server" })); });
      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_prepare_integration", { host: expect.objectContaining({ id: "host-1" }) });
      expect(drawerStatusCode()).toBe("READY");
    });

    it("surfaces a failed integration preparation in the drawer", async () => {
      mockIpc([devServer], {
        cmd_ssh_prepare_integration: () => { throw { code: "IO_ERROR", message: "Prepare failed on remote" }; },
      });
      await renderSection();
      openDetails("Dev Server");
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Prepare agent integration on Dev Server" })); });

      expect(within(screen.getByRole("dialog")).getByRole("alert")).toHaveTextContent("Prepare failed on remote");
    });

    it("removes integration only after confirmation and only from the chosen host", async () => {
      mockIpc([devServer, bastion], {
        cmd_ssh_remove_integration: () => ({ removed: ["a", "b"], keptModified: ["c"], absent: [] }),
      });
      await renderSection();

      openDetails("Bastion Box");
      fireEvent.click(screen.getByRole("button", { name: "Remove agent integration on Bastion Box" }));
      const cancelDialog = screen.getByRole("dialog", { name: "Remove agent integration from Bastion Box?" });
      fireEvent.click(within(cancelDialog).getByRole("button", { name: "Cancel" }));
      expect(screen.queryByRole("dialog", { name: "Remove agent integration from Bastion Box?" })).toBeNull();
      expect(invokeMock).not.toHaveBeenCalledWith("cmd_ssh_remove_integration", expect.anything());

      openDetails("Bastion Box");
      fireEvent.click(screen.getByRole("button", { name: "Remove agent integration on Bastion Box" }));
      expect(invokeMock).not.toHaveBeenCalledWith("cmd_ssh_remove_integration", expect.anything());
      const confirmDialog = screen.getByRole("dialog", { name: "Remove agent integration from Bastion Box?" });
      await act(async () => { fireEvent.click(within(confirmDialog).getByRole("button", { name: "Remove" })); });

      const removeCalls = invokeMock.mock.calls.filter(([command]) => command === "cmd_ssh_remove_integration");
      expect(removeCalls).toEqual([["cmd_ssh_remove_integration", { hostId: "host-2" }]]);

      openDetails("Bastion Box");
      expect(screen.getByText("Removed 2, kept 1 modified")).toBeInTheDocument();
      closeDrawer();
      openDetails("Dev Server");
      expect(screen.queryByText("Removed 2, kept 1 modified")).toBeNull();
    });
  });

  describe("disable and enable", () => {
    it("disables a machine from the row menu with a busy guard, then enables it again", async () => {
      const pending = deferred<SshHost[]>();
      mockIpc([devServer], { cmd_ssh_update_host: () => pending.promise });
      await renderSection();
      expect(screen.getByRole("button", { name: "Add Project on Dev Server" })).toBeEnabled();

      await chooseRowMenuItem("Dev Server", "Disable");
      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_update_host", { host: expect.objectContaining({ id: "host-1", disabled: true }) });
      expect(screen.getByRole("button", { name: "Add Project on Dev Server" })).toBeDisabled();
      expect(screen.getByTestId("machine-disabled-reason")).toHaveTextContent("Another action is in progress.");

      await act(async () => { pending.resolve([{ ...devServer, disabled: true }]); });
      // A disabled machine stays under My machines with a visible reason.
      expect(screen.getByTestId("machine-status")).toHaveAttribute("data-code", "DISABLED");
      expect(screen.getByRole("button", { name: "Add Project on Dev Server" })).toBeDisabled();
      expect(screen.getByTestId("machine-disabled-reason")).toHaveTextContent("Machine is disabled.");

      mockIpc([{ ...devServer, disabled: true }]);
      await chooseRowMenuItem("Dev Server", "Enable");
      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_update_host", { host: expect.objectContaining({ id: "host-1", disabled: false }) });
      expect(screen.getByRole("button", { name: "Add Project on Dev Server" })).toBeEnabled();
    });

    it("surfaces a structured error when disabling a machine fails", async () => {
      mockIpc([devServer], {
        cmd_ssh_update_host: () => { throw { code: "SSH_TOGGLE_FAILED", message: "Cannot toggle host: machine state is locked" }; },
      });
      await renderSection();
      await chooseRowMenuItem("Dev Server", "Disable");

      expect(screen.getByRole("alert")).toHaveTextContent("Cannot toggle host: machine state is locked");
      expect(screen.getByRole("button", { name: "Add Project on Dev Server" })).toBeEnabled();
    });
  });

  describe("connection test", () => {
    it("shows the in-flight test, then a success and a failure per host", async () => {
      const first = deferred<SshTargetSummary>();
      mockIpc([devServer, bastion], {
        cmd_ssh_test_connection: ({ host }) => host?.id === "host-1"
          ? first.promise
          : { host, reachable: false, lastError: "Connection timed out (port 2222)", checkedAt: 1 },
      });
      await renderSection();

      openDetails("Dev Server");
      fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" }));
      expect(screen.getByRole("button", { name: "Test connection to Dev Server" })).toBeDisabled();
      expect(drawerStatusCode()).toBe("TESTING");
      await act(async () => { first.resolve({ host: devServer, reachable: true, lastError: null, checkedAt: 1 }); });
      expect(drawerStatusCode()).toBe("REACHABLE");
      closeDrawer();

      openDetails("Bastion Box");
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Bastion Box" })); });
      expect(drawerStatusCode()).toBe("ERROR");
      expect(screen.getByText("Connection timed out (port 2222)")).toBeInTheDocument();
    });

    it("shows the message of a rejected connection test", async () => {
      mockIpc([devServer], {
        cmd_ssh_test_connection: () => { throw { code: "SSH_TEST_REJECTED", message: "Network unreachable: host is down" }; },
      });
      await renderSection();
      openDetails("Dev Server");
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });

      expect(drawerStatusCode()).toBe("ERROR");
      expect(screen.getByText("Network unreachable: host is down")).toBeInTheDocument();
    });

    it.each(["success", "failure"] as const)("ignores a late connection %s after the host configuration changes", async (outcome) => {
      const pending = deferred<SshTargetSummary>();
      mockIpc([devServer], { cmd_ssh_test_connection: () => pending.promise });
      await renderSection();
      openDetails("Dev Server");
      fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" }));

      await act(async () => { await updateSshHost({ ...devServer, hostname: "replacement.internal" }); });
      await act(async () => {
        if (outcome === "success") pending.resolve({ host: devServer, reachable: true, checkedAt: 1, environment: posixEnvironment });
        else pending.reject({ code: "IO_ERROR", message: "Old host unreachable" });
      });

      expect(screen.getByText("replacement.internal")).toBeInTheDocument();
      expect(drawerStatusCode()).toBe("UNCHECKED");
      expect(screen.queryByTestId("ssh-runtime-host-1")).toBeNull();
      expect(screen.queryByText("Old host unreachable")).toBeNull();
      expect(screen.getByRole("button", { name: "Test connection to Dev Server" })).toBeEnabled();
    });

    it.each(["success", "failure"] as const)("ignores a late integration %s after the host configuration changes", async (outcome) => {
      const pending = deferred<void>();
      mockIpc([devServer], { cmd_ssh_prepare_integration: () => pending.promise });
      await renderSection();
      openDetails("Dev Server");
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });
      fireEvent.click(screen.getByRole("button", { name: "Prepare agent integration on Dev Server" }));

      await act(async () => { await updateSshHost({ ...devServer, hostname: "replacement.internal" }); });
      await act(async () => {
        if (outcome === "success") pending.resolve();
        else pending.reject({ code: "IO_ERROR", message: "Old integration failed" });
      });
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });

      // The old preparation belonged to the previous configuration, so the host is reachable, not ready.
      expect(drawerStatusCode()).toBe("REACHABLE");
      expect(screen.queryByText("Old integration failed")).toBeNull();
    });
  });

  describe("edit and delete", () => {
    it("keeps a saved password host's method visible in the edit form", async () => {
      mockIpc([{ ...devServer, authMethod: "password" }]);
      await renderSection();
      openDetails("Dev Server");
      fireEvent.click(screen.getByRole("button", { name: "Edit Dev Server" }));

      expect(screen.getByRole("combobox", { name: "Auth Method" })).toHaveTextContent("Password");
      expect(screen.getByLabelText("SSH password")).toBeInTheDocument();
    });

    it("surfaces a structured error when saving an edit fails", async () => {
      mockIpc([devServer], {
        cmd_ssh_update_host: () => { throw { code: "SSH_SAVE_FAILED", message: "Failed to persist SSH host to disk: permission denied" }; },
      });
      await renderSection();
      openDetails("Dev Server");
      fireEvent.click(screen.getByRole("button", { name: "Edit Dev Server" }));
      fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Renamed" } });
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save Changes" })); });

      expect(within(screen.getByRole("dialog")).getByRole("alert")).toHaveTextContent("Failed to persist SSH host to disk: permission denied");
      expect(screen.getByRole("button", { name: "Save Changes" })).toBeInTheDocument();
    });

    it("removes a deleted machine from the list", async () => {
      mockIpc([devServer, bastion]);
      await renderSection();
      openDetails("Dev Server");
      fireEvent.click(screen.getByRole("button", { name: "Delete Dev Server" }));
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Confirm delete Dev Server" })); });
      closeDrawer();

      expect(screen.queryByRole("button", { name: "Details for Dev Server" })).toBeNull();
      expect(screen.getByRole("button", { name: "Details for Bastion Box" })).toBeInTheDocument();
    });

    it("surfaces a structured error when deleting a machine fails and keeps the machine", async () => {
      mockIpc([devServer], {
        cmd_ssh_delete_host: () => { throw { code: "SSH_DELETE_FAILED", message: "Cannot delete host: entry is locked by active session" }; },
      });
      await renderSection();
      openDetails("Dev Server");
      fireEvent.click(screen.getByRole("button", { name: "Delete Dev Server" }));
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Confirm delete Dev Server" })); });

      expect(within(screen.getByRole("dialog")).getByRole("alert")).toHaveTextContent("Cannot delete host: entry is locked by active session");
      closeDrawer();
      expect(screen.getByRole("button", { name: "Details for Dev Server" })).toBeInTheDocument();
    });
  });

  describe("Add Machine wizard", () => {
    it("validates the SSH form before probing the machine", async () => {
      await renderSection();
      chooseAddMachineMethod("Connect with SSH");
      const form = screen.getByRole("button", { name: "Connect SSH Machine" }).closest("form")!;

      await act(async () => { fireEvent.submit(form); });
      expect(screen.getByRole("alert")).toHaveTextContent("Label is required.");

      fireEvent.change(screen.getByLabelText("Label"), { target: { value: "My Machine" } });
      await act(async () => { fireEvent.submit(form); });
      expect(screen.getByRole("alert")).toHaveTextContent("Hostname or IP is required.");

      fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "host.example.com" } });
      fireEvent.change(screen.getByLabelText("Port"), { target: { value: "99999" } });
      await act(async () => { fireEvent.submit(form); });
      expect(screen.getByRole("alert")).toHaveTextContent("Port must be an integer between 1 and 65535.");

      expect(invokeMock).not.toHaveBeenCalledWith("cmd_ssh_test_connection", expect.anything());
      expect(invokeMock).not.toHaveBeenCalledWith("cmd_ssh_update_host", expect.anything());
    });

    it("surfaces a structured error when saving a new SSH machine fails", async () => {
      mockIpc([devServer], {
        cmd_ssh_update_host: () => { throw { code: "SSH_SAVE_FAILED", message: "Failed to persist SSH host to disk: permission denied" }; },
      });
      await renderSection();
      chooseAddMachineMethod("Connect with SSH");
      fireEvent.change(screen.getByLabelText("Label"), { target: { value: "New Box" } });
      fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "new.internal" } });
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Connect SSH Machine" })); });

      expect(screen.getByRole("alert")).toHaveTextContent("Failed to persist SSH host to disk: permission denied");
      expect(screen.queryByRole("button", { name: "Details for New Box" })).toBeNull();
    });

    it("surfaces a system SSH config read failure", async () => {
      mockIpc([devServer], {
        cmd_ssh_read_system_config: () => { throw { code: "IO_ERROR", message: "fixture-config-read-error" }; },
      });
      await renderSection();
      await chooseAddMachineMethodAndSettle("Import SSH Config");

      expect(screen.getByRole("alert")).toHaveTextContent("fixture-config-read-error");
    });

    it("reads hosts from a user-selected config file instead of the default path", async () => {
      const workHost: SshHost = { ...devServer, id: "ssh-work-box", label: "work-box", hostname: "work.example", source: "config" };
      openDialogMock.mockResolvedValue("/tmp/work-ssh-config");
      mockIpc([devServer], {
        cmd_ssh_read_system_config: ({ configPath }) => configPath === "/tmp/work-ssh-config"
          ? { path: configPath, exists: true, rawText: "Host work-box\n  HostName work.example\n", hosts: [workHost] }
          : { path: "/Users/test/.ssh/config", exists: true, rawText: "Host dev\n  HostName dev.example\n", hosts: [devServer] },
      });
      await renderSection();
      await chooseAddMachineMethodAndSettle("Import SSH Config");
      expect(screen.getByText("/Users/test/.ssh/config")).toBeInTheDocument();

      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Choose custom SSH config file" })); });

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_read_system_config", { configPath: "/tmp/work-ssh-config" });
      expect(screen.getByText("/tmp/work-ssh-config")).toBeInTheDocument();
      expect(screen.getByText("Custom SSH Config")).toBeInTheDocument();
      expect(localStorage.getItem(SSH_CONFIG_PATH_STORAGE_KEY)).toBe("/tmp/work-ssh-config");
    });

    it("restores the previously selected config file and can revert to the default", async () => {
      localStorage.setItem(SSH_CONFIG_PATH_STORAGE_KEY, "/tmp/work-ssh-config");
      mockIpc([devServer], {
        cmd_ssh_read_system_config: ({ configPath }) => ({
          path: configPath ?? "/Users/test/.ssh/config",
          exists: true,
          rawText: "Host dev\n  HostName dev.example\n",
          hosts: [devServer],
        }),
      });
      await renderSection();
      await chooseAddMachineMethodAndSettle("Import SSH Config");

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_read_system_config", { configPath: "/tmp/work-ssh-config" });
      expect(screen.getByText("/tmp/work-ssh-config")).toBeInTheDocument();

      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Use default" })); });

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_read_system_config", { configPath: null });
      expect(screen.getByText("/Users/test/.ssh/config")).toBeInTheDocument();
      expect(localStorage.getItem(SSH_CONFIG_PATH_STORAGE_KEY)).toBeNull();
    });

    it.each([
      { runtime: "desktop app", desktop: true },
      { runtime: "browser", desktop: false },
    ])("offers the custom config file chooser only inside the desktop app ($runtime)", async ({ desktop }) => {
      const pendingConfig = deferred<unknown>();
      mockIpc([devServer], { cmd_ssh_read_system_config: () => pendingConfig.promise });
      await renderSection();
      await chooseAddMachineMethodAndSettle("Import SSH Config");
      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_read_system_config", { configPath: null });

      // Outside Tauri readSystemSshConfig never returns hosts, so the System Config card would not render
      // at all. Let the read start in the desktop runtime and render its result in the chosen runtime.
      isTauriMock.mockReturnValue(desktop);
      await act(async () => {
        pendingConfig.resolve({ path: "/Users/test/.ssh/config", exists: true, rawText: "Host dev\n  HostName dev.example\n", hosts: [devServer] });
      });

      expect(screen.getByText("System SSH Config")).toBeInTheDocument();
      if (desktop) {
        expect(screen.getByRole("button", { name: "Choose custom SSH config file" })).toBeInTheDocument();
      } else {
        expect(screen.queryByRole("button", { name: "Choose custom SSH config file" })).toBeNull();
      }
    });

    it("does not probe a new SSH machine twice while the first submit is pending", async () => {
      const pendingProbe = deferred<SshTargetSummary>();
      mockIpc([devServer], { cmd_ssh_test_connection: () => pendingProbe.promise });
      await renderSection();
      chooseAddMachineMethod("Connect with SSH");
      fireEvent.change(screen.getByLabelText("Label"), { target: { value: "New Box" } });
      fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "new.internal" } });
      const submit = screen.getByRole("button", { name: "Connect SSH Machine" });
      const form = submit.closest("form")!;

      await act(async () => { fireEvent.submit(form); });
      expect(screen.getByRole("button", { name: "Testing & Connecting…" })).toBeDisabled();
      // Submitting the form directly bypasses the disabled button and reaches the handler's own busy guard.
      await act(async () => { fireEvent.submit(form); });
      const probeCalls = () => invokeMock.mock.calls.filter(([command]) => command === "cmd_ssh_test_connection");
      expect(probeCalls()).toHaveLength(1);

      await act(async () => {
        const probeArgs = probeCalls()[0][1] as { host: SshHost };
        pendingProbe.resolve({ host: probeArgs.host, reachable: true, checkedAt: 1, environment: posixEnvironment });
      });
      expect(probeCalls()).toHaveLength(1);
      expect(invokeMock.mock.calls.filter(([command]) => command === "cmd_ssh_update_host")).toHaveLength(1);
    });
  });

  describe("paired machine forget", () => {
    const pairedHost: HostView = {
      hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "paired-box"),
      machineId: "paired-box",
      relayOrigin: DEFAULT_RELAY_ORIGIN,
      displayLabel: "Linux Build Box",
      grantScope: "machine",
      generation: "1",
      authStatus: "paired",
      online: true,
    };

    it("forgets paired credentials only after explicit confirmation, and cancel keeps them", async () => {
      const { store, commands, inventory } = createInventory([pairedHost]);
      await inventory.refresh();
      await act(async () => {
        render(<RemoteSection store={store} inventory={inventory} onOpenSshProject={vi.fn()} />);
      });

      openDetails("Linux Build Box");
      fireEvent.click(screen.getByRole("button", { name: "Forget Linux Build Box" }));
      expect(commands.forget).not.toHaveBeenCalled();
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel forget" })); });
      expect(screen.queryByRole("group", { name: "Confirm credential removal" })).toBeNull();
      expect(commands.forget).not.toHaveBeenCalled();
      expect(store.getState().hosts[pairedHost.hostId]).toBeDefined();

      fireEvent.click(screen.getByRole("button", { name: "Forget Linux Build Box" }));
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Confirm forget Linux Build Box" })); });

      expect(commands.forget).toHaveBeenCalledTimes(1);
      expect(commands.forget).toHaveBeenCalledWith({ hostId: pairedHost.hostId, generation: "1" });
      expect(store.getState().hosts[pairedHost.hostId]).toBeUndefined();
    });
  });
});
