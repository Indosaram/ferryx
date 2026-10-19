import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { Sidebar, SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY } from "./Sidebar";
import { remoteHostStore } from "../state/remoteHostStore";
import { projectRootWorktree } from "../lib/projectIdentity";
import { groupProjects } from "../lib/projectGrouping";
import type { RegisteredProject } from "../lib/types";
import type { PairedHostReachabilityView } from "../lib/pairedHostReachability";
const reach = vi.hoisted(() => ({ view: { supported: false, entry: null } as PairedHostReachabilityView }));
vi.mock("../lib/pairedHostReachability", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/pairedHostReachability")>()),
  getPairedHostReachability: () => reach.view,
  subscribePairedHostReachability: () => () => undefined,
  usePairedHostReachability: () => reach.view,
}));
vi.mock("../lib/sshHosts", () => ({ useSshHosts: () => ({ hosts: [{ id: "ssh", label: "SSH machine" }] }), subscribeSshHosts: () => () => undefined }));
afterEach(() => { cleanup(); localStorage.clear(); remoteHostStore.reset(); reach.view = { supported: false, entry: null }; });
it("opens the registered SSH folder rather than its parent Git worktree", () => {
  const local: RegisteredProject = { workspaceId: "project", repoRoot: "/Users/indo/code/project", gitRoot: null };
  const remote: RegisteredProject = {
    workspaceId: "ssh:roblox", repoRoot: "C:\\Users\\sook\\code\\roblox-game-forge",
    gitRoot: "C:/Users/sook", gitBranch: "master", target: { kind: "ssh", hostId: "ssh" },
  };
  const select = vi.fn();
  localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
  render(<Sidebar
    projects={[local, remote]} activeProjectId={local.workspaceId}
    worktrees={[projectRootWorktree(local)]} agents={[]} activePath={local.repoRoot}
    inactiveProjectWorktrees={{ [remote.workspaceId]: [{
      path: "C:/Users/sook", head: "", branch: "master", bare: false,
      detached: false, locked: null, prunable: null,
    }] }}
    onSelectWorktree={select}
    onCreateWorktree={() => undefined}
  />);
  fireEvent.click(screen.getByRole("button", { name: "roblox-game-forge SSH machine" }));
  expect(select).toHaveBeenCalledWith(expect.objectContaining({
    workspaceId: remote.workspaceId, path: remote.repoRoot, branch: null,
  }));
});
it("keeps identical local, SSH and paired paths separate and selects explicit paired roots with machine status", () => {
  const base = { repoRoot: "/srv/repo", gitRoot: "/srv/repo", gitCommonDir: "/srv/repo/.git", gitBranch: "main" };
  const local: RegisteredProject = { ...base, workspaceId: "repo" };
  const ssh: RegisteredProject = { ...base, workspaceId: "ssh:repo", target: { kind: "ssh", hostId: "ssh" } };
  const paired = ["a", "b"].map((id): RegisteredProject => ({ ...base, workspaceId: `daemon:${id.repeat(64)}`, remoteWorkspaceId: "repo", target: { kind: "pairedDaemon", hostId: id } }));
  remoteHostStore.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true, hosts: Object.fromEntries(["a", "b"].map(id => [id, { hostId: id, name: id === "a" ? "Alpha" : "Beta", address: "", transport: "relay", online: id === "a", authStatus: "paired", grantScope: "machine" }])) }));
  expect(groupProjects([local, ssh, ...paired])).toHaveLength(4);
  expect(projectRootWorktree(paired[0]).workspaceId).toBe(paired[0].workspaceId);
  localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
  const select = vi.fn();
  render(<Sidebar projects={[local, ssh, ...paired]} activeProjectId={paired[0].workspaceId} activePath="/srv/repo" worktrees={[]} agents={[]} onSelectWorktree={select} onCreateWorktree={vi.fn()} />);
  expect(screen.getByRole("button", { name: "repo (Alpha)" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "repo (Beta)" })).toBeInTheDocument();
  expect(screen.getAllByText("Offline").length).toBeGreaterThan(0);
  fireEvent.click(screen.getByRole("button", { name: /main.*Beta/ }));
  expect(select).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: paired[1].workspaceId, path: "/srv/repo" }));
});
function renderPairedProject(online: boolean, onOpenSettings = vi.fn()) {
  const project: RegisteredProject = { workspaceId: `daemon:${"c".repeat(64)}`, repoRoot: "/srv/app", gitRoot: "/srv/app", remoteWorkspaceId: "app", target: { kind: "pairedDaemon", hostId: "h" } };
  remoteHostStore.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true, hosts: { h: { hostId: "h", name: "Gamma", address: "", transport: "relay", online, authStatus: "paired", grantScope: "machine", generation: "7" } } }));
  render(<Sidebar projects={[project]} activeProjectId={project.workspaceId} activePath="" worktrees={[]} agents={[]} onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} onOpenSettings={onOpenSettings} />);
  return onOpenSettings;
}
it("makes a not-connected paired project slot a button that deep-links to that machine in Settings > Remote", () => {
  const onOpenSettings = renderPairedProject(false);
  const slot = screen.getByRole("button", { name: "Offline: open remote settings for Gamma" });
  fireEvent.click(slot);
  expect(onOpenSettings).toHaveBeenCalledExactlyOnceWith("remote", {
    page: "machines", filter: "all", machine: { kind: "pairedDaemon", hostId: "h", generation: "7" },
  });
});
it("renders a connected paired project slot as plain text, not a button", () => {
  const onOpenSettings = renderPairedProject(true);
  const header = screen.getByRole("button", { name: "app (Gamma)" });
  expect(header).toHaveTextContent("Connected");
  expect(screen.queryByRole("button", { name: /open remote settings/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("Connected"));
  expect(onOpenSettings).not.toHaveBeenCalledWith("remote", expect.anything());
});
it("labels paired worktree rows with the header's status when reachability says offline but host.online is stale", () => {
  reach.view = { supported: true, entry: { hostId: "h", generation: "7", state: "offline", checkedAtMs: 1, lastError: null } };
  localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
  renderPairedProject(true);
  expect(screen.getByRole("button", { name: "Offline: open remote settings for Gamma" })).toHaveTextContent("Offline");
  expect(screen.getByText("/srv/app - Offline")).toBeInTheDocument();
  expect(screen.queryByText(/Online \(/)).not.toBeInTheDocument();
});
