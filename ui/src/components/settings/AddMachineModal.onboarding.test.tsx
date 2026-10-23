import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AddMachineModal, installKeyErrorMessage } from "./AddMachineModal";
import { AccountSessionError, type AccountMachineView, type IssueInstallKeyResponse, type MachineRequestView } from "../../remote/accountSession";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: (...args: unknown[]) => invokeMock(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const origin = "https://relay.example";
const issued: IssueInstallKeyResponse = {
  keyId: "key-1",
  key: "fxk_secret",
  expiresAt: Math.floor(Date.now() / 1000) + 3600,
  installCommand: {
    sh: `curl -fsSL ${origin}/install.sh | sh -s -- --key fxk_secret`,
    ps1: `& ([scriptblock]::Create((irm ${origin}/install.ps1))) -Key fxk_secret`,
  },
};

function machine(overrides: Partial<AccountMachineView> = {}): AccountMachineView {
  return {
    machineRecordId: "rec-old",
    machineId: "mach-old",
    displayName: "Old Box",
    publicKey: "pk",
    attachPublicKey: "apk",
    relayOrigin: origin,
    platform: "linux",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: 1,
    ...overrides,
  };
}

const request: MachineRequestView = {
  requestId: "req-1",
  machineId: "mach-new",
  displayName: "noye",
  platform: "linux",
  appVersion: "2026.1023.1",
  requesterIp: "203.0.113.42",
  createdAt: Math.floor(Date.now() / 1000),
  expiresAt: Math.floor(Date.now() / 1000) + 900,
};

function chooseMethod(name: string) {
  fireEvent.click(screen.getByRole("radio", { name }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (command: string) =>
    command === "cmd_ssh_read_system_config" ? { path: "", exists: false, rawText: "", hosts: [] } : undefined,
  );
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("AddMachineModal - install command method", () => {
  it("defaults to Install command when signed in", () => {
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} onIssueInstallKey={vi.fn()} />);
    expect(screen.getByRole("radio", { name: "Install command" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Enrollment code (manual)" })).toHaveAttribute("aria-checked", "false");
  });

  it("issues one key on open and shows the sh and PowerShell commands with copy buttons and expiry", async () => {
    const issue = vi.fn().mockResolvedValue(issued);
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} accountOrigin={origin} onIssueInstallKey={issue} accountMachines={[]} />);
    chooseMethod("Install command");

    expect(await screen.findByTestId("install-key-card")).toBeInTheDocument();
    expect(issue).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("install-command-sh")).toHaveTextContent(issued.installCommand.sh);
    expect(screen.getByTestId("install-command-ps1")).toHaveTextContent(issued.installCommand.ps1);
    expect(screen.getByRole("button", { name: "Copy Linux and macOS" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy Windows PowerShell" })).toBeInTheDocument();
    expect(screen.getByTestId("install-key-expiry")).toHaveTextContent(/Single use\. The key expires/);
    expect(screen.getByTestId("install-waiting")).toHaveTextContent(/Waiting for the machine/);
  });

  it("copies the sh command to the clipboard", async () => {
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} onIssueInstallKey={vi.fn().mockResolvedValue(issued)} initialMethod="install" />);
    await screen.findByTestId("install-key-card");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Copy Linux and macOS" })); });
    expect(invokeMock).toHaveBeenCalledWith("cmd_clipboard_write_text", { text: issued.installCommand.sh });
    expect(screen.getByRole("button", { name: "Copy Linux and macOS" })).toHaveTextContent("Copied");
  });

  it("regenerates a key with New key", async () => {
    const issue = vi.fn().mockResolvedValue(issued);
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} onIssueInstallKey={issue} initialMethod="install" />);
    await screen.findByTestId("install-key-card");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "New key" })); });
    expect(issue).toHaveBeenCalledTimes(2);
  });

  it("resolves the waiting indicator when a new machine appears in the account list", async () => {
    const issue = vi.fn().mockResolvedValue(issued);
    const props = { isOpen: true, onClose: () => {}, onSuccess: () => {}, onIssueInstallKey: issue, initialMethod: "install" as const };
    const { rerender } = render(<AddMachineModal {...props} accountMachines={[machine()]} />);
    await screen.findByTestId("install-waiting");

    // The already-enrolled machine never counts as the new one.
    rerender(<AddMachineModal {...props} accountMachines={[machine()]} />);
    expect(screen.getByTestId("install-waiting")).toBeInTheDocument();

    rerender(<AddMachineModal {...props} accountMachines={[machine(), machine({ machineRecordId: "rec-new", machineId: "mach-new", displayName: "noye" })]} />);
    expect(screen.getByTestId("install-machine-joined")).toHaveTextContent("noye joined your account.");
    expect(screen.queryByTestId("install-waiting")).toBeNull();
  });

  it("maps INSTALL_KEY_LIMIT and PLAN_LIMIT_REACHED by code", async () => {
    const issue = vi.fn().mockRejectedValue(new AccountSessionError("INSTALL_KEY_LIMIT", "raw", 429));
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} onIssueInstallKey={issue} initialMethod="install" />);
    expect(await screen.findByRole("alert")).toHaveTextContent(/10 unused install keys/);
    expect(installKeyErrorMessage(new AccountSessionError("PLAN_LIMIT_REACHED", "raw", 402))).toMatch(/machine limit/);
  });

  it("signed out: explains that sign-in is needed and issues nothing", () => {
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} initialMethod="install" />);
    expect(screen.getByText(/Sign in to get an install command/)).toBeInTheDocument();
    expect(screen.queryByTestId("install-key-card")).toBeNull();
  });
});

describe("AddMachineModal - approve method", () => {
  function renderApprove(overrides: Record<string, unknown> = {}) {
    const props = {
      onLookupMachineRequest: vi.fn().mockResolvedValue(request),
      onApproveMachineRequest: vi.fn().mockResolvedValue({ machineId: "mach-new" }),
      onDenyMachineRequest: vi.fn().mockResolvedValue(undefined),
      ...overrides,
    };
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} initialMethod="approve" {...props} />);
    return props;
  }

  it("looks up the code and shows the approval card with Deny focused", async () => {
    const props = renderApprove();
    fireEvent.change(screen.getByLabelText("Approval code"), { target: { value: "abcd-efgh" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Find machine" })); });

    expect(props.onLookupMachineRequest).toHaveBeenCalledWith("abcd-efgh");
    expect(await screen.findByTestId("machine-approval-card")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deny" })).toHaveFocus();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve" })); });
    expect(props.onApproveMachineRequest).toHaveBeenCalledWith("req-1", "abcd-efgh");
    expect(props.onDenyMachineRequest).not.toHaveBeenCalled();
  });

  it("denies through the request id and code", async () => {
    const props = renderApprove();
    fireEvent.change(screen.getByLabelText("Approval code"), { target: { value: "ABCD-EFGH" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Find machine" })); });
    await act(async () => { fireEvent.click(await screen.findByRole("button", { name: "Deny" })); });
    expect(props.onDenyMachineRequest).toHaveBeenCalledWith("req-1", "ABCD-EFGH");
  });

  it("maps a lookup refusal to its message by code", async () => {
    renderApprove({
      onLookupMachineRequest: vi.fn().mockRejectedValue(new AccountSessionError("MACHINE_REQUEST_FOREIGN_ACCOUNT", "raw", 403)),
    });
    fireEvent.change(screen.getByLabelText("Approval code"), { target: { value: "ABCD-EFGH" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Find machine" })); });
    expect(await screen.findByRole("alert")).toHaveTextContent(/different Ferryx account/);
    expect(screen.queryByTestId("machine-approval-card")).toBeNull();
  });

  it("signed out: asks to sign in", () => {
    render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} initialMethod="approve" />);
    expect(screen.getByText(/Sign in to approve a machine/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Approval code")).toBeNull();
  });
});

describe("AddMachineModal - SSH install and enroll", () => {
  async function saveSshHost(props: Record<string, unknown>) {
    const view = render(<AddMachineModal isOpen onClose={() => {}} onSuccess={() => {}} initialMethod="ssh" accountOrigin={origin} {...props} />);
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "noye" } });
    fireEvent.change(screen.getByLabelText("Hostname"), { target: { value: "100.107.15.119" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Connect SSH Machine" })); });
    await screen.findByText(/connected and verified/);
    return view;
  }

  function sshIpc(install: (args: Record<string, unknown>) => unknown) {
    invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      switch (command) {
        case "cmd_ssh_read_system_config": return { path: "", exists: false, rawText: "", hosts: [] };
        case "cmd_ssh_test_connection": return { host: args?.host, reachable: true, checkedAt: 1 };
        case "cmd_ssh_update_host": return [args?.host];
        case "cmd_ssh_install_ferryx_node": return install(args ?? {});
        default: return undefined;
      }
    });
  }

  it("is not offered when signed out", async () => {
    sshIpc(() => undefined);
    await saveSshHost({});
    expect(screen.getByText(/connected and verified/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install Ferryx and add to account" })).toBeNull();
  });

  it("issues a key, installs over SSH with the stored host id and waits until the machine is online", async () => {
    const report = {
      cliPath: "/home/q/.local/bin/ferryx-cli",
      version: "2026.1023.1",
      enrolled: { accountId: "acc-1", machineRecordId: "rec-new" },
      service: { supervised: false, backend: "detached", reason: "no systemd user manager" },
    };
    sshIpc(() => report);
    const issue = vi.fn().mockResolvedValue(issued);
    const { rerender } = await saveSshHost({ onIssueInstallKey: issue, accountMachines: [] });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Install Ferryx and add to account" })); });

    await waitFor(() => expect(screen.getByTestId("ssh-install-stage-waiting")).toHaveAttribute("data-state", "active"));
    expect(issue).toHaveBeenCalledTimes(1);
    const installCall = invokeMock.mock.calls.find(([command]) => command === "cmd_ssh_install_ferryx_node");
    expect(installCall?.[1]).toMatchObject({ accountOrigin: origin, installKey: "fxk_secret" });
    const savedHost = invokeMock.mock.calls.find(([command]) => command === "cmd_ssh_update_host")?.[1] as { host: { id: string } };
    expect(installCall?.[1]).toMatchObject({ hostId: savedHost.host.id });
    expect(screen.getByTestId("ssh-install-stage-installing")).toHaveAttribute("data-state", "done");

    rerender(
      <AddMachineModal
        isOpen
        onClose={() => {}}
        onSuccess={() => {}}
        initialMethod="ssh"
        accountOrigin={origin}
        onIssueInstallKey={issue}
        accountMachines={[machine({ machineRecordId: "rec-new", machineId: "mach-new", online: true })]}
      />,
    );
    expect(screen.getByTestId("ssh-install-stage-waiting")).toHaveAttribute("data-state", "done");
    expect(screen.getByRole("status")).toHaveTextContent(/joined your account\. Not supervised: no systemd user manager\./);
  });

  it("times out after 60 s when the machine never comes online", async () => {
    sshIpc(() => ({ cliPath: "x", version: "v", enrolled: { accountId: "a", machineRecordId: "rec-new" }, service: { supervised: true, backend: "systemd" } }));
    await saveSshHost({ onIssueInstallKey: vi.fn().mockResolvedValue(issued), accountMachines: [] });
    vi.useFakeTimers();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Install Ferryx and add to account" })); });
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByTestId("ssh-install-stage-waiting")).toHaveAttribute("data-state", "active");
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(screen.getByTestId("ssh-install-stage-waiting")).toHaveAttribute("data-state", "failed");
    expect(screen.getByRole("alert")).toHaveTextContent(/did not come online within 60 seconds/);
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("shows the structured SSH_NODE_INSTALL_FAILED stage, exit code and stderr tail", async () => {
    sshIpc(() => {
      throw { code: "SSH_NODE_INSTALL_FAILED", message: "install failed", details: { stage: "download", exitCode: 22, stderrTail: "curl: (22) 404" } };
    });
    await saveSshHost({ onIssueInstallKey: vi.fn().mockResolvedValue(issued), accountMachines: [] });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Install Ferryx and add to account" })); });

    expect(await screen.findByRole("alert")).toHaveTextContent("Installing Ferryx failed during download (exit 22).");
    expect(screen.getByTestId("ssh-install-stderr")).toHaveTextContent("curl: (22) 404");
    expect(screen.getByTestId("ssh-install-stage-issuing")).toHaveAttribute("data-state", "done");
    expect(screen.getByTestId("ssh-install-stage-installing")).toHaveAttribute("data-state", "failed");
  });

  it("reports a key-issuing refusal at the first stage without invoking the installer", async () => {
    sshIpc(() => undefined);
    await saveSshHost({ onIssueInstallKey: vi.fn().mockRejectedValue(new AccountSessionError("PLAN_LIMIT_REACHED", "raw", 402)), accountMachines: [] });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Install Ferryx and add to account" })); });

    expect(await screen.findByRole("alert")).toHaveTextContent(/machine limit is reached/);
    expect(screen.getByTestId("ssh-install-stage-issuing")).toHaveAttribute("data-state", "failed");
    expect(invokeMock.mock.calls.some(([command]) => command === "cmd_ssh_install_ferryx_node")).toBe(false);
  });
});
