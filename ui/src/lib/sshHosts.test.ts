import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { SshHost } from "./types";
import {
  __resetSshHostsCacheForTest,
  importSshHostsFromConfig,
  loadSshHosts,
  removeSshHost,
  sshHostKey,
  sshPaneTitle,
  subscribeSshHosts,
  upsertSshHost,
} from "./sshHosts";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function host(overrides: Partial<SshHost> = {}): SshHost {
  return {
    id: "h1",
    label: "win",
    hostname: "maho-win",
    username: "sook",
    port: null,
    identityFile: null,
    jumpHost: null,
    source: "config",
    authMethod: "agent",
    disabled: null,
    repoRoot: null,
    ...overrides,
  };
}

function invokeHandler(handlers: Record<string, (args: any) => unknown>) {
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    const handler = handlers[command];
    if (!handler) throw new Error(`unexpected command in test: ${command}`);
    return handler(args ?? {});
  });
}

describe("sshHosts store", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    __resetSshHostsCacheForTest();
  });

  it("loads hosts through the daemon IPC and caches a single inflight request", async () => {
    let listCalls = 0;
    invokeHandler({
      cmd_ssh_list_hosts: () => {
        listCalls += 1;
        return [host()];
      },
    });

    const [first, second] = await Promise.all([loadSshHosts(), loadSshHosts()]);
    expect(first).toEqual([host()]);
    expect(second).toEqual(first);
    expect(listCalls).toBe(1);

    const cached = await loadSshHosts();
    expect(cached).toEqual(first);
    expect(listCalls).toBe(1);

    const forced = await loadSshHosts(true);
    expect(forced).toEqual([host()]);
    expect(listCalls).toBe(2);
  });

  it("notifies subscribers on subscribe and after import propagates the deduped list", async () => {
    invokeHandler({
      cmd_ssh_list_hosts: () => [host({ id: "h1" })],
      cmd_ssh_import_config: () => [
        host({ id: "h1" }),
        host({ id: "h2", hostname: "nas", username: null, label: "nas" }),
      ],
    });

    const seen: SshHost[][] = [];
    const unsubscribe = subscribeSshHosts((hosts) => seen.push(hosts));
    await loadSshHosts();
    expect(seen).toEqual([[host({ id: "h1" })]]);

    const imported = await importSshHostsFromConfig("Host nas\n  HostName nas");
    expect(imported).toHaveLength(2);
    expect(seen).toHaveLength(2);
    expect(seen[1].map((h) => h.id)).toEqual(["h1", "h2"]);

    const cached = await loadSshHosts();
    expect(cached.map((h) => h.id)).toEqual(["h1", "h2"]);
    unsubscribe();
  });

  it("propagates manual upsert and delete through the store", async () => {
    invokeHandler({
      cmd_ssh_list_hosts: () => [],
      cmd_ssh_update_host: ({ host: updated }) => [updated],
      cmd_ssh_delete_host: ({ id }) => (id === "h1" ? [] : [host()]),
    });

    await loadSshHosts();
    const updates: number[] = [];
    const unsubscribe = subscribeSshHosts((hosts) => updates.push(hosts.length));

    const manual = host({ id: "h1", source: "manual" });
    await upsertSshHost(manual);
    expect(updates.at(-1)).toBe(1);

    await removeSshHost("h1");
    expect(updates.at(-1)).toBe(0);
    unsubscribe();
  });

  it("mirrors the Rust SshHost::key tombstone format", () => {
    expect(sshHostKey("maho-win", "sook", null)).toBe("sook@maho-win:22");
    expect(sshHostKey("nas", null, 2200)).toBe("nas:2200");
    expect(sshHostKey("nas", null, null)).toBe("nas:22");
  });

  it("maps ssh session metadata to pane titles", () => {
    expect(sshPaneTitle({ title: "sook@maho-win", remotePath: "/home/sook/proj/.orca-worktrees/wt-main" })).toBe(
      "sook@maho-win:wt-main",
    );
    expect(sshPaneTitle({ title: "nas", remotePath: "/srv/nas/repo" })).toBe("nas:repo");
    expect(sshPaneTitle({ title: "nas", remotePath: null })).toBe("nas");
    expect(sshPaneTitle({ title: "box", remotePath: "/" })).toBe("box:/");
  });
});
