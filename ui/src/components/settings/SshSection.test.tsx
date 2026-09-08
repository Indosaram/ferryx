import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resetSshHostsCache, updateSshHost, type SshHost, type SshTargetSummary } from "../../lib/sshHosts";
import { SshSection } from "./SshSection";

const invokeMock = vi.fn();
const isTauriMock = vi.fn(() => true);
const openDialogMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: () => isTauriMock(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openDialogMock(...args),
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

const mockHost1: SshHost = {
  id: "host-1",
  label: "Dev Server",
  hostname: "192.168.1.100",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

const mockHost2: SshHost = {
  id: "host-2",
  label: "Bastion Box",
  hostname: "bastion.corp.net",
  username: "ec2-user",
  port: 2222,
  identityFile: "~/.ssh/bastion_key",
  jumpHost: "gateway.corp.net",
  source: "config",
  authMethod: "key",
  disabled: true,
};

describe("SshSection Settings Component", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    localStorage.clear();
    isTauriMock.mockReturnValue(true);
  });

  afterEach(cleanup);


  it("shows the detected runtime and prepares integration only on explicit action", async () => {
    const environment = { platform: "windows", executor: "powershell", version: "5.1", home: "C:\\Users\\qa", temp: "C:\\Temp", git: false };
    invokeMock.mockImplementation(async (command: string) => {
      switch (command) {
        case "cmd_ssh_list_hosts": return [mockHost1];
        case "cmd_ssh_read_system_config": return { path: "", exists: false, rawText: "", hosts: [] };
        case "cmd_ssh_test_connection": return { host: mockHost1, reachable: true, checkedAt: 1, environment };
        case "cmd_ssh_prepare_integration": return undefined;
        default: throw new Error(`Unexpected command ${command}`);
      }
    });
    await act(async () => { render(<SshSection />); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });
    expect(screen.getByTestId("ssh-runtime-host-1")).toHaveAttribute("data-platform", "windows");
    expect(screen.getByTestId("ssh-runtime-host-1")).toHaveAttribute("data-git", "false");
    expect(invokeMock).not.toHaveBeenCalledWith("cmd_ssh_prepare_integration", expect.anything());
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Prepare agent integration on Dev Server" })); });
    expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_prepare_integration", { host: mockHost1 });
  });

  it("renders empty state when no SSH machines are configured", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);

    expect(screen.getByText("Loading SSH machines…")).toBeInTheDocument();

    await act(async () => {
      listDef.resolve([]);
    });

    expect(screen.getByTestId("ssh-empty-state")).toBeInTheDocument();
    expect(screen.getByText("No SSH machines configured")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Add Machine" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "Import Config" })).toHaveLength(1);
  });

  it("disables connection tests and integration preparation while editing", async () => {
    const environment = { platform: "posix", executor: "sh", version: "test", home: "/home/test", temp: "/tmp", git: true };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_list_hosts") return [mockHost1];
      if (command === "cmd_ssh_test_connection") return { host: mockHost1, reachable: true, checkedAt: 1, environment };
      if (command === "cmd_ssh_read_system_config") return { path: "", exists: false, hosts: [], rawText: "" };
      throw new Error(`Unexpected command ${command}`);
    });
    await act(async () => { render(<SshSection />); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });
    fireEvent.click(screen.getByRole("button", { name: "Edit Dev Server" }));
    expect(screen.getByRole("button", { name: "Test connection to Dev Server" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Prepare agent integration on Dev Server" })).toBeDisabled();
  });

  it.each(["success", "failure"] as const)("ignores a late connection %s after the host configuration changes", async (outcome) => {
    const pending = deferred<SshTargetSummary>();
    let current = mockHost1;
    invokeMock.mockImplementation(async (command: string, args?: { host?: SshHost }) => {
      if (command === "cmd_ssh_list_hosts") return [current];
      if (command === "cmd_ssh_test_connection") return pending.promise;
      if (command === "cmd_ssh_read_system_config") return { path: "", exists: false, hosts: [], rawText: "" };
      if (command === "cmd_ssh_update_host" && args?.host) {
        current = args.host;
        return [current];
      }
      throw new Error(`Unexpected command ${command}`);
    });
    await act(async () => { render(<SshSection />); });
    fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" }));
    await act(async () => { await updateSshHost({ ...mockHost1, hostname: "replacement.internal" }); });
    await act(async () => {
      if (outcome === "success") pending.resolve({ host: mockHost1, reachable: true, checkedAt: 1 });
      else pending.reject({ code: "IO_ERROR", message: "Old host unreachable" });
    });
    expect(current.hostname).toBe("replacement.internal");
    expect(screen.queryByTestId("ssh-test-success-host-1")).not.toBeInTheDocument();
    expect(screen.queryByTestId("ssh-test-error-host-1")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Test connection to Dev Server" })).toBeEnabled();
  });

  it.each(["success", "failure"] as const)("ignores a late integration %s after the host configuration changes", async (outcome) => {
    const pending = deferred<void>();
    const environment = { platform: "posix", executor: "sh", version: "test", home: "/home/test", temp: "/tmp", git: true };
    let current = mockHost1;
    invokeMock.mockImplementation(async (command: string, args?: { host?: SshHost }) => {
      if (command === "cmd_ssh_list_hosts") return [current];
      if (command === "cmd_ssh_test_connection") return { host: current, reachable: true, checkedAt: 1, environment };
      if (command === "cmd_ssh_prepare_integration") return pending.promise;
      if (command === "cmd_ssh_read_system_config") return { path: "", exists: false, hosts: [], rawText: "" };
      if (command === "cmd_ssh_update_host" && args?.host) { current = args.host; return [current]; }
      throw new Error(`Unexpected command ${command}`);
    });
    await act(async () => { render(<SshSection />); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });
    fireEvent.click(screen.getByRole("button", { name: "Prepare agent integration on Dev Server" }));
    await act(async () => { await updateSshHost({ ...mockHost1, hostname: "replacement.internal" }); });
    await act(async () => {
      if (outcome === "success") pending.resolve();
      else pending.reject({ code: "IO_ERROR", message: "Old integration failed" });
    });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test connection to Dev Server" })); });
    expect(screen.getByRole("button", { name: "Prepare agent integration on Dev Server" })).toHaveTextContent("Prepare agent integration");
    expect(screen.queryByText("Old integration failed")).not.toBeInTheDocument();
  });

  it("renders mounted inventory with badges, endpoints, and status", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);

    await act(async () => {
      listDef.resolve([mockHost1, mockHost2]);
    });

    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-host-host-2")).toBeInTheDocument();

    // Check host 1 details
    expect(screen.getByText("Dev Server")).toBeInTheDocument();
    expect(screen.getByText("ubuntu@192.168.1.100:22")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-status-badge-host-1")).toHaveTextContent("Active");

    // Check host 2 details
    expect(screen.getByText("Bastion Box")).toBeInTheDocument();
    expect(screen.getByText("ec2-user@bastion.corp.net:2222 via gateway.corp.net")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-status-badge-host-2")).toHaveTextContent("Disabled");
  });

  it("validates form input when adding a machine manually", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);
    await act(async () => {
      listDef.resolve([mockHost1]);
    });
    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();

    // Open add machine form
    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
    expect(screen.getByText("Add SSH Machine")).toBeInTheDocument();

    // Attempt to submit with empty fields
    fireEvent.click(screen.getByRole("button", { name: "Save Machine" }));

    expect(screen.getByRole("alert")).toHaveTextContent("Label is required.");

    // Fill label but omit hostname
    fireEvent.change(screen.getByLabelText(/^label/i), { target: { value: "My Machine" } });
    fireEvent.click(screen.getByRole("button", { name: "Save Machine" }));

    expect(screen.getByRole("alert")).toHaveTextContent("Hostname or IP is required.");

    // Fill hostname with invalid port
    const hostnameInput = screen.getByLabelText(/^hostname/i);
    fireEvent.change(hostnameInput, { target: { value: "host.example.com" } });
    const portInput = screen.getByLabelText(/^port/i);
    fireEvent.change(portInput, { target: { value: "99999" } });

    await act(async () => {
      fireEvent.submit(screen.getByRole("button", { name: "Save Machine" }).closest("form")!);
    });

    expect(screen.getByRole("alert")).toHaveTextContent("Port must be an integer between 1 and 65535.");

    // Cancel form
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText("Add SSH Machine")).toBeNull();
  });

  it("saves a new machine, disables duplicate submit while pending, and mounts it into inventory", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);
    await act(async () => {
      listDef.resolve([mockHost1]);
    });
    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));

    fireEvent.change(screen.getByLabelText(/label/i), { target: { value: "Staging Box" } });
    fireEvent.change(screen.getByLabelText(/hostname/i), { target: { value: "staging.internal" } });
    fireEvent.change(screen.getByLabelText(/username/i), { target: { value: "dev" } });
    fireEvent.change(screen.getByLabelText(/port/i), { target: { value: "2200" } });

    const newHost: SshHost = {
      id: "host-staging",
      label: "Staging Box",
      hostname: "staging.internal",
      username: "dev",
      port: 2200,
      source: "manual",
      authMethod: "agent",
      disabled: false,
    };

    const def = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(def.promise);

    // Submit the form
    fireEvent.click(screen.getByRole("button", { name: "Save Machine" }));

    // Submit button should be disabled with Saving... indicator (duplicate action prevention)
    const submitBtn = screen.getByRole("button", { name: /saving/i });
    expect(submitBtn).toBeDisabled();

    // Verify invoke was called with cleaned host
    expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_update_host", {
      host: expect.objectContaining({
        label: "Staging Box",
        hostname: "staging.internal",
        username: "dev",
        port: 2200,
        source: "manual",
        authMethod: "agent",
        disabled: false,
      }),
    });

    // Resolve update
    await act(async () => {
      def.resolve([mockHost1, newHost]);
    });

    // Form should close and new host should appear in inventory
    expect(screen.queryByText("Add SSH Machine")).toBeNull();
    expect(screen.getByTestId("ssh-host-host-staging")).toBeInTheDocument();
    expect(screen.getByText("Staging Box")).toBeInTheDocument();
    expect(screen.getByText("dev@staging.internal:2200")).toBeInTheDocument();
  });

  it("edits an existing machine and updates the inventory", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);
    await act(async () => {
      listDef.resolve([mockHost1]);
    });
    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Edit Dev Server" }));

    expect(screen.getByText("Edit SSH Machine")).toBeInTheDocument();
    expect(screen.getByLabelText(/label/i)).toHaveValue("Dev Server");
    expect(screen.getByLabelText(/hostname/i)).toHaveValue("192.168.1.100");

    // Modify label
    fireEvent.change(screen.getByLabelText(/label/i), { target: { value: "Updated Dev Server" } });

    const updatedHost: SshHost = {
      ...mockHost1,
      label: "Updated Dev Server",
    };

    const updateDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(updateDef.promise);

    fireEvent.click(screen.getByRole("button", { name: "Update Machine" }));

    await act(async () => {
      updateDef.resolve([updatedHost]);
    });

    expect(screen.queryByText("Edit SSH Machine")).toBeNull();
    expect(screen.getByText("Updated Dev Server")).toBeInTheDocument();

    expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_update_host", {
      host: expect.objectContaining({
        id: "host-1",
        label: "Updated Dev Server",
      }),
    });
  });

  it("toggles enabled/disabled state via switch with async busy guard", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);
    await act(async () => {
      listDef.resolve([mockHost1]);
    });
    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();

    const toggle = screen.getByRole("switch", { name: "Enable Dev Server" });
    expect(toggle).toHaveAttribute("aria-checked", "true");

    const def = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(def.promise);

    // Toggle off
    fireEvent.click(toggle);

    // Switch should be disabled during inflight update
    expect(toggle).toBeDisabled();

    const disabledHost: SshHost = { ...mockHost1, disabled: true };
    await act(async () => {
      def.resolve([disabledHost]);
    });

    expect(toggle).not.toBeDisabled();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("ssh-status-badge-host-1")).toHaveTextContent("Disabled");
  });

  it("deletes a machine from the inventory with async guard", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);
    await act(async () => {
      listDef.resolve([mockHost1, mockHost2]);
    });
    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-host-host-2")).toBeInTheDocument();

    const deleteBtn = screen.getByRole("button", { name: "Delete Dev Server" });

    const def = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(def.promise);

    fireEvent.click(deleteBtn);

    expect(deleteBtn).toBeDisabled();
    expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_delete_host", { id: "host-1" });

    await act(async () => {
      def.resolve([mockHost2]);
    });

    expect(screen.queryByTestId("ssh-host-host-1")).toBeNull();
    expect(screen.getByTestId("ssh-host-host-2")).toBeInTheDocument();
  });

  it("imports pasted SSH configuration and updates inventory", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);
    await act(async () => {
      listDef.resolve([mockHost1]);
    });
    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Import Config" }));

    expect(screen.getByText("Import from SSH Config")).toBeInTheDocument();

    // Try importing empty
    const textarea = screen.getByLabelText("Configuration Content");
    fireEvent.change(textarea, { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Import Machines" }));

    expect(screen.getByRole("alert")).toHaveTextContent("Please paste SSH configuration text.");

    // Fill valid config
    const pastedConfig = "Host imported-box\n  HostName 10.10.10.10\n  User admin\n  Port 22";
    fireEvent.change(textarea, { target: { value: pastedConfig } });

    const importedHost: SshHost = {
      id: "imported-box",
      label: "imported-box",
      hostname: "10.10.10.10",
      username: "admin",
      port: 22,
      source: "config",
      authMethod: "agent",
      disabled: false,
    };

    const def = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(def.promise);

    fireEvent.click(screen.getByRole("button", { name: "Import Machines" }));

    // Import button disabled during flight
    expect(screen.getByRole("button", { name: /importing/i })).toBeDisabled();

    await act(async () => {
      def.resolve([mockHost1, importedHost]);
    });

    expect(screen.queryByText("Import from SSH Config")).toBeNull();
    expect(screen.getByTestId("ssh-host-imported-box")).toBeInTheDocument();
    expect(screen.getByText("admin@10.10.10.10:22")).toBeInTheDocument();
  });

  it("tests host connection and renders visible success and error results", async () => {
    const listDef = deferred<SshHost[]>();
    invokeMock.mockReturnValueOnce(listDef.promise);

    render(<SshSection />);
    await act(async () => {
      listDef.resolve([mockHost1, mockHost2]);
    });
    expect(screen.getByTestId("ssh-host-host-1")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-host-host-2")).toBeInTheDocument();

    const testBtn1 = screen.getByRole("button", { name: "Test connection to Dev Server" });

    // Test successful connection for host 1
    const def1 = deferred<SshTargetSummary>();
    invokeMock.mockReturnValueOnce(def1.promise);

    fireEvent.click(testBtn1);

    expect(testBtn1).toBeDisabled();
    expect(screen.getByText(/testing connection/i)).toBeInTheDocument();

    await act(async () => {
      def1.resolve({
        host: mockHost1,
        reachable: true,
        lastError: null,
        checkedAt: Date.now(),
      });
    });

    expect(screen.getByTestId("ssh-test-success-host-1")).toHaveTextContent("Connection verified");

    // Test failed connection for host 2
    const testBtn2 = screen.getByRole("button", { name: "Test connection to Bastion Box" });
    const def2 = deferred<SshTargetSummary>();
    invokeMock.mockReturnValueOnce(def2.promise);

    fireEvent.click(testBtn2);

    await act(async () => {
      def2.resolve({
        host: mockHost2,
        reachable: false,
        lastError: "Connection timed out (port 2222)",
        checkedAt: Date.now(),
      });
    });

    const errorAlert = screen.getByTestId("ssh-test-error-host-2");
    expect(errorAlert).toBeInTheDocument();
    expect(errorAlert).toHaveTextContent("Failed: Connection timed out (port 2222)");
  });

  describe("Structured IPC mutation error surfacing", () => {
    it("surfaces backend error message when saving a machine fails with structured IPC error", async () => {
      const listDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(listDef.promise);

      render(<SshSection />);
      await act(async () => {
        listDef.resolve([mockHost1]);
      });

      fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
      fireEvent.change(screen.getByLabelText(/label/i), { target: { value: "New Box" } });
      fireEvent.change(screen.getByLabelText(/hostname/i), { target: { value: "new.internal" } });

      const saveDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(saveDef.promise);

      fireEvent.click(screen.getByRole("button", { name: "Save Machine" }));

      await act(async () => {
        saveDef.reject({
          code: "SSH_SAVE_FAILED",
          message: "Failed to persist SSH host to disk: permission denied",
        });
      });

      expect(screen.getByRole("alert")).toHaveTextContent(
        "Failed to persist SSH host to disk: permission denied",
      );
    });

    it("surfaces backend error message when importing config fails with structured IPC error", async () => {
      const listDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(listDef.promise);

      render(<SshSection />);
      await act(async () => {
        listDef.resolve([mockHost1]);
      });

      fireEvent.click(screen.getByRole("button", { name: "Import Config" }));
      const textarea = screen.getByLabelText("Configuration Content");
      fireEvent.change(textarea, { target: { value: "Host bad-config\n  HostName 1.2.3.4" } });

      const importDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(importDef.promise);

      fireEvent.click(screen.getByRole("button", { name: "Import Machines" }));

      await act(async () => {
        importDef.reject({
          code: "SSH_IMPORT_FAILED",
          message: "Configuration file contains invalid directive at line 2",
        });
      });

      expect(screen.getByRole("alert")).toHaveTextContent(
        "Configuration file contains invalid directive at line 2",
      );
    });

    it("surfaces backend error message when deleting a machine fails with structured IPC error", async () => {
      const listDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(listDef.promise);

      render(<SshSection />);
      await act(async () => {
        listDef.resolve([mockHost1]);
      });

      const deleteDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(deleteDef.promise);

      fireEvent.click(screen.getByRole("button", { name: "Delete Dev Server" }));

      await act(async () => {
        deleteDef.reject({
          code: "SSH_DELETE_FAILED",
          message: "Cannot delete host: entry is locked by active session",
        });
      });

      expect(screen.getByRole("alert")).toHaveTextContent(
        "Cannot delete host: entry is locked by active session",
      );
    });

    it("surfaces backend error message when toggling a machine fails with structured IPC error", async () => {
      const listDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(listDef.promise);

      render(<SshSection />);
      await act(async () => {
        listDef.resolve([mockHost1]);
      });

      const toggle = screen.getByRole("switch", { name: "Enable Dev Server" });
      const toggleDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(toggleDef.promise);

      fireEvent.click(toggle);

      await act(async () => {
        toggleDef.reject({
          code: "SSH_TOGGLE_FAILED",
          message: "Cannot toggle host: machine state is locked",
        });
      });

      expect(screen.getByRole("alert")).toHaveTextContent(
        "Cannot toggle host: machine state is locked",
      );
    });

    it("surfaces backend error message when testing connection rejects with structured IPC error", async () => {
      const listDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(listDef.promise);

      render(<SshSection />);
      await act(async () => {
        listDef.resolve([mockHost1]);
      });

      const testBtn = screen.getByRole("button", { name: "Test connection to Dev Server" });
      const testDef = deferred<SshTargetSummary>();
      invokeMock.mockReturnValueOnce(testDef.promise);

      fireEvent.click(testBtn);

      await act(async () => {
        testDef.reject({
          code: "SSH_TEST_REJECTED",
          message: "Network unreachable: host is down",
        });
      });

      const errorAlert = screen.getByTestId("ssh-test-error-host-1");
      expect(errorAlert).toBeInTheDocument();
      expect(errorAlert).toHaveTextContent("Failed: Network unreachable: host is down");
    });
  });

  describe("System SSH Config Auto-Discovery and Viewer", () => {
    it("surfaces system config read failures and allows retrying", async () => {
      const systemRead = deferred<never>();
      invokeMock.mockImplementation((command) => {
        if (command === "cmd_ssh_list_hosts") return Promise.resolve([]);
        if (command === "cmd_ssh_read_system_config") return systemRead.promise;
        return Promise.resolve();
      });
      render(<SshSection />);

      await act(async () => {
        systemRead.reject({ code: "IO_ERROR", message: "fixture-config-read-error" });
      });

      expect(screen.getByRole("alert")).toHaveTextContent("fixture-config-read-error");
      invokeMock.mockResolvedValueOnce({
        path: "/fixture/.ssh/config",
        exists: true,
        rawText: "Host dev\n  HostName dev.example\n",
        hosts: [mockHost1],
      });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      });
      expect(screen.queryByRole("alert")).toBeNull();
      expect(screen.getByRole("button", { name: /Import All \(1\)/i })).toBeEnabled();
    });

    it("loads and displays discovered hosts from ~/.ssh/config and allows importing them", async () => {
      const listDef = deferred<SshHost[]>();
      const sysDef = deferred<unknown>();

      invokeMock.mockImplementation((cmd) => {
        if (cmd === "cmd_ssh_list_hosts") return listDef.promise;
        if (cmd === "cmd_ssh_read_system_config") return sysDef.promise;
        if (cmd === "cmd_ssh_import_config") return Promise.resolve([]);
        return Promise.resolve();
      });

      render(<SshSection />);

      await act(async () => {
        listDef.resolve([]);
        sysDef.resolve({
          path: "/Users/test/.ssh/config",
          exists: true,
          rawText: "Host remote-vps\n  HostName 10.20.30.40\n  User debian\n  Port 22\n",
          hosts: [
            {
              id: "ssh-remote-vps",
              label: "remote-vps",
              hostname: "10.20.30.40",
              username: "debian",
              port: 22,
              source: "config",
              authMethod: "agent",
              disabled: false,
            },
          ],
        });
      });

      // Shows System SSH Config Card
      expect(screen.getByText("System SSH Config")).toBeInTheDocument();
      expect(screen.getByText("1 hosts found")).toBeInTheDocument();
      expect(screen.getByText("1 new")).toBeInTheDocument();
      expect(screen.getByText("/Users/test/.ssh/config")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /Import All \(1\)/i })).toBeInTheDocument();

      // Toggle Show Hosts
      fireEvent.click(screen.getByRole("button", { name: /Show Hosts/i }));
      expect(screen.getByText("remote-vps")).toBeInTheDocument();
      expect(screen.getByText("debian@10.20.30.40:22")).toBeInTheDocument();

      // Toggle View Raw Config
      fireEvent.click(screen.getByRole("button", { name: "View Raw Config" }));
      expect(screen.getByText(/Host remote-vps/)).toBeInTheDocument();

      // Click Import All
      fireEvent.click(screen.getByRole("button", { name: /Import All \(1\)/i }));
      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_import_config", {
        configText: "Host remote-vps\n  HostName 10.20.30.40\n  User debian\n  Port 22\n",
      });
    });

    it("reads hosts from a user-selected config file instead of the default path", async () => {
      const workHost: SshHost = {
        id: "ssh-work-box",
        label: "work-box",
        hostname: "work.example",
        username: "dev",
        port: 22,
        source: "config",
        authMethod: "agent",
      };

      openDialogMock.mockResolvedValue("/tmp/work-ssh-config");
      invokeMock.mockImplementation((cmd, args) => {
        if (cmd === "cmd_ssh_list_hosts") return Promise.resolve([]);
        if (cmd === "cmd_ssh_read_system_config") {
          const configPath = (args as { configPath?: string | null } | undefined)?.configPath ?? null;
          if (configPath === "/tmp/work-ssh-config") {
            return Promise.resolve({
              path: configPath,
              exists: true,
              rawText: "Host work-box\n  HostName work.example\n  User dev\n",
              hosts: [workHost],
            });
          }
          return Promise.resolve({
            path: "/Users/test/.ssh/config",
            exists: true,
            rawText: "",
            hosts: [],
          });
        }
        return Promise.resolve();
      });

      render(<SshSection />);
      await act(async () => {});

      expect(screen.getByText("/Users/test/.ssh/config")).toBeInTheDocument();

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: /Choose File/i }));
      });

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_read_system_config", {
        configPath: "/tmp/work-ssh-config",
      });
      expect(screen.getByText("/tmp/work-ssh-config")).toBeInTheDocument();
      expect(screen.getByText("1 hosts found")).toBeInTheDocument();
    });

    it("restores the previously selected config file on remount and can revert to the default", async () => {
      localStorage.setItem("ferryx.ssh.configPath", "/tmp/work-ssh-config");

      invokeMock.mockImplementation((cmd, args) => {
        if (cmd === "cmd_ssh_list_hosts") return Promise.resolve([]);
        if (cmd === "cmd_ssh_read_system_config") {
          const configPath = (args as { configPath?: string | null } | undefined)?.configPath ?? null;
          return Promise.resolve({
            path: configPath ?? "/Users/test/.ssh/config",
            exists: true,
            rawText: "",
            hosts: [],
          });
        }
        return Promise.resolve();
      });

      render(<SshSection />);
      await act(async () => {});

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_read_system_config", {
        configPath: "/tmp/work-ssh-config",
      });
      expect(screen.getByText("/tmp/work-ssh-config")).toBeInTheDocument();

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: /Use Default/i }));
      });

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_read_system_config", { configPath: null });
      expect(screen.getByText("/Users/test/.ssh/config")).toBeInTheDocument();
      expect(localStorage.getItem("ferryx.ssh.configPath")).toBeNull();
    });

    it("does not render the Choose File button when running outside Tauri desktop (web/remote mode)", async () => {
      isTauriMock.mockReturnValue(false);
      invokeMock.mockImplementation((cmd) => {
        if (cmd === "cmd_ssh_list_hosts") return Promise.resolve([]);
        if (cmd === "cmd_ssh_read_system_config") {
          return Promise.resolve({
            path: "~/.ssh/config",
            exists: true,
            rawText: "",
            hosts: [],
          });
        }
        return Promise.resolve();
      });

      render(<SshSection />);
      await act(async () => {});

      expect(screen.queryByRole("button", { name: /Choose File/i })).not.toBeInTheDocument();
    });
  });
});
