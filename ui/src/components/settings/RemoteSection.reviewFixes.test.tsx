import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteSection } from "./RemoteSection";
import { createRemoteHostStore, remoteHostKey } from "../../state/remoteHostStore";
import { createPairedHostInventory, DEFAULT_RELAY_ORIGIN, type HostView, type PairedHostCommands } from "../../lib/pairedHostInventory";
import { PairedOperationError } from "../../lib/pairedDaemonProject";
import { resetSshHostsCache } from "../../lib/sshHosts";
import { resetTailscaleDiscoveryForTests } from "../../lib/tailscaleDiscovery";
import type { AccountMachineView } from "../../remote/accountSession";

const { isTauriMock, invokeMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

vi.mock("./RemoteAccessSection", () => ({
  RemoteAccessSection: () => <div data-testid="remote-access-section" />,
}));

const pairedView: HostView = {
  hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "paired-box"),
  machineId: "paired-box",
  relayOrigin: DEFAULT_RELAY_ORIGIN,
  displayLabel: "Linux Build Box",
  grantScope: "machine",
  generation: "1",
  authStatus: "paired",
  online: true,
};

const accountMachine: AccountMachineView = {
  machineRecordId: "rec-1",
  machineId: "acc-box-1",
  displayName: "Account Box",
  publicKey: "pub",
  attachPublicKey: "att",
  relayOrigin: DEFAULT_RELAY_ORIGIN,
  platform: "linux",
  online: true,
  enrollmentEpoch: "1",
  lastSeenAt: 1,
};

function createInventory(initialHosts: HostView[] = []) {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = {
    list: vi.fn().mockResolvedValue(initialHosts),
    capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }),
    pair: vi.fn(),
    forget: vi.fn().mockResolvedValue(true),
    migrate: vi.fn(),
    read: vi.fn(),
  };
  return { store, inventory: createPairedHostInventory(store, commands) };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => { resolve = res; });
  return { promise, resolve };
}

const jsonResponse = (payload: unknown, status = 200) =>
  new Response(JSON.stringify(payload), { status, headers: { "Content-Type": "application/json" } });

const sshHost = {
  id: "ssh-box-1",
  label: "Dev Server",
  hostname: "192.168.1.50",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

describe("RemoteSection review fixes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    resetTailscaleDiscoveryForTests();
    localStorage.clear();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockImplementation(async (command: string) => (command === "cmd_ssh_list_hosts" ? [] : undefined));
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    resetTailscaleDiscoveryForTests();
  });

  it("does not repopulate the machine list when a Refresh response lands after sign-out", async () => {
    const { store, inventory } = createInventory();
    await inventory.refresh();
    const late = deferred<Response>();
    let calls = 0;
    vi.stubGlobal("fetch", vi.fn(async () => {
      calls += 1;
      return calls === 1 ? jsonResponse([]) : late.promise;
    }));
    vi.spyOn(await import("../../remote/accountSession"), "logoutAccountSession").mockResolvedValue(undefined);

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} accountOrigin={DEFAULT_RELAY_ORIGIN} accountSessionToken="tok" />);
    });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Refresh machines" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Sign Out" })); });
    await act(async () => { late.resolve(jsonResponse([accountMachine])); });

    expect(screen.queryByText("Account Box")).toBeNull();
  });

  it("surfaces a failed Refresh instead of swallowing it", async () => {
    const { store, inventory } = createInventory();
    await inventory.refresh();
    let calls = 0;
    vi.stubGlobal("fetch", vi.fn(async () => {
      calls += 1;
      return calls === 1 ? jsonResponse([]) : jsonResponse({ code: "RELAY_DOWN", message: "Relay is down" }, 503);
    }));

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} accountOrigin={DEFAULT_RELAY_ORIGIN} accountSessionToken="tok" />);
    });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Refresh machines" })); });

    expect(screen.getByRole("alert")).toHaveTextContent("Relay is down");
  });

  it("maps a structured paired-host error code to its explanation", async () => {
    const { store, inventory } = createInventory([pairedView]);
    await inventory.refresh();
    const negotiate = vi.fn().mockRejectedValue(new PairedOperationError("OFFLINE"));
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} negotiate={negotiate} />); });

    fireEvent.click(screen.getByRole("button", { name: "Details for Linux Build Box" }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Check capabilities for Linux Build Box" })); });

    expect(screen.getAllByTestId("machine-status")[0]).toHaveAttribute("data-code", "OFFLINE");
  });

  it("shows Copy failed (not Copied) when the clipboard write fails and labels the dismiss button", async () => {
    const { store, inventory } = createInventory();
    await inventory.refresh();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_list_hosts") return [];
      if (command === "cmd_clipboard_write_text") throw new Error("clipboard unavailable");
      return undefined;
    });
    vi.stubGlobal("fetch", vi.fn(async (url: string, opts?: RequestInit) =>
      String(url).includes("enrollment-codes") && opts?.method === "POST"
        ? jsonResponse({ code: "ENR-1", expiresAt: Date.now() + 600000 })
        : jsonResponse([])));
    Object.defineProperty(navigator, "clipboard", { value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) }, configurable: true });
    Object.defineProperty(document, "execCommand", { value: vi.fn(() => false), configurable: true });

    await act(async () => {
      render(<RemoteSection store={store} inventory={inventory} accountOrigin={DEFAULT_RELAY_ORIGIN} accountSessionToken="tok" />);
    });
    await act(async () => { fireEvent.click(await screen.findByRole("button", { name: /Issue Enrollment Code/i })); });
    expect(screen.getByRole("button", { name: "Dismiss enrollment code" })).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /Copy/ })); });

    expect(screen.getByRole("button", { name: /Copy failed/ })).toBeInTheDocument();
    expect(screen.queryByText("Copied")).toBeNull();
  });

  it("requires confirmation before deleting an SSH machine", async () => {
    const { store, inventory } = createInventory();
    await inventory.refresh();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_list_hosts") return [sshHost];
      if (command === "cmd_ssh_delete_host") return [];
      return undefined;
    });
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} />); });

    fireEvent.click(await screen.findByRole("button", { name: "Details for Dev Server" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete Dev Server" }));
    expect(invokeMock).not.toHaveBeenCalledWith("cmd_ssh_delete_host", expect.anything());

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Confirm delete Dev Server" })); });
    expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_delete_host", { id: "ssh-box-1" });
  });
});
