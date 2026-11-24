import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";
import { AlertCircle, Plus, Server } from "lucide-react";
import {
  CLOUD_HOST_LIST_COMMAND,
  listCloudMachines,
  type CloudMachineSummary,
} from "../../../lib/cloudHost";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { CloudMachineRow } from "./MachineRow";

export { CLOUD_HOST_LIST_COMMAND, type CloudMachineSummary };

export function useCloudMachines(refreshSignal = 0): CloudMachineSummary[] {
  const [machines, setMachines] = useState<CloudMachineSummary[]>([]);
  const load = useCallback(async () => {
    setMachines(await listCloudMachines());
  }, []);
  useEffect(() => {
    void load();
  }, [load, refreshSignal]);
  return machines;
}

export interface MachineListProps {
  total: number;
  shown: number;
  searchQuery: string;
  actionError: string | null;
  signedIn: boolean;
  onSearchChange: (value: string) => void;
  onAddMachine: () => void;
  onRefresh: () => void;
  cloudMachines?: CloudMachineSummary[];
  cloudRefreshSignal?: number;
  onDeleteCloudMachine?: (name: string) => void | Promise<void>;
  deletingCloudName?: string | null;
  children: ReactNode;
}

export function MachineList({
  total,
  shown,
  searchQuery,
  actionError,
  signedIn,
  onSearchChange,
  onAddMachine,
  onRefresh,
  cloudMachines,
  cloudRefreshSignal = 0,
  onDeleteCloudMachine,
  deletingCloudName = null,
  children,
}: MachineListProps) {
  const read = useCloudMachines(cloudRefreshSignal);
  const rawClouds = cloudMachines ?? read;
  const query = searchQuery.trim().toLowerCase();
  const clouds = query
    ? rawClouds.filter(
        (m) =>
          m.name.toLowerCase().includes(query) ||
          m.provider.toLowerCase().includes(query) ||
          m.sandboxId.toLowerCase().includes(query),
      )
    : rawClouds;
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
              onDelete={onDeleteCloudMachine ? () => void onDeleteCloudMachine(machine.name) : undefined}
              deleting={deletingCloudName === machine.name}
            />
          ))}
          {children}
        </div>
      )}
    </section>
  );
}
