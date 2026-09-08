import { describe, expect, it } from "vitest";
import {
  getProjectFolderName,
  groupProjects,
  isProjectGroupActive,
  matchesSameProject,
  normalizeGitRemote,
} from "./projectGrouping";
import type { RegisteredProject } from "./types";

describe("projectGrouping", () => {
  describe("normalizeGitRemote", () => {
    it("normalizes various git URLs for the same repository to identical keys", () => {
      const sshUrl = "git@github.com:Indosaram/ferryx.git";
      const httpsUrl = "https://github.com/Indosaram/ferryx";
      const httpsGitUrl = "https://github.com/Indosaram/ferryx.git";
      const sshWithProto = "ssh://git@github.com/Indosaram/ferryx.git";

      expect(normalizeGitRemote(sshUrl)).toBe("github.com/indosaram/ferryx");
      expect(normalizeGitRemote(httpsUrl)).toBe("github.com/indosaram/ferryx");
      expect(normalizeGitRemote(httpsGitUrl)).toBe("github.com/indosaram/ferryx");
      expect(normalizeGitRemote(sshWithProto)).toBe("github.com/indosaram/ferryx");
    });

    it("handles null, undefined, or empty strings gracefully", () => {
      expect(normalizeGitRemote(null)).toBeNull();
      expect(normalizeGitRemote(undefined)).toBeNull();
      expect(normalizeGitRemote("   ")).toBeNull();
    });

    it.each(["/srv/repo", "../repo", "C:\\repos\\app", "file:///srv/repo", "not a remote"])(
      "does not treat filesystem or invalid remote %s as a cross-machine identity",
      (remote) => {
        expect(normalizeGitRemote(remote)).toBeNull();
      },
    );

    it("preserves case-sensitive repository paths outside GitHub", () => {
      expect(normalizeGitRemote("ssh://git@git.example.com/Team/App.git"))
        .toBe("git.example.com/Team/App");
      expect(normalizeGitRemote("https://git.example.com/team/app.git"))
        .not.toBe(normalizeGitRemote("https://git.example.com/Team/App.git"));
    });

    it("normalizes SSH transport ports but preserves distinct HTTP endpoints", () => {
      expect(normalizeGitRemote("ssh://git@github.com:22/Org/App.git"))
        .toBe("github.com/org/app");
      expect(normalizeGitRemote("https://git.example.com:8443/team/app.git"))
        .not.toBe(normalizeGitRemote("https://git.example.com/team/app.git"));
    });
  });

  describe("getProjectFolderName", () => {
    it("extracts the folder name from unix and windows paths", () => {
      const project1: RegisteredProject = {
        workspaceId: "orca-lite",
        repoRoot: "/Users/dev/code/orca-lite",
        gitRoot: null,
      };
      const project2: RegisteredProject = {
        workspaceId: "ssh:abcd",
        repoRoot: "/home/sook/orca-lite",
        gitRoot: null,
        target: { kind: "ssh", hostId: "maho-win" },
      };
      const project3: RegisteredProject = {
        workspaceId: "win-proj",
        repoRoot: "C:\\Users\\dev\\orca-lite\\",
        gitRoot: null,
      };

      expect(getProjectFolderName(project1)).toBe("orca-lite");
      expect(getProjectFolderName(project2)).toBe("orca-lite");
      expect(getProjectFolderName(project3)).toBe("orca-lite");
    });
  });

  describe("matchesSameProject", () => {
    it("matches projects when Git remote origin URLs match even if folder names differ", () => {
      const local: RegisteredProject = {
        workspaceId: "orca-local",
        repoRoot: "/Users/dev/local-repo",
        gitRoot: "/Users/dev/local-repo",
        gitRemote: "git@github.com:Indosaram/ferryx.git",
      };
      const remote: RegisteredProject = {
        workspaceId: "ssh:remote-hash",
        repoRoot: "/srv/different-folder-name",
        gitRoot: "/srv/different-folder-name",
        gitRemote: "https://github.com/Indosaram/ferryx",
        target: { kind: "ssh", hostId: "gpu-box" },
      };

      expect(matchesSameProject(local, remote)).toBe(true);
    });

    it("keeps same-named folders separate without shared Git identity", () => {
      const local: RegisteredProject = {
        workspaceId: "my-app",
        repoRoot: "/Users/dev/my-app",
        gitRoot: null,
      };
      const remote: RegisteredProject = {
        workspaceId: "ssh:app-hash",
        repoRoot: "/home/user/my-app",
        gitRoot: null,
        target: { kind: "ssh", hostId: "dev-server" },
      };

      expect(matchesSameProject(local, remote)).toBe(false);
    });

    it("keeps same-named repositories separate when their remotes differ", () => {
      const local = { workspaceId: "a", repoRoot: "/mac/app", gitRemote: "https://github.com/alice/app.git" };
      const remote = { workspaceId: "b", repoRoot: "/linux/app", gitRemote: "https://github.com/bob/app.git" };
      expect(matchesSameProject(local, remote)).toBe(false);
    });

    it("matches linked worktrees by common Git directory on the same host without a remote", () => {
      const main = { workspaceId: "main", repoRoot: "/repo", gitCommonDir: "/repo/.git" };
      const linked = { workspaceId: "feature", repoRoot: "/worktrees/feature", gitCommonDir: "/repo/.git" };
      expect(matchesSameProject(main, linked)).toBe(true);
    });

    it("does not confuse identical Git directory paths or workspace IDs on different hosts", () => {
      const local = { workspaceId: "same-id", repoRoot: "/repo", gitCommonDir: "/repo/.git" };
      const remote = {
        ...local,
        target: { kind: "ssh", hostId: "other" } as const,
      };
      expect(matchesSameProject(local, remote)).toBe(false);
    });

    it("normalizes Windows common-directory spelling without folding POSIX path case", () => {
      const main = { workspaceId: "a", repoRoot: "C:\\repo", gitCommonDir: "\\\\?\\C:\\Repo\\.git" };
      const linked = { workspaceId: "b", repoRoot: "C:/worktree", gitCommonDir: "c:/repo/.git" };
      expect(matchesSameProject(main, linked)).toBe(true);
      expect(matchesSameProject(
        { ...main, gitCommonDir: "/repo/.git" },
        { ...linked, gitCommonDir: "/Repo/.git" },
      )).toBe(false);
    });

    it("does not match unrelated projects with different remotes and different folder names", () => {
      const projectA: RegisteredProject = {
        workspaceId: "proj-a",
        repoRoot: "/Users/dev/project-alpha",
        gitRoot: null,
        gitRemote: "https://github.com/org/alpha.git",
      };
      const projectB: RegisteredProject = {
        workspaceId: "proj-b",
        repoRoot: "/Users/dev/project-beta",
        gitRoot: null,
        gitRemote: "https://github.com/org/beta.git",
      };

      expect(matchesSameProject(projectA, projectB)).toBe(false);
    });
  });

  describe("groupProjects", () => {
    it("joins common-directory and remote matches regardless of registration order", () => {
      const main = {
        workspaceId: "main", repoRoot: "/repo", gitCommonDir: "/repo/.git",
        gitRemote: "https://github.com/org/app.git",
      };
      const linked = { workspaceId: "linked", repoRoot: "/feature", gitCommonDir: "/repo/.git" };
      const remote = {
        workspaceId: "ssh:remote", repoRoot: "/srv/checkout",
        gitRemote: "git@github.com:org/app.git",
        target: { kind: "ssh", hostId: "linux" } as const,
      };
      for (const projects of [[linked, remote, main], [remote, main, linked], [main, linked, remote]]) {
        const groups = groupProjects(projects);
        expect(groups).toHaveLength(1);
        expect(new Set(groups[0]?.memberProjects.map((project) => project.workspaceId)))
          .toEqual(new Set(["main", "linked", "ssh:remote"]));
        expect(groups[0]?.primaryProject.target?.kind).not.toBe("ssh");
      }
    });

    it("groups matching local and remote projects into one project group with local as primary", () => {
      const local: RegisteredProject = {
        workspaceId: "orca-lite",
        repoRoot: "/Users/indo/code/project/orca-lite",
        gitRoot: "/Users/indo/code/project/orca-lite",
        gitRemote: "git@github.com:Indosaram/ferryx.git",
      };
      const remote: RegisteredProject = {
        workspaceId: "ssh:maho-win-hash",
        repoRoot: "/home/sook/orca-lite",
        gitRoot: "/home/sook/orca-lite",
        gitRemote: "https://github.com/Indosaram/ferryx.git",
        target: { kind: "ssh", hostId: "maho-win" },
      };
      const other: RegisteredProject = {
        workspaceId: "other-tool",
        repoRoot: "/Users/indo/code/other-tool",
        gitRoot: null,
      };

      const groups = groupProjects([local, remote, other]);
      expect(groups).toHaveLength(2);

      const orcaGroup = groups.find((g) => g.groupId === "orca-lite");
      expect(orcaGroup).toBeDefined();
      expect(orcaGroup?.primaryProject).toBe(local);
      expect(orcaGroup?.memberProjects).toEqual([local, remote]);

      expect(isProjectGroupActive(orcaGroup!, "orca-lite")).toBe(true);
      expect(isProjectGroupActive(orcaGroup!, "ssh:maho-win-hash")).toBe(true);
      expect(isProjectGroupActive(orcaGroup!, "other-tool")).toBe(false);
    });
  });
});
