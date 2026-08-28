import { FolderGit2, Plus } from "lucide-react";

import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { SettingRow, SettingsHeading } from "./primitives";
import type { WorkspaceSectionProps } from "./types";

export function WorkspaceSection({
  projects = [],
  activeProjectId,
  activeWorktree,
  onSelectProject,
  onAddProject,
  onAddWorktree,
}: WorkspaceSectionProps) {
  return (
    <section aria-labelledby="settings-workspace-heading">
      <SettingsHeading
        icon={<FolderGit2 />}
        title="Workspace"
        description="Projects are registered with the native repository registry; worktrees keep the existing safety checks before deletion."
      />
      <h2 id="settings-workspace-heading" className="sr-only">
        Workspace
      </h2>

      <div className="mb-6 flex items-center justify-between">
        <div>
          <h3 className="text-[12px] font-semibold text-foreground">Registered Projects</h3>
          <p className="text-[11px] text-muted-foreground">Manage active projects and their associated worktrees.</p>
        </div>
        <div className="flex items-center gap-2">
          {onAddProject ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onAddProject}
              className="no-drag h-7 gap-1.5 px-2.5 text-[11px] font-medium text-muted-foreground hover:text-foreground"
            >
              <Plus className="size-3" />
              Add Project
            </Button>
          ) : null}
          {onAddWorktree ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onAddWorktree}
              className="no-drag h-7 gap-1.5 px-2.5 text-[11px] font-medium text-muted-foreground hover:text-foreground"
            >
              <Plus className="size-3" />
              Add Worktree
            </Button>
          ) : null}
        </div>
      </div>

      <Card className="divide-y divide-border rounded-lg bg-card shadow-none">
        {projects.length === 0 ? (
          <div className="p-4 text-center text-xs text-muted-foreground">No projects registered.</div>
        ) : (
          projects.map((project) => {
            const isActive = project.workspaceId === activeProjectId;
            return (
              <div key={project.workspaceId} className="flex items-center justify-between gap-4 p-3">
                <div className="min-w-0 space-y-0.5">
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-medium text-foreground">{project.workspaceId}</span>
                    {isActive ? (
                      <Badge
                        variant="secondary"
                        className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary hover:bg-primary/10"
                      >
                        Active
                      </Badge>
                    ) : null}
                  </div>
                  <div className="truncate font-mono text-[10px] text-muted-foreground">{project.repoRoot}</div>
                </div>
                <div className="flex items-center gap-2">
                  {isActive ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled
                      className="h-7 px-2.5 text-[11px] font-medium text-muted-foreground opacity-50"
                    >
                      Active
                    </Button>
                  ) : null}
                  {!isActive && onSelectProject ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => onSelectProject(project)}
                      className="h-7 px-2.5 text-[11px] text-muted-foreground hover:text-foreground"
                    >
                      Select
                    </Button>
                  ) : null}
                </div>
              </div>
            );
          })
        )}
      </Card>

      <div className="mt-8 border-y border-border">
        <SettingRow label="Active worktree" description="Currently focused git worktree directory.">
          <span className="font-mono text-[11px] text-muted-foreground">
            {activeWorktree?.path ?? "None"}
          </span>
        </SettingRow>
        <SettingRow
          label="Worktree deletion"
          description="Dirty-state and branch deletion previews remain enforced by the native safety contract."
        >
          <span className="text-[11px] text-muted-foreground">Protected</span>
        </SettingRow>
      </div>
    </section>
  );
}

export { WorkspaceSection as WorkspaceSettings };
