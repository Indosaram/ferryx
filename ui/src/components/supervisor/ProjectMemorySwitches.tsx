// The supervisor setup control for project memory: per supervised project, whether new agent
// sessions in it are given the decisions the owner taught for it. Off until the owner turns it
// on. Transport-free: the desktop passes IPC actions; anything without them shows nothing.
import { useEffect, useState } from "react";

import { cn } from "../../lib/cn";
import type { ProjectMemoryActions, ProjectMemorySwitch } from "../../lib/projectMemory";
import { Switch } from "../ui/switch";

function errorText(error: unknown): string {
  const raw = (error && typeof error === "object" ? error : {}) as { code?: unknown; message?: unknown };
  const code = typeof raw.code === "string" && raw.code ? raw.code : "INTERNAL_ERROR";
  return `${code}: ${typeof raw.message === "string" ? raw.message : String(error)}`;
}

export function ProjectMemorySwitches({ actions, className }: { actions: ProjectMemoryActions; className?: string }) {
  const [rows, setRows] = useState<ProjectMemorySwitch[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    actions.list().then(
      (next) => live && setRows(next),
      () => live && setRows([]),
    );
    return () => {
      live = false;
    };
  }, [actions]);

  if (!rows || rows.length === 0) return null;

  const toggle = async (row: ProjectMemorySwitch, enabled: boolean) => {
    if (busy) return;
    setBusy(row.repoIdentity);
    setError(null);
    try {
      setRows(await actions.set(row.repoIdentity, enabled));
    } catch (failure) {
      setError(errorText(failure));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section data-testid="project-memory-switches" aria-label="Project decisions for agents" className={cn("flex min-w-0 flex-col gap-1", className)}>
      <p className="font-medium text-foreground">Give my project decisions to new agent sessions</p>
      <p className="text-muted-foreground">Only what you taught for that project. Off until you turn it on.</p>
      <ul role="list" className="flex min-w-0 flex-col">
        {rows.map((row) => (
          <li key={row.repoIdentity} data-testid={`project-memory-${row.projectId}`} className="flex min-w-0 items-center gap-2 py-1">
            <span className="min-w-0 flex-1 truncate" title={row.repoIdentity}>
              {row.name}
              <span className="text-muted-foreground"> · {row.items === 1 ? "1 decision" : `${row.items} decisions`}</span>
            </span>
            <Switch
              aria-label={`Give ${row.name} decisions to agents`}
              checked={row.enabled}
              disabled={busy !== null}
              onCheckedChange={(checked) => void toggle(row, checked)}
            />
          </li>
        ))}
      </ul>
      {error ? (
        <p role="alert" data-testid="project-memory-error" className="break-words text-destructive">
          {error}
        </p>
      ) : null}
    </section>
  );
}
