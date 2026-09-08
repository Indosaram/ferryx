import type { RegisteredProject } from "./types";

export interface ProjectGroup {
  groupId: string;
  primaryProject: RegisteredProject;
  memberProjects: RegisteredProject[];
}

export function normalizeGitRemote(url: string | null | undefined): string | null {
  const value = url?.trim();
  if (!value || /^[a-z]:[\\/]/i.test(value)) return null;
  const scp = value.includes("://") ? null : /^(?:[^@\s/:]+@)?([^:\s/]+):(.+)$/.exec(value);
  let host: string;
  let path: string;
  if (scp) {
    host = scp[1].toLowerCase();
    path = scp[2];
  } else {
    try {
      const parsed = new URL(value);
      if (!["https:", "http:", "ssh:", "git:"].includes(parsed.protocol)) return null;
      host = parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.host : parsed.hostname;
      path = parsed.pathname;
    } catch (error) {
      if (error instanceof TypeError) return null;
      throw error;
    }
  }
  if (host === "ssh.github.com") host = "github.com";
  if (host === "github.com") path = path.toLowerCase();
  path = path.replace(/^\/+|\/+$/g, "").replace(/\.git$/, "");
  return host && path ? `${host}/${path}` : null;
}

export function getProjectFolderName(project: RegisteredProject): string {
  const raw = project.repoRoot.replace(/[\\/]+$/, "");
  const parts = raw.split(/[\\/]/).filter(Boolean);
  const folder = parts.at(-1) || project.workspaceId;
  return folder.toLowerCase();
}

export function matchesSameProject(a: RegisteredProject, b: RegisteredProject): boolean {
  const hostA = a.target?.kind === "ssh" ? `ssh:${a.target.hostId}` : "local";
  const hostB = b.target?.kind === "ssh" ? `ssh:${b.target.hostId}` : "local";
  if (hostA === hostB) {
    if (a.workspaceId === b.workspaceId) return true;
    if (a.gitCommonDir && b.gitCommonDir &&
        normalizeGitDirectory(a.gitCommonDir) === normalizeGitDirectory(b.gitCommonDir)) return true;
  }

  const remoteA = normalizeGitRemote(a.gitRemote);
  const remoteB = normalizeGitRemote(b.gitRemote);
  return remoteA !== null && remoteA === remoteB;
}

function normalizeGitDirectory(path: string): string {
  if (/^[a-z]:[\\/]/i.test(path) || path.startsWith("\\\\") || path.startsWith("//?/")) {
    return path.replace(/\\/g, "/").replace(/^\/\/\?\//, "")
      .replace(/^UNC\//i, "//").replace(/\/+$/, "").toLowerCase();
  }
  return path.replace(/\/+$/, "");
}

export function groupProjects(projects: RegisteredProject[]): ProjectGroup[] {
  const groups: ProjectGroup[] = [];

  for (const project of projects) {
    const matching = groups.filter((group) =>
      group.memberProjects.some((member) => matchesSameProject(member, project)),
    );
    const existing = matching[0];
    if (existing) {
      // A newly resolved checkout can connect a host-local worktree group to a remote group.
      for (const group of matching.slice(1)) {
        existing.memberProjects.push(...group.memberProjects);
        groups.splice(groups.indexOf(group), 1);
      }
      existing.memberProjects.push(project);
      existing.primaryProject = existing.memberProjects.find((member) => member.target?.kind !== "ssh")
        ?? existing.primaryProject;
      existing.groupId = existing.primaryProject.workspaceId;
    } else {
      groups.push({
        groupId: project.workspaceId,
        primaryProject: project,
        memberProjects: [project],
      });
    }
  }

  return groups;
}

export function isProjectGroupActive(group: ProjectGroup, activeProjectId?: string | null): boolean {
  if (!activeProjectId) return false;
  return group.memberProjects.some((member) => member.workspaceId === activeProjectId);
}

export function findGroupForWorkspace(groups: ProjectGroup[], workspaceId?: string | null): ProjectGroup | undefined {
  if (!workspaceId) return undefined;
  return groups.find((g) => g.memberProjects.some((m) => m.workspaceId === workspaceId));
}
