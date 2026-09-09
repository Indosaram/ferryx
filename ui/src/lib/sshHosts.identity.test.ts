import { beforeEach, describe, expect, it, vi } from "vitest";

import { getCachedSshHosts, listSshHosts, resetSshHostsCache, type SshHost } from "./sshHosts";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mocks.invoke(...args),
  isTauri: () => mocks.isTauri(),
}));

function host(label: string, hostname: string): SshHost {
  return {
    id: "ssh-build",
    label,
    hostname,
    source: "config",
    authMethod: "agent",
    disabled: false,
  };
}

describe("SSH host routing identity", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    mocks.isTauri.mockReturnValue(true);
  });

  it("keeps the latest endpoint when a legacy backend returns duplicate host ids", async () => {
    const windows = host("Windows", "maho-win");
    const linux = host("Linux", "omarchy");
    mocks.invoke.mockResolvedValueOnce([windows, linux]);

    const result = await listSshHosts();

    expect(mocks.invoke).toHaveBeenCalledWith("cmd_ssh_list_hosts");
    expect(result).toEqual([linux]);
    expect(getCachedSshHosts()).toEqual([linux]);
  });
});
