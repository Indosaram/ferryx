import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => ({
  loadSshHosts: vi.fn(),
  subscribeSshHosts: vi.fn(),
}));

const tauri = vi.hoisted(() => ({
  listRemoteWorktrees: vi.fn(),
}));

vi.mock("../lib/sshHosts", () => store);
vi.mock("../lib/tauri", () => tauri);

import { RemoteHostsSection } from "./RemoteHostsSection";
import type { SshHost } from "../lib/types";

function host(overrides: Partial<SshHost> = {}): SshHost {
  return {
    id: "h1",
    label: "win box",
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

function setupStore(hosts: SshHost[]) {
  store.loadSshHosts.mockResolvedValue(hosts);
  store.subscribeSshHosts.mockImplementation((_listener: unknown) => () => undefined);
}

afterEach(cleanup);
beforeEach(() => {
  for (const fn of Object.values(store)) fn.mockReset?.();
  tauri.listRemoteWorktrees.mockReset();
});

describe("RemoteHostsSection", () => {
  it("renders host rows with user@host titles and an add-host entry point", async () => {
    setupStore([host()]);
    render(<RemoteHostsSection onOpenRemoteWorktree={() => undefined} />);

    expect(await screen.findByTestId("remote-host-row-h1")).toBeDefined();
    expect(screen.getByTestId("remote-host-title-h1").textContent).toBe("sook@maho-win");
    expect(screen.getByTestId("sidebar-add-ssh-host")).toBeDefined();
  });

  it("expands a host into remote worktree rows and opens one on click", async () => {
    setupStore([host()]);
    tauri.listRemoteWorktrees.mockResolvedValue([
      { path: "/home/sook/proj/.orca-worktrees/wt-main", head: "abc", branch: "orca/ws/main", bare: false, detached: false },
    ]);
    const onOpen = vi.fn();
    render(<RemoteHostsSection onOpenRemoteWorktree={onOpen} />);

    fireEvent.click(await screen.findByTestId("remote-host-row-h1"));
    const row = await screen.findByTestId("remote-worktree-/home/sook/proj/.orca-worktrees/wt-main");
    fireEvent.click(row);

    await waitFor(() =>
      expect(onOpen).toHaveBeenCalledWith(host(), {
        path: "/home/sook/proj/.orca-worktrees/wt-main",
        head: "abc",
        branch: "orca/ws/main",
      }),
    );
  });

  it("surfaces host-unreachable errors when the remote listing fails", async () => {
    setupStore([host()]);
    tauri.listRemoteWorktrees.mockRejectedValue(new Error("ssh: connect to host maho-win port 22: Connection refused"));
    render(<RemoteHostsSection onOpenRemoteWorktree={() => undefined} />);

    fireEvent.click(await screen.findByTestId("remote-host-row-h1"));
    const error = await screen.findByTestId("remote-host-error-h1");
    expect(error.textContent).toContain("Connection refused");
  });

  it("renders nothing when no hosts are configured", async () => {
    setupStore([]);
    const { container } = render(<RemoteHostsSection onOpenRemoteWorktree={() => undefined} />);
    await screen.findByTestId("sidebar-add-ssh-host");
    expect(container.querySelector("[data-testid^='remote-host-row-']")).toBeNull();
  });
});
