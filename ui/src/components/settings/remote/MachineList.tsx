import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { AlertCircle, Plus, Server } from "lucide-react";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { CloudMachineRow } from "./MachineRow";

/** One disposable cloud machine as the catalog records it (`ferryx-cli cloud list --json`). */
export interface CloudMachineSummary {
  name: string;
  provider: string;
  state: string;
  sandboxId: string;
  transport?: "ssh" | "relay";
}

/** Mirrors `cloud_host::cli::cmd_cloud_host_list` in src-tauri. */
export const CLOUD_HOST_LIST_COMMAND = "cmd_cloud_host_list";

/**
 * The cloud rows, read once per mount.
 *
 * Read-only by construction: the catalog is written by `ferryx-cli cloud` and the reaper, so this
 * only ever reads it. A missing command (an older desktop binary) or an unreadable catalog yields
 * no rows rather than an error banner: the machine list is useful without them, and a broken
 * cloud catalog must not make the pane look broken. Outside Tauri it stays empty.
 */
export function useCloudMachines(): CloudMachineSummary[] {
  const [machines, setMachines] = useState<CloudMachineSummary[]>([]);
  const load = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const rows = await invoke<CloudMachineSummary[]>(CLOUD_HOST_LIST_COMMAND);
      setMachines(Array.isArray(rows) ? rows.filter(isCloudMachine) : []);
    } catch {
      setMachines([]);
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);
  return machines;
}

/** Boundary validation: a row missing its provider or name cannot be labelled, so it is dropped. */
function isCloudMachine(value: unknown): value is CloudMachineSummary {
  if (!value || typeof value !== "object") return false;
  const row = value as Record<string, unknown>;
  return (
    typeof row.name === "string" &&
    row.name.length > 0 &&
    typeof row.provider === "string" &&
    row.provider.length > 0 &&
    typeof row.state === "string" &&
    typeof row.sandboxId === "string"
  );
}

export interface MachineListProps {
  total: number;
  shown: number;
  searchQuery: string;
  actionError: string | null;
  /** Whether an account session exists; only affects the copy, since pairing is automatic. */
  signedIn: boolean;
  onSearchChange: (value: string) => void;
  onAddMachine: () => void;
  onRefresh: () => void;
  /**
   * Cloud machines from the catalog, rendered read-only above the local ones.
   *
   * They are counted separately from `total`/`shown`: those two numbers describe the machines this
   * Mac can connect to, and a cloud box is created, stopped and deleted by `ferryx-cli cloud` and
   * the reaper rather than from this list. Omitted, the list reads them itself
   * ([`useCloudMachines`]); pass an explicit array to render a fixed set.
   */
  cloudMachines?: CloudMachineSummary[];
  children: ReactNode;
}

export function MachineList({ total, shown, searchQuery, actionError, signedIn, onSearchChange, onAddMachine, onRefresh, cloudMachines, children }: MachineListProps) {
  const read = useCloudMachines();
  const clouds = cloudMachines ?? read;
  return (
    <section aria-labelledby="remote-my-machines" className="space-y-3">
      <header className="flex flex-col gap-1 border-b border-border pb-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 id="remote-my-machines" className="text-[13px] font-semibold text-foreground">My machines</h2>
          <p className="text-[12px] text-muted-foreground">Connect to another machine.</p>
        </div>
        <p className="text-[11px] text-muted-foreground">{total} {total === 1 ? "machine" : "machines"}</p>
      </header>
      <div className="flex flex-wrap items-center gap-2">
        <Input
          aria-label="Search machines"
          placeholder="Search machines…"
          value={searchQuery}
          onChange={e => onSearchChange(e.target.value)}
          className="h-8 min-w-0 flex-1 text-[12px] sm:max-w-64"
        />
        <Button type="button" size="sm" aria-label="Add Machine" onClick={onAddMachine}>
          <Plus className="mr-1 size-3.5" />
          Add Machine
        </Button>
        <Button type="button" variant="ghost" size="sm" aria-label="Refresh machines" onClick={onRefresh}>
          Refresh
        </Button>
      </div>
      {actionError ? (
        <div role="alert" className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2.5 text-[12px] text-destructive">
          <AlertCircle className="size-4 shrink-0" />
          <span>{actionError}</span>
        </div>
      ) : null}
      {shown === 0 && clouds.length === 0 ? (
        <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border p-8 text-center">
          <Server className="mb-2 size-8 text-muted-foreground/50" />
          <p className="text-[13px] font-medium text-foreground">{total === 0 ? "No remote machines configured" : "No matching machines found"}</p>
          <p className="mt-1 text-[12px] text-muted-foreground">
            {total === 0
              ? signedIn
                ? "The other machine pairs itself as soon as it signs in to your account. You can also use Add Machine to connect one over SSH."
                : "Sign in above to pair the machines on your account automatically, or use Add Machine to connect one over SSH."
              : "Try clearing your search query to see all machines."}
          </p>
        </div>
      ) : (
        <div role="list" aria-label="Remote machines" className="space-y-2">
          {clouds.map(machine => (
            <CloudMachineRow
              key={`cloud:${machine.name}`}
              name={machine.name}
              provider={machine.provider}
              state={machine.state}
              sandboxId={machine.sandboxId}
              transport={machine.transport}
            />
          ))}
          {children}
        </div>
      )}
    </section>
  );
}
