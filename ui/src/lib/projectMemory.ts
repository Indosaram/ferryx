// Per-project "give my project decisions to agents" switches as the hub reports them
// (`{projects: [{projectId, name, repoIdentity, enabled, items}]}`). Pure: no transport imports.
export type ProjectMemorySwitch = {
  projectId: string;
  name: string;
  repoIdentity: string;
  enabled: boolean;
  items: number;
};

export type ProjectMemoryActions = {
  readonly list: () => Promise<ProjectMemorySwitch[]>;
  readonly set: (repoIdentity: string, enabled: boolean) => Promise<ProjectMemorySwitch[]>;
};

/** The `{projects: [...]}` answer, keeping only rows this build understands. */
export function parseProjectMemory(raw: unknown): ProjectMemorySwitch[] {
  const projects = raw && typeof raw === "object" ? (raw as { projects?: unknown }).projects : null;
  if (!Array.isArray(projects)) return [];
  return projects.flatMap((row) => {
    if (!row || typeof row !== "object") return [];
    const value = row as Record<string, unknown>;
    if (typeof value.repoIdentity !== "string" || !value.repoIdentity) return [];
    return [
      {
        projectId: typeof value.projectId === "string" ? value.projectId : value.repoIdentity,
        name: typeof value.name === "string" && value.name ? value.name : value.repoIdentity,
        repoIdentity: value.repoIdentity,
        enabled: value.enabled === true,
        items: typeof value.items === "number" && value.items > 0 ? Math.floor(value.items) : 0,
      },
    ];
  });
}
