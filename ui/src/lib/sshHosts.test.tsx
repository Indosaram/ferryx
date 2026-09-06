import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  deleteSshHost,
  formatSshKey,
  formatSshTarget,
  getCachedSshHosts,
  importSshConfig,
  listSshHosts,
  resetSshHostsCache,
  testSshConnection,
  updateSshHost,
  useSshHosts,
  type SshHost,
} from "./sshHosts";

const invokeMock = vi.fn();
const isTauriMock = vi.fn(() => true);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: () => isTauriMock(),
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
  label: "Dev Box",
  hostname: "dev.internal",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

const mockHost2: SshHost = {
  id: "host-2",
  label: "Production",
  hostname: "prod.example.com",
  username: "deploy",
  port: 2222,
  source: "config",
  authMethod: "key",
  identityFile: "~/.ssh/id_rsa",
  disabled: true,
};

describe("sshHosts library and useSshHosts hook", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    isTauriMock.mockReturnValue(true);
  });

  afterEach(cleanup);

  describe("Formatting utilities", () => {
    it("formats target with and without username", () => {
      expect(formatSshTarget({ username: "root", hostname: "1.2.3.4" })).toBe("root@1.2.3.4");
      expect(formatSshTarget({ username: "", hostname: "1.2.3.4" })).toBe("1.2.3.4");
      expect(formatSshTarget({ username: null, hostname: "1.2.3.4" })).toBe("1.2.3.4");
    });

    it("formats key with default port and explicit port", () => {
      expect(formatSshKey({ username: "ubuntu", hostname: "host.net", port: 22 })).toBe(
        "ubuntu@host.net:22",
      );
      expect(formatSshKey({ username: null, hostname: "host.net", port: 8022 })).toBe(
        "host.net:8022",
      );
      expect(formatSshKey({ username: "admin", hostname: "host.net" })).toBe(
        "admin@host.net:22",
      );
    });
  });

  describe("IPC command wrappers", () => {
    it("listSshHosts invokes cmd_ssh_list_hosts and returns hosts", async () => {
      invokeMock.mockResolvedValueOnce([mockHost1]);

      const result = await listSshHosts();

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_list_hosts");
      expect(result).toEqual([mockHost1]);
    });

    it("deduplicates inflight listSshHosts calls", async () => {
      const def = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(def.promise);

      const p1 = listSshHosts();
      const p2 = listSshHosts();

      expect(invokeMock).toHaveBeenCalledTimes(1);

      def.resolve([mockHost1]);
      const [r1, r2] = await Promise.all([p1, p2]);
      expect(r1).toEqual([mockHost1]);
      expect(r2).toEqual([mockHost1]);
    });

    it("prevents in-flight list from overwriting newer mutation inventory (stale read discard)", async () => {
      // 1. Initial cached state is host-1
      invokeMock.mockResolvedValueOnce([mockHost1]);
      await listSshHosts();
      expect(getCachedSshHosts()).toEqual([mockHost1]);

      // 2. An in-flight list request begins (e.g. background refresh) with deferred promise
      const slowListDef = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(slowListDef.promise);
      const inflightListPromise = listSshHosts();

      // 3. While that list request is in-flight, a mutation occurs and resolves first with host-1 & host-2
      invokeMock.mockResolvedValueOnce([mockHost1, mockHost2]);
      await updateSshHost(mockHost2);
      expect(getCachedSshHosts()).toEqual([mockHost1, mockHost2]);

      // 4. Now the slow older list resolves with obsolete data ([mockHost1])
      slowListDef.resolve([mockHost1]);
      await inflightListPromise;

      // 5. The cache MUST NOT have reverted back to obsolete [mockHost1]!
      expect(getCachedSshHosts()).toEqual([mockHost1, mockHost2]);
    });

    it("write operations reject outside the Tauri runtime instead of silently reporting success", async () => {
      isTauriMock.mockReturnValue(false);

      await expect(updateSshHost(mockHost1)).rejects.toThrow(
        "SSH host changes are available only in the Ferryx desktop runtime",
      );
      await expect(deleteSshHost("host-1")).rejects.toThrow(
        "SSH host changes are available only in the Ferryx desktop runtime",
      );
      await expect(importSshConfig("Host x")).rejects.toThrow(
        "SSH host changes are available only in the Ferryx desktop runtime",
      );
      expect(invokeMock).not.toHaveBeenCalled();
    });

    it("rejects invalid ports instead of flooring or redirecting to the backend default", async () => {
      await expect(updateSshHost({ ...mockHost1, port: 22.5 })).rejects.toThrow(
        "Port must be an integer between 1 and 65535.",
      );
      await expect(updateSshHost({ ...mockHost1, port: 0 })).rejects.toThrow(
        "Port must be an integer between 1 and 65535.",
      );
      await expect(updateSshHost({ ...mockHost1, port: 99999 })).rejects.toThrow(
        "Port must be an integer between 1 and 65535.",
      );
      expect(invokeMock).not.toHaveBeenCalled();
    });

    it("omits an unset port from the IPC payload so the backend default applies", async () => {
      invokeMock.mockResolvedValueOnce([mockHost1]);

      await updateSshHost({ ...mockHost1, port: null });

      const payload = invokeMock.mock.calls[0][1] as { host: Record<string, unknown> };
      expect("port" in payload.host).toBe(false);
    });

    it("updateSshHost invokes cmd_ssh_update_host with cleaned payload and returns updated inventory", async () => {
      invokeMock.mockResolvedValueOnce([mockHost1, mockHost2]);

      const hostToUpdate: SshHost = {
        id: "host-2",
        label: "  Production  ",
        hostname: "  prod.example.com ",
        username: "  deploy ",
        port: 2222,
        identityFile: " ~/.ssh/id_rsa ",
        source: "config",
        authMethod: "key",
        disabled: true,
      };

      const result = await updateSshHost(hostToUpdate);

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_update_host", {
        host: {
          id: "host-2",
          label: "Production",
          hostname: "prod.example.com",
          username: "deploy",
          port: 2222,
          identityFile: "~/.ssh/id_rsa",
          source: "config",
          authMethod: "key",
          disabled: true,
        },
      });
      expect(result).toEqual([mockHost1, mockHost2]);
    });

    it("deleteSshHost invokes cmd_ssh_delete_host and returns updated inventory", async () => {
      invokeMock.mockResolvedValueOnce([mockHost1]);

      const result = await deleteSshHost("host-2");

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_delete_host", { id: "host-2" });
      expect(result).toEqual([mockHost1]);
    });

    it("importSshConfig invokes cmd_ssh_import_config with configText", async () => {
      const config = "Host alias\n  HostName 10.0.0.1";
      invokeMock.mockResolvedValueOnce([mockHost1]);

      const result = await importSshConfig(config);

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_import_config", { configText: config });
      expect(result).toEqual([mockHost1]);
    });

    it("testSshConnection invokes cmd_ssh_test_connection and returns summary", async () => {
      const summary = {
        host: mockHost1,
        reachable: true,
        lastError: null,
        checkedAt: 1234567890,
      };
      invokeMock.mockResolvedValueOnce(summary);

      const result = await testSshConnection(mockHost1);

      expect(invokeMock).toHaveBeenCalledWith("cmd_ssh_test_connection", {
        host: {
          id: "host-1",
          label: "Dev Box",
          hostname: "dev.internal",
          username: "ubuntu",
          port: 22,
          source: "manual",
          authMethod: "agent",
          disabled: false,
        },
      });
      expect(result).toEqual(summary);
    });
  });

  describe("useSshHosts React Hook", () => {
    function HostConsumer({ id }: { id: string }) {
      const { hosts, loading, error } = useSshHosts();
      return (
        <div data-testid={`consumer-${id}`}>
          <div data-testid={`loading-${id}`}>{loading ? "loading" : "idle"}</div>
          {error && <div data-testid={`error-${id}`}>{error}</div>}
          <ul data-testid={`list-${id}`}>
            {hosts.map((h) => (
              <li key={h.id} data-testid={`host-item-${id}-${h.id}`}>
                {h.label} - {h.disabled ? "disabled" : "enabled"}
              </li>
            ))}
          </ul>
        </div>
      );
    }

    it("loads hosts on initial mount and displays inventory", async () => {
      const def = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(def.promise);

      render(<HostConsumer id="1" />);

      expect(screen.getByTestId("loading-1")).toHaveTextContent("loading");

      await act(async () => {
        def.resolve([mockHost1]);
      });

      expect(screen.getByTestId("loading-1")).toHaveTextContent("idle");
      expect(screen.getByTestId("host-item-1-host-1")).toHaveTextContent("Dev Box - enabled");
    });

    it("surfaces load error when listSshHosts fails", async () => {
      const def = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(def.promise);

      render(<HostConsumer id="1" />);

      expect(screen.getByTestId("loading-1")).toHaveTextContent("loading");

      await act(async () => {
        def.reject(new Error("Network IPC failure"));
      });

      expect(screen.getByTestId("error-1")).toHaveTextContent("Network IPC failure");
      expect(screen.getByTestId("loading-1")).toHaveTextContent("idle");
    });

    it("surfaces the message from structured IPC error objects instead of [object Object]", async () => {
      const def = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(def.promise);

      render(<HostConsumer id="1" />);

      expect(screen.getByTestId("loading-1")).toHaveTextContent("loading");

      await act(async () => {
        def.reject({
          code: "IO_ERROR",
          message: "failed to write ssh store",
          details: {},
        });
      });

      expect(screen.getByTestId("error-1")).toHaveTextContent("failed to write ssh store");
      expect(screen.getByTestId("error-1")).not.toHaveTextContent("[object Object]");
    });

    it("synchronously updates all open consumers when a mutation occurs", async () => {
      const def = deferred<SshHost[]>();
      invokeMock.mockReturnValueOnce(def.promise);

      render(
        <div>
          <HostConsumer id="A" />
          <HostConsumer id="B" />
        </div>,
      );

      expect(screen.getByTestId("loading-A")).toHaveTextContent("loading");
      expect(screen.getByTestId("loading-B")).toHaveTextContent("loading");

      await act(async () => {
        def.resolve([mockHost1]);
      });

      expect(screen.getByTestId("loading-A")).toHaveTextContent("idle");
      expect(screen.getByTestId("loading-B")).toHaveTextContent("idle");
      expect(screen.getByTestId("host-item-A-host-1")).toHaveTextContent("Dev Box - enabled");
      expect(screen.getByTestId("host-item-B-host-1")).toHaveTextContent("Dev Box - enabled");

      // Now perform an update mutation that adds host-2
      invokeMock.mockResolvedValueOnce([mockHost1, mockHost2]);

      await act(async () => {
        await updateSshHost(mockHost2);
      });

      // Both consumers are updated immediately with host-2
      expect(screen.getByTestId("host-item-A-host-2")).toHaveTextContent("Production - disabled");
      expect(screen.getByTestId("host-item-B-host-2")).toHaveTextContent("Production - disabled");

      // Now perform a delete mutation
      invokeMock.mockResolvedValueOnce([mockHost2]);

      await act(async () => {
        await deleteSshHost("host-1");
      });

      // Both consumers reflect deletion of host-1
      expect(screen.queryByTestId("host-item-A-host-1")).toBeNull();
      expect(screen.queryByTestId("host-item-B-host-1")).toBeNull();
      expect(screen.getByTestId("host-item-A-host-2")).toBeInTheDocument();
      expect(screen.getByTestId("host-item-B-host-2")).toBeInTheDocument();
    });
  });
});
