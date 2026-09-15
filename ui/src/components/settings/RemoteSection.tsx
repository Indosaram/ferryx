import { useEffect, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { Plus } from "lucide-react";
import type { MachineProjectTarget, RemoteContext } from "../../lib/machineNavigation";
import { DEFAULT_RELAY_ORIGIN, pairedHostInventory } from "../../lib/pairedHostInventory";
import { resetSshHostsCache } from "../../lib/sshHosts";
import { remoteHostStore } from "../../state/remoteHostStore";
import { PairedMachinesSection } from "./PairedMachinesSection";
import { RemoteAccessSection } from "./RemoteAccessSection";
import { SshSection } from "./SshSection";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

export function RemoteSection({ initialContext, legacySsh = false, onOpenProject, onOpenSshProject }: {
  initialContext?: RemoteContext;
  legacySsh?: boolean;
  onOpenProject?: (target: MachineProjectTarget, context: RemoteContext) => void;
  onOpenSshProject?: (hostId: string) => void;
}) {
  const [context, setContext] = useState<RemoteContext>(initialContext ?? { page: "machines", filter: legacySsh ? "ssh" : "all" });
  const [searchQuery, setSearchQuery] = useState("");
  const [showAddChooser, setShowAddChooser] = useState(false);
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);

  useEffect(() => {
    if (!isTauri()) return;
    void pairedHostInventory.refresh();
    const refresh = () => { void pairedHostInventory.refresh(); };
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);

  const open = (target: MachineProjectTarget) => {
    if (onOpenProject) onOpenProject(target, { ...context, machine: target });
    else if (target.kind === "ssh") onOpenSshProject?.(target.hostId);
  };

  const handleRefreshAll = () => {
    if (isTauri()) void pairedHostInventory.refresh();
    resetSshHostsCache();
  };

  return <section aria-label="Remote" className="space-y-5">
    <h1 className="text-xl font-semibold">Remote</h1>
    <nav aria-label="Remote destinations" className="flex flex-wrap gap-2">
      {([['machines', 'Machines'], ['access', 'Access to This Machine'], ['details', 'Connection Details']] as const).map(([page, label]) =>
        <Button key={page} variant="outline" aria-current={context.page === page ? "page" : undefined} onClick={() => setContext(value => ({ ...value, page }))}>{label}</Button>)}
    </nav>
    {context.page === "machines" ? <>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <p className="text-sm text-muted-foreground">Connect to another machine.</p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label="Add Machine"
            onClick={() => setShowAddChooser(v => !v)}
          >
            <Plus className="mr-1 size-3.5" />
            Add Machine
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label="Refresh machines"
            onClick={handleRefreshAll}
          >
            Refresh
          </Button>
        </div>
      </div>

      {showAddChooser ? (
        <div role="group" aria-label="Add Machine options" className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-card/60 p-3 text-xs">
          <span className="text-muted-foreground">Choose connection type:</span>
          <Button
            type="button"
            size="sm"
            onClick={() => {
              setContext(v => ({ ...v, filter: "paired" }));
              setShowAddChooser(false);
            }}
          >
            Pair with PIN
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => {
              setContext(v => ({ ...v, filter: "ssh" }));
              setShowAddChooser(false);
            }}
          >
            Connect with SSH
          </Button>
        </div>
      ) : null}

      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Input
            aria-label="Search machines"
            placeholder="Search machines…"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-8 w-48 text-xs"
          />
          <div role="group" aria-label="Machine connection type" className="flex gap-1">
            {(['all', 'paired', 'ssh'] as const).map(filter => (
              <Button
                key={filter}
                variant="outline"
                size="sm"
                aria-pressed={context.filter === filter}
                onClick={() => setContext(value => ({ ...value, filter }))}
              >
                {filter === 'all' ? 'All' : filter === 'ssh' ? 'SSH' : 'Paired'}
              </Button>
            ))}
          </div>
        </div>
      </div>

      {context.filter === "all" ? (
        <div className="space-y-6">
          {isTauri() ? (
            <PairedMachinesSection
              searchQuery={searchQuery}
              selectedMachine={context.machine}
              onOpenProject={onOpenProject ? open : undefined}
            />
          ) : null}
          <SshSection
            searchQuery={searchQuery}
            onOpenProject={onOpenProject || onOpenSshProject ? hostId => open({ kind: "ssh", hostId }) : undefined}
          />
        </div>
      ) : context.filter === "ssh" ? (
        <SshSection
          searchQuery={searchQuery}
          onOpenProject={onOpenProject || onOpenSshProject ? hostId => open({ kind: "ssh", hostId }) : undefined}
        />
      ) : isTauri() ? (
        <PairedMachinesSection
          searchQuery={searchQuery}
          selectedMachine={context.machine}
          onOpenProject={onOpenProject ? open : undefined}
        />
      ) : (
        <p role="status">Machine pairing requires the desktop app.</p>
      )}
    </> : context.page === "access" ? <RemoteAccessSection /> : <>
      <dl className="text-sm"><dt>Native inventory</dt><dd>{state.nativeStatus}</dd><dt>Credential migration</dt><dd>{state.migrationStatus}</dd></dl>
      <RemoteAccessSection detailsOnly />
      {Object.values(state.hosts).map(host => <dl key={host.hostId} className="break-all rounded border p-3 text-xs">
        <dt>{host.name}</dt><dd>{host.hostId}</dd><dt>Relay</dt><dd>{host.relayOrigin} {host.relayOrigin !== DEFAULT_RELAY_ORIGIN ? '(Legacy)' : ''}</dd>
        <dt>Generation / grant / authorization</dt><dd>{host.generation} / {host.grantScope} / {host.authStatus}</dd>
      </dl>)}
    </>}
  </section>;
}
