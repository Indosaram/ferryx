import { beforeEach, describe, expect, it, vi } from "vitest";

import { installFerryxNode, type SshNodeInstallReport } from "./sshHosts";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: () => true,
}));

describe("installFerryxNode", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("invokes cmd_ssh_install_ferryx_node with camelCase args and returns the report", async () => {
    const report: SshNodeInstallReport = {
      cliPath: "/home/u/.local/bin/ferryx-cli",
      version: "2026.1023.1",
      enrolled: { accountId: "acc_1", machineRecordId: "m_1" },
      service: { supervised: true, backend: "systemd-user" },
    };
    invokeMock.mockResolvedValueOnce(report);

    await expect(installFerryxNode("host-1", "https://ferryx.dev", "fxk_abc")).resolves.toEqual(report);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_install_ferryx_node", {
      hostId: "host-1",
      accountOrigin: "https://ferryx.dev",
      installKey: "fxk_abc",
    });
  });

  it("propagates the structured backend error unchanged", async () => {
    const error = {
      code: "SSH_NODE_INSTALL_FAILED",
      message: "Installing Ferryx over SSH failed during install",
      details: { stage: "install", exitCode: 3, stderrTail: "denied" },
    };
    invokeMock.mockRejectedValueOnce(error);

    await expect(installFerryxNode("host-1", "https://ferryx.dev", "fxk_abc")).rejects.toBe(error);
  });
});
