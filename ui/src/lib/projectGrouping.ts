import type { RegisteredProject } from "./types";

export interface ProjectGroup {
  groupId: string;
  primaryProject: RegisteredProject;
  memberProjects: RegisteredProject[];
}

export function normalizeGitRemote(url: string | null | undefined): string | null {
  if (!url || typeof url !== "string") return null;
  let clean = url.trim().toLowerCase();
  if (!clean) return null;
  clean = clean.replace(/^(?:ssh|git|https?):\/\//, "");
  clean = clean.replace(/^[a-zA-Z0-9._-]+@/, "");
  clean = clean.replace(/^([^/:]+):/, "$1/");
  clean = clean.replace(/^([^/]+):\d+\//, "$1/");
  clean = clean.replace(/\.git\/?$/, "");
  clean = clean.replace(/\/+$/, "");
  return clean || null;
}

export function getProjectFolderName(project: RegisteredProject): string {
  const raw = project.repoRoot.replace(/[\\/]+$/, "");
  const parts = raw.split(/[\\/]/).filter(Boolean);
  const folder = parts.at(-1) || project.workspaceId;
  return folder.toLowerCase();
}

export function matchesSameProject(a: RegisteredProject, b: RegisteredProject): boolean {
  if (a.workspaceId === b.workspaceId) return true;

  const remoteA = normalizeGitRemote(a.gitRemote);
  const remoteB = normalizeGitRemote(b.gitRemote);
  if (remoteA && remoteB && remoteA === remoteB) {
    return true;
  }

  const folderA = getProjectFolderName(a);
  const folderB = getProjectFolderName(b);
  if (folderA && folderB && folderA === folderB) {
    return true;
  }

  return false;
}

export function groupProjects(projects: RegisteredProject[]): ProjectGroup[] {
  const groups: ProjectGroup[] = [];

  for (const project of projects) {
    const existing = groups.find((group) =>
      matchesSameProject(group.primaryProject, project),
    );
    if (existing) {
      existing.memberProjects.push(project);
      if (existing.primaryProject.target?.kind === "ssh" && project.target?.kind !== "ssh") {
        existing.primaryProject = project;
        existing.groupId = project.workspaceId;
      }
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
