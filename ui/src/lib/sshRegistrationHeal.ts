import type { RegisterRemoteProjectRequest } from "./remoteProject";
import type { RegisteredProject } from "./tauri";

export interface HealMissingSshRegistrationsApi {
  hasRegistered: (workspaceId: string) => Promise<boolean> | boolean;
  register: (r: RegisterRemoteProjectRequest) => Promise<unknown>;
}

export interface HealMissingSshRegistrationsResult {
  healed: string[];
  skipped: number;
  failures: Array<{ workspaceId: string; error: unknown }>;
}

export async function healMissingSshRegistrations(
  projects: RegisteredProject[],
  api: HealMissingSshRegistrationsApi,
): Promise<HealMissingSshRegistrationsResult> {
  const healed: string[] = [];
  let skipped = 0;
  const failures: Array<{ workspaceId: string; error: unknown }> = [];
  const seenWorkspaceIds = new Set<string>();

  for (const project of projects) {
    if (project.target?.kind !== "ssh") {
      skipped += 1;
      continue;
    }

    const { workspaceId, repoRoot } = project;
    const { hostId } = project.target;

    if (seenWorkspaceIds.has(workspaceId)) {
      skipped += 1;
      continue;
    }
    seenWorkspaceIds.add(workspaceId);

    try {
      const alreadyRegistered = await api.hasRegistered(workspaceId);
      if (alreadyRegistered) {
        skipped += 1;
        continue;
      }

      await api.register({
        workspaceId,
        hostId,
        repoPath: repoRoot,
      });
      healed.push(workspaceId);
    } catch (error) {
      failures.push({ workspaceId, error });
    }
  }

  return { healed, skipped, failures };
}
