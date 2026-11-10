import { afterEach, expect, it } from "vitest";
import { loadProjects, recoverProjectBootstrap, secondaryWindowBootstrap } from "./App";
import { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY } from "./lib/storageKeys";


const paired = { workspaceId: `daemon:${"a".repeat(64)}`, repoRoot: "/remote/app", gitRoot: null,
  target: { kind: "pairedDaemon", hostId: "relay/machine-a" } as const, remoteWorkspaceId: "project-a" };
afterEach(() => localStorage.clear());

it("retains paired identity when loading the stored project catalog", () => {
  localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([paired]));
  expect(loadProjects()).toEqual([expect.objectContaining(paired)]);
});

it("opens a new window on the primary window's active project instead of the picker", () => {
  const projects = [{ workspaceId: "alpha" }, { workspaceId: "beta" }] as never;
  localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "beta");
  // A fresh secondary window has no namespaced selection yet (no ?window= label here).
  expect(secondaryWindowBootstrap({ projects, activeProjectId: "alpha" }, null).activeProjectId).toBe("beta");
  // Its own selection still wins once it has one.
  expect(secondaryWindowBootstrap({ projects, activeProjectId: "beta" }, { projects, activeProjectId: "alpha" }).activeProjectId).toBe("alpha");
});

it("retains paired project metadata when reconstructing bootstrap references", () => {
  const workspace = { ...paired, worktrees: [], activeWorktreePath: null,
    layout: { splitMode: "none", primaryTabId: null, secondaryTabId: null, activeTabId: null, tabs: [] }, terminalSessions: {} };
  const result = recoverProjectBootstrap({ version: 3, timestamp: 0, activeWorkspaceId: paired.workspaceId, workspaces: { [paired.workspaceId]: workspace } });
  expect(result).toEqual({ activeProjectId: paired.workspaceId, projects: [expect.objectContaining(paired)] });
});
